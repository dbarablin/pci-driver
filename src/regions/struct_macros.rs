// SPDX-License-Identifier: MIT OR Apache-2.0

/* ---------------------------------------------------------------------------------------------- */

/// TODO: Document.
///
/// The optional length is important mostly to make
/// [`PciRegionSnapshot`](crate::regions::PciRegionSnapshot) only copy the relevant part instead of
/// a lot more.
///
/// TODO: Validate field offsets against length.
#[macro_export]
macro_rules! pci_struct {
    (
        $(
            $(#[$attr:meta])*
            $vis:vis struct $name:ident<$lifetime:lifetime> $(: $length:literal)? {
                $(
                    $(#[$field_attr:meta])*
                    $field_name:ident @ $field_offset:literal :
                    $($field_type:ident)::+$(<$($field_generics:tt),+ $(,)?>)?
                ),* $(,)?
            }
        )*
    ) => {
        $(
            $(#[$attr])*
            #[derive(Clone, Copy)]
            $vis struct $name<$lifetime> {
                subregion: $crate::regions::PciSubregion<$lifetime>,
            }

            impl<'a> $crate::regions::BackedByPciSubregion<'a> for $name<'a> {
                fn backed_by(as_subregion: impl $crate::regions::AsPciSubregion<'a>) -> Self {
                    let subregion = $crate::regions::AsPciSubregion::subregion(&as_subregion, ..$($length)?);
                    $name { subregion }
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

/// This macro is __internal__. It should __not__ be used outside of the `pci-driver` crate.
#[doc(hidden)]
#[macro_export]
macro_rules! _pci_struct_impl {
    (
        impl $name:ident<$lifetime:lifetime> {
            $(
                $(#[$field_attr:meta])*
                $field_name:ident @ $field_offset:literal :
                $($field_type:ident)::+$(<$($field_generics:tt),+ $(,)?>)?
            ),* $(,)?
        }
    ) => {
        impl ::std::fmt::Debug for $name<'_> {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                let mut debug_struct = f.debug_struct(::std::stringify!($name));
                $( debug_struct.field(::std::stringify!($field_name), &self.$field_name()); )*
                debug_struct.finish()
            }
        }

        impl<$lifetime> $name<$lifetime> {
            $(
                $(#[$field_attr])*
                pub fn $field_name(&self) -> $($field_type)::+$(<$($field_generics),+>)? {
                    let subregion = $crate::regions::AsPciSubregion::subregion(self, $field_offset..);
                    $crate::regions::BackedByPciSubregion::backed_by(subregion)
                }
            )*
        }
    };
}

/* ---------------------------------------------------------------------------------------------- */
