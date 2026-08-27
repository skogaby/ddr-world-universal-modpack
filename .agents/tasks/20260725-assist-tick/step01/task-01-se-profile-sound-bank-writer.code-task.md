# Task: SE-Profile XACT Sound Bank Writer

## Description

Add an SE-profile emission path to this crate's XACT2 sound-bank (XSB) writer, alongside the
existing song-profile path. The existing writer emits DDR's *song* profile — mix category 4/3 with
a runtime-parameter curve attached — which is correct for song audio but wrong for a sound effect:
it would place the cue on the music mix bus and attach a parameter curve referencing global audio
state that a mod never sets.

The new path emits the shape DDR's own gameplay sound effects use: **mix category 6, a bare sound
entry, and no runtime-parameter curve.**

This is the enabling change for an "assist tick" feature in a sibling project (a hook DLL for DDR
World) that needs to play its own short sound effect through the game's XACT engine.

## Background

**Working directory: the `ddr-chart-tools` repository** — a sibling checkout, not the modpack repo
that this task file lives in. Read that repository's `CLAUDE.md` and `docs/xsb_format.md` first.

DDR World's audio subsystem is Microsoft XACT 2 (`xactengine2_10.dll`). A sound bank names the cues
inside a wave bank; the engine validates a CRC-16 over the sound bank's own bytes and, on mismatch,
**silently rejects the bank** — no error, audio simply goes dark. The existing writer in
`src/xsb/mod.rs` already implements that CRC and the cue-name hash function, both reverse-engineered
from the engine DLL, and reproduces the byte layout of real DDR song sound banks. Only the
sound-entry portion needs a second variant.

The consumer will call the resulting bank's single cue by name at runtime, via the game's own
"play a cue" entry point.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` (in the sibling
  *modpack* repository) — §4.4 "Asset pipeline" for what the bank is for and the naming
  constraints; §2.3 and §6 for why silent rejection drives the offline-validation approach
- `docs/xsb_format.md` (in **this** repository) — the authoritative format specification, including
  the header layout, section offsets, the CRC-16 coverage range, and the cue-name hash

**Additional References (if relevant to this task):**
- `.agents/planning/20260725-assist-tick/research/xact-bank-format.md` (in the sibling modpack
  repository) — the reverse-engineering record behind this change. Its findings on the sound-entry
  shapes are the direct source for this task: the stock **song** sounds use category 4/3 plus an
  RPC, while stock **gameplay SE** sounds use category 6 with a bare 12-byte sound entry and no
  RPC. It also documents which stock banks the CRC and hash implementations were verified against

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Add a public SE-profile entry point alongside the existing `write` — e.g.
   `pub fn write_se(name: &str, out: &mut impl Write) -> Result<(), XsbError>`. Naming is at the
   implementer's discretion so long as it is discoverable and documented.
2. The emitted bank must contain **exactly one cue**, named `name`, pointing at wave index **0**,
   backed by **one** sound entry.
3. The sound entry must use **mix category 6**, must be the bare (non-complex) form, and must carry
   **no runtime-parameter curve**.
4. The sound-bank name field and the wave-bank name field must both be `name`, written
   byte-identically — the engine matches a sound bank to its wave bank by name, **case-sensitively**.
5. Section offsets, counts, and the total size must be recomputed for the new shape rather than
   assuming the song profile's fixed layout. Note the hash-bucket count rule from the format spec
   (`max(16, cue_count)`) still applies with a single cue.
6. The CRC-16 must be computed over the same range as the existing path and stored in the same
   field.
7. The existing song-profile path must remain **byte-identical**. This is a purely additive change;
   shared internals may be refactored, but the song profile's output must not move by one byte.
8. Input validation must match the existing path's contract for names (accepted character set and
   length), returning the crate's existing error type rather than panicking.
9. This repository's gates must pass clean: `cargo test`, `cargo clippy --all-targets -- -D warnings`
   (zero warnings), and `cargo fmt` (zero diff).

## Dependencies

- Existing `src/xsb/mod.rs` — the writer being extended, including its CRC-16 and cue-name hash
  implementations, both of which are reused unchanged
- `docs/xsb_format.md` — the format contract
- No new crate dependencies

## Implementation Approach

1. Read `docs/xsb_format.md` in full, then read `src/xsb/mod.rs` to understand how the existing
   layout computation, sound-entry emission, cue table, hash table, name index, and CRC step fit
   together.
2. Identify the minimum seam for a second profile — most likely an internal profile parameter
   threaded through the layout computation and the sound-entry emitter, with two thin public
   wrappers over it. Prefer the smallest change that keeps both profiles readable; do not
   duplicate the whole builder.
3. Emit the SE sound entry per the research findings, and recompute the layout for one cue and one
   sound.
4. Cover it with unit tests in the existing test module (see below), then run the three gates.

## Acceptance Criteria

1. **SE profile emits a structurally valid bank**
   - Given a valid four-character name
   - When the SE-profile writer is called
   - Then it returns Ok, and the emitted bytes carry the correct magic, version, a cue count of 1,
     a sound count of 1, a wave-bank count of 1, and section offsets consistent with the emitted
     section sizes

2. **The sound entry uses the SE shape**
   - Given a bank emitted by the SE-profile writer
   - When its sound entry is inspected
   - Then the mix category is 6, the entry is the bare form, no runtime-parameter curve is present,
     and the entry references wave index 0

3. **Names are written identically in both name fields**
   - Given a name whose case is mixed
   - When the SE-profile writer emits a bank
   - Then the sound-bank name field and the wave-bank name field contain that name byte-for-byte
     with case preserved, null-padded to the fixed field width

4. **The cue is resolvable by name**
   - Given a bank emitted by the SE-profile writer
   - When the cue name is run through the crate's cue-name hash and the resulting bucket is
     followed through the hash table and name index
   - Then it resolves to cue index 0, and the cue name string table contains exactly that one
     null-terminated name

5. **The CRC is self-consistent**
   - Given a bank emitted by the SE-profile writer
   - When the CRC is recomputed over the documented coverage range
   - Then it equals the value stored in the header's CRC field

6. **The song profile is unchanged**
   - Given the same inputs as before this change
   - When the existing song-profile writer is called
   - Then its output is byte-identical to the pre-change output, asserted against a committed
     expected-bytes fixture or an equivalent regression test

7. **Invalid input is rejected, not panicked on**
   - Given an empty name, an over-length name, and a name with non-permitted characters
   - When the SE-profile writer is called with each
   - Then it returns the crate's existing error variant for a bad name in every case, and no test
     panics

8. **Gates are clean**
   - Given the completed change
   - When `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` are run
   - Then all three pass with no failures, no warnings, and no diff

## Metadata
- **Complexity**: Medium
- **Labels**: binary-format, xact, audio, cross-repo, additive
- **Required Skills**: Rust; binary file-format authoring; careful byte-layout work against a
  specification; reading reverse-engineering notes
- **Generated By**: code-task-generator 2026-07-25
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 1 — Generate and commit the clap bank pair (offline asset pipeline)
