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

use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};
use std::ffi::CString;
use std::fmt::Debug;
use std::fs::File;
use std::io::{self, ErrorKind};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::prelude::OsStrExt;
use std::path::Path;
use std::sync::Arc;
use std::{mem, ptr};

use crate::backends::vfio::bindings::{
    vfio_device_info, VFIO_DEVICE_FLAGS_PCI, VFIO_PCI_BAR0_REGION_INDEX,
    VFIO_PCI_BAR5_REGION_INDEX, VFIO_PCI_CONFIG_REGION_INDEX, VFIO_PCI_ROM_REGION_INDEX,
};
use crate::backends::vfio::ioctl::{vfio_device_get_info, vfio_group_get_device_fd};
use crate::backends::vfio::regions::{
    set_up_bar_or_rom, set_up_config_space, VfioUnmappedPciRegion,
};
use crate::device::{PciDevice, PciDeviceInternal};
use crate::iommu::PciIommu;
use crate::regions::{OwningPciRegion, PciRegion, Permissions, RegionIdentifier};

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
        {
            return Err(io::Error::new(ErrorKind::Other, "TODO"));
        }

        // set up config space

        let config_region = set_up_config_space(&device_file)?;

        // set up BARs and ROM

        let bars = (VFIO_PCI_BAR0_REGION_INDEX..=VFIO_PCI_BAR5_REGION_INDEX)
            .into_iter()
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
    fn config(&self) -> &dyn PciRegion {
        &self.inner.config_region
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
}

/* ---------------------------------------------------------------------------------------------- */

#[derive(Debug)]
struct VfioPciDeviceInner {
    container: Arc<VfioContainer>,

    file: Arc<File>,

    config_region: VfioUnmappedPciRegion,
    bars: Box<[Option<Arc<VfioUnmappedPciRegion>>]>,
    rom: Option<Arc<VfioUnmappedPciRegion>>,
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
            Permissions::Read => ProtFlags::PROT_READ,
            Permissions::Write => ProtFlags::PROT_WRITE,
            Permissions::ReadWrite => ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
        };

        let address = unsafe {
            mmap(
                ptr::null_mut(),
                len,
                prot_flags,
                MapFlags::MAP_SHARED,
                self.file.as_raw_fd(),
                region.offset_in_device_file() as i64 + offset as i64,
            )
        }?;

        Ok(address.cast())
    }

    unsafe fn region_unmap(&self, _identifier: RegionIdentifier, address: *mut u8, size: usize) {
        // TODO: Do something other than crash on failure?
        unsafe { munmap(address.cast(), size) }.unwrap();
    }
}

/* ---------------------------------------------------------------------------------------------- */
