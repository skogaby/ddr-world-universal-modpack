# Task: Emitter promotion — production overlay_draw + menu hookup, POC gate removed

## Description
Promote `src/services/overlay_draw/` from the env-gated POC to the
production animated-background emitter, and hook the menu side up:
`Background::Shader { program }` in theme.rs (arrows/bubbles/wavefield
flip; MINIMAL stays Static), the ANIMATED BACKGROUND row's availability
gate goes live, an atomic open-state/appearance feed for the emitter,
and the `DDR_OVERLAY_DRAW_POC` env var is removed.

## Background
Step 8 of the overlay-menu rewrite (design §4.7, §6). Approved decisions
(2026-08-25): **layer-identity frame gate** — emit only when the wrapper
being rendered is the DLL's own widget-host wrapper (guarantees the quad
lands in the same command list as our widgets, immediately beneath them;
fallback if the identity check proves unreliable on the cabinet:
first-list-of-frame, documented); **time wrapped modulo 3600 s**
(monotonic seconds since init as f32); **MINIMAL greys the row even with
shaders available** (nothing to animate); soak rides normal play.

Current facts (verified 2026-08-25):
- `overlay_draw/encode.rs` (pure, 12 host tests via
  `scripts/validate_overlay_draw.sh`, `MODULES=(encode.rs)`): the full
  record set incl. `set_vs_const_f(reg_off, regs)` — reg_off is relative
  to c48, so `set_vs_const_f(0, &[c48, c49])` writes both registers in
  one record with an ABSOLUTE payload pointer (encode against the
  reserved arena base). `RecordWriter::new(base)`.
- `overlay_draw/mod.rs` (POC): `on_wrapper_render()` called from
  `widget_renderer::wrapper_render_hook` PRE-original
  (widget_renderer.rs:181); called once per VISIBLE WRAPPER per frame
  (~5 lists/frame observed — the production emitter must pick one).
  Gate ladder (keep ALL of it): null list / null arena ptrs / bump
  invariant `write == base+size` / per-list frame gate / 8 MiB arena
  soft cap / shader null / progs plausibility (1..=64). POC emission:
  context(1280,720) → scissor-on → SetShader(default, 0) → quad →
  SetShader(default, 0) → scissor-off, then copy + bump
  (`*(cl+0x0C) += len`, `*(cl+0x10) = base + new_size`). POC rect
  (200,100,880,520) and `POC_ENABLED` env latch both go away.
  `DEFAULT_SHADER_GLOBAL` resolved in `init` from the `default_shader`
  derived signature. Heartbeat INFO every 600 emissions.
- Layer identity: `widget_renderer` owns the widget-host wrapper —
  find how the hook identifies wrappers (the `this` pointer of
  `wrapper_render_hook`) and whether widget_renderer records ITS
  wrapper's pointer (the one hosting the DLL's widgets). Emit only when
  `this == our wrapper`. If widget_renderer doesn't currently record
  it, add that (it creates/knows its host during widget setup).
- Program indices: task-02's `theme_program_indices()` export —
  `None` ⇒ Static degrade (no emission).
- Menu side:
  - `theme.rs`: `Background::Static` (doc promises the Step 8 variant);
    THEMES all Static; guard test `backgrounds_all_static_for_now` must
    be REPLACED (arrows/bubbles/wavefield flip to
    `Shader { program: ThemeProgram::{Arrows,Bubbles,Wavefield} }` —
    suggest a small `ThemeProgram` enum rather than raw indices so
    theme.rs stays decoupled from synthesis's numbering; the emitter
    maps enum → exported index).
  - `tabs.rs:81–89`: `animate_greyed` hardcoded `false` — becomes
    "no shader path available for the ACTIVE theme": true when the
    active theme's background is Static (MINIMAL) OR
    `theme_program_indices()` is None.
  - `chrome_loader::animate_background()` / `active_theme_index()`
    already exist (Step 7).
  - `mod.rs` `ModMenuState.is_open` lives behind a mutex — add an
    ATOMIC mirror (set in `open()`/`close()`) the emitter reads
    per-frame (hot-path rule: no mutex in wrapper_render).
- Modal rect: render.rs `MODAL_X/Y/W/H = 60/60/1160/600` — the
  emitter's scissor + c48/c49 rect (expose the four constants or move
  them somewhere both can reach; keep one source of truth).
- Design §6 ladder rows: emission gate fails ⇒ static panel (already
  the base layer) + emitter latches off for the session on REPEATED
  failure; shader-fixes disabled ⇒ same degrade with the row greyed.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-24-overlay-menu-rewrite/design/detailed-design.md (§4.7 emission, §5 constant block, §6 error ladder)

**Additional References (if relevant to this task):**
- docs/overlay_draw_research.md (production recipe §"Production recipe", gate ladder, multi-list finding)
- .agents/tasks/2026-08-24-overlay-menu-rewrite/step08/task-02-theme-synthesis.code-task.md (index export contract)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. **Emitter activation state** (overlay_draw): replace the POC env
   latch with a production state feed — suggest
   `overlay_draw::set_background(Option<BackgroundParams>)` where
   `BackgroundParams { program_index: u32, rect: (u16,u16,u16,u16),
   theme_params: [f32; 2] }`, stored in atomics/a seqlock-lite cell,
   called by mod_menu on open/close/theme-change (render thread).
   `None` ⇒ inactive (single relaxed load fast path, unchanged from
   the POC's disabled cost).
2. **Frame gate**: layer-identity — `widget_renderer` records its
   widget-host wrapper pointer; `wrapper_render_hook` passes `this`
   into `on_wrapper_render(this)`; emit only when it matches (plus the
   existing per-list re-emit guard). Keep the code path for
   first-list-of-frame behind a comment as the documented fallback.
3. **Emission sequence** (per design §4.7): context(1280,720) →
   scissor-on(modal rect) → `set_vs_const_f(0, [[time, rx, ry, 0],
   [rw, rh, p0, p1]])` → SetShader(default, theme_program_index) behind
   the mandatory `progs >= idx+1` gate → one untextured quad covering
   the modal rect (near-opaque dark base color — the PS composes over
   it) → SetShader(default, 0) restore → scissor-off. Time = monotonic
   seconds since emitter init, `% 3600.0`, f32.
4. **Failure latch**: repeated gate failures (suggest ≥ 60 consecutive)
   latch the emitter off for the session with one WARN (design §6);
   individual failures stay fail-open + latched-WARN per class as
   today.
5. **POC removal**: `DDR_OVERLAY_DRAW_POC` env handling, `POC_RECT`,
   `POC_COLOR`, and the POC emission path deleted; diagnostics
   (scene/active-list diag) may stay if still useful, else delete.
   Update the module doc.
6. **theme.rs**: `ThemeProgram { Arrows, Bubbles, Wavefield }`;
   `Background::Shader { program: ThemeProgram }`; arrows/bubbles/
   wavefield flip; MINIMAL stays Static. Update the guard test to
   assert the new mapping (and MINIMAL Static).
7. **mod_menu hookup**:
   - Atomic `MENU_OPEN` mirror set in `open()`/`close()`.
   - A small `update_background_feed()` (mod.rs or render.rs) called on
     open, close, and theme/animate change: computes
     `Option<BackgroundParams>` from (open ∧ animate ∧ active theme's
     Background::Shader ∧ `theme_program_indices()`) and calls
     `overlay_draw::set_background`.
   - `tabs.rs` `animate_greyed` = active theme Static ∨ indices None.
   - The ANIMATED BACKGROUND toggle now visibly starts/stops the
     animation (snap back to the static gradient panel).
8. **Tests**: encode.rs gains a production-sequence walk test (context/
   scissor/consts/setshader/quad/restore with the real rect + c48/c49
   payload check); model/theme harness tests updated per req 6; the
   pure availability-gate decision (greyed calculation) belongs in
   model.rs or theme.rs with a test if any branching beyond a boolean
   OR emerges.
9. **Cabinet validation** (autonomous where possible): boot with
   themes on → open menu on RHYTHM/BUBBLES/WAVEFIELD (animated backdrop
   behind the panel — screenshots for the maintainer), MINIMAL (row
   greyed, static); toggle ANIMATED BACKGROUND off/on; scene churn
   attract→select with the menu opened in each; boot with shader-fixes
   disabled → row greyed, one WARN class max, stock lane visuals
   unaffected; perspective regression (player-perspective enabled ⇒
   still program 1 — a gameplay check rides the maintainer's session).
   0 panics, no gate-WARN storms.

## Dependencies
- task-02 (index export + synthesized default container).
- task-01 blobs deployed to the cabinet's data_mods.

## Implementation Approach
1. encode.rs production-sequence test (red) → emitter rework (state
   feed, layer identity, constants, latch, POC removal).
2. theme.rs/model/tabs updates + harness runs.
3. mod_menu feed wiring.
4. Gates: all three validate scripts → `cargo check` → `cargo fmt` →
   `./build.sh`.
5. Deploy + autonomous cabinet legs (req 9) + screenshot archive;
   maintainer demo = the step's visual sign-off.

## Acceptance Criteria

1. **Animated backdrop in the sandwich**
   - Given the menu open on RHYTHM with ANIMATED BACKGROUND on
   - When observing the modal
   - Then the animated arrow field renders clipped to the modal rect,
     above game content, beneath the panel/text widgets, and stops the
     instant the menu closes.

2. **Availability gating**
   - Given MINIMAL active, or shader-fixes disabled, or synthesis
     degraded
   - When the APPEARANCE tab renders
   - Then the ANIMATED BACKGROUND row is greyed and no emission occurs
     (static gradient only), with at most one WARN naming the degrade.

3. **Toggle + theme switch**
   - Given the animation running
   - When ANIMATED BACKGROUND toggles off / the THEME row changes
   - Then the backdrop snaps to static / switches programs on the next
     frame without a stale-program frame (indices re-resolved through
     the feed).

4. **Safety invariants**
   - Given any scene (boot/attract/select/gameplay incl. versus)
   - When the menu opens and closes repeatedly
   - Then the bump invariant, program-count gate, and state restore
     hold (log-verified: zero unexpected WARNs across the churn run),
     and a synthetic failure (e.g. forced null list) latches the
     emitter off after the threshold with one WARN.

5. **POC gate gone**
   - Given the final tree
   - When grepping for `DDR_OVERLAY_DRAW_POC`
   - Then no hits remain (docs may reference it historically).

## Metadata
- **Complexity**: High
- **Labels**: overlay-draw, emitter, mod-menu, theme, command-list
- **Required Skills**: Rust, repo hook-DLL conventions (hot-path rules, panic-free hooks), command-list RE facts
- **Generated By**: code-task-generator 2026-08-25
- **Source Plan**: .agents/planning/2026-08-24-overlay-menu-rewrite/implementation/plan.md
- **Plan Step**: Step 8: Animated shader backgrounds
