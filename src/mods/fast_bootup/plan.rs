//! Pure boot-plan computation for the ultrafast-boot cache
//! (design §Data Models → Boot plan; §Architecture → flip eligibility).
//!
//! Decides, from the boot actor's work list plus caller-resolved cache
//! verdicts, which work items are replayed from cache and which file
//! records may be flipped from status 1 (queued) to the stock
//! "complete, empty" shape (status 6, null buffer).
//!
//! SAFETY CONTRACT (the reason this layer exists): a song's five work items
//! share one file record. Flipping a record that any stock-path item still
//! needs would make that item analyze an empty buffer — zeroed results AND
//! the game's own ME1529 corruption reporter, a hard boot blocker on real
//! hardware. The invariants enforced here make that structurally
//! impossible:
//!
//! 1. The FINAL work item is always `Stock` (design FR-7: the game's own
//!    completion block must run while processing it).
//! 2. Items with `entry_index <= 0` are always `Stock` (the game stores -1
//!    for charts whose SSQ couldn't be registered; the existing hook gates
//!    own that case).
//! 3. An entry_index lands in `flips` ONLY IF every item referencing it is
//!    `Replay` — which, combined with (1), also keeps the final item's
//!    record unflipped.
//!
//! The caller resolves what "hit" means (identity verdict + both modes'
//! payloads present) BEFORE calling; this layer is deliberately blind to
//! files and payloads so the invariants stay trivially auditable.
//!
//! **Quarantine** (injected StepMania files that failed the pre-flight
//! check, see `quarantine.rs`): such items become `Quarantine` — the hook
//! releases and advances them without ever calling the original, so the
//! stock analysis (and its `ME1529` reporter) never sees the bad bytes.
//! For flip eligibility a `Quarantine` item counts like `Replay` (nothing
//! reads its buffer). The final item stays `Stock` regardless; when it
//! belongs to a quarantined file and the caller can run it with the
//! entry's `hasChart` answered "no" (`final_neuter`), it counts as
//! buffer-free too, so the bad file is never loaded at all.
//! Dependency-free — host-tested via `scripts/validate_fast_bootup.sh`.

use std::collections::HashMap;

/// One work item's planning input: its record index plus the caller's
/// already-resolved cache verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedInput {
    /// The step-data record index from the work item (`{+0}`); the game
    /// uses `-1` for unregistered charts.
    pub entry_index: i32,
    /// True iff the item's file identity verified AND the cache holds both
    /// modes' payloads for the item's difficulty.
    pub hit: bool,
    /// True iff the item's file is an injected SSQ that failed the
    /// pre-flight check (set for every item of that file). Wins over `hit`.
    pub quarantine: bool,
}

/// Per-item outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemPlan {
    /// Inject cached outputs; never call the original for this item.
    Replay,
    /// Existing gated stock path (load + analyze + capture).
    Stock,
    /// Bad injected file: release + advance only; never call the original.
    Quarantine,
}

/// The computed plan for one boot pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BootPlan {
    /// One entry per work item, same order as the work list.
    pub items: Vec<ItemPlan>,
    /// Record indices eligible for the 1→6 status flip.
    pub flips: Vec<i32>,
    /// The final (always `Stock`) item belongs to a quarantined file and
    /// must run with `hasChart` neutered.
    pub final_quarantined: bool,
}

impl BootPlan {
    /// Count of items planned for replay (for the one-shot boot log line).
    pub fn replay_count(&self) -> usize {
        self.items
            .iter()
            .filter(|p| matches!(p, ItemPlan::Replay))
            .count()
    }

    /// Count of items planned for quarantine.
    pub fn quarantine_count(&self) -> usize {
        self.items
            .iter()
            .filter(|p| matches!(p, ItemPlan::Quarantine))
            .count()
    }
}

/// Compute the plan. See the module docs for the enforced invariants.
/// `final_neuter`: the caller can run the final item with the entry's
/// `hasChart` neutered (derived vtable slot available).
pub fn compute(inputs: &[PlannedInput], final_neuter: bool) -> BootPlan {
    let last = match inputs.len().checked_sub(1) {
        Some(last) => last,
        None => return BootPlan::default(),
    };

    let items: Vec<ItemPlan> = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| {
            if input.entry_index <= 0 || i == last {
                ItemPlan::Stock
            } else if input.quarantine {
                ItemPlan::Quarantine
            } else if input.hit {
                ItemPlan::Replay
            } else {
                ItemPlan::Stock
            }
        })
        .collect();

    let final_quarantined = inputs[last].entry_index > 0 && inputs[last].quarantine;

    // A record is flip-eligible iff no item referencing it reads its buffer:
    // every item is Replay or Quarantine, or it is the final item of a
    // quarantined file that will run neutered.
    let mut buffer_free: HashMap<i32, bool> = HashMap::new();
    for (i, (input, plan)) in inputs.iter().zip(&items).enumerate() {
        if input.entry_index <= 0 {
            continue;
        }
        let free = match plan {
            ItemPlan::Replay | ItemPlan::Quarantine => true,
            ItemPlan::Stock => i == last && final_quarantined && final_neuter,
        };
        let entry = buffer_free.entry(input.entry_index).or_insert(true);
        *entry &= free;
    }
    let mut flips: Vec<i32> = buffer_free
        .into_iter()
        .filter_map(|(idx, all)| all.then_some(idx))
        .collect();
    flips.sort_unstable();

    BootPlan {
        items,
        flips,
        final_quarantined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(entry_index: i32, hit: bool) -> PlannedInput {
        PlannedInput {
            entry_index,
            hit,
            quarantine: false,
        }
    }

    fn bad(entry_index: i32) -> PlannedInput {
        PlannedInput {
            entry_index,
            hit: false,
            quarantine: true,
        }
    }

    /// One song = five items sharing a record, like onInit builds.
    fn song(entry_index: i32, hit: bool) -> Vec<PlannedInput> {
        vec![item(entry_index, hit); 5]
    }

    #[test]
    fn final_item_always_stock() {
        let mut inputs = song(7, true);
        inputs.extend(song(9, true));
        let plan = compute(&inputs, true);
        assert_eq!(plan.items.len(), 10);
        assert_eq!(plan.items[9], ItemPlan::Stock, "final item must be stock");
        assert!(plan.items[..9].iter().all(|p| *p == ItemPlan::Replay));
        // Record 7 (fully replayed) flips; record 9 (final item stock) must not.
        assert_eq!(plan.flips, vec![7]);
    }

    #[test]
    fn shared_record_mixed_hit_never_flips() {
        let mut inputs = song(7, true);
        inputs[2].hit = false; // one difficulty missed
        inputs.extend(song(9, true)); // fully-hit song, flippable
        inputs.extend(song(11, true)); // trailing song absorbs the final item
        let plan = compute(&inputs, true);
        assert_eq!(plan.items[2], ItemPlan::Stock);
        assert_eq!(plan.items[0], ItemPlan::Replay);
        assert!(!plan.flips.contains(&7), "mixed song must not flip");
        assert!(
            !plan.flips.contains(&11),
            "final item's record must not flip"
        );
        assert_eq!(plan.flips, vec![9]);
    }

    #[test]
    fn all_hit_single_song_keeps_record_unflipped() {
        let plan = compute(&song(7, true), true);
        assert_eq!(plan.items[4], ItemPlan::Stock);
        assert!(plan.items[..4].iter().all(|p| *p == ItemPlan::Replay));
        assert!(plan.flips.is_empty(), "final item shares the only record");
    }

    #[test]
    fn split_files_flip_independently() {
        // Five distinct records (split-chart song), then a trailing song.
        let mut inputs: Vec<_> = (11..16).map(|idx| item(idx, true)).collect();
        inputs.extend(song(20, true));
        let plan = compute(&inputs, true);
        assert_eq!(plan.flips, vec![11, 12, 13, 14, 15]);
    }

    #[test]
    fn unregistered_entries_always_stock_never_flip() {
        let mut inputs = vec![item(0, true), item(-1, true)];
        inputs.extend(song(5, true));
        let plan = compute(&inputs, true);
        assert_eq!(plan.items[0], ItemPlan::Stock);
        assert_eq!(plan.items[1], ItemPlan::Stock);
        assert!(!plan.flips.contains(&0) && !plan.flips.contains(&-1));
    }

    #[test]
    fn miss_items_are_stock() {
        let mut inputs = song(3, false);
        inputs.extend(song(4, true));
        let plan = compute(&inputs, true);
        assert!(plan.items[..5].iter().all(|p| *p == ItemPlan::Stock));
        assert!(!plan.flips.contains(&3));
    }

    #[test]
    fn empty_list() {
        let plan = compute(&[], true);
        assert!(plan.items.is_empty() && plan.flips.is_empty());
        assert_eq!(plan.replay_count(), 0);
    }

    #[test]
    fn quarantined_song_mid_list_flips_and_never_stock() {
        let mut inputs = song(3, true);
        inputs.extend(vec![bad(4); 5]);
        inputs.extend(song(5, true));
        let plan = compute(&inputs, false);
        assert!(plan.items[5..10].iter().all(|p| *p == ItemPlan::Quarantine));
        assert_eq!(plan.quarantine_count(), 5);
        assert!(plan.flips.contains(&4), "bad file must never load");
        assert!(!plan.final_quarantined);
    }

    #[test]
    fn quarantine_wins_over_hit() {
        let mut inputs = vec![PlannedInput {
            entry_index: 4,
            hit: true,
            quarantine: true,
        }];
        inputs.extend(song(5, true));
        let plan = compute(&inputs, true);
        assert_eq!(plan.items[0], ItemPlan::Quarantine);
    }

    #[test]
    fn quarantined_final_song_stays_stock_and_flips_only_when_neutered() {
        let mut inputs = song(3, true);
        inputs.extend(vec![bad(9); 5]);
        let neutered = compute(&inputs, true);
        assert_eq!(
            neutered.items[9],
            ItemPlan::Stock,
            "final item must be stock"
        );
        assert!(neutered.items[5..9]
            .iter()
            .all(|p| *p == ItemPlan::Quarantine));
        assert!(neutered.final_quarantined);
        assert!(neutered.flips.contains(&9));

        let plain = compute(&inputs, false);
        assert_eq!(plain.items[9], ItemPlan::Stock);
        assert!(plain.final_quarantined);
        assert!(
            !plain.flips.contains(&9),
            "un-neutered final item still reads its buffer"
        );
    }

    #[test]
    fn mixed_quarantine_and_stock_record_never_flips() {
        // A miss item on the same record as a quarantined one (caller bug):
        // the stock item reads the buffer, so the record must stay loaded.
        let mut inputs = vec![bad(4), item(4, false)];
        inputs.extend(song(5, true));
        let plan = compute(&inputs, true);
        assert!(!plan.flips.contains(&4));
    }

    #[test]
    fn unregistered_quarantine_input_is_stock() {
        let mut inputs = vec![bad(-1)];
        inputs.extend(song(5, true));
        let plan = compute(&inputs, true);
        assert_eq!(plan.items[0], ItemPlan::Stock);
        assert!(!plan.flips.contains(&-1));
    }
}
