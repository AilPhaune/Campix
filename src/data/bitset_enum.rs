#[macro_export]
macro_rules! debuggable_bitset_enum {
    ($t:ident, $vis:vis enum $name:ident {
        $(
            $variant:ident = $value:expr,
        )*
    }, $sname: ident) => {
        #[repr($t)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
        $vis enum $name {
            $(
                $variant = $value,
            )*
        }

        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd)]
        $vis struct $sname($t);

        impl core::fmt::Debug for $sname {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(
                    f,
                    "{:#x} {:?}",
                    { self.0 },
                    [$($value,)*]
                        .iter()
                        .filter_map(|i| {
                            let masked = self.0 & i;
                            if masked == 0 {
                                None
                            } else {
                                Some(unsafe { core::mem::transmute::<$t, $name>(masked) })
                            }
                        })
                        .collect::<$crate::alloc::vec::Vec<_>>()
                )
            }
        }

        impl $sname {
            pub const fn empty() -> Self {
                Self(0)
            }

            pub const fn get(&self) -> $t {
                self.0
            }

            pub const fn set(&mut self, value: $name) -> &mut Self {
                self.0 |= value as $t;
                self
            }

            pub const fn unset(&mut self, value: $name) -> &mut Self {
                self.0 &= !(value as $t);
                self
            }

            pub const fn clear(&mut self) -> &mut Self {
                self.0 = 0;
                self
            }

            pub const fn toggle(&mut self, value: $name) -> &mut Self {
                self.0 ^= value as $t;
                self
            }

            pub const fn has(&self, value: $name) -> bool {
                self.0 & (value as $t) != 0
            }

            pub const fn is_empty(&self) -> bool {
                self.0 == 0
            }
        }

        impl From<$t> for $sname {
            fn from(value: $t) -> Self {
                Self(value)
            }
        }

        impl From<$sname> for $t {
            fn from(value: $sname) -> Self {
                value.0
            }
        }

        impl From<$name> for $sname {
            fn from(value: $name) -> Self {
                Self(value as $t)
            }
        }

        impl From<$sname> for $name {
            fn from(value: $sname) -> Self {
                unsafe { core::mem::transmute(value.0) }
            }
        }

        impl core::ops::BitOr<$sname> for $sname {
            type Output = Self;

            fn bitor(self, rhs: Self) -> Self::Output {
                Self(self.0 | rhs.0)
            }
        }

        impl core::ops::BitOrAssign for $sname {
            fn bitor_assign(&mut self, rhs: Self) {
                self.0 |= rhs.0;
            }
        }

        impl core::ops::BitAnd<$sname> for $sname {
            type Output = Self;

            fn bitand(self, rhs: Self) -> Self::Output {
                Self(self.0 & rhs.0)
            }
        }

        impl core::ops::BitAndAssign for $sname {
            fn bitand_assign(&mut self, rhs: Self) {
                self.0 &= rhs.0;
            }
        }

        impl core::ops::BitXor<$sname> for $sname {
            type Output = Self;

            fn bitxor(self, rhs: Self) -> Self::Output {
                Self(self.0 ^ rhs.0)
            }
        }

        impl core::ops::BitXorAssign for $sname {
            fn bitxor_assign(&mut self, rhs: Self) {
                self.0 ^= rhs.0;
            }
        }

        impl core::ops::Not for $sname {
            type Output = Self;

            fn not(self) -> Self::Output {
                Self(!self.0)
            }
        }

        impl core::ops::BitOr<$name> for $sname {
            type Output = Self;

            fn bitor(self, rhs: $name) -> Self::Output {
                Self(self.0 | rhs as $t)
            }
        }

        impl core::ops::BitOrAssign<$name> for $sname {
            fn bitor_assign(&mut self, rhs: $name) {
                self.0 |= rhs as $t;
            }
        }

        impl core::ops::BitAnd<$name> for $sname {
            type Output = Self;

            fn bitand(self, rhs: $name) -> Self::Output {
                Self(self.0 & rhs as $t)
            }
        }

        impl core::ops::BitAndAssign<$name> for $sname {
            fn bitand_assign(&mut self, rhs: $name) {
                self.0 &= rhs as $t;
            }
        }

        impl core::ops::BitXor<$name> for $sname {
            type Output = Self;

            fn bitxor(self, rhs: $name) -> Self::Output {
                Self(self.0 ^ rhs as $t)
            }
        }

        impl core::ops::BitXorAssign<$name> for $sname {
            fn bitxor_assign(&mut self, rhs: $name) {
                self.0 ^= rhs as $t;
            }
        }
    }
}
