//! Opaque editor-core entity identifiers.
//!
//! Identity stays in the editor domain. These types contain no media or UI
//! policy and intentionally have no dependencies.

macro_rules! entity_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u64);

        impl $name {
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            pub const fn raw(self) -> u64 {
                self.0
            }
        }
    };
}

entity_id!(AssetId);
entity_id!(ClipId);
entity_id!(TimelineId);
entity_id!(TrackId);
