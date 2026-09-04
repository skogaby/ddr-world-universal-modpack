# Training Mode — Loop / Marker / Timeline Revisions: Detailed Design

Status: Approved 2026-09-04

## 1. Overview

The shipped Training Mode (`training-mode` mod) lets a player set a song
section (SONG START TIME / SONG END TIME rows, or pinpad 4/6 markers during
gameplay), optionally LOOP it, and scrub with 7/9, with a chart-strip timeline
HUD. Two defects motivate this revision:

1. **READY-banner soft-lock.** Pressing **6** (set end marker) while the
   "READY?" banner is up, with LOOP SONG off, soft-locks the game. The
   gesture reads the GamePlayActor `+0x178` music count before the game has
   anchored the run's clock; pre-anchor that field holds the raw frame tick
   (minutes-since-boot scale — cabinet finding 2026-08-14, already defended
   against inside the loop driver), so a garbage section end is written into
   the ControlMessageActor thresholds of a run whose DancePlaySequence (DPS)
   is still in its pre-song init states.
2. **Product model.** A section should only be playable as a *loop*. Today a
   section with LOOP off truncates the song to a one-shot early end. That
   behavior is retired: SONG START/END TIME become child rows of LOOP SONG,
   the 4/5/6 marker hotkeys are inert unless the song is looping, and the
   timeline HUD draws its section decorations (blue veil, A/B lines) only for
   looping songs. The cursor, readout, and strip remain in every scenario.

No new signatures, detours, or memory offsets. Everything rides existing
`song_reset` predicates and the custom-options framework.

## 2. Detailed Requirements

Consolidated from the accepted decision register (D1–D13).

| Req | Statement |
|---|---|
| R1 | Every training gesture (pinpad 4, 5, 6, 7, 9) is dropped unless the run is **in-song**: DPS at its in-song step, every GamePlayActor at its in-song step with a nonzero clock anchor, AND the live music count is credible (`count < chart_end`). (D1, D2) |
| R2 | The in-song predicate is ONE shared function in `services/song_reset`, used by both the gesture surface and the loop driver's initial bound compute. (D1) |
| R3 | Pinpad 4/5/6 are additionally dropped unless LOOP SONG is **latched for the current song** (`bounds::loop_latched()`). 7/9 scrub is NOT loop-gated. (D3) |
| R4 | SONG START TIME / SONG END TIME are child rows of LOOP SONG, visible only when LOOP SONG = ON for that side. LOOP SONG is registered before them. (D7) |
| R5 | With LOOP SONG off, retained START/END values are **ignored** everywhere they are read: the GAMEPLAY-entry engagement predicate, the row-derived resolution, and the bind-time pre-shift. The song resolves as defaults (no A/B, no threshold writes, no pre-shift). Toggling LOOP SONG refreshes the pre-shift. Values are retained (not reset) so toggling back ON restores the section. (D4) |
| R6 | The v1 "LOOP OFF + section end ⇒ early natural end" behavior is retired. The pure `end_policy` function and its tests remain (defensive). (D5) |
| R7 | Timeline HUD: veil and A/B lines render only while LOOP SONG is latched for the song; cursor, readout, and strip/fallback track are unconditional (subject to the existing TIMELINE PLACEMENT visibility). (D6) |
| R8 | Refused-press feedback: READY window ⇒ silent (`log_debug`). LOOP OFF during gameplay ⇒ one toast per song on the first refused 4/5/6 press: "Enable LOOP SONG to set markers". (D9) |
| R9 | Shipped `mod-config.json` `option_menu_settings` order becomes loop → start → end → placement. (D8) |
| R10 | No new signatures/detours/offsets; pure gate logic lives in `section_math.rs` with host tests; versus mirror and label textures unchanged. (D10–D12) |
| R11 | Documentation reflects the new model (AGENTS.md training rows, README, `docs/training_mode_research.md`). |

Assumptions: the pre-anchor music-count finding (2026-08-14) is the operative
root cause chain of the soft-lock; the gate removes every candidate link
regardless of which one actually wedged. `ShowWhen::Equals` child visibility
works identically in the in-game MODS tab and the overlay menu (proven by the
`preserve_pitch` / `assist_tick_volume` precedents).

## 3. Architecture Overview

```mermaid
flowchart LR
    subgraph services/song_reset
        RIS[run_in_song()]
        FAF[first_anchored_frame()]
        CNT[current_raw_music_count()]
        END[chart_end_raw()]
        RIS --> FAF
        RIS --> CNT
        RIS --> END
    end
    subgraph mods/training_mode
        IN[bounds::on_input_event] -->|R1| RIS
        IN -->|R3 4/5/6| LL[bounds::loop_latched]
        IN --> GG[section_math::gesture_gate]
        DRV[driver::loop_step] -->|R2| RIS
        RES[bounds::try_resolve_row_bounds] -->|R5| LR[row_loop_song]
        SC[bounds::on_scene_change rows_engaged] -->|R5| LR
        PS[mod.rs refresh_pre_shift] -->|R5| LR
        HUD[strip_hud::overlay_update] -->|R7| LL
        HUD --> DV[section_math::decorations_visible]
    end
    subgraph custom_options
        LOOP[training_loop_song] -->|ShowWhen::Equals 1| START[training_start_time]
        LOOP -->|ShowWhen::Equals 1| ENDR[training_end_time]
    end
```

All changes are inside `src/services/song_reset/mod.rs` (one new predicate),
`src/mods/training_mode/{mod.rs,bounds.rs,driver.rs,strip_hud.rs,section_math.rs}`,
`mod-config.json`, and docs.

## 4. Components and Interfaces

### 4.1 `song_reset::run_in_song() -> bool` (new)

```rust
/// The run is live and its music count is trustworthy: first_anchored_frame()
/// AND the `+0x178` count reads below the (min over sides) raw chart end.
pub fn run_in_song() -> bool
```

Composition of three existing accessors. `first_anchored_frame` keeps its
name and doc (its doc gains a note that it is a state predicate). The loop
driver's `loop_step` initial-compute guard (`first_anchored_frame` + inline
credibility match) is replaced by `run_in_song()`; the driver's
`adjust_pending && first_anchored_frame()` use stays as-is (the adjust
deliberately fires on the first anchored frame and re-anchors the count
itself).

### 4.2 Pure gate logic — `section_math.rs`

```rust
pub enum GestureKind { Marker, Scrub }          // 4/5/6 vs 7/9
pub enum GestureVerdict { Allow, DropPreSong, DropLoopOff }

/// Total decision for one press. Pre-song wins over loop-off (a READY-window
/// press is never reported as a loop problem).
pub fn gesture_gate(kind: GestureKind, in_song: bool, loop_latched: bool) -> GestureVerdict

/// Whether the HUD's section decorations (veil + A/B lines) render.
pub fn decorations_visible(loop_latched: bool) -> bool
```

Host-tested: the 2×2×2 truth table for `gesture_gate`; the two-case
`decorations_visible`. Trivial by design — the value is one named decision
point that the input callback, the HUD, and the tests all reference.

### 4.3 Gesture surface — `bounds::on_input_event`

After the existing `Pressed` / `GESTURES_ACTIVE` / button-class / scene
checks, compute `verdict = gesture_gate(kind, song_reset::run_in_song(),
loop_latched())`:

- `Allow` → existing dispatch (`set_marker` / `clear_live_bounds` / `scrub`).
- `DropPreSong` → `log_debug!`, return.
- `DropLoopOff` → if `!LOOP_HINT_SHOWN.swap(true)`:
  `toast::flash("Enable LOOP SONG to set markers")`; `log_debug!`; return.

`LOOP_HINT_SHOWN: AtomicBool` is per-song state cleared in
`clear_session_state`.

### 4.4 Row hierarchy — `mod.rs::register_bound_rows`

Registration order becomes LOOP SONG → SONG START TIME → SONG END TIME →
TIMELINE PLACEMENT. START/END specs gain
`.show_when(ShowWhen::Equals { parent_id: OPT_LOOP_SONG.into(), value: 1 })`.
Framework behavior: parent value changes call `update_children_visibility`
per side; hidden rows keep their registry values. `Duplicate` on re-enable
remains success.

`on_loop_song_change(side, value)` additionally calls `refresh_pre_shift()`
after storing the atomic (R5's pre-shift refresh trigger).

### 4.5 Retained-but-ignored — the three readers

1. **`bounds::on_scene_change` (GAMEPLAY entry):**
   `rows_engaged = (0..2).any(|side| row_loop_song(side))`. START/END alone no
   longer arm the resolution.
2. **`bounds::try_resolve_row_bounds`:** after the loop latch block, if
   `!loop_on` the function stores `CHART_END_MS`, clears
   `RESOLUTION_PENDING`, runs `apply_end_policy()` (a `Natural` no-op) and
   returns `true` — START/END are never converted into A/B. (Reachable only
   from re-entrant/defensive paths since (1) no longer arms it for loop-off
   songs; kept as the resolution's own safety property.)
3. **`mod.rs::refresh_pre_shift`:** the armed branch requires
   `bounds::row_loop_song(side)` for the governing side (`pre_shift_side()`);
   otherwise the mapping is cleared (`set_initial_content_mapping_ms(0,0,0)`).

Everything downstream (driver arm, loop leg, taint) is unchanged: with
LOOP OFF nothing sets `SESSION_ACTIVE` except a scrub, which taints exactly as
today.

### 4.6 HUD — `strip_hud::overlay_update`

`let decorate = section_math::decorations_visible(bounds::loop_latched());`
- veil: existing span computation executes only when `decorate`; otherwise
  `veil.hide()`.
- A/B lines: `place_line(line_a, decorate.then(|| ...), ...)` — the existing
  `Option` parameter already hides on `None`.
- cursor / readout / track: unchanged.

`hide_overlay()` unchanged (still hides everything on gate-off).

### 4.7 Config and docs

`mod-config.json` `option_menu_settings`: reorder the four training ids to
`training_loop_song`, `training_start_time`, `training_end_time`,
`training_progress_pos`. README gains a sentence that START/END live under
LOOP SONG and markers require looping; AGENTS.md training rows and
`docs/training_mode_research.md` note the retired LOOP-OFF section end and
the READY gate.

## 5. Data Models

No persistent data changes. Per-song in-memory state gains one
`AtomicBool` (`LOOP_HINT_SHOWN`) in `bounds.rs`, reset in
`clear_session_state`. Row ids, `PersistMode`s, versus-mirror set, and wire
fields are unchanged.

State summary for one song:

| State | Owner | Set | Cleared |
|---|---|---|---|
| `LOOP_LATCHED` | bounds | resolution (loop row ON) | session clear, `on_loop_disarmed` |
| `LOOP_HINT_SHOWN` (new) | bounds | first refused 4/5/6 with loop off | session clear |
| `RESOLUTION_PENDING` | bounds | GAMEPLAY entry iff any side's loop row ON (changed) | resolution |

## 6. Error Handling

All fail-open, matching the mod's existing ladder:

- `run_in_song()` returns `false` on any unreadable DPS/actor/count/threshold
  → gestures drop (conservative; the song is never disturbed). A permanently
  unreadable predicate would make gestures dead for the song — the loop
  driver already tolerates the same condition (it never arms), so this
  introduces no new failure class.
- Toast unavailable → the hint is skipped silently.
- `ShowWhen` registration failure of a child row → existing per-row WARN and
  `return false` (rows degraded, gestures still work).
- Pre-shift refresh from the LOOP callback uses the same `ACTIVE` gate as
  every other refresh.

## 7. Testing Strategy

**Host (pure):** `section_math` tests for `gesture_gate` (full truth table,
pre-song precedence) and `decorations_visible`. Run via a new
`scripts/validate_training_mode.sh` (the `validate_auto_calibration.sh`
temp-crate pattern, mounting `src/mods/training_mode/section_math.rs`, which
is dependency-free). `cargo check --target x86_64-pc-windows-msvc` and
`./build.sh` clean.

**Cabinet checklist (the only validation for the engine-facing gates):**

1. LOOP OFF, press 6 repeatedly during READY → no toast, no marker, song
   starts and ends normally (the soft-lock repro).
2. LOOP ON, press 4/6 during READY → nothing; after the arrows start, 4/6
   set markers as before.
3. LOOP OFF, mid-song press 4 → one toast "Enable LOOP SONG to set markers",
   no marker; press 6 again → no second toast; HUD shows strip + cursor +
   readout only (no veil, no A/B lines).
4. LOOP OFF, mid-song press 7/9 → scrub works, indicator flashes, HUD cursor
   follows.
5. Options menu: LOOP SONG OFF hides SONG START/END TIME; ON shows them
   directly beneath (both in-game MODS tab and overlay).
6. Set START/END with LOOP ON, toggle LOOP OFF, play → song plays whole from
   0 (no pre-shift, no truncation). Toggle LOOP ON → section values are
   still there and the loop grinds the section.
7. Versus: both sides see the same LOOP state and child visibility.
8. Quick restart (1) mid-loop and restart-from-A still work.

## Appendix A — Alternatives considered

- **Gate on `quick_restart_or_fail::dps_pre_song()`**: weaker (DPS step only,
  no actor/anchor/count check) and private to that mod. Rejected for
  `song_reset::run_in_song()`.
- **Make gestures WORK during READY** (set a marker at content 0): the
  pre-anchor count is not a position at all; there is nothing meaningful to
  set. Rejected per the maintainer's direction.
- **Reset START/END when LOOP toggles OFF**: destroys the user's section on a
  momentary toggle; the framework already retains hidden values
  (`preserve_pitch`). Rejected for retained-but-ignored.
- **Hoist the DPS step constants shared by `quick_restart_or_fail` and
  `song_reset`**: pre-existing duplication, out of scope.
