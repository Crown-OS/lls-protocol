/// Declares a value whose set of meanings may grow: a transparent integer with named constants,
/// so a receiver maps a value it does not know to a documented fallback instead of failing to
/// decode the whole message.
macro_rules! tolerant_value {
    (
        $(#[$meta:meta])*
        $name:ident($repr:ty) {
            $($(#[$constant_meta:meta])* $constant:ident = $value:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub $repr);

        impl $name {
            $($(#[$constant_meta])* pub const $constant: Self = Self($value);)+
        }
    };
}

/// Declares an append-only set of capability bits, kept as received so bits a newer peer
/// sets survive.
macro_rules! tolerant_bits {
    (
        $(#[$meta:meta])*
        $name:ident { $($constant:ident = $bit:literal),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, Default, PartialEq, Eq, Hash,
            serde::Serialize, serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u32);

        impl $name {
            pub const EMPTY: Self = Self(0);
            $(pub const $constant: Self = Self(1 << $bit);)+

            #[must_use]
            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            #[must_use]
            pub const fn with(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }
        }
    };
}
