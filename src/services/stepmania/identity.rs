//! Identity derivation for injected songs — pure (design §5.2, feasibility
//! §6.4).
//!
//! `key = casefold(pack_folder) + "/" + casefold(song_folder)`;
//! `h = fnv1a64(key)`; `basename = "z" + base36(h)[0..5]` (the 13-digit
//! zero-padded base36 rendering of the u64, first five digits — the same
//! derivation `tools/sm_pack_check` shipped as its Step 5 placeholder, kept
//! byte-identical so codes are stable across the move); `mcode = 9_000_000 +
//! (h >> 40) % 1_000_000`.
//!
//! Why these shapes: stock basenames are 4–5 characters, so a 6-character
//! `z…` code cannot collide with any current Konami code; the `music::Info`
//! constructor truncates at 6, so nothing longer is usable; stock mcodes are
//! ≤ ~39k and the 9,000,000 window stays below `0x8000_0000` (several
//! consumers treat mcode as `int`). Both are pure functions of the folder
//! path, so clearing `_cache` never renumbers a song.
//!
//! Collisions inside one index (two keys hashing to the same basename or
//! mcode) are resolved by deterministic probing: keys are visited in sorted
//! order and a colliding key re-hashes `key + "#" + i`, i = 1.., until it is
//! free of every earlier assignment and of the reserved set (`lesa`,
//! `toho1..4` and the seven special-cased mcodes — unreachable from the
//! `z…` / 9M window by construction, checked anyway). Collisions with the
//! base musicdb are the transform's business (`musicdb::transform` drops
//! them with a WARN; never renumbered on the fly).
//!
//! Purity contract: `std` only; mounted by `scripts/validate_stepmania.sh`
//! and `tools/sm_pack_check`.

use std::collections::HashSet;

/// First mcode of the injected window.
pub const MCODE_BASE: u32 = 9_000_000;
/// Window size.
pub const MCODE_SPAN: u64 = 1_000_000;
/// Basename length (the `music::Info` inline-copy limit).
pub const BASENAME_LEN: usize = 6;
/// Probe cap — beyond this the key is unassignable (astronomically unlikely).
pub const MAX_PROBES: u32 = 64;

/// Basenames the game special-cases (`split_ssq_research.md` §8).
pub const RESERVED_BASENAMES: [&str; 5] = ["lesa", "toho1", "toho2", "toho3", "toho4"];
/// mcodes special-cased by the parser / play sequences / flare.
pub const RESERVED_MCODES: [u32; 7] = [0x931C, 0x9525, 0x939D, 0x9306, 0x94E7, 0x9733, 0x950C];

/// One song's derived identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identity {
    pub basename: String,
    pub mcode: u32,
    /// Which probe produced it (0 = the plain key).
    pub probe: u32,
}

#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Base-36 rendering (`0-9a-z`), no padding; `"0"` for zero.
#[must_use]
pub fn base36(mut v: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if v == 0 {
        return "0".into();
    }
    let mut out = Vec::with_capacity(13);
    while v > 0 {
        out.push(DIGITS[(v % 36) as usize]);
        v /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Unicode-aware case fold (Rust's `to_lowercase`), the same on every host.
#[must_use]
pub fn casefold(s: &str) -> String {
    s.to_lowercase()
}

/// The identity key of a song folder.
#[must_use]
pub fn key(pack_folder: &str, song_folder: &str) -> String {
    format!("{}/{}", casefold(pack_folder), casefold(song_folder))
}

/// Identity for `key` at probe `probe` (0 = unprobed).
#[must_use]
pub fn derive(key: &str, probe: u32) -> Identity {
    let h = if probe == 0 {
        fnv1a64(key.as_bytes())
    } else {
        fnv1a64(format!("{key}#{probe}").as_bytes())
    };
    let digits = format!("{:0>13}", base36(h));
    let basename = format!("z{}", &digits[..BASENAME_LEN - 1]);
    let mcode = MCODE_BASE + ((h >> 40) % MCODE_SPAN) as u32;
    Identity {
        basename,
        mcode,
        probe,
    }
}

/// Whether an identity touches the reserved set.
#[must_use]
pub fn is_reserved(id: &Identity) -> bool {
    RESERVED_BASENAMES.contains(&id.basename.as_str()) || RESERVED_MCODES.contains(&id.mcode)
}

/// Whether a basename has the shape the game and the XACT names need:
/// exactly 6 bytes of `[a-z0-9]`.
#[must_use]
pub fn is_well_formed_basename(b: &str) -> bool {
    b.len() == BASENAME_LEN
        && b.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

/// Assign identities to a set of keys with deterministic probing. Input
/// order does not matter (keys are sorted internally); duplicates collapse
/// to one assignment. Returns `(key, identity)` pairs in sorted-key order; a
/// key that exhausts [`MAX_PROBES`] is omitted (callers log it).
#[must_use]
pub fn assign(keys: &[String]) -> Vec<(String, Identity)> {
    let mut sorted: Vec<&String> = keys.iter().collect();
    sorted.sort();
    sorted.dedup();
    let mut used_basenames: HashSet<String> = HashSet::with_capacity(sorted.len());
    let mut used_mcodes: HashSet<u32> = HashSet::with_capacity(sorted.len());
    let mut out = Vec::with_capacity(sorted.len());
    for k in sorted {
        let mut chosen = None;
        for probe in 0..=MAX_PROBES {
            let id = derive(k, probe);
            if is_reserved(&id)
                || used_basenames.contains(&id.basename)
                || used_mcodes.contains(&id.mcode)
            {
                continue;
            }
            chosen = Some(id);
            break;
        }
        if let Some(id) = chosen {
            used_basenames.insert(id.basename.clone());
            used_mcodes.insert(id.mcode);
            out.push((k.clone(), id));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_matches_the_step5_placeholder_and_is_well_formed() {
        let k = key("Stamina RPG 5", "[11] Big Money (Medium)");
        let a = derive(&k, 0);
        let b = derive(&key("STAMINA rpg 5", "[11] BIG MONEY (Medium)"), 0);
        assert_eq!(a, b, "casefolded key");
        assert!(a.basename.starts_with('z'));
        assert!(is_well_formed_basename(&a.basename));
        assert!((MCODE_BASE..MCODE_BASE + MCODE_SPAN as u32).contains(&a.mcode));
        assert_ne!(a, derive(&key("Stamina RPG 5", "[11] Burn Up (Hard)"), 0));
        // Probing changes the identity deterministically.
        let p1 = derive(&k, 1);
        assert_ne!(p1.basename, a.basename);
        assert_eq!(p1, derive(&k, 1));
        assert_eq!(p1.probe, 1);
    }

    #[test]
    fn base36_and_fnv_vectors() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(base36(u64::MAX).len(), 13);
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn assign_is_order_independent_and_collision_free() {
        let keys: Vec<String> = (0..500)
            .map(|i| key("Pack", &format!("Song {i}")))
            .collect();
        let mut shuffled = keys.clone();
        shuffled.reverse();
        let a = assign(&keys);
        let b = assign(&shuffled);
        assert_eq!(a, b);
        assert_eq!(a.len(), 500);
        let mut basenames: Vec<&str> = a.iter().map(|(_, id)| id.basename.as_str()).collect();
        basenames.sort_unstable();
        basenames.dedup();
        assert_eq!(basenames.len(), 500);
        let mut mcodes: Vec<u32> = a.iter().map(|(_, id)| id.mcode).collect();
        mcodes.sort_unstable();
        mcodes.dedup();
        assert_eq!(mcodes.len(), 500);
        assert!(a.iter().all(|(_, id)| !is_reserved(id)));
        // Duplicates collapse.
        let dup = assign(&[keys[0].clone(), keys[0].clone()]);
        assert_eq!(dup.len(), 1);
    }

    #[test]
    fn reserved_set_is_detected() {
        assert!(is_reserved(&Identity {
            basename: "lesa".into(),
            mcode: 1,
            probe: 0,
        }));
        assert!(is_reserved(&Identity {
            basename: "zaaaaa".into(),
            mcode: 0x931C,
            probe: 0,
        }));
        assert!(!is_reserved(&derive("x/y", 0)));
        assert!(!is_well_formed_basename("zAb12c"));
        assert!(!is_well_formed_basename("zab12"));
    }
}
