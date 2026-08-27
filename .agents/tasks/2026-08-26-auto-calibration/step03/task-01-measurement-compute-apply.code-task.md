# Task: Measurement, Decision Core, and Offset Apply

## Description
Fill the Step 2 measurement seams: a lock-free calibration tap in
power_user_statistics' `judge_submit` detour (with idempotent install so
calibration works when that mod is disabled), the pure `compute` decision core
with host tests, and the apply pipeline (autoplay guard → `set_offset(SOUND)`
→ result/failure toasts + diagnostic logs). After this task a full calibration
run works end-to-end; the cabinet demo doubles as the sign-direction
verification.

## Background
Per-step ms error exists only inside `judge_submit_hook` in
`src/mods/power_user_statistics/data_feed.rs` (one-detour-per-target rule):
grade = opcode − 0x1028 (0..=6 = M/P/G/Gd/Boo/Miss/OK), side =
`*(actor+0x84)`, ms error = `*(scratch+4)` (i32, negative = early, positive =
late; OK carries none). `install` currently fails if called twice
(`DETOUR.set` on a populated OnceLock) and is only called from the PUS mod's
`init`.

Correction model: `mean(error) ≈ real_latency − SOUND_OFFSET`
("higher = audio later"), so `new = clamp(old + round(mean), -1000, 1000)`.
The apply-time INFO log (`old / mean / count / stddev / new`) makes the
on-cabinet sign verification conclusive; a wrong sign is a one-character fix
in `compute`.

The write path already exists: `timing_offsets::set_offset(0, new)` clamps,
pushes into the live config map (master on), and persists to
`mod-config.json`; the game latches SOUND_OFFSET into the `GamePlayActor` at
ctor, so the end-of-song write is effective from the next song.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-26-auto-calibration/design/detailed-design.md
  (§2 decision core + apply path, §3 data_feed amendments; Requirements 5–6,
  10; Error Handling table)

**Additional References (if relevant to this task):**
- src/mods/power_user_statistics/data_feed.rs — the hook body being tapped
- src/mods/timing_offsets/calibration.rs — the seams to fill
  (`start_collecting` / `reset_collection` / `finish_collecting`)
- src/services/score_guard.rs — `is_stage_suppressed(side)` (autoplay taint)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `data_feed.rs` amendments:
   - `install` returns `true` immediately when `DETOUR.get().is_some()`
     (idempotent); add `pub fn is_installed() -> bool`.
   - Calibration tap statics: `CALIB_SIDE: AtomicI32` (−1 = disarmed),
     `CALIB_SUM: AtomicI64`, `CALIB_SUM_SQ: AtomicI64`, `CALIB_COUNT: AtomicU32`.
   - `pub fn calibration_arm(side: usize)` (reset counters then set side),
     `pub fn calibration_reset()` (counters only),
     `pub fn calibration_take() -> (i64, i64, u32)` or equivalent struct
     (snapshot then disarm).
   - Hook-body addition inside the existing `has_ms_error` branch: gate on
     `grade_index <= 4` (excludes Miss; OK already excluded) AND
     `player_side as i32 == CALIB_SIDE.load(Relaxed)`; then three relaxed
     `fetch_add`s. No locks, no allocation, panic-free; disarmed cost = one
     relaxed load + compare.
2. `timing_offsets::init` (in `src/mods/timing_offsets/mod.rs`) calls
   `data_feed::install(ctx.signatures)` (idempotent); store the result and
   have `calibration::enable()` refuse (WARN, no row) when the feed is
   unavailable.
3. Pure decision core in `src/mods/timing_offsets/compute.rs`
   (dependency-free; harness already mounts it):
   - `pub struct CalibStats { pub sum: i64, pub sum_sq: i64, pub count: u32 }`
   - `pub enum Outcome { Apply { new_offset: i32, mean: f64, stddev: f64 },
     RefuseTooFewSamples { count: u32 }, RefuseMeanOutOfRange { mean: f64 },
     RefuseAutoplay }`
   - `pub fn compute(stats: &CalibStats, old_offset: i32, autoplay_tainted: bool) -> Outcome`
   - Constants `MIN_SAMPLES: u32 = 30`, `MAX_ABS_MEAN_MS: f64 = 500.0`;
     autoplay refusal takes precedence; mean = sum/count; new offset =
     `(old + mean).round()` clamped to ±1000; stddev from `sum_sq` (log-only).
4. `calibration.rs` seam fills:
   - `start_collecting(side)` → `data_feed::calibration_arm(side)`.
   - `reset_collection()` → `data_feed::calibration_reset()`.
   - `finish_collecting(side)` → take stats; `autoplay =
     score_guard::is_stage_suppressed(side)`; belt-and-braces rate re-check
     (`clock_patch::snapshot().is_non_identity_commit()` ⇒ treat as refusal);
     `compute(...)`; on `Apply`: `super::set_offset(0, new_offset)`, 5 s toast
     `CALIBRATED: {old} -> {new} ({delta:+} MS)`, INFO
     `calibration: old=<> mean=<:+.1> count=<> stddev=<:.1> -> new=<>`;
     refusals: 3 s toasts `AUTOPLAY ACTIVE, CALIBRATION DISCARDED` /
     `CALIBRATION FAILED: NOT ENOUGH STEPS` /
     `CALIBRATION FAILED: TIMING TOO INCONSISTENT` + WARN.
5. Host tests in `compute.rs` (run via the existing harness): count 29/30
   boundary; |mean| 500.0 edge (inclusive apply at exactly 500.0 is NOT
   required — refuse strictly-greater); clamp at ±1000; ±0.5 rounding
   (round half away from zero); sign direction (positive mean ⇒ higher
   offset, negative ⇒ lower); stddev derivation; autoplay precedence over
   too-few-samples.

## Dependencies
- Step 2 (session lifecycle + seams) and Step 1 (toasts).

## Implementation Approach
1. TDD the `compute` core in `compute.rs` (harness runs it).
2. data_feed amendments (tap + idempotent install).
3. Fill the calibration seams; wire `init`.
4. `./scripts/validate_auto_calibration.sh`, `cargo check`, `./build.sh`.

## Acceptance Criteria

1. **End-to-end calibration (cabinet)**
   - Given SOUND OFFSET deliberately set 30 ms below a known-good value, the
     row armed, one player at 100 % rate
   - When one song is played timing to the audio
   - Then the INFO log shows old/mean/count/stddev/new, the 5 s CALIBRATED
     toast shows, the overlay SOUND OFFSET row and `mod-config.json` carry the
     new value, and the value moved back toward the known-good baseline

2. **Too few steps**
   - Given a calibration song exited after fewer than 30 judged steps
   - When gameplay ends
   - Then "CALIBRATION FAILED: NOT ENOUGH STEPS" (3 s) shows, nothing is
     written, and the row reads OFF

3. **Autoplay discarded**
   - Given autoplay active on the playing side during a calibration song
   - When gameplay ends
   - Then "AUTOPLAY ACTIVE, CALIBRATION DISCARDED" (3 s) shows and nothing is
     written

4. **Reset integrity**
   - Given a calibration song quick-restarted (or Training-Mode scrubbed)
     mid-song
   - When the song finally ends
   - Then the logged sample count reflects only post-reset steps

5. **PUS-disabled independence**
   - Given `power-user-statistics` disabled in `mod-config.json`
   - When a calibration run completes
   - Then it works identically (timing_offsets' init installed the feed)

6. **Decision-core host tests**
   - Given the harness
   - When `./scripts/validate_auto_calibration.sh` runs
   - Then all `compute` boundary/rounding/sign/stddev/precedence tests pass

7. **Hot-path discipline**
   - Given the tap disarmed (no calibration in flight)
   - When judgments stream during ordinary play
   - Then the added cost is one relaxed atomic load + compare per judgment
     (code-review criterion; no locks/allocations added to the hook body)

## Metadata
- **Complexity**: High
- **Labels**: rust, timing-offsets, data-feed, hot-path, calibration-core
- **Required Skills**: Rust atomics, this codebase's detour/hot-path rules
- **Generated By**: code-task-generator 2026-08-26
- **Source Plan**: .agents/planning/2026-08-26-auto-calibration/implementation/plan.md
- **Plan Step**: Step 3: Measurement, decision core, and offset apply (core end-to-end)
