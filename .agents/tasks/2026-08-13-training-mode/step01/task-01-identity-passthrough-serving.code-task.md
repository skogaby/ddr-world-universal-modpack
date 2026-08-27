# Task: Identity passthrough serving with `{shift, lead}` content mapping

## Description
Add the pure-layer half of the training-mode audio path to the song-rate
engine: an identity-percent virtual-bank plan whose MAIN entry is a
verbatim passthrough (stock header values), a new `IdentityPassthrough`
serve mode, and a `{shift_blocks, lead_blocks}` content mapping honored by
main-entry serving in both serve modes. This is the deliberately
front-loaded risk of the feature: if shifted serving misbehaves, the
design is revisited before anything else exists.

## Background
Training-mode seeks work by stopping the song cue and replaying it while
the virtual bank serves content from a different offset (the engine parses
the bank header exactly once, so the layout is fixed and only the content
mapping may change — see the design's §4.5 and the research §5.1). At
100 % speed there is currently no binding at all; this task gives the
engine an identity mode where the main entry is served verbatim from the
resident source copy (no WSOLA, no producer thread), with the mapping:
virtual block `v < lead_blocks` ⇒ pre-encoded silent block; else source
block `v − lead_blocks + shift_blocks`; silent tiling past the source end.
CRITICAL: the identity plan must use `passthrough_plan` for the main entry
— `plan_entry(…, 100)` block-quantizes the duration and is NOT
stock-shaped (research §5.3).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.5, §5, §6)

**Additional References (if relevant to this task):**
- docs/training_mode_research.md §5 (shifted serving design, header-parsed-once constraint, silent-block coverage)
- docs/xact_streaming_research.md (read-pattern contract the serving must honor)
- src/services/song_rate/binding.rs + binding_tests.rs (the serve dispatch, side-entry verbatim arm — the structural model for identity passthrough)
- src/core/xact/virtual_bank.rs (plan_virtual_bank / passthrough_plan)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `virtual_bank`: an identity whole-bank plan (`plan_virtual_bank`
   variant or parameter) where the MAIN entry uses `passthrough_plan` —
   header values byte-equal to stock; preview entry unchanged.
2. `binding`: `serve_mode: IdentityPassthrough | Stretch` and the mapping
   pair `shift_blocks: AtomicU64` / `lead_blocks: AtomicU64` (default 0/0),
   block-aligned by construction.
3. Main-entry serving honors the mapping in BOTH modes:
   - IdentityPassthrough: allocation-free copies from the resident source
     (model: the existing side-entry always-available spans), lead region
     served from the per-entry pre-encoded silent block, silent tiling
     past `source_end`.
   - Stretch: mapping feeds the generator's reposition path; a mapping
     change bumps the ring seqlock (`ring_rewind`) and production restarts
     at output 0 under the new mapping.
4. No producer thread is spawned for IdentityPassthrough bindings.
5. Serve-path code stays allocation-free and log-free (detour context —
   existing convention in `binding.rs`).
6. Preserve the identity pin: nothing in this task changes behavior for
   unbound (ordinary) 100 % plays — new code is reachable only through a
   binding that Task 02 will make armable.

## Dependencies
- None (pure layer; host-testable). First task of Step 1.

## Implementation Approach
1. Add the identity plan path in `src/core/xact/virtual_bank.rs`.
2. Add serve mode + mapping to `Binding` construction/state in
   `src/services/song_rate/binding.rs`; thread the mapping through
   `check_spans`/`copy_spans` (main entry only; side entry untouched).
3. Generator: consume the mapping on (re)positioning; reuse the existing
   behind-window/rewind machinery for mapping changes.
4. Host tests alongside the code (same files/patterns as existing suites).

## Acceptance Criteria
1. **Identity byte-identity at zero mapping**
   - Given an IdentityPassthrough binding over a synthetic song bank with mapping `{0, 0}`
   - When the full virtual file is served span by span (header read shapes included)
   - Then every byte equals the stock bank byte-for-byte, and the advertised header values equal stock
2. **Shifted serving with silent lead**
   - Given mapping `{shift = B(T), lead = L}` on an identity binding
   - When the main entry is served
   - Then blocks `[0, L)` are the entry-format silent block, blocks `[L, …)` equal source blocks from `B(T)`, and blocks past `source_end − B(T) + L` are silent
3. **Stretch-mode mapping change**
   - Given a Stretch binding mid-production
   - When the mapping changes
   - Then the ring seqlock bumps, previously served spans are invalidated, and re-served output equals a reference produced from the new mapping
4. **No producer for identity**
   - Given an IdentityPassthrough binding
   - When it is constructed and served
   - Then no generator thread exists and serving never defers for production (deferral count stays 0 in tests)
5. **Host suite green**
   - Given the existing song_rate/xact host tests
   - When the suite runs
   - Then all pre-existing tests still pass (the identity pin and stretch paths are unregressed)

## Metadata
- **Complexity**: High
- **Labels**: song-rate, audio, virtual-bank, host-tested
- **Required Skills**: Rust, the repo's song_rate architecture, XWB/ADPCM formats
- **Generated By**: code-task-generator 2026-08-13
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 1: Identity arm + shifted serving in song_rate
