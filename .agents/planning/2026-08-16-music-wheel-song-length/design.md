# Music Wheel Song Length — Design

Date: 2026-08-16. Decisions settled with the maintainer; mechanism facts in
`research.md`.

## Summary

New top-level mod `music-wheel-song-length` (`src/mods/music_wheel_song_length.rs`):
renders `LENGTH M:SS` in the song-select header card, styled identically to
the stock BPM readout, using the game's own `sequence::SpriteLayer` widget
class for rendering + lifecycle. No new detours; no binary patches.

## Decisions (maintainer-settled)

| # | Decision | Choice |
|---|---|---|
| D1 | Rendering | Native AFP/BM2D art match via `sequence::SpriteLayer` — NOT the DLL's TextWidget/ImageWidget (maintainer explicitly wants the game's native label lifecycle + digit rendering) |
| D2 | Staleness | Blank the instant the wheel moves off a song; fill in when a fresh `selected_song` publication arrives whose digest matches the current selection (poll model + digest match) |
| D3 | "LENGTH" caption | Borrowed from the friend's pack: baked into the card background texture `muca_card_current_bg2.png` (plain LayeredFS texture replace, permission granted) |
| D4 | Colon glyph | NET-NEW texture (`muca_card_len_c`, ≤15 chars for SSO) injected via `atlas_cloner` with donor `muca_card_bpm_question`; art borrowed from the friend's colon. Stock `?` glyph untouched (his build sacrificed it; ???-BPM songs would show `:::`) |
| D5 | Digits | Stock `muca_card_bpm_{0..9}` textures by name — zero art duplication, pixel-identical |
| D6 | Config | `mods` map toggle only; no in-game option row. Dev-time offset knobs allowed in a `music_wheel_song_length` config section if calibration needs iteration |
| D7 | Format | `M:SS` (no zero-padded minutes; matches the friend's `1:34`), minutes can run ≥10 naturally |

## Architecture

### Art (data_mods, shipped with the mod)

```
data_mods/music_wheel_song_length/
└── data/bm2d/select_music_card_v3_ifs/tex/
    ├── muca_card_current_bg2.png        # friend's bg with LENGTH caption (replace)
    └── texturelist.merged.xml + cloned atlas  # emitted by atlas_cloner at enable:
        muca_card_len_c                  # colon, donor muca_card_bpm_question
```

- The bg replace flows through the standard `ifs_textures` path.
- The colon uses `atlas_cloner::generate_cloned_atlases` at mod enable
  (same call order documented in atlas_cloner's header), donor
  `muca_card_bpm_question` — same 24×24 cell family as the digits, UV math
  preserved.
- Source PNGs live in the repo under the mod's asset dir and are deployed by
  `scripts/deploy.sh` like other data_mods content.

### Runtime (the mod)

One mod-owned `sequence::SpriteLayer` instance (created lazily, lives
forever, blank = empty names list):

- Allocated by the DLL (`memory::alloc_zeroed(0xF8)`) and initialized with
  the game's ctor (`spritelayer_ctor` signature). Never destroyed.
- Fields: parent = the game's `music_info` CMovieClip wrapper (found by
  scanning the BM2D CMovieClip pool for the clip named `music_info`);
  anchor name `"bpm_usr"` (SSO, in-place); priority 4; spacing −10.0;
  fixed scale 1.0; **x/y offset (+0xC0/+0xC8) positioning the digits after
  the baked LENGTH caption** (calibrated on cabinet).
- Per-frame (via `input_manager::on_frame`, scene-25-gated, panic-contained):
  1. Re-resolve the `music_info` wrapper; if changed/absent → clear parent,
     blank. (The card layer is recreated per scene entry.)
  2. Read the highlighted-song shared_ptr at `selectmusic_model + 0x1B0`.
     Pointer changed → set empty names (blank immediately).
  3. If blank and a `selected_song()` publication exists whose
     `code_digest == song_code_digest(vt+0x08 basename)` of the current
     selection → build the names list
     `[len_digit…, muca_card_len_c, digit, digit]` from `audio_len_ms`
     (rounded to seconds) and call `spritelayer_set_names`.
  4. Call the layout vfunc (object vtable slot 0) — every frame while a
     parent is bound, mirroring the game's own card tick, so alpha fades /
     hide-on-anchor-missing behave natively.
- Names vector ABI: strings ≤15 chars (all are) stay SSO — no cross-heap
  string allocation; the vector backing array is allocated with
  `game_malloc` because the setter's move leaves ownership with the game
  object. Exact second-arg semantics of `FUN_1801d3070` verified during
  implementation (research §9).

### Signatures (all → `required_signatures`, mod absent on miss)

1. `spritelayer_ctor` — `FUN_1801d2e00` field-init body.
2. `spritelayer_set_names` — `FUN_1801d3070` prologue.
3. `selectmusic_model` — RIP-derived global `DAT_1806f2d50` (anchor inside
   the card tick or the preview-request function; validated like other
   derivations).

The layout call needs no signature (object's own vtable). Anchor resolution
happens inside layout. The BM2D pool for finding `music_info` reuses the
existing `bm2d_api` machinery.

### Failure model

Fail-open everywhere: missing signatures → mod reports unavailable and
never registers its frame callback. Missing textures → SpriteLayer setter
logs the engine's own "bitmap not found" once; blank display. Digest
mismatch / stale publication → stays blank (never shows a wrong length).
No panics in the frame callback (catch_unwind at the callback boundary,
consistent with scene_manager patterns).

## UX behaviors inherited natively

- Fade in/out with the card (glyphs read the anchor's alpha per layout).
- Hidden on folders / blank cards (anchor missing or our blank rule).
- Hidden outside scene 25 (clip destroyed → anchor resolve fails → glyphs
  hidden; plus our gate).
- Brief blank while the wheel scrolls; value appears when the preview bank
  loads (wheel settle) — the same cadence the preview audio follows.
