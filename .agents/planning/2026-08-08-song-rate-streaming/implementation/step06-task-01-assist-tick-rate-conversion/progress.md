# Progress — Step 6 task-01: Assist Tick Rate Conversion + 1200 s Capacity

Updated: 2026-08-11
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] 1. Tests first (RED): `tick_domain_tests.rs` vectors (bit-identity grid,
      exact 25/50/75/125/175 conversions, restart skips, proceeds-at-rate,
      clamp/fail-soft pins)
- [x] 2. `tick_domain.rs` pure module (GREEN in the fast harness)
- [x] 3. `assist_tick.rs` rewire: snapshot latch, Action::Anchor snapshot,
      Commit/Rewind skip via tick_domain, spawn_synthesis conversion, scaffold
      gate REMOVED (Action::RateGated + call site + log)
- [x] 4. `TICK_CAPACITY_MS` → 1_200_000 + comment corrections
- [x] 5. Validator edits: file-presence list (+2 files); se-bank check 8
      re-derived from the constant
- [x] 6. Full gate set green (all five, logs in `logs/`); record closed

## What landed

- **`src/services/song_rate/tick_domain.rs` (new, pure, no logging):**
  `tick_track_positions(times, jt_signed, sound_offset, m0, &RateSnapshot)`
  and `restart_skip_ms(mc, m0, &RateSnapshot)`. Identity/uncommitted
  (`!is_non_identity_commit()`) reproduces the mod's legacy arithmetic
  LITERALLY (same saturating call order — the 100 % bit-identity pin is
  structural, not arithmetic). Committed non-identity applies the KEEPER
  formula: `wall(t) = clamp_i32(content_to_wall_ms(t + jt − m0) − so)`,
  `skip = clamp_i32(content_to_wall_ms(mc − m0))` — JT converts (the Q31 stub
  outputs content-domain counts and the game applies `timing_music` against
  that clock), `sound_offset` subtracted AFTER conversion (wall, unscaled).
  A `content_to_wall_ms` Err (can't-happen behind the seqlock) falls back to
  the identity arithmetic — deterministic, never panics on the judge path.
- **`src/mods/assist_tick.rs`:** `SongState.rate: RateSnapshot` latched in
  the `AwaitAnchor` arm (strictly after any loader-thread commit; the live
  publication resets at gameplay exit so commit/rewind consume THIS copy);
  `Action::Anchor` carries the snapshot into `spawn_synthesis`, whose
  shift/position computation is now one `tick_domain::tick_track_positions`
  call (mod-owned `JUDGMENT_TIMING_SIGN` applied at the call site — same
  product value, bit-identity preserved); Commit/Rewind skips go through
  `tick_domain::restart_skip_ms`. **Scaffold gate REMOVED**: `Action::RateGated`
  variant, its match arm + log line, and the `is_non_identity_commit()` gate
  call are gone (grep-verified: zero `RateGated` references anywhere).
  Synthesis INFO gained `rate={}%` for Step 7's live listening evidence.
  Header/FR-3/FR-8 comments updated to the rate-aware truth.
- **`RateSnapshot::is_non_identity_commit()` KEPT** (task req 2's condition:
  still referenced — it is now tick_domain's path selector); its doc comment
  and its clock_patch test retitled to the new role
  (`non_identity_commit_predicate_selects_the_tick_conversion_path`).
- **`src/services/se_bank_synth/containers.rs`:** `TICK_CAPACITY_MS`
  300_000 → **1_200_000** (D15: 1200 s wall keeps 300 s of chart content at
  25 %); size comments corrected (sample segment ~28.9 MB, mix buffer
  ~106 MB); lazy registration and the truncation WARN contract untouched
  (`ensure_tick_bank_registered` and the WARN text unchanged — the printed
  seconds derive from the constant).
- **Validators:** song-rate file-presence list gained
  `tick_domain.rs tick_domain_tests.rs`; se-bank check 8's clip/drop vector
  re-derived from `ours::TICK_CAPACITY_MS` (was hardcoded 299_999/300_000/
  400_000 — would have inverted at the new capacity), same
  mixed=3/clipped=1/dropped=2 shape, now doubling as the 1200 s truncation
  boundary pin (AC-4). No backticks/dollars introduced (unquoted heredoc).

## TDD cycles

1. `tick_domain_tests.rs` written first + module declarations → RED
   (E0583 file not found for module `tick_domain`). Module implemented →
   5/5 green, harness 135/135 (was 130).
2. Mod rewire + capacity + validator edits → windows check 0 warnings,
   harness still 135/135.

## Acceptance criteria → evidence

1. **Exact conversion at every rate:** `positions_convert_exactly_at_each_supported_rate`
   + `restart_skips_convert_exactly_at_each_supported_rate` — 25/50/75/125/175
   via the production `target_for_percent` path with a non-block-clean
   9_876_543-frame source (fixture honesty), checked against an independent
   i128 half-away oracle AND literal hand-pinned vectors (computed outside
   the crate); sound_offset pinned unscaled (subtracted post-conversion).
2. **100 % bit-identical:** `identity_and_uncommitted_paths_are_bit_identical_to_the_legacy_arithmetic`
   — grid incl. i32 extremes over IDENTITY, committed-100, and
   uncommitted-75 snapshots, positions AND skips equal the literal legacy
   formula.
3. **Scaffold gate gone:** `committed_rate_synthesis_proceeds_with_converted_positions`
   (committed 50 % yields the full converted list) + structural removal
   (zero `RateGated` symbols repo-wide; the AwaitAnchor arm always anchors).
4. **1200 s capacity:** se-bank validator `duration covers TICK_CAPACITY_MS`
   (symbolic) + the re-derived boundary check (cap−1 mixes; cap and beyond
   drop; one WARN contract unchanged) — both PASS at 1_200_000.
5. **Tree green:** gates below.

## Gates (all green, logs in `logs/`)

1. `./scripts/validate_song_playback_speed.sh` — validation passed; cargo-test
   phase **166/166** (was 161; +5 tick_domain) in 7.36 s
2. `./scripts/validate_se_bank_synth.sh` — ALL CHECKS PASSED (incl. the new
   capacity-boundary check)
3. `cargo check --target x86_64-pc-windows-msvc` — 0 warnings
4. `cargo fmt --check` — clean (whole-crate fmt run first)
5. `./build.sh` — release DLL OK in 44.7 s

## Deviations

- **Synthesis INFO log gained `rate={}%`** (not in the task text): one field
  on the existing per-song line, added for Step 7's live tick-alignment
  matrix — the log is the only evidence channel that the conversion applied.
  Conservative, no new log lines.
- **clock_patch predicate doc + test retitled** to the conversion-selector
  role (the task removed the gate but kept the predicate; leaving "the gate's
  predicate" docs would have described a deleted consumer).
- Everything else landed exactly as planned; no design-contradicting
  deviations.

## Notes

- Step 7's docs pass owes: AGENTS.md "Assist tick" row (still says 300 s /
  `TICK_CAPACITY_MS` = 300 s and describes the per-tick formula as
  content-only) and the live 50 %/100 % alignment listening check.
- Validator runtime grew slightly (the se-bank harness now mixes/encodes
  1200 s buffers) — host-only cost, accepted.
