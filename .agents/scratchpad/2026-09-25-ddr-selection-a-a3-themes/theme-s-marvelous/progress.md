# Progress — theme-s-marvelous

- [x] RE / data check: theme texture names; `_v1` ≡ `_v2` (17 donors); 5 splash shapes per theme
      template; the word structures (DDR A ⇒ `violet_glow` (0.40, 0.70), A3 ⇒ `violet_outline`
      (0.20, 0.45)).
- [x] `targets.rs` + tests (red first): `LEGACY_SKINS` 1..=8, `is_theme`, `tex_number`, `art_set`,
      grade sheets 4..=8, combo names via the tex number, `skin_bit` → `u16`. The s_marvelous
      harness host suites are green (172 + 105).
- [x] `policy::theme_package(base, skin)` + test (red first; harness 186, was 185).
- [x] Engine:
      - `assets.rs`: `legacy_target` names themes through policy with a single candidate, plus
        `package_label`;
      - `AtomicU16` masks in legacy / afp_patches / splash / combo / flash;
      - the combo seam check in `combo::add_legacy`;
      - log labels use the registered name;
      - module docs (`targets`, `legacy`, `assets`, `combo`, `s_marvelous/mod.rs`,
        `ddr_selection/mod.rs`).
- [x] Generator: art sets 1..=7, `tex_number` / `package`, a fixed-name arc probe, the
      `open_set_package` twin guard (fires on a mismatch: checked by forcing set 6 against `_v2`),
      `OUTLINE` 6 / 7, grade sheets 4..=7. Eras regenerate pixel-identical (57 files). Generated
      `data_mods/ddr_selection/s_marvelous/{6,7}/` (36 files); contact sheet reviewed.
- [x] `validate_s_marvelous.sh`:
      - `smarv_legacy_word` accepts 1..=8, with the donor region via the tex number;
      - Leg H runs over skins 1..=8 (packages by tail, art by set);
      - 111 art files match their donors;
      - the word and splash recipes pass on every theme template;
      - the render proofs of skins 6 / 7 / 8 were reviewed.
- [x] `data_mods/ddr_selection/s_marvelous/README.md` (art-set table, sizes, recipes, generator).
- [x] Gate: `cargo check` / `cargo fmt` / `./build.sh` clean; ddr_selection harness 186;
      s_marvelous harness OK (Leg H incl. themes); `--check-world` OK.
- [x] Maintainer art review (2026-09-26): ALL PURPLE (both) and DDR A's PURPLE SHADOW approved.
      A3's PURPLE SHADOW was too light, so `OUTLINE[7]` gained a shade of 0.70 (violet (168, 86,
      209) → (118, 60, 146)). The generator's new `--review DIR` rewrote
      `target/smarv_legacy_review/` (all sets), plus a one-off `a3_purple_shadow_shades.png`
      (1.00 … 0.40). The full regeneration matches the shipped art (93 files).
- [x] Cabinet demo (maintainer) — passed 2026-09-26 (follow-up: task-02 `smarv-combo-purple-shadow`)

Status: Complete (uncommitted — maintainer commits manually)
