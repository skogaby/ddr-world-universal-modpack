# AFP Tooling Research — bemaniutils capability + modpack inventory

Date: 2026-08-29. Feeds D20 (client-side runtime AFP synthesis in Rust).
Sources: read-only survey of the sibling bemaniutils checkout
(github.com/DragonMinded/bemaniutils, **Unlicense** — freely transcribable) and
of this repo's existing format/runtime layers.

## 1. Verdict up front

The gap is **narrower than assumed**. The modpack already ships minimal AP2
binary editing (`core/afp.rs` + `services/afp_patcher.rs`, cabinet-proven in
`series_expansion` and `folder_expansion`), full arc repack, donor-anchored and
FRESH atlas injection, and GE2D label rewriting. bemaniutils supplies a
complete AP2 **read** specification but has **no AP2 writer** — TXP2
`unparse()` round-trips the AP2 blob verbatim. What must be built is exactly
one thing: a real AP2 tag/timeline document model with round-trip
serialization (`core/ap2/`), transcribed from bemaniutils' parser.

## 2. bemaniutils — what it documents (all in `bemani/format/afp/`)

- `README.md` (210 lines): container relationships (AFP + BSI + geo + textures;
  TXP2 for older games, standard IFS with `afp/`, `afp/bsi/`, `geo/`, `tex/`
  for newer — DDR World is the IFS shape), animation model, per-tag semantics.
- `swf.py` (2934 lines): the complete AP2 parser. Key structures:
  - Header: magic @0, length @4, name offset @10, tag-section ptr @36,
    string table offset/size @48/52; container versions 7–10.
  - BSI descramble: u16 words — `offset=(w&0x7F)*2`, `swap_type=w>>13`,
    `loops=(w>>7)&0x3F`; self-inverse byte-reversal runs.
  - String table: rolling cipher (`(byte − key) & 0xFF`, key starts 128,
    increments per byte); null-terminated UTF-8, u16 table-relative offsets.
  - **Tag section** (`__parse_tags`): header `<HHIIIII` = name_reference_flags,
    name_reference_count, frame_count, tags_count, name_reference_offset,
    frame_offset, tags_offset. Frames = packed u32 each (low 20 bits = start
    tag index, next 12 = tag count for that frame). Tags = u32 header
    (`tagid=(w>>22)&0x3FF`, `size=w&0x3FFFFF`) + payload, 4-byte aligned.
  - **Frame labels are NOT tags**: a trailing name-reference array of `<HH>`
    (frame_number, string_offset) pairs. Root movie and every
    `AP2_DEFINE_SPRITE` carry their own label→frame map.
  - `AP2_DEFINE_SPRITE (0x79)` is **recursive** — contains a nested tag
    section (own frames/tags/labels).
  - `AP2_PLACE_OBJECT (0x7F)`: flag-driven encoding; **flag 0x20 =
    movie_name** (named instance — how layouts address widgets); documented
    fixed-point conventions (scale s32/1024, translate s32/20, colors s16/255…).
  - `AP2_DEFINE_EDIT_TEXT (0x7E)`: the dynamic text field — fixed 44-byte
    record (flags, id, font tag id, height, rect/20, RGBA, variable_name,
    optional default text). Fonts are atlas lookups via fontdata BinXML.
- `geo.py`: GE2D shape format — vertex/UV floats, labels = texture REGION
  names (obfuscated strings), draw params; UVs are absolute atlas coordinates.
- `render.py`: full CPU renderer (`AFPRenderer`) → PIL frames / GIF / WEBP —
  usable as an **offline validation oracle** for our edited files (research
  tooling only, per D20).
- CLI: `./afputils extract|update|print|parseafp|decompile|parsegeo|render|list`.

## 3. Modpack — what already exists (reusable)

| Capability | Where | Status |
|---|---|---|
| AP2 descramble/rescramble + string-table cipher round-trip | `src/core/afp.rs` (`apply_bsi`, `decode_stringtable`/`encode_stringtable`) | shipped |
| Tag-header math, DefineSprite/PlaceObject(0x22) construction, frame-0 child injection **with frame-table shifting** | `src/core/afp.rs` (`patch_inject_children`) | shipped (series_expansion) |
| **In-memory AP2 patch seam**: name-keyed patches at `afp_stream_do_create` — data arrives ALREADY DESCRAMBLED, no arc/MD5/BSI handling needed | `src/services/afp_patcher.rs` (`register_patch(name, fn)`) | shipped; also `register_raw_interceptor` (all stream data incl. GEO, currently unused) |
| Arc parse/extract/repack/path-rewrite | `src/core/arc.rs` | shipped, host-tested |
| Lazy arc synthesis + fingerprint cache under `data_mods/_cache/` | `avs_layeredfs/arc_handler.rs` + `shader_synthesis.rs` (dispatch is a hardwired `if norm_path == "arc/shader.arc"` — needs a small registry refactor for a second producer) | shipped |
| Texture injection: donor-anchored clone (keeps GE2D UVs valid) + FRESH shelf-pack; merged texturelist serving | `avs_layeredfs/atlas_cloner.rs`, `ifs_textures.rs`, `xml_merger.rs` | shipped |
| GE2D label (region-name) rewriting | `folder_expansion.rs::patch_ge2d_labels` | shipped |
| MD5-member serving incl. manual AFP/BSI/GEO mapping registration | `ifs_textures.rs` (`register_afp_*_mapping`) | shipped |
| Runtime clip ops: `layer_play_raw`, `layer_set_attribute_raw`, `mc_op(0xF08 SetFrame)`, `mc_load_bitmap`, find/search child, color/scale/matrix raw | `src/services/bm2d_api.rs` | shipped |
| BM2D package load/lookup/release (loads synthesized arcs — proven by bg_preview alias arcs) | `src/services/bm2d_package.rs` | shipped |

**Missing runtime wrappers:** frame-label lookup (`FUN_18026F3E0`) and
goto-frame (`FUN_18026EE80`) are gamemdx-internal (not libafp exports) — the
stock judgement-flash handler uses them. Alternative that avoids new
signatures: since OUR synthesizer places the labels, the mod knows the frame
numbers and can drive `mc_op(0xF08, frame)` numerically.

## 4. Gap list for the Rust AP2 editor (`core/ap2/`)

1. Full tag/timeline document model: enumerate tags by type, recursive
   DefineSprite subsections, round-trip parse → mutate → serialize with all
   offset/length fixups (bemaniutils `swf.py` is the transcription source).
2. Frame-label table read/write (name-reference arrays, root + per-sprite).
3. Mid-timeline segment insertion (frames + tags + labels + later-frame
   fixups) — `patch_inject_children` only splices at root frame 0.
4. Richer PlaceObject encoding (matrix/position, color transform, update
   semantics, RemoveObject) — currently only flags 0x22.
5. GE2D synthesis beyond label rewrite (vertex/UV/draw-param editing or new
   shape creation) — AVOIDABLE if new art donor-clones atlas cells at
   identical pixel rects and we only rewrite labels (shipped pattern).
6. Optional: frame-label/goto wrappers (or the numeric-frame alternative above).
7. Host tests for the format layer (house pure-module style: inline
   `cargo test`, plus a validate script with byte-identity oracles — e.g.
   fixtures cross-checked against bemaniutils `parseafp`/renderer offline).

## 5. Emergent synthesis architecture (for the design)

Per-surface, using ONLY client-side generation (D20):

- **Textures**: recolored PNGs committed under `data_mods/s_marvelous/...`;
  injected at runtime via atlas_cloner (donor-anchored where cloned geo UVs
  must stay valid; FRESH where the mod controls UVs).
- **AP2 timeline edits** (dance_judge `in_smarvelous` segment, FC splash
  label, results-layout row): `afp_patcher::register_patch(template_name, fn)`
  — in-memory, descrambled, no arc synthesis needed for the AFP bytes
  themselves. The patch fn runs the `core/ap2` editor: clone the donor sprite
  segment, re-point shape → new geo name, add label, splice.
- **Geo**: clone donor GE2D binary + `patch_ge2d_labels`-style region-name
  rewrite; serve via `register_afp_geo_mapping` (or raw interceptor).
- **Caching**: afp_patcher route is in-memory at stream-create (cheap; may not
  need `_cache` at all for AP2 — measure; texture atlas rebuilds already
  cache under `data_mods/_cache/`).

## 6. Open RE topics (display-side, Ghidra)

1. **Results graph tab** (in scope per D21): data source of the per-step/
   per-section judge markers (`scre_tab_graph_judge_%s`), whether a marker
   variant can be added display-side, legend handling (compiled-in Shift-JIS
   strings — what renders them).
2. **Results score tab**: how `*_num_usr` widgets are defined in the layout
   AFP (EDIT_TEXT? named MC?), what a mod-side set-text-by-name path looks
   like, and where row label art (`scre_tab_detail_*`) is placed — to add the
   S-MARVELOUS row natively.
3. **FC splash**: clip identity/access for msg 0x1034 re-drive (label set,
   actor layout) + whether a results-screen FC emblem exists as a distinct
   element to override for S-MFC.
4. **Combo digits**: confirm the mod-side re-drive path for the suffix
   mechanism (`daco_combo{suffix}_{digit}` loads via `afp_mc_load_bitmap`)
   and where the digit MCs live (ComboActor +0x6C context).
