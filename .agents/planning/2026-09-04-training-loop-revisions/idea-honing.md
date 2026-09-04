# Idea Honing — Training loop / marker / timeline revisions

Decision register. Status ∈ Proposed / Accepted / Overridden / Assumed / Open.
Ordered by blast radius (user-visible behavior first, cosmetic last).
★ = the maintainer likely hasn't considered this one.

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | READY-window gate predicate for training gestures | Root of the soft-lock: gestures read the `+0x178` music count before the run has an anchor (garbage tick). | Gate on `song_reset::first_anchored_frame()` **+** the driver's count-credibility check (`count < chart_end`), hoisted into ONE shared `song_reset` predicate (`run_in_song()` or similar) that the loop driver also switches to. Not `dps_pre_song` (weaker, private to quick_restart). | Accepted |
| D2 ★ | Which gestures the READY gate covers | 7/9 scrub shares the same garbage-count exposure; the user only named 4/5/6. | ALL training gestures (4/5/6 **and** 7/9) drop until `run_in_song()`. | Accepted |
| D3 | LOOP-OFF gesture lockout source of truth | "Is looping on for this song" must survive a mid-song disarm and match what the driver does. | Gate 4/5/6 on the per-song latch `bounds::loop_latched()` (set at resolution, dropped by `on_loop_disarmed`), NOT the raw row value. 7/9 scrub stays available with LOOP OFF (pure timeline adjuster; not loop-specific). | Accepted |
| D4 ★ | Row-value semantics while LOOP is OFF | Hidden rows keep their registry values (framework behavior); a retained START>0 would still pre-shift the bank / a retained END would still truncate. | Values are RETAINED but IGNORED: `rows_engaged`, `try_resolve_row_bounds`, and `refresh_pre_shift` all consult the governing side's loop row; with LOOP OFF the song resolves as defaults (a=b=0, no threshold writes, no pre-shift). `on_loop_song_change` triggers `refresh_pre_shift`. Preserve_pitch precedent (toggle OFF→ON restores your section). Rejected: reset START/END to seeded defaults on LOOP OFF — loses the user's section for a momentary toggle. | Accepted |
| D5 | Retire the v1 "LOOP OFF + section end = early natural end" behavior | This is the explicit product change: a section is only playable in a loop. | Yes. `section_math::end_policy`'s `WriteThresholds` arm becomes unreachable from input; KEEP the pure fn + tests (defensive, zero cost) but delete `bounds::write_end_thresholds`' reachable call path only if it simplifies — otherwise leave. Update AGENTS.md / README / research doc statements. | Accepted |
| D6 | Timeline HUD with LOOP OFF | User-specified: keep strip + cursor; hide veil + A/B lines. | Keyed off `bounds::loop_latched()` per frame in `overlay_update` (same source as D3 — a mid-song disarm hides veil/markers too). Cursor + readout + strip texture unconditional. Reverses the 2026-08-15 "always shade the active region" amendment for non-loop songs only. | Accepted |
| D7 | Row hierarchy + registration order | Framework requires the parent registered first; `ShowWhen::Equals` hides children at the parent's OFF. | Register LOOP first, then START/END with `ShowWhen::Equals { training_loop_song, 1 }`, then PLACEMENT. Both menus (in-game MODS tab + overlay) inherit visibility from the framework (song_speed → preserve_pitch precedent). | Accepted |
| D8 | Shipped `option_menu_settings` order | Config drives display order; children should sit under their parent. | Change `mod-config.json` to loop → start → end → placement. Operator configs with the old order keep working (framework hides children regardless of position); note it in README. | Accepted |
| D9 ★ | Feedback on a refused press | Silent drops can read as "the hotkey is broken". | READY window: silent (`log_debug`) — a toast over the READY banner is noise. LOOP OFF during gameplay: ONE short toast per song on the first refused 4/5/6 press, "Enable LOOP SONG to set markers" (reuses `services::toast::flash`). Alternative: fully silent both cases. | Accepted |
| D10 | Scope: no new signatures / detours / offsets | Keeps the change to the fail-open, host-testable layer. | Everything rides existing `song_reset` predicates + custom-options framework. Any new pure gate logic (e.g. a `gesture_gate(state) -> Verdict` fn) lands in `section_math.rs` with host tests. | Accepted (assumed, confirmed) |
| D11 | Versus mirror unchanged | Same four rows; the hierarchy doesn't change what's mirrored. | No change to `MIRRORED_OPTIONS`. Child visibility is per-side but values are mirrored, so both sides see the same hidden/shown state. | Accepted (assumed, confirmed) |
| D12 | Label textures unchanged | Row ids and display names are unchanged; only hierarchy/visibility moves. | No `gen_option_labels.py` regeneration. Descriptions in `option_strings.py` may get a wording touch (START/END "…within the loop") — optional. | Accepted (assumed, confirmed) |
| D13 | Validation | Engine-facing gates have no harness; the pure gate math does. | Host tests for the pure gate fn(s) + veil/marker visibility helper; cabinet checklist: press 6 during READY (LOOP OFF and ON) ⇒ no marker, no soft-lock; LOOP OFF gameplay press 4/6 ⇒ toast once, no marker, HUD shows cursor only; LOOP ON ⇒ v1 behavior; retained START/END with LOOP OFF ⇒ song plays whole from 0. | Accepted (assumed, confirmed) |

## Decision details

### D1 — READY-window gate predicate
`song_reset::first_anchored_frame()` (`src/services/song_reset/mod.rs`) is already a
state predicate: live DPS at in-song step 7, ≥1 GamePlayActor, all actors at
in-song step with a nonzero `+0x160` anchor. The driver adds a one-frame
credibility guard because `+0x178` is a per-frame cached value that can hold
the stale pre-anchor tick on the first anchored frame. Hoisting both into one
named predicate gives the gestures and the driver one definition of "the run
is live", and gives the name honest semantics (`first_anchored_frame` reads
like an edge).

### D3/D6 — latch, not row
`LOOP_LATCHED` is set in `try_resolve_row_bounds` (once per song) and cleared
by `on_loop_disarmed` when the driver's refusal ladder gives up (degenerate
section / thresholds unreadable) — in which case stock thresholds are
restored and the song plays whole. Gating gestures and HUD decorations on the
same latch keeps every surface consistent with what the driver will actually
do. Note the latch is only set once the resolution runs (a few frames after
GAMEPLAY entry, when the actor tree is up); the READY gate (D1) already covers
that window, so no gesture can observe the pre-latch state.

### D4 — retained-but-ignored
Three readers of START/END exist outside the resolution: the `rows_engaged`
arm predicate at GAMEPLAY entry, the resolution itself, and the bind-time
pre-shift (`refresh_pre_shift` → `song_rate::runtime::set_initial_content_mapping_ms`).
The pre-shift is the dangerous one: it fires at scene 25/26 boundaries from
the START row alone and would silently start a LOOP-OFF song mid-way. The
governing side for the loop check is the same `pre_shift_side()` the
pre-shift already uses.

### D9 — refused-press feedback
`services::toast` is the shared bottom-center text toast (`flash` /
`flash_with_hold` / `show_pulsing`). A once-per-song latch (cleared in
`clear_session_state`) keeps the hint from spamming.

## Readiness

Register accepted wholesale by the maintainer 2026-09-04 (D1–D13; no overrides, no Open items). No research step beyond `research/orientation.md` was needed.

Readiness Confirmed 2026-09-04
