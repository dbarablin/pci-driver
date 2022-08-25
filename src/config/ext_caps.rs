// SPDX-License-Identifier: MIT OR Apache-2.0

//! Provides facilities for accessing Extended Capabilities described in some PCI configuration
//! space.
//!
//! For plain old non-extended Capabilities, see [`pci_driver::config::caps`](`super::caps`).
//!
//! The following table relates the section numbers and titles from the "PCI Express® Base
//! Specification Revision 6.0" describing Extended Capabilities to the corresponding type:
//!
//! | Section number | Section title | Type |
//! |-|-|-|
//! | 7.9.5 | Vendor-Specific Extended Capability | [`VendorSpecificExtendedCapability`] |
//! | 7.9.28 | Null Extended Capability | [`NullExtendedCapability`] |

/* ---------------------------------------------------------------------------------------------- */

use std::fmt::Debug;
use std::io::{self, ErrorKind};
use std::iter::{Flatten, FusedIterator};
use std::marker::PhantomData;
use std::ops::Range;
use std::vec;

use crate::config::caps::PciExpressCapability;
use crate::config::PciConfig;
use crate::pci_bit_field;
use crate::regions::{AsPciSubregion, BackedByPciSubregion, PciRegion, PciSubregion};

/* ---------------------------------------------------------------------------------------------- */

/// Some specific type of PCI Extended Capability.
pub trait ExtendedCapability<'a>:
    PciRegion + AsPciSubregion<'a> + Clone + Copy + Debug + Sized
{
    /// Tries to create an instance of this `Capability` backed by the given [`AsPciSubregion`]. If
    /// things like for instance the Capablity ID and Capability Version and possibly other factors
    /// don't match what is expected for the present type, returns `Ok(None)`.
    ///
    /// Implementations should also make sure that the subregion is big enough, and fail with an
    /// error if it isn't.
    fn backed_by(as_subregion: impl AsPciSubregion<'a>) -> io::Result<Option<Self>>;

    /// The header of the Extended Capability.
    fn header(&self) -> ExtendedCapabilityHeader<'a>;
}

pci_bit_field! {
    pub struct ExtendedCapabilityHeader<'a> : RO u32 {
        capability_id          @  0--15 : RO u16,
        /// This field is a PCI-SIG defined version number that indicates the version of the
        /// Capability structure present.
        capability_version     @ 16--19 : RO u8,
        /// This field contains the offset to the next PCI Express Capability structure or 0x000 if
        /// no other items exist in the linked list of Capabilities.
        ///
        /// You don't need to be using this directly. Use [`PciExtendedCapabilities`] to iterate
        /// over capabilities instead.
        next_capability_offset @ 20--31 : RO u16,
    }
}

/* ---------------------------------------------------------------------------------------------- */

/// Lets you inspect and manipulate the PCI Extended Capabilities defined in the configuration space
/// of some PCI device.
#[derive(Clone, Debug)]
pub struct PciExtendedCapabilities<'a> {
    cap_subregions: Box<[PciSubregion<'a>]>,
}

impl<'a> PciExtendedCapabilities<'a> {
    pub fn backed_by(config_space: PciConfig<'a>) -> io::Result<Self> {
        const CAP_RANGE: Range<usize> = 0x100..0x1000;

        // Number of 2-byte words in extended config space
        const ITERATIONS_UPPER_BOUND: usize = (CAP_RANGE.end - CAP_RANGE.start) / 2;

        if config_space.len() < 0x1000 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "Config space is 0x{:x} bytes long, expected at least 0x1000",
                    config_space.len()
                ),
            ));
        }

        // This is somewhat expensive, but ensures we don't give unexpected results when the device
        // is not PCI Express.
        if config_space
            .capabilities()?
            .of_type::<PciExpressCapability>()?
            .next()
            .is_none()
        {
            // not a PCI Express device
            return Ok(PciExtendedCapabilities {
                cap_subregions: Box::new([]),
            });
        }

        let mut cap_subregions = Vec::new();
        let mut next_cap_offset = 0x100; // there's always at least one extended capability

        while next_cap_offset != 0x000 {
            if !CAP_RANGE.contains(&(next_cap_offset as usize)) {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Extended Capability has offset 0x{:03x}, should be in [0x100, 0xfff]",
                        next_cap_offset,
                    ),
                ));
            }

            if next_cap_offset % 2 != 0 {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Extended Capability has offset 0x{:03x}, expected multiple of two",
                        next_cap_offset,
                    ),
                ));
            }

            if cap_subregions.len() == ITERATIONS_UPPER_BOUND {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Found more than {} Extended Capabilities, which implies a capability \
                        list cycle",
                        ITERATIONS_UPPER_BOUND,
                    ),
                ));
            }

            let cap_subregion = config_space.subregion(next_cap_offset.into()..0x1000);
            let cap_header = ExtendedCapabilityHeader::backed_by(cap_subregion);

            cap_subregions.push(cap_subregion);
            next_cap_offset = cap_header.next_capability_offset().read()? & 0xfffc;
        }

        Ok(PciExtendedCapabilities {
            cap_subregions: cap_subregions.into_boxed_slice(),
        })
    }

    /// Returns an iterator over all extended capabilities.
    pub fn iter(&self) -> PciExtendedCapabilitiesIter<'a, UnspecifiedExtendedCapability<'a>> {
        // UnspecifiedExtendedCapability::backed_by() never fails, so we unwrap()
        self.of_type().unwrap()
    }

    /// Returns an iterator over the capabilities that can be represented by `C`.
    ///
    /// This works by trying [`C::backed_by`](ExtendedCapability::backed_by) on every capability.
    pub fn of_type<C: ExtendedCapability<'a>>(
        &self,
    ) -> io::Result<PciExtendedCapabilitiesIter<'a, C>> {
        let iter = self
            .cap_subregions
            .iter()
            .map(C::backed_by)
            .collect::<io::Result<Vec<_>>>()?
            .into_iter()
            .flatten();

        Ok(PciExtendedCapabilitiesIter {
            iter,
            phantom: PhantomData,
        })
    }
}

impl<'a> IntoIterator for PciExtendedCapabilities<'a> {
    type Item = UnspecifiedExtendedCapability<'a>;
    type IntoIter = PciExtendedCapabilitiesIntoIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        PciExtendedCapabilitiesIntoIter {
            iter: Vec::from(self.cap_subregions).into_iter(),
        }
    }
}

impl<'a, 'b> IntoIterator for &'b PciExtendedCapabilities<'a> {
    type Item = UnspecifiedExtendedCapability<'a>;
    type IntoIter = PciExtendedCapabilitiesIter<'a, UnspecifiedExtendedCapability<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/* ---------------------------------------------------------------------------------------------- */

/// An iterator over all PCI Extended Capabilities of a device.
pub struct PciExtendedCapabilitiesIntoIter<'a> {
    iter: vec::IntoIter<PciSubregion<'a>>,
}

impl<'a> Iterator for PciExtendedCapabilitiesIntoIter<'a> {
    type Item = UnspecifiedExtendedCapability<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let subregion = self.iter.next()?;
        UnspecifiedExtendedCapability::backed_by(subregion).unwrap()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl FusedIterator for PciExtendedCapabilitiesIntoIter<'_> {}

/* ---------------------------------------------------------------------------------------------- */

/// An iterator over a device's PCI Extended Capabilities of a certain type.
pub struct PciExtendedCapabilitiesIter<'a, C: ExtendedCapability<'a>> {
    iter: Flatten<vec::IntoIter<Option<C>>>,
    phantom: PhantomData<&'a ()>,
}

impl<'a, C: ExtendedCapability<'a>> Iterator for PciExtendedCapabilitiesIter<'a, C> {
    type Item = C;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, C: ExtendedCapability<'a>> FusedIterator for PciExtendedCapabilitiesIter<'a, C> {}

/* ---------------------------------------------------------------------------------------------- */

macro_rules! pci_extended_capability {
    (
        $(
            $(#[$attr:meta])*
            $vis:vis struct $name:ident<$lifetime:lifetime> {
                $(Id = $id:literal,)?
                $(MinVersion = $min_version:literal,)?
                $(Matcher = $matcher:expr,)?
                Length = $length:expr,
                Fields = {
                    $(
                        $(#[$field_attr:meta])*
                        $field_name:ident @ $field_offset:literal :
                        $($field_type:ident)::+$(<$($field_generics:tt),+ $(,)?>)?
                    ),* $(,)?
                },
            }
        )*
    ) => {
        $(
            $(#[$attr])*
            #[derive(Clone, Copy)]
            $vis struct $name<$lifetime> {
                subregion: $crate::regions::PciSubregion<$lifetime>,
            }

            impl<'a> ExtendedCapability<'a> for $name<'a> {
                fn backed_by(as_subregion: impl $crate::regions::AsPciSubregion<'a>) -> ::std::io::Result<Option<Self>> {
                    let subregion = $crate::regions::AsPciSubregion::as_subregion(&as_subregion);

                    $(
                        let header = $crate::config::ext_caps::ExtendedCapabilityHeader::backed_by(subregion);
                        if header.capability_id().read()? != $id {
                            return ::std::io::Result::Ok(::std::option::Option::None);
                        }
                    )?

                    $(
                        let header = $crate::config::ext_caps::ExtendedCapabilityHeader::backed_by(subregion);
                        if header.capability_version().read()? < $min_version {
                            return ::std::io::Result::Ok(::std::option::Option::None);
                        }
                    )?

                    // construct capability from a subregion that may be unnecessary long

                    let cap = $name { subregion };

                    $(
                        let matcher_fn: fn(&Self) -> ::std::io::Result<()> = $matcher;
                        if !matcher_fn(&cap)? {
                            return ::std::io::Result::Ok(::std::option::Option::None);
                        }
                    )?

                    let length_fn: fn(&Self) -> ::std::io::Result<u16> = $length;
                    let length: u64 = length_fn(&cap)?.into();

                    // construct new capability from a subregion with just the right size

                    let cap = $name {
                        subregion: subregion.subregion(..length),
                    };

                    ::std::io::Result::Ok(::std::option::Option::Some(cap))
                }

                fn header(&self) -> $crate::config::ext_caps::ExtendedCapabilityHeader<'a> {
                    $crate::regions::BackedByPciSubregion::backed_by(self.subregion)
                }
            }

            impl<'a> $crate::regions::AsPciSubregion<'a> for $name<'a> {
                fn as_subregion(&self) -> $crate::regions::PciSubregion<'a> {
                    self.subregion
                }
            }

            $crate::_pci_struct_impl! {
                impl $name<$lifetime> {
                    $(
                        $(#[$field_attr])*
                        $field_name @ $field_offset :
                        $($field_type)::+$(<$($field_generics),+>)?
                    ),*
                }
            }
        )*
    };
}

/* ---------------------------------------------------------------------------------------------- */

pci_extended_capability! {
    /// Any PCI Extended Capability.
    pub struct UnspecifiedExtendedCapability<'a> {
        Length = |_cap| Ok(0x004),
        Fields = {},
    }
}

// 7.9.5 Vendor-Specific Extended Capability

pci_extended_capability! {
    /// Described in Section 7.9.5 of the "PCI Express® Base Specification Revision 6.0".
    pub struct VendorSpecificExtendedCapability<'a> {
        Id = 0x000b,
        MinVersion = 0x1,
        Length = |cap| cap.vendor_specific_header().vsec_length().read(),
        Fields = {
            vendor_specific_header @ 0x004 : VendorSpecificHeader,
            // TODO
        },
    }
}

pci_bit_field! {
    /// Described in Section 7.9.5.2 of the "PCI Express® Base Specification Revision 6.0".
    pub struct VendorSpecificHeader<'a> : RO u32 {
        vsec_id     @  0--15 : RO u16,
        vsec_rev    @ 16--19 : RO u8,
        vsec_length @ 20--31 : RO u16,
    }
}

// 7.9.28 Null Extended Capability

pci_extended_capability! {
    /// Described in Section 7.9.28 of the "PCI Express® Base Specification Revision 6.0".
    pub struct NullExtendedCapability<'a> {
        Id = 0x000b,
        Length = |_cap| Ok(0x004),
        Fields = {},
    }
}

/* ---------------------------------------------------------------------------------------------- */
