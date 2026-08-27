# Task: Audio Signature Patterns and Derivations

## Description

Add the three byte-pattern signatures the assist-tick feature's audio binding needs to this
repository's central signature registry, plus the derivation that turns one of those matches into
the address of the game's audio-manager global.

Nothing consumes these addresses yet — that is the next task. This one exists on its own because a
mis-derived audio-manager global would not fail loudly; it would look like "no audio" several steps
later, after a great deal of code had been written on top of it. Resolving and logging the addresses
first makes that failure mode visible in one boot.

## Background

**Working directory: this repository** (the DDR World hook DLL / modpack).

The game's audio subsystem is Microsoft XACT 2, wrapped by an in-house "audio manager" singleton
that owns six sound-bank slots and exposes a small "play a sound effect" façade. The assist-tick mod
will register its own bank into a free slot on that manager and play a cue through that façade, so it
needs three things resolved at runtime: the façade's public entry point, the manager global, and one
safety constant.

Two properties make this delicate:

- **The audio-manager global's address changes on every game build.** It was verified at four
  different addresses across four builds. It must therefore be *derived* by RIP-relative decode from
  a scanned landmark, never scanned for or hard-coded.
- **The landmark's neighbour is nearly identical.** The "play" and "prepare" inner functions are
  byte-for-byte identical for their first ~0x65 bytes apart from that one displacement, so the
  pattern has to run far enough to include the vtable index that distinguishes them. A shorter
  pattern matches both.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` — §4.3 "New signature
  patterns" for what the three patterns are and why each exists, and §4.1's "Address surface"
  subsection for what consumes them

**Additional References (if relevant to this task):**
- `.agents/planning/20260725-assist-tick/research/bank-slot-and-anchors.md` — **the primary
  specification for this task.** Its "Proposed signatures" section gives the three byte patterns
  verbatim (S1, S2, S3) with per-byte wildcard rationale and the exact match addresses on four game
  builds; its "Derivation chains" section gives chains A, B and C — the derivations to implement —
  and chain D as a documented last-resort fallback that is deliberately **not** being implemented.
  Its "Required guards" subsection explains what the third pattern's safety constant is for
- `.agents/planning/20260725-assist-tick/research/game-sound-engine.md` — §"Play/Stop API + inferred
  signatures" for the façade's shape, and §"Cross-Version Caution" for what is and is not stable
  across builds
- `.agents/steering/reverse-engineering.md` — address-space conventions and derivation-anchor practice
- `src/core/signatures.rs` — `derive_app_heap_handle` is the closest existing model: it decodes a
  RIP-relative operand at a fixed offset from an anchor, checks the instruction bytes before
  trusting the offset, and cross-checks a `CALL` target against another resolved signature, warning
  rather than panicking on disagreement

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Add three `SignatureDefinition` entries to `src/core/signatures.rs`, following the file's existing
   conventions (`?` wildcards, space-separated hex, a `description` that says what the match *is* and
   what is derived from it). Use the patterns exactly as the research note gives them — they were
   verified to match uniquely on four builds, and the wildcarding is deliberate.
2. Every byte that is an address, a RIP displacement, a stack-frame displacement or a branch
   displacement must be wildcarded. Semantic immediates must be left **literal**, so that a
   meaningful change to the game breaks the match instead of silently mis-resolving.
3. Add a `derive_*` method, called from `resolve_derived`, that produces:
   - **the audio-manager global**, by RIP-relative decode of the displacement inside the primary
     match. Use the existing scanner primitive; do not hand-roll displacement decoding.
   - **the inner play function's entry**, at the documented negative offset from the primary match,
     accepted **only** after verifying the expected function prologue bytes are there. If they are
     not, fall back to the existing "find the function entry" helper rather than trusting the offset.
   - **the named-bank count**, read as a byte at the documented offset from the third pattern's
     match. This is a safety gate: it must read `4`. Record the value; a different value means a
     future build added a named bank and the free-slot assumption no longer holds.
4. Cross-check the two play-related signatures against each other: the first `CALL rel32` in the
   public entry point's body must target the derived inner function. On disagreement, log a warning
   naming both addresses — one of the two signatures has mis-resolved. Do not panic and do not
   abort other signature resolution.
5. Every resolved and derived address must be logged at boot in the style the surrounding code
   already uses, as a module-relative offset, so a maintainer can compare it against the research
   note's per-build table by eye.
6. Failure of any one of these must degrade gracefully: log and return, leaving the rest of
   signature resolution untouched. No panics, no `unwrap`, no `expect`.
7. No hardcoded absolute game addresses anywhere. The research note's per-build address tables are
   for verification by a human reading the log, not for the code.
8. Nothing outside `src/core/signatures.rs` should need to change.

## Dependencies

- `src/core/signatures.rs` — the registry being extended, and `resolve_derived`, which must call the
  new derivation
- `src/core/scanner.rs` — `decode_rip_relative`, `decode_call_rel32`, `scan_first_call_rel32`,
  `find_function_entry`. Per this repository's conventions these are the only sanctioned way to do
  displacement decoding; do not reimplement any of them inline
- No new crate dependencies
- No dependency on Step 1's committed asset

## Implementation Approach

1. Read the research note's "Proposed signatures" and "Derivation chains" sections in full, including
   the wildcard-rationale table. The patterns are transcribed from verified matches; retyping them
   from the disassembly instead is how a subtle difference gets introduced.
2. Read `derive_app_heap_handle` in `src/core/signatures.rs` and follow its shape: guard on the
   anchor resolving, verify the instruction bytes at the expected offset before decoding, insert the
   derived address, log it as a module-relative offset, then do the cross-check and warn on mismatch.
3. Add the three definitions, then the derivation, then wire it into `resolve_derived`.
4. Build, install the DLL into the local game install, launch, and read the boot log.

## Acceptance Criteria

1. **All three patterns resolve uniquely on the running build**
   - Given the DLL is loaded into the game
   - When signature resolution runs at boot
   - Then each of the three new signatures reports exactly one match, and the boot log shows each
     one's module-relative offset

2. **The audio-manager global is derived, not scanned**
   - Given the primary pattern matched
   - When the derivation runs
   - Then the manager global's address is obtained by RIP-relative decode from within that match, is
     logged as a module-relative offset, and appears nowhere in the source as a literal

3. **The inner play function's entry is verified before it is trusted**
   - Given the primary pattern matched
   - When the derivation computes the function entry at the documented negative offset
   - Then it checks the expected prologue bytes at that address, and on mismatch falls back to the
     function-entry helper and says so in the log rather than accepting the offset blindly

4. **The named-bank-count safety gate is read and reported**
   - Given the third pattern matched
   - When the derivation reads the byte at the documented offset
   - Then the value is logged, and a value other than `4` produces a clearly-worded warning saying
     the free-slot assumption may no longer hold

5. **The two play signatures corroborate each other**
   - Given both play-related signatures resolved
   - When the first `CALL rel32` in the public entry point's body is decoded
   - Then it equals the derived inner function's address; and if it does not, a warning naming both
     addresses is logged and resolution continues

6. **Missing anchors degrade gracefully**
   - Given any one of the three patterns fails to match
   - When resolution runs
   - Then exactly one warning is logged for it, no panic occurs, the game boots normally, and every
     other signature still resolves

7. **The build gates pass**
   - Given the completed change
   - When `cargo check --target x86_64-pc-windows-msvc`, then `cargo fmt`, then `./build.sh` are run
   - Then all three complete cleanly

## Metadata
- **Complexity**: Medium
- **Labels**: reverse-engineering, signatures, aob, audio, cross-version
- **Required Skills**: Rust; x86-64 instruction encoding (RIP-relative and `CALL rel32`
  displacements); AOB signature authoring with deliberate wildcarding; reading reverse-engineering
  notes and preserving their verified byte patterns exactly
- **Generated By**: code-task-generator 2026-07-26
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 2 — `services::game_audio` — signatures, bank registration, cue playback
