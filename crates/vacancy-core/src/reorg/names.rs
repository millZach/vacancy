//! Lossless Windows UTF-16 components, validated again after journal decoding.
use super::Error;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Name(pub(crate) Vec<u16>);

impl Name {
    pub(crate) fn checked(units: Vec<u16>) -> Result<Self, Error> {
        if units.is_empty()
            || units.len() > 255
            || units
                .iter()
                .any(|&c| c < 32 || matches!(c, 34 | 42 | 47 | 58 | 60 | 62 | 63 | 92 | 124))
            || matches!(units.last(), Some(32 | 46))
            || units == [46]
            || units == [46, 46]
        {
            return Err(Error::InvalidName);
        }
        let ascii = String::from_utf16_lossy(&units);
        let stem = ascii.split('.').next().unwrap_or("").to_uppercase();
        if matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|p| {
            stem.strip_prefix(p).is_some_and(|n| {
                matches!(
                    n,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        }) || ascii.contains('~')
        {
            return Err(Error::InvalidName);
        }
        Ok(Self(units))
    }

    pub(crate) fn text(text: &str) -> Result<Self, Error> {
        if text.len() > 4 * 255 {
            return Err(Error::InvalidName);
        }
        Self::checked(text.encode_utf16().take(256).collect())
    }

    pub(crate) fn display(&self) -> String {
        display(self.0.iter().copied())
    }

    #[cfg(windows)]
    pub(crate) fn os(&self) -> std::ffi::OsString {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&self.0)
    }
}

/// Lossless review text. Backslashes are escaped too, so literal escape-looking text is distinct.
pub(super) fn display(units: impl IntoIterator<Item = u16>) -> String {
    let mut out = String::new();
    for unit in char::decode_utf16(units) {
        match unit {
            Ok('\\') => out.push_str("\\\\"),
            Ok(character) => out.push(character),
            Err(error) => out.push_str(&format!("\\u{{{:04X}}}", error.unpaired_surrogate())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_path_stream_device_alias_and_ambiguous_names() {
        for name in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "C:",
            "x:stream",
            "NUL.txt",
            "com1",
            "LPT².txt",
            "x.",
            "x ",
            "a\0b",
            "SHORT~1",
        ] {
            assert!(Name::text(name).is_err(), "{name:?}");
        }
        assert!(Name::text("Résumé (2).pdf").is_ok());
    }
    #[test]
    fn native_units_survive_json_without_display_becoming_authority() {
        let name = Name::checked(vec![0xd800, 65, 46, 116]).unwrap();
        let stored = serde_json::to_vec(&name).unwrap();
        let loaded: Name = serde_json::from_slice(&stored).unwrap();
        assert_eq!(loaded, name);
        assert_eq!(name.display(), "\\u{D800}A.t");
        assert!(Name::text(&name.display()).is_err());
        assert_ne!(name.display(), Name::text("\u{FFFD}A.t").unwrap().display());
        assert_ne!(
            display([92, 117, 123, 68, 56, 48, 48, 125]),
            display([0xd800])
        );
        assert!(Name::text(&"a".repeat(1024 * 1024)).is_err());
    }
}
