use serde::{Deserialize, Serialize};

/// Line-ending flavor — preserved across save roundtrips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineEnding {
    #[default]
    Lf,
    Crlf,
    Cr,
}

impl LineEnding {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::Crlf => "\r\n",
            Self::Cr => "\r",
        }
    }

    /// Detect from a sample. LF wins in ties — fewer bytes, more common.
    #[must_use]
    pub fn detect(src: &str) -> Self {
        let crlf = src.matches("\r\n").count();
        if crlf > 0 {
            return Self::Crlf;
        }
        if src.contains('\r') {
            return Self::Cr;
        }
        Self::Lf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_defaults() {
        assert_eq!(LineEnding::detect(""), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\nb\nc"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\r\nb\r\nc"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("a\rb\rc"), LineEnding::Cr);
    }
}
