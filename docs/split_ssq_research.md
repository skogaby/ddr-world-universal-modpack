# Split SSQ Discovery — `build_ssq_path` and the Hardcoded Split-Chart Table

> RE record for a proposed mod that replaces the game's hardcoded "which SSQ file
> holds this chart" table with runtime discovery, so an older `gamemdx.dll` can load
> split-chart data shipped by newer game revisions (as long as the song is in the
> installed `musicdb.xml`).
>
> All addresses are file-relative to image base `0x180000000`, program
> `gamemdx_20260721.dll` unless tagged with another build. Cross-build facts were
> produced by an offline capstone walk over the four PE-mapped builds in
> `~/Desktop/ddr_modules` (20250805 / 20260224 / 20260721 / 20260825); the
> installed cabinet binary (`$DDR_WORLD_INSTALL/modules/gamemdx.dll`, PE stamp
> `0x6A547E65`) is byte-identical in stamp/size to `gamemdx_20260721.dll`.
> Investigated 2026-09-03. Companion docs: `ultrafast_boot_research.md` (the boot
> pass that consumes this function), `ssq_format.md` (chunk layout, chart codes),
> `binary_modpack_research.md` §10 (a third party's hex-edit of the same function
> on 20250805).

## 1. Overview

A DDR song's charts normally live in one file, `data/mdb_apx/ssq/<basename>.ssq`,
which carries ONE tempo chunk (type 1) plus one step chunk (type 3) per chart. A
handful of songs have charts whose timing gimmicks (different BPM curves / stops
per difficulty) cannot share a tempo chunk, so Konami splits them into
`<basename>_<N>.ssq` files, `N ∈ 1..5` = Beginner..Challenge. The base file keeps
the easy charts, `_N` files hold the hard ones.

The decision "for `(basename, difficulty)`, which file?" is made by ONE function —
`build_ssq_path` (`0x1801B43F0`) — whose body is a hardcoded chain of
`repe cmpsb` string compares against an `.rdata` song-code table. There is no
data-driven table anywhere; adding a split song requires a new DLL. Every SSQ
consumer in the game (boot analysis pass, normal gameplay, matching play, courses)
calls this function and hands the resulting path to the FileManager, so the
function is the single choke point for the whole feature.

**Why per-(basename, difficulty) is the only correct level to intervene.** One
could imagine a LayeredFS-level fix (serve a synthesized merged `<basename>.ssq`).
That is impossible: the split exists precisely because the files carry DIFFERENT
tempo chunks (see §6 — e.g. `hkhk.ssq` and `hkhk_3.ssq` have different type-1 and
type-2 chunks), and an SSQ has exactly one tempo chunk. The file choice must stay
per difficulty, which is exactly this function's contract.

## 2. Key Addresses

| Symbol | 20250805 | 20260224 | 20260721 | 20260825 | Notes |
|---|---|---|---|---|---|
| `build_ssq_path` | `0x18019E8D0` | `0x1801A1730` | `0x1801B43F0` | `0x1801B4090` | body size `0x3A9` / `0x55C` / `0x70F` / `0x70F` (grows with the table) |
| `"data/mdb_apx/ssq/"` | — | — | `0x180381B28` | — | prefix; song-code table follows immediately (8-byte cells) |
| `"%s%s_%c.ssq"` | `0x18035F5A0` | `0x180366C50` | `0x180381C58` | `0x180381C78` | split format; ONLY xref is the builder's `LEA R8` (see §7) |
| `"%s%s.ssq"` | — | — | `0x180381C68` | — | unsplit format |
| `_snprintf`-style writer (`FUN_18022FEF0`) | — | — | `0x18022FEF0` | — | `(buf, cap, fmt, ...)`; NUL-terminates at `buf[cap-1]` on overflow |
| FileManager register (`FUN_1801FEF30`) | — | — | `0x1801FEF30` | — | `(mgr, path) → entry index`; see `ultrafast_boot_research.md` §2 |
| FileManager singleton | — | — | `DAT_1806F2F48` | — | = signature `step_data_global_table` |

Callers on 20260721 (all four are `CALL rel32` sites):

| Call site | Caller | Identity |
|---|---|---|
| `0x180032188` | `FUN_180032030` | `sequence::common::CheckStepDataActor::onInit` (vtable[4]) — the boot analysis pass |
| `0x1800578C5` | `FUN_180057480` | `sequence::dance::DancePlaySequence` vtable slot 4 (`onSetup`; vtable `0x180360AB8`) |
| `0x1800617FB` | `FUN_180061680` | `sequence::dance::MatchingDancePlaySequence` vtable slot 4 (`onSetup`; vtable `0x180360FC8`) |
| `0x1801DF608` | `FUN_1801DF490` | `ddr::player::PlayerCourseWork::prepare` (named by its own `XCnbrep` log string; called from `FUN_1800FDFA0`) — the course batch-preload |

(Class identities recovered by walking each vtable back to its RTTI complete
object locator and reading the type descriptor name.)

## 3. ABI

```
void build_ssq_path(char out[0x100] /*RCX*/, const char *basename /*RDX*/, int difficulty /*R8D*/)
```

* `out` — caller-owned 256-byte stack buffer (`local_138[256]` in every caller);
  the writer is invoked with cap `0x100`.
* `basename` — NUL-terminated song code, ≤ 7 chars (the music-DB entry's inline
  code at `music::Info+0xD`, obtained via entry vfunc `+0x08`). See §8 for the one
  case where it is NOT the DB basename.
* `difficulty` — `0..4` = Beginner, Basic, Difficult, Expert, Challenge. The
  split-file suffix character is literally `difficulty + 0x31` (`'1'..'5'`). The
  function performs NO range check.
* Returns nothing; all effect is the string written to `out`.
* The function uses `RSI`/`RDI` for `repe cmpsb` but saves/restores both, so from
  the caller's view it is a plain MS-x64 3-argument call. A `GenericDetour` with
  signature `extern "C" fn(*mut u8, *const u8, i32)` is sufficient.

Prologue / epilogue (20260721):

```
1801b43f0: 48 89 74 24 08          MOV  [RSP+8],RSI
1801b43f5: 57                      PUSH RDI
1801b43f6: 48 83 EC 30             SUB  RSP,0x30
1801b43fa: 4C 8B D1                MOV  R10,RCX            ; out
1801b43fd: 48 8D 3D 38 D7 1C 00    LEA  RDI,["acef"]       ; first table cell
1801b4404: 48 8B F2                MOV  RSI,RDX            ; basename
1801b4407: B9 05 00 00 00          MOV  ECX,5
1801b440c: F3 A6                   REPE CMPSB
1801b440e: 0F 84 F5 03 00 00       JZ   stage2             ; acef: split at ANY difficulty
...
1801b47de: 48 89 54 24 20          MOV  [RSP+0x20],RDX     ; basename
1801b47e3: 4C 8D 0D 3E 33 1C 00    LEA  R9,["data/mdb_apx/ssq/"]
1801b47ea: 4C 8D 05 77 34 1C 00    LEA  R8,["%s%s.ssq"]
1801b47f1: BA 00 01 00 00          MOV  EDX,0x100
1801b47f6: 49 8B CA                MOV  RCX,R10
1801b47f9: E8 F2 B7 07 00          CALL FUN_18022fef0      ; unsplit path
...
1801b4ac6: 41 B8 02 00 00 00       MOV  R8D,2              ; forced "_3"
1801b4acc: 41 8D 40 31             LEA  EAX,[R8+0x31]      ; suffix char
1801b4ad0: 4C 8D 0D 51 30 1C 00    LEA  R9,["data/mdb_apx/ssq/"]
1801b4ad7: 4C 8D 05 7A 31 1C 00    LEA  R8,["%s%s_%c.ssq"]
1801b4ade: 89 44 24 28             MOV  [RSP+0x28],EAX
1801b4ae2: 48 89 54 24 20          MOV  [RSP+0x20],RDX
1801b4ae7: BA 00 01 00 00          MOV  EDX,0x100
1801b4aec: 49 8B CA                MOV  RCX,R10
1801b4aef: E8 FC B4 07 00          CALL FUN_18022fef0      ; split path
```

## 4. Decision Algorithm

The body is two `repe cmpsb` chains. Each cell is `LEA RDI,[code]; MOV RSI,RDX;
MOV ECX,len; REPE CMPSB` where `len` = strlen(code)+1 (5 for 4-char codes, 6 for
5-char codes — the terminator is compared too, so `"stvi"` does not match
`"stvi2"`), followed by either an unconditional `JZ` (match at any difficulty) or
`JNZ next; CMP R8D,imm; Jcc target`.

**Stage 1 — "is this a split request?"** Walk the table; on a match whose
difficulty predicate holds, jump to stage 2. Otherwise fall through to the
unsplit format `"%s%s.ssq"`. Predicates seen: `any`, `== 4`, `>= 3`, `>= 2`.

**Stage 2 — "which suffix?"** Default suffix is `difficulty + '1'`. A second,
shorter chain forces `R8D = 2` (⇒ `_3`) for songs whose Expert (and, for most,
Challenge) charts live in the `_3` file. Predicates: `== 3` (stvi, dopa2 — their
Challenge stays in `_5`) or `>= 3` (everyone else in the chain — Challenge
collapses to `_3` as well). The final cell uses `JL` with the fallthrough being the
forced write, which is the same predicate written the other way round.

Decompiler warning: Ghidra renders `CMP R8D,2; JGE` as `bVar4 = param_3 == 2,
param_3 < 2`, which reads as an equality test. Trust the disassembly (`JGE`).

### 4.1 Effective mapping (identical on every build where the song is present)

| Pattern | Songs | diff 0 | 1 | 2 | 3 | 4 |
|---|---|---|---|---|---|---|
| A — fully split | `acef` | `_1` | `_2` | `_3` | `_4` | `_5` |
| B — Challenge only | `chao2 kanb leda file shuk lien konr yuwo` | base | base | base | base | `_5` |
| C — Expert+ split, no collapse | `rabb` | base | base | base | `_4` | `_5` |
| D — hard charts in `_3`, Challenge in `_5` | `stvi dopa2` | base | base | `_3` | `_3` | `_5` |
| E — hard charts all in `_3` | `sabm mons flor mega yush sipp zend eoth buco mero kjnf2 houu2 scre suma gogg dede fizz casr hkhk smin danz mlwt gien` | base | base | `_3` | `_3` | `_3` |

Note the builder takes no mode argument: a `_N` file holds BOTH the single and the
double chart of that level (confirmed on disk, §6).

### 4.2 Table growth per build

| Build | Entries | Added vs previous |
|---|---|---|
| 20250805 | 19 | acef chao2 kanb leda file shuk rabb stvi lien dopa2 sabm mons flor mega yush sipp zend eoth buco |
| 20260224 | 27 | + mero kjnf2 houu2 konr scre suma gogg dede |
| 20260721 | 35 | + fizz casr hkhk smin danz mlwt gien yuwo |
| 20260825 | 35 | (no change) |

The table only ever grows, entries keep their order and predicates, and the
two-stage structure is byte-for-byte the same shape on all four builds (only the
cell count and the `rel32`/`RIP` displacements differ). This is exactly the
user-reported problem in binary form: a 20250805 binary knows nothing about the
16 songs added later, so with newer data it requests `fizz.ssq` for Expert and
finds no chart there.

## 5. How the Result Is Consumed

Every caller does the same thing with the path (decompiled shape, 20260721):

```c
build_ssq_path(path, basename, difficulty);
idx = FUN_1801fef30(DAT_1806f2f48, path);            // FileManager::register → entry index (dedupes by FNV name hash)
if (idx != -1) {
    name_rec = *(mgr+0x28) + idx*0xA0;
    if (name_rec[0x91] == 0) set_ext(name_rec+0x90, "ssq" /* boot */ or "default" /* gameplay */);
}
*(mgr+0x08 + idx*0x40 + 0x38) = priority;            // -99 (boot) / 2 (gameplay)
```

* **Boot pass** (`CheckStepDataActor::onInit`): `for song in musicDB: for d in 0..5:
  build → register → push work item {idx, d, mcode}`. All 5 difficulties are
  registered for every song regardless of `hasChart`; a nonexistent file simply
  fails to load (record status 5) and its Analyze runs on a zeroed result. The
  **corruption flag + `ME1529` "FILE CORRUPTION ERROR"** fires only when the DB's
  `hasChart` vslot says the chart should exist but the loaded file yielded no
  notes (`ultrafast_boot_research.md` §3.8). This is the failure mode a wrong file
  choice produces: a boot-blocking service error, not a silent miss.
* **`DancePlaySequence::onSetup`**: for each present side, `difficulty =
  *(int*)(side_info + 4)`; skipped entirely in course mode (`*(GlobalConfig+0x70)
  != 0`), where the indices come from the course work instead.
* **`MatchingDancePlaySequence::onSetup`**: same per-side loop.
* **`PlayerCourseWork::prepare`**: for each course stage, `difficulty =
  u8 course_stage_difficulty`, registering every stage's SSQ up front (this is the
  "courses batch-preload" behaviour recorded in `per_song_judgement_offsets.md`).

Everything downstream of the register — the async loader (AVS `avs_fs_open` on
device `local`, hence LayeredFS-visible), `SsqReader`, `IStepReader::Analyze`
(the `services::analyze_hook` boundary used by note_types_expansion mines and the
fast_bootup capture), the per-song judgement-offset SSQ-open observer — operates
on whatever file the builder named. **Redirecting the builder therefore covers
every in-game consumer with one detour.** The one DLL-side path builder that would
NOT follow is `src/services/chart_length.rs` (opens `mdb_apx/ssq/<code>.ssq`
itself); it should reuse the same resolver.

## 6. On-Disk Validation Against the Installed Data

The cabinet install (`$DDR_WORLD_INSTALL/data/mdb_apx/ssq`, 1576 files) contains
39 split files for 32 songs. Type-3 chart codes per file (S/D = single/double;
B b D E C = Beginner Basic Difficult Expert Challenge; codes per `ssq_format.md`
§5.1):

```
acef    base:MISSING            _1:SB  _2:Sb/Db  _3:SD/DD  _4:SE/DE  _5:SC/DC
rabb    base:SB/Sb/SD/Db/DD     _4:SE/DE                    (no Challenge chart in musicdb)
stvi    base:SB/Sb/Db           _3:SD/SE/DD/DE   _5:SC/DC
dopa2   base:SB/Sb/Db           _3:SD/SE/DD/DE   _5:SC/DC
sabm    base:SB/Sb/Db           _3:SD/SE/SC/DD/DE/DC   _5:SC/DC    (see below)
chao2 kanb leda file shuk lien konr
        base:SB/Sb/SD/SE/Db/DD/DE                 _5:SC/DC
buco casr danz eoth fizz flor gogg kjnf2 scre sipp smin stvi zend
        base:SB/Sb/Db           _3:SD/SE/DD/DE
hkhk    base:SB/Sb/Db           _3:Sb/SD/SE/SC/DD/DE/DC      (a redundant Basic copy in _3)
houu2 mega mero mlwt mons suma yush
        base:SB/Sb/Db           _3:SD/SE/SC/DD/DE/DC
```

Observations that matter for a dynamic rule:

1. **The stock table and the files agree** for every (song, difficulty) pair
   whose chart exists, with one redundancy: `sabm_5.ssq` is a strict subset of
   `sabm_3.ssq` (identical MD5 for the type-1, type-2, and both Challenge type-3
   chunks); the stock table never requests it (`sabm` is pattern E). Either file
   is correct for `sabm` Challenge.
2. **The table is AHEAD of this install's data**: `dede`, `gien`, `yuwo` are in the
   20260721 chain but have neither files nor `musicdb.xml` entries here. Harmless
   (never requested), and the mirror image of the target scenario.
3. **Split files genuinely differ in timing**: `hkhk.ssq` vs `hkhk_3.ssq` have
   different type-1 (tempo) and type-2 chunks. No merged single-file
   representation exists.
4. `acef` has NO base file — pattern A must resolve every difficulty to a `_N`.

### 6.1 Candidate discovery rules, checked against stock

Simulated over the install, comparing only (song, difficulty) pairs where the
stock-chosen file actually contains the chart:

| Rule | Result |
|---|---|
| **A** — highest `N ≤ difficulty+1` such that `<basename>_N.ssq` exists AND contains a type-3 chunk of that level (either mode); else base | matches stock everywhere except `sabm` Challenge (`_5` vs stock `_3`) — chunk-identical, harmless |
| **C** — highest `N ≤ difficulty+1` such that the file exists (filename only) | same as A on this data set |
| **B** — exact `_{difficulty+1}` if it exists, else base | WRONG for every pattern D/E song at Expert/Challenge (falls to base, which lacks the chart) — rejected |

Rule A is the recommendation: it reproduces stock on real data, it degrades to
"base" when a `_N` file is absent (identical to a build without the entry), and —
because it inspects the chart set — it can never point the loader at a file that
lacks the chart, which is the one outcome (§5) that raises the boot-blocking
corruption error. Rule C is a filename-only approximation that happens to match
here but has no such guarantee.

## 7. Signatures

### 7.1 Function entry (recommended anchor)

```
48 89 74 24 08 57 48 83 EC 30 4C 8B D1 48 8D 3D ?? ?? ?? ?? 48 8B F2 B9 05 00 00 00 F3 A6 0F 84
```

`MOV [RSP+8],RSI / PUSH RDI / SUB RSP,0x30 / MOV R10,RCX / LEA RDI,[rip+X] /
MOV RSI,RDX / MOV ECX,5 / REPE CMPSB / JZ rel32`. Wildcards: only the `LEA`
displacement. The `0F 84` tail pins the first cell's unconditional-match shape
(`acef`, the only "split at any difficulty" song — present since 20250805).
**Exactly one hit on all four builds** (addresses in §2). Match = function entry.

The shorter anchor recorded in `binary_modpack_research.md`
(`4C 8B D1 48 8D 3D ?? ?? ?? ?? 48 8B F2 B9 05 00 00 00 F3 A6`) also hits exactly
once per build, at entry `+0xA`; note that the inner `48 8B F2 B9 05 00 00 00 F3
A6` fragment alone repeats ~30× inside the body, so any variant must keep the
`4C 8B D1` prefix.

### 7.2 Structural alternate (derivation)

`"%s%s_%c.ssq"` has exactly ONE code xref on every build: the builder's
`4C 8D 05 <rel32>` (`LEA R8,[rip+X]`) in its split epilogue. Locate the string in
`.rdata`, scan `.text` for the one `LEA R8` whose target equals it, then walk back
to the nearest `48 89 74 24 08 57 48 83 EC 30` prologue. This survives table
growth (which only lengthens the middle of the function) and is a suitable
`_v2`-style fallback for `resolve_derived`.

### 7.3 What a hook may NOT assume

Nothing at `match+N` beyond the prologue is stable across builds: the chain
length, every `rel32`, and the epilogue offsets all move with the table
(`0x3A9`→`0x55C`→`0x70F`). A full-function replacement (call our resolver, never
read the original body) is the only shape that needs no per-build offsets.

### 7.4 Modded 20250805 binaries

`binary_modpack_research.md` §10 documents a third-party hex-edit that REWRITES
this function on 20250805 into a data-table walk. On such a DLL neither anchor
above is guaranteed (the prologue was replaced). The mod must treat "signature
missing" as fail-open (stock/patched behaviour continues) rather than required.

## 8. Gotchas

* **`toho%d` random basename.** Both play-sequence `onSetup`s rewrite the basename
  buffer to `"toho%d"` (`(rand & 3) + 1`) for mcode `0x939D` BEFORE calling the
  builder, so the hook can see `toho1..toho4` — codes that are not `musicdb.xml`
  basenames. `PlayerCourseWork::prepare` does the same. (Mcodes `0x9306` /
  `0x94E7` alter a DIFFERENT string — the `std::string` at `DPS+0xC8` /
  `MDPS+0xC0` used for the "basename:musicID:Difficulty" report — not the SSQ
  basename.) A directory-scan-based index handles this naturally; a
  musicdb-driven index would not.
* **Call volume.** The boot pass calls the builder `5 × songs` times (~7200 on a
  1441-song DB) synchronously on the game thread inside `onInit`. The resolver
  must be a precomputed lookup (hash map keyed by basename), never a per-call
  filesystem probe.
* **Basename length.** Inline DB code buffer is ≤ 7 chars (`music::Info+0xC`
  length byte, `+0xD` chars); the compare lengths in the chain are 5 or 6. Do
  not assume 4-char codes (`chao2`, `dopa2`, `kjnf2`, `houu2` are 5).
* **Difficulty numbering.** `0..4` here, suffix `'1'..'5'` — the SAME numbering
  as the boot work item's `difficulty` and the per-side `side_info+4`, and it is
  level-only (mode-agnostic). Do not confuse with the 10-slot `idx = difficulty +
  mode*5` used for the music-DB writes, nor with the type-3 chart code
  (`0x0114`…) whose high byte is `04/01/02/03/06` for B/b/D/E/C.
* **Output cap.** `out` is 256 bytes; the writer truncates safely, but a hook
  writing the buffer directly must keep `strlen < 0x100` and NUL-terminate.
* **LayeredFS interplay.** The produced path is opened through AVS (`local`
  device) and is therefore subject to `avs_layeredfs` mod-folder replacement. A
  discovery index must union the stock directory with every mod folder's
  `mdb_apx/ssq/` (resolution order per `mod_paths`), so a `_N` file that exists
  ONLY in a mod folder is discoverable — and so LayeredFS then serves it.
* **fast_bootup cache.** Its per-item identity is keyed on the path the game
  REGISTERED (`identity.rs` reads the name record), so a resolver that changes a
  path simply produces cache misses for those items on the next boot — no schema
  change. The `plan.rs` flip-safety invariants already model split songs as
  distinct files per item.
* **`chart_length.rs`** only opens `<code>.ssq`; for split songs it computes
  lengths from the base file's charts alone. It should consume the same
  resolver (per-difficulty, or union of all files) once one exists.
* **AVS off-thread.** If discovery is done off the game thread, use host
  `std::fs` on resolved paths (AVS trampolines are game-thread-only —
  `per_song_judgement_offsets.md`).

## 9. Implementation Sketch (for the SDE — no code here)

1. **Signature**: `build_ssq_path` = §7.1 AOB (entry), with §7.2 as the derived
   alternate. Soft dependency — mod disables itself with one WARN if unresolved.
2. **Index** (built once at mod init, before `CheckStepDataActor::onInit` — i.e.
   during DLL `init()`, which precedes the boot screen): enumerate
   `data/mdb_apx/ssq/*_[1-5].ssq` in the stock directory and in every LayeredFS
   mod folder; for each file read only the chunk headers (12 bytes each, skip by
   `length`) and record the set of levels present (from type-3 `param2 >> 8`).
   Result: `HashMap<basename, [Option<u8>; 5]>` = per-difficulty chosen `N` via
   rule A. ~40 files, negligible cost.
3. **Detour** (`GenericDetour`, 3 args, void): look up `basename`; if the index
   has a file for `(basename, difficulty)` write
   `data/mdb_apx/ssq/<basename>_<N>.ssq`, else write the unsplit path. Optionally
   fall back to calling the original ONLY when the index itself failed to build
   (keeps stock behaviour on init failure). Never read `match+N`.
4. **Validation on the cabinet**: boot log must show 0 `INVALID SSQ` / `ME1529`
   lines with stock data (rule A reproduces stock), then place a newer revision's
   `<code>_3.ssq` + matching `musicdb.xml` entry and confirm the Expert chart
   loads on a binary whose chain lacks that code.
5. Follow-ups: route `chart_length.rs` through the resolver; keep
   `validate_musicdb.py`'s union-of-candidates check as the offline oracle for
   the rule.

## 10. Cross-Version Notes

* Function shape, ABI, string formats, and the two-stage predicate structure are
  identical on 20250805 / 20260224 / 20260721 / 20260825; only the cell count
  (19/27/35/35) and displacements differ. Effective mapping for any song present
  on two builds is identical on both.
* The `.rdata` song-code table is laid out in 8-byte cells directly after
  `"data/mdb_apx/ssq/"` on 20260721 (`0x180381B3C` = `"acef"`, cells step by 8,
  terminated by an all-zero cell before `"%s%s_%c.ssq"`). It is referenced only
  by the chain's `LEA RDI`s — there is no length or count anywhere; it is not a
  runtime table.
* No other function references the two format strings or the code table
  (Ghidra xrefs: `"%s%s_%c.ssq"` ← builder only; `"data/mdb_apx/ssq/"` ← builder
  ×2). The split-file knowledge lives nowhere else in `gamemdx.dll`.
