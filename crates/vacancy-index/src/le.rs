//! Bounds-checked little-endian reads. Every read of on-disk bytes goes through these, so a field that runs past
//! its buffer is `None`, never a panic or an out-of-bounds read.

/// `N` bytes at `at`, if they are all inside `b`.
pub(crate) fn array<const N: usize>(b: &[u8], at: usize) -> Option<[u8; N]> {
    let end = at.checked_add(N)?;
    b.get(at..end)?.try_into().ok()
}

pub(crate) fn u8_at(b: &[u8], at: usize) -> Option<u8> {
    b.get(at).copied()
}

pub(crate) fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    array(b, at).map(u16::from_le_bytes)
}

pub(crate) fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    array(b, at).map(u32::from_le_bytes)
}

pub(crate) fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    array(b, at).map(u64::from_le_bytes)
}

pub(crate) fn i64_at(b: &[u8], at: usize) -> Option<i64> {
    array(b, at).map(i64::from_le_bytes)
}

/// `len` bytes at `at`, if they are all inside `b`.
pub(crate) fn slice(b: &[u8], at: usize, len: usize) -> Option<&[u8]> {
    b.get(at..at.checked_add(len)?)
}

/// `units` UTF-16 code units at `at`.
pub(crate) fn utf16_at(b: &[u8], at: usize, units: usize) -> Option<Vec<u16>> {
    let bytes = slice(b, at, units.checked_mul(2)?)?;
    Some(
        bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_past_the_end_are_none() {
        let b = [1u8, 2, 3, 4];
        assert_eq!(u16_at(&b, 2), Some(0x0403));
        assert_eq!(u16_at(&b, 3), None);
        assert_eq!(u32_at(&b, 1), None);
        assert_eq!(u64_at(&b, usize::MAX), None);
        assert_eq!(slice(&b, usize::MAX, 2), None);
        assert_eq!(utf16_at(&b, 0, usize::MAX), None);
        assert_eq!(utf16_at(&b, 0, 2), Some(vec![0x0201, 0x0403]));
    }
}
