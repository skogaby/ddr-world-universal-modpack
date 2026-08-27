# Idea Honing — Bulk Hack Porting (20260523)

This document captures the requirements clarification Q&A for the bulk
hack-porting feature. Each section is one question + the user's chosen
answer (with alternatives noted where they were considered).

---

## Q1: Mod-menu gating change — scope and triggers

The rough idea says "remove the gates in the mod menu that currently exist
around scene ID, and instead allow the mod menu to be opened on any screen,
no matter what." The current mod menu (`src/mods/mod_menu.rs`) has two
scene-related gates:

  1. **Open gate**: `open()` early-returns if `current_scene() > ATTRACT_SCENE_MAX` (16).
  2. **Auto-close**: a `scene_manager::on_scene_change` callback closes the
     menu when the new scene is past `ATTRACT_SCENE_MAX`.

Plus, the open gesture is **triple-press of NUM_5** on either pinpad. The
gesture itself is wired through `input_manager::on_input_event`, which
dispatches on the render thread.

**Question:** When you say "open on any screen, no matter what", do you mean:

  (a) Drop both gates entirely — triple-5 opens the menu and the menu stays open
      across any scene transition. (Simplest; matches the rough idea literally.)
  (b) Drop both gates, AND add a safety: while the menu is open during gameplay
      (scene 28) the menu also intercepts/blocks pinpad input from reaching the
      game so the user doesn't accidentally interact with gameplay while
      navigating. (`set_exclusive_consumer` is already in use during attract.)
  (c) Drop the open gate but keep auto-close on certain scene ranges (e.g. while
      in gameplay scenes 28/29 the menu auto-closes if it ever opens, but it can
      open from anywhere outside gameplay).
  (d) Something else.

Note that the menu already grabs `input_manager::set_exclusive_consumer` on
open, so gameplay panel inputs are already suppressed while it's open during
attract. This needs verification for in-gameplay scenes — pinpad/panel inputs
*do* reach the game via different code paths during scene 28.

**Answer:** (b) Drop both gates AND ensure exclusive input consumption holds
during gameplay so menu-navigation pinpad presses do not bleed through to the
game. Verify (during implementation) that `input_manager::set_exclusive_consumer`
suppresses input on all scenes including 28 (GAMEPLAY); if not, extend the
suppression mechanism. Panel inputs (the four arrow panels) are out of scope —
those are read by the game from a different code path (the actor's IFootPanel)
and are not what the user uses to navigate the mod menu.

---

## Q2: PowerUserStatistics structure — three sub-features and their toggles

The three sub-features (Timing Stats, Pacemaker→MsError, CSV Export) all
consume per-step ms-error from `judge_hook`, but they are otherwise
independent. Pacemaker→MsError additionally needs a child option for the
white-pacemaker-zone threshold.

**Answer:** Single mod (`PowerUserStatistics`) with three independent
per-player sub-feature toggles plus a child threshold option. Options menu
rows on the Mods tab (Page6) for one player:

```
TIMING STATS              [ON / OFF]
PACEMAKER → MS ERROR      [ON / OFF]
  WHITE THRESHOLD          [10 ms]   <- visible only when above is ON
EXPORT STEP DATA          [ON / OFF]
```

The mod registers/unregisters its `judge_hook` callback based on whether
any sub-feature is enabled by any player. State (per-step accumulators,
ring buffers) is lazy-allocated per sub-feature. The `WHITE THRESHOLD`
row uses `ShowWhen::Equals { parent_id: "pacemaker_to_mserror", value: 1 }`
from the custom_options framework.

---

## Q3: StageProgressionHacks — split into two mods

The original rough idea grouped Premium Free, Quick Restart, and Quick Fail
under a single `StageProgressionHacks` mod. On reflection, splitting Premium
Free out makes more sense (it needs an independent toggle since "unlimited
stages" is a meaningfully different lifestyle decision from "let me restart
or fail a song quickly"). Quick Restart and Quick Fail are conceptually
similar (both are mid-gameplay scene-redirect gestures), so they group
naturally.

**Answer:** Replace the original `StageProgressionHacks` mod with **two**
separate mods:

  1. **`PremiumFreeMod`** — single mod-menu toggle. Hooks the stage-counter
     increment to freeze the stage number at the current round.

  2. **`QuickRestartOrFailMod`** — single mod-menu toggle. Listens for the
     two triple-press gestures during gameplay:
       - Triple-1 within 1.5s → song restart (re-enter scene 28 fresh).
       - Triple-3 within 1.5s → song fail (transition to scene 25).

This means the project now ships **four** new mods (was three): `PremiumFreeMod`,
`QuickRestartOrFailMod`, `SongSelectionImprovementsMod`, `PowerUserStatisticsMod`.
Each appears as a top-level entry in the in-game mod menu.

---

## Q4: Premium Free — score-save behavior under frozen stage counter

The original mod (R9 patch, `binary_modpack_research.md §3`) hooks the per-frame
stage-counter increment and zeroes it before re-incrementing — keeping the
player on stage 1 forever. The user's concern: does the game still emit a
"save score" packet after each song? If yes, perpetually overwriting the same
stage slot in the save packet works (most DDR World backends — including
bemani-buddy — take the last index on save).

**Answer:** (c) Defer the concern. Implement Premium Free first using the
straightforward stage-counter-freeze hook (R9 anchor). Deploy and observe
the live save behavior — if scores are saved correctly after each song,
no further work is needed. If they aren't, return here and do light RE work
to identify whether the save packet is suppressed under frozen-stage and
either force-emit a save (option b above) or document the limitation.

This means the design and implementation can proceed without extra RE work
on the save path. The verification step belongs in the demo / acceptance
criteria for the Premium Free mod task: "play two songs, confirm both
appear in the backend with the expected scores".

---

## Q5: Quick Restart — RE the scene-transition trigger and actor reset

**Answer:** (c) Do a focused RE pass before locking the design. Goals:

  1. Identify how the game initiates a *fresh* gameplay run — is it via
     `TransitionSequence::createNextSequence(28)` from outside scene 28,
     or via a different trigger function?
  2. Determine whether scene 28 → scene 28 transitions cleanly destroy and
     reconstruct the `GamePlayActor`, or whether the actor persists.
  3. If the actor persists, identify the per-step timing accumulator
     fields on `GamePlayActor` so we can reset them manually before the
     re-entry.
  4. Settle whether a "bounce through scene 26 (SONG_TO_STAGE_INTERSTITIAL)"
     approach is cleaner than scene-28 → scene-28 direct re-entry.

This RE work belongs in the research phase. Document findings in
`research/quick-restart-re.md`. The gesture mechanic itself (triple-1 within
1.5s during scene 28, either pinpad) is independent of these unknowns and
can be designed up front.

---

## Q6: Quick Fail — gesture, transition mechanism, Premium Free interaction

**Answer:** Triple-3 within 1.5s during scene 28 (either pinpad) — same UX
shape as Quick Restart's triple-1.

For the underlying transition mechanism, **reuse the original mod's
state-pair hijack** (`binary_modpack_research.md §16`, R19). The mechanic:

  - The post-stage state machine (`FUN_1800b3a80`, case `0x1c`) normally
    selects state pair `0x20`/`0x37` (normal post-stage transition) but
    hijacks to `0x21`/`0x38` (failed/quit-out) when a precondition is met.
    On 20260421 these IDs shifted to `0x21`/`0x39` and `0x22`/`0x3a`; the
    AOB anchor wildcards the immediates so the patch is portable.
  - The original mod's precondition was "is either player's Start held?".
    Our port's precondition becomes "did the QuickRestartOrFail mod
    detect a triple-3 gesture in the gameplay scene that hasn't been
    consumed yet?".
  - On gesture detection during scene 28, our gesture handler sets a
    one-shot "force-fail-out" flag. The R19 hook reads the flag at the
    case-0x1c dispatch site, applies the alternate state pair, and
    clears the flag.

This sidesteps the "what does direct scene 25 jump leave dangling?"
question — the state-machine path is what the game itself uses for the
quit-out flow, so all teardown is correct by construction.

**Premium Free interaction:** When `PremiumFreeMod` is enabled and
`QuickRestartOrFailMod` is also enabled, triple-3 must NOT bump the
stage counter (because Premium Free already freezes it). Two ways to
guarantee this:

  (i) The state-pair hijack changes which post-stage transition the game
      enters; the stage-counter increment is governed by `PremiumFreeMod`'s
      hook (R9), independent of which state pair is in flight. So as long
      as PremiumFree's hook is installed, the counter stays frozen
      regardless of which path we exit through. **No interaction needed.**

  (ii) We may want PremiumFree to additionally suppress the increment that
       the state machine would normally do at the case-0x1c boundary (not
       just the per-frame increment R9 hooks). Defer this to live testing —
       if scores save correctly across Quick Fail with Premium Free on,
       no further work is needed.

**RE that's still needed:** Confirm the R19 anchor still resolves on the
20250805 stock and 20260421 versions (the research doc claims this; we
verify during implementation). Confirm the scene-machine hijack triggers
the result-screen-skip behavior we want even without holding Start (i.e.
the original modder's intent maps cleanly to ours). This belongs in the
same `research/quick-restart-re.md` document.

---

## Q7: Pinpad input availability during gameplay (scene 28)

**Answer:** (a) Trust the input pipeline. `services::input_manager` polls
arkmdxbio2's `arkMDXGet10Key` continuously — the driver doesn't gate on
scene. If a deploy reveals otherwise, add a diagnostic log and redeploy
(cheap fallback). No research-phase prerequisite check needed.

---

## Q8: SongSelectionImprovements — single toggle vs sub-toggles

**Answer:** (a) Single mod-menu toggle for the parent mod, but with a
new `song_selection_improvements` section in `mod-config.json` exposing
three independent sub-feature toggles for power-user mix-and-match
(see Q11 below for the final shape). Default: all three sub-features
on. These are global game-behavior tweaks with no per-player
differentiator. Mod-menu disable still kills all three; JSON sub-toggles
let you disable one piece while keeping the others.

```json
{
  "song_selection_improvements": {
    "real_speed_core_bpm": true,
    "flare_to_clear_lamps": true
  }
}
```

(Per Q9: `speed_toggle_smaller_steps` was originally part of this
section but has been DROPPED from scope after the user confirmed
20260421 implements the coarse/fine semantics natively.)

The mod's `enable()` reads each sub-toggle and only installs the
corresponding hook(s) for sub-features that are on. Missing JSON
section / individual missing keys default to `true`.

---

## Q9: Updated Speed Toggle — DROPPED from scope (Konami implemented natively)

**Original question / decision (preserved for history):** the user wanted
the speed option to step by ±0.05× normally and ±0.50× when Start is
held, mirroring `binary_modpack_research.md §13` from the pre-modded
20250805 build.

**Final answer:** **DROP THIS SUB-FEATURE FROM THE PROJECT SCOPE.**

  - Live observation by the user on 20260421: the in-game speed rate
    toggle DEFINITIVELY has both fine (±0.05×) and coarse (±0.50×, with
    Start held) semantics already, identical to what the mod was trying
    to implement.
  - This means Konami implemented the feature natively somewhere between
    20250805 and 20260421. The mod is functionally redundant on the
    current target version; porting it would be no-op or worse.
  - The research agent's static analysis of `OptionHispeed` was based
    on disassembly of the speed-step path it found, which on 20260421
    apparently doesn't include the new coarse/fine logic Konami added.
    This is fine — we trust the live game over static analysis here.

**Impact on `SongSelectionImprovementsMod`:**
  - The mod now ships with TWO sub-features instead of three:
    `real_speed_core_bpm` and `flare_to_clear_lamps`.
  - The `speed_toggle_smaller_steps` JSON sub-toggle (Q8 / Q11) is
    REMOVED.
  - The Q9 research note (`research/speed-toggle-re.md`) is preserved
    as a historical record but its recommendations are not implemented.

---

## Q10: Real Speed Fix — port both BPM swap and logf guard

**Answer:** (a) Port both pieces of the original mod (`§4`):

  1. **BPM divisor swap (R24-R26):** Replace the Max BPM divisor with
     Core BPM read from `[ChartData + 0x88]` in the scroll-speed
     display formula. AOB anchor: `F2 0F 5E 01 48 8D 4C 24 40` (verified
     unique on 20250805 and 20260421).
  2. **logf guard (R15-R16):** Wrap the bare `logf` call so `logf(0)`
     returns `0` instead of `-inf`/`NaN` (which would otherwise display
     briefly before the song starts). AOB anchor:
     `0F 28 C7 E8 ?? ?? ?? ?? F3 0F 58 C6` (verified unique on
     20250805 and 20260421).

Both share the same parent function and are cheap to install together.
Both are gated by a single JSON sub-toggle: `real_speed_core_bpm`
(Q8 / Q11). When `false`, neither hook is installed.

---

## Q11: Flare → Lamps gating — JSON sub-toggle

**Answer:** Final shape per the user's preference: the
`song_selection_improvements` JSON section (Q8) carries three
independent sub-toggles, one of which is `flare_to_clear_lamps`. Default
`true`. When `false`, the flare-banner-setup hook is not installed.

---

## Q12: Timing Stats During Gameplay — widget shape and content

**Answer:** (a) Match the original mod's final form for metrics:

  - **Current ms-error** — most recent step's signed delta
  - **Max ms-error** — running max(|signed delta|) over the song
  - **Abs(μ)** — running mean of |signed delta|
  - **μ (Mean)** — running mean of signed delta

Layout: **per-player widget groups**. P1's group on the left half of the
screen, P2's group on the right (positions tunable during implementation
via `widgets/text_widget` constants). Each group is a tight
vertically-stacked column of the four labeled values. Per-player
positioning gives us flexibility for:

  - Single-player mode (hide the inactive side's group)
  - Double-play (one group used for the doubles player; the other hidden)
  - Versus mode (both groups visible, side-aligned)

Visibility: only during scene 28 (GAMEPLAY). The per-player gating
ALSO honors the `timing_stats` per-player option toggle (Q2): a player
who disabled the option in their card's options menu gets no widget for
their side, while the other player's widget still shows (if their
toggle is on).

Format: 2-decimal precision (matching the original's `"%0.2lf"` format
swap from R37). Units `ms`. Sample text:
  ```
  CURRENT  +12.34 ms
  MAX     -45.67 ms
  ABS(μ)   18.50 ms
  MEAN     -2.10 ms
  ```

(Final layout/colors/positions tunable post-deploy.)

---

## Q13: Pacemaker → MsError — porting strategy

**Answer:** (a) Full parity port — both R13 and R14, but with a research-
informed change to the data-feed hook point:

  - **R13 (mid-render value override).** Hook the score-render input read
    site (`mov rdx, [rdi+0xb0]` anchor — `48 8B 97 B0 00 00 00`, verified
    unique on 20250805 and 20260421). When the active player's
    `pacemaker_to_mserror` option is ON, overwrite the formatter input
    with the most-recent ms-error from the per-player step buffer.
  - **R14 (white-zone color trigger).** Hook the test-and-jump anchor
    (`48 8B 01 85 F6 75 ?? F3 0F 10 0D`, verified unique on both
    versions). When the option is ON AND `|current_ms_error| <
    white_threshold`, force ZF=1 so the rendering falls through to the
    white-pacemaker-color path.

  Per-player options:
    - `pus_pacemaker_to_mserror`: bool toggle (default OFF). Master switch
      that gates both R13 and R14 hooks for this player.
    - `pus_pacemaker_threshold`: scalar (default 10, range 1..=50,
      step 1). Visible only when the master toggle is ON
      (`ShowWhen::Equals { parent_id: "pus_pacemaker_to_mserror", value: 1 }`).
      Units: milliseconds.

**Data-feed source — REVISED per research findings:**

  The original plan was to feed per-step ms-error from a
  `judge_hook::register_post` subscriber. **The research note
  `research/per-step-data-feed.md` showed this is too late for the
  pacemaker swap.** The pacemaker render is a tail-call inside
  `judgeNotes` (via `FUN_1800609b0` → `FUN_18007ba70` case 0x1036),
  so by the time `register_post` fires, the wrong pacemaker value has
  already been rendered for the current step.

  **Revised plan:** Install a NEW retour detour at `FUN_1800603a0` (the
  per-step judgment-result handler, called once per judged step from
  inside `judgeNotes` with `(actor, result, opcode, &delta_struct)`).
  This hook runs early enough to populate the per-player ms-error
  buffer BEFORE the in-flight pacemaker render reads it.

  All three PowerUserStatistics sub-features (Timing Stats, Pacemaker→MS,
  CSV Export) read from this single shared per-player buffer. Timing
  Stats and CSV Export are not latency-sensitive (they just need the
  data eventually, by song-end), but the shared single-buffer design
  keeps the data flow uniform and avoids duplicate hooks.

---

## Q14: CSV Export — output path, filename, write trigger, gating

**Answer:** (b) Defaults with difficulty in the filename:

  - **Output directory:** `./step_data_exports/` relative to the process
    CWD (which is the game dir when launched via spice2x). Mod creates
    the directory on enable if it doesn't exist.
  - **Filename:** `<YYYY-MM-DD>_<HH-MM-SS>_<songcode>_<difficulty>_P<n>.csv`
    where `<difficulty>` is the difficulty index resolved to its short
    label (e.g., `single_basic`, `double_challenge`) — drop to a numeric
    code if the resolved label isn't easily available at write-time.
    `<n>` is the 1-indexed player slot.
  - **CSV header:** `Expected,Actual,Delta (Ms Error)\r\n` (matches
    original mod's format).
  - **Row format:** `<expected_ms>,<actual_ms>,<delta_ms>\r\n` (signed
    `int32` ms values).
  - **Write trigger:** At scene-28 → scene-29 transition (or whatever
    canonical "song complete" event the gameplay actor exposes —
    settle via research alongside the per-step accumulator field
    discovery if needed).
  - **Per-player gating:** Write only the file(s) for player(s) whose
    `step_data_export` option was ON at song start. If a player toggles
    off mid-song, still write their file at song-end (the data was
    being collected and the user's intent at song-start is what
    governs).
  - **Failure mode:** If file create/write fails (permissions, disk
    full), log a WARN and continue. Do not crash gameplay or block the
    scene transition.

---

## Q15: Custom options framework — coverage, naming, label texture pipeline

**Answer:** (b) Bundle texture-shipping into this feature's scope, AND
consolidate all existing label-generation scripts into a single
unified script.

**Per-player options for `PowerUserStatisticsMod`** (registered via
`services::custom_options::register_option`, all on Page6 / Mods tab):

  | Option ID                  | Kind   | Default | Range / Values     | Visibility |
  |----------------------------|--------|---------|--------------------|-----------|
  | `pus_timing_stats`         | bool   | OFF     | OFF / ON           | Always |
  | `pus_pacemaker_to_mserror` | bool   | OFF     | OFF / ON           | Always |
  | `pus_pacemaker_threshold`  | scalar | 10      | 1..=50, step 1     | Visible iff `pus_pacemaker_to_mserror == 1` |
  | `pus_step_data_export`     | bool   | OFF     | OFF / ON           | Always |

  All option IDs are prefixed with `pus_` (PowerUserStatistics) for
  namespacing on the shared Mods tab. Existing option IDs (`autoplay`,
  the WebUiOptions IDs) stay as-is; future multi-option mods should
  follow the same prefix convention.

**Label texture pipeline consolidation.** Existing scripts:
  - `scripts/gen_webui_option_labels.py` — generates `seop_item_*` and
    `seop_op_*` labels for the WebUiOptions mod's many cosmetic options.
  - `scripts/gen_scroll_dummy_labels.py` — generates dummy scroll-test
    labels.

  Consolidate both into a single new script:
  `scripts/gen_custom_option_labels.py`. The script reads a manifest
  (Python dict or YAML in the script itself) of every label texture
  every mod needs, and emits all PNGs in one pass. Manifest entries:
    - `id`: the bare label slug (e.g. `pus_timing_stats`)
    - `text`: rendered text (e.g. `"TIMING STATS"`)
    - `category`: row label (`seop_item_<id>`) or value label
      (`seop_op_<id>`) — most rows reuse stock `seop_op_on/off`, so
      mostly we generate `seop_item_*` entries
  Output goes to the same LayeredFS path the existing scripts target:
  `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`.

  Migrate WebUiOptions and any scroll-test labels into the consolidated
  script, then add the 4 new PowerUserStatistics labels. Delete the
  old scripts once parity is confirmed. This consolidation is part of
  this feature's deliverables (scoped as one task in the
  implementation plan).

**Network persistence.** All four PowerUserStatistics options use the
default `persist: true`. Values round-trip through
`services::custom_options_persistence` as `mod_pus_*` fields in the
save packet. No save/load transforms needed — values are already
canonical i32s.

---

## Q16: Hook architecture — per-mod detours vs shared dispatcher

**Answer:** (a) Per-mod detours at distinct addresses are fine. No new
shared-dispatcher service needed for `FUN_180077a00` or any other
function.

**Why retour can do this safely.** `retour::GenericDetour::new` operates
at the *address* level, not the function level. The two operational
constraints are:

  1. **One detour per address.** Stacking two `GenericDetour` handles
     on the same address composes-broken — the second's "call original"
     path captures the first's JMP. This is why `judge_hook` exists
     (multiple mods want hooks at the SAME `judgeNotes` entry address).
  2. **Trampoline ranges must not overlap.** A detour at address X
     copies the original instructions at X (5–15 bytes) into a
     trampoline buffer. A second detour at address X′ within X's
     trampoline range corrupts the instruction stream.

**Audit of planned hook sites:**

  | Mod | Site | Function | Type | Notes |
  |---|---|---|---|---|
  | PremiumFreeMod | R9 (`0x180030595`) | per-frame stage-counter inc | retour detour | distinct function |
  | QuickRestartOrFailMod | R19 (`0x1800bf09f`+6) | post-stage state machine | retour detour | distinct function |
  | SongSelImp / RealSpeed | R15 | logf JMP rel8 disp | byte-level write | **not a detour** |
  | SongSelImp / RealSpeed | R16 | logf call rel32 redirect | byte-level write | **not a detour** |
  | SongSelImp / RealSpeed | R24 | BPM JMP rel8 disp | byte-level write | **not a detour** |
  | SongSelImp / RealSpeed | R25 | BPM divsd ModRM | byte-level write | **not a detour** |
  | SongSelImp / RealSpeed | R26 | int3 cave write (12 bytes) | memory write | **not a detour** |
  | SongSelImp / SpeedToggle | (TBD) | speed option row's step values | memory write or detour, depends on Q9 RE outcome | **not in `FUN_180077a00`** |
  | SongSelImp / Flare→Lamps | R21 | results banner setup | retour detour | distinct function |
  | PowerUserStats / Pacemaker | R13 (`0x18007bba6`) | score-render | retour detour | shared function w/ R14 |
  | PowerUserStats / Pacemaker | R14 (`0x18007bbf8`) | score-render | retour detour | 0x52 bytes from R13 — clear of trampoline range |
  | PowerUserStats / Timing Stats | (no game-side hook) | — | `judge_hook` subscriber | data-only |
  | PowerUserStats / CSV Export | (no game-side hook) | — | `judge_hook` subscriber + scene callback | data-only |

  R13 and R14 are the only two retour detours that land in the same
  function. They are owned by the same mod (PowerUserStatisticsMod),
  installation order is under our control, they are 0x52 bytes apart
  (well clear of trampoline range). No conflict.

  Real Speed's patches are all byte-level memory writes (no
  trampolines), so they don't interact with retour at all.

**Decision:** No new shared-dispatcher service. Each mod installs and
owns its own retour detours and memory writes. If a future mod wants
to hook a site that another mod already hooks, we re-evaluate then
(probably extending PowerUserStatisticsMod's score-render subscriber
list, similar to how `judge_hook` works).

---

## Q17: Requirements clarification — closing checkpoint

**Answer:** Requirements are sufficient. Move to research phase.

**Implicit defaults accepted (not asked, but worth recording):**

  - **Cross-version testing scope.** Each AOB / pattern verified on
    20250805 stock and 20260421 (matching the research doc's existing
    coverage). If a future game version ships during development, add
    it to the verification list.
  - **Logging verbosity.** Per-mod lifecycle INFO (`enable`/`disable`),
    hook-installation INFO (one line per address resolved + one per
    detour installed), gesture-detection DEBUG (one line per
    triple-press cycle and the resulting action). Hot-path callbacks
    (judge_hook subscribers, render callbacks) emit no logs in steady
    state.
  - **Existing-mod compatibility.** None of the four new mods conflict
    with existing mods. `PowerUserStatisticsMod` shares `judge_hook`
    via the subscriber pattern (clean composition with Autoplay's
    existing pre/post subscribers). The mod-menu gating change (Q1) is
    a behavior change for `mod_menu.rs`, not a structural change —
    existing mods are unaffected.

---

## Research-Phase Verifications (parent-side spot-checks)

The user (rightly) questioned the research agent's claim that the BPM
triple lives on `ddr::player::Option` rather than on `ChartData`.
Parent-side Ghidra spot-check on 20260421 confirmed the agent's claim:

  - `FUN_1801df8b0` is `ddr::player::Option::SetScrollSpeed` (Konami's
    own `Ordinal_382` log call inside the function names it). It reads
    the BPM divisor from `param_1 + 0x90` where `param_1` is the
    `Option*` (slot in vtable at `+0xD0`).
  - `FUN_1801df840` is the BPM-setter on the same vtable (slot at
    `+0xC0`). It writes 8-byte values to `param_1[0x10]/[0x11]/[0x12]`
    (i.e., offsets `+0x80/+0x88/+0x90`) and then dispatches
    `SetScrollSpeed` to recompute the display.
  - There is no `ddr::player::Option::SetBpm` debug string — the BPM
    setter isn't a user-facing option, it's an internal chart-load
    callback.
  - Architecture: chart-load copies the chart's BPM triple into each
    player's `Option` so per-player display settings (SUDDEN+, speed
    type, etc.) can compose cleanly without re-walking to the chart on
    every render frame.

So `[rbx + 0x88]` for Core BPM in our R26 cave write is correct, with
`rbx` = `Option*`. The conceptual model "BPM is a chart attribute" is
true at chart-load time, but the displayed-by-Real-Speed value is
read from a per-player snapshot.

This kind of spot-check is exactly what `.spec/learnings/sdd-reverse-engineer.md`
Learning 3 calls out: "treat what the agent observed in memory as
evidence, but what the agent inferred about the function's role as a
hypothesis needing re-proof."
