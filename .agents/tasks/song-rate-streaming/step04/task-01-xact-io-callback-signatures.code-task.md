# Task: XACT File-IO Callback Signatures and Runtime Address Derivations

## Description

Add the streaming rate engine's boot-time address resolution to
`src/core/signatures.rs`: ONE new AOB on the audio-manager constructor's
callback-registration region that RIP-decodes BOTH XACT file-IO callback
targets (readFile + getOverlappedResult), plus the derivations the runtime
path needs — the file-table global (path/buffer/size rows for the binding's
source view and dance-path check) and the handle→file_id resolution inputs
the read detour requires. Cross-verify everything in Ghidra on all four
supported builds and record the evidence in
`docs/xact_streaming_research.md`'s cross-version section. Everything is
inert until tasks 03–04 consume it — the tree stays behaviorally identical.

## Background

The design (req 9) detours gamemdx's registered XACT file-IO callbacks as a
pair; on build 2026-07-21 they are `FUN_1801aa250` (readFile) and
`FUN_1801aa350` (getOverlappedResult). Both are resolved from one anchor:
the manager constructor (`FUN_1801aab60`) builds `XACT_RUNTIME_PARAMETERS`
with `lookAheadTime = 0xFA` followed by three `LEA RAX,[rip+disp32]` /
`MOV [RBP+disp8], RAX` pairs (notification, readFile, getOverlappedResult) —
the `0xFA` immediate plus the three-pair shape is the AOB anchor; the second
and third LEAs RIP-decode to the two detour targets. Wildcard the LEA
disp32s and the frame disp8s so the pattern survives across builds
(`docs/xact_streaming_research.md` §2, §6).

Beyond the two callback addresses, the runtime path (tasks 03–04) needs:

- **The file-table global** (`DAT_1806f2f48` on 20260721): data rows at
  `[global+0x8] + file_id*0x40` (buffer pointer +0x8, size u32 +0x14) and
  path rows at `[global+0x28] + file_id*0xA0 + 0x11`. The existing
  `song_rate_wavebank_unregister` pattern already matches code performing
  exactly this access (the `+0x28` load, the 0xA0-stride row math, the
  `+0x11` path offset are literal in the pattern) — the global is
  RIP-decodable from that match without a new pattern.
- **Handle→file_id resolution inputs** (design req 11: the read detour
  performs the same AVS-mutex-guarded sorted-vector walk the stock callback
  performs). Mechanism free, behavior binding — candidate derivations per
  the RE note: RIP-decode the lookup helper (`FUN_1801aba70`) from the CALL
  inside the resolved readFile callback body, or derive the manager global
  + handle-vector offset (manager `+0x20C8..+0x20D0`) + the AVS mutex gate
  (libavs imports `XCnbrep700000f`/`XCnbrep7000010` gated on a flag
  global). Prefer whichever decodes robustly across all four builds;
  document the choice.

The repository's conventions for this work are established:
`SignatureDefinition` entries with rationale comments, a
`derive_*` function on `SignatureStore` that validates and publishes
derived addresses fail-closed (see `derive_song_rate_runtime_sites` — it
re-validates match uniqueness and literal bytes before publishing), and
fail-open integration: the new signatures MUST NOT join the required set —
absence means `binding::integration_available()` stays false, readiness
never conjoins true, and the DLL boots stock (design req 40).

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-08-08-song-rate-streaming/design/detailed-design.md`
  (reqs 9–11, 40; Appendix: Reverse-Engineering Basis)

**Additional References (if relevant to this task):**
- `docs/xact_streaming_research.md` — §2 (registration site disassembly,
  callback bodies, the manager/handle-vector facts), §6 (cross-version
  notes and the owed verification), §7 (gotchas)
- `.agents/planning/2026-08-08-song-rate-streaming/research/streaming-mechanism.md`
  — the binding-by-file_id and both-callbacks-mandatory findings

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. One new `SignatureDefinition` for the callback-registration region
   (`0xFA` immediate + three LEA/MOV pairs; LEA disp32s and frame disp8s
   wildcarded; semantic immediates literal) with a description naming both
   decoded targets and the design requirement it serves.
2. A derivation function (following `derive_song_rate_runtime_sites`
   conventions) that: re-validates the match is unique in the module,
   RIP-decodes the second and third LEAs, publishes
   `song_rate_readfile_callback` and `song_rate_overlapped_callback` (names
   free; consistent with the existing `song_rate_*` namespace), and removes
   everything it published on any validation failure with one WARN
   (fail-closed derivation, fail-open feature).
3. The file-table global derived from the existing
   `song_rate_wavebank_unregister` match (RIP-decode its global load;
   validate the surrounding literal bytes match the expected access shape
   before publishing) — published for tasks 03–04 to build the source view
   (buffer/size) and path lookup.
4. Handle→file_id resolution inputs derived and published (mechanism per
   the Background — helper-function address or manager-global + offset +
   mutex gate), sufficient for task-04's read detour to replicate the stock
   locked walk exactly.
5. None of the new names enter the required-signature set; a resolution
   failure of any piece leaves the DLL booting with the streaming
   integration structurally absent (existing Step-1 semantics).
6. Ghidra cross-verification on ALL FOUR builds (2026-03-24, 2026-04-21,
   2026-06-16, 2026-07-21): the AOB matches exactly once per build; the
   decoded targets equal the known callback addresses on 20260721 and are
   recorded per build for the others; the file-table global and
   handle-resolution derivations checked the same way. If any build is
   missing from the Ghidra project, HALT and report rather than shipping a
   partially verified pattern.
7. `docs/xact_streaming_research.md` §6 gains the cross-version table
   (per-build resolved addresses for every published name) and closes the
   "owed before implementation" note.
8. Host tests where the logic is pure (e.g., RIP-decode arithmetic on
   synthetic byte buffers if factored as a testable helper); boot-time
   validation covers the rest — matching the repository's existing
   signature-derivation test posture.

## Dependencies

- None within Step 4 (first task). Tasks 03 and 04 consume the published
  addresses. Steps 1–3 complete.

## Implementation Approach

1. In Ghidra (DDRWorld project, build 20260721): confirm the registration
   region bytes, draft the pattern, verify single-match and decode; then
   repeat on 0324/0421/0616, adjusting wildcards until all four match
   exactly once.
2. Implement the `SignatureDefinition` + derivation function; wire the
   derivation into the store's derivation pass next to
   `derive_song_rate_runtime_sites`.
3. Derive the file-table global from the existing unregister match and the
   handle-resolution inputs per the chosen mechanism; validate fail-closed.
4. Update `docs/xact_streaming_research.md` §6 with the cross-version
   table and the chosen handle-resolution mechanism.
5. Record progress in
   `.agents/planning/2026-08-08-song-rate-streaming/implementation/` (repo
   convention: NEVER `.agents/scratchpad/`); run the full gate set.

## Acceptance Criteria

1. **The pattern resolves both callbacks on all four builds**
   - Given the DDRWorld Ghidra project's four supported builds
   - When the AOB is searched and its LEAs are RIP-decoded per build
   - Then it matches exactly once per build, the 20260721 targets equal
     `FUN_1801aa250`/`FUN_1801aa350`, and all per-build addresses are
     recorded in the RE note's cross-version table

2. **Derivations publish fail-closed**
   - Given a store where the pattern or a validation check fails
   - When the derivation pass runs
   - Then no partial names are published, one WARN is emitted, and the
     existing `song_rate_*` trio and every other signature are unaffected

3. **The feature stays fail-open**
   - Given a boot where any new name is unresolved
   - When readiness is evaluated
   - Then `integration_available()` remains false and the DLL behaves
     exactly as the Step-1 identity base (no required-signature panic)

4. **Tree is green**
   - Given the completed task
   - When running the five standing gates
   - Then all pass, with the Windows-target check at 0 warnings

## Metadata

- **Complexity**: Medium
- **Labels**: signatures, reverse-engineering, song-rate, streaming, ghidra
- **Required Skills**: Rust, x86-64 pattern/RIP-decode work, Ghidra
  cross-version verification, repository signature conventions
- **Generated By**: code-task-generator 2026-08-10
- **Source Plan**: `.agents/planning/2026-08-08-song-rate-streaming/implementation/plan.md`
- **Plan Step**: Step 4: Wire the callback detours, binding, and generator into the transaction
