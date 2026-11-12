//! Pure pre-flight planning for injected (StepMania) step-data files.
//!
//! An injected song's SSQ was self-checked when the index wrote it, but the
//! file on disk is what the boot analysis reads — a later edit, truncation
//! or disk fault would make `CheckStepDataActor` analyse zero steps for a
//! declared slot and raise `ME1529`, a hard boot blocker. So before the
//! first stock item runs, the hook re-checks every injected file the cache
//! cannot vouch for and quarantines the ones that fail (see `plan.rs`).
//!
//! This file decides WHICH files to check and AGAINST WHICH slots; the hook
//! does the I/O and runs `core::ssq::check::check_ssq`. A work item
//! `{entry_index, difficulty}` analyses both styles of its difficulty, so
//! the slots that must hold steps are `{d, d + 5}` ∩ the song's declared
//! levels; a file shared by several items (the unsplit `<b>.ssq`) is
//! checked once against the union.
//!
//! Dependency-free — host-tested via `scripts/validate_fast_bootup.sh`.

use std::collections::BTreeMap;

/// Music-DB slots per song (`difficulty + 5 · style`).
pub const SLOTS: usize = 10;

/// One work item's pre-flight input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemInput {
    pub entry_index: i32,
    pub difficulty: i32,
    /// The injected song's declared levels per slot; `None` = stock song.
    pub levels: Option<[u8; SLOTS]>,
    /// The cache already vouches for this item (identity verified, payloads
    /// present) — the file was analysed cleanly before and is unchanged.
    pub hit: bool,
}

/// The slots item `difficulty` analyses that the song declares.
pub fn item_slots(levels: &[u8; SLOTS], difficulty: i32) -> Vec<u8> {
    if !(0..5).contains(&difficulty) {
        return Vec::new();
    }
    let d = difficulty as usize;
    [d, d + 5]
        .into_iter()
        .filter(|&s| levels[s] != 0)
        .map(|s| s as u8)
        .collect()
}

/// Files to pre-flight: `entry_index → sorted declared slots`. A file is
/// checked when it carries at least one injected item the cache does not
/// vouch for; its slot set is the union over ALL its injected items (the
/// verdict applies to the whole file).
pub fn files_to_check(items: &[ItemInput]) -> BTreeMap<i32, Vec<u8>> {
    let mut needs: BTreeMap<i32, bool> = BTreeMap::new();
    let mut slots: BTreeMap<i32, Vec<u8>> = BTreeMap::new();
    for it in items {
        let Some(levels) = it.levels else { continue };
        if it.entry_index <= 0 {
            continue;
        }
        *needs.entry(it.entry_index).or_insert(false) |= !it.hit;
        let set = slots.entry(it.entry_index).or_default();
        for s in item_slots(&levels, it.difficulty) {
            if !set.contains(&s) {
                set.push(s);
            }
        }
    }
    slots
        .into_iter()
        .filter(|(idx, _)| needs.get(idx).copied().unwrap_or(false))
        .map(|(idx, mut v)| {
            v.sort_unstable();
            (idx, v)
        })
        .collect()
}

/// The SSQ files an injected song's work items will read, each with the
/// declared slots it must hold, from the song's own index record (no work
/// list needed — the background pre-flight runs before the boot actor
/// exists). `file_for(d)` is the song's split rule (`Some(N)` = `<b>_N.ssq`
/// answers difficulty `d`, `None` = `<b>.ssq`). Files that serve no declared
/// slot are omitted.
pub fn entry_files(
    basename: &str,
    levels: &[u8; SLOTS],
    file_for: impl Fn(usize) -> Option<u8>,
) -> Vec<(String, Vec<u8>)> {
    let mut files: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    for d in 0..5usize {
        let slots = item_slots(levels, d as i32);
        if slots.is_empty() {
            continue;
        }
        files
            .entry(file_for(d).unwrap_or(0))
            .or_default()
            .extend(slots);
    }
    files
        .into_iter()
        .map(|(n, mut slots)| {
            slots.sort_unstable();
            let name = if n == 0 {
                format!("{basename}.ssq")
            } else {
                format!("{basename}_{n}.ssq")
            };
            (name, slots)
        })
        .collect()
}

/// Per-item quarantine flags from the per-file verdicts (`true` = failed).
pub fn item_flags(items: &[ItemInput], failed: &BTreeMap<i32, bool>) -> Vec<bool> {
    items
        .iter()
        .map(|it| it.entry_index > 0 && failed.get(&it.entry_index).copied().unwrap_or(false))
        .collect()
}

/// The actor's ten per-side radar accumulators (`+0xA8..+0xCC`). Injected
/// items are excluded by restoring this image after their stock call.
pub type RadarImage = [i32; 10];

/// Restore rule: the snapshot taken before an injected stock item wins over
/// whatever the original folded in.
pub fn restore_radar(after: &RadarImage, before: &RadarImage, injected: bool) -> RadarImage {
    if injected {
        *before
    } else {
        *after
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn levels(pairs: &[(usize, u8)]) -> [u8; SLOTS] {
        let mut l = [0u8; SLOTS];
        for &(s, v) in pairs {
            l[s] = v;
        }
        l
    }

    fn it(entry_index: i32, difficulty: i32, l: Option<[u8; SLOTS]>, hit: bool) -> ItemInput {
        ItemInput {
            entry_index,
            difficulty,
            levels: l,
            hit,
        }
    }

    #[test]
    fn item_slots_both_styles() {
        let l = levels(&[(3, 12), (8, 13), (4, 15)]);
        assert_eq!(item_slots(&l, 3), vec![3, 8]);
        assert_eq!(item_slots(&l, 4), vec![4]);
        assert!(item_slots(&l, 0).is_empty());
        assert!(item_slots(&l, 5).is_empty());
        assert!(item_slots(&l, -1).is_empty());
    }

    #[test]
    fn unsplit_song_checked_once_against_union() {
        let l = Some(levels(&[(1, 5), (3, 12), (8, 13)]));
        let items: Vec<_> = (0..5).map(|d| it(7, d, l, false)).collect();
        let files = files_to_check(&items);
        assert_eq!(files.len(), 1);
        assert_eq!(files[&7], vec![1, 3, 8]);
    }

    #[test]
    fn split_files_checked_per_difficulty() {
        let l = Some(levels(&[(3, 12), (4, 15), (9, 16)]));
        let items = vec![it(11, 3, l, false), it(12, 4, l, false)];
        let files = files_to_check(&items);
        assert_eq!(files[&11], vec![3]);
        assert_eq!(files[&12], vec![4, 9]);
    }

    #[test]
    fn stock_and_vouched_files_skipped() {
        let l = Some(levels(&[(3, 12)]));
        let items = vec![
            it(5, 3, None, false), // stock song
            it(6, 3, l, true),     // injected, fully vouched
            it(7, 2, l, true),     // injected, one vouched + one miss
            it(7, 3, l, false),
            it(-1, 3, l, false), // unregistered
        ];
        let files = files_to_check(&items);
        assert_eq!(files.keys().copied().collect::<Vec<_>>(), vec![7]);
        assert_eq!(files[&7], vec![3]);
    }

    #[test]
    fn flags_follow_file_verdict() {
        let l = Some(levels(&[(3, 12)]));
        let items = vec![it(7, 2, l, false), it(7, 3, l, false), it(8, 3, l, false)];
        let mut failed = BTreeMap::new();
        failed.insert(7, true);
        failed.insert(8, false);
        assert_eq!(item_flags(&items, &failed), vec![true, true, false]);
    }

    #[test]
    fn entry_files_unsplit_and_split() {
        let l = levels(&[(1, 5), (3, 12), (8, 13), (4, 15)]);
        assert_eq!(
            entry_files("zab12c", &l, |_| None),
            vec![("zab12c.ssq".to_string(), vec![1, 3, 4, 8])]
        );
        // Expert (d 3) and Challenge (d 4) in their own split files.
        let split = entry_files("zab12c", &l, |d| match d {
            3 => Some(4),
            4 => Some(5),
            _ => None,
        });
        assert_eq!(
            split,
            vec![
                ("zab12c.ssq".to_string(), vec![1]),
                ("zab12c_4.ssq".to_string(), vec![3, 8]),
                ("zab12c_5.ssq".to_string(), vec![4]),
            ]
        );
        assert!(entry_files("zab12c", &[0; SLOTS], |_| None).is_empty());
    }

    #[test]
    fn radar_restore_only_for_injected() {
        let before = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
        let after = [1, 2, 300, 4, 5, 6, 7, 800, 9, 10];
        assert_eq!(restore_radar(&after, &before, true), before);
        assert_eq!(restore_radar(&after, &before, false), after);
    }
}
