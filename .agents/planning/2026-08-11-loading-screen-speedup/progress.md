# Progress — Loading-screen speedup (Phase 1)

Updated: 2026-08-11
Status: Step 4 of 4 — cabinet-validated; awaiting commit approval + Phase-2
go/no-go.
NEXT ACTION: maintainer — approve the commit (message proposed in the session),
and decide Phase 2 (RAM cache) go/no-go given the remaining ~0.6 s.

Resume protocol: read `implementation/plan.md` (approved 2026-08-11) for the
step breakdown, `design/detailed-design.md` for the spec, `idea-honing.md` for
decisions, `research/orientation.md` for code findings + the empirical open
counts.

## Checklist (mirrors plan.md)

- [x] Step 1: 1-based scalar display (`ScalarFormat::OffsetInteger`)
- [x] Step 2: Convert the 9 cosmetic categories to scalar rows (+ chrome, dead-code removal)
- [x] Step 3: Retire the item-ribbon assets (script + committed PNGs)
- [ ] Step 4: Docs + final build gates + cabinet validation
      (code/docs/gates DONE; commit + cabinet measurement PENDING)

## Done

- Step 1: `ScalarFormat::OffsetInteger { display_offset }` added
  (`src/services/custom_options/api.rs`), formatter arm in
  `rows.rs::format_scalar_value` (`saturating_add`, display-only). `cargo check`
  clean.
- Step 2: `RenderMode::EnumIndexed` deleted; 9 cosmetic `CategoryDef`s flipped
  to `Scalar` (`discovery.rs`); scalar arm now calls
  `preview_gen::generate_chrome` before `RegisterSpec::scalar(id, 0, count-1, 1,
  OffsetInteger{1})`; `build_indexed_enum_values` + "enum-indexed" mode string
  removed (`mod.rs`). Grep gate: no functional `EnumIndexed`/`seop_op_item`
  references left in `src/`.
- Step 3: `gen_option_labels.py` — `ITEM_RIBBON_COUNT` + item comprehension
  removed, docstring/comments updated; 150 committed `seop_op_item_*.png`
  deleted. Script re-run: 26 labels + 4 ribbons + 21 previews, zero item chips,
  remaining outputs byte-stable (git shows no PNG modifications).
- Step 4 (code/docs): new `docs/scene_load_analysis.md`; AGENTS.md Key Entry
  Points row (scalar pickers + cabinet PNG warning); README picker sentence;
  stale `docs/option_preview_image_box.md` paragraph rewritten. Gates:
  `cargo check` clean → `cargo fmt` (whole crate) → `./build.sh` clean →
  `scripts/validate_song_playback_speed.sh` PASS (host harness compiles api.rs).

## In flight

Nothing — awaiting maintainer actions above.

## Deploy & test log

- **2026-08-11 23:43 run (non-verbose, dev-mode off) — Phase 1 VALIDATED:**
  - CAUTION (scene 21): **7 s → 5 s** (~30% faster; 23:44:15 → 23:44:20 vs
    baseline 21:19:33 → 21:19:40, both verbose-off runs).
  - Scene 18: ~1–2 s, unchanged as predicted (nothing injected there).
  - Atlas flush: `142 ribbon(s)` → **`4 ribbon(s)`** — exactly the 138
    registered item chips gone (138 + 4 bespoke = 142; matches the measured
    138 slow-path opens one-for-one).
  - All 9 cosmetic rows registered `(range 0..N-1, scalar)`; VIDEO SIZE still
    enum. Maintainer confirmed pickers/previews work by hand.
  - WARN set identical to baseline (pre-existing custom_series/afplist noise);
    no new warnings.
  - Cabinet `data_mods` tex dir: 0 `seop_op_item_*` remaining (80 files).
  - Measured saving ≈ 2 s ≈ 138 opens × ~9 ms/open — matches the prediction.
- **Phase 2 decision:** remaining slow-path population is ~70 opens ≈ ~0.6 s of
  the 5 s CAUTION load. Maintainer to judge whether that's worth the RAM-cache
  work.

Step-4 cabinet protocol:
1. MODS tab: cosmetic rows read "1".."N", cycle, apply, previews show chrome +
   live art (backgrounds animate).
2. Card-out/in: selections seed back (SaveOnly round-trip).
3. One `layeredfs.verbose: true` run: CAUTION `select_music_option_lang_eng_v3`
   cache-served opens ~208 → ~70; record scene-21 wall time vs baselines
   (~11 s verbose / ~7 s non-verbose). Scene 18 expected unchanged.
4. Decision: Phase 2 (RAM cache) go/no-go.

## Deviations & open questions

- Commit policy: implementation complete through the build gates; the commit
  waits for the maintainer's go-ahead (CLAUDE.md: "Commit/push only when the
  maintainer asks").
- Plan's Step-3 grep gate ("no `seop_op_item` in scripts/") refined to "no
  FUNCTIONAL references": one retirement note comment intentionally remains in
  `gen_option_labels.py` (plus the rationale mention in `api.rs`).
- Pre-existing pyright nits in `gen_option_labels.py` (return-type annotation,
  `Image.LANCZOS` alias) left untouched — not ours.

## Key facts for a cold resume

- Only CAUTION (scene 21) benefits; scene 18 carries no injected textures.
- Measured: 208 slow-path opens in the CAUTION window; 138 are
  `seop_op_item_*` (the removal target); ~70 remain after Phase 1.
- Scalar rows keep the preview box: `install_ioptionelement_vtable` is shared
  by enum + scalar builders; `generate_chrome` now runs in the scalar arm.
- Atlas cache self-invalidates via `atlasbatch.md5` (spec-list keyed); stale
  cabinet `_cache/` files are harmless (never requested again).
- **Cabinet deploy note:** the 150 PNGs must also be deleted from the CABINET's
  `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/` — if left
  there, LayeredFS's extra-PNG texture injection (`inject_new_textures`) would
  re-inject them as new textures even with no option referencing them.
- Phase 2 (RAM cache) is a design sketch only; go/no-go decided by the cabinet
  measurement.
