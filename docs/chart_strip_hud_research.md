# Chart-Strip Timeline HUD — feasibility research (Training Mode Step 6 amendment)

Status: feasibility GO (2026-08-14). Addresses file-relative to
`gamemdx.dll` base `0x180000000`, build **20260616** (the
custom-arrow-renderer research's build; cross-check on 20260721 needed
during implementation — the arrow fill family shifted ~0xC0 between
builds, e.g. the tap palette-column selector `FUN_180028130` @ 616 sits
inside `FUN_180028070`'s neighborhood on 20260721).

## 1. Goal

Replace the planned horizontal progress bar (design R7) with a vertical
chart-strip timeline (left/right screen edge, per the maintainer's iOS
reference app): the strip renders the ACTUAL chart — noteskin-accurate
arrow glyphs at their content-time positions — pre-rendered ONCE per song
into a single texture shown by ONE static ImageWidget, plus ≤4 dynamic
marker widgets (current-time cursor, A/B markers, loop window).
Maintainer constraints: (a) performance — per-frame cost must be constant
w.r.t. chart density; (b) noteskin fidelity — the strip must show the
player's chosen arrow design AND the game's real quantization coloring,
sourced from live game state so future quantization-granularity hacks
propagate for free.

## 2. Rendering pipeline (all shipped machinery)

1. **Chart data**: `song_reset::decoded_notes(side)` — per note:
   `music_count` (raw ms → vertical position), `beat_count` (+0x04 →
   quantization color), `kind` (+0x00 discriminator), per-panel
   `state[8]` (+0x1C, columns) and `length[8]` (+0x3C, freeze bodies).
2. **Rasterize** on a background thread per song (assist_tick synthesis
   model, generation-tokened): stamp downscaled glyph cells into an RGBA
   strip (~64×640). Cost is synthesis-time only.
3. **Encode**: `image` crate (already a dependency — `preview_gen.rs`
   precedent) → PNG under `data_mods/_cache/training_hud/`.
4. **Load**: `file_manager_load(<disk path>.png)` → engine PngFileCallback
   registers the GPU texture under the filename stem →
   `get_texture_data(get_texture_hash_value(stem))` → handle →
   `ImageWidget::set_texture_id`. Exactly the mine-texture pipeline
   (`note_types_expansion/texture_loader.rs`); lazy per-frame poll while
   the async load completes; refcounted release on song exit
   (`asset_loader` precedent).
5. **Dynamic overlays**: cursor y from
   `song_reset::current_raw_music_count()` / `chart_end_raw` fractions;
   A/B from the bounds accessors. Content-domain axis ⇒ rate-independent;
   seeks/loops/FF-RW scrobbles are pure cursor moves.

## 3. Noteskin glyph sourcing

- **Sheets**: `data/arc/2d/2d_arrow00.arc` … `2d_arrow07.arc` — one per
  arrow design. Each: ARC v1 (`core/arc.rs::parse/extract` +
  `avslz::decompress`) containing `data/2d/arrowNN/arrowNN.dds` —
  **uncompressed A8R8G8B8, 768×192** (no DXT; header masks
  R=0x00ff0000 G=0x0000ff00 B=0x000000ff A=0xff000000; 589,824 byte
  payload after the 128-byte DDS header). Direct pixel access; ~20 lines
  of header skip. Host-inspectable via `scripts/unpack_arc.py` (and
  ifstools for IFS content).
- **Cell layout** (from `docs/custom_arrow_renderer_research.md` §2.3 +
  visual confirmation of the extracted sheet): 96×96 cells — tap cell
  `[0..96]×[0..96]`, freeze head `[96..192]×[0..96]`, freeze bottom-cap
  art rows `[96..192]`, freeze body columns at `x = col·96 + 384`
  (col = `{0,0,1,2,2,1,3,3}[dir·2+reverse] & 3`), body tiles vertically.
  Only ONE direction is baked (rotation is per-sprite at render:
  `set_direction`); the strip rasterizer rotates the cell per column.
- **Player's chosen design**: `Option[+0x60]` = arrow_shape 0..7 — the
  exact chain `note_types_expansion` already caches per song
  (`CACHED_ARROW_SHAPE`, primed from the pre-judge callback's
  GamePlayActor). Reuse it (or the shared player-option-table derivation).

## 4. Color: the palette-indexed pipeline (drive the game, copy nothing)

The sheets are PALETTE-INDEXED (playfield_styling research §7 /
shader_replacement research §5): atlas RED channel = palette U index;
palette V (row) = per-sprite vertex color R byte; the arrow PS composes
`color = palette[atlas.red][row]`.

- **Row selector** (quantization → palette row): `FUN_180028130(renderer,
  note_beat)` @ 616, called from the tap fill `FUN_1800278a0` — fully
  decoded:
  - color-option field `renderer+0xE8` ∈ {0,5} ⇒ beat-DIVISION mode:
    tick = `(beat & 0x3FF)`, `q = (tick>>6)*3 + ((tick&0x3F)*3+0x20>>6)`;
    `q % 0x30 == 0` ⇒ row 1 (4th), `q % 0x18 == 0` ⇒ row 3 (8th),
    `q % 0x0C == 0` ⇒ row 2 (16th), else row 4.
  - otherwise ⇒ beat-CYCLING mode: `row = (((beat & 0x3FF) + 0xDC) >> 8
    & 3) + 1`.
  For future-proofing, resolve and CALL this via signature with the live
  renderer rather than replicating (the maintainer plans a
  quantization-granularity hack here).
- **Palette generators**: class family `screen::ArrowPalette{Note,Rainbow,
  Vivid}{4,8,16,Other}` + `Spot/Judge/Hidden/Freeze` (RTTI @
  `0x18047db38..`). Factory `FUN_180025100(paletteMgr, colorOption)`
  builds a row→generator table at `mgr+0x28` (slot·8: row1=…Note4@+0x08,
  row2=…Note16@+0x10, row3=…Note8@+0x18 — matches the selector's rows) and
  creates the composed **256×32 dynamic texture** via
  `FUN_1802488e0(0x100, 0x20, 1, 0x15, 0x2002)` (handle at `mgr+0x20`);
  family keyed by option (0=NOTE, 1=RAINBOW, 3/else=VIVID variants).
  `ArrowPaletteFreeze` reads `hold16.bin`. Note: composed palette is
  256×**32** (older doc said 256×16).
- **Per-note color evaluator**: generator vtable slot 1 (`+0x08`):
  `u32 evaluate(this, int rowArg, int column, int beatPhase)` (e.g.
  Note16 = `FUN_18002c100` via vftable @ `0x18035bd40`).
- **Per-frame palette update — RESOLVED (2026-08-14)**: the manager class
  `screen::ArrowPalette` (vftable @ `0x18035baa8`; **owned by the
  GamePlayActor at `+0x130`**, created in the actor's step-0 init
  `FUN_18005cca0` which also names the sheets `"arrow%02d"` from the
  option read) updates via vtable slot 4 (`+0x20` →
  `FUN_180025670`): LOCK the 256×32 texture (`FUN_180248eb0(handle@
  mgr+0x20, …) → mapped ptr + pitch`), then for each row 0..31 × column
  0..255 call `generator->evaluate(rowArg, column, phase)` and store the
  u32, then UNLOCK (`FUN_1802492e0`). Row→generator: table at
  `mgr+0x28..+0x30` (8-byte ptrs); rows 8..15 fold to slot 7
  (`ArrowPaletteFreeze`, `rowArg = row − 7`); rows past the table end
  fold to the last slot. Phase = `mgr+0x18` (beat input, written per
  frame by the actor alongside `mgr+0x1C` = time); row 0 gets phase −1
  unless the enable byte `mgr+0x48` is set. Registered on the global
  per-frame updatee list via `FUN_180217810(*(DAT_1806f1d20+0xC8), mgr)`.
  Texture rows: 0 = spot/receptor, **1–4 = tap note colors** (the
  selector's rows), 5 = judge, 6 = hidden, **7–15 = freeze** (head/body
  rows — the fill encodes freeze rows as 8..15 / 0xE), 16+ = Other.
- **Strip color strategy — DECIDED: (b), fully specified.** There is NO
  persistent CPU palette buffer (the update writes into the locked GPU
  texture), so the strip synthesis walks the LIVE manager
  (`GamePlayActor+0x130` → table @ `+0x28`) and calls the game's own
  `evaluate(rowArg, col, phase)` per needed row (1..4 + freeze rows) at
  the synthesis-time phase, building a private CPU palette — literally
  the game's update loop pointed at our buffer. Game-thread-only calls
  (the generators may read game state); a future quantization hack that
  changes the selector/generators propagates automatically. Fallback
  ladder unchanged (missing manager/table ⇒ flat quantization colors ⇒
  no strip).

## 5. Risks / probes

1. **Texture stem refresh across songs** — per-song stems (e.g.
   `training_strip_<gen>`) vs re-registering one stem; needs one cabinet
   probe of ResourceManager release/reload behavior. Mitigation: per-song
   stem + paired release (engine refcounts; mine loader shows lazy
   availability polling).
2. **PngFileCallback tolerance** of `image`-crate PNG output — near-zero
   risk (stock encoder output; the pipeline already consumes arbitrary
   mod-authored PNGs).
3. **Build drift** — all new addresses must resolve by AOB on 20260721
   (arrow fill family shifted between 0616/0721; the selector's imm
   pattern `0x3FF/0xDC/0x30/0x18/0xC` is highly signature-friendly).
4. **Legibility** at ~5 px/s — synthesis-time tuning (glyph size,
   column width, optional multi-lap wrap like the reference app).

## 6. Scope decisions (maintainer, 2026-08-14)

- Step 6 = the strip timeline (vertical, LEFT/RIGHT placement row,
  default RIGHT, `PersistMode::Full`). FF/RW scrobbling = its own step
  right after (pinpad 7=RW / 9=FF, single-press per increment,
  `training_mode.{ff,rw}_increment_ms` config, default 5000 ms — design
  R12's reserved keys).
- Noteskin-accurate glyph art from day one (no dots/bars v1).
- PUS remains out of score-suppression scope (verified: zero score_guard
  calls).
