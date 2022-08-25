// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crate for developing user-space PCI and PCIe drivers.
//!
//! The driver development interface revolves around the [`PciDevice`](device::PciDevice) trait,
//! which represents a PCI __function__ and allows you to:
//!
//! 1. Access its Configuration Space;
//! 2. Access the regions defined by its Base Address Registers (BARs);
//! 3. Access its Expansion ROM;
//! 4. Add and remove mappings from the IOMMU that controls its DMA operations;
//! 5. Configure its INTx, MSI, and MSI-X interrupt vectors;
//! 6. Reset it.
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
//! ## BARs and Expansion ROM
//!
//! The [`PciDevice::bar`](device::PciDevice::bar) method can be used to retrieved an
//! [`OwningPciRegion`](regions::OwningPciRegion) corresponding to a given Base Address Register
//! (BAR) of the device. The [`OwningPciRegion`](regions::OwningPciRegion) provides the ability to
//! map the region onto process memory (if the region is mappable).
//!
//! A similar [`PciDevice::rom`](device::PciDevice::rom) method is also provided, giving access to
//! the device's "Expansion ROM".
//!
//! Example usage:
//!
//! ```no_run
//! use pci_driver::device::PciDevice;
//! use pci_driver::regions::{MappedOwningPciRegion, OwningPciRegion, PciRegion, Permissions};
//!
//! let device: &dyn PciDevice = unimplemented!();
//!
//! let bar_0: OwningPciRegion = device.bar(0).expect("expected device to have BAR 0");
//! let rom: OwningPciRegion = device.rom().expect("expected device to have Expansion ROM");
//!
//! // Non-memory mapped access (always works, may be slower)
//!
//! assert!(bar_0.permissions().can_read());
//! let value = bar_0.read_le_u32(0x20)?;
//!
//! // Memory-mapped access using `PciRegion` methods
//!
//! assert!(bar_0.permissions() == Permissions::ReadWrite);
//! assert!(bar_0.is_mappable());
//! let mapped_bar_0: MappedOwningPciRegion = bar_0.map(..4096, Permissions::Read)?;
//!
//! let value = mapped_bar_0.read_le_u32(0x20)?;
//!
//! // Memory-mapped access using raw pointers
//!
//! let value = u32::from_le(
//!     unsafe { mapped_bar_0.as_ptr().offset(0x20).cast::<u32>().read_volatile() }
//! );
//! # std::io::Result::Ok(())
//! ```
//!
//! ## IOMMU
//!
//! The [`PciDevice::iommu`](device::PciDevice::iommu) method returns a
//! [`PciIommu`](iommu::PciIommu) value, which can in turn be used to manipulate IOMMU mapping
//! affecting the device.
//!
//! Example usage:
//!
//! ```no_run
//! use pci_driver::device::PciDevice;
//! use pci_driver::regions::Permissions;
//!
//! let device: &dyn PciDevice = unimplemented!();
//!
//! let iova: u64 = 0x12345678;
//! let region_ptr: *const u8 = unimplemented!();
//! let region_len: usize = 4096;
//!
//! unsafe { device.iommu().map(iova, region_len, region_ptr, Permissions::ReadWrite) };
//! // ...
//! unsafe { device.iommu().unmap(iova, region_len) };
//! # std::io::Result::Ok(())
//! ```
//!
//! ## Interrupts
//!
//! The [`PciDevice::interrupts`](device::PciDevice::interrupts) method returns a
//! [`PciInterrupts`](interrupts::PciInterrupts) value, which provides control over the device's
//! interrupt vectors. It allows you to associate specific interrupt vectors with eventfd
//! descriptors, and to undo that association.
//!
//! Example usage:
//!
//! ```no_run
//! use std::os::unix::io::RawFd;
//! use pci_driver::device::PciDevice;
//!
//! let device: &dyn PciDevice = unimplemented!();
//! let eventfds: &[RawFd] = unimplemented!();
//!
//! let max_enabled_intx_vectors = device.interrupts().intx().max();
//! device.interrupts().intx().enable(eventfds)?;
//! device.interrupts().intx().disable()?;
//!
//! let max_enabled_msi_vectors = device.interrupts().msi().max();
//! device.interrupts().msi().enable(eventfds)?;
//! device.interrupts().msi().disable()?;
//!
//! let max_enabled_msi_x_vectors = device.interrupts().msi_x().max();
//! device.interrupts().msi_x().enable(eventfds)?;
//! device.interrupts().msi_x().disable()?;
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
//! use pci_driver::device::PciDevice;
//! use pci_driver::regions::Permissions;
//!
//! let container: Arc<VfioContainer> = Arc::new(VfioContainer::new(&[42, 123])?);
//!
//! let device_a = VfioPciDevice::open_in_container("/sys/bus/pci/devices/0000:00:01.0", Arc::clone(&container))?;
//! let device_b = VfioPciDevice::open_in_container("/sys/bus/pci/devices/0000:00:02.0", Arc::clone(&container))?;
//! let device_c = VfioPciDevice::open_in_container("/sys/bus/pci/devices/0000:00:03.0", Arc::clone(&container))?;
//!
//! unsafe {
//!     let iova: u64 = 0x12345678;
//!     let region_ptr: *const u8 = unimplemented!();
//!     let region_len: usize = 4096;
//!
//!     // All of the following calls are equivalent.
//!
//!     container.iommu().map(iova, region_len, region_ptr, Permissions::ReadWrite);
//!
//!     device_a.iommu().map(iova, region_len, region_ptr, Permissions::ReadWrite);
//!     device_b.iommu().map(iova, region_len, region_ptr, Permissions::ReadWrite);
//!     device_c.iommu().map(iova, region_len, region_ptr, Permissions::ReadWrite);
//! }
//!
//! // Shorthand for when a device is the only one (that we care about) in its group, and the group
//! // is the only one in its container
//!
//! let device = VfioPciDevice::open("/sys/bus/pci/devices/0000:00:01.0")?;
//!
//! // Resetting a PCI function, which may not be supported
//!
//! device.reset()?;
//!
//! // Resetting a whole container, which may also not be supported
//!
//! device.container().reset()?;
//! # std::io::Result::Ok(())
//! ```

/* ---------------------------------------------------------------------------------------------- */

#![cfg_attr(feature = "_unsafe-op-in-unsafe-fn", deny(unsafe_op_in_unsafe_fn))]
#![cfg_attr(not(feature = "_unsafe-op-in-unsafe-fn"), allow(unused_unsafe))]

// TODO: enable:
// #![warn(missing_docs)]

pub mod backends;
pub mod device;
pub mod interrupts;
pub mod iommu;
pub mod regions;

/* ---------------------------------------------------------------------------------------------- */
