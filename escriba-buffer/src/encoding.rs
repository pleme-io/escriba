use serde::{Deserialize, Serialize};

/// Encoding — phase 1 supports only UTF-8; Latin-1 / UTF-16 land in phase 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Encoding {
    #[default]
    Utf8,
    Latin1,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Latin1 => "latin1",
            Self::Utf16Le => "utf-16le",
            Self::Utf16Be => "utf-16be",
        }
    }
}
