# Plan: parameterize-preview-gen-per-language

Status: Approved 2026-08-17 (via verified upstream approval — auto mode)

## Test scenarios

1. **Path-derivation unit test** (`#[cfg(test)]` in preview_gen.rs, TDD-first,
   red = missing symbols): `preview_dir(lang)` yields
   `./data_mods/custom_options/select_music_option_lang_<code>_v3_ifs/tex`
   for each of the three `OPTION_LANGS`, and `template_path_in(lang, id)`
   embeds `seop_image_<id>_TEMPLATE.png` under that dir.
2. **Build gates**: cargo check → fmt → build.sh.
3. **Runtime behavior** (cabinet, Step 1 demo): per-language chrome files
   appear in all three dirs on first boot after deploy; overlays place art
   correctly (marker path); a renamed-away language template logs the
   per-language warn and doesn't abort.

The chrome/marker functions are file-I/O orchestration validated on the
cabinet per repo convention; the pure path derivation is the unit-tested
surface.

## Implementation shape

- `use crate::services::custom_options::asset_gen::{OptionLang, OPTION_LANGS};`
- `fn preview_dir(lang: &OptionLang) -> String` replaces `PREVIEW_OUT_DIR`.
- `fn template_path_in(lang: &OptionLang, option_id: &str) -> String`
  replaces `template_path_for` (which has no external callers; removed).
- `generate_chrome(option_id)`: for each lang — out path exists → count as
  ok, skip; else open template (missing → warn naming lang, continue), clear
  markers, save. Returns `any_ok`.
- `marker_rect_for(option_id, color)`: iterate OPTION_LANGS in order; first
  template that opens AND yields a clean marker wins; log fallback when the
  winning lang isn't eng; all missing → warn (existing message shape) + None.
- Module doc comment: output-dir sentence updated to per-language.

## Checklist

(mirrored in progress.md)
