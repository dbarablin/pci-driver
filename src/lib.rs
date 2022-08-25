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
//! Calling [`PciDevice::config`](device::PciDevice::config) returns a
//! [`PciConfig`](config::PciConfig) value, which provides access to the device's configuration
//! space. Configuration space is made up of 8-bit, 16-bit, and 32-bit registers.
//!
//! Each register may represent a single numeric value (_e.g._, "Vendor ID") or be a bit field. Bit
//! fields are composed of several independent bits (_e.g._, "Command") or sequences of bits
//! (_e.g._, "Status"). In some cases, related registers are organized hierarchically into groups
//! (_e.g._, "Class Code"). (The terms being used here might not match exactly the terminology of
//! the PCI/PCIe specifications.)
//!
//! This crate provides "structured" access to each register, bit field, bit sequence, and bit using
//! specialized accessor methods, so you don't have to remember details like the offsets of
//! registers, masking and shifting for operating on bits, subtleties related to write-1-to-clear
//! and reserved-zero bits, etc.
//!
//! Still, if you really want to, you can bypass all of this and just read and write directly at
//! arbitrary offsets of the configuration space.
//!
//! The API also makes it easy to iterate over Capabilities and Extended Capabilities, and to find
//! capabilities with specific Capability IDs, all while providing the same kind of structured
//! access interface described above.
//!
//! Example usage:
//!
//! ```no_run
//! use pci_driver::config::caps::{Capability, PciExpressCapability};
//! use pci_driver::config::ext_caps::{ExtendedCapability, VendorSpecificExtendedCapability};
//! use pci_driver::config::{PciClassCode, PciConfig};
//! use pci_driver::device::PciDevice;
//! use pci_driver::regions::{BackedByPciSubregion, PciRegion, PciRegionSnapshot};
//!
//! let device: &dyn PciDevice = unimplemented!();
//!
//! // Raw config space access
//!
//! let vendor_id: u16 = device.config().read_le_u16(0x00)?;
//! let device_id: u16 = device.config().read_le_u16(0x02)?;
//!
//! // Structured config space access
//!
//! let device_id: u16 = device.config().device_id().read()?;
//!
//! let memory_space_enable: bool = device.config().command().memory_space_enable().read()?;
//! device.config().command().memory_space_enable().write(true)?;
//!
//! device.config().status().master_data_parity_error().clear()?;
//!
//! let class_code: PciClassCode = device.config().class_code();
//! let base_class_code: u8 = class_code.base_class_code().read()?;
//! let sub_class_code: u8 = class_code.sub_class_code().read()?;
//! let programming_interface: u8 = class_code.programming_interface().read()?;
//!
//! // Capabilities
//!
//! for cap in device.config().capabilities()? {
//!     // cap has type UnspecifiedCapability
//!     let cap_id: u8 = cap.header().capability_id().read()?;
//! }
//!
//! let pcie_cap: Option<PciExpressCapability> = device
//!     .config()
//!     .capabilities()?
//!     .of_type::<PciExpressCapability>()?
//!     .next();
//!
//! if let Some(pcie_cap) = pcie_cap {
//!     println!("PCI Express device");
//!     let supports_flr: bool = pcie_cap
//!         .device_capabilities()
//!         .function_level_reset_capability()
//!         .read()?;
//! } else {
//!     println!("Conventional PCI device");
//! }
//!
//! // Extended capabilities
//!
//! for ext_cap in device.config().extended_capabilities()? {
//!     // cap has type UnspecifiedExtendedCapability
//!     let cap_id: u16 = ext_cap.header().capability_id().read()?;
//! }
//!
//! let vendor_specific_ext_caps: Vec<VendorSpecificExtendedCapability> = device
//!     .config()
//!     .extended_capabilities()?
//!     .of_type::<VendorSpecificExtendedCapability>()?
//!     .collect();
//!
//! // Taking snapshot of entire config space, may improve performance if reading many registers
//!
//! let config_space_snapshot: PciRegionSnapshot = PciRegionSnapshot::take(device.config())?;
//! let device_id: u16 = config_space_snapshot.read_le_u16(0x02)?;
//!
//! let config_space: PciConfig = PciConfig::backed_by(&config_space_snapshot);
//! let device_id: u16 = config_space.read_le_u16(0x02)?;
//! let device_id: u16 = config_space.device_id().read()?;
//! let memory_space_enable: bool = config_space.command().memory_space_enable().read()?;
//!
//! // Taking snapshot only of a specific capability
//!
//! let pcie_cap_snapshot: PciRegionSnapshot = PciRegionSnapshot::take(
//!     config_space
//!         .capabilities()?
//!         .of_type::<PciExpressCapability>()?
//!         .next()
//!         .expect("not a PCIe device")
//! )?;
//!
//! let pcie_cap = PciExpressCapability::backed_by(&pcie_cap_snapshot)?.unwrap();
//! # std::io::Result::Ok(())
//! ```
//!
//! ## BARs and Expansion ROM
//!
//! The [`PciDevice::bar`](device::PciDevice::bar) method can be used to retrieved an
//! [`OwningPciRegion`](regions::OwningPciRegion) corresponding to a given Base Address Register
//! (BAR) of the device. This value behaves similarly to an instance of
//! [`PciConfig`](config::PciConfig), but does not provide the aforementioned "structured access"
//! functionality, as BAR contents are device-specific. In addition,
//! [`OwningPciRegion`](regions::OwningPciRegion) provides the ability to map the region onto
//! process memory (if the region is mappable).
//!
//! A similar [`PciDevice::rom`](device::PciDevice::rom) method is also provided, giving access to
//! the device's "Expansion ROM".
//!
//! Example usage:
//!
//! ```no_run
//! use pci_driver::device::PciDevice;
//! use pci_driver::regions::{MappedOwningPciRegion, OwningPciRegion, PciRegion, PciRegionSnapshot, Permissions};
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
//!
//! // Taking snapshot of BAR 0
//!
//! let bar_0_snapshot: PciRegionSnapshot = PciRegionSnapshot::take(&bar_0)?;
//! # std::io::Result::Ok(())
//! ```
//!
//! See [`pci_struct!` and `pci_bit_field!`](#pci_struct-and-pci_bit_field) further below to see how
//! to easily create structured access APIs of your own, which you can use to access BARs and other
//! regions with device-specific layouts.
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
//!
//! ## `pci_struct!` and `pci_bit_field!`
//!
//! Many times, your device's BARs or ROM will be structured into registers and bit fields similarly
//! to configuration space, or there will be some capabilities that this crate doesn't provide a
//! structured access API for, or even the vendor-specific contents of the Vendor Specific
//! Capability will have some structure that this crate naturally can't capture.
//!
//! In C, you would usually cast pointers to overlay a `struct` on the memory corresponding to the
//! region you want to access, and then use volatile accesses. You can do similar things in Rust,
//! but that is unsafe, only works when the BAR or ROM is memory-mapped, and it can be error-prone
//! to build the structs properly due to padding.
//!
//! A safe alternative is to have `read()` and `write()` functions that take an offset and a length,
//! or are parameterized by the type you want to read or write. This crate provides that kind of
//! access API through the [`PciRegion`](crate::regions::PciRegion) trait, but using it can be
//! cumbersome and it is easy to pass in the wrong offset or read/write the wrong type. Things get
//! even worse when you're bit fiddling to manipulate flags and applying write masks to preserve
//! some bits, etc.
//!
//! To solve this, this crate provides the [`pci_struct!`](crate::pci_struct) and
//! [`pci_bit_field!`](crate::pci_bit_field) macros, which you can use to easily define
//! semantically-aware types that provide structured access to device regions and bit field
//! registers. These are also used by the crate itself to define types like
//! [`PciConfig`](crate::config::PciConfig) and [`PciStatus`](crate::config::PciStatus).
//!
//! Take [`PciClassCode`](crate::config::PciClassCode) as an example:
//!
//! ```no_run
//! use pci_driver::pci_struct;
//! use pci_driver::regions::structured::PciRegisterRo;
//!
//! pci_struct! {
//!     pub struct PciClassCode<'a> : 0x03 {
//!         base_class_code       @ 0x00 : PciRegisterRo<'a, u8>,
//!         sub_class_code        @ 0x01 : PciRegisterRo<'a, u8>,
//!         programming_interface @ 0x02 : PciRegisterRo<'a, u8>,
//!     }
//! }
//! ```
//!
//! Values of this type can be created using `PciClassCode::backed_by(subregion)`, where `subregion`
//! is anything that implements `AsPciSubregion<'a>`, and the structure is taken to begin at the
//! start of that subregion.
//!
//! Each field follows the format `name @ offset : type` and gives rise to a method with the given
//! `name` that returns a value of the given `type`. The `offset` is in bytes from the start of the
//! structure.
//!
//! [`PciConfig`](crate::config::PciConfig)'s definition is another good example:
//!
//! ```no_run
//! use pci_driver::config::{PciClassCode, PciCommand, PciStatus};
//! use pci_driver::pci_struct;
//! use pci_driver::regions::structured::PciRegisterRo;
//!
//! pci_struct! {
//!     pub struct PciConfig<'a> {
//!         vendor_id   @ 0x00 : PciRegisterRo<'a, u16>,
//!         device_id   @ 0x02 : PciRegisterRo<'a, u16>,
//!         command     @ 0x04 : PciCommand<'a>,
//!         status      @ 0x06 : PciStatus<'a>,
//!         revision_id @ 0x08 : PciRegisterRo<'a, u8>,
//!         class_code  @ 0x09 : PciClassCode<'a>,
//!         // ... more fields ...
//!     }
//! }
//! ```
//!
//! Note that one of the fields is actually of the type we defined above: `PciClassCode`. We also
//! specify an offset for it, which will serve as the base offset for the fields that it in turn
//! contains.
//!
//! Note also the "Command" and "Status" fields. These are _bit fields_. Here's how
//! [`PciStatus`](crate::config::PciStatus) is defined:
//!
//! ```no_run
//! use pci_driver::pci_bit_field;
//!
//! pci_bit_field! {
//!     pub struct PciStatus<'a> : RW u16 {
//!         immediate_readiness                    @     0 : RO,
//!         __                                     @  1--2 : RsvdZ,
//!         interrupt_status                       @     3 : RO,
//!         capabilities_list                      @     4 : RO,
//!         mhz_66_capable                         @     5 : RO,
//!         __                                     @     6 : RsvdZ,
//!         fast_back_to_back_transactions_capable @     7 : RO,
//!         master_data_parity_error               @     8 : RW1C,
//!         devsel_timing                          @ 9--10 : RO u8,
//!         signaled_target_abort                  @    11 : RW1C,
//!         received_target_abort                  @    12 : RW1C,
//!         received_master_abort                  @    13 : RW1C,
//!         signaled_system_error                  @    14 : RW1C,
//!         detected_parity_error                  @    15 : RW1C,
//!     }
//! }
//! ```
//!
//! Values of this type can be created using `PciStatus::backed_by(subregion)`, exactly like types
//! defined using `pci_struct!`. The bit field is taken to be at the start of the given subregion.
//!
//! `PciStatus`'s definition also follows the same general scheme as if using `pci_struct!`, but now
//! each line represents a bit or set of bits in a register. First, note the `: RW u16` after the
//! struct name: this means that the struct represents a read-write register that is 16 bits wide.
//!
//! Then, we have the "Immediate Readiness" bit at position 0, i.e., the lowest-order bit in the
//! register. It is read-only, hence the `RO`. The format for each bit is `name @ bit : mode`, while
//! for sets of more than 1 bit it is `name @ first_bit--last_bit : mode` (`first_bit` and
//! `last_bit` are inclusive).
//!
//! Then we have a `__` line with mode `RsvdZ` that represents 2 consecutive bits. The `RsvdZ`
//! terminology comes from the PCI/PCIe specifications, and means that when writing to the register
//! as a whole, these bits must always be written as 0. There's also a `RsvdP` mode which means that
//! the affected bits must be written exactly how they currently read. (These two modes exist for
//! forward-compatibility purposes).
//!
//! A few lines down, we get to the "Master Data Parity Error" bit, which has mode `RW1C`. This
//! means that the bit can be read as usual, and it can be _cleared_ (i.e., made to be 0), but it
//! cannot be _set_ (made to be 1), so the bit isn't quite read-write. (RW1C once again comes from
//! the PCI/PCIe specifications and approximately stands for Read-or-Write-1-to-Clear.) There's also
//! plain `RW` bits, which can be freely read, cleared, and set, and are not showcased in this
//! example.
//!
//! And finally, let's look at "DEVSEL Timing", which occupies bits 9 and 10 and has mode `RO u8`.
//! This is a set of two bits which may only be read, not written, and which reads back as an `u8`
//! (it could also have been `u16` or `u32`).
//!
//! In all these cases, the name of the field gives rise to a method that returns a value that
//! allows you to inspect (and possibly manipulate) the bit or set of bits. (Note that the `name` of
//! the field is ignored for `RsvdZ` and `RsvdP` bits, but it has to be there. It cannot be a single
//! `_` as that is not an identifier, so we use `__` instead.)
//!
//! You don't have to cover every bit in the register, although we do so in the example above.
//! Leaving bits unspecified is equivalent to specifying them as `RsvdP`.
//!
//! Finally, note that when using `pci_struct!` and `pci_bit_field!`, you can add doc comments both
//! to the struct or bit field type itself, and to each of their fields or bits.

/* ---------------------------------------------------------------------------------------------- */

#![cfg_attr(feature = "_unsafe-op-in-unsafe-fn", deny(unsafe_op_in_unsafe_fn))]
#![cfg_attr(not(feature = "_unsafe-op-in-unsafe-fn"), allow(unused_unsafe))]

// TODO: enable:
// #![warn(missing_docs)]

pub mod backends;
pub mod config;
pub mod device;
pub mod interrupts;
pub mod iommu;
pub mod regions;

/* ---------------------------------------------------------------------------------------------- */
