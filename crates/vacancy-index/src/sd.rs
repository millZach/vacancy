//! Checking a self-relative security descriptor read off the disk before Windows sees it.
//!
//! `IsValidSecurityDescriptor`, `GetSecurityDescriptorLength` and `AccessCheck` take no buffer length: they follow
//! the descriptor's own offsets. On a crafted volume those offsets could point anywhere, in the elevated reader. So
//! every offset, SID, ACL and ACE is checked here to lie inside the bytes first; anything that doesn't is not a
//! descriptor, and its folder counts as locked.

use crate::le::{u8_at, u16_at, u32_at};

const SE_SELF_RELATIVE: u16 = 0x8000;
const HEADER: usize = 20;

/// Whether `sd` is a well-formed self-relative security descriptor, wholly inside its bytes, with an owner and a
/// group (`AccessCheck` needs both).
pub fn is_well_formed(sd: &[u8]) -> bool {
    let (Some(revision), Some(control)) = (u8_at(sd, 0), u16_at(sd, 2)) else {
        return false;
    };
    if sd.len() < HEADER || revision != 1 || control & SE_SELF_RELATIVE == 0 {
        return false;
    }
    let field = |at: usize| u32_at(sd, at).map(|o| o as usize);
    let (Some(owner), Some(group), Some(sacl), Some(dacl)) =
        (field(4), field(8), field(12), field(16))
    else {
        return false;
    };
    if owner == 0 || group == 0 {
        return false;
    }
    let in_body = |o: usize| o >= HEADER && o < sd.len();
    if !in_body(owner) || sid_len(sd, owner, sd.len()).is_none() {
        return false;
    }
    if !in_body(group) || sid_len(sd, group, sd.len()).is_none() {
        return false;
    }
    if sacl != 0 && (!in_body(sacl) || !acl_ok(sd, sacl)) {
        return false;
    }
    if dacl != 0 && (!in_body(dacl) || !acl_ok(sd, dacl)) {
        return false;
    }
    // A DACL marked present with no offset is a NULL DACL (everyone allowed): valid.
    true
}

/// The length of the SID at `at`, if it ends at or before `end`.
fn sid_len(b: &[u8], at: usize, end: usize) -> Option<usize> {
    let revision = u8_at(b, at)?;
    let count = usize::from(u8_at(b, at + 1)?);
    if revision != 1 || count > 15 {
        return None;
    }
    let len = 8 + 4 * count;
    (at.checked_add(len)? <= end.min(b.len())).then_some(len)
}

/// Whether the ACL at `at` and every ACE in it lie inside the descriptor, each ACE's SID inside the ACE.
fn acl_ok(b: &[u8], at: usize) -> bool {
    let (Some(revision), Some(size), Some(count)) =
        (u8_at(b, at), u16_at(b, at + 2), u16_at(b, at + 4))
    else {
        return false;
    };
    let size = usize::from(size);
    let Some(end) = at.checked_add(size) else {
        return false;
    };
    if !(2..=4).contains(&revision) || size < 8 || end > b.len() {
        return false;
    }
    let mut ace = at + 8;
    for _ in 0..count {
        let (Some(kind), Some(ace_size)) = (u8_at(b, ace), u16_at(b, ace + 2)) else {
            return false;
        };
        let ace_size = usize::from(ace_size);
        let Some(ace_end) = ace.checked_add(ace_size) else {
            return false;
        };
        if ace_size < 8 || ace_end > end {
            return false;
        }
        let sid_at = match kind {
            // Allowed, denied, audit, alarm, callback forms, mandatory label, resource attribute, scoped policy,
            // process trust label, access filter: the access mask, then the SID.
            0x00..=0x03 | 0x09 | 0x0A | 0x0D | 0x0E | 0x11..=0x15 => ace + 8,
            // Object ACEs: the mask, object flags, up to two GUIDs as the flags say, then the SID.
            0x05..=0x08 | 0x0B | 0x0C | 0x0F | 0x10 => {
                let Some(flags) = u32_at(b, ace + 8) else {
                    return false;
                };
                ace + 12 + if flags & 1 != 0 { 16 } else { 0 } + if flags & 2 != 0 { 16 } else { 0 }
            }
            _ => return false,
        };
        if sid_len(b, sid_at, ace_end).is_none() {
            return false;
        }
        ace = ace_end;
    }
    true
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const SE_DACL_PRESENT: u16 = 0x0004;

    fn sid(subs: &[u32]) -> Vec<u8> {
        let mut s = vec![1, subs.len() as u8, 0, 0, 0, 0, 0, 5];
        for v in subs {
            s.extend_from_slice(&v.to_le_bytes());
        }
        s
    }

    fn ace(kind: u8, sid: &[u8]) -> Vec<u8> {
        let size = 8 + sid.len();
        let mut a = vec![kind, 0];
        a.extend_from_slice(&(size as u16).to_le_bytes());
        a.extend_from_slice(&0x0012_0089u32.to_le_bytes());
        a.extend_from_slice(sid);
        a
    }

    fn acl(aces: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = aces.concat();
        let mut a = vec![2, 0];
        a.extend_from_slice(&((8 + body.len()) as u16).to_le_bytes());
        a.extend_from_slice(&(aces.len() as u16).to_le_bytes());
        a.extend_from_slice(&[0, 0]);
        a.extend(body);
        a
    }

    /// Owner, group and a DACL with one allow ACE.
    pub(crate) fn good() -> Vec<u8> {
        let owner = sid(&[32, 544]);
        let group = sid(&[18]);
        let dacl = acl(&[ace(0, &sid(&[11]))]);
        let mut sd = vec![1, 0];
        sd.extend_from_slice(&(SE_SELF_RELATIVE | SE_DACL_PRESENT).to_le_bytes());
        let o = HEADER;
        let g = o + owner.len();
        let d = g + group.len();
        for v in [o as u32, g as u32, 0, d as u32] {
            sd.extend_from_slice(&v.to_le_bytes());
        }
        sd.extend(owner);
        sd.extend(group);
        sd.extend(dacl);
        sd
    }

    #[test]
    fn accepts_a_whole_descriptor() {
        assert!(is_well_formed(&good()));
    }

    #[test]
    fn refuses_anything_that_reaches_outside_its_bytes() {
        let sd = good();
        assert!(!is_well_formed(&sd[..sd.len() - 1]), "cut short");
        assert!(!is_well_formed(&sd[..10]));
        let mut absolute = sd.clone();
        absolute[3] &= 0x7F; // not self-relative: its offsets would be pointers
        assert!(!is_well_formed(&absolute));
        let mut far = sd.clone();
        far[4..8].copy_from_slice(&0xFFFF_0000u32.to_le_bytes());
        assert!(!is_well_formed(&far), "an owner past the end");
        let mut no_group = sd.clone();
        no_group[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert!(!is_well_formed(&no_group), "no group");
        let mut many = sd.clone();
        many[HEADER + 1] = 200; // the owner SID's sub-authority count
        assert!(!is_well_formed(&many));
        let d = u32::from_le_bytes(sd[16..20].try_into().unwrap()) as usize;
        let mut big_acl = sd.clone();
        big_acl[d + 2..d + 4].copy_from_slice(&0x7FFFu16.to_le_bytes());
        assert!(
            !is_well_formed(&big_acl),
            "an ACL longer than the descriptor"
        );
        let mut more_aces = sd.clone();
        more_aces[d + 4..d + 6].copy_from_slice(&5u16.to_le_bytes());
        assert!(!is_well_formed(&more_aces), "more ACEs than the ACL holds");
        let mut odd_ace = sd.clone();
        odd_ace[d + 8] = 0x77;
        assert!(!is_well_formed(&odd_ace), "an ACE type nobody knows");
        let mut long_sid = sd.clone();
        long_sid[d + 8 + 8 + 1] = 9; // the ACE's SID claims more than the ACE holds
        assert!(!is_well_formed(&long_sid));
        // Every truncation and every single-byte change: never a panic.
        for i in 0..sd.len() {
            let _ = is_well_formed(&sd[..i]);
            let mut b = sd.clone();
            b[i] ^= 0xFF;
            let _ = is_well_formed(&b);
        }
    }
}
