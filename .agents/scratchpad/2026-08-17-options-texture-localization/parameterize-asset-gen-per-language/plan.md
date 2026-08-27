# Plan: parameterize-asset-gen-per-language

Status: Approved 2026-08-17 (via verified upstream approval — auto mode;
task descends from the approved plan/design recorded in context.md)

## Test scenarios

The module is engine-facing orchestration (file I/O + game ARC data) with no
host harness for its runtime behavior — per repo convention its validation is
compile gates + cabinet deploy (step demo). The testable pure surface is the
new language table, which gets a host unit test:

1. **Language-table invariants** (`#[cfg(test)]` in asset_gen.rs, runs under
   `cargo test`):
   - exactly 3 entries with ifs_codes {eng, jpn, kor}, no duplicates;
   - every `arc_path`/`ifs_name`/`ifs_mod_path` embeds its `ifs_code` and
     follows the `select_music_option_lang_<code>_v3` shape;
   - `atlas_prefix` = `copt_mods_lang_<code>`, `preview_atlas_prefix` =
     `copt_prev_<code>`; all six prefixes pairwise distinct (acceptance
     criterion 3's compile-time analog).
   Written FIRST against the not-yet-existing table (fails to compile → then
   table added → passes). TDD adapted to compile-time data: the failing state
   is the missing symbol.

2. **Build gates**: `cargo check` → `cargo fmt` → `./build.sh` (logs under
   logs/).

3. **Scaffold verification** (shell): jpn/kor tex dirs contain byte-identical
   copies of every eng PNG/XML-relevant file (diff -r, excluding .DS_Store).

4. **Cabinet expectations** (step demo, maintainer): three per-language flush
   log lines; WARN+skip when a language folder is renamed away; MODS tab
   renders in all three game languages.

## Implementation shape

1. `OptionLang { ifs_code, arc_path, ifs_name, ifs_mod_path, atlas_prefix,
   preview_atlas_prefix }`, all `&'static str`; `pub(crate) const
   OPTION_LANGS: [OptionLang; 3]` written longhand. Replaces the four
   `LANG_ENG_*` consts + `PREVIEW_ATLAS_PREFIX`.
2. `rebuild_lang_eng_atlas(xml)` → `rebuild_lang_atlas(lang: &OptionLang,
   xml: &str) -> bool`: same body with `lang.*` substitutions; the
   AVAILABLE_PREVIEWS pass moves OUT of it (it must run once, not thrice).
3. `flush_label_atlas()`:
   - snapshot registration counts (log once),
   - populate AVAILABLE_PREVIEWS from the eng folder (language-invariance
     comment),
   - loop OPTION_LANGS: `load_stock_texturelist(lang.arc_path, lang.ifs_name)`
     → on None, `log_warn!` naming the language, continue; else
     `rebuild_lang_atlas` + per-language info log; OR the successes.
4. Module doc comment updated (three-language story).
5. Scaffold copy: `cp` eng tex dir contents → jpn/kor tex dirs (exclude
   .DS_Store).

Risks / notes:
- `copt_prev` → `copt_prev_eng` renames the eng preview atlas: one-time eng
  cache bust on first boot (known watch item, by design).
- Cache isolation is already per-`ifs_mod_path` (verified in atlas_cloner) —
  no cache-key work needed.
- `custom_options::mod.rs` re-export and `lib.rs` call site unchanged
  (signature stable).

## Checklist

(mirrored in progress.md)
