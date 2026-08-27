# Implementation Plan — Bulk Hack Porting (20260523)

## Implementation Checklist

- [x] **Step 1**: Mod-menu scene-gate removal (foundation)
- [x] **Step 2**: PremiumFreeMod — NOP-out the per-frame stage counter increment
- [x] **Step 3**: scene_manager — `current_transition_sequence()` accessor
- [x] **Step 4**: QuickRestartOrFailMod — gesture detector landed; restart approach pivoted (see Step 5)
- [x] **Step 5**: QuickRestartOrFailMod — Quick Fail (gauge::GAME_OVER + step write) + Quick Restart (gauge::GAME_OVER + scene redirect). Verified working 2026-05-24.
- [x] **Step 6**: Label-gen script consolidation — `scripts/gen_custom_option_labels.py`
- [x] **Step 7**: SongSelectionImprovementsMod skeleton + JSON config
- [x] **Step 8**: RealSpeedCalculationFix (née SongSelectionImprovementsMod) — BPM swap + logf guard. Flare→Lamps dropped from scope.
- [x] ~~**Step 9**: Flare→Lamps sub-feature~~ — DROPPED (caused false positives on unplayed songs; low value)
- [x] **Step 10**: PowerUserStatisticsMod skeleton + custom_options registration
- [x] **Step 11**: PowerUserStatisticsMod / per-step ms-error data feed (`judge_submit` detour)
- [x] **Step 12**: PowerUserStatisticsMod / Timing Stats widget
- [x] **Step 13**: PowerUserStatisticsMod / Pacemaker→MsError swap (inline JMP stub at pacemaker render site)
- [x] **Step 14**: PowerUserStatisticsMod / CSV Export
- [ ] **Step 15**: End-to-end verification + cross-version (20250805 stock) re-test (deferred — all features implemented and tested on 20260421)

---

## Plan

The implementation is sequenced so that each step ships a working,
demoable increment. The first step (mod-menu gating) lands first
because every other mod relies on the user being able to open the
menu from any scene to toggle them on for testing during the rest
of the work.

The mods are then sequenced from least-coupled to most-coupled:
PremiumFree (no shared state), QuickRestartOrFail (depends on a new
scene_manager accessor), Real Speed (byte-write only, no shared state),
Flare→Lamps (single hook), then the three PowerUserStatistics
sub-features which share the per-step ms-error buffer.

Cross-version verification (Step 15) is an explicit final pass: the
research notes confirm anchors on both 20250805 stock and 20260421,
but the live behavior on stock 805 deserves one validation pass.

There are no dedicated "testing" steps — testing is folded into each
step's Demo acceptance criterion, and the codebase has no unit tests
(per `.spec/steering/tech.md`). Each step's deploy + observation IS
the test.

---

### Step 1: Mod-menu scene-gate removal (foundation)

**Objective**

Allow the in-game mod menu (triple-5 gesture) to open on any scene at
any time — including during gameplay (scene 28). This is foundational
because every subsequent mod needs to be toggled on/off from the menu
during testing, and we don't want to back-port testability later.

**Implementation guidance**

In `src/mods/mod_menu.rs`:
- In `open()`, delete the early-return that gates on
  `current_scene() > ATTRACT_SCENE_MAX`.
- In `enable()`, delete the `scene_manager::on_scene_change` callback
  registration (and the `scene_cb_id` field).
- In `disable()`, drop the corresponding `remove_callback` call.

The `set_exclusive_consumer` mechanism (already in use) is sufficient
to prevent menu navigation pinpad inputs from bleeding through. Per
`research/mod-menu-input-gating.md`, gameplay only reads bit 0 (Start)
and bits 5-8 (foot-panel arrows); numpad bits 9-20 are ignored during
scene 28.

**Test requirements**

`cargo check --target x86_64-pc-windows-msvc` succeeds. The change is
narrow enough that compile-success + visual demo together constitute
acceptance.

**Integration with previous work**

None (foundation step).

**Demo**

Deploy. Triple-5 during scene 28 (during a song). Mod menu opens.
Navigate up/down with NUM_2/NUM_8 — only the menu cursor moves; the
gameplay actor's foot-panel-arrow detection continues to work
correctly. Triple-5 again to close the menu. Resume gameplay.

---

### Step 2: PremiumFreeMod — stub assembly, hook, mod-menu registration

**Objective**

Ship `PremiumFreeMod` as a togglable mod that freezes the per-stage
counter at the current round.

**Implementation guidance**

1. Add the new signature to `src/core/signatures.rs`:
   - Name: `premium_free_stage_inc`
   - Pattern: `FF 41 0C 45 33 C0 41 8D 50 68 48 8B 0D`

2. Create `src/mods/premium_free.rs` implementing the `Mod` trait
   per the codebase convention (see `mods/mod_trait.rs`,
   `mods/timer_freeze.rs` as the closest reference for byte-write +
   stub mods).

3. Stub assembly:
   - At enable: AOB-scan for `premium_free_stage_inc`.
   - VirtualAlloc 16 bytes near the patch site via
     `core::memory::alloc_near` (RWX).
   - Assemble the stub bytes by hand:
     ```
     mov rcx, [rax]               ; 48 8B 08
     cmp byte [rip+ENABLED], 0    ; 80 3D <rel32> 00
     je inc                       ; 74 03
     mov dword [rcx+0xc], 0       ; C7 41 0C 00 00 00 00  -- inline if needed
     inc dword [rcx+0xc]          ; FF 41 0C
     jmp <return_addr>            ; E9 <rel32>
     ```
     Resolve all relative displacements against the stub address +
     instruction position.
   - Patch the stock 6 bytes at the anchor with `E9 <rel32>` (5-byte
     JMP to stub) + 1-byte NOP (`90`).
   - The static `ENABLED: AtomicBool` is a Rust static; the stub's
     `cmp byte [rip+disp]` decodes a runtime address into the rel32.

4. Save the stock 6 bytes before patching so `disable()` can restore
   them.

5. Register the mod in `src/lib.rs` after the existing mods. Add
   `pub mod premium_free;` to `src/mods/mod.rs`.

**Test requirements**

`cargo check --target x86_64-pc-windows-msvc` succeeds. Deploy +
manual demo confirms the freeze.

**Integration with previous work**

Builds on Step 1 (mod menu opens during gameplay/results, so we can
toggle the mod on between stages without restarting the game).

**Demo**

Deploy. Open mod menu, toggle "Premium Free" ON. Play a song. After
song-end, the player remains on stage 1 (not stage 2). Play 2 more
songs, all on stage 1. Toggle the mod OFF mid-session. Play 1 more
song; the stage advances normally to stage 2. Verify scores appear
in the backend (or spice2x network log) for at least the first 2
songs played under Premium Free — if only the most recent shows,
the deferred score-save concern (Q4) is realized; create a follow-up
task and document.

---

### Step 3: scene_manager — `current_transition_sequence()` accessor

**Objective**

Add a public accessor on the existing `services::scene_manager` that
returns a snapshot of the active `TransitionSequence*`. This unblocks
QuickRestartOrFailMod's gesture-triggered scene transitions.

**Implementation guidance**

In `src/services/scene_manager.rs`:
- Add `static CURRENT_TS: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());`
- In the existing `createNextSequence` detour (or its underlying
  callback), store the `this` pointer (RCX = first argument) into
  `CURRENT_TS` with `Release` ordering before/after dispatching the
  original.
- Add public function:
  ```rust
  pub fn current_transition_sequence() -> Option<*mut u8> {
      let p = CURRENT_TS.load(Ordering::Acquire);
      if p.is_null() { None } else { Some(p) }
  }
  ```

**Test requirements**

`cargo check` succeeds. No standalone demo (the accessor is consumed
by Step 4). Verify by adding a one-shot INFO log that prints the
captured pointer the first time it changes; deploy briefly to confirm
the pointer is non-null after the first scene transition.

**Integration with previous work**

Independent of mods. Foundation for Step 4.

**Demo**

Deploy with the one-shot diagnostic log. Boot the game, observe a log
line like `scene_manager: captured TransitionSequence* @ 0x...`.
Remove the diagnostic log before committing.

---

### Step 4: QuickRestartOrFailMod skeleton + gesture detector

**Status**: skeleton + gesture detector + scene-leave clearing landed
in commits up to `6a173f1`. The original "trigger 28→28 via
`returnToParent` / `FUN_18002de40`" approach was implemented and
deployed but produced a half-initialized "limbo" state (verified live
via Cheat Engine MCP — see `research/quick-restart-pivot.md` for the
root-cause writeup). The implementation in `quick_restart_or_fail.rs`
is being preserved as scaffolding (gesture detector, scene-leave
buffer reset, mod-menu registration) but its `trigger_restart()` body
will be rewritten in Step 5 against the new approach.

**Carried forward from this step**

- `GestureBuffer` struct + `GESTURE_STATE` static.
- `input_manager::on_input_event` registration.
- `scene_manager::on_scene_change` registration that clears all four
  `VecDeque`s on leaving scene 28.
- `Mod` trait registration in `src/lib.rs` and `src/mods/mod.rs`.
- `scene_manager::current_transition_sequence()` accessor (Step 3).

**Removed / superseded**

- The `sequence_return_to_parent` and `shutter_open` signatures
  (added during the limbo investigation, not used by the new path).
- The `transition_advance_to_scene` signature (deleted earlier).
- The `trigger_restart()` body (`returnToParent(active_child, 0x1d)`).

---

### Step 5: Quick Fail + Quick Restart (unified — gauge::GAME_OVER + scene redirect)

**Objective**

Ship both Quick Fail (triple-3) and Quick Restart (triple-1) using a
shared "trigger natural fail-out" helper. Quick Restart is Quick Fail
plus a one-shot scene redirect.

**Why this is one step now**

Per `research/quick-restart-pivot.md`, the limbo bug from Step 4
demonstrated that out-of-band actor-tree advances are unsafe across
versions. The natural fail-out flow runs entirely from inside the
framework's update tick (where the `[parent+0x60]=1` lock is held),
so it doesn't have that problem. Both gestures map to the same
helper:

- **Quick Fail**: register one-shot redirect (29 → 25) so the user
  bypasses the results screen on a bailed song, then dispatch
  `gauge::GAME_OVER`. Natural fail flow runs, scene_manager rewrites
  STAGE_RESULT → SONG_SELECT mid-flight.
- **Quick Restart**: register one-shot redirect (29 → 28), then
  dispatch `gauge::GAME_OVER`. Framework advances to STAGE_RESULT,
  our scene_manager hook rewrites it to GAMEPLAY mid-flight, and a
  fresh DPS is constructed.

**Implementation guidance**

1. Add a new signature `gameplay_actor_vtable` derived via
   `find_vtable_by_rtti(".?AVGamePlayActor@dance@sequence@@", ...)`
   in `src/core/signatures.rs`. Same mechanism as the existing
   `auto_foot_panel_vtable` and `check_step_data_vtable`. Document
   that this is used to RTTI-match GamePlayActor instances at
   runtime.

2. Identify the numeric value of `gauge::GAME_OVER`. Two paths,
   pick whichever lands faster in the deploy iteration:

   **Path A (preferred — static)**: disassemble
   `GamePlayActor::onReceiveMessage` (= vtable slot 3, offset +0x18),
   walk the case-statement, find the case constant whose body is
   `m_step.stepSet(STEP_GAME_OVER)` (i.e. `mov [reg+OFFSET], 5`).
   Hardcode the constant with a comment citing the offset where it
   was verified. Likely in the `0x10??` range.

   **Path B (diagnostic)**: install a one-shot detour on
   GamePlayActor's `onReceive` and log every msg id received.
   Naturally fail a song (use `IMMORTAL` gauge inversion or the
   debug cancel) and observe which msg id arrives just before
   the screen fade. Hardcode that constant.

3. Rewrite `quick_restart_or_fail.rs`:
   - Add a private `find_gameplay_actors() -> Vec<*mut u8>`
     helper that walks `current_transition_sequence() + 0x58 = DPS`,
     then `DPS+0x18` (first child) → `child+0x10` (next sibling),
     filtering by `*(actor as *const usize) ==
     signatures::require_address("gameplay_actor_vtable") as usize`
     (vtable pointer match — more robust than name match).
   - Add a private `dispatch_game_over(actor: *mut u8)` helper
     that calls `actor->vtable[3](actor, GAUGE_GAME_OVER, NULL)`.
   - Implement `trigger_fail()` that calls
     `dispatch_game_over` for every found GamePlayActor.
   - Implement `trigger_restart()` that registers a one-shot scene
     redirect (29 → 28), then calls `trigger_fail()`. The redirect
     must clear itself after firing once — extend
     `scene_manager::add_redirect` to support `add_redirect_once`,
     OR clear from the `scene_manager::on_scene_change` callback
     when `next == GAMEPLAY` and a "pending restart" flag is set.

4. Wire `trigger_fail()` to triple-3 in the existing `on_input_event`
   handler. (Currently triple-3 logs and does nothing.)

5. Remove the `sequence_return_to_parent` and `shutter_open`
   signatures from `signatures.rs` (no longer used).

**Test requirements**

`cargo check --target x86_64-pc-windows-msvc` succeeds. Deploy + manual
demo for both gestures.

**Integration with previous work**

- Step 4 (gesture detector + scene_manager
  `current_transition_sequence()` accessor + scene-leave buffer reset).
- Existing `scene_manager::add_redirect` mechanism (used today only
  for `7 → 14` license-screen skip; we'll add a one-shot variant).

**Demo**

Deploy. Open mod menu, enable Quick Restart / Fail.

- **Quick Fail**: Start a song. Press NUM_3 three times within 1.5s
  mid-song. The song fades out, shutter closes, and the game
  advances directly to SONG_SELECT (scene 25) — the results screen
  is skipped via the one-shot 29→25 redirect. Score is marked as
  failed (no ranking submission). Confirm with `PremiumFreeMod`
  also enabled — stage counter stays at 1.
- **Quick Restart**: Start a song. Press NUM_1 three times within
  1.5s mid-song. The song fades out, shutter closes — but instead of
  STAGE_RESULT, the game enters a fresh GAMEPLAY scene with the same
  song, fresh chart state, fresh judgment buffers. Stage counter
  stays at 1.

**Post-deploy verification**

After 5 restarts of the same song, play to completion and check the
end-of-song stats screen. Mean/max numbers should look reasonable for
a single play. If they accumulate, the natural-flow's
score-suppression-on-IsDead branch isn't fully active for our
dispatch — add a follow-up task to clear per-stage accumulator state
at `[DAT_1806edff0[player] + 0x594 + stage_idx*0x2b8]` before the
restart redirect fires.

If the framework lands on a scene OTHER than STAGE_RESULT (= 29) on
fail (per `research/quick-restart-pivot.md`'s "What this leaves as
future work" note), update the redirect target accordingly. Use the
existing `scene_manager` log lines to identify which scene actually
fires.

---

### Step 6: Label-gen script consolidation

**Objective**

Consolidate the two scattered label-generation scripts
(`gen_webui_option_labels.py` and `gen_scroll_dummy_labels.py`) into
a single new `gen_custom_option_labels.py` with a unified manifest.
This unblocks Step 10 (PowerUserStatistics needs new `pus_*` label
PNGs).

**Implementation guidance**

1. Create `scripts/gen_custom_option_labels.py`:
   - In-script `LABELS = { id: text, ... }` dict.
   - Migrate every entry from the existing two scripts.
   - Add the four new entries:
     ```python
     "seop_item_pus_timing_stats":         "TIMING STATISTICS",
     "seop_item_pus_pacemaker_to_mserror": "PACEMAKER -> MS ERROR",
     "seop_item_pus_pacemaker_threshold":  "WHITE THRESHOLD",
     "seop_item_pus_step_data_export":     "EXPORT STEP DATA (CSV)",
     ```
   - Render each label via Pillow (or the existing rendering helper if
     already abstracted) to a PNG matching the existing aesthetic
     (font, size, anti-aliasing).
   - Output path: `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/<id>.png`.

2. Run the script. Confirm parity with existing labels by visual diff
   against the previous output.

3. Delete `scripts/gen_webui_option_labels.py` and
   `scripts/gen_scroll_dummy_labels.py`.

4. Update any documentation that references the old scripts
   (`README.md`, `.spec/steering/structure.md` if applicable).

**Test requirements**

The script runs without error and produces all expected PNGs. Visual
diff confirms parity with the old scripts.

**Integration with previous work**

Independent. Tooling foundation for Step 10.

**Demo**

`python -m scripts.gen_custom_option_labels`. Output directory contains
the expected PNGs (existing WebUI labels at parity, plus 4 new
`pus_*` entries). Open one of the new labels in an image viewer to
spot-check rendering.

---

### Step 7: SongSelectionImprovementsMod skeleton + JSON config

**Objective**

Ship the bare skeleton of `SongSelectionImprovementsMod`: registered,
appears in the mod menu, reads the JSON config section. Subsequent
steps (8 and 9) plug in the actual sub-features.

**Implementation guidance**

1. Create `src/mods/song_selection_improvements/mod.rs` (multi-file
   directory pattern). Empty submodules `real_speed.rs`,
   `logf_stub.rs`, `flare_lamps.rs` for now (just a `pub fn enable()`
   / `pub fn disable()` no-op apiece).

2. Add `pub mod song_selection_improvements;` to `src/mods/mod.rs`.

3. Add JSON-config parsing logic in `mod.rs`:
   - Read `mod-config.json`'s `song_selection_improvements` section
     via `mods::config`.
   - Default to `{ real_speed_core_bpm: true, flare_to_clear_lamps: true }`.

4. Implement the `Mod` trait:
   - `id() = "song-selection-improvements"`.
   - `enable()` calls each sub-feature module's `enable()` if its
     toggle is ON.
   - `disable()` calls each sub-feature module's `disable()`.

5. Register in `src/lib.rs`.

**Test requirements**

`cargo check` succeeds. Mod appears in the in-game mod menu. JSON
config section is read; missing section falls back to defaults
gracefully.

**Integration with previous work**

Builds on Step 1 (mod menu opens during any scene).

**Demo**

Deploy. Open mod menu, see "Song Selection UX Improvements" listed.
Toggle ON / OFF. No behavior changes yet (sub-features are no-op).
Edit `mod-config.json`, set `song_selection_improvements.real_speed_core_bpm = false`,
restart game. Open mod menu, toggle ON. Log line confirms the
sub-toggle was read and the sub-feature was skipped.

---

### Step 8: SongSelectionImprovementsMod / Real Speed sub-feature

**Objective**

Implement the BPM divisor swap (Max BPM → Core BPM) and logf guard.
Both target `ddr::player::Option::SetScrollSpeed` and its caller's
display formula.

**Implementation guidance**

1. Add new signatures to `src/core/signatures.rs`:
   - `real_speed_bpm_anchor`: `F2 0F 5E 01 48 8D 4C 24 40`
   - `real_speed_logf_anchor`: `0F 28 C7 E8 ?? ?? ?? ?? F3 0F 58 C6`

2. In `song_selection_improvements/logf_stub.rs`:
   - At enable: read the bare `logf` address from the stock R16 site
     (`r16_anchor + 4` is the rel32; resolve to absolute address).
   - Allocate a 14-byte stub via `core::memory::alloc_near`.
   - Assemble the guarded-logf stub bytes per the design:
     ```
     0F 57 C9              ; xorps  xmm1, xmm1
     0F 2E C1              ; ucomiss xmm0, xmm1
     75 01                 ; jne    +1
     C3                    ; ret
     E9 <rel32 bare_logf>  ; jmp bare logf
     ```
   - Save the stub address; expose it for `real_speed.rs` to use.

3. In `song_selection_improvements/real_speed.rs`:
   - At enable:
     - Resolve `r24/r25/r26/r15/r16` patch addresses from the two
       anchors (offsets per design's table).
     - Save stock bytes for restoration.
     - Write patches in order:
       - R24: `EB 64`
       - R25: `C2`
       - R26: `F2 0F 10 93 88 00 00 00 77 97 EB 90` (12 bytes)
       - R15: `0x37`
       - R16: rel32 to the logf stub (computed: `stub - (r16_anchor + 4 + 4)`)
   - At disable: restore stock bytes and free the stub.

**Test requirements**

`cargo check` succeeds. Deploy + manual demo.

**Integration with previous work**

Builds on Step 7 (sub-feature plugs into the existing
`song_selection_improvements/mod.rs` skeleton).

**Demo**

Deploy. Open mod menu, toggle "Song Selection UX Improvements" ON.
Open song select. Pick a song with a wide BPM range (e.g., a song
with BPM `100-300`). The Real Speed display before song-pick should
reflect Core BPM, not Max BPM (visibly different scroll multiplier).
Pick the song; observe the loading screen — no `NaN` or `-inf` text
where the Real Speed display would otherwise show garbage. Toggle
the mod OFF, restart game, repeat — vanilla behavior returns
(Max-BPM-based Real Speed, with potential brief NaN flash before
song starts).

---

### Step 9: SongSelectionImprovementsMod / Flare→Lamps sub-feature

**Objective**

Replace the flare-clear banner on the results screen with clear-lamp
colors (MFC = white FLARE EX, PFC = gold FLARE IX, etc.).

**Implementation guidance**

1. Add new signature to `src/core/signatures.rs`:
   - `flare_lamps_anchor`: `48 8B 11 83 3A 01 0F 45 F0` (anchor − 12
     is the call site).

2. In `song_selection_improvements/flare_lamps.rs`:
   - At enable:
     - Resolve `flare_lamps_anchor − 12` to the call instruction.
     - Install a `retour::GenericDetour` on the call target (the
       stock flare-clear getter, `FUN_1800f2700`).
     - In the detour callback, optionally call `FUN_1800f3c00`
       (clear-lamp getter) instead and remap via the lookup table.
   - The remap table values are protocol-stable and can be a Rust
     `const` array. Verify on first deploy by logging the in/out
     pairs once.

3. Verify the `FUN_1800f2700` and `FUN_1800f3c00` addresses are
   reachable on both versions. Use `scanner::decode_call_rel32` to
   walk from `flare_lamps_anchor − 12` to find their absolute
   addresses.

**Test requirements**

`cargo check` succeeds. Deploy + manual demo.

**Integration with previous work**

Builds on Step 7 (plugs into the existing
`song_selection_improvements/mod.rs` skeleton).

**Demo**

Deploy. Toggle "Song Selection UX Improvements" ON. Play a song to
completion. On the results screen, the flare-clear banner shows
clear-lamp colors instead of stock flare grades (e.g., a Marvelous
Full Combo shows white "FLARE EX" instead of stock "FLARE 9";
similar for Perfect Full Combo → gold "FLARE IX"). Disable the
sub-feature via JSON config (`flare_to_clear_lamps: false`),
restart, replay — stock flare grades return.

---

### Step 10: PowerUserStatisticsMod skeleton + custom_options registration

**Objective**

Ship the bare skeleton of `PowerUserStatisticsMod`: registered,
appears in the mod menu, registers the four per-player options on
the Mods tab. Subsequent steps (11-14) plug in the data feed,
widgets, pacemaker swap, and CSV export.

**Implementation guidance**

1. Create `src/mods/power_user_statistics/mod.rs` (multi-file
   directory). Empty submodules: `data_feed.rs`,
   `timing_stats_widget.rs`, `pacemaker_swap.rs`, `csv_export.rs`.

2. Add `pub mod power_user_statistics;` to `src/mods/mod.rs`.

3. In `mod.rs`, implement the `Mod` trait:
   - `id() = "power-user-statistics"`.
   - On `enable()`, register the four custom options via
     `services::custom_options::register_option`:
     - `pus_timing_stats`: `RegisterSpec::bool_toggle("pus_timing_stats")`
     - `pus_pacemaker_to_mserror`: `RegisterSpec::bool_toggle(...)`
     - `pus_pacemaker_threshold`: `RegisterSpec::scalar("pus_pacemaker_threshold", 1, 50, 1, ScalarFormat::Integer)`
       with `.show_when(ShowWhen::Equals { parent_id: "pus_pacemaker_to_mserror".into(), value: 1 })`
     - `pus_step_data_export`: `RegisterSpec::bool_toggle(...)`
   - Each option has a no-op `on_change` callback for now; later
     steps wire in real callbacks.

4. Register the mod in `src/lib.rs`.

**Test requirements**

`cargo check` succeeds. The four options appear on the Mods tab
(Page6) when a player swipes their card and opens the options menu.
The `pus_pacemaker_threshold` row appears only when
`pus_pacemaker_to_mserror` is ON (verified by toggling it on and
seeing the threshold row appear).

**Integration with previous work**

- Step 6 (label PNGs for the four new option IDs are shipped via
  LayeredFS).
- Step 1 (mod menu enables/disables the umbrella mod).

**Demo**

Deploy. Open mod menu, see "Power User Statistics" listed. Toggle
ON. Swipe card, open options menu, navigate to the Mods tab. See:
- `TIMING STATISTICS    [OFF / ON]`
- `PACEMAKER -> MS ERROR [OFF / ON]`
- `EXPORT STEP DATA (CSV) [OFF / ON]`
Toggle `PACEMAKER -> MS ERROR` to ON; the row immediately below
expands to show:
- `WHITE THRESHOLD       [10]`
which is editable. Toggle the parent OFF; the threshold row hides.

---

### Step 11: PowerUserStatisticsMod / per-step ms-error data feed

**Objective**

Hook `FUN_1800603a0` (the per-step judgment-result handler) and
populate a per-player ms-error buffer. This is the foundation for
the three sub-features (Timing Stats, Pacemaker→MS, CSV Export).

**Implementation guidance**

1. Add new signature to `src/core/signatures.rs`:
   - `judge_per_step_handler` — AOB pattern documented in
     `research/per-step-data-feed.md`.

2. In `power_user_statistics/data_feed.rs`:
   - Define the `MsErrorAccum` struct per the design.
   - Static `MS_ERROR_BUFFER: Lazy<[Mutex<MsErrorAccum>; 2]>`.
   - Install a `retour::GenericDetour` on the resolved
     `judge_per_step_handler` address.
   - Detour callback:
     - Wrap body in `std::panic::catch_unwind`.
     - Read the function's args per its calling convention. Compute
       `delta_ms = result.actual_ts - note.expected_ts`.
     - Determine player side from the actor (offset documented in
       research).
     - Lock the per-player buffer's Mutex briefly:
       - Update `current`, `max_abs`, `sum_abs`, `sum`, `count`.
       - If `pus_step_data_export[side]` is ON AND `per_step` is
         `Some(_)`, push a new `StepRecord`.
     - Drop lock, then call the original detour function.
   - Reset on song-start: `scene_manager::on_scene_change` callback,
     when transitioning into scene 28, zero the accumulators
     (preserve `per_step` if `pus_step_data_export` ON; allocate
     fresh `Vec` if needed).

3. The buffer is the single source of truth for the three
   sub-features; later steps read from it.

**Test requirements**

`cargo check` succeeds. Deploy with one-shot diagnostic logging
that prints `current` after every step for the first ~20 steps of
a song. Verify values are non-zero, signed, and consistent with
play timing.

**Integration with previous work**

- Step 10 (option flags drive the lazy `per_step` allocation).
- Existing services (judge_hook for `judgeNotes`'s per-frame timing,
  although this hook is OUTSIDE judge_hook — it's a separate detour
  we install at `FUN_1800603a0` directly).

**Demo**

Deploy with diagnostic logs. Play a song. Observe in the spice2x
log a stream of lines like:
```
[DDR-Hook][INFO] data_feed: side=0, delta=+12 ms, count=42, max_abs=58
[DDR-Hook][INFO] data_feed: side=0, delta=-3 ms, count=43, max_abs=58
```
Remove the diagnostic logs before committing.

---

### Step 12: PowerUserStatisticsMod / Timing Stats widget

**Objective**

Render per-player text widgets during scene 28 showing Current,
Max, Abs(μ), and μ ms-error. Per-player gating via the
`pus_timing_stats` option.

**Implementation guidance**

1. In `power_user_statistics/timing_stats_widget.rs`:
   - Allocate widget groups lazily on first scene-28 entry per side.
     Each group is 4 stacked `TextWidget`s.
   - Per-side position constants in source (e.g. P1 left, P2 right).
   - Per-frame update: schedule via
     `widget_renderer::run_on_render_thread`, lock the `MsErrorAccum`
     briefly to read values, format them as `"+12.34 ms"` strings,
     call `TextWidget::set_text`.
   - Visibility: only during scene 28 (use scene_manager callback to
     show/hide). Per-side gating: hide a side's group if
     `pus_timing_stats[side]` is OFF.
   - On `disable()`, destroy all widgets (release the renderer pool
     slots).

2. Wire the widget update into a per-frame tick. Two options:
   - Call from inside the `FUN_1800603a0` detour after updating the
     buffer (per-step rather than per-frame, but sufficient — the
     widget only needs to reflect the most-recent step).
   - Schedule a `run_on_render_thread` closure from the data_feed
     buffer-update path.

**Test requirements**

`cargo check` succeeds. Deploy + manual demo.

**Integration with previous work**

- Step 11 (reads the shared per-player ms-error buffer).
- Step 10 (per-player option toggle gates visibility).

**Demo**

Deploy. Toggle "Power User Statistics" ON in the mod menu. Swipe
card, set `TIMING STATISTICS` to ON, save. Start a song. During
gameplay, see the player's per-step ms-error displayed in their
half of the screen with 2-decimal precision (e.g.,
`CURRENT  +12.34 ms`). Numbers update every step. Disable the
option mid-song (via the options menu — but probably not feasible;
the option is read at song-start). Verify P2's widget doesn't
appear if P2 didn't enable the option.

---

### Step 13: PowerUserStatisticsMod / Pacemaker→MsError swap

**Objective**

Two `retour::GenericDetour` handles on `FUN_180077a00`'s R13 and R14
sites. When `pus_pacemaker_to_mserror[side]` is ON, swap the
pacemaker readout for the most-recent ms-error and force the
pacemaker color to white when `|ms_error| < pus_pacemaker_threshold`.

**Implementation guidance**

1. Add two new signatures to `src/core/signatures.rs`:
   - `pacemaker_render_input`: `48 8B 97 B0 00 00 00`
   - `pacemaker_render_zf`: `48 8B 01 85 F6 75 ?? F3 0F 10 0D`

2. In `power_user_statistics/pacemaker_swap.rs`:
   - At enable:
     - Resolve both anchors.
     - Install `retour::GenericDetour` on each.

3. **R13 detour** (`mov rdx, [rdi+0xb0]` site):
   - Determine player side from calling convention (`r13` register
     per research).
   - If `pus_pacemaker_to_mserror[side]` is ON:
     - Lock the buffer briefly, read `current` ms-error.
     - Write the ms-error to `[r14 + 8]` (the formatter input slot).
   - Always run the original `mov rdx, [rdi+0xb0]` (or its trampoline
     equivalent — retour handles this).

4. **R14 detour** (`mov rax, [rcx]; test esi, esi` site):
   - Same player-side determination.
   - If option is ON AND `|current| < pus_pacemaker_threshold[side]`:
     - Force ZF=1 in the EFLAGS register that the subsequent
       `jne` reads.
   - Otherwise, behave like the stock instructions.

5. Both detours wrap their bodies in `std::panic::catch_unwind`.

**Test requirements**

`cargo check` succeeds. Deploy + manual demo.

**Integration with previous work**

- Step 11 (reads the shared ms-error buffer).
- Step 10 (per-player option flags + threshold).

**Demo**

Deploy. Set `pus_pacemaker_to_mserror = ON`, threshold = 15. Play a
song. Pacemaker readout shows `+12 ms` instead of `+5000 score`.
Color is white when |error| < 15ms; flips back to vanilla colors when
the player is consistently late/early. Disable the option, restart,
replay — vanilla pacemaker behavior returns.

---

### Step 14: PowerUserStatisticsMod / CSV Export

**Objective**

Write per-player CSV files at scene 28→29 transitions for any side
whose `pus_step_data_export` was ON during the song.

**Implementation guidance**

1. In `power_user_statistics/csv_export.rs`:
   - At enable, ensure `./step_data_exports/` exists (create if
     missing, log warn-and-skip if create fails).
   - Snapshot `SongIdentity` at song-start: hook the
     non-gameplay → gameplay scene transition via
     `scene_manager::on_scene_change`. On entry to scene 28:
     - For each player side whose `pus_step_data_export` is ON:
       - Read the session struct (path documented in research note).
       - Extract songcode, difficulty.
       - Set `MsErrorAccum.song_start_snapshot = Some(SongIdentity { ... })`.
   - Lazy-allocate `MsErrorAccum.per_step = Some(Vec::new())` for any
     side whose option is ON (Step 11 already does this; ensure
     consistency).
   - On scene 28 → 29 transition (or scene 28 → any non-28):
     - For each side whose `song_start_snapshot.is_some()`:
       - Write the per-step rows to the CSV file with the documented
         filename format.
       - Reset the buffer (keep the option flag, drop per_step and
         snapshot).

2. Filename format:
   `./step_data_exports/<YYYY-MM-DD>_<HH-MM-SS>_<songcode>_<difficulty>_P<n>.csv`

3. CSV format:
   ```
   Expected,Actual,Delta (Ms Error)
   <expected_ms>,<actual_ms>,<delta_ms>
   ...
   ```
   Use `\r\n` line endings.

4. Failure handling: any I/O error → log warn, do not crash.

**Test requirements**

`cargo check` succeeds. Deploy + manual demo.

**Integration with previous work**

- Step 11 (reads `per_step` and `song_start_snapshot` from the buffer).
- Step 10 (per-player option flag).

**Demo**

Deploy. Toggle `EXPORT STEP DATA (CSV) = ON` for P1. Play a song.
After song-end (transition to results), check
`./step_data_exports/`. Find a file like:
`2026-05-23_18-42-15_acef_single_difficult_P1.csv`. Open it; first
line is the header, subsequent lines are signed-int triples per step.
Row count matches the song's note count. Toggle OFF for P2; only
P1's file should appear.

---

### Step 15: End-to-end verification + cross-version (20250805 stock) re-test

**Objective**

Final integration test: every mod is enabled simultaneously, plays
together cleanly, and works on both 20250805 stock and 20260421.

**Implementation guidance**

1. Deploy on 20260421 with all four new mods enabled. Play a multi-song
   session covering:
   - Premium Free (verify stage counter freezes; verify scores save
     across multiple songs).
   - Quick Restart (verify mid-song restart works; verify combination
     with Premium Free leaves stage counter frozen).
   - Quick Fail (verify mid-song fail-out works; verify combination
     with Premium Free is consistent).
   - Real Speed (verify Core BPM display).
   - Flare→Lamps (verify clear-lamp colors on results).
   - Power User Stats (Timing Stats widget, Pacemaker→MS swap, CSV
     Export — all three).

2. Switch the test deploy to 20250805 stock (whichever mechanism the
   user uses for version switching). Repeat the smoke test.

3. Address any cross-version regressions. The most likely failure is
   Quick Fail's flag-write target offset (`0xE8`) shifting between
   versions — re-verify per the research note.

4. Document any deviations in
   `.spec/learnings/sdd-software-developer.md` if they reveal new
   patterns worth capturing.

**Test requirements**

All demos from steps 2-14 reproduce on both versions without
regression.

**Integration with previous work**

End-to-end validation of all preceding steps.

**Demo**

A 30-minute play session on 20260421 with every new mod enabled,
exercising each feature. Then a 15-minute smoke test on 20250805
stock confirming key features (PremiumFree, Real Speed, Pacemaker→MS,
CSV Export) still work. Both sessions clean — no crashes, no log
errors, no visual artifacts.

---

## Notes on Sequencing Rationale

- **Step 1 first** because it removes a testing friction point (every
  subsequent step's deploy + demo is easier when we can open the mod
  menu mid-gameplay to toggle).
- **Step 2 (PremiumFree) before Steps 3-5 (QuickRestart/Fail)** because
  PremiumFree is self-contained while QuickRestart/Fail needs the new
  scene_manager accessor (Step 3).
- **Step 6 (label-gen consolidation) before Step 10 (PUS skeleton +
  options)** because PUS needs the new label PNGs.
- **Steps 7-9 (SongSelectionImprovements) before Step 10-14 (PUS)**
  for ordering convenience — SongSelectionImprovements has fewer
  internal dependencies and its deploy is faster.
- **Step 15 last** — full cross-version smoke test should be the final
  gate before merging to main.

## Open Questions Tracked Through Implementation

These appear in the design's "Constraints and Limitations" appendix
and resolve during implementation rather than design:

1. **Premium Free score-save behavior** (Step 2 demo). Verify scores
   appear in the backend; if not, escalate to RE on the save path.
2. **Quick Restart accumulator pollution** (Step 4 post-deploy
   verification). May need a per-stage block-zero before transition.
3. **Quick Fail mid-song timing** (Step 5 implementation). The exact
   second `scene_id` argument for `FUN_18002de40` is determined by a
   diagnostic-build phase.
4. **Flare→Lamps remap table version-stability** (Step 9). Verify on
   first deploy and decide between hardcoded const vs. ARC/IFS lookup.
