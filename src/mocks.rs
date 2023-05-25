// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use std::io;

use mockall::mock;

use crate::config::PciConfig;
use crate::device::PciDevice;
use crate::device::Sealed as DeviceSealed;
use crate::interrupts::PciInterrupts;
use crate::iommu::PciIommu;
use crate::regions::OwningPciRegion;
use crate::regions::PciRegion;
use crate::regions::Permissions;
use crate::regions::Sealed as RegionSealed;

/* ---------------------------------------------------------------------------------------------- */

mock! {
    /// Since the PciDevice trait is sealed and cannot be implemented by users of the crate,
    /// we provide a convenient MockPciDevice struct to facilitate crate user's testing.
    #[derive(Debug)]
    pub PciDevice {}

    impl PciDevice for PciDevice {
        fn config<'a>(&self) -> PciConfig<'static>;
        fn bar<'a>(&self, index: usize) -> Option<OwningPciRegion>;
        fn bar_region<'a>(&self, index: usize) -> Option<Box<dyn PciRegion>>;
        fn rom<'a>(&self) -> Option<OwningPciRegion>;
        fn iommu<'a>(&self) -> Option<PciIommu<'static>>;
        fn interrupts<'a>(&self) -> PciInterrupts<'static>;
        fn reset<'a>(&self) -> io::Result<()>;
    }

    impl DeviceSealed for PciDevice {}
}

mock! {
    #[derive(Debug)]
    pub PciRegion {}

    impl PciRegion for PciRegion {
        fn len(&self) -> u64;
        fn permissions(&self) -> Permissions;
        fn as_ptr(&self) -> Option<*const u8>;
        fn as_mut_ptr(&self) -> Option<*mut u8>;
        fn read_bytes(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()>;
        fn read_u8(&self, offset: u64) -> io::Result<u8>;
        fn write_u8(&self, offset: u64, value: u8) -> io::Result<()>;
        fn read_le_u16(&self, offset: u64) -> io::Result<u16>;
        fn write_le_u16(&self, offset: u64, value: u16) -> io::Result<()>;
        fn read_le_u32(&self, offset: u64) -> io::Result<u32>;
        fn write_le_u32(&self, offset: u64, value: u32) -> io::Result<()>;
    }

    impl RegionSealed for PciRegion {}
}

// TODO: Add mocks for other structs

/* ---------------------------------------------------------------------------------------------- */
