// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

pub mod caps;
pub mod ext_caps;

use std::io;

use crate::config::caps::PciCapabilities;
use crate::config::ext_caps::PciExtendedCapabilities;
use crate::regions::structured::{PciRegisterRo, PciRegisterRw};
use crate::{pci_bit_field, pci_struct};

/* ---------------------------------------------------------------------------------------------- */

pci_struct! {
    /// This lets you interact with the config space (conventional or extended) of a PCI device.
    ///
    /// This doesn't have a definite length because we want to preserve the `PciSubregion` over the
    /// whole of config space.
    pub struct PciConfig<'a> {
        vendor_id           @ 0x00 : PciRegisterRo<'a, u16>,
        device_id           @ 0x02 : PciRegisterRo<'a, u16>,
        command             @ 0x04 : PciCommand<'a>,
        status              @ 0x06 : PciStatus<'a>,
        revision_id         @ 0x08 : PciRegisterRo<'a, u8>,
        class_code          @ 0x09 : PciClassCode<'a>,
        cache_line_size     @ 0x0c : PciRegisterRw<'a, u8>,
        latency_timer       @ 0x0d : PciRegisterRo<'a, u8>,
        header_type         @ 0x0e : PciHeaderType<'a>,
        bist                @ 0x0f : PciBist<'a>,
        cardbus_cis_pointer @ 0x28 : PciRegisterRo<'a, u32>,
        subsystem_vendor_id @ 0x2c : PciRegisterRo<'a, u16>,
        subsystem_id        @ 0x2e : PciRegisterRo<'a, u16>,
        interrupt_line      @ 0x3c : PciRegisterRw<'a, u8>,
        interrupt_pin       @ 0x3d : PciRegisterRo<'a, u8>,
        min_gnt             @ 0x3e : PciRegisterRo<'a, u8>,
        max_lat             @ 0x3f : PciRegisterRo<'a, u8>,
    }
}

impl<'a> PciConfig<'a> {
    /// Returns a thing that lets you access the PCI Capabilities.
    ///
    /// Calling this will (re)scan all Capabilities, which is why it can fail.
    pub fn capabilities(&self) -> io::Result<PciCapabilities<'a>> {
        PciCapabilities::backed_by(*self)
    }

    /// Returns a thing that lets you access the PCI Extended Capabilities.
    ///
    /// Calling this will (re)scan all Extended Capabilities, which is why it can fail.
    pub fn extended_capabilities(&self) -> io::Result<PciExtendedCapabilities<'a>> {
        PciExtendedCapabilities::backed_by(*self)
    }
}

// 7.5.1.1.3 Command Register

pci_bit_field! {
    pub struct PciCommand<'a> : RW u16 {
        io_space_enable                       @      0 : RW,
        memory_space_enable                   @      1 : RW,
        bus_master_enable                     @      2 : RW,
        special_cycle_enable                  @      3 : RO,
        memory_write_and_invalidate           @      4 : RO,
        vga_palette_snoop                     @      5 : RO,
        parity_error_response                 @      6 : RW,
        idsel_stepping_wait_cycle_control     @      7 : RO,
        serr_enable                           @      8 : RW,
        fast_back_to_back_transactions_enable @      9 : RO,
        interrupt_disable                     @     10 : RW1C,
        __                                    @ 11--15 : RsvdP,
    }
}

// 7.5.1.1.4 Status Register

pci_bit_field! {
    pub struct PciStatus<'a> : RW u16 {
        immediate_readiness                    @     0 : RO,
        __                                     @  1--2 : RsvdZ,
        interrupt_status                       @     3 : RO,
        capabilities_list                      @     4 : RO,
        mhz_66_capable                         @     5 : RO,
        __                                     @     6 : RsvdZ,
        fast_back_to_back_transactions_capable @     7 : RO,
        master_data_parity_error               @     8 : RW1C,
        devsel_timing                          @ 9--10 : RO u8,
        signaled_target_abort                  @    11 : RW1C,
        received_target_abort                  @    12 : RW1C,
        received_master_abort                  @    13 : RW1C,
        signaled_system_error                  @    14 : RW1C,
        detected_parity_error                  @    15 : RW1C,
    }
}

// 7.5.1.1.6 Class Code Register

pci_struct! {
    pub struct PciClassCode<'a> : 0x03 {
        base_class_code       @ 0x00 : PciRegisterRo<'a, u8>,
        sub_class_code        @ 0x01 : PciRegisterRo<'a, u8>,
        programming_interface @ 0x02 : PciRegisterRo<'a, u8>,
    }
}

// 7.5.1.1.9 Header Type Register

pci_bit_field! {
    pub struct PciHeaderType<'a> : RO u8 {
        header_layout         @ 0--6 : RO u8,
        multi_function_device @    7 : RO,
    }
}

// 7.5.1.1.10 BIST Register

pci_bit_field! {
    pub struct PciBist<'a> : RW u8 {
        completion_code @ 0--3 : RO u8,
        __              @ 4--5 : RsvdP,
        start_bist      @    6 : RW,
        bist_capable    @    7 : RO,
    }
}

/* ---------------------------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use crate::backends::mock::MockPciDevice;
    use crate::config::caps::Capability;
    use crate::config::ext_caps::ExtendedCapability;
    use crate::device::PciDevice;

    #[test]
    fn test_lifetimes() {
        let device: &dyn PciDevice = &MockPciDevice;

        let value_1 = device.config().command().io_space_enable();
        let value_2 = device
            .config()
            .capabilities()
            .unwrap()
            .iter()
            .next()
            .unwrap()
            .header()
            .capability_id();

        value_1.read().unwrap();
        value_2.read().unwrap();
        value_1.read().unwrap();
    }

    #[test]
    fn test_capabilities() {
        let device: &dyn PciDevice = &MockPciDevice;

        let cap_ids: Vec<_> = device
            .config()
            .capabilities()
            .unwrap()
            .iter()
            .map(|cap| cap.header().capability_id().read().unwrap())
            .collect();

        assert_eq!(cap_ids, vec![0x01, 0x05, 0x10, 0x11]);
    }

    #[test]
    fn test_extended_capabilities() {
        let device: &dyn PciDevice = &MockPciDevice;

        let ext_cap_ids: Vec<_> = device
            .config()
            .extended_capabilities()
            .unwrap()
            .iter()
            .map(|cap| cap.header().capability_id().read().unwrap())
            .collect();

        assert_eq!(
            ext_cap_ids,
            vec![0x0001, 0x0003, 0x0004, 0x0019, 0x0018, 0x001e]
        );
    }
}

/* ---------------------------------------------------------------------------------------------- */
