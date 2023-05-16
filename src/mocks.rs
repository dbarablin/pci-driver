// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use std::io;

use mockall::mock;

use crate::config::PciConfig;
use crate::device::PciDevice;
use crate::device::Sealed;
use crate::interrupts::PciInterrupts;
use crate::iommu::PciIommu;
use crate::regions::OwningPciRegion;

/* ---------------------------------------------------------------------------------------------- */

mock! {
    /// Since the PciDevice trait is sealed and cannot be implemented by users of the crate,
    /// we provide a convenient MockPciDevice struct to facilitate crate user's testing.
    #[derive(Debug)]
    pub PciDevice {}

    impl PciDevice for PciDevice {
        fn config<'a>(&self) -> PciConfig<'static>;
        fn bar<'a>(&self, index: usize) -> Option<OwningPciRegion>;
        fn rom<'a>(&self) -> Option<OwningPciRegion>;
        fn iommu<'a>(&self) -> Option<PciIommu<'static>>;
        fn interrupts<'a>(&self) -> PciInterrupts<'static>;
        fn reset<'a>(&self) -> io::Result<()>;
    }

    impl Sealed for PciDevice {}
}

// TODO: Add mocks for other structs

/* ---------------------------------------------------------------------------------------------- */
