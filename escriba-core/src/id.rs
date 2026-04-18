//! Opaque identifiers — one counter per domain. All are transparent newtypes
//! so serde emits them as plain numbers.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Default,
            Serialize,
            Deserialize,
            JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}#{}", stringify!($name), self.0)
            }
        }
    };
}

id_type!(
    BufferId,
    "Identifies one open buffer — unique across the editor session."
);
id_type!(
    WindowId,
    "Identifies one UI window (one viewport over a buffer)."
);
id_type!(
    CaretId,
    "Identifies a caret within a selection, stable across edits."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_newtypes() {
        let a = BufferId(1);
        let b = BufferId(1);
        assert_eq!(a, b);
        let s = serde_json::to_string(&a).unwrap();
        assert_eq!(s, "1");
    }

    #[test]
    fn display_format() {
        assert_eq!(BufferId(42).to_string(), "BufferId#42");
    }
}
