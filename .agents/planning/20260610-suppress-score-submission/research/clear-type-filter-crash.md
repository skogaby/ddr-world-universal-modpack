# Research: Clear-Type "FC" Filter Crash (series_expansion label-builder over-match)

**Status:** ✅ FIXED + cabinet-verified (2026-06-11). Root cause identified
(Ghidra) + corroborated by the maintainer's three-way live test; fix confirmed to
resolve the FC-selection crash on the reporter's install.

## Fix (landed)

`series_expansion.rs`: the `filter_label_builder_count` matches are now filtered by
`builder_seeds_with_ddr(match_addr)` before patching — only the VERSION builder
(the one whose enclosing function seeds its result string with the `"DDR "`
literal) is patched; look-alike sibling builders (Clear Type, etc.) are skipped
with a log line. `builder_seeds_with_ddr` scans a bounded window (`0xC0` bytes)
backward from the match for `LEA RDX, [rip+disp32]` (`48 8D 15`) whose decoded
target holds `"DDR "`. Cross-version-safe (no hardcoded address; decodes the
RIP-relative seed pointer; the `"DDR "` seed verified on both 0421 and 0526). No
signature change — the over-broad AOB stays, but its extra match is now discarded.

## Report

On the reporter's install (build = May/0526; properties.xml `soft_id_code` of
2026042100 is stale, the gamemdx.dll binary size matches 0526): selecting the
**Clear Type → FC** filter crashes immediately, even with no other filters.
**PFC and other clear types do NOT crash — only FC.**

Maintainer's three-way isolation test (decisive):
- Stock game, **no hook DLL** → no crash.
- Hook loaded, **series_expansion disabled** → no crash.
- Hook loaded, **series_expansion enabled** (even with a clean 2-series config) →
  crash. (So it's not the config; an earlier sparse/duplicate-series theory was a
  red herring.)

⇒ **series_expansion is the cause**, regardless of config.

## Crash (symbolized vs gamemdx 0526, base `0x7ff81a210000`)

Fault is in **`memcpy`** (`0x1802791c0`), all frames in gamemdx (none in our DLL).
Chain runs through the song-select **filter-summary text builder**
`FUN_18011a8f0` (builds the active-filter breadcrumb `"A / B / C"` + `"N Songs"`)
→ per-filter label fetch → `FUN_180122e70` (filter-result/label list builder,
seeds a std::string then loops a predicate) → `memcpy` on a bad string pointer.
Frame 7 (`+0x126170`) sits inside `FUN_180126090` — see below.

## Root cause: `filter_label_builder_count` signature over-matches

`series_expansion` patches "filter label builder" sites to redirect the label
table LEA at its own version-entry table (stride `ENTRY_STRIDE = 0x88`) and
overwrite the entry count (`MOV EDX, 9` → `VANILLA_ENTRY_COUNT + n_custom`). It
applies this to **every** match of `filter_label_builder_count` via
`get_all_matches`. The signature pattern hardcodes `BA 09 00 00 00` (`MOV EDX, 9`),
so it matches **any filter category whose builder has 9 entries** — and the
description even says "Multiple instances exist (one per filter category)."

There are **2 matches** (both builds):
| Build | VERSION builder (want) | Other 9-entry builder (wrongly patched) |
|---|---|---|
| 0526 | site `0x123856` in `FUN_180123790` | site `0x126155` in `FUN_180126090` |
| 0421 | site `0x123d06` in `FUN_180123c40` | site `0x126605` in `FUN_180126540` |

Both builders are near-identical (same `FUN_180122e70(buf, 9, …)` shape, parallel
lambdas). They differ by their **seed string literal**:
- VERSION builder seeds with **`"DDR "`** (`FUN_180003860(buf, "DDR ", 4)`) —
  `DAT_180370114` (0526) / `DAT_18036f0f4` (0421), both verified = `"DDR "`.
- The other builder seeds with the **empty string** (`FUN_180003860(buf,
  &DAT_1802dca70 /* "" */, 0)`).

So series_expansion **wrongly repoints the second (non-VERSION) category's label
builder at the version table**. When that category's filter is active, its
summary-label fetch indexes our version table at the wrong slot → bad/garbage
string pointer → `memcpy` over-read → crash. `FUN_180126090` (the mispatched
builder) is literally in the crash stack (frame 7).

**Why only FC (not PFC/others):** the mispatched builder is the **Clear Type**
category builder. FC's entry index into our mispointed version table lands on a
malformed/sentinel slot (we only populate `VANILLA_ENTRY_COUNT(9) + n_custom`
entries with a sentinel at the end); other clear-type indices happen to hit slots
whose bytes still form a non-faulting pointer. So the crash is value-specific to FC.

## Discriminator for the fix (cross-version verified)

The VERSION builder is the `filter_label_builder_count` site whose enclosing
function seeds the result string with the **`"DDR "`** literal (LEA → "DDR ";
`MOV r8d/EDX, 4`; `CALL FUN_180003860`). The crash builder seeds with the empty
string. Confirmed on both 0526 and 0421. So the fix can select the correct site by
checking, near each match, for the `"DDR "` seed (or its LEA), and patch only that
one — never the empty-seed sibling.

## Fix direction (to design)

`series_expansion` must patch **only the VERSION label builder**, not all 9-entry
builders. Options:
1. **Disambiguate at the match** — for each `filter_label_builder_count` hit, verify
   the enclosing builder seeds with `"DDR "` (scan backward for the seed LEA /
   the `FUN_180003860(.., "DDR ", 4)` call) and patch only that site. Drop the
   blanket `get_all_matches` patch-all.
2. **Tighten the AOB** — extend `filter_label_builder_count` (or add a VERSION-
   specific variant) so it only matches the `"DDR "`-seeded builder. Cleaner if a
   unique, stable pattern exists that includes the seed-string LEA.

Either way: **stop patching the empty-seed (Clear Type) builder.** Then re-test FC
+ PFC + the VERSION filter (custom series labels must still render) on both builds.

## Relationship to the other filter crashes

Separate bug from the FilterButton dangling-pointer crashes (open/close filter
menu). Those were `series_filter_scroll` holding freed pointers; this is
`series_expansion` corrupting an unrelated category's label table via an
over-broad signature. Same mod family (filter UI), different mechanism.
