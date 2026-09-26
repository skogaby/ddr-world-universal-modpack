# Progress — score-set-re

Task: `.agents/tasks/2026-09-25-ddr-selection-a-a3-themes/step05/task-01-score-set-re.code-task.md`

- [x] Best-record anchor: `ghost_id_lookup` case-0 call (+160), score db disp32 at +151
      (0x178 / 0x188 old), callee prologue identical on 20250805 / 20260825
- [x] Target: World's resolver not called (unchecked walk); probed search + pinned getters;
      class table {3,3,3,3,0,1,2}; area = rival `set+0x54` / holder `name-4` (no World getter)
- [x] Area: `PW+0x20` behind `PW+5` (both layouts); region = `arkMDXGetLicenceKeyVersion`;
      language = `arkMDXGetGameOptionsLanguage` → World's suffix table
- [x] A3 no-record display, glyphs (`FUN_1800ffe00`), digits (`FUN_1800ff9d0`)
- [x] Install checks: theme roots' score-set trees and textures, `common_texture_v0` glyphs /
      digits (no name shared with `_v3`), all 119 area textures in the 8 area packages
- [x] `docs/ddr_selection_theme_score_sets.md`

Status: Complete (uncommitted — maintainer commits manually)
