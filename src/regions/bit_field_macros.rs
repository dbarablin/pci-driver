// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

/// TODO: Document.
#[macro_export]
macro_rules! pci_bit_field {
    (
        $(
            $(#[$attr:meta])*
            $vis:vis struct $name:ident<$lifetime:lifetime> : $mode:ident $type:ty {
                $(
                    $(#[$elem_attr:meta])*
                    $elem_name:ident @ $elem_first_bit:literal$(--$elem_last_bit:literal)? :
                    $elem_mode:ident $($elem_type:ty)?
                ),* $(,)?
            }
        )*
    ) => {
        $(
            $(#[$attr])*
            #[derive(Clone, Copy)]
            $vis struct $name<$lifetime> {
                region: &$lifetime dyn $crate::regions::PciRegion,
                offset: u64,
            }

            impl<'a> $crate::regions::BackedByPciSubregion<'a> for $name<'a> {
                fn backed_by(as_subregion: impl $crate::regions::AsPciSubregion<'a>) -> Self {
                    let subregion = $crate::regions::AsPciSubregion::as_subregion(&as_subregion);
                    $name {
                        region: subregion.underlying_region(),
                        offset: subregion.offset_in_underlying_region(),
                    }
                }
            }

            impl<'a> $crate::regions::AsPciSubregion<'a> for $name<'a> {
                fn as_subregion(&self) -> $crate::regions::PciSubregion<'a> {
                    self.region
                        .subregion(self.offset..self.offset + ::std::mem::size_of::<$type>() as u64)
                }
            }

            impl $crate::regions::structured::PciBitFieldReadable for $name<'_> {
                type Type = $type;

                fn read(&self) -> ::std::io::Result<$type> {
                    $crate::regions::structured::PciRegisterValue::read(
                        self.region,
                        self.offset,
                    )
                }
            }

            impl ::std::fmt::Debug for $name<'_> {
                fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                    let mut debug_struct = f.debug_struct(::std::stringify!($name));
                    $(
                        $crate::_pci_bit_field_debug_elem!(
                            self, debug_struct, $elem_name : $elem_mode $($elem_type)?
                        );
                    )*
                    debug_struct.finish()
                }
            }

            impl<$lifetime> $name<$lifetime> {
                $(
                    $crate::_pci_bit_field_elem! {
                        $lifetime $type :
                        $(#[$elem_attr])*
                        $elem_name @ $elem_first_bit$(--$elem_last_bit)? :
                        $elem_mode $($elem_type)?
                    }
                )*
            }

            $crate::_pci_bit_field_impl_writeable_part! {
                impl $name<$lifetime> : $mode $type {
                    $(
                        $(#[$elem_attr])*
                        $elem_name @ $elem_first_bit$(--$elem_last_bit)? :
                        $elem_mode $($elem_type)?
                    ),*
                }
            }
        )*
    };
}

/// This macro is __internal__. It should __not__ be used outside of the `pci-driver` crate.
#[doc(hidden)]
#[macro_export]
macro_rules! _pci_bit_field_debug_elem {
    ( $self:ident, $debug_struct:ident, $elem_name:ident : RsvdP ) => {};
    ( $self:ident, $debug_struct:ident, $elem_name:ident : RsvdZ ) => {};
    ( $self:ident, $debug_struct:ident, $elem_name:ident : $elem_mode:ident $($elem_type:ty)? ) => {
        $debug_struct.field(::std::stringify!($elem_name), &$self.$elem_name())
    };
}

/// This macro is __internal__. It should __not__ be used outside of the `pci-driver` crate.
#[doc(hidden)]
#[macro_export]
macro_rules! _pci_bit_field_impl_writeable_part {
    (
        impl $name:ident<$lifetime:lifetime> : RO $type:ty {
            $(
                $(#[$elem_attr:meta])*
                $elem_name:ident @ $elem_first_bit:literal$(--$elem_last_bit:literal)? :
                $elem_mode:ident $($elem_type:ty)?
            ),* $(,)?
        }
    ) => {};

    (
        impl $name:ident<$lifetime:lifetime> : RW $type:ty {
            $(
                $(#[$elem_attr:meta])*
                $elem_name:ident @ $elem_first_bit:literal$(--$elem_last_bit:literal)? :
                $elem_mode:ident $($elem_type:ty)?
            ),* $(,)?
        }
    ) => {
        impl $crate::regions::structured::PciBitFieldWriteable for $name<'_> {
            const WRITE_MASK: $type = $crate::_pci_bit_field_write_mask!(
                $type,
                $(
                    @ $elem_first_bit$(--$elem_last_bit)? :
                    $elem_mode $($elem_type)?
                ),*
            );

            fn write(&self, value: $type) -> ::std::io::Result<()> {
                $crate::regions::structured::PciRegisterValue::write(
                    value,
                    self.region,
                    self.offset,
                )
            }
        }
    }
}

/// This macro is __internal__. It should __not__ be used outside of the `pci-driver` crate.
#[doc(hidden)]
#[macro_export]
macro_rules! _pci_bit_field_elem {
    (
        $lifetime:lifetime $field_type:ty :
        $(#[$elem_attr:meta])*
        $elem_name:ident @ $elem_bit:literal : RO
    ) => {
        $(#[$elem_attr])*
        pub fn $elem_name(&self) -> $crate::regions::structured::PciBitReadOnly<$lifetime, $field_type> {
            $crate::regions::structured::PciBitReadOnly::backed_by(
                self.region,
                self.offset,
                1 << $elem_bit, // mask
            )
        }
    };

    (
        $lifetime:lifetime $field_type:ty :
        $(#[$elem_attr:meta])*
        $elem_name:ident @ $elem_first_bit:literal--$elem_last_bit:literal : RO $elem_type:ty
    ) => {
        $(#[$elem_attr])*
        pub fn $elem_name(&self) -> $crate::regions::structured::PciBitsReadOnly<$lifetime, $field_type, $elem_type> {
            const MASK: $field_type = $crate::_bit_range!($field_type, $elem_first_bit, $elem_last_bit);
            $crate::regions::structured::PciBitsReadOnly::backed_by(
                self.region,
                self.offset,
                MASK,
                $elem_first_bit, // shift
            )
        }
    };

    (
        $lifetime:lifetime $field_type:ty :
        $(#[$elem_attr:meta])*
        $elem_name:ident @ $elem_bit:literal : RW
    ) => {
        $(#[$elem_attr])*
        pub fn $elem_name(&self) -> $crate::regions::structured::PciBitReadWrite<$lifetime, $field_type> {
            $crate::regions::structured::PciBitReadWrite::backed_by(
                self.region,
                self.offset,
                1 << $elem_bit, // mask
                <Self as $crate::regions::structured::PciBitFieldWriteable>::WRITE_MASK,
            )
        }
    };

    (
        $lifetime:lifetime $field_type:ty :
        $(#[$elem_attr:meta])*
        $elem_name:ident @ $elem_first_bit:literal--$elem_last_bit:literal : RW $elem_type:ty
    ) => {
        $(#[$elem_attr])*
        pub fn $elem_name(&self) -> $crate::regions::structured::PciBitsReadWrite<$lifetime, $field_type, $elem_type> {
            const MASK: $field_type = $crate::_bit_range!($field_type, $elem_first_bit, $elem_last_bit);
            $crate::regions::structured::PciBitsReadWrite::backed_by(
                self.region,
                self.offset,
                MASK,
                $elem_first_bit, // shift
                <Self as $crate::regions::structured::PciBitFieldWriteable>::WRITE_MASK
            )
        }
    };

    (
        $lifetime:lifetime $field_type:ty :
        $(#[$elem_attr:meta])*
        $elem_name:ident @ $elem_bit:literal : RW1C
    ) => {
        $(#[$elem_attr])*
        pub fn $elem_name(&self) -> $crate::regions::structured::PciBitReadClear<$lifetime, $field_type> {
            $crate::regions::structured::PciBitReadClear::backed_by(
                self.region,
                self.offset,
                1 << $elem_bit, // mask
                <Self as $crate::regions::structured::PciBitFieldWriteable>::WRITE_MASK,
            )
        }
    };

    (
        $lifetime:lifetime $field_type:ty :
        $elem_name:ident @ $elem_bit:literal : RsvdP
    ) => {};

    (
        $lifetime:lifetime $field_type:ty :
        $elem_name:ident @ $elem_first_bit:literal--$elem_last_bit:literal : RsvdP
    ) => {};

    (
        $lifetime:lifetime $field_type:ty :
        $elem_name:ident @ $elem_bit:literal : RsvdZ
    ) => {};

    (
        $lifetime:lifetime $field_type:ty :
        $elem_name:ident @ $elem_first_bit:literal--$elem_last_bit:literal : RsvdZ
    ) => {};
}

/// This macro is __internal__. It should __not__ be used outside of the `pci-driver` crate.
#[doc(hidden)]
#[macro_export]
macro_rules! _pci_bit_field_write_mask {
    (
        $field_type:ty,
        $(
            @ $elem_first_bit:literal$(--$elem_last_bit:literal)? :
            $elem_mode:ident $($elem_type:ty)?
        ),* $(,)?
    ) => {
        $(
            $crate::_pci_bit_field_write_mask_elem!(
                $field_type,
                @ $elem_first_bit$(--$elem_last_bit)? :
                $elem_mode $($elem_type)?
            ) &
        )* !0
    };
}

/// This macro is __internal__. It should __not__ be used outside of the `pci-driver` crate.
#[doc(hidden)]
#[macro_export]
macro_rules! _pci_bit_field_write_mask_elem {
    ($field_type:ty, @ $elem_bit:literal : RW1C) => {{
        !(1 << $elem_bit)
    }};

    ($field_type:ty, @ $elem_bit:literal : RsvdZ) => {{
        !(1 << $elem_bit)
    }};

    ($field_type:ty, @ $elem_first_bit:literal--$elem_last_bit:literal : RsvdZ) => {{
        !$crate::_bit_range!($field_type, $elem_first_bit, $elem_last_bit)
    }};

    (
        $field_type:ty,
        @ $elem_first_bit:literal$(--$elem_last_bit:literal)? :
        $elem_mode:ident $($elem_type:ty)?
    ) => {{
        !0
    }};
}

/// This macro is __internal__. It should __not__ be used outside of the `pci-driver` crate.
#[doc(hidden)]
#[macro_export]
macro_rules! _bit_range {
    ($field_type:ty, $elem_first_bit:literal, $elem_last_bit:literal) => {{
        let one: $field_type = 1;
        let mask_1 = match one.checked_shl($elem_last_bit + 1) {
            ::std::option::Option::Some(v) => v - 1,
            ::std::option::Option::None => !0,
        };
        let mask_2 = (1 << $elem_first_bit) - 1;
        mask_1 & !mask_2
    }};
}

/* ---------------------------------------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    #[test]
    fn test_pci_bit_field_write_mask() {
        assert_eq!(
            _pci_bit_field_write_mask!(
                u8,
                @    3 : RsvdZ,
                @ 6--7 : RsvdZ,
            ),
            0b_0011_0111_u8
        );
    }

    #[test]
    fn test_pci_bit_field_write_mask_elem() {
        assert_eq!(
            _pci_bit_field_write_mask_elem!(u8, @ 0 : RsvdZ),
            0b_1111_1110_u8
        );
        assert_eq!(
            _pci_bit_field_write_mask_elem!(u8, @ 3 : RsvdZ),
            0b_1111_0111_u8
        );
        assert_eq!(
            _pci_bit_field_write_mask_elem!(u8, @ 7 : RsvdZ),
            0b_0111_1111_u8
        );

        assert_eq!(
            _pci_bit_field_write_mask_elem!(u8, @ 0--1 : RsvdZ),
            0b_1111_1100_u8
        );
        assert_eq!(
            _pci_bit_field_write_mask_elem!(u8, @ 3--5 : RsvdZ),
            0b_1100_0111_u8
        );
        assert_eq!(
            _pci_bit_field_write_mask_elem!(u8, @ 6--7 : RsvdZ),
            0b_0011_1111_u8
        );
    }
}

/* ---------------------------------------------------------------------------------------------- */
