// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

// override the crate-level `deny(unsafe_op_in_unsafe_fn)`
#[cfg_attr(feature = "_unsafe-op-in-unsafe-fn", allow(unsafe_op_in_unsafe_fn))]
#[allow(
    dead_code,
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals
)]
mod bindings;

mod containers;
mod ioctl;
mod regions;

use libc::{mmap64, munmap, MAP_FAILED, MAP_SHARED, PROT_READ, PROT_WRITE};
use std::alloc::{self, Layout};
use std::ffi::CString;
use std::fmt::Debug;
use std::fs::File;
use std::io::{self, ErrorKind};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::prelude::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use std::{mem, ptr};

use crate::backends::vfio::bindings::{
    __IncompleteArrayField, vfio_device_info, vfio_irq_info, vfio_irq_set, VFIO_DEVICE_FLAGS_PCI,
    VFIO_IRQ_INFO_EVENTFD, VFIO_IRQ_SET_ACTION_TRIGGER, VFIO_IRQ_SET_DATA_EVENTFD,
    VFIO_IRQ_SET_DATA_NONE, VFIO_PCI_BAR0_REGION_INDEX, VFIO_PCI_BAR5_REGION_INDEX,
    VFIO_PCI_CONFIG_REGION_INDEX, VFIO_PCI_INTX_IRQ_INDEX, VFIO_PCI_MSIX_IRQ_INDEX,
    VFIO_PCI_MSI_IRQ_INDEX, VFIO_PCI_ROM_REGION_INDEX,
};
use crate::backends::vfio::ioctl::{
    vfio_device_get_info, vfio_device_get_irq_info, vfio_device_reset, vfio_device_set_irqs,
    vfio_group_get_device_fd,
};
use crate::backends::vfio::regions::{
    set_up_bar_or_rom, set_up_config_space, VfioUnmappedPciRegion,
};
use crate::config::PciConfig;
use crate::device::{PciDevice, PciDeviceInternal};
use crate::interrupts::{PciInterruptKind, PciInterrupts};
use crate::iommu::PciIommu;
use crate::regions::{BackedByPciSubregion, OwningPciRegion, Permissions, RegionIdentifier};

pub use containers::VfioContainer;

/* ---------------------------------------------------------------------------------------------- */

fn get_device_address<P: AsRef<Path>>(device_sysfs_path: P) -> io::Result<CString> {
    let path = device_sysfs_path.as_ref().canonicalize()?;
    let address = path.file_name().unwrap();

    Ok(CString::new(address.as_bytes()).unwrap())
}

fn get_device_group_number<P: AsRef<Path>>(device_sysfs_path: P) -> io::Result<u32> {
    let group_sysfs_path = device_sysfs_path
        .as_ref()
        .join("iommu_group")
        .canonicalize()?;

    let group_dir_name = group_sysfs_path
        .file_name()
        .unwrap()
        .to_str()
        .ok_or_else(|| io::Error::new(ErrorKind::Other, "TODO"))?;

    group_dir_name
        .parse()
        .map_err(|_| io::Error::new(ErrorKind::Other, "TODO"))
}

/* ---------------------------------------------------------------------------------------------- */

/// Provides control over a PCI device using VFIO.
#[derive(Debug)]
pub struct VfioPciDevice {
    inner: Arc<VfioPciDeviceInner>,
}

impl VfioPciDevice {
    /// Creates a new [`VfioContainer`] containing only the group that contains the given vfio-pci
    /// device, then calls [`VfioPciDevice::open_in_container`] with the same path and the created
    /// container.
    ///
    /// Note that this only works if no other [`VfioContainer`] already contains the device's group,
    /// and so you must use [`VfioPciDevice::open_in_container`] if you want to drive several
    /// devices from the same VFIO group.
    pub fn open<P: AsRef<Path>>(sysfs_path: P) -> io::Result<VfioPciDevice> {
        let group_number = get_device_group_number(&sysfs_path)?;
        let container = Arc::new(VfioContainer::new(&[group_number])?);

        Self::open_in_container(sysfs_path, container)
    }

    /// Opens a vfio-pci device and adds it to the given container.
    ///
    /// `sysfs_path` must correspond to the device's sysfs directory, *e.g.*,
    /// `/sys/bus/pci/devices/0000:00:01.0`. `container` must contain the group to which the device
    /// belongs.
    ///
    /// Returns a `VfioPciDevice` corresponding to the opened device.
    pub fn open_in_container<P: AsRef<Path>>(
        sysfs_path: P,
        container: Arc<VfioContainer>,
    ) -> io::Result<VfioPciDevice> {
        let device_address = get_device_address(&sysfs_path)?;
        let group_number = get_device_group_number(&sysfs_path)?;

        // get group file

        let group_file = container
            .groups
            .get(&group_number)
            .ok_or_else(|| io::Error::new(ErrorKind::Other, "TODO"))?;

        // get device file

        let device_file = unsafe {
            let fd = vfio_group_get_device_fd(group_file.as_raw_fd(), device_address.as_ptr())?;
            Arc::new(File::from_raw_fd(fd))
        };

        // validate device info

        let mut device_info = vfio_device_info {
            argsz: mem::size_of::<vfio_device_info>() as u32,
            flags: 0,
            num_regions: 0,
            num_irqs: 0,
            cap_offset: 0,
        };

        unsafe { vfio_device_get_info(device_file.as_raw_fd(), &mut device_info)? };

        if device_info.flags & VFIO_DEVICE_FLAGS_PCI == 0
            || device_info.num_regions < VFIO_PCI_CONFIG_REGION_INDEX + 1
            || device_info.num_irqs < VFIO_PCI_MSIX_IRQ_INDEX + 1
        {
            return Err(io::Error::new(ErrorKind::Other, "TODO"));
        }

        // get interrupt info

        let get_max_interrupts = |index| {
            let mut irq_info = vfio_irq_info {
                argsz: mem::size_of::<vfio_irq_info>() as u32,
                flags: 0,
                index,
                count: 0,
            };

            unsafe { vfio_device_get_irq_info(device_file.as_raw_fd(), &mut irq_info)? };

            if irq_info.flags & VFIO_IRQ_INFO_EVENTFD == 0 {
                return Err(io::Error::new(ErrorKind::Other, "TODO"));
            }

            Ok(irq_info.count as usize)
        };

        let max_interrupts = [
            get_max_interrupts(VFIO_PCI_INTX_IRQ_INDEX)?,
            get_max_interrupts(VFIO_PCI_MSI_IRQ_INDEX)?,
            get_max_interrupts(VFIO_PCI_MSIX_IRQ_INDEX)?,
        ];

        // set up config space

        let config_region = set_up_config_space(&device_file)?;

        // set up BARs and ROM

        let bars = (VFIO_PCI_BAR0_REGION_INDEX..=VFIO_PCI_BAR5_REGION_INDEX)
            .map(|index| set_up_bar_or_rom(&device_file, index))
            .collect::<io::Result<_>>()?;

        let rom = set_up_bar_or_rom(&device_file, VFIO_PCI_ROM_REGION_INDEX)?;

        // success

        Ok(VfioPciDevice {
            inner: Arc::new(VfioPciDeviceInner {
                container,
                file: device_file,
                config_region,
                bars,
                rom,
                max_interrupts,
            }),
        })
    }

    /// Returns a reference to the container to which the device's group belongs.
    pub fn container(&self) -> &Arc<VfioContainer> {
        &self.inner.container
    }
}

impl crate::device::Sealed for VfioPciDevice {}
impl PciDevice for VfioPciDevice {
    fn config(&self) -> PciConfig {
        PciConfig::backed_by(&self.inner.config_region)
    }

    fn bar(&self, index: usize) -> Option<OwningPciRegion> {
        let bar = self.inner.bars.get(index)?.as_ref()?;

        Some(OwningPciRegion::new(
            Arc::<VfioPciDeviceInner>::clone(&self.inner),
            Arc::<VfioUnmappedPciRegion>::clone(bar),
            RegionIdentifier::Bar(index),
            bar.is_mappable(),
        ))
    }

    fn rom(&self) -> Option<OwningPciRegion> {
        let rom = self.inner.rom.as_ref()?;

        Some(OwningPciRegion::new(
            Arc::<VfioPciDeviceInner>::clone(&self.inner),
            Arc::<VfioUnmappedPciRegion>::clone(rom),
            RegionIdentifier::Rom,
            rom.is_mappable(),
        ))
    }

    fn iommu(&self) -> PciIommu {
        self.inner.container.iommu()
    }

    fn interrupts(&self) -> PciInterrupts {
        PciInterrupts {
            device: &*self.inner,
        }
    }

    fn reset(&self) -> io::Result<()> {
        unsafe { vfio_device_reset(self.inner.file.as_raw_fd())? };
        Ok(())
    }
}

/* ---------------------------------------------------------------------------------------------- */

#[derive(Debug)]
struct VfioPciDeviceInner {
    container: Arc<VfioContainer>,

    file: Arc<File>,

    config_region: VfioUnmappedPciRegion,
    bars: Box<[Option<Arc<VfioUnmappedPciRegion>>]>,
    rom: Option<Arc<VfioUnmappedPciRegion>>,

    max_interrupts: [usize; 3],
}

impl PciDeviceInternal for VfioPciDeviceInner {
    // BARs / ROM

    fn region_map(
        &self,
        identifier: RegionIdentifier,
        offset: u64,
        len: usize,
        permissions: Permissions,
    ) -> io::Result<*mut u8> {
        let region = match identifier {
            RegionIdentifier::Bar(index) => &self.bars[index],
            RegionIdentifier::Rom => &self.rom,
        };

        let region = region.as_ref().unwrap();

        let prot_flags = match permissions {
            Permissions::Read => PROT_READ,
            Permissions::Write => PROT_WRITE,
            Permissions::ReadWrite => PROT_READ | PROT_WRITE,
        };

        let address = unsafe {
            mmap64(
                ptr::null_mut(),
                len,
                prot_flags,
                MAP_SHARED,
                self.file.as_raw_fd(),
                region.offset_in_device_file() as i64 + offset as i64,
            )
        };

        if address == MAP_FAILED {
            Err(io::Error::last_os_error())
        } else {
            Ok(address.cast())
        }
    }

    unsafe fn region_unmap(&self, _identifier: RegionIdentifier, address: *mut u8, size: usize) {
        let result = if unsafe { munmap(address.cast(), size) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        };

        // TODO: Do something other than crash on failure?
        result.unwrap();
    }

    // Interrupts

    fn interrupts_max(&self, kind: PciInterruptKind) -> usize {
        self.max_interrupts[kind as usize]
    }

    fn interrupts_enable(&self, kind: PciInterruptKind, eventfds: &[RawFd]) -> io::Result<()> {
        if eventfds.len() > self.max_interrupts[kind as usize] {
            return Err(io::Error::new(ErrorKind::Other, "TODO"));
        }

        // allocate memory for vfio_irq_set

        let eventfds_size = eventfds.len() * mem::size_of::<i32>();
        let total_size = mem::size_of::<vfio_irq_set>() + eventfds_size;

        let layout = Layout::from_size_align(total_size, 4)
            .map_err(|_| io::Error::new(ErrorKind::Other, "TODO"))?;

        let mem = unsafe { alloc::alloc(layout) };

        if mem.is_null() {
            alloc::handle_alloc_error(layout);
        }

        // initialize vfio_irq_set

        let irq_set = mem as *mut vfio_irq_set;

        unsafe {
            (*irq_set).argsz = total_size as u32;
            (*irq_set).flags = VFIO_IRQ_SET_DATA_EVENTFD | VFIO_IRQ_SET_ACTION_TRIGGER;
            (*irq_set).index = interrupt_index_from_kind(kind);
            (*irq_set).start = 0;
            (*irq_set).count = eventfds.len() as u32;
        }

        let eventfd_mem_iter = unsafe {
            (*irq_set)
                .data
                .as_mut_slice(eventfds_size)
                .chunks_exact_mut(4)
        };

        for (mem, eventfd) in eventfd_mem_iter.zip(eventfds) {
            mem.copy_from_slice(&eventfd.to_ne_bytes());
        }

        // enable interrupt vectors

        unsafe { vfio_device_set_irqs(self.file.as_raw_fd(), irq_set)? };

        Ok(())
    }

    fn interrupts_disable(&self, kind: PciInterruptKind) -> io::Result<()> {
        let irq_set = vfio_irq_set {
            argsz: mem::size_of::<vfio_irq_set>() as u32,
            flags: VFIO_IRQ_SET_DATA_NONE | VFIO_IRQ_SET_ACTION_TRIGGER,
            index: interrupt_index_from_kind(kind),
            start: 0,
            count: 0,
            data: __IncompleteArrayField::new(),
        };

        unsafe { vfio_device_set_irqs(self.file.as_raw_fd(), &irq_set)? };

        Ok(())
    }
}

fn interrupt_index_from_kind(kind: PciInterruptKind) -> u32 {
    match kind {
        PciInterruptKind::Intx => VFIO_PCI_INTX_IRQ_INDEX,
        PciInterruptKind::Msi => VFIO_PCI_MSI_IRQ_INDEX,
        PciInterruptKind::MsiX => VFIO_PCI_MSIX_IRQ_INDEX,
    }
}

/* ---------------------------------------------------------------------------------------------- */
