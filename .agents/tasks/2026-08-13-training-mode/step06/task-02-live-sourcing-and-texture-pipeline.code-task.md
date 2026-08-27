# Task: Live sourcing + texture pipeline (sheet, palette, synthesis, load)

## Description
The engine-facing half of the Step-6 strip (design R7 as amended): per
song, resolve the player's chosen arrow design, read the chart, build the
palette by calling the game's OWN palette generators, run task-01's
synthesis on a background thread, write the PNG to the cache, load it
through the FileManager pipeline, and display it as ONE static
ImageWidget. Includes the texture-stem-refresh cabinet probe (research
§5 risk 1).

## Background
Everything here drives shipped machinery or freshly-RE'd surfaces
(docs/chart_strip_hud_research.md — read §2–§5 in full):

- **Chosen design**: `Option[+0x60]` = arrow_shape 0..7 (the exact chain
  `note_types_expansion` primes into `CACHED_ARROW_SHAPE` from its
  pre-judge callback — reuse or replicate that pattern). Sheet file:
  `data/arc/2d/2d_arrow0N.arc` read from disk (std::fs — plain files;
  layered mods may override the path via data_mods, resolve like the
  webui asset scan does if applicable).
- **Palette (RESOLVED RE, research §4)**: the GamePlayActor owns a
  `screen::ArrowPalette` manager at **`+0x130`**; row→generator table at
  `mgr+0x28` (8-byte ptrs; rows 8..15 fold to slot 7 = Freeze with
  `rowArg = row − 7`); per-row color = generator vtable slot 1
  (`+0x08`): `u32 evaluate(this, rowArg, column 0..255, phase)` with
  phase = `mgr+0x18`. Build the strip's private palette by calling the
  evaluators for the needed rows (1..4 note rows + freeze rows) at the
  synthesis-time phase — GAME THREAD ONLY (generators may read game
  state), via `run_on_render_thread`, copying into a plain buffer the
  background synthesis consumes. The tap note's row comes from the
  game's row selector (`FUN_180028130` @ 20260616 — decoded in research
  §4; resolve BY SIGNATURE and CALL it with the live ArrowRenderer so a
  future quantization hack propagates; its imm pattern
  0x3FF/0xDC/0x30/0x18/0xC is signature-friendly). Freeze row encoding
  mirrors the fill (research §4: head/body rows 8..15, 0xF ⇒ row 8).
- **New AOBs (20260721 cabinet build + validated on 0616/0324 where
  present)**: the row selector; the palette-manager offset anchor
  (GamePlayActor `+0x130` — derive from the actor init `FUN_18005cca0`'s
  store or validate the RTTI vtable `screen::ArrowPalette` @ the read
  pointer, fail-closed); the ArrowRenderer instance (the actor stores it
  at `+0x138` per the same init — validate by RTTI). All optional:
  resolution failure degrades per the fail-open ladder.
- **Synthesis lifecycle**: assist_tick's model — armed at GAMEPLAY entry,
  built at/after the first judge dispatch (notes + manager + renderer all
  live by then), generation-tokened against restarts, background thread
  for the rasterize/encode, engine reads snapshotted on the game thread
  first (notes via `song_reset::decoded_notes`, palette via the evaluator
  walk, design via the cached shape).
- **Texture pipeline** (mine-loader precedent,
  `note_types_expansion/texture_loader.rs` + `services/asset_loader.rs`):
  write PNG to `./data_mods/_cache/training_hud/strip_<generation>.png`,
  `file_manager_load(path)` → poll
  `get_texture_data(get_texture_hash_value(stem))` lazily per frame →
  `ImageWidget::set_texture_id(handle)` → show. Release + delete on song
  exit/new song. **Probe first** (risk 1): per-song stems vs one reused
  stem — validate on cabinet that a second song's strip actually
  refreshes; per-song stems with paired release is the expected answer.
- **Widget**: one ImageWidget, vertical strip at the screen edge
  (position/side wiring finalized in task-03 — default RIGHT constant
  here), created hidden, shown when the texture resolves AND
  `bounds::training_session_active()`.

Visibility for Step 6 = training sessions only (the design's session
predicate) — the strip is part of the training HUD.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (R7 as amended 2026-08-14; §6 error handling)

**Additional References (if relevant to this task):**
- docs/chart_strip_hud_research.md (the whole doc — §4's palette recipe and §5's probes are this task's spec)
- src/mods/note_types_expansion/texture_loader.rs (FileManager pipeline + lazy poll)
- src/services/asset_loader.rs (release-capable load precedent, threading rules)
- src/mods/assist_tick.rs (per-song background synthesis lifecycle, generation tokens)
- src/services/song_reset/mod.rs (`decoded_notes`, `chart_end_raw`)
- src/core/signatures.rs (AOB + derivation house style, `required_signatures` vs optional)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. Signature work in `src/core/signatures.rs`: the row selector AOB + the
   GamePlayActor palette-manager/renderer offset derivations (RTTI
   vtable-validated, fail-closed to None). Optional (not in
   `required_signatures`).
2. Palette snapshot: game-thread evaluator walk building the private
   palette rows needed by task-01's rasterizer; row selector called per
   note (or per distinct beat_count — cache by beat value) on the game
   thread during the snapshot, producing per-note palette rows the
   background rasterizer consumes.
3. Sheet load: disk read + task-01 extraction, cached per (design, boot)
   — re-read only when the design changes.
4. Synthesis driver: gameplay-entry arm → first-dispatch snapshot →
   background rasterize/encode → PNG write → file_manager_load → lazy
   handle poll → widget show. Generation-tokened; a restart/new song
   supersedes cleanly; release + cache-file cleanup on exit.
5. Fail-open ladder (one WARN per song, design §6): missing
   selector/manager/renderer ⇒ flat quantization colors (research §4
   fallback); sheet unreadable ⇒ no strip (task-03's markers still run);
   synthesis/load failure ⇒ no strip. Session never blocked.
6. No new host tests beyond task-01's (engine-facing); the pre-existing
   suite stays green. Gates + the stem-refresh cabinet probe (a probe
   build with two-song verification is acceptable as the task's deploy).

## Dependencies
- task-01 (extraction + rasterizer + layout + PNG).
- Steps 1–5 shipped (session predicate, decoded_notes, widget_renderer).

## Implementation Approach
1. Signatures + derivations (validate on 20260721; cross-check 0616).
2. Palette/selector snapshot plumbing (game thread) + sheet cache.
3. Synthesis driver + texture pipeline + widget; probe stem refresh.
4. Gates; cabinet check of the strip on 2+ consecutive songs.

## Acceptance Criteria

1. **Strip appears with real chart + noteskin**
   - Given a training-active song (any bound/loop/gesture state) with
     arrow design N
   - When gameplay starts
   - Then the strip texture shows design N's glyphs at the chart's
     positions with the game's live quantization colors (visual check
     against the lane)
2. **Per-song refresh**
   - Given two different songs played back-to-back in one session
   - When each song's gameplay starts
   - Then each shows ITS chart (no stale texture — the risk-1 probe)
3. **Design/palette future-proofing**
   - Given the row selector and generators are resolved
   - When colors are produced
   - Then they come from calling the game's own functions (verified in
     code review; no replicated color math on the live path)
4. **Fail-open**
   - Given a forced resolution failure (e.g. env-gated skip of the
     selector)
   - When a training song plays
   - Then one WARN, flat-color or absent strip per the ladder, and the
     song plays/judges/scores normally
5. **Performance**
   - Given a dense chart (10+ footer)
   - When the strip is visible during play
   - Then no observable frame cost vs strip-off (the per-frame path is
     one static widget; synthesis is off-thread)

## Metadata
- **Complexity**: High
- **Labels**: training-mode, hud, engine-facing, signatures, cabinet-probe
- **Required Skills**: Rust, Ghidra-informed RE, the repo's signature/derivation and texture-pipeline conventions
- **Generated By**: code-task-generator 2026-08-14
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 6: Chart-strip timeline HUD + placement row
