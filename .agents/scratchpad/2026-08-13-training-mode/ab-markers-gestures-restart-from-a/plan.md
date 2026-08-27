# Plan: ab-markers-gestures-restart-from-a

Status: Approved 2026-08-13 (verified upstream approval — same chain as tasks 01–03)

## Implementation shape

1. **`song_reset::current_raw_music_count()`** — `GPA_RAW_COUNT_OFFSET = 0x178`
   constant + accessor (live DPS → first GamePlayActor → i32 read, sanity
   range −60_000..=3_600_000; the clock is shared across sides).
2. **`src/mods/training_mode/bounds.rs`** (new):
   - Marker state: `A_MS`/`B_MS` AtomicI32 (0 = none, design data model).
   - `set_marker_a()/set_marker_b()/clear_markers(reason)` — A from
     `current_raw_music_count()` quantized via the seek composition
     (`seek::wall_ms` → `seek::quantize_seek(active_content_grid)` →
     `seek::content_ms`; grid missing ⇒ raw), clamped below
     `min(chart_end_raw(0), chart_end_raw(1)) − 1000` when available; one
     INFO per accepted gesture.
   - `active_section_start() -> Option<i32>` (Some iff a_ms > 0).
   - Per-side `GestureBuffer` triplet-buffers (quick_logout's shape) keyed by
     button (7/9/5); `on_input_event` gated on Pressed + scene 28 + mod
     ACTIVE; `on_scene_change` clears markers + buffers on gameplay
     entry/exit.
3. **`training_mode/mod.rs`**: `pub mod bounds;` + re-export
   `active_section_start`; enable() registers input + scene callbacks
   (ids stored on the struct, removed in disable()); REMOVE the
   `DDR_TRAINING_TEST_SHIFT_MS` block (disable()'s initial-mapping clear
   stays).
4. **`quick_restart_or_fail::trigger_restart`**: before the shipped
   reset call, consult `training_mode::bounds::active_section_start()`;
   when set: `request_reset(a_ms, max(TRAINING_LEAD_MS, restart_delay),
   Zero, Some(restart_reset_recovery))` — Started ⇒ log + return;
   Refused ⇒ one WARN + fall THROUGH to the shipped restart-at-0 chain
   (in-place reset at 0 → scene-jump fast path → natural death), which is
   exactly the R6 ladder.

## Validation
- Gates: harness (unchanged suites) → cargo check → fmt → `./build.sh`.
- Cabinet (maintainer-run Step-2 demo): mid-song triple-7 sets A; triple-1
  restarts at A after the 2.5 s silent approach — combo/score reset, claps
  aligned — at 100 %, 75 %, 125 %; triple-5 clears; triple-9 at song select
  still logs out; seek-refusal fallback observable by disabling the binding
  (e.g. non-eligible session). NOTE: score containment arrives in Step 5 —
  a seek-practiced song's score WILL submit during this demo.

## Risks
- Gesture buffers keyed per button per side (3 buttons × 2 sides): use a
  small fixed array; no allocation in the input callback beyond the
  VecDeque the precedent already uses.
