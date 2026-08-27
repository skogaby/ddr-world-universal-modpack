# Context: Step 5 Task 2 — Add the Player-Facing SONG SPEED Option (DLL Side)

Task file: `.agents/tasks/song-playback-speed/step05/task-02-add-player-facing-song-speed-option.code-task.md`
Mode: auto. Same verified approval lineage as task 01 (breakdown maintainer-approved
2026-08-07; plan/design `Status: Approved 2026-08-05`; scalar 25..=175 step 5/10 is the
recorded 2026-08-07 amendment). Host-only: **no deployment.**

## Build / test commands
Same five gates as task 01. New host test FILES must be registered in BOTH the
validator's file list and its generated harness `main.rs` (the harness has
`once_cell` available and can host a `custom_options` mini-module of api+registry).

## What exists (verified)

- `RegisterSpec::scalar(id, min, max, step_fine, format)` + `.step_coarse()` +
  `.default_value()` + `.on_change(fn(side: u8, value: i32))` +
  `.persist_transform(save, load)` (`load_transform: fn(id, value) -> i32` applied at
  the single choke point `resolve_from_load`, `custom_options/mod.rs:266-270` — no
  current user; song_speed becomes the first). Default persist = `PersistMode::Full`
  (network `mod_song_speed` wire field + JSON cache — automatic).
- `RegisteredOption` (registry.rs:24-40) has NO availability flag; entries stable,
  handles append-only. `register_option` auto-registers the label asset
  (`seop_item_<id>`); atlas flush at `lib.rs:492` AFTER mod enable (step 8) — rows
  MUST register during `enable()`.
- Builder hook injects rows from a per-open snapshot (`builder_hook.rs:169-183`,
  ordering applied :191-195; loop :224-250 already skips scalar allocation failures
  silently) — filtering the snapshot gives exact "next form rebuild" semantics; open
  forms are naturally immutable (rows created only in the builder detour, freed at
  modal close by the dtor hook).
- Readiness pieces: `custom_options::is_available()` (pub), `rows::is_scalar_ready()`
  (pub(crate) — is_ready + scalar donor ctor/vtable + coarse-step + textlayer),
  `builder_hook::is_ready()` (pub(crate)); `filter_hook::init` returns bool but is
  not latched (miss ⇒ Page6 rows never show — load-bearing for visibility).
- Precedents: `playfield_styling` (scalar rows 25..=125 step 5/25, per-side atomics
  via one on_change per option, Duplicate-on-reenable = success, enable-time reseed
  because a Duplicate re-register does not re-fire the prime, all-or-nothing
  readiness gate before register_rows, `is_active` self-report);
  `player_perspective` (PersistMode::Full, legacy-value clamps, gameplay latch).
- Task 01 already provides the arm source: `song_rate::runtime::set_desired_percent
  (side, percent)` + the permanent scene callback that classifies with per-side
  desired percents (identity when 100/invalid). The mod does NOT register a scene
  callback and does NOT arm — it only normalizes option values into the atomics.
- Label PNGs: `scripts/gen_option_labels.py` `LABELS` tuple → 176x16 PNG at
  `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/seop_item_<id>.png`
  (committed); framework auto-registers by id at register_option.
- Mod trait: `id/name/description/required_signatures/init/enable/disable/is_active`
  (self-disable ⇒ is_active false so the registry/menu shows OFF, mod_trait.rs:81-92).
  Mod instance list at `lib.rs:108-133`; song_rate runtime init (5b) precedes mod
  registration (step 7)/enable (step 8).

## Interpretation & assumptions (auto mode)

- `set_option_available` = plain `available: bool` on `RegisteredOption` (mutated
  under the STATE mutex), filtered at the builder-hook snapshot (silent
  non-injection — matches the requested semantics; the ShowWhen dynamic mask path
  is deliberately NOT reused because it can hide rows on an open form).
- `row_injection_available()` = `is_available() && rows::is_scalar_ready() &&
  builder_hook::is_ready() && FILTER_READY` (new latch of `filter_hook::init`'s
  result). "Required assets" beyond the scalar donor pieces are non-fatal by
  framework design (missing label PNG degrades to a blank label) and are not part
  of the strict predicate.
- The mod's runtime-integration readiness = new
  `song_rate::runtime::integration_ready()` (BOOT_READY && live
  `score_guard::is_full_sanitization_available()`), checked at enable.
- Load normalization: pure `lifecycle::snap_rate_percent(i32) -> i32` (clamp
  25..=175, then half-up snap to the nearest multiple of 5) — host-tested; the
  mod's `load_transform` and `on_change` both delegate (defense in depth; the
  native row itself can only produce domain values).
- Disable semantics: `set_option_available(false)` + desired percents reset to 100
  (future policy disabled); the active generation/current attempt is untouched
  (lifecycle owns it; desired atomics are read only at the next scene-26 arm).
  Mid-song edit isolation is structural for the same reason.
- Host-test scope: pure normalization (lifecycle_tests) + registry availability
  semantics (new `custom_options/availability_tests.rs` via a harness mini-module
  of api+registry only). Atlas-flush ordering, form-rebuild visibility, and P1/P2
  live isolation are cargo-check + task-04 cabinet legs (consistent with every
  prior mod — mods/* are not harness-compiled).

## Files to touch

- `src/services/song_rate/lifecycle.rs` (+ lifecycle_tests.rs): `snap_rate_percent`
- `src/services/custom_options/registry.rs`: `available` flag + setter + tests hook
- `src/services/custom_options/mod.rs`: `set_option_available`, `row_injection_available`,
  FILTER_READY latch
- `src/services/custom_options/builder_hook.rs`: snapshot availability filter
- `src/services/custom_options/availability_tests.rs` (new, host)
- `src/services/song_rate/runtime.rs`: `integration_ready()`
- `src/mods/song_playback_speed.rs` (new), `src/mods/mod.rs`, `src/lib.rs`
- `scripts/gen_option_labels.py` + generated `seop_item_song_speed.png`
- `mod-config.json` (mods entry + row_order example)
- `scripts/validate_song_playback_speed.sh` (file list + harness mini-module)
