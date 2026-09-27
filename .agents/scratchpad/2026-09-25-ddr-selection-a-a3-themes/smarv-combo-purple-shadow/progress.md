# Context + progress — smarv-combo-purple-shadow

Task: `.agents/tasks/2026-09-25-ddr-selection-a-a3-themes/step08/task-02-smarv-combo-purple-shadow.code-task.md`
(maintainer request 2026-09-26, during the Step 8 review; an extension of plan Step 8).

Recipe choice (candidate strips in `$TMPDIR/opencode/s8/combo_exp*.png`, from an untracked
experiment script):

- **Set 4 (X):** first shipped as `outline+glow` (the black outline violet too, matching X's word).
  Maintainer review 2026-09-26: it lost the bold black outline. It is now `glow` like 5 and 6: the
  outline is kept and only the glow around it turns violet. The `outline+glow` mode was removed from
  the generator.
- **Set 5 (2013-A) and set 6 (DDR A):** `glow` with the word's thresholds. Seeding the flood from
  every translucent pixel (to reach the counters) made no visible difference, so the border seeding
  was kept.
- **Set 7 (A3):** `outline` at shade 0.70 with a pooled sheet violet of (118, 60, 146), the same as
  the word.

Progress:

- [x] Generator:
  - `COMBO_SHADOW`, `letter_violet` (pooled), `combo_shadow`, the new file names;
  - `--review` shows both variants and `combos_only.png`;
  - the `all_purple` files are byte-identical to the old `smarvelous_{key}.png`, which were
    removed;
  - a full regeneration matches the shipped art (137 files, no strays).
- [x] `targets::legacy_combo_png(skin, color, key)` + tests (red first).
- [x] `assets.rs`: `serve_image`, `combo_pngs_for`, `stage_legacy_combo(target, color)`,
      `restage_legacy_combo`; the word's enable-time copy goes through `serve_image`.
- [x] `combo.rs`: `add_legacy(skin, color)`, `LEGACY_STAGED`, `set_legacy_color`. `legacy.rs` and
      the Judgement Color row (plus its hint) call it.
- [x] Leg H checks both variants (166 files match their donors).
- [x] Docs: data_mods README, module docs, README (S-Marvelous sentence, config row).
- [x] Gate: `cargo check` / `cargo fmt` / `./build.sh` clean; ddr_selection harness 186;
      s_marvelous harness OK; `--check-world` OK; review pages regenerated.
- [x] Cabinet demo (maintainer) — passed 2026-09-26

Status: Complete (uncommitted — maintainer commits manually)
