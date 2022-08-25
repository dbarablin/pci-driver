// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use nix::libc::c_char;
use nix::{
    ioctl_none, ioctl_read_bad, ioctl_readwrite_bad, ioctl_write_int_bad, ioctl_write_ptr_bad,
    request_code_none,
};

use crate::backends::vfio::bindings::{
    vfio_device_info, vfio_group_status, vfio_region_info, VFIO_BASE, VFIO_TYPE,
};

/* ---------------------------------------------------------------------------------------------- */

ioctl_none!(vfio_get_api_version, VFIO_TYPE, VFIO_BASE);

ioctl_write_int_bad!(
    vfio_check_extension,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 1)
);

ioctl_write_int_bad!(vfio_set_iommu, request_code_none!(VFIO_TYPE, VFIO_BASE + 2));

ioctl_read_bad!(
    vfio_group_get_status,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 3),
    vfio_group_status
);

ioctl_write_ptr_bad!(
    vfio_group_set_container,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 4),
    i32
);

ioctl_write_ptr_bad!(
    vfio_group_get_device_fd,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 6),
    c_char
);

ioctl_read_bad!(
    vfio_device_get_info,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 7),
    vfio_device_info
);

ioctl_readwrite_bad!(
    vfio_device_get_region_info,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 8),
    vfio_region_info
);

/* ---------------------------------------------------------------------------------------------- */
