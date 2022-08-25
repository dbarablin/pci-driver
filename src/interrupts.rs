// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use std::io;
use std::os::unix::io::RawFd;

use crate::device::PciDeviceInternal;

/* ---------------------------------------------------------------------------------------------- */

/// Gives you control over a PCI device's interrupt mechanisms: INTx, MSI, and MSI-X.
///
/// Each device may only support a subset of these mechanisms. The [`PciInterruptMechanism::max`]
/// method returns 0 for unsupported mechanisms.
pub struct PciInterrupts<'a> {
    pub(crate) device: &'a dyn PciDeviceInternal,
}

impl PciInterrupts<'_> {
    /// Returns a thing that gives you control over a PCI device's INTx interrupts.
    pub fn intx(&self) -> PciInterruptMechanism {
        PciInterruptMechanism {
            device_internal: self.device,
            kind: PciInterruptKind::Intx,
        }
    }

    /// Returns a thing that gives you control over a PCI device's MSI interrupts.
    pub fn msi(&self) -> PciInterruptMechanism {
        PciInterruptMechanism {
            device_internal: self.device,
            kind: PciInterruptKind::Msi,
        }
    }

    /// Returns a thing that gives you control over a PCI device's MSI-X interrupts.
    pub fn msi_x(&self) -> PciInterruptMechanism {
        PciInterruptMechanism {
            device_internal: self.device,
            kind: PciInterruptKind::MsiX,
        }
    }
}

/* ---------------------------------------------------------------------------------------------- */

/// Gives you control over a PCI device's specific interrupt mechanism, which may be INTx, MSI, or
/// MSI-X.
pub struct PciInterruptMechanism<'a> {
    pub(crate) device_internal: &'a dyn PciDeviceInternal,
    pub(crate) kind: PciInterruptKind,
}

impl PciInterruptMechanism<'_> {
    /// Maximum number of vectors that may be enabled for this particular interrupt mechanism.
    pub fn max(&self) -> usize {
        self.device_internal.interrupts_max(self.kind)
    }

    /// Enables vectors `0` through `eventfds.len() - 1` of this particular interrupt mechanism.
    ///
    /// Fails if `eventfds.len() > self.max()`.
    pub fn enable(&self, eventfds: &[RawFd]) -> io::Result<()> {
        self.device_internal.interrupts_enable(self.kind, eventfds)
    }

    /// Disables all enabled vectors of this particular interrupt mechanism.
    pub fn disable(&self) -> io::Result<()> {
        self.device_internal.interrupts_disable(self.kind)
    }

    // TODO: Add interrupt masking? VFIO only supports masking INTx interrupts, though.
}

/* ---------------------------------------------------------------------------------------------- */

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PciInterruptKind {
    Intx = 0,
    Msi = 1,
    MsiX = 2,
}

/* ---------------------------------------------------------------------------------------------- */
