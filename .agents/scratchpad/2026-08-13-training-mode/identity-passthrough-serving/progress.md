# Progress — task-01 identity passthrough serving

## Checklist

- [x] Baseline: harness `cargo test` green before any change (182/182)
- [x] Tests: identity plan + plan_entry(100) hazard pin (core/xact/tests.rs)
- [x] Tests: identity serving byte-identity + shifted + mapping-rejection (binding_tests.rs)
- [x] Tests: stretch mapping change + generator identity refusal (generator_tests.rs)
- [x] Impl: `plan_identity_bank` (virtual_bank.rs)
- [x] Impl: ServeMode + packed mapping + identity ctor + mode-aware serving (binding.rs)
- [x] Impl: generator mapping snapshot + remap restart + identity refusal (generator.rs)
- [x] Validate: host suite green — **189 passed / 0 failed** (was 182; +7 new)
- [x] Validate: `cargo check --target x86_64-pc-windows-msvc` clean, `cargo fmt` applied, `./build.sh` release DLL built
- [x] Close (no commit — see below)

## TDD cycles

1. Wrote all 7 tests first against the missing API → confirmed the failing
   state (13 compile errors naming exactly the new surface: `plan_identity_bank`,
   `new_identity_passthrough`, `set_content_mapping`, `content_mapping`,
   `ring_rewind_count`, generator refusal).
2. Implemented `plan_identity_bank` (both entries `passthrough_plan`, canonical
   `stream_pre_data`), the `ServeMode` enum, packed mapping + epoch/applied
   handshake, `new_identity_passthrough` (minimal ring, passthrough-shape
   validation → `BindingError::IdentityLayoutMismatch`), mode-aware
   `check_spans` (identity main spans always available; Stretch spans defer
   while a mapping is pending), `copy_mapped_main` (lead/content/tail walk,
   allocation-free), generator `snapshot_mapping` + remap restart at output 0
   (`ring_rewind` = the seqlock bump), mapped `produce_chunk` with
   `ensure_feed_at`/`emit_silence`, lazy `rewind_to`.
3. Full suite green on the first post-implementation run: 189/189.

## Test environment note (important for sibling tasks)

Plain `cargo test` in the repo root does NOT work on this host — `retour` is
x86-only and fails to compile for aarch64-apple-darwin. Host tests run through
a temp-dir `#[path]` harness mirroring `scripts/validate_song_playback_speed.sh`'s
mounts (xact + memory_patch/hook_transaction + scenes + movie_policy +
score_guard + song_rate + custom_options kernel, each with its test modules).
This session's harness: `/var/folders/31/yq10yrk557l1q0wyb1nx4vg40000gp/T/opencode/ddr-host-harness`
(recreate from the validator's `main.rs` heredoc if lost; deps: once_cell).

## Deviations

- Mapping stored as ONE packed AtomicU64 (shift:u32 hi | lead:u32 lo) plus
  `mapping_epoch`/`mapping_applied`, instead of the design data-model's two
  bare AtomicU64 fields — a reader can never observe a torn shift/lead pair,
  and the epoch pair closes the set→producer-pickup staleness window by
  deferring Stretch main-entry reads until the producer acknowledges (reuses
  the existing pending-slot machinery; the ring seqlock is still bumped by
  the producer's `ring_rewind`, per the AGENTS gotcha). Field shapes free,
  behavior binding (recorded precedent from the Ring `base` cursor).
- Identity bindings allocate a 4 KiB dummy ring instead of 16 MiB (the ring
  is never read/written in that mode).
- `GeneratorCore::new` refuses IdentityPassthrough bindings
  (`GeneratorError::IdentityPassthrough`) — defensive; task-02's prepare path
  simply never spawns for them.

## Files changed

- `src/core/xact/virtual_bank.rs` — `plan_identity_bank` + doc updates
- `src/services/song_rate/binding.rs` — ServeMode, mapping API, identity
  ctor, mode-aware serving, `copy_mapped_main`, diagnostics accessors
- `src/services/song_rate/generator.rs` — identity refusal, mapping snapshot,
  remap restart, mapped produce/rewind
- Tests: `src/core/xact/tests.rs` (+2), `src/services/song_rate/binding_tests.rs`
  (+3 + `serve_identity_file` helper), `src/services/song_rate/generator_tests.rs`
  (+2 + `make_identity_binding`/`remap_main_entry` shared helpers)

## Commit

**Commit intentionally not made** — the handoff (and repo convention) says the
maintainer manages `git commit` themselves; do not commit/push unless asked.
All gates green as of close.

Status: Complete (uncommitted — maintainer handles git)
