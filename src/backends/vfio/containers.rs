// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use std::alloc::{self, Layout};
use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::mem;
use std::ops::Range;
use std::os::unix::io::AsRawFd;
use std::os::unix::prelude::RawFd;

use crate::backends::vfio::bindings::{
    vfio_group_status, vfio_info_cap_header, vfio_iommu_type1_dma_map, vfio_iommu_type1_dma_unmap,
    vfio_iommu_type1_info, VFIO_TYPE1v2_IOMMU, __IncompleteArrayField,
    vfio_iommu_type1_info_cap_iova_range, VFIO_API_VERSION, VFIO_DMA_MAP_FLAG_READ,
    VFIO_DMA_MAP_FLAG_WRITE, VFIO_GROUP_FLAGS_VIABLE, VFIO_IOMMU_INFO_PGSIZES,
    VFIO_IOMMU_TYPE1_INFO_CAP_IOVA_RANGE,
};
use crate::backends::vfio::ioctl::{
    vfio_check_extension, vfio_get_api_version, vfio_group_get_status, vfio_group_set_container,
    vfio_iommu_get_info, vfio_iommu_map_dma, vfio_iommu_unmap_dma, vfio_set_iommu,
};
use crate::iommu::{PciIommu, PciIommuInternal};
use crate::regions::Permissions;

/* ---------------------------------------------------------------------------------------------- */

fn open_group(group_number: u32) -> io::Result<File> {
    // open group

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(format!("/dev/vfio/{}", group_number))?;

    // check if group is viable

    let mut group_status = vfio_group_status {
        argsz: mem::size_of::<vfio_group_status>() as u32,
        flags: 0,
    };

    unsafe { vfio_group_get_status(file.as_raw_fd(), &mut group_status)? };

    if group_status.flags & VFIO_GROUP_FLAGS_VIABLE == 0 {
        return Err(io::Error::new(
            ErrorKind::Other,
            "Group is not viable; are all devices in the group bound to vfio or unbound?",
        ));
    }

    // success

    Ok(file)
}

fn get_iommu_info(device_fd: RawFd) -> io::Result<(usize, Box<[Range<u64>]>)> {
    let mut iommu_info = vfio_iommu_type1_info {
        argsz: mem::size_of::<vfio_iommu_type1_info>() as u32,
        flags: 0,
        iova_pgsizes: 0,
        cap_offset: 0,
    };

    unsafe { vfio_iommu_get_info(device_fd, &mut iommu_info)? };

    // get page size

    if iommu_info.flags & VFIO_IOMMU_INFO_PGSIZES == 0 {
        return Err(io::Error::new(
            ErrorKind::Other,
            "VFIO didn't report IOMMU mapping alignment requirement",
        ));
    }

    let iova_alignment = 1usize << iommu_info.iova_pgsizes.trailing_zeros();

    // get valid iova ranges

    let valid_iova_ranges = if iommu_info.argsz > mem::size_of::<vfio_iommu_type1_info>() as u32 {
        // Actual vfio_iommu_type1_info struct is bigger, must re-retrieve it with full argsz.

        let layout = Layout::from_size_align(iommu_info.argsz as usize, 8)
            .map_err(|_| io::Error::new(ErrorKind::Other, "TODO"))?;

        let bigger_info = unsafe { alloc::alloc(layout) } as *mut vfio_iommu_type1_info;
        if bigger_info.is_null() {
            alloc::handle_alloc_error(layout);
        }

        unsafe {
            *bigger_info = vfio_iommu_type1_info {
                argsz: iommu_info.argsz,
                flags: 0,
                iova_pgsizes: 0,
                cap_offset: 0,
            };
        }

        unsafe { vfio_iommu_get_info(device_fd, bigger_info)? };

        let mut ranges = get_iommu_cap_iova_ranges(bigger_info)?;

        // validate and adjust ranges

        ranges.sort_by_key(|r| r.start);

        if !ranges.is_empty() && ranges[0].start == 0 {
            // First valid IOVA is 0x0, which can cause problems with some protocols or hypervisors.
            // Make the user's life easier by dropping the first page of IOVA space.
            ranges[0].start = iova_alignment as u64;
            if ranges[0].start >= ranges[0].end {
                ranges.remove(0);
            }
        }

        if !ranges.windows(2).all(|r| r[0].end <= r[1].start) {
            return Err(io::Error::new(
                ErrorKind::Other,
                "VFIO reported overlapping IOVA ranges",
            ));
        }

        ranges.into_boxed_slice()
    } else {
        Box::new([])
    };

    // validate and adjust iova ranges

    Ok((iova_alignment, valid_iova_ranges))
}

fn get_iommu_cap_iova_ranges(info: *const vfio_iommu_type1_info) -> io::Result<Vec<Range<u64>>> {
    let mut offset = unsafe { *info }.cap_offset as usize;

    while offset != 0 {
        let header = unsafe { info.cast::<u8>().add(offset).cast::<vfio_info_cap_header>() };

        if unsafe { *header }.id as u32 == VFIO_IOMMU_TYPE1_INFO_CAP_IOVA_RANGE {
            let cap = header.cast::<vfio_iommu_type1_info_cap_iova_range>();
            let ranges = unsafe { (*cap).iova_ranges.as_slice((*cap).nr_iovas as usize) };
            let ranges = ranges.iter().map(|range| range.start..range.end).collect();
            return Ok(ranges);
        }

        offset = unsafe { *header }.next as usize;
    }

    Err(io::Error::new(
        ErrorKind::Other,
        "VFIO did not provide information about valid IOVA ranges",
    ))
}

/* ---------------------------------------------------------------------------------------------- */

/// A VFIO container representing an IOMMU context that may contain zero or more VFIO groups.
#[derive(Debug)]
pub struct VfioContainer {
    file: File,
    group_numbers: Box<[u32]>,
    pub(crate) groups: HashMap<u32, File>,
    iommu_iova_alignment: usize,
    iommu_valid_iova_ranges: Box<[Range<u64>]>,
}

impl VfioContainer {
    /// Creates a new, empty [`VfioContainer`].
    ///
    /// This fails if not all devices in all given groups have been bound to vfio-pci (the VFIO docs
    /// say "it's also sufficient to only unbind the device from host drivers if a VFIO driver is
    /// unavailable").
    ///
    /// This fails if any of the groups is already open elsewhere, for instance if another
    /// [`VfioContainer`] containing one of the groups already currently exists.
    pub fn new(groups: &[u32]) -> io::Result<VfioContainer> {
        // open groups

        let group_numbers = Vec::from(groups)
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Box<[_]>>();

        let groups: HashMap<_, _> = group_numbers
            .iter()
            .map(|&n| Ok((n, open_group(n)?)))
            .collect::<io::Result<_>>()?;

        // create container

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/vfio/vfio")?;

        let fd = file.as_raw_fd();

        // check API version

        if unsafe { vfio_get_api_version(fd)? } != VFIO_API_VERSION as i32 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "Wrong VFIO_API_VERSION",
            ));
        }

        // check extension

        if unsafe { vfio_check_extension(fd, VFIO_TYPE1v2_IOMMU as i32)? } != 1 {
            return Err(io::Error::new(ErrorKind::InvalidInput, "TODO"));
        }

        // add groups to container

        for group_file in groups.values() {
            unsafe { vfio_group_set_container(group_file.as_raw_fd(), &fd)? };
        }

        // enable IOMMU

        unsafe { vfio_set_iommu(fd, VFIO_TYPE1v2_IOMMU as i32)? };

        // get IOMMU info

        let (iommu_iova_alignment, iommu_valid_iova_ranges) = get_iommu_info(fd)?;

        // success

        Ok(VfioContainer {
            file,
            group_numbers,
            groups,
            iommu_iova_alignment,
            iommu_valid_iova_ranges,
        })
    }

    /// The group numbers of the groups this container contains.
    ///
    /// In ascending order, without duplicates.
    pub fn groups(&self) -> &[u32] {
        &self.group_numbers
    }

    /// Returns a thing that lets you manage IOMMU mappings for DMA for all devices in all groups
    /// that belong to this container.
    pub fn iommu(&self) -> PciIommu {
        PciIommu { internal: self }
    }

    /// Tries to reset all the PCI functions in all the VFIO groups that `self` refers to.
    ///
    /// This requires that the user has "ownership" over all the affected functions / permissions to
    /// do it.
    ///
    /// TODO: Reset granularity might not match container granularity. Will probably need to expose
    /// reset topology properly eventually.
    ///
    /// TODO: Should probably advertise whether this granularity of reset is supported, so the user
    /// doesn't have to try resetting to find out.
    pub fn reset(&self) -> io::Result<()> {
        // TODO: Implement.
        Err(io::Error::new(ErrorKind::Other, "not yet implemented"))
    }
}

impl PciIommuInternal for VfioContainer {
    fn alignment(&self) -> usize {
        self.iommu_iova_alignment
    }

    fn valid_iova_ranges(&self) -> &[Range<u64>] {
        &self.iommu_valid_iova_ranges
    }

    unsafe fn map(
        &self,
        iova: u64,
        size: usize,
        address: *const u8,
        device_permissions: Permissions,
    ) -> io::Result<()> {
        // map region

        let flags = match device_permissions {
            Permissions::Read => VFIO_DMA_MAP_FLAG_READ,
            Permissions::Write => VFIO_DMA_MAP_FLAG_WRITE,
            Permissions::ReadWrite => VFIO_DMA_MAP_FLAG_READ | VFIO_DMA_MAP_FLAG_WRITE,
        };

        let dma_map = vfio_iommu_type1_dma_map {
            argsz: mem::size_of::<vfio_iommu_type1_dma_map>() as u32,
            flags,
            vaddr: address as u64,
            iova,
            size: size as u64,
        };

        unsafe { vfio_iommu_map_dma(self.file.as_raw_fd(), &dma_map) }.map_err(|e| {
            io::Error::new(
                ErrorKind::Other,
                format!(
                    "Failed to set up IOMMU mapping process memory [{:#x}, {:#x}) to device \
                    memory [{:#x}, {:#x}): {}",
                    address as usize,
                    address as usize + size,
                    iova,
                    iova + size as u64,
                    e
                ),
            )
        })?;

        // success

        Ok(())
    }

    fn unmap(&self, iova: u64, size: usize) -> io::Result<()> {
        let mut dma_unmap = vfio_iommu_type1_dma_unmap {
            argsz: mem::size_of::<vfio_iommu_type1_dma_unmap>() as u32,
            flags: 0,
            iova,
            size: size as u64,
            data: __IncompleteArrayField::new(),
        };

        unsafe { vfio_iommu_unmap_dma(self.file.as_raw_fd(), &mut dma_unmap)? };

        Ok(())
    }
}

/* ---------------------------------------------------------------------------------------------- */
