# Task: Hide Judgement Overlays and Suppress PUS Readouts During Calibration

## Description
While a calibration `Collecting` session is live, force every judgement-
feedback surface invisible so the player times to the audio instead of chasing
the current judgment windows: the overlay clips (judge, freeze O.K./N.G.,
FAST/SLOW, combo, pacemaker) via a calibration-hide override in
`overlay_element_styling`, and power_user_statistics' realtime readouts
(timing-stats widget, pacemaker→ms-error swap) via a suppression flag.

## Background
`overlay_element_styling` classifies exactly the right clip set by
`CMovieClip::Create` template name (`dance_judge`, `dance_judge_for_freeze`,
`dance_fast_slow`, `dance_combo_root*`, `dance_score_compare`), and ALL of its
opacity consumers route through two functions in its `mod.rs`: `opacity_pct`
(registry-preferred; consumed by the bind-time alpha one-shot for
Judge/FreezeJudge/FastSlow) and `opacity_pct_fast` (atomic-only; consumed by
the SetColor compose detours for Combo/Pacemaker). An override returning 0
from both covers every element with zero new hooks. The one-shot runs at clip
BIND, so the override must be set at scene-28 entry — which is exactly where
the calibration session latches. Alpha 0 also visually neutralizes
pacemaker_swap's force-visible attribute write (independent channels).

The PUS leaks: `timing_stats_widget::update_text` (called per judgment from
the data_feed hook; shows the widget lazily on first update) and
`pacemaker_swap`'s per-dispatch option read. Both check their gates live, so a
flag beside those reads suppresses them for the song.

Fail-open: with `overlay-element-styling` disabled its hooks are not live and
there is no hide path — calibration proceeds with visible overlays and one
WARN. With PUS disabled there is nothing to suppress (no-op).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-26-auto-calibration/design/detailed-design.md
  (§4 overlay_element_styling amendment, §5 PUS amendment, Requirements
  11–12, Error Handling rows)

**Additional References (if relevant to this task):**
- src/mods/overlay_element_styling/mod.rs — `opacity_pct` / `opacity_pct_fast`
  / `is_enabled` (`MOD_ENABLED`)
- src/mods/power_user_statistics/timing_stats_widget.rs — `update_text` show
  path
- src/mods/power_user_statistics/pacemaker_swap.rs — option read +
  force-visible write

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `overlay_element_styling/mod.rs`:
   - `static CALIBRATION_HIDE: AtomicBool` (default false).
   - `pub fn set_calibration_hide(on: bool) -> bool` — stores the flag and
     returns whether the mechanism is live (mod enabled); the mod's
     `disable()` clears the flag.
   - `opacity_pct` and `opacity_pct_fast` return 0 while the flag is set
     (checked FIRST — a single relaxed load; both functions are on warm/hot
     paths).
2. `power_user_statistics`:
   - `static CALIBRATION_SUPPRESS: AtomicBool` + `pub fn
     set_calibration_suppress(on: bool)` (in `mod.rs` or a shared submodule).
   - `timing_stats_widget::update_text`: early-return (before any `show()`)
     while suppressed.
   - `pacemaker_swap`: while suppressed, behave exactly as if the
     `pacemaker_to_mserror` option were OFF for the dispatch (return the
     stock value, skip the force-visible write).
3. `timing_offsets/calibration.rs` session wiring:
   - Entry (guards passed, alongside the pulsing toast):
     `overlay_element_styling::set_calibration_hide(true)` — on `false`
     return, one WARN ("overlay hide unavailable -- overlays stay visible");
     `power_user_statistics::set_calibration_suppress(true)`.
   - Exit (`Collecting` teardown) and `calibration::disable()`: clear both.
   - `ConsumeOnly` sessions never set them.
4. Hot-path discipline: the added checks are single relaxed loads; no locks,
   no allocation.

## Dependencies
- Steps 2–3 (the session transitions being wired into).

## Implementation Approach
1. Add the override + accessor to overlay_element_styling; thread the check
   into the two opacity functions.
2. Add the suppression flag + two consumer checks in PUS.
3. Wire both into calibration entry/exit/disable.
4. `cargo check`, validation script, `./build.sh`.

## Acceptance Criteria

1. **Calibration song is feedback-free**
   - Given a calibration song with default styling values and `timing_stats` ON
   - When gameplay runs
   - Then no step judgements, freeze O.K./N.G., FAST/SLOW, combo, pacemaker,
     or timing-stats widget are visible — only arrows and the pulsing toast

2. **Everything returns next song**
   - Given the calibration song ended
   - When the next (non-calibration) song plays
   - Then all overlays and readouts render normally with the players' own
     styling values (no stuck opacity-0 or suppression)

3. **Styling-mod-disabled fail-open**
   - Given `overlay-element-styling` disabled in `mod-config.json`
   - When a calibration run completes
   - Then it applies normally, overlays were visible, and one WARN was logged

4. **Refused songs don't hide**
   - Given a 2P or song-speed refusal (ConsumeOnly)
   - When the song plays
   - Then overlays render normally (no hide, no suppression)

5. **Mid-session disable cleans up**
   - Given the timing-offsets mod disabled during a calibration song
   - When `disable()` runs
   - Then the hide override and suppression flag are cleared

## Metadata
- **Complexity**: Medium
- **Labels**: rust, overlay-element-styling, power-user-statistics, hot-path
- **Required Skills**: Rust atomics, this codebase's overlay/compose patterns
- **Generated By**: code-task-generator 2026-08-26
- **Source Plan**: .agents/planning/2026-08-26-auto-calibration/implementation/plan.md
- **Plan Step**: Step 4: Hide judgement overlays and suppress PUS readouts
