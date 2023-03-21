// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use std::io;
use std::os::unix::io::RawFd;

use libc::{c_char, c_ulong, ioctl};

use crate::backends::vfio::bindings::{
    vfio_device_info, vfio_group_status, vfio_iommu_type1_dma_map, vfio_iommu_type1_dma_unmap,
    vfio_iommu_type1_info, vfio_irq_info, vfio_irq_set, vfio_region_info, VFIO_BASE, VFIO_TYPE,
};

/* ---------------------------------------------------------------------------------------------- */

macro_rules! define_ioctl {
    ($name:ident, $index:literal) => {
        pub unsafe fn $name(fd: RawFd) -> io::Result<i32> {
            const CMD: c_ulong = ioctl_cmd($index);
            let ret = unsafe { ioctl(fd, CMD) };
            ioctl_return_to_result(ret)
        }
    };
    ($name:ident, $index:literal, $arg_name:ident: usize) => {
        pub unsafe fn $name(fd: RawFd, $arg_name: usize) -> io::Result<i32> {
            const CMD: c_ulong = ioctl_cmd($index);
            let ret = unsafe { ioctl(fd, CMD, $arg_name) };
            ioctl_return_to_result(ret)
        }
    };
    ($name:ident, $index:literal, $arg_name:ident: $arg_type:ty) => {
        pub unsafe fn $name(fd: RawFd, $arg_name: $arg_type) -> io::Result<i32> {
            const CMD: c_ulong = ioctl_cmd($index);
            let ret = unsafe { ioctl(fd, CMD, $arg_name as *const _) };
            ioctl_return_to_result(ret)
        }
    };
}

const fn ioctl_cmd(index: c_ulong) -> c_ulong {
    const IOC_NRBITS: c_ulong = 8;
    const IOC_TYPEBITS: c_ulong = 8;
    const IOC_SIZEBITS: c_ulong = 14;

    const IOC_NRSHIFT: c_ulong = 0;
    const IOC_TYPESHIFT: c_ulong = IOC_NRSHIFT + IOC_NRBITS;
    const IOC_SIZESHIFT: c_ulong = IOC_TYPESHIFT + IOC_TYPEBITS;
    const IOC_DIRSHIFT: c_ulong = IOC_SIZESHIFT + IOC_SIZEBITS;

    const IOC_NONE: c_ulong = 0;

    (IOC_NONE << IOC_DIRSHIFT)
        | ((VFIO_TYPE as c_ulong) << IOC_TYPESHIFT)
        | ((VFIO_BASE as c_ulong + index) << IOC_NRSHIFT)
        | (0 << IOC_SIZESHIFT)
}

fn ioctl_return_to_result(ret: i32) -> io::Result<i32> {
    if ret >= 0 {
        Ok(ret)
    } else {
        Err(io::Error::last_os_error())
    }
}

/* ---------------------------------------------------------------------------------------------- */

define_ioctl!(vfio_get_api_version, 0);
define_ioctl!(vfio_check_extension, 1, extension: usize);
define_ioctl!(vfio_set_iommu, 2, iommu_type: usize);

define_ioctl!(vfio_group_get_status, 3, status: *mut vfio_group_status);
define_ioctl!(vfio_group_set_container, 4, fd: *const i32);
define_ioctl!(vfio_group_get_device_fd, 6, address: *const c_char);

define_ioctl!(vfio_device_get_info, 7, info: *mut vfio_device_info);
define_ioctl!(vfio_device_get_region_info, 8, info: *mut vfio_region_info);
define_ioctl!(vfio_device_get_irq_info, 9, info: *mut vfio_irq_info);
define_ioctl!(vfio_device_set_irqs, 10, set: *const vfio_irq_set);
define_ioctl!(vfio_device_reset, 11);

define_ioctl!(vfio_iommu_get_info, 12, info: *mut vfio_iommu_type1_info);
define_ioctl!(
    vfio_iommu_map_dma,
    13,
    info: *const vfio_iommu_type1_dma_map
);
define_ioctl!(
    vfio_iommu_unmap_dma,
    14,
    info: *mut vfio_iommu_type1_dma_unmap
);

/* ---------------------------------------------------------------------------------------------- */
