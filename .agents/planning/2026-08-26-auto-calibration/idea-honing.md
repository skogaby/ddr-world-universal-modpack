# Idea Honing — Auto-Calibration

Decision register. Status: Proposed / Accepted / Overridden / Assumed / Open.

Readiness Confirmed 2026-08-26 — all decisions Accepted (D16 resolved: refusals
consume the arm; one rule — any song end while ON flips it OFF).

| ID | Decision | Why it matters | Resolution | Status |
|----|----------|----------------|----------------|--------|
| D1 | Mod placement & data source | Lifecycle coupling, one-detour rule | Sub-feature of the **timing-offsets mod** (`src/mods/timing_offsets/` gains `calibration.rs`); `data_feed::install` made idempotent, called from timing_offsets too | Overridden → Accepted |
| D2 | Offset write path & dependency | Whether calibration silently no-ops | Internal to timing_offsets: `set_offset(SOUND, …)` directly; disabled mod ⇒ row hidden, nothing arms — failure mode designed away | Accepted (amended) |
| D3 | Correction formula & sign | Core correctness | `new = clamp(old + round(mean(delta_ms)))`; cabinet-verify direction with diagnostic logs | Accepted |
| D4 | Sample source & filter | Skew from misses/holds | Grades M/P/G/Gd/Boo (0–4) only; exclude Miss, OK; **single playing side only** (see D16) | Accepted (amended) |
| D5 | Minimum samples & always-flip-OFF | Garbage-in protection, predictable UX | Apply only if ≥ 30 samples; toggle ALWAYS flips OFF at song end | Accepted |
| D6 | Refusal guards | Corrupt calibrations | Refuse at song rate ≠ 100% (3 s toast at song start); exclude autoplay-tainted side; training mode allowed | Accepted |
| D7 | Sanity bound | Protect cabinet from a garbage run | Refuse apply if abs(mean) > 500 ms (failure toast + WARN) | Accepted |
| D8 | Toast promotion & animation | "Use the toast system we have" | Promote to `services/toast.rs`; **pulsing** "Calibrating..." (slow fade in/out loop) for the song's duration; 5 s result toast; 3 s refusal toasts; owned-String support | Accepted (amended) |
| D9 | Option surface | UI placement | **No custom_options row.** Overlay GLOBAL SETTINGS enum row (OFF/ON) "Calibrate next song?" at the top of the timing-offsets section; no persistence, no mirroring, no textures, not in the in-game menu | Overridden → Accepted |
| D10 | Mid-song toggle semantics | Edge behavior | Latch at GAMEPLAY entry only; mid-song enable applies next song | Accepted |
| D11 | What the mean measures | Conceptual scope | The whole latency chain; folding the net error into SOUND_OFFSET is the intent | Accepted |
| D12 | Quick restart / training scrub | Sample integrity | `song_reset::on_song_reset` ⇒ reset accumulator; apply at final GAMEPLAY exit | Accepted |
| D13 | Boot default | Safety | Toggle always OFF at boot (in-memory only, never persisted) | Accepted |
| D14 | Result feedback | Player confidence | 5 s toast `CALIBRATED: 87 -> 93 (+6 MS)` + INFO log (count/mean/stddev); reason-specific failure toasts | Accepted |
| D15 | Per-song judgement offsets interplay | Representative measurement | Per-song offsets and JUDGEMENT TIMING apply during calibration **by design** (they're additive master-data corrections); no warning, no guard | Accepted (amended) |
| D16 | 2P play & arm consumption | Predictability | 2P detected ⇒ calibration disabled, 3 s toast "2P MODE DETECTED, CALIBRATION DISABLED"; the arm is still consumed (flips OFF at song end) — ONE rule: any song end while ON clears it | Accepted |
| D17 | Side detection | Implementation anchor | Entered sides via `stage_records`/`player_work` (announcer_mute precedent); samples keyed by `actor+0x84` side must match the single entered side | Assumed |
| D18 | Hide judgement overlays during calibration | Visible judgements bias the player toward the CURRENT offset | Per-song opacity override (0) inside `overlay_element_styling` — one atomic consulted by its two existing opacity read paths; covers judge, freeze O.K./N.G., FAST/SLOW, combo, pacemaker. Styling mod disabled ⇒ fail-open (overlays visible, one WARN), calibration still runs | Accepted (fail-open policy: Assumed) |
| D19 | Suppress PUS timing readouts during calibration | Timing-stats widget shows the live ms error — the worst bias leak | Suppression flag in power_user_statistics: timing-stats widget stays hidden + pacemaker→ms-error swap returns stock (and skips its force-visible) for the calibration song | Assumed |

---

## D1 — Mod placement & data source (overridden)

**Resolution:** Calibration belongs to the timing-offsets mod — conceptually it IS
a timing-offsets feature (it writes SOUND_OFFSET), and it kills the "write
silently fails when timing-offsets is disabled" failure mode: the calibrate row
renders under the timing-offsets section on the GLOBAL SETTINGS tab and is
hidden while the mod is disabled (`visible_when` mechanism, `mod_menu/rows.rs:48-51`).
`src/mods/timing_offsets.rs` becomes `src/mods/timing_offsets/` (`mod.rs` +
`calibration.rs`) per the "mods that outgrow a single file get a subdirectory"
rule.

Data source unchanged: `power_user_statistics::data_feed`'s `judge_submit`
detour is the only per-step ms-error source. `data_feed::install` becomes
idempotent (returns true when already installed) and timing_offsets' `init`
calls it as well, so calibration works with the PUS mod disabled. data_feed
gains a minimal calibration tap: an atomic collecting flag + per-side filtered
`sum/count` (grades 0–4). Hot-path cost when idle: one relaxed atomic load.

## D2 — Offset write path (amended)

Internal calls within the mod: read baseline `get_offset(0)` (SOUND index),
apply via the existing `set_offset(0, new)` path (clamp → live config-map push →
`mod-config.json` persistence). The game latches SOUND_OFFSET into the
`GamePlayActor` at ctor, so the end-of-song write is effective exactly from the
next song. No refusal path needed for "mod disabled" — the row can't be armed
in that state.

## D3 — Correction formula & sign

`new_sound = clamp(old_sound + round(mean(delta_ms)), -1000, 1000)`.
ms error: negative = early, positive = late
(`error = playhead_music_count − note.music_count`). SOUND_OFFSET: higher =
audio later; `mean(error) ≈ L_actual − SOUND_OFFSET`, so late hits (positive
mean) ⇒ raise the offset. Cabinet verification in the plan: apply path logs
`old / mean / count / new` at INFO; a wrong sign is a one-character fix.

## D4 — Sample source & filter (amended)

Only the single playing side contributes (D16 disables 2P entirely). Grades
Marvelous/Perfect/Great/Good/Boo (opcodes 0x1028–0x102C); Miss deltas sit at
the window edge and OK carries no ms error. Plain arithmetic mean, no outlier
trimming in v1.

## D5 — Minimum samples & always-flip-OFF

≥ 30 valid samples to apply; below that the run fails with a "NOT ENOUGH STEPS"
toast. The toggle always flips OFF at song end, success or failure.

## D6 — Refusal guards

- **Song rate ≠ 100%**: don't arm; 3 s toast at song start ("SONG SPEED ACTIVE,
  CALIBRATION DISABLED"); arm still consumed at song end (D16 rule).
- **Autoplay**: an autoplay-tainted side contributes no samples ⇒ run fails
  with "NOT ENOUGH STEPS" (or a dedicated autoplay refusal toast at arm time if
  cheaply detectable).
- **Training mode**: allowed — judgments are real; scrubs/loops reset samples (D12).

## D7 — Sanity bound

abs(mean) > 500 ms ⇒ refuse (WARN + failure toast). A genuine audio chain is
never half a second off.

## D8 — Toast promotion & animation (amended)

Move `src/mods/training_mode/toast.rs` → `src/services/toast.rs` (shared
service; training_mode call sites updated). API grows:

- Owned `String` text (result toasts carry numbers).
- **Pulsing persistent mode**: "Calibrating..." slowly fades in and out in a
  loop for the duration of the song, dismissed at gameplay exit.
- **Flash with caller-specified hold**: 5 s for the result toast, 3 s for the
  refusal toasts; existing 250 ms default preserved for Training Mode.

Same widget, bottom-center (640, 630), same render-thread discipline
(generation-tokened self-requeueing animation, already the module's pattern).

## D9 — Option surface (overridden)

No custom_options registration at all. The control is a contributed overlay row
on the GLOBAL SETTINGS tab via the frozen `mod_menu::register_enum_row` API
(values `[0,1]`, labels `["OFF","ON"]` — no bool-row public API exists and the
enum row is exactly equivalent): key `timing_calibrate_next`, label
"Calibrate next song?", registered BEFORE the four offset scalar rows so it
renders at the top of the timing-offsets section. Consequences:

- Not in the in-game options menu, not per-player, no mirroring needed.
- No `option_menu_settings`, no label textures, no persistence of any kind —
  the armed state is an in-memory atomic owned by timing_offsets and is
  explicitly excluded from `persist_all()`.
- The programmatic flip-OFF at song end updates the row store (small
  `rows.rs` setter wrapping the existing `set_row_value_and_refresh`, or
  idempotent re-registration — design decides).

## D14 — Result feedback

Success: 5 s toast `CALIBRATED: 87 -> 93 (+6 MS)` + INFO log with sample count,
mean, stddev. Failures: reason-specific toasts + WARN. All apply/flip work in
the scene callback at gameplay exit (catch_unwind context provided by
scene_manager).

## D15 — Per-song judgement offsets interplay (amended)

Per-song offsets are seeded from master data with the "correct" per-song
corrections and are additive with the global offset — having them active during
calibration gives the most representative measurement. They apply as normal; no
warning, no guard, no documentation caveat beyond a code comment.

## D16 — 2P play & arm consumption (accepted)

When two players are entered, calibration is disabled for that song with a 3 s
toast at song start: "2P MODE DETECTED, CALIBRATION DISABLED".

**Accepted:** the 2P song consumes the arm. **One rule: any song ending while
the toggle is ON flips it OFF**, whether calibration ran, was refused (2P, song
rate), or failed (too few samples). Matches the stated semantics ("flips off at
the end of the song") and avoids a silently-armed toggle surprising players
songs later.

## D17 — Side detection (assumed)

Which side is "the playing side": entered-state via `stage_records`
(`player_work+0x4`, the announcer_mute/quick_logout precedent), evaluated at
GAMEPLAY entry latch time. Exactly one entered side ⇒ calibrate that side
(samples filtered by `actor+0x84` side match); two ⇒ D16; zero/unreadable ⇒
don't arm (fail-open, WARN).

## D18 — Hide judgement overlays during calibration (accepted 2026-08-26, post-design)

If the player can see judgements/pacemaker during calibration they will
subconsciously adjust to satisfy the CURRENT timing settings instead of playing
naturally against the audio.

**Resolution:** `overlay_element_styling` already captures exactly the right
clip set (`dance_judge`, `dance_judge_for_freeze`, `dance_fast_slow`,
`dance_combo_root*`, `dance_score_compare`) and ALL of its opacity consumers
route through two functions (`opacity_pct` / `opacity_pct_fast`). It gains a
per-song override atomic (sentinel = none, 0 during calibration) consulted in
those two functions: Judge/FreezeJudge/FastSlow get an alpha-0 one-shot at clip
bind, Combo/Pacemaker are zeroed by the compose detour on every game color
write. Calibration sets the override for both sides at GAMEPLAY entry (before
clips bind) and clears it at exit. **Fail-open (assumed):** if the
overlay-element-styling mod is disabled its hooks aren't live and there is no
hide path — calibration proceeds with visible overlays and one WARN rather
than refusing (measurement is still valid, just noisier).

## D19 — Suppress PUS timing readouts during calibration (assumed)

The timing-stats widget (live `Current: +Xms` readout) and the
pacemaker→ms-error swap leak the exact signal being calibrated. Both are
power_user_statistics features that read their option gates live per dispatch,
so PUS gains a `calibration_suppress(bool)` flag: while set, the timing-stats
widget's `update_text` early-returns before showing, and `pacemaker_swap`
returns the stock value and skips its force-visible write. Set/cleared
alongside D18's override. (The pacemaker clip itself is also alpha-0 via D18
when the styling mod is on — D19 covers the styling-mod-disabled case and the
widget, which lives outside the AFP clip pipeline.)
