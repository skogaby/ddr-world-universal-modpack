# Plan — split-ssq-auto-discovery

Status: Approved 2026-09-03 (inherits the approved PDD plan; maintainer authorized auto-run)

## Test scenarios (resolver.rs, host harness)
1. parse_split_filename: accept `casr_3.ssq`→(casr,3), `dopa2_5.ssq`→(dopa2,5); reject `casr.ssq`,
   `casr_6.ssq`, `casr_0.ssq`, `casr_33.ssq`, `_3.ssq`, `casr_3.SSQ`, `casr_3.ssqx`.
2. levels_in_blob: synthetic blob [type1][type3 0x0314][type3 0x0618] ⇒ bits 3,4; sentinel 0xFFFF
   stops; zero-length terminator stops; truncated header tolerated; malformed length stops.
3. Stock-table fixture: 39 installed files' level sets (RE §6) ⇒ resolve equals RE §4.1 for every
   (song,d) with a chart; `sabm d4 ⇒ Split(5)`; toho1..4/unknown ⇒ Base; acef [1..5]; rabb
   [B,B,B,4,5]; hkhk d1 ⇒ Base.
4. Index::build merges duplicate (basename,n) by OR.
5. format_path exact bytes for Base/Split + NUL; false on overflow (cap too small).
6. paths_differ NUL-aware compare (Step 3).
7. discovery::collect_from_listing dedupes across sources and ignores non-matching names (Step 2).

## Implementation shape
Per design §Components. Callback panic-free; oracle compare added in phase 3.
