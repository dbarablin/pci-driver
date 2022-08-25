// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use std::fmt::Debug;

use crate::regions::PciRegion;

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
    fn config(&self) -> &dyn PciRegion;
}

/* ---------------------------------------------------------------------------------------------- */
