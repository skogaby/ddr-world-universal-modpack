# progress — textures-and-config

Task `.agents/tasks/2026-09-21-background-dancers-selection-options/step01/task-04-textures-and-config.code-task.md`;
approval chain verified; auto mode. Design §4.7 amended 2026-09-21 by the maintainer (SPLIT layout kept,
marker = right column, not 16:9).

- [x] `scripts/option_strings.py`: `LABELS` (en/ja/ko) + `TEMPLATES` for `background_dancer` /
      `background_stage` — SPLIT layout, marker `(191, 11, 170, 150)`, description lines in the text column.
- [x] `python3 scripts/gen_option_labels.py`: all three languages; only the 4 new PNGs per language added
      (`git status`: no existing PNG changed). Visual check: text left, solid green marker right, no overlap.
- [x] `scripts/check_option_takeover.py`: templates without a hand-authored reference are skipped.
- [x] `mod-config.json`: `background_dancer`, `background_stage` (`overlay: false, in_game: true`) after
      `arrow_opacity`; JSON validated.

## Deviations
- None beyond the design amendment (recorded in the design itself).

Status: Complete (uncommitted — maintainer commits manually)
