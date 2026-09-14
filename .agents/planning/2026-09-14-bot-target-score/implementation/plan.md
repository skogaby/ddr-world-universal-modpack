# Implementation Plan — Multiplayer Bot "Target Score" tier

Status: Approved 2026-09-14

Design: `design/detailed-design.md` (same directory). Each step leaves the DLL
buildable (`cargo check --target x86_64-pc-windows-msvc`) and its own tests
green; the maintainer commits manually.

- [x] Step 1: Options-framework additions (`PersistMode::Local`, `ScalarFormat::Labeled`)
- [x] Step 2: GhostActor signature + derivation + 4-build sweep
- [x] Step 3: Pure layer — ghost sampler, planner N.G. extensions, mode plumbing
- [x] Step 4: Engine wiring — ghost source, filler binding + fallback, rows, plate, diagnostics
- [x] Step 5: Docs, generated option strings, readiness gates

---

Step 1: Options-framework additions

- Objective: give the bot rows a local-only persistence mode and a text
  formatter for the enum-like level row, with no behaviour change for existing
  options.
- Guidance: `src/services/custom_options/api.rs` — add `PersistMode::Local`
  (+ doc row in the matrix table) and `ScalarFormat::Labeled { prefix,
  terminal_value, terminal_label }` (handled in `format_scalar_value`); add
  `LoadSource { Network, JsonPrime }` and thread it through
  `custom_options::resolve_from_load` (`src/services/custom_options/mod.rs`)
  with the split gate; update the two call sites in
  `src/services/custom_options_persistence.rs`. Grep for every `match` on
  `PersistMode`/`ScalarFormat` (the compiler enforces exhaustiveness).
- Tests: `persist_matrix_tests.rs` (exact matrix + updated invariants + the
  `Local` save/load/JSON behaviours), `scalar_format_tests.rs` / `api.rs`
  (`Labeled` bytes + UTF-8), existing suites unchanged.
- Integration: none yet — the bot still registers with `Full` + `Integer`.
- Demo: `cargo test` green; a `Local` fixture option never appears in the
  save snapshot and is applied only by a `JsonPrime` load.

Step 2: GhostActor signature + derivation

- Objective: publish `gpa_ghost_actor_off` per build and resolve
  `ghost_actor_vtable`, fail-closed.
- Guidance: `src/core/signatures.rs` — `gpa_ghost_actor_probe` definition (with
  the wait-site disassembly in its comment, per-build addresses 20260825
  `0x18005d186` / 20260224 `0x180058dc6` / 20250805 `0x180059d86`), the
  derivation (`derive_ghost_actor_probe`: disp32 at +3, CALL at +12 →
  callee prologue byte-run check within 0x30 bytes, `publish_value`), the
  `gpa_ghost_actor_off()` accessor, and `.?AVGhostActor@dance@sequence@@` in
  the RTTI list. Wire the derivation into `resolve_derived`.
- Tests: `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` —
  `[+] gpa_ghost_actor_probe` on all four builds, derived value `0x1F0` on
  20250805/20260224 and `0x1F8` on 20260721/20260825; `shape_diff.py` review of
  the new AOB's window.
- Integration: consumed by Step 4's `ghost_source::init`.
- Demo: sweep output + the boot log line `gpa_ghost_actor_off (derived) = 0x1F8`.

Step 3: Pure layer

- Objective: every rule of the replay is decided in dependency-free code with
  host tests.
- Guidance: new `src/mods/multiplayer_bot/ghost.rs` (`tap_band`, `decide_tap`,
  `histogram`, `expected_grade`, grade constants); `skill.rs` (`Form::next_side`
  factored out of `next_lean`, `Rng::below`); `planner.rs`
  (`SongState::with_ghost`, ghost-sourced `resolve`, `drop_hold` freeze N.G.
  with the bounded tail lookup, shock N.G. press, `repro_miss`);
  `eligibility.rs` (`TARGET_VALUE`, `clamp_value`, `BotMode`, `Plan.mode`);
  `session.rs` (`format_bot_name(BotMode)`). Mount `ghost.rs` in
  `tools/bot_sim/src/bot/mod.rs`; adjust the simulator's call sites for the
  `Plan.mode` / `SongState` signature changes (level runs unchanged).
- Tests: the suites listed in the design's Testing Strategy for `ghost.rs`,
  `planner.rs`, `eligibility.rs`, `session.rs`, plus the existing `skill.rs`
  side-chain statistics tests (unchanged expectations).
- Integration: `filler.rs`/`impersonation.rs` compile against the new
  signatures with `BotMode::Level` only (Target still unreachable).
- Demo: `./scripts/validate_multiplayer_bot.sh` green; `scripts/bot_sim.sh`
  level reports unchanged.

Step 4: Engine wiring

- Objective: the feature end to end on the cabinet.
- Guidance: new `src/mods/multiplayer_bot/ghost_source.rs` (init from
  signatures, `read_human_ghost` with every probe/gate from the design);
  `filler.rs` (`start_song(mode)`, ghost bind on the Results rebuild, S-Marv
  floor via the new `s_marvelous::state::armed_window`, Level-10 fallback +
  once-per-song WARN + toast, `SongSummary` fields); `impersonation.rs`
  (`Active.mode`, plate, seed, flip/restore INFO); `mod.rs` (row spec 1..=11
  `Labeled` + `Local` on both rows, `mode(side)`, `clamp_value` load transform,
  `ghost_source::init` in `init`, disable path unchanged); register the new
  module in `mod.rs`.
- Tests: no host harness for engine code — cabinet checks 1–5 of the design's
  Testing Strategy; `cargo check` + `./build.sh` clean.
- Integration: completes the chain Step 1 → Step 3 opened.
- Demo: pick `Target Score`, play against your PB, watch the bot finish on
  exactly the target's points with the log's `mode=target … repro_miss=0`.

Step 5: Docs + readiness

- Objective: leave the repo consistent for the next agent and the release.
- Guidance: AGENTS.md multiplayer-bot row (Target tier, `Local` persistence,
  `Labeled` format, the `+0x1F0/+0x1F8` fork note in the build-dependent-layouts
  row); `docs/multiplayer_bot_research.md` addendum (GhostActor wait, per-build
  offset, identity gate, ghost alphabet/alignment); README option text;
  `scripts/option_strings.py` description for `bot_opponent_level` and a
  regeneration of the affected label PNGs if the generator runs cleanly on
  this host (otherwise note it for the maintainer).
- Tests: readiness gates — `cargo check`, `cargo fmt` (whole crate),
  `./build.sh`, `./scripts/validate_signatures.sh ~/Desktop/ddr_modules`,
  `./scripts/validate_multiplayer_bot.sh`, `cargo test`;
  `git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` adds no new hits.
- Demo: a release build + the docs describing exactly what shipped.
