# Plan — task-04 boot-plan

Status: Approved 2026-08-24 (verified upstream approval chain; auto mode)

## Test scenarios

1. **Final item always stock:** all-hit 2-song list ⇒ last item Stock, its
   record absent from flips; every other item Replay; the other song's record
   flipped.
2. **Shared-record safety:** one song, items 0..4 same entry_index, item 2 a
   miss ⇒ items keep individual verdicts (4 Replay, 1 Stock) but the record
   is NOT in flips.
3. **Final-item contamination:** all-hit single song (its items are the tail
   of the list) ⇒ all its non-final items Replay, final Stock, record NOT
   flipped (final item shares it).
4. **Split-file independence:** 5 items with 5 distinct entry_indexes, all
   hit, none final (another song follows) ⇒ each of the 5 records flipped.
5. **Unregistered charts:** entry_index 0 and -1 ⇒ Stock, never flipped,
   even when the verdict map says hit.
6. **Miss files:** items whose game_path has no verdict / Miss verdict /
   missing payload for (difficulty, mode-pair) ⇒ Stock.
7. **Empty list** ⇒ empty plan (degenerate safety).

## Implementation approach

`src/mods/fast_bootup/plan.rs`, pure:
- Inputs kept game-free: `WorkItem { entry_index: i32, difficulty: i32,
  mcode: i32 }` plus a caller-resolved per-item file key
  `Option<usize>` → index into a slice of `FileVerdict { hit: bool,
  has_payload_for: fn-free bitmap }`. Simplest faithful shape: caller passes
  `items: &[WorkItem]`, `item_files: &[Option<u32>]` (per-item file id) and
  `hit_lookup: &dyn Fn(u32, i32) -> bool` — but house style avoids closures
  in pure layers; use a prepared `Vec<ItemVerdict>` instead:
  `plan::compute(items: &[PlannedInput]) -> BootPlan` where
  `PlannedInput { entry_index: i32, hit: bool }` — the caller (Step 7)
  resolves hits (identity verdict + payload presence) before calling.
  Keeps this layer purely about the SAFETY invariants (grouping, final item,
  entry_index domain).
- `BootPlan { items: Vec<ItemPlan>, flips: Vec<i32> }`,
  `ItemPlan::{Replay, Stock}`.
- Algorithm: mark Stock for !hit, entry_index<=0, or last index; group item
  indices by entry_index; flip = groups where all items Replay.
