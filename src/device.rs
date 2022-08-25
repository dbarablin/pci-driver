// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use std::fmt::Debug;
use std::io;
use std::os::unix::io::RawFd;

use crate::config::PciConfig;
use crate::interrupts::{PciInterruptKind, PciInterrupts};
use crate::iommu::PciIommu;
use crate::regions::{OwningPciRegion, Permissions, RegionIdentifier};

/* ---------------------------------------------------------------------------------------------- */

pub(crate) use private::Sealed;
mod private {
    /// Private trait that can be used as a supertrait to make other traits non-implementable from
    /// outside this crate: https://jack.wrenn.fyi/blog/private-trait-methods/
    pub trait Sealed {}
}

/// Represents a PCI __function__.
///
/// This trait is _sealed_ for forward-compatibility reasons, and thus cannot be implemented by
/// users of the crate.
pub trait PciDevice: Debug + Send + Sync + Sealed {
    /// Returns a thing that lets you access the PCI configuration space.
    ///
    /// The returned value borrows the `PciDevice`.
    fn config(&self) -> PciConfig;

    /// Returns a region that corresponds to the Base Address Register (BAR) with the given index,
    /// or `None` if there is no such BAR or it is unused by the device.
    ///
    /// Unused BARs appear as [`None`]. Also, if you want to refer to a 64-bit BAR, use the lower
    /// index (of the underlying, consecutive 32-bit BARs); the higher index is [`None`] in that
    /// case.
    ///
    /// Note that PCI allows used BARs to be interspersed with unused BARs. Also, 64-bit BARs don't
    /// need to be "aligned". For instance, it is possible for a device to use BAR 0 as a 32-bit
    /// BAR, leave BARs 1 and 2 unused, used 3 and 4 for a 64-bit BAR, and use BAR 5 for another
    /// 32-bit BAR.
    ///
    /// The returned value does _not_ borrow the `PciDevice`, instead sharing ownership of its
    /// internal resources, so take care to drop it when you want to fully let go of the device.
    fn bar(&self, index: usize) -> Option<OwningPciRegion>;

    /// Returns a region that is the PCI Expansion ROM, or `None` if the device doesn't have one.
    ///
    /// The returned value does _not_ borrow the `PciDevice`, instead sharing ownership of its
    /// internal resources, so take care to drop it when you want to fully let go of the device.
    fn rom(&self) -> Option<OwningPciRegion>;

    // TODO: Also expose VGA space?

    /// Returns a thing that lets you manage IOMMU mappings for DMA.
    ///
    /// NOTE: Depending on the backend and on how the `PciDevice` was instantiated, this may also
    /// affect IOMMU mappings for other PCI functions.
    ///
    /// The returned value borrows the `PciDevice`.
    fn iommu(&self) -> PciIommu;

    /// Returns a thing that lets you manage interrupts.
    ///
    /// The returned value borrows the `PciDevice`.
    fn interrupts(&self) -> PciInterrupts;

    /// Reset this function, and only it.
    ///
    /// This will fail if it would be necessary to reset other functions or devices as well to get
    /// this one to be reset (probably can only happen with multi-function devices that don't
    /// support Function-Level Reset).
    ///
    /// This can also fail for other unspecified reasons.
    ///
    /// TODO: Should probably advertise whether this granularity of reset is supported, so the user
    /// doesn't have to try resetting to find out.
    fn reset(&self) -> io::Result<()>;
}

/* ---------------------------------------------------------------------------------------------- */

pub(crate) trait PciDeviceInternal: Debug + Send + Sync {
    // BARs / ROM

    fn region_map(
        &self,
        identifier: RegionIdentifier,
        offset: u64,
        len: usize,
        permissions: Permissions,
    ) -> io::Result<*mut u8>;

    unsafe fn region_unmap(&self, identifier: RegionIdentifier, address: *mut u8, length: usize);

    // Interrupts

    fn interrupts_max(&self, kind: PciInterruptKind) -> usize;
    fn interrupts_enable(&self, kind: PciInterruptKind, eventfds: &[RawFd]) -> io::Result<()>;
    fn interrupts_disable(&self, kind: PciInterruptKind) -> io::Result<()>;
}

/* ---------------------------------------------------------------------------------------------- */
