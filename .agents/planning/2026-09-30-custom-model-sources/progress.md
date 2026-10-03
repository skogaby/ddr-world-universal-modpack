# Progress — Custom dancer / stage SOURCES

Updated: 2026-10-03
Status: DONE — Steps 1–5 complete, cabinet-validated 2026-09-30 (maintainer: "everything works perfectly");
uncommitted — maintainer commits manually.
NEXT ACTION (maintainer): commit (`git add -A data_mods/custom_models` pairs the content move as renames).
Optional follow-ups: re-run the codebase-summary workflow (`.agents/summary/interfaces.md` still describes
`RegisterSpec` without the texture aliases / the family ordering rule); group the two stages into source
folders when more stages arrive (today they are legacy ⇒ CUSTOM); delete `data_mods/_cache/custom_models/`
once to drop the orphaned pre-move cache arcs.

Resume protocol: read `implementation/plan.md` (checklist + steps), `design/detailed-design.md`
(§ references), then this file. Per-task working records live under
`.agents/scratchpad/2026-09-30-custom-model-sources/<task>/` (code-assist's `progress.md` with a
`Status: Complete` line marks a finished task). Maintainer authorised autonomous progression through the
steps until a cabinet test is needed (2026-09-30); no commits — maintainer commits.

## Done

- PDD: register accepted, design + plan approved (2026-09-30).
- Step 1 (framework): `RegisterSpec::label_texture_like` / `preview_texture_like` (+ `RegisteredOption`
  stems, `register_option` registers the label under the stem); family-aware `compute_order`
  (`parent_positions`, 3-branch rule) threaded through `builder_hook` and `overlay_snapshot_rows`.
  `validate_custom_options.sh` 72/72 (was 59); `cargo check` clean. Task records:
  `.agents/scratchpad/2026-09-30-custom-model-sources/{texture-aliases,family-aware-ordering}/`.

- Step 2 (discovery): NEW pure `sources.rs` (slug / resolve_source / dir_role / has_model_content /
  row ids); `custom_content.rs` `PackDir.source`, `Plan.entries: Vec<CustomEntry>` (+ `labels()`,
  `source_counts()`), `SourceResolver` (refused ⇒ WARN + skip; same-slug ⇒ merge INFO), `//!` contract;
  `custom_scan.rs` three-level walk + per-source INFO; `lifecycle::Tables.custom` /
  `custom_entries_snapshot()`; `mod.rs` flat adapter (Step 3 replaces). background_dancers harness
  207/207 (was 196); `./build.sh` clean. Walker itself is cabinet-validated at Step 4.

- Step 3 (pure pick layer): `catalog.rs` grouped (`SourceCatalog`, `Catalog::{sources, has_custom,
  source_count, source_label, count, entry, label, key, keys, flat_entries}`, STOCK block byte-identical);
  `selection.rs` `StageChoice` / `DancerChoice` / `resolve_choice` / `source_stage_pool` /
  `source_dancer_pool` / `PickSource::Source`; `option_pick` adapted (identical draws); `options.rs`
  carries a Step-3 `Flat` shim (Step 4 replaces). Harness 211/211; `./build.sh` clean.

- Step 4 (rows — code): NEW pure `options_logic.rs` (row table, `Request`, `request_for`,
  `row_choice_key`, `row_max`, `label_for_row`; 4 tests); `options.rs` rewritten (source row +
  per-source model rows with `ShowWhen::Equals` + texture aliases, `Duplicate` re-show, versus mirror via
  a value-changed observer, `stage_request` / `dancer_request` / `row_choice_key`); `lifecycle::option_pick`
  consumes requests (within-source pools, screen-rule fallback WARN, `{source}` provenance);
  `preview/mod.rs` wired; `DANCER SOURCE` / `STAGE SOURCE` label PNGs (en/ja/ko); shipped `mod-config.json`
  lists the two source ids. Gate: `cargo check` clean, `cargo fmt`, harnesses 215 / 72, `./build.sh` clean.

- Step 5 (docs): `docs/background_dancers_research.md` §6.1 "Sources" (layout, identity, rows, pick,
  pure-vs-engine); README "Custom Background Dancers and Stages" paragraph + mod table mention. No
  learnings entry yet (none surfaced before the cabinet pass). `.agents/summary/*` is generated — re-run
  the codebase-summary workflow after the pass (components / hook-ownership / data-models mention the rows).

- Content move (maintainer + agent, 2026-09-30): the 115 dancer folders now live under
  `dancers/{Custom, DDR 3rdMIX, DDR 4thMIX, DDR 5thMIX, DDR Strike, DDR Ultramix 1, DDR Ultramix 2,
  DDR Ultramix 3}/`; the 110 DDR folders lost their series prefix (`3rdMIX Afro` → `Afro`), no collisions,
  every source label ≤ 14 bytes. Stages untouched (legacy ⇒ CUSTOM).

## In flight

- Nothing. No learnings entry: the pass surfaced no trap beyond what the module docs and
  `docs/background_dancers_research.md` §6.1 already record.

- 2026-10-03 — NEW SOURCE `DDR X + X2` (dancers + stages), content-only, uncommitted, NOT yet cabinet-tested:
  `data_mods/custom_models/dancers/DDR X + X2/` (33 folders: X's costume 01 of all twelve as `<Name> 1`,
  `Baby-Lon 2`, `Bonnie 1`, `Zero 1`; X2's recolours as `<Name> 2` / `Baby-Lon 3`, `Bonnie 2`, `Zero 2`,
  `Pix 1..4`; keys `x<skin>` / `x2<skin>`; X's costumes 02 / 03 = SuperNova 2's and are not duplicated) and
  `stages/DDR X + X2/Stage 01..06` (keys `xstage001..006`, no foot panel). Produced by
  `port_character_supernova.py GAME=x|x2` / `port_stage_supernova.py GAME=x` (RE: `docs/ps2_ddr_filedata_research.md`
  §7.6; playbook: `tools/blender_ddr_addon/README.md`). Three things to watch on the cabinet:
  1. **Stage 02's five TV surfaces are `offscreen1` screens** — expect `stages with screens: … xstage002`
     at enable and, on a movie song with Background Movies = STAGE SCREENS, the log line
     `… 5 screen material(s) sample 'offscreen1' … bound texture 1280x1280 hash 0x3420C1B9` and the movie on the
     big TV + the three stacked sub-TVs + the top band (v band remapped to 0.21875–0.78125, u unmirrored —
     check the movie is not flipped). The pin takes a stage key as well (`DDR_DANCERS_PIN=xstage002,xafro01`).
  2. **Concent's chest fan** (`x<skin>_body01` material slot, `Spine1`-weighted) — visible through the chest
     window on the SuperNova-2-style costumes (`Concent 2`), inside the jacket on `Concent 1`. The same
     `part_overlay` path now adds the fan to SuperNova 2's `Concent 1/2` on a re-run
     (`GAME=sn2 DANCERS=concent01,concent02`) — the shipped `DDR SUPRNVA 1+2` Concents predate it.
  3. **Shadow scales come from the X table** (Baby-Lon / Pix 0.35 … Concent 0.85) rather than the SN
     0.75 / 0.8 convention — check the pigs' and Baby-Lon's shadows are not comically small.
  Pin dancers with `DDR_DANCERS_PIN=xafro01,xconcent01,x2pix01,xbabylon01`. Host validation done:
  `test_tzm_dump.py` (36), `validate_ps2_ddr_tools.sh` on both extractions (0 problems),
  `validate_background_dancers.sh` (222), SN / SN2 regeneration byte-identical to the shipped files.

- 2026-10-03 — TWO NEW DANCER SOURCES, content-only, uncommitted, NOT yet cabinet-tested:
  `data_mods/custom_models/dancers/DDR FESTIVAL/` (26: `Blues 1..3` .. `Emi 1..3`, `Rhythm` / `Bass` 1..4;
  keys `fest<name><n>00`) and `dancers/DDR PARTY COLLN/` (60: `Afro 1st` .. `Bus 7th`, CS dancers as
  `Space Man CS1st` etc.; keys `pc<name><mix>00`). The maintainer asked for `DDR PARTY COLL.` and chose
  `DDR PARTY COLLN` (a trailing dot can't exist on Windows; learnings 2026-10-03). No stages: neither disc
  has 3D stage geometry (IPU movie backgrounds), so the maintainer chose dancers only. Produced by
  `port_character_strike.py GAME=festival|pc`. That script now also writes STRIKE into `DDR Strike/<Name> <n>`;
  its table was checked against the shipped folders but STRIKE was not re-run (no extraction on disk).
  RE: `docs/ps2_ddr_filedata_research.md` §6.1. Things to watch on the cabinet:
  1. Boot INFO lists the two new sources (`ddr_festival` 26, `ddr_party_colln` 60) with no key-collision
     or label-length WARNs. Total custom dancers: 257.
  2. The six **20-object** Party Collection dancers (`pckonsento1st00`, `pcspacemancs1st00`,
     `pckonsento2nd00`, `pctamakocs1st00`, `pcosharezukin1st00`, `pckaeruzukin2nd00`) have a 21-bone rig
     (no hand helpers). Check that their faces still swap and their hands render.
  3. **DISK A / DISK B** (`pcdiska1st00`, `pcdiskb1st00`) are stacks of thin rings + a ♂/♀ symbol. Check
     they read as intended at World's camera distance. **Baby-Lon 5th** plays at the table's 0.4 scale,
     like STRIKE's.
  4. Duplicates are shipped deliberately (each source = its game's whole cast). Festival's Disco / Emi /
     Rage 1..3 and Lady 1 are PC's Afro / Emi / Rage / Lady meshes and textures, and Festival `Lady 3` is
     STRIKE `Lady 1` (research §6.1 lists them all). Prune if they clutter RANDOM.
  Pin with `DDR_DANCERS_PIN=pckonsento1st00,pcdiska1st00,festblues100`. Host validation done:
  `validate_ps2_ddr_tools.sh` on both extractions (0 problems), all 1376 clips < 0.11 mm,
  `validate_background_dancers.sh` (222), re-run byte-identical (one dancer per game), all 86 previews re-import
  and render.

## Deploy & test log

2026-09-30 — Step 4 build deployed with the moved content (8 dancer sources); maintainer reports every item
below behaves as specified ("everything works perfectly as far as I can tell in-game"). No fix-ups needed.

Checklist for the Step 4 pass (design §7.3):
1. Boot: `custom content -- N dancer(s) + M stage(s) … in K source(s): CUSTOM 115[, TEST SOURCE 1]`;
   `tables ready … custom in K source(s)`; `option rows live -- DANCER: source row + K model rows (…);
   STAGE: stock row only (…)` (stages have no custom source until stages move too — Grove Street /
   Griffin House are legacy ⇒ CUSTOM ⇒ a STAGE SOURCE row with RANDOM · STOCK · CUSTOM appears).
   No new WARNs for the untouched legacy folders.
2. Options modal: DANCER SOURCE directly above BACKGROUND DANCER; stepping the source swaps the model row
   on the same frame; RANDOM hides it; labels `RANDOM` / `STOCK` / `CUSTOM` / source names render; the
   source row steps by 1, model rows coarse-step by 5 (Start held).
3. Preview: a model row focused previews its value; the source row focused previews the effective pick
   (or the RANDOM badge).
4. Songs: source RANDOM ⇒ `{random}`; source S + RANDOM ⇒ `{source}` (+ the `no stage in source …`
   WARN on a movie song when S has no screen stage, then a stage of S); explicit ⇒ `{option}`.
5. Versus: P1's STAGE SOURCE / stage row edits mirror to P2 (and P2's visible stage row follows);
   dancer rows independent.
6. Reboot: every row value persists (`custom_options.p1/p2.background_dancer_*`); an old
   `background_dancer` value ≥ 27 loads as RANDOM; a value under a removed source id disappears at the
   next save.

## Deviations & open questions

- D5 refined at design time: same-slug source folders MERGE (not skip); only the reserved slug `source`
  and unprintable names are refused.
- `source_stage_pool` returns `(subset, StagePool)` (the task allowed either shape).
- Legacy friendly folders holding a nested model-bearing subdirectory now promote to a SOURCE (was: the
  nested dir was ignored) — documented in the `custom_content.rs` layout block; note in README at Step 5.

## Key facts for a cold resume

- Feature lives in `src/mods/background_dancers/`; framework in `src/services/custom_options/`.
- Harnesses: `scripts/validate_custom_options.sh` (framework), `scripts/validate_background_dancers.sh`
  (mod pure layers). Engine wiring is cabinet-only (Step 4).
- Readiness gate per step: `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` → `./build.sh`.
- Step 4 deploy must carry `data_mods/` (new label PNGs) and the shipped `mod-config.json` change.
