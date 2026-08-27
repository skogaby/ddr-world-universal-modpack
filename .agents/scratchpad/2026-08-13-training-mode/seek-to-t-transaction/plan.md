# Plan: seek-to-t-transaction

Status: Approved 2026-08-13 (verified upstream approval — same chain as tasks 01–02)

## Implementation shape

### 1. Pure additions — `song_reset/seek.rs` (TDD, harness)
- `wall_ms(t_ms, snapshot) -> i32`: content→wall (identity ⇒ t, the tick_domain
  selector; failure ⇒ identity fallback).
- `content_ms(wall_ms, snapshot) -> i32`: wall→content via the inverted exact
  ratio (identity ⇒ wall).
- Tests: identity pins; 50 % round trips (`content(wall(t)) == t` for exact
  values); anchor consistency (`anchor_tick(now, 0, content_ms(W_q), s)` within
  1 ms of `now − W_q` — the documented integer-domain slop).

### 2. Signatures — `src/core/signatures.rs`
- New AOB `judge_rebuild_anchor`:
  `48 89 91 60 01 00 00 C7 81 90 01 00 00 FF FF FF FF` (the 0x1044 rewind
  worker's anchor stores; unique + byte-stable on 20260616/20260721).
- `derive_judge_rebuild_trio()`: from the match, scan forward ≤ 0x60 bytes for
  the first THREE `E8` sites (the flash-renderer call between is `FF 50 10`,
  never E8); validate the first is immediately preceded by
  `49 8D 8C 24 B0 00 00 00` (`LEA RCX,[R12+0xB0]` — the records vector) and all
  three targets are distinct and in-module ⇒ insert `judge_rebuild_clear`,
  `judge_rebuild_reserve`, `judge_rebuild_rebuild`. Any check fails ⇒ one `[-]`
  WARN, nothing inserted (nonzero-T seeks refuse; t=0 unaffected).
- CMA vtable: add `(".?AVControlMessageActor@dance@sequence@@",
  "control_message_actor_vtable")` to the `find_gauge_vtables` RTTI table
  (rename-scope note: keep the fn, it is the "gameplay actor children" set).

### 3. Runtime accessor — `song_rate`
- `binding.rs`: `BindingRegistry::active_content_grid(&self) -> Option<ContentGrid>`
  (`ContentGrid { samples_per_block, sample_rate, stream_blocks }` from the
  active binding's main-entry format + `streamed.data_len / block_align`).
- `runtime.rs`: thin `pub fn active_content_grid()` wrapper. Doubles as the
  "binding live" preflight.

### 4. Transaction — `song_reset/mod.rs`
- `pub enum AccumulatorPolicy { Zero, Keep }`;
  `request_reset(t_ms, delay_ms, policy, on_recovery)`. `Keep` ⇒ WARN-once +
  `Refused` (v1). t=0 flow untouched beyond the signature.
- Seek constants: CMA field offsets (+0x58/+0x82 step machine — reuse
  `read_step`; `+0x94` display end; `+0x98` raw end), `CMA_STEP_RAW_END_FIRED = 5`,
  refuse at step ≥ 4; `SEEK_END_MARGIN_MS = 1000`; note-vector offsets +0x90/+0x98;
  records vector +0xB0; counts +0x194/+0x198/+0x19C (all documented from the
  live decompile).
- New gates (after the shipped Phase-0 set, before any mutation): trio + CMA
  vtable resolved; `active_content_grid()` Some; per-actor CMA child found,
  step < 4, `chart_end_raw` sane; quantize (wall domain) → `t_q <
  min(chart_end_raw) − MARGIN` else Refused.
- Transaction (single driver, no countdown): stop → `set_content_mapping(shift,
  lead)` (false here ⇒ WARN + recovery — the song is stopped; the preflight
  makes this a race-only path) → replay → `driver_step` with a `SeekPlan
  { t_q, delay_wall_ms }` → on prepared + revalidated: `perform_seek`.
- `perform_seek(dps, actors, snapshot_sides, plan)`:
  1. `0x1043` broadcast; `0x1044 {seek::anchor_tick(now, delay, t_q, rate_snapshot)}`
     broadcast (engine rebuild at 0 + re-anchor);
  2. per actor: trio at `t_q` (clear → reserve(sum of counts) → rebuild with
     `ctx {actor, t_q}`, out = 2 stack qwords);
  3. neutralization: read note bytes (+0x90..+0x98, size sanity ≤ 4 MiB +
     stride check), `seek::decode_notes` + `seek::neutralization_writes`,
     apply to records base (+0xB0) with per-write bounds check against the
     vector's live end (+0xB8);
  4. policy Zero: the shipped accumulator/gauge/HUD block — extracted as
     `reset_side_state(dps, actors, snapshot)` and shared with `perform_reset`
     (t=0 stays bit-identical by construction);
  5. `notify_subscribers(t_q)` (parameterized; t=0 paths pass 0).
- Accessors for task-04/Steps 3–4: `pub fn chart_end_raw(actor: side) ->
  Option<i32>` (live walk) + `pub fn seek_available() -> bool`.
- `quick_restart_or_fail.rs`: add `AccumulatorPolicy::Zero` at the single call.

## Test scenarios
- Harness: the new conversion helpers (identity pin, 50 % exactness, round-trip
  slop bound) — everything else is engine-facing (AC-1/2/3/4 validated on the
  cabinet via task-04's demo; AC-2's Refused legs observable in logs).
- Full suite + check + fmt.

## Risks
- Trio E8 scan false-positive: mitigated by the LEA-prefix pin + distinct
  in-module target validation + the byte-identical cross-build evidence.
- reserve() reallocates on the game heap: called on the frame thread inside the
  engine's own allocator context (exactly what the 0x1044 handler does) — safe.
- The 0x1044 broadcast rebuilds at 0 first (wasted work, engine-owned), then we
  rebuild at t_q — mirrors the design's step 4 ordering; no double-notify.
