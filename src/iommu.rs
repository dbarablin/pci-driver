// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use std::io;
use std::ops::Range;

use crate::regions::Permissions;

/* ---------------------------------------------------------------------------------------------- */

/// Represents an IOMMU that controls DMA done by some PCI function, device, or group of devices.
///
/// You'll probably need [`std::sync::atomic::fence`] or use types like
/// [`AtomicU32`](std::sync::atomic::AtomicU32) somewhere to synchronize accesses properly with the
/// device.
pub struct PciIommu<'a> {
    pub(crate) internal: &'a dyn PciIommuInternal,
}

impl PciIommu<'_> {
    /// Both `iova` and process `address` must be aligned to this value.
    ///
    /// This is always a power of 2, and never less than the system's page size.
    pub fn alignment(&self) -> usize {
        self.internal.alignment()
    }

    /// IOVA ranges given to [`PciIommu::map`] must be contained in one of the ranges that this
    /// method returns.
    pub fn valid_iova_ranges(&self) -> &[Range<u64>] {
        self.internal.valid_iova_ranges()
    }

    /// Add the given mapping to the IOMMU.
    ///
    /// - `iova` is the start address of the region in the device's address space.
    /// - `size` is the length of the region.
    /// - `address` is a pointer (in the current process' address space) to the start of the region
    ///   to be mapped.
    ///
    /// TODO: Alignment constraints?
    ///
    /// # Safety
    ///
    /// Must make sense.
    pub unsafe fn map(
        &self,
        iova: u64,
        length: usize,
        address: *const u8,
        device_permissions: Permissions,
    ) -> io::Result<()> {
        unsafe { self.internal.map(iova, length, address, device_permissions) }
    }

    /// Remove the given mapping from the IOMMU.
    ///
    /// TODO: Alignment constraints?
    ///
    /// Must unmap exactly a full range that was previously mapped using [`PciIommu::map`], or
    /// several full ranges as long as they are contiguous. Otherwise, this fails.
    pub fn unmap(&self, iova: u64, size: usize) -> io::Result<()> {
        self.internal.unmap(iova, size)
    }
}

/* ---------------------------------------------------------------------------------------------- */

pub(crate) trait PciIommuInternal {
    fn alignment(&self) -> usize;

    fn valid_iova_ranges(&self) -> &[Range<u64>];

    unsafe fn map(
        &self,
        iova: u64,
        length: usize,
        address: *const u8,
        device_permissions: Permissions,
    ) -> io::Result<()>;

    fn unmap(&self, iova: u64, length: usize) -> io::Result<()>;
}

/* ---------------------------------------------------------------------------------------------- */
