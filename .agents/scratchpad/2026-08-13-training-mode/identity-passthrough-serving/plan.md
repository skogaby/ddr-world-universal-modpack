# Plan — task-01 identity passthrough serving

Status: Approved 2026-08-13 (via approved upstream plan/design — code-assist
auto mode; see context.md approval chain)

## Implementation approach

### 1. `src/core/xact/virtual_bank.rs`

- New `pub fn plan_identity_bank(source: &xwb::SongBank<'_>) ->
  Result<VirtualBankLayout, PlanError>`: BOTH entries via
  `passthrough_plan` (stock header values), pre-data via the same
  `stream_pre_data` canonical emission. Never touches
  `plan_entry`/`rate::target_for_percent` — no block quantization anywhere.
- `passthrough_plan` stays private; the doc comment gains the identity-arm
  citation.

### 2. `src/services/song_rate/binding.rs`

- `pub enum ServeMode { Stretch, IdentityPassthrough }` + accessor.
- `Binding` fields: `serve_mode`, `mapping: AtomicU64` (packed
  shift:u32 hi | lead:u32 lo, block units), `mapping_epoch: AtomicU64`,
  `main_source_offset: usize` + `main_source_len: u64` (stock main-entry
  data range in the source copy — the identity serving base).
- Constructors: existing `new`/`with_ring_capacity` stay Stretch and
  delegate to a private mode-aware builder; new
  `pub fn new_identity_passthrough(file_id, generation, layout, source)`
  — rate = IDENTITY, minimal ring allocation, validates the layout's main
  entry is passthrough-shaped against the parsed source (stock data_len).
- `pub fn set_content_mapping(&self, shift_blocks: u64, lead_blocks: u64)
  -> bool`: u32-fit validation (else false, unchanged), packed store +
  epoch bump. `pub fn content_mapping(&self) -> (u64, u64)`;
  `pub(crate) fn mapping_epoch()`; `pub fn ring_rewind_count()` diagnostic.
- `check_spans`: main-entry ring checks apply only in Stretch mode
  (IdentityPassthrough main spans are always available, like the side
  entry).
- `copy_spans`: identity main spans go through a new
  `copy_mapped_main(within, out, len, shift_bytes, lead_bytes)` — walks
  lead/content/tail sub-regions: silent-block tiling (phase `within %
  align`), verbatim source copy at `within − lead + shift`, silent tail.
  Mapping loaded ONCE per serve/complete call (consistent view).
- `pub(crate) fn silent_block(&self, entry)` for the generator.

### 3. `src/services/song_rate/generator.rs`

- `GeneratorError::IdentityPassthrough`; `GeneratorCore::new` refuses
  identity-mode bindings (no producer by design).
- `GeneratorCore` snapshots `mapping_epoch` + byte-domain
  `{shift_bytes, lead_bytes}`. `step()` checks the epoch right after the
  stop token: on change → re-snapshot, drop the main feed, `ring_rewind
  (main_data_start)`, cursor = start (production restarts at output 0).
- `produce_chunk()` becomes mapped-region aware: lead region emits silent
  blocks; content region drives the feed (lazily positioned via
  `Feed::new`/`Feed::positioned_at` at `within − lead + shift`); tail
  region emits silent blocks to `main_data_end`.
- `rewind_to(target)`: block-align, set cursor + `ring_rewind`, drop the
  feed (produce_chunk repositions under the mapping) — the same
  deterministic bytes as today at mapping {0,0}.

## Test scenarios (all host, `cargo test`)

| # | Criterion | Test |
|---|---|---|
| 1 | Identity byte-identity at {0,0} | `core/xact/tests.rs`: `plan_identity_bank` advertises stock values for both entries (both orders); `binding_tests.rs`: serve the full virtual file (0x1000 header read + packet walk + EOF read) from an IdentityPassthrough binding — byte-equal to the fixture, all serves synchronous |
| 2 | Shifted serving with silent lead | `binding_tests.rs`: mapping {shift=4 blocks, lead=3 blocks} — main region equals: 3 silent blocks ++ source main data from block 4 ++ silent tiling to the declared end; side entry + pre-data untouched |
| 3 | Stretch-mode mapping change | `generator_tests.rs`: produce under {0,0}, `set_content_mapping(s,l)` → `ring_rewind_count` bumps, re-served file equals reference (lead silence ++ oracle stretched main from s·align ++ silent tail) |
| 4 | No producer for identity | `binding_tests.rs`: full serve completes with `deferral_count == 0`, never Pending; `generator_tests.rs`: `GeneratorCore::new` refuses the identity binding |
| 5 | Existing suite green | `cargo test` — zero regressions |

Failure-first: tests 1–4 are written against the new API surface and cannot
pass before it exists (compile failure = the failing state for new-API
tests; test 3's behavioral assertions fail against a stub that stores but
does not remap).

## Risks

- Byte-identity of pre-data vs fixture depends on the canonical emitter
  matching `build_bank_bytes` — already pinned by
  `virtual_bank_pre_data_matches_streaming_serializer`.
- Serve-path additions must stay allocation-free: `copy_mapped_main` is
  pointer arithmetic + `copy_nonoverlapping` + silent-block tiling only.
- Generator remap must not disturb the mapping-{0,0} paths: at zero mapping
  every new branch degenerates to the existing arithmetic (content_pos ==
  within), pinned by the untouched existing tests.
