# Research: musicdb enumeration and options-UI mechanics

## 1. Enumerating all song basenames (stock + custom) — recommended approach

**Hook-free background task reusing the LayeredFS merge machinery.** No new
detours.

- The game opens `/data/gamedata/musicdb.xml` via `avs_fs_open` (~750 ms after
  boot); AVS resolves it out of `data/arc/startup.arc` transparently. The
  LayeredFS `fs_open` hook redirects `.xml` opens through
  `xml_merger::merge_xmls(norm_path, original_path)`
  (`src/services/avs_layeredfs/xml_merger.rs:19-93`), which:
  - collects mod fragments via `mod_paths::find_all_modfile("gamedata/musicdb.merged.xml")`,
  - loads the stock XML through AVS trampolines
    (`load_xml_from_avs_path`, kbin-safe, arc-transparent),
  - appends fragments before `</mdb>` and writes the merged file to
    `./data_mods/_cache/gamedata/musicdb.xml` (hash-cached across boots),
  - returns `None` when no fragments exist (game reads stock directly).
- **Our crawl**: spawn a background thread; call
  `xml_merger::merge_xmls("gamedata/musicdb.xml", "/data/gamedata/musicdb.xml")`
  ourselves — `Some(path)` → read the merged cache file (byte-identical to what
  the game parses, custom songs included); `None` → check
  `find_first_modfile("gamedata/musicdb.xml")` (whole-file override) then
  `load_xml_from_avs_path` (stock). Scan flat `<basename>xxxx</basename>` tags.
- Timing: LayeredFS init (lib.rs step 3c) precedes the game's musicdb read;
  AVS trampolines are usable once hooks are installed. Background-thread
  precedents: splash poller (lib.rs:513), chart_length worker.
- Rust ARC parsing (`core/arc.rs::parse/extract` + `avslz::decompress`) works
  on stock startup.arc but is NOT needed with the merge-reuse approach (and
  would re-implement first-mod-wins/merge resolution).
- Caveat: the XML union includes ~11 entries the game's availability filter
  later drops from its in-memory vector — acceptable for a CSV seed.

## 2. Scalar row rendering — negatives

- Scalar value display flows `format_scalar_value` (sign-aware) →
  `textlayer_set_text` on the row's value TextLayer (`row+0x130`) → the game's
  own `seop_num_*` digit-sprite compositor. Not a Rust widget; no per-value
  textures.
- The **stock JUDGE TIMING row (±100) renders through this exact pipeline**, so
  the glyph set includes `-` and 4-glyph strings. Failure mode if wrong: blank
  glyph, not a crash. One cabinet sanity check recommended on first deploy.
- Press handler math is sign-safe (`saturating_add` + clamp from `min`);
  marker index derivation computes offsets from `min` in i64 — `min = -100`
  works as-is.
- Note: mod_menu overlay rows with negative mins (timing_offsets,
  music_wheel_song_length) are a DIFFERENT render path (KBF font widgets) and
  prove nothing about the options-menu compositor.

## 3. Bool + child scalar row mechanics

- `RegisterSpec::bool_toggle(id)` = `UiKind::Enum` over stock ribbon textures
  `seop_op_off`/`seop_op_on` — zero new value-side assets.
- `ShowWhen::Equals { parent_id, value: 1 }` on a scalar child = the shipped
  assist_tick → assist_tick_volume pattern. Parent must register first
  (synchronous `UnknownParent` validation). Live toggle remask: press handler →
  `update_children_visibility` → `options_scroll::reapply_mask_for_side` —
  child appears/disappears same frame, per side.
- Child registration must be gated on `custom_options::row_injection_available()`
  (scalar machinery can be absent while the bool row works).
- Programmatic updates while the menu is open are safe and repaint same-frame:
  values are read live from the registry each frame (`mod.rs:398-406`);
  `set_value_silent` (no callback) is the seed primitive
  (`profile_fields::seed` pattern).

## 4. Assets checklist (two new rows)

Strings are data-only in `scripts/option_strings.py`; `gen_option_labels.py`
writes all three language dirs
`data_mods/custom_options/select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/`.

- Parent bool `adjust_song_offset`:
  - `LABELS["adjust_song_offset"]` (en/ja/ko)
  - `PreviewSpec('adjust_song_offset', 'off'|'on', ...)` × 2
  - Generated ×3 langs: `seop_item_adjust_song_offset.png`,
    `seop_image_adjust_song_offset_{off,on}.png`
- Child scalar `current_song_offset`:
  - `LABELS["current_song_offset"]`
  - `PreviewSpec('current_song_offset', None, ...)` (scalar = single panel)
  - Generated ×3 langs: `seop_item_current_song_offset.png`,
    `seop_image_current_song_offset.png`
- No ribbons, no digit assets (`seop_num_*` stock), no preview chrome
  (`generate_chrome` is WebUI-picker-only). Missing preview PNG = hidden
  preview box, not an error.
- Atlas injection is automatic via `asset_gen::flush_label_atlas` at init.

## 5. Song-wheel selection (from orientation)

`music_wheel_song_length` pattern verbatim: `selectmusic_model` signature →
`*(model)+0x1B0/+0x1B8` weak_ptr poll in `input_manager::on_frame` (scene 25
gate) → liveness via ctrl strong count → `read_song_code()` (guarded vtable
getter). Selection change = raw pointer compare.
