// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use std::fmt::Debug;
use std::fs::File;
use std::io::{self, ErrorKind};
use std::mem;
use std::os::unix::fs::FileExt;
use std::os::unix::io::AsRawFd;
use std::sync::Arc;

use crate::backends::vfio::bindings::{
    vfio_region_info, VFIO_PCI_CONFIG_REGION_INDEX, VFIO_REGION_INFO_FLAG_MMAP,
    VFIO_REGION_INFO_FLAG_READ, VFIO_REGION_INFO_FLAG_WRITE,
};
use crate::backends::vfio::ioctl::vfio_device_get_region_info;
use crate::regions::{AsPciSubregion, PciRegion, PciSubregion, Permissions};

/* ---------------------------------------------------------------------------------------------- */

#[derive(Debug)]
pub struct VfioUnmappedPciRegion {
    device_file: Arc<File>,
    offset_in_device_file: u64,
    length: u64,
    permissions: Permissions,
    is_mappable: bool,
}

impl VfioUnmappedPciRegion {
    pub(crate) fn offset_in_device_file(&self) -> u64 {
        self.offset_in_device_file
    }

    pub(crate) fn is_mappable(&self) -> bool {
        self.is_mappable
    }

    fn validate_access(
        &self,
        required_alignment: u64,
        offset: u64,
        length: usize,
    ) -> io::Result<()> {
        let end = offset + length as u64;

        if end > self.length {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "Tried to read region range [{:#x}, {:#x}), must be in [0x0, {:#x})",
                    offset, end, self.length
                ),
            ));
        }

        if offset % required_alignment != 0 || length as u64 % required_alignment != 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!("Access must be {}-byte aligned", required_alignment),
            ));
        }

        Ok(())
    }

    fn read(&self, required_alignment: u64, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        self.validate_access(required_alignment, offset, buffer.len())?;
        self.device_file
            .read_exact_at(buffer, self.offset_in_device_file + offset)
    }

    fn write(&self, required_alignment: u64, offset: u64, buffer: &[u8]) -> io::Result<()> {
        self.validate_access(required_alignment, offset, buffer.len())?;
        self.device_file
            .write_all_at(buffer, self.offset_in_device_file + offset)
    }
}

impl crate::regions::Sealed for VfioUnmappedPciRegion {}
impl PciRegion for VfioUnmappedPciRegion {
    fn len(&self) -> u64 {
        self.length
    }

    fn permissions(&self) -> Permissions {
        self.permissions
    }

    fn as_ptr(&self) -> Option<*const u8> {
        None
    }

    fn as_mut_ptr(&self) -> Option<*mut u8> {
        None
    }

    fn read_bytes(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        self.read(1, offset, buffer)
    }

    fn read_u8(&self, offset: u64) -> io::Result<u8> {
        let mut buffer = [0; 1];
        self.read(1, offset, &mut buffer)?;
        Ok(buffer[0])
    }

    fn write_u8(&self, offset: u64, value: u8) -> io::Result<()> {
        self.write(1, offset, &[value])
    }

    fn read_le_u16(&self, offset: u64) -> io::Result<u16> {
        let mut buffer = [0; 2];
        self.read(2, offset, &mut buffer)?;
        Ok(u16::from_le_bytes(buffer))
    }

    fn write_le_u16(&self, offset: u64, value: u16) -> io::Result<()> {
        self.write(2, offset, &value.to_le_bytes())
    }

    fn read_le_u32(&self, offset: u64) -> io::Result<u32> {
        let mut buffer = [0; 4];
        self.read(4, offset, &mut buffer)?;
        Ok(u32::from_le_bytes(buffer))
    }

    fn write_le_u32(&self, offset: u64, value: u32) -> io::Result<()> {
        self.write(4, offset, &value.to_le_bytes())
    }
}

impl<'a> AsPciSubregion<'a> for &'a VfioUnmappedPciRegion {
    fn as_subregion(&self) -> PciSubregion<'a> {
        let region: &'a dyn PciRegion = *self;
        <&dyn PciRegion>::as_subregion(&region)
    }
}

/* ---------------------------------------------------------------------------------------------- */

pub(crate) fn set_up_config_space(device_file: &Arc<File>) -> io::Result<VfioUnmappedPciRegion> {
    let mut region_info = vfio_region_info {
        argsz: mem::size_of::<vfio_region_info>() as u32,
        flags: 0,
        index: VFIO_PCI_CONFIG_REGION_INDEX,
        cap_offset: 0,
        size: 0,
        offset: 0,
    };

    unsafe { vfio_device_get_region_info(device_file.as_raw_fd(), &mut region_info)? };

    if region_info.size == 0 {
        return Err(io::Error::new(ErrorKind::InvalidData, "TODO"));
    }

    if region_info.flags & VFIO_REGION_INFO_FLAG_READ == 0
        || region_info.flags & VFIO_REGION_INFO_FLAG_WRITE == 0
    {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Expected config space to be both readable and writable",
        ));
    }

    let region = VfioUnmappedPciRegion {
        device_file: Arc::clone(device_file),
        offset_in_device_file: region_info.offset,
        length: region_info.size,
        permissions: Permissions::ReadWrite,
        is_mappable: false,
    };

    Ok(region)
}

pub(crate) fn set_up_bar_or_rom(
    device_file: &Arc<File>,
    vfio_region_index: u32,
) -> io::Result<Option<Arc<VfioUnmappedPciRegion>>> {
    let mut region_info = vfio_region_info {
        argsz: mem::size_of::<vfio_region_info>() as u32,
        flags: 0,
        index: vfio_region_index,
        cap_offset: 0,
        size: 0,
        offset: 0,
    };

    unsafe { vfio_device_get_region_info(device_file.as_raw_fd(), &mut region_info)? };

    if region_info.size == 0 {
        return Ok(None); // no such region
    }

    let readable = region_info.flags & VFIO_REGION_INFO_FLAG_READ != 0;
    let writable = region_info.flags & VFIO_REGION_INFO_FLAG_WRITE != 0;

    let permissions = Permissions::new(readable, writable).ok_or_else(|| {
        io::Error::new(
            ErrorKind::Other,
            "Found a region that is neither readable nor writeable",
        )
    })?;

    let region = VfioUnmappedPciRegion {
        device_file: Arc::clone(device_file),
        offset_in_device_file: region_info.offset,
        length: region_info.size,
        permissions,
        is_mappable: region_is_mappable(&region_info),
    };

    Ok(Some(Arc::new(region)))
}

fn region_is_mappable(region_info: &vfio_region_info) -> bool {
    // TODO: Probably not necessary to check if length fits in address space?
    region_info.flags & VFIO_REGION_INFO_FLAG_MMAP != 0 && region_info.size <= usize::MAX as u64
}

/* ---------------------------------------------------------------------------------------------- */
