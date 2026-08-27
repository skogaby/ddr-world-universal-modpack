# Plan — Audio Signature Patterns and Derivations (Step 2, task 01)

**Status: Approved 2026-07-25** (inherited: the task descends from an approved
`implementation/plan.md` and an approved `design/detailed-design.md`; see `context.md` →
"Upstream approval chain")
**Date:** 2026-07-26

## Verification approach (no test harness)

This repo has no unit tests. Per the plan's Step 2 "Tests covering this step" and the feature's
verification split, each acceptance criterion is discharged by a **log line the change itself emits**,
read out of `$DDR_WORLD_INSTALL/log.txt`, plus the build gates. The table below is this task's
substitute for a test list: every row names the observable evidence and the value it must show.

| AC | Observable evidence in `log.txt` | Passes when |
|---|---|---|
| 1 — three patterns resolve uniquely | `[+] se_play @ +0x…`, `[+] se_play_inner_body @ +0x…`, `[+] bank_slot_of_file_loop @ +0x…` from `resolve_all`, plus one `audio signatures: matches se_play=1 se_play_inner_body=1 bank_slot_of_file_loop=1` line | three `[+]` lines present; all three counts are `1` |
| 2 — manager global derived, not scanned | `[+] audio_manager_global (derived from se_play_inner_body RIP disp32) @ +0x…` | present; on the 20260721 build the offset reads `0x6F2D60` (research table); no absolute address in the source |
| 3 — inner entry verified before trusted | `[+] se_play_inner (derived, prologue verified) @ +0x…` | present, and equals the S1 match offset minus `0xF`. The fallback wording `(derived via find_function_entry — prologue mismatch: …)` must **not** appear |
| 4 — named-bank-count gate read and reported | `[+] audio_named_bank_count_site @ +0x… (count=4)` | count reads `4`; a different value additionally emits `[!] … free-slot assumption may no longer hold` |
| 5 — the two play signatures corroborate | absence of `[!] se_play first CALL target … != se_play_inner …` | no `[!]` line for that pair |
| 6 — missing anchors degrade gracefully | with a pattern deliberately broken: exactly one `[-] …` line, boot completes, every other `[+]` still present | no crash, no missing unrelated signature |
| 7 — build gates | `cargo check`, `cargo fmt`, `./build.sh` | all clean |

AC6 is exercised offline rather than in the game: a temporary local edit that corrupts one literal
byte of each pattern in turn, verified by `cargo check` plus reading the resulting code path, then
reverted. Deliberately breaking the shipped patterns in the installed DLL would prove nothing the
code path does not already make evident, and the boot log lines for the `None` branches are
unconditional.

Expected offsets on the running build (20260721), from the research note's per-build tables — these
are what the maintainer compares the log against by eye:

| Thing | Expected `+0x…` |
|---|---|
| `se_play_inner_body` match | `1AB7AF` |
| `se_play_inner` (match − 0xF) | `1AB7A0` |
| `audio_manager_global` | `6F2D60` |
| `se_play` | `1AA6E0` |
| `bank_slot_of_file_loop` match | `1AA440` |

## Implementation approach

All changes in `src/core/signatures.rs`. Nothing else.

### 1. Three registry entries, appended at the end of `SIGNATURES`

Grouped under one banner comment that states the feature, the research note path, and the four-build
verification, following the style of the `movie_build_graph` entry directly above them.

| `name` | Source | Match is | Description says |
|---|---|---|---|
| `se_play` | S2 | function entry | the public play façade `(bank_id, cue, pan)`; pan travels in **XMM2**; cross-checked via its first `CALL rel32` |
| `se_play_inner_body` | S1 | `se_play_inner + 0xF` | landmark for the audio-manager global; must run to the `41 FF 52 20` vtable index or it also matches `se_prepare_inner` |
| `bank_slot_of_file_loop` | S3 | the match loop's `MOV R9,[RSI]` | sources the named-bank count at `+0x2C`, the slot-4-is-free safety gate |

Patterns transcribed verbatim from the note, `??` wildcards, single-line strings (the file's
convention). The wildcard/literal split is the note's and is not second-guessed: RIP disp32, stack
displacements, rel8 branches and the `strncmp` rel32 are wildcards; `41 FF 52 20`,
`B9 FF FF 00 00`, `41 83 F8 04`, `B8 05 00 00 00` and the `48 8D 47 01 / 48 03 C0 / 48 8B 1C C6`
slot-stride triple stay literal so a semantic change breaks the match instead of mis-resolving.

### 2. `derive_game_audio_addresses(&mut self)`

Placed immediately after `derive_app_heap_handle` (the model it follows) and called last from
`resolve_derived`.

Order of work, each stage independent so a later failure does not lose an earlier success:

1. **Match-count diagnostic.** `get_all_matches` for the three names; one `log_info!` with all three
   counts; `log_warn!` per name whose count is not exactly 1. Runs first so the count is in the log
   above any derivation that a duplicate would have poisoned.
2. **Chain A — from the `se_play_inner_body` anchor.** Absent ⇒ one `[-]`, return (nothing else in
   the method can proceed without it).
   - `audio_manager_global = decode_rip_relative(anchor + 3)`. The three bytes before the disp32 are
     literal in the pattern, so a match guarantees the instruction shape; what is *not* guaranteed is
     that the displacement lands in the module, so bound-check the result against
     `[base, base + size)` (as `derive_file_manager_singleton` does) and refuse on failure.
   - `se_play_inner = anchor − 0xF`, accepted only if the 15 prologue bytes
     `48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 40` are present, else
     `find_function_entry(anchor, base)` with a `[!]` naming the bytes actually seen. Bound-check the
     subtraction against `base` first.
3. **Chain B — cross-check.** Only if `se_play` resolved: `scan_first_call_rel32(se_play, 0x80)`
   must equal the derived inner entry; on disagreement one `[!]` naming **both** module-relative
   offsets, then continue. A missing `se_play` is already reported by `resolve_all`, so no second
   warning for it.
4. **Chain C — the safety gate.** From `bank_slot_of_file_loop`: publish `match + 0x2C` as
   `audio_named_bank_count_site`, log the byte it holds, and `log_warn!` if it is not `4`, in the
   words the acceptance criterion asks for ("a future build added a named bank — the free-slot
   assumption may no longer hold"). The site is published regardless of the value so Step 2's
   `register_bank` can apply guard G1 itself.

Chain D is not implemented; the note documents it as a last resort only.

### Derived names published

| Name | Value | Consumer |
|---|---|---|
| `audio_manager_global` | address of the `void*` global holding the audio manager | Step 2 `game_audio`, null-checked before every call |
| `se_play_inner` | inner play entry | Step 2's documented one-line mitigation if the SE mute filter vetoes our bank |
| `audio_named_bank_count_site` | address of the imm8 named-bank count | Step 2 `register_bank` guard G1 |

## Risks and how they are contained

| Risk | Containment |
|---|---|
| S1 pattern truncated ⇒ also matches `se_prepare_inner` (a Prepare, not a Play — silently no audio) | pattern includes `41 FF 52 20`; the match-count diagnostic would report 2 |
| Manager global mis-derived ⇒ looks like "no audio" several steps later | this task exists precisely to surface it in one boot; offset logged for eye-comparison against the research table |
| A future build shifts the `−0xF` entry offset | prologue check, with the `find_function_entry` fallback and a `[!]` in the log |
| A future build adds a fifth named bank | the count gate, plus Step 2 computing the free slot rather than assuming 4 |
| Extra boot cost from three whole-module `scan_pattern_all` passes | single-needle Aho-Corasick each; `resolve_derived` already runs several. Noted in a comment so it is a considered cost, not an accident |

## Maintainability notes

The three offsets the code uses (`+3`, `−0xF`, `+0x2C`) each appear exactly once, adjacent to a
comment naming the instruction they index into, so the next reader can re-derive them from the note
without re-reading the disassembly.
