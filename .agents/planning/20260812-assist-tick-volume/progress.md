# Progress: Assist Tick Volume Option

Updated: 2026-08-12
Status: DONE — all 6 steps complete (cabinet-validated 2026-08-12)

NEXT ACTION: None. Feature complete. (Working-tree changes in both repos await the
maintainer's own commits, per their directive that the agent never commits.)

Resume protocol: read `implementation/plan.md` (approved, has the step checklist) and
`design/detailed-design.md` (approved, R1–R11 + component specs); `research/orientation.md`
carries the file:line map of every touch point; `code-assist/` holds the run's context,
plan, and logs.

## Done

- 2026-08-12 — PDD complete: register (12/12 accepted), design approved (incl. backend
  codegen revision + consolidated validation), 6-step plan approved.
- 2026-08-12 — Plan Step 1 (textures): `scripts/gen_option_labels.py` LABELS + Preview
  entries; regenerated; two new PNGs verified (176×16 label renders "TICK EFFECT
  VOLUME (%)"; 368×172 preview copy verified against dark backdrop). Uncommitted
  (maintainer commits).
- 2026-08-12 — Plan Step 2 (row/latch/threading): constants, TICK_VOLUME/LATCHED_VOLUME,
  normalize_volume/on_volume_change/latched_volume/register_volume_row (fail-open, WARN
  paths, Duplicate reseed), SongState.volume_percent, gameplay-entry latch, rebuild_for
  read + `volume=` in song-build log, disable() resets. `cargo check` clean.
- 2026-08-12 — Plan Step 3 (synthesis): `se_bank_synth::scale_pcm` (+ re-export),
  Action::Anchor 7th field, spawn_synthesis volume param + identity-shortcut scale,
  `volume={}%` in synthesis log. Gates: check ✓ fmt ✓ build.sh ✓
  validate_se_bank_synth.sh ✓ (ALL CHECKS PASSED).
- 2026-08-12 — Plan Step 4 (bemani-buddy): `mod_assist_tick_volume` s32? in both model
  JSON option blocks → codegen regenerated `playdata_3.rs` (8 lines, field-only);
  migration `013_ddr_world_assist_tick_volume.sql` (nullable, 008/011 convention comment;
  applied to the local DB); profile model + DAO (row_to_profile, UPDATE cols + binds);
  handler load emission / fresh-profile None / save `child_i32` capture; 5 unit tests
  mirroring the song_speed set; `.sqlx` cache regenerated. Gates: build ✓ test ✓
  (all pass, incl. the maintainer's 5 song_speed tests) clippy ✓.
- 2026-08-12 — Plan Step 5 (docs): README row_order example + id list +
  Assist Tick feature-row sentence; AGENTS.md Assist Tick entry (volume child +
  scale_pcm + migration 013 + planning-dir pointer). Final gates: check ✓ fmt ✓
  build.sh ✓.
- 2026-08-12 — Plan Step 6 (cabinet validation): maintainer-run end-to-end pass —
  everything worked. Feature closed.

## Deviations

- **bemani-buddy `cargo fmt` reverted.** That repo's AGENTS.md lists `cargo fmt` among
  its commands, but its tree is not kept fmt-clean: a workspace-wide fmt reformatted ~53
  files untouched by this feature (interleaved with the maintainer's uncommitted
  song_speed work). Resolution (conservative): reverted every file outside the feature's
  5-file set to HEAD from captured diffs, restored the maintainer's song_speed hunks
  verbatim, re-ran codegen for the wire structs, and matched surrounding style by hand
  instead of running fmt. Final diffs verified purely additive (0 deletions in the
  handler; codegen file = 8 added lines only). The reflowed UPDATE query string changed
  its sqlx hash, so `.sqlx` was re-prepared against the final query text.
- **Handler save-path comment consolidated.** The maintainer's uncommitted song_speed
  hunk carried a two-line "stored verbatim" comment; with two adjacent verbatim-stored
  fields the reapplied version uses one shared comment covering both. Content preserved,
  wording pluralized.

## In flight

- All Step 1–5 changes sit UNCOMMITTED in both working trees (maintainer directive):
  modpack — `scripts/gen_option_labels.py`, 2 new PNGs, `src/mods/assist_tick.rs`,
  `src/services/se_bank_synth/{containers,mod}.rs`, `README.md`, `AGENTS.md`;
  bemani-buddy — model JSON, regenerated `playdata_3.rs`, migration 013, profile
  model/DAO, handler + tests, `.sqlx` (stacked on the maintainer's uncommitted
  song_speed change, preserved intact).

## Deploy & test log

- 2026-08-12 — Maintainer's consolidated Step 6 cabinet pass (deploy against
  bemani-buddy with migration 013 applied): everything worked end to end.
  Feature accepted and closed.

## Deviations & open questions

- None yet.

## Key facts for a cold resume

- New child scalar row `assist_tick_volume` under `assist_tick` (ShowWhen::Equals, value 1);
  25–175 / fine 5 / coarse 10 / default 100; `ScalarFormat::Integer` ("%" lives in the label
  "TICK EFFECT VOLUME (%)").
- Chosen side's (FR-5) value applies; latched at GAMEPLAY entry beside `LATCHED_ENABLED`;
  applies next song; 100 % must skip scaling entirely (byte-identical track — R4).
- Volume applied by pre-scaling the clap PCM on the synthesis thread via new pure
  `se_bank_synth::scale_pcm`; `synthesize_track` and the host-validation harness untouched.
- Persistence: generic framework (`mod_assist_tick_volume` wire + JSON cache) with
  clamp+snap `load_transform`; fail-open to unity if the scalar row can't register.
- Backend (bemani-buddy): model JSON `models/ddr_world/playdata_3.json` → codegen (never
  hand-edit `@generated`) → migration (next after `012`) → db model/queries → playdata.rs
  handler → regenerate `.sqlx`. That repo's working tree currently carries the uncommitted
  `mod_song_speed` change — stack on it, don't clobber.
