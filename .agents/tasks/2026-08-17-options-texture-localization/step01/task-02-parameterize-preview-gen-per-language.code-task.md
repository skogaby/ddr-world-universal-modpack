# Task: Parameterize preview_gen per language

## Description

Make the WebUI Options preview-chrome machinery in
`src/mods/webui_options/preview_gen.rs` language-aware: the generated base
chrome (`seop_image_<id>.png`) is produced per language from that language's
`_TEMPLATE.png`, while marker-rect lookups keep a single geometry source with
cross-language fallback. Completes the DLL half of the localization pipeline
started by task 01.

## Background

- `preview_gen.rs` hardcodes `PREVIEW_OUT_DIR` to the eng tex dir (line ~49).
  Three consumers ride it:
  - `template_path_for(option_id)` → `seop_image_<id>_TEMPLATE.png` lookup,
  - `generate_chrome(option_id)` (called from `src/mods/webui_options/mod.rs`
    ~line 184 on the scalar registration path) — loads the template, clears
    every solid green/red marker rect to transparent, writes
    `seop_image_<id>.png` skip-if-exists,
  - `marker_rect_for(option_id, color)` — re-reads the template so
    `preview_overlay`/`bg_preview_overlay` place live art exactly in the
    cleared region.
- Chrome carries baked per-language text (from the templates), so the OUTPUT
  must be per-language. Marker GEOMETRY is language-invariant by design
  (requirement R4: rects byte-identical across languages), so marker lookups
  need only one authoritative template, with fallback in case a language's
  file is missing.
- Task 01 defines the `pub(crate)` `OptionLang` table (`OPTION_LANGS`) in
  `src/services/custom_options/asset_gen.rs`; this task derives its three tex
  dirs from it — do not duplicate the code list.
- Task 01's scaffold copies put (English) `_TEMPLATE.png` files in all three
  language dirs, so this task is fully exercisable now.
- `preview_overlay.rs` / `bg_preview_overlay.rs` consume `marker_rect_for`
  and must not need changes.

## Reference Documentation

**Required:**
- Design: .agents/planning/2026-08-17-options-texture-localization/design/detailed-design.md
  (sections: Detailed Requirements R2/R4, Components and Interfaces §5,
  Error Handling)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Replace `PREVIEW_OUT_DIR` with a `preview_dir(ifs_mod_path: &str)`- or
   `OptionLang`-based helper deriving
   `./data_mods/custom_options/<ifs_mod_path>/tex` from the task-01 table.
2. `generate_chrome(option_id)` iterates the three languages: for each, if
   `seop_image_<id>.png` exists in that language dir, skip (per-language
   skip-if-exists — preserves authored base chrome like
   `seop_image_customize_background.png` per dir); else load that language's
   template, clear all markers, write that language's chrome. A missing/
   unreadable template in one language logs one warn line naming the language
   and skips it; other languages proceed. Return `true` if a base image
   exists (shipped or generated) for at least the languages whose templates
   exist — preserve the current caller contract for the eng case.
3. `template_path_for` becomes language-explicit (takes the language dir or
   an `OptionLang`), or is replaced by an internal helper; no caller outside
   `preview_gen.rs` uses it today except via `marker_rect_for` — verify with
   a grep before changing its signature.
4. `marker_rect_for(option_id, color)` resolves the template eng-first, then
   jpn, then kor, returning the first language whose template loads and
   yields a clean marker; log (once per option) when falling back past eng.
   Geometry is language-invariant by construction, so any hit is
   authoritative.
5. `find_marker`, `apply_gamma`, `find_asset_arc*` and the `MarkerRect` type
   are unchanged.
6. Update the module doc comment's description of the output directory.

## Dependencies

- Task 01 (`task-01-parameterize-asset-gen-per-language`): the `OPTION_LANGS`
  table and the jpn/kor scaffold content.

## Implementation Approach

1. Import the language table from `custom_options::asset_gen` (add a
   `pub(crate)` re-export through `custom_options` if module visibility
   requires it).
2. Thread the language dir through `template_path_for` / `generate_chrome` /
   `marker_rect_for` per the requirements; keep the public (crate-internal)
   call sites' signatures unchanged where possible (`generate_chrome(option_id)`
   and `marker_rect_for(option_id, color)` keep their current signatures —
   the language handling is internal).
3. Run the readiness gates: `cargo check --target x86_64-pc-windows-msvc`,
   `cargo fmt` (whole crate), `./build.sh`.

## Acceptance Criteria

1. **Per-language chrome generation**
   - Given the three language dirs contain `seop_image_<id>_TEMPLATE.png`
     files and no corresponding `seop_image_<id>.png`
   - When `generate_chrome(option_id)` runs at registration time
   - Then each language dir gains its own `seop_image_<id>.png` with every
     marker rect cleared to transparent, generated from that dir's template

2. **Per-language skip-if-exists**
   - Given one language dir already contains `seop_image_<id>.png`
   - When `generate_chrome(option_id)` runs
   - Then that language's file is left untouched while the other languages
     still generate

3. **Missing-template isolation**
   - Given one language dir lacks the template
   - When `generate_chrome(option_id)` runs
   - Then one warn naming that language is logged, the other languages
     generate normally, and no panic or early abort occurs

4. **Marker lookup with fallback**
   - Given the eng template is present
   - When `marker_rect_for` is called
   - Then the rect comes from the eng template; and given the eng template is
     absent but jpn's is present, the rect comes from jpn with a fallback log
     line

5. **Overlay consumers unchanged**
   - Given `preview_overlay.rs` and `bg_preview_overlay.rs` are not modified
   - When the crate builds
   - Then no changes are required in either file (the `marker_rect_for` /
     `generate_chrome` surfaces are source-compatible)

6. **Build gates pass**
   - Given the implementation is complete
   - When running `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`,
     and `./build.sh`
   - Then all three complete cleanly

## Metadata

- **Complexity**: Low-Medium
- **Labels**: localization, webui-options, preview, dll
- **Required Skills**: Rust, project webui_options/preview pipeline familiarity
- **Generated By**: code-task-generator 2026-08-17
- **Source Plan**: .agents/planning/2026-08-17-options-texture-localization/implementation/plan.md
- **Plan Step**: Step 1: Parameterize the DLL per language and prove the runtime pipeline end-to-end
