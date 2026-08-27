# Progress — Audio Signature Patterns and Derivations (Step 2, task 01)

**Updated:** 2026-07-26
**Status:** Complete (not committed — the maintainer owns commits in this repo).
All seven acceptance criteria verified: offline against the real DLLs in Ghidra, then confirmed on a
live boot of the local install.

## Checklist

- [x] Setup: working dir, upstream approval chain verified, build commands recorded
- [x] Explore: research-note transcription audit (every used offset cross-checked ≥2 ways), precedents identified
- [x] Plan: verification table (log evidence per acceptance criterion) + implementation approach
- [x] Add the three `SignatureDefinition` entries (S1/S2/S3) to `SIGNATURES`
- [x] Add `derive_game_audio_addresses` (+ two sub-methods), called from `resolve_derived`
- [x] Offline verification of all three patterns and all four derivations, in Ghidra, on the real DLLs
- [x] Gate: `cargo check --target x86_64-pc-windows-msvc` — clean, no warnings
- [x] Gate: `cargo fmt` — no reformatting of the new code needed
- [x] Gate: `./build.sh` — clean release build
- [x] Install the DLL (and, once, `data_mods/assist_tick/`) into the local install
- [x] Launch, read `log.txt`, confirm AC1–AC6

## What was built

`src/core/signatures.rs` only (+261 lines), as required — nothing else in the crate changed.

**Three registry entries**, appended at the end of `SIGNATURES` under one banner comment pointing at
the RE record:

| `name` | Match is | Yields |
|---|---|---|
| `se_play` | function entry | the public play façade; pan in **XMM2** |
| `se_play_inner_body` | `se_play_inner + 0xF` | the anchor for everything in chain A |
| `bank_slot_of_file_loop` | the name-match loop | the named-bank count at `+0x2C` |

**`derive_game_audio_addresses`**, called last from `resolve_derived`, in three independent stages:

1. Match-count diagnostic (`get_all_matches` ×3) — one info line with all three counts, a `[!]` for
   any count > 1. Exists because `resolve_all` is first-match-per-name, and S1's neighbour
   `se_prepare_inner` is byte-identical for ~0x65 bytes: binding Prepare instead of Play would look
   like "no audio" several steps later instead of failing here.
2. `derive_audio_manager_and_play` — chain A (`audio_manager_global` by RIP decode at anchor+3,
   bound-checked against the module; `se_play_inner` at anchor−0xF, accepted only after its 15
   prologue bytes verify, else `find_function_entry` with a `[!]`) and chain B (`se_play`'s first
   `CALL rel32` must equal the derived inner entry; `[!]` naming both offsets on disagreement, then
   continue).
3. `derive_audio_named_bank_count` — chain C: publishes `audio_named_bank_count_site` (the address of
   the imm8) and logs the value, with a `[!]` if it is not 4.

Chain D is not implemented (the note documents it as a last resort only). `se_mute_filter`, also
available from chain B, is not derived — nothing consumes it.

## Deviations

None from the plan. Two judgement calls, both pre-recorded in `context.md` → "Ambiguities found":

1. The named-bank count is published as the **address of the imm8** rather than as a value, because
   `SignatureStore`'s map is `String → *const u8`. This keeps the `+0x2C` offset inside
   `signatures.rs` and lets Step 2's `register_bank` apply guard G1 itself.
2. `se_mute_filter` is not derived.

One compile-time fix during implementation: the `in_module` bound-check closure initially captured
`self.size` (moving `&mut self`), which conflicted with the later `self.resolved.insert`. Fixed by
copying `base`/`size` into locals before the closure.

## Cycles

Not a red/green TDD sequence — there is no harness (see `context.md` → "Build / test commands"). The
substitute, per the plan, is diagnostic logging verified against the real binaries.

| # | Work | Result |
|---|---|---|
| 1 | Three registry entries appended to `SIGNATURES` | `cargo check` clean |
| 2 | `derive_game_audio_addresses` + sub-methods + `resolve_derived` call | E0382 (closure captured `self`) → fixed → clean, no warnings |
| 3 | Offline verification in Ghidra (below) | all four builds, all derivations correct |
| 4 | `cargo fmt`, `./build.sh` | both clean |
| 5 | Install + launch + read `log.txt` | every expected line present, no `[!]` |

Logs: `logs/cargo-check.log`, `logs/build.log`, `logs/boot-audio-signatures.log`.

## Verification

### Offline, in Ghidra, against the actual game DLLs

Done **before** installing anything, so the boot was a confirmation rather than a discovery. Every
number below is the research note's predicted value, independently reproduced from my transcription
of the patterns (not the note's):

**Pattern uniqueness and match addresses** (`ghidra_search_byte_patterns`, exactly one hit each):

| Pattern | 20260721 | 20260616 | 20260421 | 20260324 |
|---|---|---|---|---|
| `se_play_inner_body` | `+0x1AB7AF` ✅ | `+0x1AA74F` ✅ | `+0x1A98EF` ✅ | `+0x1A8C4F` ✅ |
| `se_play` | `+0x1AA6E0` ✅ | `+0x1A9650` ✅ | — | — |
| `bank_slot_of_file_loop` | `+0x1AA440` ✅ | `+0x1A93B0` ✅ | — | — |

**Derivations, by reading the bytes** (20260721):

| Derivation | Evidence |
|---|---|
| prologue at anchor−0xF | `0x1801AB7A0` reads `48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 40` — byte-equal to the constant, immediately followed by `48 8B 35 aa 75 54 00` at `0x1801AB7AF` (so the match really is entry+0xF) |
| `audio_manager_global` | disp32 `0x005475AA` at `0x1801AB7B2` → `0x1801AB7B6 + 0x5475AA` = `0x1806F2D60` = **+0x6F2D60** |
| named-bank count | `0x1801AA46C` reads `04`, followed by `72 D1` then `B8 05 00 00 00` — the loop bound and the literal fallback 5, exactly where the pattern places them |
| chain B cross-check | first `0xE8` in `se_play`'s first 128 bytes is at **+0x73** (all 115 preceding bytes read out and confirmed free of `E8`, so `scan_first_call_rel32` cannot mis-fire); `E8 48 10 00 00` → `0x1801AA758 + 0x1048` = `0x1801AB7A0` = the derived `se_play_inner` ✅ |

### On the running build

`log.txt`, boot of 2026-07-26 15:52 (`logs/boot-audio-signatures.log`):

```
[+] se_play @ +0x1AA6E0
[+] se_play_inner_body @ +0x1AB7AF
[+] bank_slot_of_file_loop @ +0x1AA440
[+] audio signature match counts: se_play=1 se_play_inner_body=1 bank_slot_of_file_loop=1
[+] audio_manager_global (derived, se_play_inner_body RIP disp32) @ +0x6F2D60
[+] se_play_inner (derived, prologue verified) @ +0x1AB7A0
[+] audio_named_bank_count_site @ +0x1AA46C (named bank count = 4)
```

| AC | Verdict |
|---|---|
| 1 — three patterns resolve uniquely | ✅ three `[+]` lines, all three counts `1` (also proved in Ghidra on 4 builds) |
| 2 — manager global derived, not scanned | ✅ `+0x6F2D60`, matching the note's table; no absolute address in the source |
| 3 — inner entry verified before trusted | ✅ "prologue verified", `+0x1AB7A0` = match − 0xF; the fallback wording did not appear |
| 4 — named-bank gate read and reported | ✅ `count = 4`, no `[!]` |
| 5 — the two play signatures corroborate | ✅ no `[!]` for the pair (independently proved by hand above) |
| 6 — missing anchors degrade gracefully | ✅ by construction: every stage guards `get_address` and returns after one `log_warn!`. Corroborated live by the three *pre-existing* `[-]` misses (`series_label_lookup_inlined`, `folder_register`, `textlayer_bind_anchor` — all older-build twins whose `_standalone`/`_v2`/`_direct` partners resolved), which did not disturb the 122 successful `[+]` resolutions |
| 7 — build gates | ✅ `cargo check`, `cargo fmt`, `./build.sh` all clean |

Boot health otherwise unchanged: 122 `[+]` lines, the same three known pattern misses as before, no
new warnings, game reached the title screen normally and shut down cleanly.

## Notes for the next task

- Consume `audio_manager_global`, `se_play`, `se_play_inner` and `audio_named_bank_count_site` by
  name from the `SignatureStore`.
- `audio_named_bank_count_site` points at the **imm8**; `register_bank`'s guard G1 reads one byte
  there and declines if it is not 4.
- `se_play_inner` is resolved but unused: it is the one-line swap documented in design §6 should the
  SE mute filter turn out to veto our bank.
- The `data_mods/assist_tick/` tree is now installed at
  `$DDR_WORLD_INSTALL/data_mods/assist_tick/` (sha256 verified identical to the repo copies), so
  Step 2's bank load has its files.

## Boot cost added

Three single-needle whole-module `scan_pattern_all` passes for the match-count diagnostic, commented
at the call site as a deliberate cost. Not measurable against the existing boot profile by eye.

**Status: Complete (uncommitted — maintainer owns commits)**
