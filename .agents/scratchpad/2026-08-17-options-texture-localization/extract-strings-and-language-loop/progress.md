# Progress: extract-strings-and-language-loop

Updated: 2026-08-17
Status: Complete (uncommitted — maintainer commits)

## Checklist

- [x] Pre-refactor snapshot: temp-patched copy of the old script → 71 files
- [x] `scripts/option_strings.py` generated programmatically FROM the old
      module's lists (fidelity by construction): LABELS(33)/HEADER_LABELS(4)/
      RIBBONS(4)/PREVIEWS(30) with `{"en": ...}` dicts; `PreviewSpec` with
      `art`/`art_pos` (None); `TemplateSpec` declared; `TEMPLATES = {}`;
      `AVAILABLE_LANGS = ["en"]`; data-only (no Pillow) — verified
- [x] `gen_option_labels.py` refactor: imports from option_strings; `LANGS`;
      `out_dir(ifs_code)`; `FontSet` + `load_fonts(lang_key)` (en wired,
      ja/ko raise until the translation step); `render_preview` takes
      `lang_key` + `PreviewSpec`; `main()` → argparse `--lang` +
      `generate_language(lang_key, ifs_code)`
- [x] GATE: post-refactor `--lang en` temp output byte-identical to the
      pre-refactor snapshot (`diff -r` clean, 71/71)
- [x] Fresh output == shipped data_mods files (71/71 `cmp` equal — no
      environment drift; the in-place final run produced zero git diff)
- [x] `--lang en` writes eng only (jpn/kor untouched — git status clean for
      them beyond Step 1's untracked scaffold)

## Notes

- Requesting a language outside AVAILABLE_LANGS exits with an explicit error
  (both via --lang validation and per-entry `text_for` guards).
- No layout constant or rendering function body changed (only signatures:
  render_preview gains lang_key; data flows from option_strings).

Status: Complete (uncommitted — maintainer commits)
