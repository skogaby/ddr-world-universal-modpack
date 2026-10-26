//! The injection ceiling — how many indexed songs one boot may put into the
//! game, decided deterministically from the index and the game's limits.
//! Pure: `std` + `super::model` only; mounted by
//! `scripts/validate_stepmania.sh`.
//!
//! There is no operator knob. The index may hold any number of songs (the
//! `index.bin` / artefact caches are unbounded); what reaches `musicdb.xml`,
//! the thumbnail arc and the boot pass is the subset this module selects,
//! and the rest is reported in one WARN. Two limits apply, in order:
//!
//! 1. **Count** — [`MAX_INJECTED_SONGS`]. The tested headroom of everything
//!    downstream of the music DB that scales with the injected count and
//!    that no patch widens: the series thumbnail arc the game loads whole
//!    at boot (~18.5 KiB per song), the `CheckStepDataActor` boot pass over
//!    every injected SSQ, the wheel. The largest libraries booted so far
//!    are ~1,400 songs (a tester cabinet) and 627 (the maintainer's); a
//!    14,000-song library never reached the game. Raise this constant as
//!    larger libraries are validated — never per cabinet.
//! 2. **Arena text budget** — the AVS property arena the game parses the
//!    whole document into (`mods::song_limit_expansion`). The tree costs
//!    roughly the text; the stock 716 KB file fits the stock 1 MiB arena, so
//!    the tree-over-text factor is ≤ 1.5 and [`text_budget`] admits
//!    `arena × 2/3` bytes of document. With the stock arena (patch failed or
//!    the mod is off) nothing is injected: the stock document already uses
//!    most of it and the headroom is unknown. With the expanded arena this
//!    budget never binds below the count ceiling; it is the safety net for a
//!    large operator omnimix overlay or a future count raise.
//!
//! Selection order is the sorted `pack/folder` key, so the same subset is
//! kept on every boot regardless of index completion order, and the
//! operator can predict it: the first packs alphabetically stay.

use std::cmp::Ordering;

use super::model::SongEntry;

/// Count ceiling (see the module doc for why this value).
pub const MAX_INJECTED_SONGS: usize = 4_000;
/// The stock property arena (the imm32 at every parser site); at or below
/// this nothing is injected.
pub const STOCK_ARENA_BYTES: usize = 0x0010_0000;
/// The arena `mods::song_limit_expansion` patches in (64 MiB; AGCS heap,
/// freed after the parse). Defined here, in a mountable file, so the prep
/// tool's ceiling report and the DLL's patch share one number.
pub const EXPANDED_ARENA_BYTES: usize = 0x0400_0000;
/// Tree-over-text safety factor, as `text ≤ arena × NUM / DEN`.
const ARENA_TEXT_NUM: usize = 2;
const ARENA_TEXT_DEN: usize = 3;

/// Document bytes the arena may safely hold.
#[must_use]
pub const fn text_budget(arena_bytes: usize) -> usize {
    arena_bytes / ARENA_TEXT_DEN * ARENA_TEXT_NUM
}

/// What limits the selection worked under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// The arena the game's parser will use this boot.
    pub arena_bytes: usize,
    /// The count ceiling (normally [`MAX_INJECTED_SONGS`]).
    pub max_songs: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            arena_bytes: STOCK_ARENA_BYTES,
            max_songs: MAX_INJECTED_SONGS,
        }
    }
}

/// The selection outcome.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    /// Indices into the input slice of the songs to inject, in selection
    /// (sorted-key) order.
    pub kept: Vec<usize>,
    /// Songs left out by the count ceiling.
    pub over_count: usize,
    /// The arena is stock-sized: everything was left out.
    pub arena_unpatched: bool,
}

impl Selection {
    #[must_use]
    pub fn left_out(&self) -> usize {
        self.over_count
    }
}

fn key_cmp(a: &SongEntry, b: &SongEntry) -> Ordering {
    a.pack
        .cmp(&b.pack)
        .then_with(|| a.folder.cmp(&b.folder))
        .then_with(|| a.basename.cmp(&b.basename))
}

/// Choose the songs to inject under `budget`. Deterministic for a given
/// set of entries (sorted by `pack/folder`, input order irrelevant).
#[must_use]
pub fn select<T: std::borrow::Borrow<SongEntry>>(entries: &[T], budget: &Budget) -> Selection {
    let mut sel = Selection::default();
    if budget.arena_bytes <= STOCK_ARENA_BYTES {
        sel.arena_unpatched = true;
        return sel;
    }
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|&a, &b| key_cmp(entries[a].borrow(), entries[b].borrow()));
    let keep = order.len().min(budget.max_songs);
    sel.over_count = order.len() - keep;
    order.truncate(keep);
    sel.kept = order;
    sel
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pack: &str, folder: &str, n: u32) -> SongEntry {
        SongEntry {
            pack: pack.into(),
            folder: folder.into(),
            basename: format!("z{n:05}"),
            mcode: 9_000_000 + n,
            levels: [0, 0, 0, 0, 5, 0, 0, 0, 0, 0],
            ..SongEntry::default()
        }
    }

    const EXPANDED: usize = EXPANDED_ARENA_BYTES;

    #[test]
    fn text_budget_is_two_thirds() {
        assert_eq!(text_budget(0x0400_0000), 0x0400_0000 / 3 * 2);
        assert_eq!(text_budget(3), 2);
        // The stock arena would admit less than the stock document.
        assert!(text_budget(STOCK_ARENA_BYTES) < 716_552);
    }

    #[test]
    fn stock_arena_injects_nothing() {
        let e = [entry("A", "1", 1), entry("A", "2", 2)];
        let sel = select(
            &e,
            &Budget {
                arena_bytes: STOCK_ARENA_BYTES,
                max_songs: 10,
            },
        );
        assert!(sel.arena_unpatched);
        assert!(sel.kept.is_empty());
        assert_eq!(sel.over_count, 0);
        assert!(select(&e, &Budget::default()).arena_unpatched);
    }

    #[test]
    fn selection_is_sorted_by_key_and_order_independent() {
        let a = entry("Pack B", "song", 1);
        let b = entry("Pack A", "zeta", 2);
        let c = entry("Pack A", "alpha", 3);
        let budget = Budget {
            arena_bytes: EXPANDED,
            max_songs: 10,
        };
        let s1 = select(&[a.clone(), b.clone(), c.clone()], &budget);
        assert_eq!(s1.kept, vec![2, 1, 0]);
        let s2 = select(&[c, b, a], &budget);
        assert_eq!(s2.kept, vec![0, 1, 2]);
        assert_eq!(s1.over_count, 0);
        assert!(!s1.arena_unpatched);
    }

    #[test]
    fn count_ceiling_keeps_the_first_keys() {
        let entries: Vec<SongEntry> = (0..10u32)
            .rev()
            .map(|i| entry("P", &format!("{i:02}"), i))
            .collect();
        let sel = select(
            &entries,
            &Budget {
                arena_bytes: EXPANDED,
                max_songs: 4,
            },
        );
        assert_eq!(sel.over_count, 6);
        let kept: Vec<&str> = sel
            .kept
            .iter()
            .map(|&i| entries[i].folder.as_str())
            .collect();
        assert_eq!(kept, ["00", "01", "02", "03"]);
        // Removing a kept pack lets the next keys in — predictable for the
        // operator.
        let fewer: Vec<SongEntry> = entries[..8].to_vec(); // folders 09..02
        let sel2 = select(
            &fewer,
            &Budget {
                arena_bytes: EXPANDED,
                max_songs: 4,
            },
        );
        let kept2: Vec<&str> = sel2
            .kept
            .iter()
            .map(|&i| fewer[i].folder.as_str())
            .collect();
        assert_eq!(kept2, ["02", "03", "04", "05"]);
    }

    #[test]
    fn default_ceiling_is_sane() {
        // Thumbnail arc at the ceiling stays under 100 MiB; the document
        // at the ceiling stays far inside the expanded arena.
        assert!(MAX_INJECTED_SONGS * 18_560 < 100 * 1024 * 1024);
        assert!(MAX_INJECTED_SONGS * 600 + 2 * 1024 * 1024 < text_budget(EXPANDED));
    }
}
