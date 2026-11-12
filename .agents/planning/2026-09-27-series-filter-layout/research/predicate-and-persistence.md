# Research: VERSION predicate, table readers, summary chip, persistence, thumbnail loop

Read-only RE for the enhanced (config-driven) VERSION layout. Sources: Ghidra
(`gamemdx_20260915.dll` for semantics), raw-byte/capstone sweeps of all five supported builds in
`~/Desktop/ddr_modules/`, `scripts/sig_harness/shape_diff.py` run on a hand-built sweep JSON, and
a read-only listing of `$DDR_WORLD_INSTALL/data/arc/thumbnail/`. Addresses are file-relative to
`0x180000000`; "20260915" unless a build is named.

## TL;DR

| Plan assumption | Verdict |
|---|---|
| Second compare disp32 `0xB8` can be repointed to `+0x34` | **Holds.** Same instruction, same offset (`match+0x34`), identical bytes on all 5 builds; only the LEA disp32 differs. |
| `+0x34` of a VERSION entry is unused padding | **Holds.** No reader or writer anywhere (Ghidra xrefs + full RIP-relative sweep, all builds). Stock value is 0 (zero-init `.data`). |
| Nothing else relies on `table[i+1].start` | **Holds.** Only the predicate's second CMP. |
| Summary builder only reads table strings | **Holds.** Copies `+0x38` / `+0x60` via `basic_string::assign(const string&, 0, npos)`; never writes/destroys. Non-SSO strings in mod memory are safe. |
| Count N ≤ 64 is required and sufficient for persistence | **Holds.** Load uses `SHL RDX,CL` (count mod 64 → aliasing), save uses `ROL RDI,1` + `ADD` (wrap + carry corruption). 64 is exact. |
| Summary builder count = N | **Holds, with a caveat:** its loop is `0..=count` (inclusive) — keep a valid sentinel row at index N. |
| Thumbnail bound "255 crashed because of too many AVS opens" | **Contradicted / better explanation:** `CMP RSI,imm8` sign-extends and `JBE` is unsigned — any imm8 ≥ `0x80` never terminates. Hard cap is `0x7F`. (Legacy code has this latent bug for `series_value ≥ 128`.) |
| Rows are "raw-series ranges" | **Partly:** the predicate compares the *mapped* value; raw 16 → 15 stays merged unless one more 4-byte jump-table patch is applied (§4). |

## 1. Predicate shape (Q1)

`version_predicate_lea` (`48 8B 50 08 48 8B 0A 48 3B CA 74 ? 4C 8D 05`) is **not unique**: 4
hits on 20250805/20260721/20260825/20260915, 5 on 20260224. The resolver keeps the **first** hit
(`scan_patterns_batch`, first-match-per-name), which is the VERSION predicate on every build
(verified: its function calls the series mapper at `match−0x22`). The other hits are sibling
predicates (on 20260915: LEVEL `FUN_1801259F0`, FLARE `FUN_180125E20`, CLEAR `FUN_180126740`).

| Build | Predicate fn | Match | LEA R8 (+0x0C) | CMP2 (+0x30) | disp32 (+0x34) | VERSION table | Mapper (call @ −0x22) |
|---|---|---|---|---|---|---|---|
| 20250805 | `0x180117070` | `0x1801170A2` | `0x1801170AE` | `0x1801170D2` | `0x1801170D6` | `0x180CB8FF0` | `0x1800F3E40` |
| 20260224 | `0x180119A90` | `0x180119AC2` | `0x180119ACE` | `0x180119AF2` | `0x180119AF6` | `0x180CCCE20` | `0x1800F5F40` |
| 20260721 | `0x180124140` | `0x180124172` | `0x18012417E` | `0x1801241A2` | `0x1801241A6` | `0x180CF62D0` | `0x1800FF9C0` |
| 20260825 | `0x180123C60` | `0x180123C92` | `0x180123C9E` | `0x180123CC2` | `0x180123CC6` | `0x180CF6280` | `0x1800FF6B0` |
| 20260915 | `0x180123E40` | `0x180123E72` | `0x180123E7E` | `0x180123EA2` | `0x180123EA6` | `0x180CF6270` | `0x1800FFCB0` |

Bytes `match+0x00 … +0x39` (58 bytes), identical on all five builds except `+0x0F..+0x12`:

```
+00  48 8B 50 08                 mov  rdx,[rax+8]          ; list head (std::list<int>)
+04  48 8B 0A                    mov  rcx,[rdx]
+07  48 3B CA                    cmp  rcx,rdx
+0A  74 36                       je   no_match
+0C  4C 8D 05 ?? ?? ?? ??        lea  r8,[rip+table]       ; disp32 @ +0x0F (existing patch)
+13  66 66 66 0F 1F 84 00 00 00 00 00   nop (align)
+1E  48 63 41 10                 movsxd rax,[rcx+0x10]     ; selection index
+22  48 69 C0 88 00 00 00        imul rax,rax,0x88         ; stride imm32 @ +0x25
+29  42 39 7C 00 30              cmp  [rax+r8+0x30],edi    ; start (disp8 0x30 @ +0x2D); edi = mapped value
+2E  7F 0A                       jg   next                 ; signed: start > v → skip
+30  42 3B BC 00 B8 00 00 00     cmp  edi,[rax+r8+0xB8]    ; disp32 @ +0x34  ← patch B8 → 34
+38  7C 15                       jl   match                ; signed: v < end → true
```

- First compare `+0x30` (disp8) and stride `0x88` are identical on all builds. Compares are
  **signed i32**: a row matches iff `start <= v && v < end`. Inclusive `[s, e]` → store
  `start = s`, `end = e + 1` (256 max, fits). An inert row: `start = i32::MAX`, `end = 0`.
- The patch is a pure disp32 rewrite: `B8 00 00 00` → `34 00 00 00` at `match+0x34`
  (`42 3B BC 00 34 00 00 00` is valid, no re-encode).
- **Shape check** (fail-closed): compare `match+0x00..+0x39` literally against the block above,
  masking only `+0x0F..+0x12`. Recommended extra anchor: `E8 rel32` at `match−0x22` targets
  `series_mapper_bounds − 0x5D` (mapper start on all 5 builds), which proves the first hit is the
  VERSION predicate. The existing `lea_ok`-style check that the LEA target equals the summary
  builder's LEA target still applies.
- Alternative: the full 58-byte string above (with the LEA disp32 wildcarded) is **unique on all
  five builds** and resolves to the same address — a candidate replacement/companion signature
  (`version_predicate_range`) that removes the first-match dependency.
- `shape_diff.py --ref 20260915 --window 0x3A` on the five matches: identical (`=` on every build).
  `thumbnail_arc_loop`, the `"DDR "` `filter_label_builder_count` site, `filter_entry_count_table`
  and `series_mapper_bounds` are also identical through `+0x10`.
- Ordering: apply the disp32 patch together with (or before) the LEA redirect; restore in reverse.
  With only the LEA redirected, `+0xB8` would read the *next* mod row's `+0x30`.

## 2. Every reader/writer of the VERSION entry table (Q2)

Method: Ghidra references to every byte of `[0x180CF6270, 0x180CF67C0)` (10 × 0x88; TITLE's
table starts exactly at `+0x550`) plus a capstone linear sweep of `.text` for RIP-relative operands
landing in the range. Both agree; every build has **169** refs with the same shape.

| Site (20260915) | Function | Access |
|---|---|---|
| `0x18012048B … 0x180120A5C` | filter init `FUN_18011F600` | Writer: `+0x00` group (dword), `+0x08/+0x38/+0x60` SSO string init + `assign`, `+0x30` start (dword). **Never `+0x04` or `+0x34`.** |
| `0x180123E7E` | predicate `FUN_180123E40` | `LEA R8,[table]`; reads `[i*0x88+0x30]` and `[i*0x88+0xB8]` (= next entry's `+0x30`). Only consumer of the contiguous-range assumption. |
| `0x180123F3F` | VERSION summary lambda `FUN_180123ED0` | One `LEA RCX,[table]`, stored into both label-lambda captures (lambda57, lambda58). Reads `+0x38` / `+0x60` only (§3). |
| `0x18012446E` | builder `FUN_180124220` | `LEA RBX,[entry8+0x08]`, walks back by 0x88, reads the key string only (`CMP [RBX+0x18],0x10` / `MOV R9,[RBX]`). Replaced by the planned builder detour. |
| `0x18029F6D0 … 0x18029F8E0` | EH unwind funclets of the init | Destroy partially built strings if init throws. |
| `0x1802D5BFC` | atexit dtor | `eh vector destructor iterator(table, 0x88, 10, dtor)` — stock table only. |

Per-build equivalents: predicate LEA / summary LEA / builder LEA RBX / atexit:
20250805 `0x1801170AE` `0x18011716F` `0x18011769E` `0x1802B760C`; 20260224 `0x180119ACE`
`0x180119B8F` `0x18011A0BE` `0x1802BE8CC`; 20260721 `0x18012417E` `0x18012423F` `0x18012476E`
`0x1802D606C`; 20260825 `0x180123C9E` `0x180123D5F` `0x18012428E` `0x1802D5B4C`.

Not readers: the VERSION sorter (id 4) and its song-wheel headers use the mapper and the per-song
name table (§4), not this table; the group-tab press reads the **group** table `0x180CF3E40`;
`FilterCard` only invokes the summary `std::function`. No code reads `+0x34` of any entry, and no
table pointer escapes into other captures.

## 3. Summary chip builder (Q3)

`FUN_180123ED0(capture{?, FilterManager* @+8, category @+0x10}, std::string* out)`:
1. `out = "DDR "` — literal at `0x180371164` (`FUN_180003990(out, "DDR ", 4)`); this is the
   seed `builder_seeds_with_ddr` finds.
2. Builds three `std::function`s: lambda56 `is_selected(i)` (vtable `0x180371EB8`, call slot
   `FUN_1801310A0`: `find(selection_list[cat], i) != end`), lambda57 `first(i)` (vtable
   `0x180371EF0` → `FUN_180127770`: copy of `table[i]+0x38`), lambda58 `last(i)` (vtable
   `0x180371F28` → `FUN_1801277C0`: copy of `table[i]+0x60`).
3. `FUN_1801235B0(tmp, count=9 (MOV EDX,9 @ 0x180123FA3), is_selected, first, last)`, then
   `out.append(tmp)`.

Run builder `FUN_1801235B0` (20250805 `0x1801167E0`, 20260224 `0x180119200`, 20260721
`0x1801238B0`, 20260825 `0x1801233D0`):
- Loop `i = 0 ..= count` (**inclusive**: `CMP EDI,[RSP+0x40]; JLE` at `0x1801238CF`, same on all
  builds). The stock call therefore probes the sentinel index 9.
- For selected `i`: if the run vector is non-empty and `i != prev+1` → flush. Push `first(i)`;
  push `last(i)` only if it differs from `first(i)`. `prev = i`.
- Flush `FUN_180127570`: runs joined by `", "` (`0x18037120C`); a 1-element run prints `"%s"`
  (`0x1802E1324`, the element), otherwise `"%s～%s"` (`0x180371210`, SJIS `81 60` fullwidth
  tilde) with the vector's **front** and **back**.
- The label lambdas copy with `FUN_180003530` = `basic_string::assign(const string&, pos, n)`:
  reads `src+0x10` (size) / `src+0x18` (cap) / buffer, never writes the source. The copy lives on
  the game heap and is freed by the game. Lambda bodies are unique per build (IMUL sites:
  20250805 `0x18011A969`/`0x18011A9B9`, 20260224 `0x18011D359`/`0x18011D3A9`, 20260721
  `0x180127A99`/`0x180127AE9`, 20260825 `0x1801275B9`/`0x180127609`, 20260915
  `0x180127799`/`0x1801277E9`), all stride 0x88, `+0x38` / `+0x60`.

With the mod table (index = selection index, `+0x38 = label_lo`, `+0x60 = label_hi`) and count N:
- one selected row with `lo == hi` → `DDR lo`; with `lo != hi` → `DDR lo～hi`;
- adjacent selected rows `a..b` → `DDR lo_a～hi_b`; gaps → `DDR lo_a～hi_b, lo_c～hi_d`.
- Requirements: table must hold **N + 1** entries (index N = sentinel with valid empty SSO
  strings), since index N is probed; the table (and its strings) must never be freed while the
  process lives — the captures copy the table pointer into `std::function`s.
- Caveat: merging follows **selection-index adjacency**, not range adjacency — config rows
  that are adjacent but semantically unrelated merge into a misleading `lo_a～hi_b`.

## 4. Series mapper `FUN_1800FFCB0` (Q4)

Real callers (Ghidra + E8 scan): only two.

| Caller | Role |
|---|---|
| `0x180123E50` in the VERSION predicate | filter value (`EDI`) |
| `0x180192FD3` in `FUN_180192FB0` | the "thin wrapper": `_Do_call` of `_anon_6457BC0D::<lambda2>` (`int(shared_ptr<ChartMetadata>)`, vtable `0x18037E7C0`) = the **VERSION sorter's int key** (sort/grouping). Its sibling lambda3 (`FUN_1801930A0` → `FUN_180191F60`) makes the `"Version / %s"` header from the per-song name accessor (vtable `+0xA8`, raw-indexed table — the `series_label_lookup_*` patch site), not from the mapper. |

The other two "xrefs" (`0x181291A0C`, `0x18040763C`) are `.pdata` / EH data.

Mapping (jump table `0x1800FFDD8`, module-base-relative, 21 entries for raw 1..21): raw 1–15 and
17–21 identity; **raw 16 → target of raw 15 (`MOV EAX,0xF`)**; raw 0 → `DEC` underflows →
default. With the existing default patch (`XOR EAX,EAX` → `MOV EAX,ESI`, ESI = `MOVZX` raw), raw 0
→ 0 and raw ≥ 22 → raw. Same layout on all builds (default / jump table):
20250805 `0x1800F3F53`/`0x1800F3F68`, 20260224 `0x1800F6053`/`0x1800F6068`, 20260721
`0x1800FFAD3`/`0x1800FFAE8`, 20260825 `0x1800FF7C3`/`0x1800FF7D8`; derivable from the
`series_mapper_bounds` match: JA rel32 @ `+0x0B` → default, `8B 8C 82 disp32` @ `+0x18`
(disp32 @ `+0x1B`) → jump-table RVA.

Leaving the mapper as is breaks nothing else (two consumers only). The only semantic gap for
"raw ranges": raw 16 is indistinguishable from 15 — a row `[16,16]` matches nothing, and any row
containing 15 also matches raw-16 songs. Optional fix: set jump-table entry `[15]` (raw 16) to
the default-case RVA; with the default patch that returns 16. Side effect: the sorter key splits
15/16 into two groups (headers still both read "…2014"). Must be a checked patch (entry[15] ==
entry[14] and default bytes `31/33 C0` or already `89 F0`).

## 5. Persistence with count > 9 (Q5)

- **Count fn** `FUN_1801D55B0` (`filter_entry_count_table`): exactly **two callers** on every build
  (Ghidra + E8 scan; no pointer refs): the FilterManager-ctor load loop and the save mask builder.
  Neither sizes an array by the count. Categories 1 and 12 share one switch arm (the detour keys
  on 1 only).
- **Load** (`FUN_1800FD2F0`, loop at `0x1800FD696`): for `cat = 0..12`, `mask` = u64 from
  player-work `std::map<int,u64>` (`+0x1760/+0x1768`); for `i < count(cat)`:
  `MOV EDX,1; SHL RDX,CL` (`0x1800FD6B7`) → `set(state, cat, i, (mask & bit) == bit)` for every
  i (clears unset bits too). 64-bit SHL masks CL to 6 bits → index `64+k` mirrors bit k
  (phantom selections).
- **Save** (`FUN_1801D58A0`, caller `FUN_1800FDF70` writes the u64 back into the player-work
  map for categories 0..12): `bit = 1; for i < count: if selected(i) acc += bit;
  ROL RDI,1` (`0x1801D5911`). Index `64+k` adds bit k again → aliasing, and a carry into bit
  k+1 when both are selected.
- Per build (count fn / save builder / load call site / SHL / ROL): 20250805 `0x1801BF3A0`
  `0x1801BF690` `0x1800F1796` `0x1800F17B7` `0x1801BF701`; 20260224 `0x1801C2760` `0x1801C2A50`
  `0x1800F3876` `0x1800F3897` `0x1801C2AC1`; 20260721 `0x1801D5750` `0x1801D5A40` `0x1800FD366`
  `0x1800FD387` `0x1801D5AB1`; 20260825 `0x1801D5C50` `0x1801D5F40` `0x1800FD096` `0x1800FD0B7`
  `0x1801D5FB1`. Identical `ADD RBP,RDI; ROL RDI,1` everywhere.
- ⇒ **cap N at 64: required and sufficient** on the game side (0..63 map 1:1 onto bits 0..63).
- Other selection-list consumers (`FUN_1801D59E0` callers) don't assume ≤ 9: active-filter set
  (`FUN_180100AD0`, non-empty test), list-size (`FUN_1801304A0`), lambda95 is-selected, CLEAR RANK
  remap, set/clear/clear-all. `FUN_1801D5680` is an unbounded `std::list` insert/erase.
- **Unbounded reader:** the predicate reads `table[i]` for *any* selected index. Stock paths only
  create indices `< count`, **except the stock group-tab press** (`FUN_180127810`), which selects
  from the stock group table (indices up to 8). If that handler is not replaced in enhanced mode
  and N < 9, the predicate reads past the mod table. Mitigation: replace/detour the tab press (as
  planned) **and** pad the table to `max(N, 9) + 1` rows with inert rows.

## 6. Thumbnail loop `FUN_18003C270` (Q6)

`sequence::common::ThumbnailLoadActor` (vtable `0x18035EF78`): slot 4 = `FUN_18003C270`
(request), slot 5 = `FUN_18003C500` (release all), slot 6 = `FUN_18003C440` (update; calls
`FUN_18003C550` to free the raw file buffers of loaded ids). Per build: fn / `INC RSI` match
(imm8 at `+6`): 20250805 `0x18003BAA0`/`0x18003BC31`, 20260224 `0x18003B3C0`/`0x18003B551`,
20260721 `0x18003B700`/`0x18003B891`, 20260825 `0x18003BD10`/`0x18003BEA1`, 20260915
`0x18003C270`/`0x18003C401`. Code identical across builds.

Per N (`RSI = 0 ..= imm8`, unsigned `JBE`):
1. `sprintf("data/arc/thumbnail/jacket_thumbnails_%s_%d.arc", region, N)`; region `"ua"` if the
   area query returns 1 or 4, else `"ja"`.
2. `id = FUN_1801FEBF0(resmgr, path)` — **registers** the path in the global resource manager
   (`DAT_1806F2F48`, 4096-slot pool built by `FUN_1801FD210` with `0x1000` from `FUN_1800020D0`),
   deduped by FNV-1a hash (refcount++ if present). It does **not** check existence. Returns −1
   **only when the pool is exhausted** — and on that path it has already `memcpy`'d 0xA0 bytes to
   `desc[−1]` and written a 0x40 record at `slot[−1]` (heap corruption).
3. If the descriptor type is empty, set it to `"thumbnail"`.
4. `slot[id].+0x38 = 0xFFFFFF9E` (−98 = I/O priority handed to the reader). With `id = −1`
   this is a fixed write at `slot_array − 8` (the question's OOB write) — real, but reachable
   only after pool exhaustion.
5. `ids.push_back(id)` (`ThumbnailLoadActor+0x88`).

Missing arcs are handled gracefully by the loader (`FUN_1801FD8B0`): the reader object is always
allocated (`FUN_1801FE5C0`), size query returns 0 → slot state 6 (failed), reader destroyed;
`FUN_18003C550` skips slots with size 0. New opens are throttled to `resmgr+0x70` = 4 per
pump (config default from `FUN_1801FCFD0`). Each extra N costs one resource slot for the session
and one failed async open.

**Why 255 crashed:** `48 83 FE imm8` is `CMP RSI, sign-extended imm8`, followed by unsigned
`JBE`. imm8 `0xFF` → compare with `0xFFFF…FFFF` → the loop never ends: it registers paths
until the 4096-slot pool is exhausted, then keeps corrupting `desc[−1]` / `slot[−1]` /
`slot_array−8` and growing the id vector. Any imm8 ≥ `0x80` behaves the same. **Max safe imm8 =
`0x7F`.** (The legacy mode writes the max `series_value` u8 unchecked — a custom value ≥ 128 would
hang boot.)

Consumers: the id vector is used only for lifetime (slots 5/6); the format string has a single
xref; nothing indexes thumbnails by series number, so N > 21 only matters if an arc with that
number exists. How a song's thumbnail is looked up inside the loaded arcs was not traced (not
needed: it can't reach the id vector).

Arcs present: `$DDR_WORLD_INSTALL/data/arc/thumbnail/jacket_thumbnails_ja_{0..21}.arc` (22 files,
`ja` only, sizes track per-series song counts — N is the raw series); **none** under
`$DDR_WORLD_INSTALL/data_mods/*/` and none in the repo.

**Recommended policy (enhanced mode):** don't derive the bound from row ranges (a catch-all row
like 22–255 would otherwise force 255). Bound = the highest N in `22..=0x7F` for which
`jacket_thumbnails_<region>_<N>.arc` exists (stock `data/` or any LayeredFS mod folder, region
chosen like the game), else leave `0x15` unpatched. Always clamp the imm8 to `0x7F`. Gaps
between 21 and the bound are harmless (failed loads). Apply the same clamp to the legacy path.

## 7. Risks and unknowns

1. `version_predicate_lea` is non-unique; correctness depends on first-match ordering. Use the
   58-byte shape check + mapper-call anchor, or the unique long signature (§1).
2. Stock group-tab press selecting indices ≥ N (§5) — predicate OOB read unless replaced and/or
   the table is padded with inert rows.
3. Summary loop is `0..=N`: sentinel row N required; table must outlive every `std::function`
   copy (never free on disable).
4. Run merging by selection-index adjacency can produce misleading chips for non-contiguous
   config rows.
5. Mapper 16→15 merge: rows are effectively over *mapped* values unless the jump-table patch
   (§4) ships.
6. Label strings are rendered by the chip font; the builder's own tilde is SJIS — non-ASCII
   labels must be SJIS, not UTF-8 (not verified beyond the separator literal).
7. Network servers may not round-trip `filtersort/version` bits ≥ 9 (unverified, as before).
8. Saved masks are keyed by selection index: reordering config rows silently changes users'
   saved VERSION filters.
9. AVS / LayeredFS behaviour for many failed opens was not analysed; the imm8 explanation covers
   the reported 255 crash, but keep the bound minimal anyway.
10. Pool-exhaustion path of `FUN_1801FEBF0` corrupts memory; any future feature that registers
    many resources inherits this.
