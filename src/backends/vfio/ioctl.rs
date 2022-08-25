// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use nix::libc::c_char;
use nix::{
    ioctl_none, ioctl_read_bad, ioctl_readwrite_bad, ioctl_write_int_bad, ioctl_write_ptr_bad,
    request_code_none,
};

use crate::backends::vfio::bindings::{
    vfio_device_info, vfio_group_status, vfio_iommu_type1_dma_map, vfio_iommu_type1_dma_unmap,
    vfio_iommu_type1_info, vfio_irq_info, vfio_irq_set, vfio_region_info, VFIO_BASE, VFIO_TYPE,
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

ioctl_readwrite_bad!(
    vfio_device_get_irq_info,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 9),
    vfio_irq_info
);

ioctl_write_ptr_bad!(
    vfio_device_set_irqs,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 10),
    vfio_irq_set
);

ioctl_none!(vfio_device_reset, VFIO_TYPE, VFIO_BASE + 11);

ioctl_read_bad!(
    vfio_iommu_get_info,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 12),
    vfio_iommu_type1_info
);

ioctl_write_ptr_bad!(
    vfio_iommu_map_dma,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 13),
    vfio_iommu_type1_dma_map
);

ioctl_readwrite_bad!(
    vfio_iommu_unmap_dma,
    request_code_none!(VFIO_TYPE, VFIO_BASE + 14),
    vfio_iommu_type1_dma_unmap
);

/* ---------------------------------------------------------------------------------------------- */
