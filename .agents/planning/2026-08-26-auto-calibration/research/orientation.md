# Orientation — Auto-Calibration (StepMania-style AutoSync)

Date: 2026-08-26

## What the idea needs, and what already exists

### 1. Per-step ms error — exists, but privately owned

- The shared `judge_hook` (`src/services/judge_hook.rs`) does NOT carry per-step
  timing error — callbacks get only `(actor, music_count)` once per frame per side.
- The only per-step ms-error source is `power_user_statistics`' private
  `judge_submit` detour: `src/mods/power_user_statistics/data_feed.rs`.
  - Fields per step: grade (opcode `0x1028 + grade`, M/P/G/Gd/Boo/Miss/OK = 0..6),
    side (`actor+0x84`), `ms_error = *(scratch+4)` (i32).
  - Grades 0..5 carry ms error; OK (freeze hold, grade 6) does not.
  - Sign convention: **negative = early, positive = late**
    (`ms_error = playhead_music_count − note.music_count`).
  - Already accumulates `sum`/`count` per side per song (`MsErrorAccum`,
    data_feed.rs:43-55) — but with NO grade filtering (Miss deltas included),
    so calibration needs its own filtered accumulator.
- One-detour-per-target rule: auto-calibration cannot install a second
  `judge_submit` detour. `data_feed::install` is currently only called from the
  PUS mod's `init()` (`power_user_statistics/mod.rs:49`) and is not idempotent
  (`DETOUR.set` fails on second call, data_feed.rs:165-168). To make calibration
  independent of PUS being enabled, `install` must become idempotent
  (return true if already installed) and calibration calls it too.

### 2. Sound offset write path — exists and is exactly right

- `src/mods/timing_offsets.rs` owns the config-map setter detour.
  `pub fn set_offset(idx, value)` (line 364) clamps to [-1000, 1000], stores,
  pushes live into the game's config map (**iff master on**), and persists to
  `mod-config.json` under `timing_offsets`. `pub fn get_offset(idx)` (line 387)
  reads the configured value. SOUND is index 0.
- Liveness: all four offsets are latched into the `GamePlayActor` at ctor
  (`+0x16c` for SOUND) — changes take effect **next song**, which is exactly the
  calibration timeline (apply at song end → effective next song).
- Units: ms. Semantics: **higher = audio plays later** (stock default 87).
- If the timing-offsets master is OFF, `set_offset` stores + persists but does
  not push live (timing_offsets.rs:378-382) — a calibration applied then would
  silently not take effect. Needs a policy decision.
- The effective per-song value the engine actually used is readable at
  `GamePlayActor+0x16c` (assist_tick reads it the same way,
  `assist_tick.rs:116-120, 1067`).

### 3. Sign math (to verify in design/cabinet)

Model: `mean(ms_error) ≈ L_actual − SOUND_OFFSET` (audio-chain latency vs the
configured compensation). Player timing to the audio and hitting consistently
LATE (positive mean) means the real latency exceeds the configured offset ⇒
**increase** SOUND_OFFSET: `new = old + mean(delta_ms)`. Consistent with
"higher = audio later". Direction must be confirmed on-cabinet (single-song
test; a wrong sign is a one-character fix, and the diagnostic log should print
old/mean/new so the test is conclusive).

### 4. Toast system — exists but private and flash-only

- `src/mods/training_mode/toast.rs`: one native `TextWidget`, bottom-center
  (640, 630), amber, `pub(super) show(&'static str)` / `dismiss()`.
- Fixed animation: 100ms fade-in / 250ms hold / 300ms fade-out — a flash, not a
  persistent indicator. "Calibrating..." during a whole song needs either a
  persistent mode or repeated flashes.
- To use "the toast system we already have" from a new mod, the module must be
  promoted to a shared service (e.g. `src/services/toast.rs`) with visibility
  widened and (optionally) a persistent-show variant. Training Mode keeps its
  behavior via the same service.

### 5. Options row + mirroring + placement — all precedented

- Bool row: `RegisterSpec::bool_toggle("auto_calibration")` via
  `custom_options::register_option`. One registration covers BOTH the in-game
  MODS-tab menu and the overlay 000 menu's PLAYER SETTINGS tab (the overlay
  mirrors the custom_options registry automatically; `MenuPlacement` defaults
  to both). Placement under the "POWER USER OPTIONS" header is positional and
  operator-owned via `custom_options.option_menu_settings` in mod-config.json.
- Mirroring precedent: `per_song_judgement_offsets/ui.rs::apply_edit` — the
  editing side's `on_change` applies state for both sides and re-seeds the
  OTHER side's row via `custom_options::set_value_silent` (silent = no
  on_change loop; never re-seed the editing side).
- PersistMode: the toggle is transient by design (auto-flips OFF) —
  `PersistMode::None` (no network, no JSON cache) fits.
- Label textures: `seop_item_auto_calibration.png` + optional preview images,
  generated via `scripts/option_strings.py` + `scripts/gen_option_labels.py`
  (91-file per-language sets; DLL-only deploys leave the row label blank).

### 6. Song boundaries

- Start latch: scene callback `next == scene::GAMEPLAY` (28) — standard pattern
  (assist_tick, playfield_styling). No actors exist yet at entry.
- End: `prev == scene::GAMEPLAY` fires synchronously in `createNextSequence`
  for every exit shape incl. quick-restart/fail redirects and course
  inter-stage transitions (per_song_judgement_offsets relies on this).
- In-place quick restarts never leave scene 28 — subscribe to
  `song_reset::on_song_reset` to reset the sample accumulator.
- PUS itself resets `data_feed` buffers on scene-28 entry.

### 7. Interactions worth deciding

- **Song Playback Speed**: at rate ≠ 100%, ms errors are content-domain while
  audio latency is wall-domain — samples would be scaled by the rate.
  Calibration should refuse/skip at non-100% rates.
- **Per-song judgement offsets / JUDGEMENT TIMING (Option+0x24)**: these shift
  judgment per side but the ms error is computed before/independently? — the
  offset writes `Option+0x24` which the engine consumes in judging, so the
  measured ms error already reflects them. Calibrating while a per-song offset
  is active would fold that offset into the global value. Worth a guard or at
  least a documented caveat.
- **Autoplay** (PUS/autoplay pre @ Late): autoplay steps would calibrate to ~0
  error and wipe out the real offset. Must exclude calibration when autoplay is
  active (score_guard already tracks tainted sides).
- **Training mode**: scrubs/loops replay sections; judgments are still real.
  Simplest: allow, reset on song_reset.
- **Versus (two players)**: cabinet-level offset is global; both sides' samples
  can be pooled.

## Proposed shape

New standalone mod `src/mods/auto_calibration.rs`:
- Registers the mirrored bool row (PersistMode::None, both menus).
- Depends on: `data_feed` (made idempotent-installable + given a calibration
  tap or a grade-filtered accumulator), `timing_offsets::set_offset/get_offset`,
  shared toast service (promoted from training_mode), `scene_manager`,
  `song_reset`.
- Flow: toggle ON (either side ⇒ both) → GAMEPLAY entry latches "calibrating"
  + persistent toast → per-step filtered accumulation (exclude Miss/OK,
  exclude autoplay-tainted, refuse non-100% rate) → GAMEPLAY exit: if
  count ≥ threshold, `new_sound = clamp(old + round(mean))`,
  `timing_offsets::set_offset(SOUND, new)`, toast/log the result → flip both
  sides' rows OFF via `set_value_silent`.

## Sequence proposal

Research is largely complete (three parallel tracks done). Go straight to the
decision register (Step 3), with one residual research item folded into design:
confirming the exact sign relation from the timing-offsets RE notes, with a
cabinet verification step in the plan.
