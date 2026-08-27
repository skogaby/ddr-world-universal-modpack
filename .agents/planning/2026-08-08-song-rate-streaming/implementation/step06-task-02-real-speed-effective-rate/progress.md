# Progress — Step 6 task-02: Real Speed × Effective Rate

Updated: 2026-08-11
Status: Complete (uncommitted — maintainer commits personally)

## Checklist

- [x] 1. Tests first (RED): `real_speed_tests.rs` (derivation pins at
      25/50/75/125/175, clamp bounds, identity/uncommitted ⇒ None,
      fail-soft core/target/ratio guards, toggle independence)
- [x] 2. `real_speed.rs` pure `rate_adjusted_multiplier` (GREEN in the
      fast harness)
- [x] 3. Windows glue: init/activate/deactivate, per-side once-latch judge
      subscriber, scene reset, guarded Option walk, actor-cluster +
      Option+0x10 writes
- [x] 4. `song_playback_speed.rs` wiring (init/enable/disable)
- [x] 5. mod.rs declarations + validator file-presence list (+2 files)
- [x] 6. Full gate set green (all five, logs in `logs/`); record closed

## What landed

- **`src/services/song_rate/real_speed.rs` (new):** pure
  `rate_adjusted_multiplier(target, core_bpm, &RateSnapshot) -> Option<i32>`
  — `None` at identity/uncommitted (no write; both fix-toggle states keep
  today's behavior bit-identically by construction) and for untrusted
  inputs (target ∉ 1..=100_000, core ∉ (0, 10_000], zero ratio); otherwise
  the native derivation with the effective divisor:
  `clamp(trunc(target·100 / (core × source/output)), 25, 800)` —
  byte-faithful to `SetScrollSpeed`'s `(int)((double)(target·100)/divisor)`
  + the image-read 25/800 clamp; the fix toggle is structurally absent.
  `#[cfg(windows)]` glue (harness-safe): `init` resolves
  `player_option_table`; `activate`/`deactivate` register/unregister a
  judge_hook pre-subscriber + a GAMEPLAY-entry scene reset; the subscriber
  consumes a per-side once-per-song latch at that side's FIRST judge
  dispatch (strictly post-commit, post-actor-construction — the assist-tick
  anchor guarantee), walks the guarded Option chain (`*(table+side·8)` →
  holder → ctx → Option = ctx+0xE0), skips fixed-multiplier sides
  (type@+0x8 ≠ 0), and on `Some(m)` writes the ACTOR multiplier cluster
  (`+0x29C` int; `+0x290`/`+0x294` f32 m/100 — the fields the actor copies
  into the arrow/spot renderers EVERY frame) plus `Option+0x10` for display
  consistency. One INFO per applied side; one latched WARN per session on
  an unreadable chain; every failure leg = no write = stock (fail-open).
- **`src/mods/song_playback_speed.rs`:** owns the lifecycle —
  `real_speed::init` at mod init, `activate()` at enable (after the row
  registers), `deactivate()` at disable. Deliberately NOT the Real Speed
  Fix mod: req 33's "regardless of the toggle" forbids gating on it.
- **Validator:** file-presence list +2 (`real_speed.rs`,
  `real_speed_tests.rs`); no check changes needed.

## TDD cycles

1. `real_speed_tests.rs` + mod.rs declarations → RED (E0583 file not found
   for module `real_speed`). Pure fn implemented → 5/5 green (a
   target-domain guard added mid-cycle moved the huge-clamp vector from
   1 000 000 to the in-domain 10 000).
2. Glue + mod wiring + validator line → windows check 0 warnings, harness
   140/140.

## Acceptance criteria → evidence

1. **Non-identity derives from the effective tempo, both toggle states:**
   `non_identity_derivation_matches_the_native_formula_at_each_rate`
   (25/50/75/125/175 via `target_for_percent` on the non-block-clean
   9_876_543-frame fixture, core 148.5, targets 100/400/600 — literal
   IEEE-f64 pins incl. the 800-clamp interaction) +
   `the_fix_toggle_is_structurally_absent_from_the_rate_path` (purity pin:
   the toggle never enters the math — that IS "both states yield the
   Core × rate derivation").
2. **Identity keeps the toggle's meaning:**
   `identity_and_uncommitted_snapshots_derive_nothing` — IDENTITY,
   committed-100, uncommitted-75 all `None` ⇒ no write anywhere ⇒ each
   toggle state's stock output bit-identical (the glue's identity leg
   returns before any read beyond the snapshot).
3. **Tree green:** gates below.

## Gates (all green, logs in `logs/`)

1. `./scripts/validate_song_playback_speed.sh` — validation passed;
   cargo-test phase **171/171** (was 166; +5 real_speed) in 7.37 s
2. `./scripts/validate_se_bank_synth.sh` — ALL CHECKS PASSED
3. `cargo check --target x86_64-pc-windows-msvc` — 0 warnings
4. `cargo fmt --check` — clean (whole-crate fmt run first)
5. `./build.sh` — release DLL OK in 45.96 s

## Deviations

- **Task-file premise corrected (headline; plan.md + context.md):**
  `real_speed_fix.rs` holds no Rust derivation (it is the R24/R25/R26 byte
  patches into the native setter). Implemented the KEEPER design's
  raw-recompute mechanism instead — owned by the song-rate feature, no
  competing patch, acceptance criteria unchanged and all satisfied. Fresh
  RE (Ghidra, 20260721 + 20260616 byte spot-check; full chain in
  context.md) resolved the predecessor design's flagged unknowns: Option
  speed-type/target/core offsets confirmed; **the GamePlayActor latches the
  multiplier at construction into `+0x290/+0x294/+0x29C` and re-writes the
  renderers per frame** — so the actor cluster (not Option+0x10 alone) is
  the effective write target, and the first judge dispatch is the correct
  (and assist-tick-proven) timing window.
- **Fixed-multiplier sides (speed type 1) untouched** — per the KEEPER text
  ("keeps the player's selected target unchanged and derives its normalized
  multiplier"); there is no BPM derivation in that mode.
- **Fail-soft availability** (old design made a failed layout derivation
  block non-100 % entirely; the streaming design carries no such readiness
  leg): missing table/chain ⇒ Real Speed stays stock at rates, one WARN,
  feature otherwise unaffected. Conservative direction: a skipped write is
  stock behavior.
- Old-design write-site wording ("at gameplay entry… while scene-manager
  callback iteration is locked") replaced by the first-judge-dispatch site:
  scene-entry fires BEFORE the gameplay scene constructs (stale chart BPMs;
  `SetBPMs` → `SetScrollSpeed` re-derives at chart load and the actor
  latches after that), so an entry-time write would be clobbered/inert.

## Notes

- Live validation of the write path lands in Step 7's matrix (same status
  as tick alignment): oracle = at 50 %, a Real-Speed-mode player sees the
  same on-screen arrow velocity as at 100 % (multiplier doubles), both fix
  states; at 100 % everything literally stock.
- Step 7 docs owe: the new module + the actor multiplier-cluster RE (this
  record + context.md are the source), AGENTS.md Real Speed row if one is
  added.
