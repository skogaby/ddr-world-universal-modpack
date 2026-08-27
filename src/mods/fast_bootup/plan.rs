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
}

/// Per-item outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemPlan {
    /// Inject cached outputs; never call the original for this item.
    Replay,
    /// Existing gated stock path (load + analyze + capture).
    Stock,
}

/// The computed plan for one boot pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BootPlan {
    /// One entry per work item, same order as the work list.
    pub items: Vec<ItemPlan>,
    /// Record indices eligible for the 1→6 status flip.
    pub flips: Vec<i32>,
}

impl BootPlan {
    /// Count of items planned for replay (for the one-shot boot log line).
    pub fn replay_count(&self) -> usize {
        self.items
            .iter()
            .filter(|p| matches!(p, ItemPlan::Replay))
            .count()
    }
}

/// Compute the plan. See the module docs for the enforced invariants.
pub fn compute(inputs: &[PlannedInput]) -> BootPlan {
    let last = match inputs.len().checked_sub(1) {
        Some(last) => last,
        None => return BootPlan::default(),
    };

    let items: Vec<ItemPlan> = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| {
            if input.hit && input.entry_index > 0 && i != last {
                ItemPlan::Replay
            } else {
                ItemPlan::Stock
            }
        })
        .collect();

    // A record is flip-eligible iff every item referencing it is Replay.
    let mut all_replay: HashMap<i32, bool> = HashMap::new();
    for (input, plan) in inputs.iter().zip(&items) {
        if input.entry_index <= 0 {
            continue;
        }
        let entry = all_replay.entry(input.entry_index).or_insert(true);
        *entry &= matches!(plan, ItemPlan::Replay);
    }
    let mut flips: Vec<i32> = all_replay
        .into_iter()
        .filter_map(|(idx, all)| all.then_some(idx))
        .collect();
    flips.sort_unstable();

    BootPlan { items, flips }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(entry_index: i32, hit: bool) -> PlannedInput {
        PlannedInput { entry_index, hit }
    }

    /// One song = five items sharing a record, like onInit builds.
    fn song(entry_index: i32, hit: bool) -> Vec<PlannedInput> {
        vec![item(entry_index, hit); 5]
    }

    #[test]
    fn final_item_always_stock() {
        let mut inputs = song(7, true);
        inputs.extend(song(9, true));
        let plan = compute(&inputs);
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
        let plan = compute(&inputs);
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
        let plan = compute(&song(7, true));
        assert_eq!(plan.items[4], ItemPlan::Stock);
        assert!(plan.items[..4].iter().all(|p| *p == ItemPlan::Replay));
        assert!(plan.flips.is_empty(), "final item shares the only record");
    }

    #[test]
    fn split_files_flip_independently() {
        // Five distinct records (split-chart song), then a trailing song.
        let mut inputs: Vec<_> = (11..16).map(|idx| item(idx, true)).collect();
        inputs.extend(song(20, true));
        let plan = compute(&inputs);
        assert_eq!(plan.flips, vec![11, 12, 13, 14, 15]);
    }

    #[test]
    fn unregistered_entries_always_stock_never_flip() {
        let mut inputs = vec![item(0, true), item(-1, true)];
        inputs.extend(song(5, true));
        let plan = compute(&inputs);
        assert_eq!(plan.items[0], ItemPlan::Stock);
        assert_eq!(plan.items[1], ItemPlan::Stock);
        assert!(!plan.flips.contains(&0) && !plan.flips.contains(&-1));
    }

    #[test]
    fn miss_items_are_stock() {
        let mut inputs = song(3, false);
        inputs.extend(song(4, true));
        let plan = compute(&inputs);
        assert!(plan.items[..5].iter().all(|p| *p == ItemPlan::Stock));
        assert!(!plan.flips.contains(&3));
    }

    #[test]
    fn empty_list() {
        let plan = compute(&[]);
        assert!(plan.items.is_empty() && plan.flips.is_empty());
        assert_eq!(plan.replay_count(), 0);
    }
}
