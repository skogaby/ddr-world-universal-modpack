# Music Wheel Song Length — Research

Date: 2026-08-16. Addresses are from `gamemdx_20260721.dll`, file-relative to
`0x180000000` unless noted. Cross-checked against the friend's hex-edited pack
(`~/Desktop/Latest Patch (rev. 8-11-26)`, targets an older binary).

## 1. Goal

Show the highlighted song's length as `LENGTH M:SS` in the song-select header
card (the white banner with jacket / title / artist / BPM), styled identically
to the stock BPM readout. Version-agnostic (no binary patching, no hardcoded
offsets), riding the game's own rendering + lifecycle machinery.

## 2. How the header card renders (SELECT MUSIC scene 25)

The header is the **MusicCard** object (`sequence::selectmusic` namespace).
Key functions:

| Function | Role |
|---|---|
| `FUN_18015f030` | Card setup: creates the `music_info` CMovieClip layer (BM2D pool `DAT_1806fa600`, 0x400 slots × 0x48*8 bytes) and constructs 5 child widgets |
| `FUN_18015fc10` | Card updater — runs when the highlighted song changes; sets title/artist/source text, jacket bitmap, and the BPM digit string |
| `FUN_180160910` | **Per-frame tick** — the keystone (see §4) |

Child widgets on the card (offsets in the MusicCard object):

| Offset | Widget | Anchor MC (child of `music_info` clip) |
|---|---|---|
| +0xC8 | TextLayer (title) | `music_name_usr` |
| +0xD8 | TextLayer (artist) | `artist_name_usr` |
| +0xE8 | TextLayer (source) | `source_usr` |
| +0xF8 | **SpriteLayer (BPM digits)** | `bpm_usr` |
| +0x108 | SpriteLayer (gimmick icons) | `gimmick_top_usr` |

- Title/artist = real text via the **TextLayer** pipeline (the DLL already has
  `textlayer_ctor/bind/set_text` signatures from custom_options).
- The green "BPM" caption = static texture `muca_card_bpm_title` (48×24) baked
  into the card art. The digits = **SpriteLayer** (see §3).
- BPM digit formatting (in `FUN_18015fc10`): formats `"%d ~ %d"` or `"%d"`,
  then `FUN_1800f8d60`/`FUN_1800e15f0` (sprintf-into-std::string helpers),
  then `FUN_1801d2d40(&names_vec, str, "muca_card_bpm_%s")` maps each CHAR to
  a texture name, then `FUN_1801d3070(spritelayer, &names_vec)` applies.
  Glyph textures in `select_music_card` IFS: `muca_card_bpm_{0..9}` (24×24),
  `_tilde` (24×24), `_question`, `_blank` (20×24), `_title`. **No colon.**

## 3. `sequence::SpriteLayer` — the native bitmap-string widget

RTTI `.?AVSpriteLayer@sequence@@` @ 0x1804bf5c0. A general-purpose "row of
bitmaps by texture name" widget used all over the game (results, gameplay,
song select — 40+ call sites of the setter).

- **ctor** `FUN_1801d2e00` — pure field init on a 0xF8-byte struct (no
  allocation, no registration). vftable @ `0x180387088`:
  slot 0 = layout `FUN_1801d33e0`, slot 1 = scalar dtor.
- **shared_ptr factory** `FUN_180038f50` — `_Ref_count_obj<SpriteLayer>` via
  `FUN_180279714(0x108)` (game CRT `operator new`). The game wraps instances
  in shared_ptrs; a mod-owned instance doesn't need to.
- **setter** `FUN_1801d3070(this, names_vec)` — moves the names vector in
  (`FUN_1801d3b20(this+0x28, …)`), releases current bitmaps
  (`FUN_1801d2fa0`: vfunc+0x18 on each pool object), then per name: allocates
  a **CBitmap** from the global pool `DAT_18078a600` (0x1000 slots ×
  0x47*8 bytes) by texture name (`FUN_1802733b0`), sets priority (+0x94) and
  attribute, queries size, pushes `{obj, w, h}` (0x20 stride) into the
  bitmaps vector at +0x08. Ends with a virtual layout call.
- **layout** `FUN_1801d33e0` (vfunc slot 0):
  - Resolves the anchor: `FUN_18026f0e0(parent_clip /*+0x60*/, anchor_name
    /*SSO string @ +0x68*/)` — find-child-MC-wrapper-by-name.
  - **Shows/hides every bitmap based on anchor presence**
    (`vfunc+0x20(bitmap, anchor != NULL)`), i.e. the string vanishes
    automatically whenever the clip/anchor is gone.
  - Reads the anchor MC's live position (param 0x1008), scale (0x100D), size
    (0x1015/0x1016) and **alpha (0x100A)** — glyphs inherit the anchor's
    fade each layout call.
  - Lays glyphs sequentially with spacing `+0xE8` (double; bpm_usr uses
    -10.0, gimmick row -2.0), alignment ints +0x9C/+0xA0, fit-to-anchor
    (+0xD8=1) or fixed scale `+0xE0` (double; card uses +0xD8=0, scale 1.0),
    color floats +0xA4..+0xB0, alpha multiplier double +0xB8.
  - **X/Y pixel offsets added to the anchor position: doubles at
    +0xC0/+0xC8** — this is how a second instance can anchor to `bpm_usr`
    yet render to the right of the stock digits.

### SpriteLayer field map (from ctor disasm + call sites)

| Offset | Type | Meaning | Card's bpm_usr value |
|---|---|---|---|
| +0x00 | ptr | vftable (0x180387088) | |
| +0x08/10/18 | vec | bitmaps `{CBitmap*, w f64, h f64, pad}` ×0x20 | |
| +0x28/30/38 | vec | pending names (std::string ×0x28) | |
| +0x48/50/58 | f64×3 | last layout extents | |
| +0x60 | ptr | parent CMovieClip wrapper | music_info clip |
| +0x68..0x88 | SSO str | anchor child MC name | "bpm_usr" |
| +0x90 | u8 | skip param-init walk of anchor if set | 0 |
| +0x94 | i32 | bitmap priority | 4 |
| +0x98 | i32 | group? | 0x7FFFFFFF |
| +0x9C/+0xA0 | i32 | x/y alignment | 1/1 |
| +0xA4..B0 | f32×4 | color RGBA | white |
| +0xB8 | f64 | alpha multiplier | 1.0 |
| +0xC0/+0xC8 | f64 | **x/y pixel offset from anchor** | 0/0 |
| +0xD8 | u8 | 1 = fit-to-anchor scale, 0 = fixed | 0 |
| +0xE0 | f64 | fixed scale | 1.0 |
| +0xE8 | f64 | per-glyph spacing (px, negative = tighten) | -10.0 |
| +0xF0 | u8 | axis? (ctor 1) | 1 (unchanged) |

## 4. The card tick — selection signal + per-frame layout

`FUN_180160910` (MusicCard tick, every frame at scene 25):

1. Reads the **highlighted-song shared_ptr at `DAT_1806f2d50 + 0x1B0`**
   (obj) / `+0x1B8` (refcount) — the same global the preview player uses
   (training research §8.1). Compares against the card's cached ptr at
   +0x150; on change, refreshes the card via `FUN_18015fc10` (with fade).
   Folder/blank selections = null object.
2. **Calls vfunc slot 0 (layout) on all five child widgets every frame** —
   this is why glyph alpha/position/visibility track live. A mod-owned
   SpriteLayer must be layout-ticked the same way (our frame callback).

Song code from the music object: vt+0x08 = basename getter (same getter the
preview request `FUN_18010eab0` uses to build `data/sound/win/dance/<code>`).
`song_code_digest(code)` (already in `song_rate::binding`) lets us match the
selection against the `selected_song` publication digest exactly.

## 5. Song length source (already in the DLL)

`song_rate::selected_song::selected_song()` → `SelectedSongInfo
{ code_digest, audio_len_ms, generation }`. Published on EVERY slot-5
dance-bank create — including the preview player's load, which fires when the
wheel settles. Installed unconditionally at init (`lib.rs:192`), independent
of the song-playback-speed mod. Length = XWB MAIN entry duration (ms) —
audio length, an upper bound on chart content (`training_mode_research.md`
§8.3); for display purposes this matches what players expect.

## 6. The friend's hex-edit implementation (fully decoded 2026-08-16)

His pack (`Latest Patch rev. 8-11-26`, built on 20250805; analyzed as
`gamemdx_20250805_MODIFIED_2.dll`) renders `LENGTH 1:34` at the right edge
of the header card. Art (borrowed with permission): two changed textures —
`muca_card_current_bg2.png` (bg with baked "LENGTH" caption) and
`muca_card_bpm_question.png` (the `?` glyph replaced with colon art; his
???-BPM songs render `:::`). We keep the bg replace but inject the colon
as a NET-NEW name via `atlas_cloner`, leaving stock `?` intact.

Binary side — THREE code caves (region 0x1802db580..0x1802db6b0):

1. **Card-updater hook** (`CALL 0x1802db668` injected at 0x18014b08c,
   right before the BPM glyph-list build): appends `"M?SS"` to the BPM
   digit string (so the stock char→`muca_card_bpm_%s` mapper renders his
   colon via the `?` slot) and stashes the card's SpriteLayer pointer to a
   scratch global (0x1802b8fd8). Reads length as `*(music::Info + 0x14)`.
2. **SpriteLayer-layout hook**: when laying out the stashed BPM
   SpriteLayer, detects the last 4 glyphs (count−0x80 vector-bytes
   heuristic) and teleports them to hardcoded x≈878 (0x36E) — that's how
   `M:SS` sits at the card's right edge despite being appended to the BPM
   string.
3. **Length computation + cache**: `music::Info+0x14` is UNUSED PADDING in
   the stock class (verified: every Info ctor on 20250805 AND 20260721
   writes the 8-byte inline code buffer at +0xC..+0x13 — len byte at +0xC,
   ≤7-char code at +0xD, the vt+0x08 getter literally `return this+0xD` —
   and the next field, title, starts at +0x18). A third cave computes the
   length from chart timing data (walks a 0x60-stride array, dword ms at
   +0x08 per entry, `(last−first)/1000 + 1`), maxes it into Info+0x14, and
   he also patched the Info copy ctor to carry the private field through
   copies. **The stock game has NO song-length field at select time** —
   which is why our design derives length from the wavebank publication
   (audio length) instead.

## 6b. music::Info / selection-chain layout (statically verified 20260721)

- `selectmusic_model` global (0x1806f2d50) holds a POINTER to the model —
  one deref BEFORE the +0x1B0 offset (the card tick does
  `MOV R11,[global]` then `[R11+0x1B0]`). Missing this deref caused the
  2026-08-16 cabinet crash (wild call through garbage "vtable").
- `model+0x1B0/+0x1B8` = weak_ptr{holder, ctrl} of the highlighted entry.
  The holder (`selectmusic::sequence::ChartMetadata`, no vtable) has the
  inner `shared_ptr<music::Info>` at +0x00/+0x08 (the pair the weak-lock
  helper `FUN_1801a7930` reads).
- `music::Info` vftable @ 0x18036e858 (RTTI `.?AVInfo@music@@`): slot 1
  (vt+0x08) = code getter returning the inline buffer at this+0xD; vt+0x20
  title SSO string at +0x18; vt+0x30 artist.

## 7. Existing DLL machinery this rides on

- `bm2d_api` — named libafp exports (already resolves `afp_mc_get_param`
  etc.); CMovieClip pool scanning exists (`scan_for_child`,
  `for_each_active`). The music_info clip wrapper can be found by scanning
  the BM2D pool for name @ wrapper+0x114 (the error-log name field) or via
  a pool-walk by layer name.
- `avs_layeredfs::ifs_textures` — plain texture replacement (the bg).
- `avs_layeredfs::atlas_cloner` — net-new texture names at donor positions
  (`texturelist.merged.xml` + cloned atlas cache) — built for exactly the
  colon case. Donor: `muca_card_bpm_question`.
- `input_manager::on_frame` — per-frame callback (added for preview restart).
- `scene_manager` — scene 25 gating.
- Allocator discipline: the names vector's strings are freed by game CRT
  `free` after the move — keep every injected texture name ≤15 chars
  (SSO, no heap) and allocate the vector backing array with `game_malloc`.
  Stock digit names (`muca_card_bpm_0` = 15 chars) are SSO-safe too.

## 8. New signatures needed (all fail-open → mod absent)

1. `spritelayer_ctor` (`FUN_1801d2e00`) — distinctive long field-init.
2. `spritelayer_set_names` (`FUN_1801d3070`) — or its inner pieces; the
   layout vfunc comes from the constructed object's own vtable (slot 0), and
   the dtor path is not needed (mod instance lives forever, blanked by
   setting an empty names list).
3. `selectmusic_model_global` → `DAT_1806f2d50` (+0x1B0/+0x1B8) — derivable
   from the preview-request function or the card tick; RIP-decode.
4. (maybe) `mc_find_child_wrapper` (`FUN_18026f0e0`) — only if we validate
   the anchor ourselves; layout calls it internally, so likely not needed.

## 9. Open items for implementation

- Verify `FUN_1801d3070`'s exact second-arg ABI (vector move semantics) on
  a live build before wiring; the decompiler under-reported its params.
- Extract colon art from friend's `muca_card_bpm_question.png` replacement;
  verify his bg2 texture dimensions match stock bg2 (plain replace).
- Which card backgrounds exist: `bg2` is the song-card bg; folder cards use
  different art (`muca_card_bg_brank` etc.) — LENGTH caption only shows on
  song cards. Blank-value period (wheel scrolling) shows caption with no
  digits — same UX as the friend's build.
- Offset calibration (+0xC0/+0xC8) to align digits after the baked caption —
  cabinet iteration; consider a config knob during development.
