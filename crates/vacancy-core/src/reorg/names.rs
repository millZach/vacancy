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
            || units.iter().any(|&c| c < 32 || matches!(c, 34 | 42 | 47 | 58 | 60 | 62 | 63 | 92 | 124))
            || matches!(units.last(), Some(32 | 46))
            || units == [46]
            || units == [46, 46]
        {
            return Err(Error::InvalidName);
        }
        let ascii = String::from_utf16_lossy(&units);
        let stem = ascii.split('.').next().unwrap_or("").to_uppercase();
        if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
            || ["COM", "LPT"].iter().any(|p| stem.strip_prefix(p).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")))
            || ascii.contains('~')
        {
            return Err(Error::InvalidName);
        }
        Ok(Self(units))
    }

    pub(crate) fn text(text: &str) -> Result<Self, Error> {
        Self::checked(text.encode_utf16().collect())
    }

    pub(crate) fn display(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }

    #[cfg(windows)]
    pub(crate) fn os(&self) -> std::ffi::OsString {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_path_stream_device_alias_and_ambiguous_names() {
        for name in ["", ".", "..", "a/b", "a\\b", "C:", "x:stream", "NUL.txt", "com1", "LPT².txt", "x.", "x ", "a\0b", "SHORT~1"] {
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
        assert_ne!(Name::text(&name.display()).unwrap(), name);
    }
}
