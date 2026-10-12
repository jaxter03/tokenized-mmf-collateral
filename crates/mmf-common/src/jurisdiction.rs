//! Jurisdictions as ISO 3166-1 alpha-2 codes and the 676-bit allowed set (research R11,
//! docs/shared-model.md §1 "Jurisdiction").
//!
//! `index(c1, c2) = (c1 − 'A') × 26 + (c2 − 'A')`, range 0..675. Bit `i` of a set lives in word
//! `i / 64` at bit `i % 64`; bits 676..703 must stay zero.

use crate::iso3166::ASSIGNED_CODES;

/// Number of possible two-letter codes (26 × 26).
pub const CODE_SPACE: usize = 676;
/// Words in a jurisdiction set (`[u64; 11]` = 704 bits ≥ 676).
pub const SET_WORDS: usize = 11;
/// A set of jurisdictions, as stored in `Registry.allowed_jurisdictions`.
pub type JurisdictionSet = [u64; SET_WORDS];

/// Bit index of an uppercase two-letter code, or `None` if either byte isn't `A`..=`Z`.
pub const fn index(code: [u8; 2]) -> Option<usize> {
    let (a, b) = (code[0], code[1]);
    if a < b'A' || a > b'Z' || b < b'A' || b > b'Z' {
        return None;
    }
    Some((a - b'A') as usize * 26 + (b - b'A') as usize)
}

const fn build_assigned() -> JurisdictionSet {
    let mut set = [0u64; SET_WORDS];
    let mut i = 0;
    while i < ASSIGNED_CODES.len() {
        let idx = match index(ASSIGNED_CODES[i]) {
            Some(idx) => idx,
            None => panic!("non-letter code in ISO table"),
        };
        set[idx / 64] |= 1u64 << (idx % 64);
        i += 1;
    }
    set
}

/// The officially assigned codes as a set (computed at compile time from [`ASSIGNED_CODES`]).
pub const ASSIGNED_ISO3166: JurisdictionSet = build_assigned();

/// True if `set` contains bit `idx`.
pub const fn bit(set: &JurisdictionSet, idx: usize) -> bool {
    idx < CODE_SPACE && set[idx / 64] & (1u64 << (idx % 64)) != 0
}

/// True if `code` is an officially assigned ISO 3166-1 alpha-2 code.
pub const fn is_assigned(code: [u8; 2]) -> bool {
    match index(code) {
        Some(idx) => bit(&ASSIGNED_ISO3166, idx),
        None => false,
    }
}

/// True if `set` contains `code`. Codes that aren't uppercase letters are never contained.
pub const fn set_contains(set: &JurisdictionSet, code: [u8; 2]) -> bool {
    match index(code) {
        Some(idx) => bit(set, idx),
        None => false,
    }
}

/// Adds `code` to `set`. Returns false (and leaves `set` unchanged) if `code` isn't assigned.
pub fn set_insert(set: &mut JurisdictionSet, code: [u8; 2]) -> bool {
    if !is_assigned(code) {
        return false;
    }
    let idx = index(code).expect("assigned codes are letters");
    set[idx / 64] |= 1u64 << (idx % 64);
    true
}

/// A set is valid when every set bit is an assigned code (so no bit ≥ 676 is set either).
pub const fn valid_set(set: &JurisdictionSet) -> bool {
    let mut w = 0;
    while w < SET_WORDS {
        if set[w] & !ASSIGNED_ISO3166[w] != 0 {
            return false;
        }
        w += 1;
    }
    true
}

/// True if no jurisdiction is in the set.
pub const fn is_empty(set: &JurisdictionSet) -> bool {
    let mut w = 0;
    while w < SET_WORDS {
        if set[w] != 0 {
            return false;
        }
        w += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_249_distinct_codes() {
        let count: u32 = ASSIGNED_ISO3166.iter().map(|w| w.count_ones()).sum();
        assert_eq!(count, 249);
        assert_eq!(ASSIGNED_CODES.len(), 249);
    }

    #[test]
    fn index_formula() {
        assert_eq!(index(*b"AA"), Some(0));
        assert_eq!(index(*b"AB"), Some(1));
        assert_eq!(index(*b"BA"), Some(26));
        assert_eq!(index(*b"ZZ"), Some(675));
        assert_eq!(index(*b"us"), None);
        assert_eq!(index(*b"U1"), None);
        assert_eq!(index([0, 0]), None);
    }

    #[test]
    fn assigned_codes() {
        for c in [*b"US", *b"GB", *b"SS", *b"FR", *b"SG"] {
            assert!(is_assigned(c), "{c:?}");
        }
        for c in [*b"AA", *b"ZZ", *b"XK", *b"us", *b"U1", [0, 0]] {
            assert!(!is_assigned(c), "{c:?}");
        }
    }

    #[test]
    fn sets() {
        let mut s: JurisdictionSet = [0; SET_WORDS];
        assert!(is_empty(&s) && valid_set(&s));
        assert!(set_insert(&mut s, *b"US"));
        assert!(set_insert(&mut s, *b"GB"));
        assert!(!set_insert(&mut s, *b"AA"));
        assert!(set_contains(&s, *b"US") && set_contains(&s, *b"GB"));
        assert!(!set_contains(&s, *b"FR") && !set_contains(&s, *b"us"));
        assert!(valid_set(&s) && !is_empty(&s));

        let mut unassigned = s;
        let aa = index(*b"AA").unwrap();
        unassigned[aa / 64] |= 1 << (aa % 64);
        assert!(!valid_set(&unassigned));

        let mut high = s;
        high[10] |= 1 << (676 % 64); // bit 676, beyond the code space
        assert!(!valid_set(&high));
    }
}
