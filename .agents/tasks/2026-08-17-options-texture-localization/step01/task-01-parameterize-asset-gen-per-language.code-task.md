# Task: Parameterize asset_gen per language

## Description

Replace the eng-hardcoded constants in `src/services/custom_options/asset_gen.rs`
with a three-entry language table (`eng`, `jpn`, `kor`) and restructure the
Mods-tab atlas flush to build all three languages' injected atlases at init,
with per-language failure isolation. Also populate the jpn/kor mod folders
with verbatim copies of the shipped English textures as scaffold content so
the new pipeline is exercisable end-to-end on the cabinet before real
translations exist (a later plan step overwrites these copies).

This is the foundation of the options-texture localization feature: today a
player who sets the game to Japanese or Korean gets blank labels on the MODS
tab because the donor-clone atlas build only ever targets
`select_music_option_lang_eng_v3`.

## Background

- The game loads a language-specific options IFS
  (`select_music_option_lang_<code>_v3.ifs`, code ∈ `eng`/`jpn`/`kor`) chosen
  by the player's per-user language setting. The DLL never needs to know the
  active language — it prepares all three IFSes' injections and the game
  opens exactly one.
- `asset_gen.rs` currently drives the whole injection from four constants
  (`LANG_ENG_ARC`, `LANG_ENG_IFS`, `LANG_ENG_IFS_MOD_PATH`,
  `LANG_ENG_ATLAS_PREFIX`, lines ~40-43) plus `PREVIEW_ATLAS_PREFIX`
  (`copt_prev`). `flush_label_atlas()` (called once from `lib.rs::init` after
  all mods enable) loads the stock texturelist from the eng ARC and
  `rebuild_lang_eng_atlas` builds one base `AtlasSet` (tab title + labels +
  ribbons, donor-slot mode) plus one fresh `AtlasSet` (previews) sourced from
  the eng mod folder.
- The stock donors (`seop_item_appearance`, `seop_op_on`,
  `seop_image_scroll_speed`, `seop_tab_title_basic`) are verified to exist
  with identical dimensions in all three stock language IFSes, each of which
  carries its own `texturelist.xml`.
- The atlas build is disk-cached (`generate_cloned_atlases_cached` keyed on
  inputs); warm boots skip the expensive decode/pack/convert. Language-
  distinct atlas prefixes keep the three languages' cache entries and texture
  names disjoint.
- `AVAILABLE_PREVIEWS` (which PNGs existed at flush time; gates the preview-
  box name getter) is currently populated from the eng folder. With scaffold
  copies the three folders have identical content; keep this eng-sourced (it
  is a per-name availability set, not per-language) — note this in a comment.
- `generate_static_tab_assets` (tab icon in the BASE, non-language IFS) is
  out of scope — leave untouched.

## Reference Documentation

**Required:**
- Design: .agents/planning/2026-08-17-options-texture-localization/design/detailed-design.md
  (sections: Detailed Requirements R1/R2, Components and Interfaces §4,
  Data Models "DLL language table", Error Handling)

**Additional References (if relevant to this task):**
- .agents/planning/2026-08-17-options-texture-localization/research/stock-lang-ifs.md
  (donor verification and atlas-prefix collision rationale)
- .agents/planning/2026-08-17-options-texture-localization/research/orientation.md
  (§ "DLL-side findings" — why auto-inject is not sufficient for these textures)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Define a `pub(crate)` `OptionLang` struct and a 3-entry
   `OPTION_LANGS: [OptionLang; 3]` table in `asset_gen.rs` with fields per
   the design: `ifs_code`, `arc_path`, `ifs_name`, `ifs_mod_path`,
   `atlas_prefix` — written out longhand (three const entries, greppable),
   not string-formatted at runtime. Add a `preview_atlas_prefix` (or derive
   it consistently): `copt_prev_<code>` (e.g. `copt_prev_eng_000` once the
   cloner appends its spill counter).
2. Expose the table (or an iterator over it) `pub(crate)` so
   `src/mods/webui_options/preview_gen.rs` can derive its per-language paths
   from the same source in the follow-up task.
3. Restructure `flush_label_atlas()` to loop `OPTION_LANGS`: each iteration
   loads that language's stock texturelist from its ARC and rebuilds that
   language's atlases sourced from that language's mod folder
   (`data_mods/custom_options/<ifs_mod_path>/tex/`).
4. Per-language failure isolation: a missing/unreadable stock ARC, or a
   cloner failure, logs one `log_warn!` naming the language and continues to
   the next language. `flush_label_atlas` returns `true` if at least one
   language's merged texturelist is present afterward.
5. The registration surfaces (`register_label_for`, `register_preview_images`,
   `register_op_ribbons`, `LABEL_REGISTRATIONS` etc.) stay language-agnostic
   and unchanged — the same registered name set drives every language's
   atlas.
6. `AVAILABLE_PREVIEWS` population stays a single (eng-folder) pass, with a
   comment noting the language-invariance assumption (all languages ship the
   same file set).
7. Scaffold content: copy every file in
   `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/` verbatim
   into `data_mods/custom_options/select_music_option_lang_jpn_v3_ifs/tex/`
   and `.../select_music_option_lang_kor_v3_ifs/tex/` (one-time `cp`, not a
   build step; these are placeholder English textures a later step replaces
   with real translations).
8. Log lines must identify the language (e.g.
   `flushed lang_jpn atlas with N label(s) + ...`) so the cabinet boot log
   shows three distinct passes.

## Dependencies

- `atlas_cloner` APIs used today (`load_stock_texturelist`, `AtlasSet`,
  `OwnedTextureSpec`, `generate_cloned_atlases_cached`, `BatchResult`) — no
  changes expected; the loop passes different arguments per iteration.
- No new crates.

## Implementation Approach

1. Introduce `OptionLang` + `OPTION_LANGS`; delete the four `LANG_ENG_*`
   constants and `PREVIEW_ATLAS_PREFIX`, fixing up all uses.
2. Rename `rebuild_lang_eng_atlas` to a language-parameterized
   `rebuild_lang_atlas(lang: &OptionLang, xml: &str) -> bool` (same body,
   constants swapped for `lang` fields; `tex()` closure keyed on
   `lang.ifs_mod_path`).
3. Rewrite `flush_label_atlas` as the `OPTION_LANGS` loop with per-language
   texturelist load, warn+skip, and per-language success logging; aggregate
   the return value.
4. Keep the `AVAILABLE_PREVIEWS` pass exactly once (eng), before/outside the
   loop, with the language-invariance comment.
5. Update the module doc comment (it currently narrates the lang_eng-only
   design).
6. Copy the eng tex dir to the jpn/kor mod folders (scaffold content).
7. Run the readiness gates: `cargo check --target x86_64-pc-windows-msvc`,
   `cargo fmt` (whole crate), `./build.sh`.

## Acceptance Criteria

1. **Three-language atlas flush**
   - Given all mods have registered their options and the three language mod
     folders contain texture PNGs
   - When `flush_label_atlas()` runs at init
   - Then the atlas build executes once per language against that language's
     stock ARC and mod folder, and the log shows three distinct
     per-language flush lines

2. **Per-language failure isolation**
   - Given one language's stock ARC (or mod folder) is missing
   - When `flush_label_atlas()` runs
   - Then exactly one WARN naming that language is logged, the other two
     languages build normally, and the function returns `true`

3. **Distinct atlas namespaces**
   - Given the three languages build successfully
   - When their merged texturelists are generated
   - Then base atlas names are `copt_mods_lang_<code>*` and preview atlas
     names are `copt_prev_<code>*` — no atlas texture name is shared between
     two languages' outputs

4. **No regression for English**
   - Given the eng mod folder content is unchanged
   - When the game runs in English after this change
   - Then the MODS tab renders identically to before (labels, ribbons,
     previews, tab title) — cabinet-verified as part of the step demo

5. **Scaffold content in place**
   - Given the copy in requirement 7 was performed
   - When comparing the three tex dirs
   - Then jpn and kor contain byte-identical copies of every eng file

6. **Build gates pass**
   - Given the implementation is complete
   - When running `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`,
     and `./build.sh`
   - Then all three complete cleanly

## Metadata

- **Complexity**: Medium
- **Labels**: localization, custom-options, atlas, dll
- **Required Skills**: Rust, project atlas-cloner/LayeredFS familiarity
- **Generated By**: code-task-generator 2026-08-17
- **Source Plan**: .agents/planning/2026-08-17-options-texture-localization/implementation/plan.md
- **Plan Step**: Step 1: Parameterize the DLL per language and prove the runtime pipeline end-to-end
