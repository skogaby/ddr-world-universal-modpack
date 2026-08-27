# Plan — task-03 replay-arithmetic

Status: Approved 2026-08-24 (verified upstream approval chain; auto mode)

## Test scenarios

1. **Normal chart:** steps/freezes/shocks (250/12/3), BPM 65→180 core 175,
   has_chart=true, ret=1 ⇒ ints truncated correctly, shock flag 1, EX
   score (250+12+3)*3, corruption 0, u16 contributions Some(180)/Some(65).
2. **Zero-BPM (failed/empty payload):** all-zero result, ret=0,
   has_chart=false ⇒ zero ints written, flags 0, EX 0, u16 contributions
   None (skip-zero rule), corruption 0 (no chart expected).
3. **Corruption truth table:** has_chart × ret × (steps+shocks) — flag 1 iff
   has_chart && (ret==0 || steps+shocks==0). Freezes alone (steps=shocks=0,
   freezes>0) still triggers (the game sums only result[0]+result[2]).
4. **Variable-BPM threshold:** |max−min| just above / exactly at / just
   below threshold ⇒ flag 1 / 0 / 0 (strictly-greater comparison).
5. **Truncation semantics:** fractional BPMs (400.5, 65.9) truncate toward
   zero (400, 65); exact integers unchanged.
6. **result[4] flag:** >0 ⇒ 1 else 0.
7. **Accumulator fold:** fold_radar over several payloads yields per-index
   maxima; sota/thr8 gating passed by caller (`SpecialFile::{Sota, Thr8,
   None}`) — radar[0] only folds for Sota, radar[1] only for Thr8,
   radar[2..4] always.

## Implementation approach

`src/mods/fast_bootup/replay.rs` (pure part only this task):
- `pub struct SlotWrites { max_bpm: i32, core_bpm: i32, min_bpm: i32,
  shock: bool, variable_bpm: bool, flag_12e: bool, ex_score: i32,
  corrupt: bool, song_max_bpm: Option<u16>, song_min_bpm: Option<u16> }`
- `pub fn compute_slot(payload, has_chart, threshold) -> SlotWrites`
- f64 reconstruction helper from i32 bit-pairs
- `pub enum SpecialFile { None, Sota, Thr8 }` +
  `pub fn fold_radar(acc: &mut [i32; 5], radar: &[i32; 5], special: SpecialFile)`
  (acc indices 0/1 only touched under the matching special)
- Doc comments cite design §Data Models → Replay write set. No unsafe, no
  game types (the applier lands in Step 7).
