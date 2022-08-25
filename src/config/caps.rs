// SPDX-License-Identifier: MIT OR Apache-2.0

//! Provides facilities for accessing Capabilities described in the PCI Configuration Space.
//!
//! For Extended Capabilities, see [`pci_driver::config::ext_caps`](`super::ext_caps`).
//!
//! The following table relates the section numbers and titles from the "PCI Express® Base
//! Specification Revision 6.0" describing Capabilities to the corresponding type:
//!
//! | Section number | Section title | Type |
//! |-|-|-|
//! | 7.5.2 | PCI Power Management Capability Structure | [`PciPowerManagementCapability`] |
//! | 7.5.3 | PCI Express Capability Structure | [`PciExpressCapability`] |
//! | 7.7.1 | MSI Capability Structures | [`MsiCapability`] <br> [`Msi32BitCapability`] <br> [`Msi64BitCapability`] <br> [`Msi32BitPvmCapability`] <br> [`Msi64BitPvmCapability`] |
//! | 7.7.2 | MSI-X Capability and Table Structure | [`MsiXCapability`] |
//! | 7.8.5 | Enhanced Allocation Capability Structure (EA) | [`EnhancedAllocationCapability`] |
//! | 7.9.4 | Vendor-Specific Capability | [`VendorSpecificCapability`] |
//! | 7.9.18 | Vital Product Data Capability (VPD Capability) | [`VitalProductDataCapability`] |
//! | 7.9.21 | Conventional PCI Advanced Features Capability (AF) | [`ConventionalPciAdvancedFeaturesCapability`] |
//! | 7.9.27 | Null Capability | [`NullCapability`] |

/* ---------------------------------------------------------------------------------------------- */

use std::fmt::Debug;
use std::io::{self, ErrorKind};
use std::iter::{Flatten, FusedIterator};
use std::marker::PhantomData;
use std::ops::Range;
use std::vec;

use crate::config::PciConfig;
use crate::regions::structured::{PciRegisterRo, PciRegisterRw};
use crate::regions::{AsPciSubregion, BackedByPciSubregion, PciRegion, PciSubregion};
use crate::{pci_bit_field, pci_struct};

/* ---------------------------------------------------------------------------------------------- */

/// Some specific type of PCI Capability.
pub trait Capability<'a>: PciRegion + AsPciSubregion<'a> + Clone + Copy + Debug + Sized {
    /// Tries to create an instance of this `Capability` backed by the given [`AsPciSubregion`]. If
    /// things like for instance the Capablity ID and possibly other factors don't match what is
    /// expected for the present type, returns `Ok(None)`.
    ///
    /// Implementations should also make sure that the subregion is big enough, and fail with an
    /// error if it isn't.
    fn backed_by(as_subregion: impl AsPciSubregion<'a>) -> io::Result<Option<Self>>;

    /// The spec doesn't really define a header part explicitly, but this holds the two fields that
    /// are common to all Capabilities.
    fn header(&self) -> CapabilityHeader<'a>;
}

pci_struct! {
    /// The spec doesn't really define a header part explicitly, but this holds the two fields that
    /// are common to all Capabilities.
    pub struct CapabilityHeader<'a> : 0x02 {
        capability_id           @ 0x00 : PciRegisterRo<'a, u8>,
        /// This field contains the offset to the next PCI Capability structure or 0x00 if no other
        /// items exist in the linked list of Capabilities.
        ///
        /// You don't need to be using this directly. Use [`PciCapabilities`] to iterate over
        /// capabilities instead.
        next_capability_pointer @ 0x01 : PciRegisterRo<'a, u8>,
    }
}

/* ---------------------------------------------------------------------------------------------- */

/// Lets you inspect and manipulate the PCI Capabilities defined in the configuration space of some
/// PCI device.
#[derive(Clone, Debug)]
pub struct PciCapabilities<'a> {
    cap_subregions: Box<[PciSubregion<'a>]>,
}

impl<'a> PciCapabilities<'a> {
    pub fn backed_by(config_space: PciConfig<'a>) -> io::Result<Self> {
        const CAP_RANGE: Range<usize> = 0x40..0x100;

        // Number of bytes after PCI header and before end of compat config space
        const ITERATIONS_UPPER_BOUND: usize = CAP_RANGE.end - CAP_RANGE.start;

        if config_space.len() < 0x100 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "Config space is 0x{:x} bytes long, expected at least 0x100",
                    config_space.len(),
                ),
            ));
        }

        if !config_space.status().capabilities_list().read()? {
            // no capabilities pointer
            return Ok(PciCapabilities {
                cap_subregions: Box::new([]),
            });
        }

        let mut cap_subregions = Vec::new();
        let mut next_cap_offset = config_space.read_u8(0x34)? & 0xfc;

        while next_cap_offset != 0x00 {
            if !CAP_RANGE.contains(&(next_cap_offset as usize)) {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Capability has offset 0x{:02x}, should be in [0x40, 0xff]",
                        next_cap_offset,
                    ),
                ));
            }

            if cap_subregions.len() == ITERATIONS_UPPER_BOUND {
                return Err(io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Found more than {} Capabilities, which implies a capability list cycle",
                        ITERATIONS_UPPER_BOUND,
                    ),
                ));
            }

            let cap_subregion = config_space.subregion(next_cap_offset.into()..0x100);
            let cap_header = CapabilityHeader::backed_by(cap_subregion);

            cap_subregions.push(cap_subregion);
            next_cap_offset = cap_header.next_capability_pointer().read()? & 0xfc;
        }

        Ok(PciCapabilities {
            cap_subregions: cap_subregions.into_boxed_slice(),
        })
    }

    /// Returns an iterator over all capabilities.
    pub fn iter(&self) -> PciCapabilitiesIter<'a, UnspecifiedCapability<'a>> {
        // UnspecifiedCapability::backed_by() never fails, so we unwrap()
        self.of_type().unwrap()
    }

    /// Returns an iterator over the capabilities that can be represented by `C`.
    ///
    /// This works by trying [`C::backed_by`](Capability::backed_by) on every capability.
    pub fn of_type<C: Capability<'a>>(&self) -> io::Result<PciCapabilitiesIter<'a, C>> {
        let iter = self
            .cap_subregions
            .iter()
            .map(C::backed_by)
            .collect::<io::Result<Vec<_>>>()?
            .into_iter()
            .flatten();

        Ok(PciCapabilitiesIter {
            iter,
            phantom: PhantomData,
        })
    }
}

impl<'a> IntoIterator for PciCapabilities<'a> {
    type Item = UnspecifiedCapability<'a>;
    type IntoIter = PciCapabilitiesIntoIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        PciCapabilitiesIntoIter {
            iter: Vec::from(self.cap_subregions).into_iter(),
        }
    }
}

impl<'a, 'b> IntoIterator for &'b PciCapabilities<'a> {
    type Item = UnspecifiedCapability<'a>;
    type IntoIter = PciCapabilitiesIter<'a, UnspecifiedCapability<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/* ---------------------------------------------------------------------------------------------- */

/// An iterator over all PCI Capabilities of a device.
pub struct PciCapabilitiesIntoIter<'a> {
    iter: vec::IntoIter<PciSubregion<'a>>,
}

impl<'a> Iterator for PciCapabilitiesIntoIter<'a> {
    type Item = UnspecifiedCapability<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let subregion = self.iter.next()?;
        UnspecifiedCapability::backed_by(subregion).unwrap()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl FusedIterator for PciCapabilitiesIntoIter<'_> {}

/* ---------------------------------------------------------------------------------------------- */

/// An iterator over a device's PCI Capabilities of a certain type.
pub struct PciCapabilitiesIter<'a, C: Capability<'a>> {
    iter: Flatten<vec::IntoIter<Option<C>>>,
    phantom: PhantomData<&'a ()>,
}

impl<'a, C: Capability<'a>> Iterator for PciCapabilitiesIter<'a, C> {
    type Item = C;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl<'a, C: Capability<'a>> FusedIterator for PciCapabilitiesIter<'a, C> {}

/* ---------------------------------------------------------------------------------------------- */

macro_rules! pci_capability {
    (
        $(
            $(#[$attr:meta])*
            $vis:vis struct $name:ident<$lifetime:lifetime> {
                $(Id = $id:literal,)?
                $(Matcher = $matcher:expr,)?
                Length = $length:expr,
                Fields = {
                    $(
                        $(#[$field_attr:meta])*
                        $field_name:ident @ $field_offset:literal :
                        $($field_type:ident)::+$(<$($field_generics:tt),+ $(,)?>)?
                    ),* $(,)?
                } $(,)?
            }
        )*
    ) => {
        $(
            $(#[$attr])*
            #[derive(Clone, Copy)]
            $vis struct $name<$lifetime> {
                subregion: $crate::regions::PciSubregion<$lifetime>,
            }

            impl<'a> Capability<'a> for $name<'a> {
                fn backed_by(as_subregion: impl $crate::regions::AsPciSubregion<'a>) -> ::std::io::Result<Option<Self>> {
                    let subregion = $crate::regions::AsPciSubregion::as_subregion(&as_subregion);

                    $(
                        let header = $crate::config::caps::CapabilityHeader::backed_by(subregion);
                        if header.capability_id().read()? != $id {
                            return ::std::io::Result::Ok(::std::option::Option::None);
                        }
                    )?

                    // construct capability from a subregion that may be unnecessary long

                    let cap = $name { subregion };

                    $(
                        let matcher_fn: fn(&Self) -> ::std::io::Result<bool> = $matcher;
                        if !matcher_fn(&cap)? {
                            return ::std::io::Result::Ok(::std::option::Option::None);
                        }
                    )?

                    let length_fn: fn(&Self) -> ::std::io::Result<u8> = $length;
                    let length: u64 = length_fn(&cap)?.into();

                    // construct new capability from a subregion with just the right size

                    let cap = $name {
                        subregion: subregion.subregion(..length),
                    };

                    ::std::io::Result::Ok(::std::option::Option::Some(cap))
                }

                fn header(&self) -> $crate::config::caps::CapabilityHeader<'a> {
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

pci_capability! {
    /// Some/any PCI Capability.
    pub struct UnspecifiedCapability<'a> {
        Length = |_cap| Ok(0x02),
        Fields = {},
    }
}

// 7.5.2 PCI Power Management Capability Structure

pci_capability! {
    pub struct PciPowerManagementCapability<'a> {
        Id = 0x01,
        Length = |_cap| Ok(0x08),
        Fields = {
            // TODO
        },
    }
}

// 7.5.3 PCI Express Capability Structure

pci_capability! {
    /// TODO: Should take the "Capability Version" into consideration.
    pub struct PciExpressCapability<'a> {
        Id = 0x10,
        Length = |_cap| Ok(0x3c),
        Fields = {
            capabilities          @ 0x02 : PciExpressCapabilities,
            device_capabilities   @ 0x04 : PciExpressDeviceCapabilities,
            device_control        @ 0x08 : PciExpressDeviceControl,
            device_status         @ 0x0a : PciExpressDeviceStatus,
            link_capabilities     @ 0x0c : PciExpressLinkCapabilities,
            link_control          @ 0x10 : PciExpressLinkControl,
            link_status           @ 0x12 : PciExpressLinkStatus,
            device_capabilities_2 @ 0x24 : PciExpressDeviceCapabilities2,
            device_control_2      @ 0x28 : PciExpressDeviceControl2,
            link_capabilities_2   @ 0x2c : PciExpressLinkCapabilities2,
            link_control_2        @ 0x30 : PciExpressLinkControl2,
            link_status_2         @ 0x32 : PciExpressLinkStatus2,
        },
    }
}

pci_bit_field! {
    pub struct PciExpressCapabilities<'a> : RO u16 {
        // TODO
    }

    pub struct PciExpressDeviceCapabilities<'a> : RO u32 {
        max_payload_size_supported      @   0--2 : RO u8,
        phantom_functions_supported     @   3--4 : RO u8,
        extended_tag_field_supported    @      5 : RO,
        endpoint_l0s_acceptable_latency @  6-- 8 : RO u8,
        endpoint_l1_acceptable_latency  @  9--11 : RO u8,
        __                              @ 12--14 : RsvdP,
        role_based_error_reporting      @     15 : RO,
        err_cor_subclass_capable        @     16 : RO,
        rx_mps_fixed                    @     17 : RO,
        captured_slot_power_limit_value @ 18--25 : RO u8,
        captured_slot_power_limit_scale @ 26--27 : RO u8,
        function_level_reset_capability @     28 : RO,
        mixed_mps_supported             @     29 : RO,
        __                              @ 30--31 : RsvdP,
    }

    pub struct PciExpressDeviceControl<'a> : RW u16 {
        // TODO
    }

    pub struct PciExpressDeviceStatus<'a> : RW u16 {
        // TODO
    }

    pub struct PciExpressLinkCapabilities<'a> : RO u32 {
        // TODO
    }

    pub struct PciExpressLinkControl<'a> : RW u16 {
        // TODO
    }

    pub struct PciExpressLinkStatus<'a> : RW u16 {
        // TODO
    }

    pub struct PciExpressDeviceCapabilities2<'a> : RO u32 {
        // TODO
    }

    pub struct PciExpressDeviceControl2<'a> : RW u16 {
        // TODO
    }

    pub struct PciExpressLinkCapabilities2<'a> : RO u32 {
        // TODO
    }

    pub struct PciExpressLinkControl2<'a> : RW u16 {
        // TODO
    }

    pub struct PciExpressLinkStatus2<'a> : RW u16 {
        // TODO
    }
}

// 7.7.1 MSI Capability Structures

pci_capability! {
    pub struct MsiCapability<'a> {
        Id = 0x05,
        Length = |cap| {
            let bit_64 = cap.message_control().bit_64_address_capable().read()?;
            let pvm = cap.message_control().per_vector_masking_capable().read()?;
            Ok(match (bit_64, pvm) {
                (false, false) => 0x0c,
                ( true, false) => 0x10,
                (false,  true) => 0x14,
                ( true,  true) => 0x18,
            })
        },
        Fields = {
            message_control @ 0x02 : MsiMessageControl<'a>,
            // TODO
        },
    }

    pub struct Msi32BitCapability<'a> {
        Id = 0x05,
        Matcher = |cap| {
            let bit_64 = cap.message_control().bit_64_address_capable().read()?;
            let pvm = cap.message_control().per_vector_masking_capable().read()?;
            Ok(!bit_64 && !pvm)
        },
        Length = |_cap| Ok(0x0c),
        Fields = {
            message_control @ 0x02 : MsiMessageControl<'a>,
            // TODO
        },
    }

    pub struct Msi64BitCapability<'a> {
        Id = 0x05,
        Matcher = |cap| {
            let bit_64 = cap.message_control().bit_64_address_capable().read()?;
            let pvm = cap.message_control().per_vector_masking_capable().read()?;
            Ok(bit_64 && !pvm)
        },
        Length = |_cap| Ok(0x10),
        Fields = {
            message_control @ 0x02 : MsiMessageControl<'a>,
            // TODO
        },
    }

    pub struct Msi32BitPvmCapability<'a> {
        Id = 0x05,
        Matcher = |cap| {
            let bit_64 = cap.message_control().bit_64_address_capable().read()?;
            let pvm = cap.message_control().per_vector_masking_capable().read()?;
            Ok(!bit_64 && pvm)
        },
        Length = |_cap| Ok(0x14),
        Fields = {
            message_control @ 0x02 : MsiMessageControl<'a>,
            // TODO
        },
    }

    pub struct Msi64BitPvmCapability<'a> {
        Id = 0x05,
        Matcher = |cap| {
            let bit_64 = cap.message_control().bit_64_address_capable().read()?;
            let pvm = cap.message_control().per_vector_masking_capable().read()?;
            Ok(bit_64 && pvm)
        },
        Length = |_cap| Ok(0x18),
        Fields = {
            message_control @ 0x02 : MsiMessageControl<'a>,
            // TODO
        },
    }
}

pci_bit_field! {
    pub struct MsiMessageControl<'a> : RW u16 {
        msi_enable                    @      0 : RW,
        multiple_message_capable      @   1--3 : RO u8,
        multiple_message_enable       @   4--6 : RW u8,
        bit_64_address_capable        @      7 : RO,
        per_vector_masking_capable    @      8 : RO,
        extended_message_data_capable @      9 : RO,
        extended_message_data_enable  @     10 : RW,
        __                            @ 11--15 : RsvdP,
    }
}

// 7.7.2 MSI-X Capability and Table Structure

pci_capability! {
    pub struct MsiXCapability<'a> {
        Id = 0x11,
        Length = |_cap| Ok(0x0c),
        Fields = {
            // TODO
        },
    }
}

// 7.8.5 Enhanced Allocation Capability Structure (EA)

pci_capability! {
    pub struct EnhancedAllocationCapability<'a> {
        Id = 0x14,
        Length = |cap| {
            let num_entries = cap.read_u8(0x02)? & 0x3f;
            let mut cursor = 0x04;
            for _ in 0..num_entries {
                let entry_size = cap.read_u8(cursor.into())? & 0x07;
                cursor += 1 + entry_size;
            }
            Ok(cursor)
        },
        Fields = {
            // TODO
        },
    }
}

// 7.9.4 Vendor-Specific Capability

pci_capability! {
    pub struct VendorSpecificCapability<'a> {
        Id = 0x09,
        Length = |cap| cap.capability_length().read(),
        Fields = {
            capability_length @ 0x02 : PciRegisterRo<'a, u8>,
        },
    }
}

// 7.9.18 Vital Product Data Capability (VPD Capability)

pci_capability! {
    pub struct VitalProductDataCapability<'a> {
        Id = 0x03,
        Length = |_cap| Ok(0x08),
        Fields = {
            vpd_address_register @ 0x02 : VpdAddressRegister,
            vpd_data_register    @ 0x04 : PciRegisterRw<'a, u32>,
        },
    }
}

pci_bit_field! {
    /// The "VPD Address Register"'s two elements are/can be RW, but we expose them as RO because
    /// they are supposed to be written only together at once.
    pub struct VpdAddressRegister<'a> : RW u16 {
        vpd_address @ 0--14 : RO u16,
        f           @    15 : RO,
    }
}

// 7.9.21 Conventional PCI Advanced Features Capability (AF)

pci_capability! {
    pub struct ConventionalPciAdvancedFeaturesCapability<'a> {
        Id = 0x13,
        Length = |_cap| Ok(0x06),
        Fields = {
            // TODO
        },
    }
}

// 7.9.27 Null Capability

pci_capability! {
    pub struct NullCapability<'a> {
        Id = 0x00,
        Length = |_cap| Ok(0x02),
        Fields = {},
    }
}

/* ---------------------------------------------------------------------------------------------- */
