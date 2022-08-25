// SPDX-License-Identifier: MIT OR Apache-2.0

//! Types representing PCI regions (config space, BARs, etc.) and other related types.
//!
//! ## Base machinery
//!
//! - [`trait PciRegion`](PciRegion). Sealed.
//!   - `&'a dyn PciRegion` implements `AsPciSubregion<'a>`, for all `'a`.
//!
//! - [`struct PciSubregion<'a>`](PciSubregion).
//!   - `PciSubregion<'a>` implements `PciRegion`, for all `'a`.
//!   - `PciSubregion<'a>` implements `AsPciSubregion<'a>`, for all `'a`.
//!
//! - [`trait AsPciSubregion<'a>`](AsPciSubregion). Unlike `PciRegion`, this trait is not sealed.
//!   - If `T` implements `AsPciSubregion<'a>`, then `&'b T` implements `AsPciSubregion<'a>`, for
//!     all `'a`, `'b`, `T`.
//!   - If `T` implements `AsPciSubregion<'a> + Debug + Send + Sync`, then `T` implements
//!     `PciRegion`, for all `'a`, `T`.
//!
//! ## `PciRegion` implementations
//!
//! - [`struct PciMemoryRegion<'a>`](PciMemoryRegion). A region backed by a `&'a [u8]`, `&'a mut
//!   [u8]`, or raw memory.
//!   - `PciMemoryRegion<'a>` implements `PciRegion`, for all `'a`.
//!   - `&'a PciMemoryRegion<'b>` implements `AsPciSubregion<'a>`, for all `'a`, `'b`.

/* ---------------------------------------------------------------------------------------------- */

use std::fmt::Debug;
use std::io::{self, ErrorKind};
use std::marker::PhantomData;
use std::mem;
use std::ops::{Bound, Range, RangeBounds};

/* ---------------------------------------------------------------------------------------------- */

/// Describes which operations may be performed on some piece of memory or other data region.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permissions {
    /// Only reading is allowed.
    Read,
    /// Only writing is allowed.
    Write,
    /// Both reading and writing are allowed.
    ReadWrite,
}

impl Permissions {
    pub fn new(can_read: bool, can_write: bool) -> Option<Permissions> {
        match (can_read, can_write) {
            (false, false) => None,
            (true, false) => Some(Permissions::Read),
            (false, true) => Some(Permissions::Write),
            (true, true) => Some(Permissions::ReadWrite),
        }
    }

    pub fn can_read(&self) -> bool {
        match self {
            Permissions::Read => true,
            Permissions::Write => false,
            Permissions::ReadWrite => true,
        }
    }

    pub fn can_write(&self) -> bool {
        match self {
            Permissions::Read => false,
            Permissions::Write => true,
            Permissions::ReadWrite => true,
        }
    }
}

/* ---------------------------------------------------------------------------------------------- */

pub(crate) use private::Sealed;
mod private {
    /// Like [`crate::device::private::Sealed`]. We can't use that same trait here because users
    /// would be able to indirectly implement it for their own types by implementing
    /// `AsPciSubregion`, so we define another one with the same name.
    pub trait Sealed {}
}

/// A region of PCI Configuration Space, or a BAR, or the Expansion ROM, or VGA Space, or some other
/// device region, or maybe something else, as long it is safe to read and write to it concurrently
/// with no data races.
///
/// The region does not necessarily have RAM semantics, i.e., values can change suddenly, writes
/// might not actually write what is being written, reads can have side effects, etc.
///
/// Offsets are [`u64`], not [`usize`], so you can operate on 64-bit `PciRegion`s even when
/// compiling for 32-bit.
///
/// This trait is _sealed_ for forward-compatibility reasons, and thus cannot be implemented by
/// users of the crate.
#[allow(clippy::len_without_is_empty)]
pub trait PciRegion: Debug + Send + Sync + Sealed {
    /// The length of the region in bytes.
    fn len(&self) -> u64;

    /// Whether the region may be read, written, or both.
    fn permissions(&self) -> Permissions;

    /// Returns a `const` pointer to the beginning of the `PciRegion`.
    ///
    /// If the region is not mapped into memory, this returns `None`.
    fn as_ptr(&self) -> Option<*const u8>;

    /// Returns a `mut` pointer to the beginning of the `PciRegion`.
    ///
    /// If the region is not writeable or not mapped into memory, this returns `None`.
    fn as_mut_ptr(&self) -> Option<*mut u8>;

    /// Read from a contiguous range of the region into a byte buffer.
    ///
    /// There is no guarantee that the access will be atomic in any sense, or terribly efficient.
    fn read_bytes(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()>;

    /// Read an [`u8`] at the given byte offset from the beginning of the `PciRegion`.
    ///
    /// This will fail if `offset + 1 > self.len()`.
    fn read_u8(&self, offset: u64) -> io::Result<u8>;

    /// Write an [`u8`] at the given byte offset from the beginning of the `PciRegion`.
    ///
    /// This will fail if `offset + 1 > self.len()`.
    fn write_u8(&self, offset: u64, value: u8) -> io::Result<()>;

    /// Read a little-endian [`u16`] at the given byte offset from the beginning of the `PciRegion`.
    ///
    /// The read value will be converted from little-endian to the native endianness before being
    /// returned.
    ///
    /// This will fail if `offset + 2 > self.len()`, or if the region requires aligned accesses and
    /// `offset` is not 2-byte aligned.
    fn read_le_u16(&self, offset: u64) -> io::Result<u16>;

    /// Write a little-endian [`u16`] at the given byte offset from the beginning of the
    /// `PciRegion`.
    ///
    /// The value will be converted from the native endianness to little-endian before being
    /// written.
    ///
    /// This will fail if `offset + 2 > self.len()`, or if the region requires aligned accesses and
    /// `offset` is not 2-byte aligned.
    fn write_le_u16(&self, offset: u64, value: u16) -> io::Result<()>;

    /// Read a little-endian [`u32`] at the given byte offset from the beginning of the `PciRegion`.
    ///
    /// The read value will be converted from little-endian to the native endianness before being
    /// returned.
    ///
    /// This will fail if `offset + 4 > self.len()`, or if the region requires aligned accesses and
    /// `offset` is not 4-byte aligned.
    fn read_le_u32(&self, offset: u64) -> io::Result<u32>;

    /// Write a little-endian [`u32`] at the given byte offset from the beginning of the
    /// `PciRegion`.
    ///
    /// The value will be converted from the native endianness to little-endian before being
    /// written.
    ///
    /// This will fail if `offset + 4 > self.len()`, or if the region requires aligned accesses and
    /// `offset` is not 4-byte aligned.
    fn write_le_u32(&self, offset: u64, value: u32) -> io::Result<()>;
}

/* ---------------------------------------------------------------------------------------------- */

/// A contiguous part of a [`PciRegion`], which is itself also a `PciRegion`.
///
/// Simply redirects accesses to the underlying `PciRegion`, offset by the `PciSubregion`'s offset.
/// Also makes sure those accesses don't exceed the `PciSubregion`'s end (offset + length).
///
/// Create instances of this by calling [`AsPciSubregion::subregion`] on anything that implements
/// it, for instance a [`&dyn PciRegion`](PciRegion).
#[derive(Clone, Copy, Debug)]
pub struct PciSubregion<'a> {
    region: &'a dyn PciRegion,
    offset: u64,
    length: u64,
}

impl<'a> PciSubregion<'a> {
    pub fn underlying_region(&self) -> &'a dyn PciRegion {
        self.region
    }

    pub fn offset_in_underlying_region(&self) -> u64 {
        self.offset
    }

    fn validate_access(&self, offset: u64, len: usize) -> io::Result<()> {
        let len = len as u64;

        if offset + len > self.length {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "Tried to access region range [{:#x}, {:#x}), must be within [0x0, {:#x})",
                    offset,
                    offset + len,
                    self.length
                ),
            ));
        }

        Ok(())
    }
}

/* ---------------------------------------------------------------------------------------------- */

/// For when it is possible to obtain a [`PciSubregion`] representation of a value cheaply.
///
/// Also provides a handy [`AsPciSubregion::subregion`] method with a default implementation.
pub trait AsPciSubregion<'a> {
    /// Returns a [`PciSubregion`] corresponding to `self`.
    fn as_subregion(&self) -> PciSubregion<'a>;

    /// Returns a [`PciSubregion`] corresponding to a range of `self`.
    fn subregion(&self, range: impl RangeBounds<u64>) -> PciSubregion<'a> {
        let subregion = Self::as_subregion(self);
        let range = clamp_range(range, subregion.len());

        PciSubregion {
            region: subregion.underlying_region(),
            offset: subregion.offset_in_underlying_region() + range.start,
            length: range.end - range.start,
        }
    }
}

// If a `T` is `AsPciSubregion<'a>`, then any `&T` is also.
impl<'a, 'b, T> AsPciSubregion<'a> for &'b T
where
    T: AsPciSubregion<'a>,
{
    fn as_subregion(&self) -> PciSubregion<'a> {
        T::as_subregion(*self)
    }
}

impl<'a> AsPciSubregion<'a> for &'a dyn PciRegion {
    fn as_subregion(&self) -> PciSubregion<'a> {
        PciSubregion {
            region: *self,
            offset: 0,
            length: PciRegion::len(*self),
        }
    }
}

impl<'a> AsPciSubregion<'a> for PciSubregion<'a> {
    fn as_subregion(&self) -> PciSubregion<'a> {
        *self
    }
}

impl<'a, T> Sealed for T where T: AsPciSubregion<'a> + Debug + Send + Sync {}
impl<'a, T> PciRegion for T
where
    T: AsPciSubregion<'a> + Debug + Send + Sync,
{
    fn len(&self) -> u64 {
        let subregion = T::as_subregion(self);
        subregion.length
    }

    fn permissions(&self) -> Permissions {
        let subregion = T::as_subregion(self);
        subregion.region.permissions()
    }

    fn as_ptr(&self) -> Option<*const u8> {
        let subregion = T::as_subregion(self);
        let ptr = subregion.region.as_ptr()?;
        // TODO: Can any of this overflow?
        Some(unsafe { ptr.add(subregion.offset as usize) })
    }

    fn as_mut_ptr(&self) -> Option<*mut u8> {
        let subregion = T::as_subregion(self);
        let ptr = subregion.region.as_mut_ptr()?;
        // TODO: Can any of this overflow?
        Some(unsafe { ptr.add(subregion.offset as usize) })
    }

    fn read_bytes(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        let subregion = T::as_subregion(self);
        subregion.validate_access(offset, buffer.len())?;
        subregion
            .region
            .read_bytes(subregion.offset + offset, buffer)
    }

    fn read_u8(&self, offset: u64) -> io::Result<u8> {
        let subregion = T::as_subregion(self);
        subregion.validate_access(offset, mem::size_of::<u8>())?;
        subregion.region.read_u8(subregion.offset + offset)
    }

    fn write_u8(&self, offset: u64, value: u8) -> io::Result<()> {
        let subregion = T::as_subregion(self);
        subregion.validate_access(offset, mem::size_of::<u8>())?;
        subregion.region.write_u8(subregion.offset + offset, value)
    }

    fn read_le_u16(&self, offset: u64) -> io::Result<u16> {
        let subregion = T::as_subregion(self);
        subregion.validate_access(offset, mem::size_of::<u16>())?;
        subregion.region.read_le_u16(subregion.offset + offset)
    }

    fn write_le_u16(&self, offset: u64, value: u16) -> io::Result<()> {
        let subregion = T::as_subregion(self);
        subregion.validate_access(offset, mem::size_of::<u16>())?;
        subregion
            .region
            .write_le_u16(subregion.offset + offset, value)
    }

    fn read_le_u32(&self, offset: u64) -> io::Result<u32> {
        let subregion = T::as_subregion(self);
        subregion.validate_access(offset, mem::size_of::<u32>())?;
        subregion.region.read_le_u32(subregion.offset + offset)
    }

    fn write_le_u32(&self, offset: u64, value: u32) -> io::Result<()> {
        let subregion = T::as_subregion(self);
        subregion.validate_access(offset, mem::size_of::<u32>())?;
        subregion
            .region
            .write_le_u32(subregion.offset + offset, value)
    }
}

/* ---------------------------------------------------------------------------------------------- */

#[derive(Clone, Copy, Debug)]
pub struct PciMemoryRegion<'a> {
    ptr: *mut u8,
    length: usize,
    permissions: Permissions,
    phantom: PhantomData<&'a ()>,
}

unsafe impl Send for PciMemoryRegion<'_> {}
unsafe impl Sync for PciMemoryRegion<'_> {}

impl PciMemoryRegion<'_> {
    pub fn new(data: &[u8]) -> PciMemoryRegion {
        PciMemoryRegion {
            ptr: data.as_ptr() as *mut _,
            length: data.len(),
            permissions: Permissions::Read,
            phantom: PhantomData,
        }
    }

    pub fn new_mut(data: &mut [u8]) -> PciMemoryRegion {
        PciMemoryRegion {
            ptr: data.as_mut_ptr(),
            length: data.len(),
            permissions: Permissions::ReadWrite,
            phantom: PhantomData,
        }
    }

    /// # Safety
    ///
    /// The returned `PciMemoryRegion` must not outlive the data.
    pub unsafe fn new_raw<'a>(
        data: *mut u8,
        length: usize,
        permissions: Permissions,
    ) -> PciMemoryRegion<'a> {
        PciMemoryRegion {
            ptr: data,
            length,
            permissions,
            phantom: PhantomData,
        }
    }

    fn get_ptr<T>(&self, offset: u64) -> io::Result<*mut T> {
        // TODO: Handle overflow.

        let size = std::mem::size_of::<T>() as u64;

        if offset + size > self.length as u64 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "Access falls outside region",
            ));
        }

        if offset % size != 0 {
            return Err(io::Error::new(ErrorKind::InvalidInput, "Unaligned access"));
        }

        Ok(unsafe { self.ptr.add(offset as usize).cast::<T>() })
    }
}

impl Sealed for PciMemoryRegion<'_> {}
impl PciRegion for PciMemoryRegion<'_> {
    fn len(&self) -> u64 {
        self.length as u64
    }

    fn permissions(&self) -> Permissions {
        self.permissions
    }

    fn as_ptr(&self) -> Option<*const u8> {
        Some(self.ptr)
    }

    fn as_mut_ptr(&self) -> Option<*mut u8> {
        Some(self.ptr)
    }

    fn read_bytes(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        let end = offset + buffer.len() as u64;

        if end > self.length as u64 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "Invalid configuration space range [{:#x}, {:#x}), must be within [0x0, {:#x})",
                    offset,
                    end,
                    self.len()
                ),
            ));
        }

        // TODO: Will these 1-byte accesses always work?

        for (off, byte) in (offset..).zip(buffer) {
            *byte = unsafe { self.get_ptr::<u8>(off)?.read_volatile() };
        }

        Ok(())
    }

    fn read_u8(&self, offset: u64) -> io::Result<u8> {
        let v = unsafe { self.get_ptr::<u8>(offset)?.read_volatile() };
        Ok(v)
    }

    fn write_u8(&self, offset: u64, value: u8) -> io::Result<()> {
        unsafe { self.get_ptr::<u8>(offset)?.write_volatile(value) };
        Ok(())
    }

    fn read_le_u16(&self, offset: u64) -> io::Result<u16> {
        let v = unsafe { self.get_ptr::<u16>(offset)?.read_volatile() };
        Ok(u16::from_le(v))
    }

    fn write_le_u16(&self, offset: u64, value: u16) -> io::Result<()> {
        unsafe { self.get_ptr::<u16>(offset)?.write_volatile(value.to_le()) };
        Ok(())
    }

    fn read_le_u32(&self, offset: u64) -> io::Result<u32> {
        let v = unsafe { self.get_ptr::<u32>(offset)?.read_volatile() };
        Ok(u32::from_le(v))
    }

    fn write_le_u32(&self, offset: u64, value: u32) -> io::Result<()> {
        unsafe { self.get_ptr::<u32>(offset)?.write_volatile(value.to_le()) };
        Ok(())
    }
}

impl<'a> AsPciSubregion<'a> for &'a PciMemoryRegion<'_> {
    fn as_subregion(&self) -> PciSubregion<'a> {
        let region: &dyn PciRegion = *self;
        <&dyn PciRegion>::as_subregion(&region)
    }
}

/* ---------------------------------------------------------------------------------------------- */

fn clamp_range(range: impl RangeBounds<u64>, max_length: u64) -> Range<u64> {
    let start = match range.start_bound() {
        Bound::Included(&b) => b,
        Bound::Excluded(&b) => b + 1,
        Bound::Unbounded => 0,
    };

    let end = match range.end_bound() {
        Bound::Included(&b) => b + 1,
        Bound::Excluded(&b) => b,
        Bound::Unbounded => max_length,
    };

    Range {
        start: start.min(max_length),
        end: end.max(start).min(max_length),
    }
}

/* ---------------------------------------------------------------------------------------------- */
