// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crate for developing user-space PCI and PCIe drivers.
//!
//! The driver development interface revolves around the [`PciDevice`](device::PciDevice) trait,
//! which represents a PCI __function__ and allows you to:
//!
//! 1. Access its Configuration Space.
//!
//! Implementations of this trait are called _backends_. For now, a single
//! [`VfioPciDevice`](backends::vfio::VfioPciDevice) backend is provided, which relies on Linux's
//! VFIO driver framework. The availability of this backend can be controlled through the `vfio`
//! crate feature. Future backends will each have a corresponding feature. Note that the user cannot
//! implement additional backends from outside this crate.
//!
//! This crate requires Rust 1.47 or above.
//!
//! The following sections showcase [`PciDevice`](device::PciDevice)'s features.
//!
//! ## Configuration space
//!
//! Calling [`PciDevice::config`](device::PciDevice::config) returns a reference to a
//! [`PciRegion`](regions::PciRegion), which provides access to the device's configuration space.
//! Configuration space is made up of 8-bit, 16-bit, and 32-bit registers.
//!
//! Each register may represent a single numeric value (_e.g._, "Vendor ID") or be a bit field. Bit
//! fields are composed of several independent bits (_e.g._, "Command") or sequences of bits
//! (_e.g._, "Status"). In some cases, related registers are organized hierarchically into groups
//! (_e.g._, "Class Code"). (The terms being used here might not match exactly the terminology of
//! the PCI/PCIe specifications.)
//!
//! Example usage:
//!
//! ```no_run
//! use pci_driver::device::PciDevice;
//! use pci_driver::regions::PciRegion;
//!
//! let device: &dyn PciDevice = unimplemented!();
//!
//! // Raw config space access
//!
//! let vendor_id: u16 = device.config().read_le_u16(0x00)?;
//! let device_id: u16 = device.config().read_le_u16(0x02)?;
//! # std::io::Result::Ok(())
//! ```
//!
//! ## VFIO backend specificities
//!
//! In the following example, devices 0000:00:01.0 and 0000:00:02.0 belong to VFIO group 42, device
//! 0000:00:03.0 to group 123.
//!
//! ```no_run
//! use std::sync::Arc;
//! use pci_driver::backends::vfio::{VfioContainer, VfioPciDevice};
//!
//! let container: Arc<VfioContainer> = Arc::new(VfioContainer::new(&[42, 123])?);
//!
//! let device_a = VfioPciDevice::open_in_container("/sys/bus/pci/devices/0000:00:01.0", Arc::clone(&container))?;
//! let device_b = VfioPciDevice::open_in_container("/sys/bus/pci/devices/0000:00:02.0", Arc::clone(&container))?;
//! let device_c = VfioPciDevice::open_in_container("/sys/bus/pci/devices/0000:00:03.0", Arc::clone(&container))?;
//!
//! // Shorthand for when a device is the only one (that we care about) in its group, and the group
//! // is the only one in its container
//!
//! let device = VfioPciDevice::open("/sys/bus/pci/devices/0000:00:01.0")?;
//! # std::io::Result::Ok(())
//! ```

/* ---------------------------------------------------------------------------------------------- */

#![cfg_attr(feature = "_unsafe-op-in-unsafe-fn", deny(unsafe_op_in_unsafe_fn))]
#![cfg_attr(not(feature = "_unsafe-op-in-unsafe-fn"), allow(unused_unsafe))]

// TODO: enable:
// #![warn(missing_docs)]

pub mod backends;
pub mod device;
pub mod regions;

/* ---------------------------------------------------------------------------------------------- */
