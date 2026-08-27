# Implementation Plan — Auto-Calibration (Timing Offsets)

Status: Approved 2026-08-26

Design: `.agents/planning/2026-08-26-auto-calibration/design/detailed-design.md`
(Approved 2026-08-26). This plan decomposes it; design detail is not restated.

## Checklist

- [x] Step 1: Promote the toast to a shared service with pulse and hold modes
- [x] Step 2: Calibrate row, arm state, and session lifecycle (consume-only)
- [x] Step 3: Measurement, decision core, and offset apply (core end-to-end)
- [x] Step 4: Hide judgement overlays and suppress PUS readouts
- [x] Step 5: Documentation and full cabinet regression sweep

---

## Step 1: Promote the toast to a shared service with pulse and hold modes

**Objective:** `src/services/toast.rs` exists with the full API
(`flash`, `flash_with_hold`, `show_pulsing`, `dismiss`), owned-`String` text,
and the `ToastMode { Flash { hold_ms }, Pulse }` animation curves;
`src/mods/training_mode/toast.rs` is deleted and its call sites migrated.

**Implementation guidance:** Move the module (design §6): keep the widget
lifecycle, position, colors, generation-token supersession, and the
no-lock-across-schedule discipline byte-for-byte where possible; the changes
are `text: String`, the mode enum, the generalized fade evaluator (pulse loop
never returns `None`), and the three public constructors. Update
`src/services/mod.rs`, `src/mods/training_mode/bounds.rs` (3 call sites →
`toast::flash`), `src/mods/training_mode/mod.rs` (`dismiss`). Extract the fade
evaluators as pure functions of `(mode, elapsed_ms)`.

**Tests:** Host tests for the pure curves — flash at default/3000/5000 ms
holds (boundary alphas, terminal `None`), pulse periodicity (alpha at
0/800/1600/2400/2800 ms, never `None`). Create
`scripts/validate_auto_calibration.sh` now, following the temp-crate harness
pattern of `scripts/validate_judgement_offsets.sh` (plain `cargo test` cannot
compile `retour` on ARM hosts); it runs the toast-curve tests in this step and
grows in Step 3.

**Integration:** `cargo check` + `./build.sh` clean; no behavior change
anywhere except the new (unused) API.

**Demo:** On the cabinet, Training Mode marker gestures still flash their
toasts exactly as before (set/clear markers). Host: the validation script
passes.

## Step 2: Calibrate row, arm state, and session lifecycle (consume-only)

**Objective:** The timing-offsets mod is restructured
(`src/mods/timing_offsets.rs` → `src/mods/timing_offsets/mod.rs` +
`calibration.rs`), the "Calibrate next song?" row renders at the top of its
GLOBAL SETTINGS section, and the full arm/consume lifecycle works — entry
guards (side census, rate), refusal toasts, and the always-flip-OFF rule —
with measurement stubbed out.

**Implementation guidance:** Design §1–§2. The git move first (pure rename
commit-sized change, no content edits), then `calibration.rs` with: `ARMED` +
`Session` state, `enable()`/`disable()`, the `EnumRowSpec` registration
(before the four scalar rows), the idempotent re-registration flip-OFF helper,
the scene callback (entry: guards → `Collecting`/`ConsumeOnly` + pulsing
toast / 3 s refusal toasts; exit: dismiss + flip OFF — the apply branch just
logs "measurement not yet implemented"), and the `song_reset` subscription
(no-op body for now). Gate `enable()` on scene_manager/song_reset
availability. Keep the entered-side census as a pure function of the two
`Option<bool>`s for host testing.

**Tests:** Host tests (extend the Step 1 script) for the census function
(one/two/zero/None combinations → side / refusal reason) — added here with the
functionality.

**Integration:** Builds on Step 1 (toasts). The row's `on_change`, the scene
callback, and `remove_rows_for` on disable are all live.

**Demo:** On the cabinet — arm the row in the 000 menu; play a 1P song:
pulsing "Calibrating..." for the whole song, row reads OFF afterwards. Play a
2P song armed: 3 s "2P MODE DETECTED, CALIBRATION DISABLED" at song start, row
OFF afterwards. Arm + song speed ≠ 100 %: the rate refusal toast. Arm and back
out without playing: row stays ON.

## Step 3: Measurement, decision core, and offset apply (core end-to-end)

**Objective:** A full calibration run works: per-step errors accumulate,
`compute` decides, the offset is written via `set_offset`, and the result/
failure toasts + logs fire. This step carries the design's one deliberate
unknown (sign direction) to cabinet verification.

**Implementation guidance:** Design §2 (decision core + apply path) and §3
(data_feed): idempotent `install` + `is_installed`, the
`CALIB_SIDE/SUM/SUM_SQ/COUNT` tap in the hook body (grade ≤ 4, side match,
relaxed atomics only), `calibration_arm/reset/take`; timing-offsets `init`
calls `install`. In `calibration.rs`: `CalibStats`, `Outcome`, `compute`
(MIN_SAMPLES 30, MAX_ABS_MEAN_MS 500, round + clamp), the apply wrapper
(autoplay read via `score_guard::is_stage_suppressed`, `get_offset`/
`set_offset`, 5 s success toast, 3 s failure toasts, INFO with
count/mean/stddev), and the real `song_reset` body
(`calibration_reset`).

**Tests:** Host tests (same script) for `compute`: count 29/30 boundary, mean
±500.0 edge, clamp at ±1000, ±0.5 rounding, sign direction (positive mean
raises the offset), stddev derivation, autoplay precedence over sample-count.

**Integration:** Fills the Step 2 stub; data_feed tap is armed/taken by the
Step 2 session lifecycle; toasts from Step 1.

**Demo (cabinet, includes the sign test):** Set SOUND_OFFSET 30 ms below the
known-good value via the overlay row. Arm, play one song timing to the audio.
Verify: INFO log `calibration: old=.. mean=.. count=.. stddev=.. -> new=..`,
5 s `CALIBRATED: X -> Y (+Z MS)` toast, the overlay scalar row and
`mod-config.json` show the new value, and the value moved back toward the
baseline (if it moved away, flip the sign in `compute` and re-verify). Also:
a deliberately sparse song (< 30 steps) → "NOT ENOUGH STEPS"; a quick restart
mid-song → the logged count covers only post-restart steps; calibration runs
with `power-user-statistics` disabled in `mod-config.json`.

## Step 4: Hide judgement overlays and suppress PUS readouts

**Objective:** During a `Collecting` song, all judgement-feedback surfaces are
invisible: overlay clips (judge, freeze O.K./N.G., FAST/SLOW, combo,
pacemaker) at opacity 0 via the styling-mod override, and the PUS timing-stats
widget + pacemaker→ms-error swap suppressed.

**Implementation guidance:** Design §4 (`CALIBRATION_HIDE` consulted in
`opacity_pct` / `opacity_pct_fast`; `set_calibration_hide(on) -> bool`
reporting liveness; cleared in the styling mod's `disable()`) and §5
(`CALIBRATION_SUPPRESS` + the two consumer checks in
`timing_stats_widget::update_text` and `pacemaker_swap`). Wire both into the
Step 2 session transitions (set on entry alongside the pulsing toast — before
clips bind; clear on exit and in `calibration::disable()`); one WARN when the
hide reports not-live (D18 fail-open).

**Tests:** No new pure logic beyond trivial flag checks; covered by the
cabinet demo. (The override read paths are hot — keep them a single relaxed
load, mirroring the tap.)

**Integration:** Pure addition onto the Step 2/3 session; no change to
measurement.

**Demo (cabinet):** Calibration song with styling values at defaults and
`timing_stats` ON: no judgements, no freeze O.K./N.G., no FAST/SLOW, no combo,
no pacemaker, no timing widget — only arrows and the pulsing toast. Next
(non-calibration) song: everything back, styling values intact. With
`overlay-element-styling` disabled: calibration still completes, WARN in log,
overlays visible.

## Step 5: Documentation and full cabinet regression sweep

**Objective:** The feature is documented for future agents and operators, and
the complete cabinet checklist from the design's Testing Strategy has passed.

**Implementation guidance:** Add the auto-calibration row to the AGENTS.md Key
Entry Points table (timing_offsets entry gains the calibration summary:
mechanism, guards, D16 one-rule, D18/D19 hides, sign model, validation
script). Update `docs/README.md` (operator-facing) with the feature
description and usage. Record the cabinet-verified sign direction in the
design doc's requirement 10 (re-date the approval marker if the sign flipped).

**Tests:** Re-run `scripts/validate_auto_calibration.sh`; `cargo check` +
`cargo fmt` + `./build.sh` readiness gates.

**Integration:** Final sweep of the design's cabinet checklist items not yet
covered: toast bleed check at results, arm-consumption matrix (success /
too-few / 2P / rate), Training Mode scrub reset, timing-offsets scalar rows
regression, Training Mode toast regression, no stuck opacity/suppression on
the song after a calibration.

**Demo:** The full regression checklist recorded in the feature's
`progress.md` deploy log, with the calibrated cabinet playing in sync.
