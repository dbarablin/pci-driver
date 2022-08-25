// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

use num_traits::{PrimInt, Unsigned};
use std::convert::TryInto;
use std::fmt::{self, Binary, Debug, LowerHex, UpperHex};
use std::io::{self, ErrorKind};
use std::marker::PhantomData;

use crate::regions::{AsPciSubregion, BackedByPciSubregion, PciRegion};

/* ---------------------------------------------------------------------------------------------- */

use private::Sealed;
mod private {
    /// Like [`crate::device::private::Sealed`].
    pub trait Sealed {}
}

/// Trait for types that represent the value of a PCI field or register.
///
/// This is implemented for [`u8`], [`u16`], and [`u32`].
///
/// This trait is _sealed_, and thus cannot be implemented by users of the crate.
pub trait PciRegisterValue:
    PrimInt + Unsigned + Debug + LowerHex + UpperHex + Binary + Sealed
{
    /// Delegates to [`PciRegion::read_u8`], [`PciRegion::read_le_u16`], or
    /// [`PciRegion::read_le_u32`].
    fn read(region: &dyn PciRegion, offset: u64) -> io::Result<Self>;

    /// Delegates to [`PciRegion::write_u8`], [`PciRegion::write_le_u16`], or
    /// [`PciRegion::write_le_u32`].
    fn write(self, region: &dyn PciRegion, offset: u64) -> io::Result<()>;
}

impl Sealed for u8 {}
impl PciRegisterValue for u8 {
    fn read(region: &dyn PciRegion, offset: u64) -> io::Result<Self> {
        region.read_u8(offset)
    }

    fn write(self, region: &dyn PciRegion, offset: u64) -> io::Result<()> {
        region.write_u8(offset, self)
    }
}

impl Sealed for u16 {}
impl PciRegisterValue for u16 {
    fn read(region: &dyn PciRegion, offset: u64) -> io::Result<Self> {
        region.read_le_u16(offset)
    }

    fn write(self, region: &dyn PciRegion, offset: u64) -> io::Result<()> {
        region.write_le_u16(offset, self)
    }
}

impl Sealed for u32 {}
impl PciRegisterValue for u32 {
    fn read(region: &dyn PciRegion, offset: u64) -> io::Result<Self> {
        region.read_le_u32(offset)
    }

    fn write(self, region: &dyn PciRegion, offset: u64) -> io::Result<()> {
        region.write_le_u32(offset, self)
    }
}

fn print_debug_hex<T: Debug + LowerHex>(
    value: io::Result<T>,
    f: &mut fmt::Formatter,
) -> fmt::Result {
    if let Ok(v) = value {
        // Avoid newlines around short values, and print in hex since that is usually more useful.
        write!(f, "Ok({:#x})", v)
    } else {
        Debug::fmt(&value, f)
    }
}

fn print_debug_bool(value: io::Result<bool>, f: &mut fmt::Formatter) -> fmt::Result {
    if let Ok(v) = value {
        // Avoid newlines around short values.
        write!(f, "Ok({})", v)
    } else {
        Debug::fmt(&value, f)
    }
}

/* ---------------------------------------------------------------------------------------------- */

// READ-ONLY REGISTERS

/// An 8-bit, 16-bit, or 32-bit PCI register that is read-only.
#[derive(Clone, Copy)]
pub struct PciRegisterRo<'a, T: PciRegisterValue> {
    region: &'a dyn PciRegion,
    offset: u64,
    phantom: PhantomData<T>,
}

impl<'a, T: PciRegisterValue> PciRegisterRo<'a, T> {
    /// Read the field.
    pub fn read(&self) -> io::Result<T> {
        T::read(self.region, self.offset)
    }
}

impl<'a, T: PciRegisterValue> BackedByPciSubregion<'a> for PciRegisterRo<'a, T> {
    fn backed_by(as_subregion: impl AsPciSubregion<'a>) -> Self {
        let subregion = as_subregion.as_subregion();
        PciRegisterRo {
            region: subregion.underlying_region(),
            offset: subregion.offset_in_underlying_region(),
            phantom: PhantomData,
        }
    }
}

impl<T: PciRegisterValue> Debug for PciRegisterRo<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        print_debug_hex(self.read(), f)
    }
}

// READ-WRITE REGISTERS

/// An 8-bit, 16-bit, or 32-bit PCI register that is read-write.
#[derive(Clone, Copy)]
pub struct PciRegisterRw<'a, T: PciRegisterValue> {
    region: &'a dyn PciRegion,
    offset: u64,
    phantom: PhantomData<T>,
}

impl<'a, T: PciRegisterValue> PciRegisterRw<'a, T> {
    /// Read the field.
    pub fn read(&self) -> io::Result<T> {
        T::read(self.region, self.offset)
    }

    /// Write the field.
    pub fn write(&self, value: T) -> io::Result<()> {
        value.write(self.region, self.offset)
    }
}

impl<'a, T: PciRegisterValue> BackedByPciSubregion<'a> for PciRegisterRw<'a, T> {
    fn backed_by(as_subregion: impl AsPciSubregion<'a>) -> Self {
        let subregion = as_subregion.as_subregion();
        PciRegisterRw {
            region: subregion.underlying_region(),
            offset: subregion.offset_in_underlying_region(),
            phantom: PhantomData,
        }
    }
}

impl<T: PciRegisterValue> Debug for PciRegisterRw<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        print_debug_hex(self.read(), f)
    }
}

/* ---------------------------------------------------------------------------------------------- */

// BIT FIELD TRAITS

/// A PCI register of type that is a bit field and may be read.
pub trait PciBitFieldReadable: Debug {
    /// The type of the register's value.
    type Type: PciRegisterValue;

    /// Read the entire bit field at once.
    fn read(&self) -> io::Result<Self::Type>;
}

/// A PCI register of type that is a bit field and may be written.
pub trait PciBitFieldWriteable: PciBitFieldReadable {
    /// Write mask for the register.
    ///
    /// One wants to alter the value of only some bits of the register. However, the register may
    /// only be read/written in its entirety. Further, some of the bits in the register may need to
    /// be written with the value 0 in order not to change their actual state in the device. This
    /// write mask has those bits set to 0, and the others set to 1. Thus, to alter the value of
    /// only some bits of the register, this must be done:
    ///
    /// - Read the register into `x`;
    /// - Apply the write mask to `x`, _i.e._, `let y = x & WRITE_MASK`;
    /// - Modify `y` as needed;
    /// - Write the resulting value into the register.
    ///
    /// Of course, you most likely can just use the member functions that this type provides to
    /// manipulate individual parts of the register, but sometimes you may need to do these steps by
    /// yourself, _e.g._, to atomically alter several parts of the register at once.
    const WRITE_MASK: Self::Type;

    /// Write the entire bit field at once.
    fn write(&self, value: Self::Type) -> io::Result<()>;
}

// TODO: Probably make these below use a PciSubregion, so they can check if they are reading/writing
// past the end of the region.

// READ-ONLY BIT SEQUENCES

/// A read-only sequence of bits that is part of a PCI register.
#[derive(Clone, Copy)]
pub struct PciBitsReadOnly<'a, T, U>
where
    T: PciRegisterValue + TryInto<U>,
    T::Error: Debug,
    U: PciRegisterValue,
{
    region: &'a dyn PciRegion,
    offset: u64,
    mask: T,
    shift: u8,
    phantom: PhantomData<U>,
}

impl<'a, T, U> PciBitsReadOnly<'a, T, U>
where
    T: PciRegisterValue + TryInto<U>,
    T::Error: Debug,
    U: PciRegisterValue,
{
    pub fn backed_by(region: &'a dyn PciRegion, offset: u64, mask: T, shift: u8) -> Self {
        PciBitsReadOnly {
            region,
            offset,
            mask,
            shift,
            phantom: PhantomData,
        }
    }

    /// Read the bit sequence.
    ///
    /// This reads the entire register and then masks and shifts the part we're interested in.
    pub fn read(&self) -> io::Result<U> {
        let value = (T::read(self.region, self.offset)? & self.mask) >> self.shift.into();
        // TODO: Ensure at compile time that this can't fail.
        Ok(value.try_into().unwrap())
    }
}

impl<T, U> Debug for PciBitsReadOnly<'_, T, U>
where
    T: PciRegisterValue + TryInto<U>,
    T::Error: Debug,
    U: PciRegisterValue,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        print_debug_hex(self.read(), f)
    }
}

// READ-WRITE BIT SEQUENCES

/// A read-write sequence of bits that is part of a PCI register.
#[derive(Clone, Copy)]
pub struct PciBitsReadWrite<'a, T, U>
where
    T: PciRegisterValue + TryInto<U>,
    T::Error: Debug,
    U: PciRegisterValue + Into<T>,
{
    region: &'a dyn PciRegion,
    offset: u64,
    mask: T,
    shift: u8,
    write_mask: T, // must 'and' with this after reading but before altering the bits
    phantom: PhantomData<U>,
}

impl<'a, T, U> PciBitsReadWrite<'a, T, U>
where
    T: PciRegisterValue + TryInto<U>,
    T::Error: Debug,
    U: PciRegisterValue + Into<T>,
{
    pub fn backed_by(
        region: &'a dyn PciRegion,
        offset: u64,
        mask: T,
        shift: u8,
        write_mask: T,
    ) -> Self {
        PciBitsReadWrite {
            region,
            offset,
            mask,
            shift,
            write_mask,
            phantom: PhantomData,
        }
    }

    /// Read the bit sequence.
    ///
    /// This reads the entire register and then masks and shifts the part we're interested in.
    pub fn read(&self) -> io::Result<U> {
        let value = (T::read(self.region, self.offset)? & self.mask) >> self.shift.into();
        // TODO: Ensure at compile time that this can't fail.
        Ok(value.try_into().unwrap())
    }

    /// Write the bit sequence.
    ///
    /// This shifts the value and makes sure to not affect any other bits in the underlying
    /// register.
    pub fn write(&self, value: U) -> io::Result<()> {
        let shifted = value.into() << self.shift.into();

        if shifted >> self.shift.into() != value.into() || shifted & !self.mask != T::zero() {
            return Err(io::Error::new(ErrorKind::InvalidInput, "Value is too big"));
        }

        let to_write = (T::read(self.region, self.offset)? & self.write_mask) | shifted;
        to_write.write(self.region, self.offset)
    }
}

impl<T, U> Debug for PciBitsReadWrite<'_, T, U>
where
    T: PciRegisterValue + TryInto<U>,
    T::Error: Debug,
    U: PciRegisterValue + Into<T>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        print_debug_hex(self.read(), f)
    }
}

// READ-ONLY INDIVIDUAL BITS

/// A read-only single bit that is part of a PCI register.
#[derive(Clone, Copy)]
pub struct PciBitReadOnly<'a, T: PciRegisterValue> {
    region: &'a dyn PciRegion,
    offset: u64,
    mask: T,
}

impl<'a, T: PciRegisterValue> PciBitReadOnly<'a, T> {
    pub fn backed_by(region: &'a dyn PciRegion, offset: u64, mask: T) -> Self {
        PciBitReadOnly {
            region,
            offset,
            mask,
        }
    }

    /// Read the bit.
    ///
    /// This reads the entire register and then checks the bit we're interested in.
    pub fn read(&self) -> io::Result<bool> {
        Ok(T::read(self.region, self.offset)? & self.mask != T::zero())
    }
}

impl<T: PciRegisterValue> Debug for PciBitReadOnly<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        print_debug_bool(self.read(), f)
    }
}

// READ-WRITE INDIVIDUAL BITS

/// A read-write single bit that is part of a PCI register.
#[derive(Clone, Copy)]
pub struct PciBitReadWrite<'a, T: PciRegisterValue> {
    region: &'a dyn PciRegion,
    offset: u64,
    mask: T,
    write_mask: T, // must 'and' with this after reading but before altering the bits
}

impl<'a, T: PciRegisterValue> PciBitReadWrite<'a, T> {
    pub fn backed_by(region: &'a dyn PciRegion, offset: u64, mask: T, write_mask: T) -> Self {
        PciBitReadWrite {
            region,
            offset,
            mask,
            write_mask,
        }
    }

    /// Read the bit.
    ///
    /// This reads the entire register and then checks the bit we're interested in.
    pub fn read(&self) -> io::Result<bool> {
        Ok(T::read(self.region, self.offset)? & self.mask != T::zero())
    }

    /// Write the bit.
    ///
    /// This makes sure to not affect any other bits in the underlying register.
    pub fn write(&self, value: bool) -> io::Result<()> {
        let old = T::read(self.region, self.offset)? & self.write_mask;

        let new = if value {
            old | self.mask
        } else {
            old & !self.mask
        };

        new.write(self.region, self.offset)
    }
}

impl<T: PciRegisterValue> Debug for PciBitReadWrite<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        print_debug_bool(self.read(), f)
    }
}

// READ-CLEAR INDIVIDUAL BITS

/// A read-clear (RW1C in the spec) single bit that is part of a PCI register.
#[derive(Clone, Copy)]
pub struct PciBitReadClear<'a, T: PciRegisterValue> {
    rw: PciBitReadWrite<'a, T>,
}

impl<'a, T: PciRegisterValue> PciBitReadClear<'a, T> {
    pub fn backed_by(region: &'a dyn PciRegion, offset: u64, mask: T, write_mask: T) -> Self {
        PciBitReadClear {
            rw: PciBitReadWrite {
                region,
                offset,
                mask,
                write_mask,
            },
        }
    }

    /// Read the bit.
    ///
    /// This reads the entire register and then checks the bit we're interested in.
    pub fn read(&self) -> io::Result<bool> {
        self.rw.read()
    }

    /// Clear the bit (_i.e._, set it to 0).
    ///
    /// This makes sure to not affect any other bits in the underlying register.
    pub fn clear(&self) -> io::Result<()> {
        self.rw.write(true)
    }
}

impl<T: PciRegisterValue> Debug for PciBitReadClear<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        print_debug_bool(self.read(), f)
    }
}

/* ---------------------------------------------------------------------------------------------- */
