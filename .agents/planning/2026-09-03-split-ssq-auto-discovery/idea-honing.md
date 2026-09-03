# Idea Honing — Split SSQ Auto-Discovery

Decision register. Ordered by blast radius. Status ∈ Proposed / Accepted /
Overridden / Assumed / Open.

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | Hook mechanism | Determines coverage + build stability | Full-replacement `GenericDetour` on `build_ssq_path`; when the index is built the original is NEVER consulted for the answer. Covers all 4 game consumers with one detour; needs no `match+N` reads (function body size varies per build). | Accepted 2026-09-03 |
| D2 | Discovery rule | Wrong file ⇒ boot-blocking `ME1529` | **Rule A**: for `(basename, d)`, pick the highest `N ≤ d+1` such that `<basename>_N.ssq` exists AND contains a type-3 chunk of level `d` (either mode); else the unsplit `<basename>.ssq`. Reproduces stock on the installed data (only divergence: `sabm` Challenge → `_5`, chunk-identical to stock's `_3`). Filename-only rules rejected: "exact `_N`" is wrong for 25 songs; "highest existing `_N`" has no content guarantee. | Accepted 2026-09-03 |
| D3 | Basename handling (`toho`) | Maintainer requirement | Resolver is **basename-opaque**: it looks up exactly the string the game passes and never consults `musicdb.xml`. `toho1..4` (randomized by the play sequences pre-call) index like any other code; with no `tohoN_*.ssq` on disk they resolve to `tohoN.ssq`, byte-identical to stock. If someone ships `toho2_3.ssq` it would be honored — correct by construction. | Accepted 2026-09-03 |
| D4 | Discovery sources | LayeredFS songs must work | Union of the stock `data/mdb_apx/ssq/` listing and every LayeredFS mod folder's `mdb_apx/ssq/` (`mod_paths::available_mods()`), filenames matching `*_[1-5].ssq`. For the content check, read the file LayeredFS would actually serve (`find_first_modfile` → else stock), so the index agrees with what the game loads. | Accepted 2026-09-03 |
| D5 | Fallback behavior | Fail-open | (a) `(basename, d)` not in index ⇒ write the unsplit path ourselves (same as stock for an unknown song). (b) Index failed to build (I/O error) ⇒ call the original — literal stock behavior, one WARN. (c) `d ∉ 0..4` ⇒ call the original. (d) Signature miss ⇒ mod not registered (`required_signatures`), which also covers third-party hex-edited 20250805 DLLs whose prologue is rewritten. | Accepted 2026-09-03 |
| D6 | Index build timing | Must precede the boot pass's ~7200 calls | Built **synchronously in `enable()`** (≈40 files, 12-byte chunk-header walks only — milliseconds), rebuilt on every enable so a mod-menu toggle re-scans. No background thread: the first builder call arrives with `CheckStepDataActor::onInit` shortly after enable and a lost race would silently give stock behavior. | Accepted 2026-09-03 |
| D7 | Configuration | Maintainer requirement | None. Single `mods["split-ssq-auto-discovery"]` toggle, default ON. Live-toggleable (disable removes the detour; already-registered boot paths are unaffected until next boot). | Accepted 2026-09-03 (maintainer requirement) |
| D8 | Divergence diagnostics | Cabinet validation of the "reproduces stock" claim | In the detour, ALSO call the original into a scratch buffer and log one INFO per distinct `(basename, d)` where our answer differs from stock (capped at 64 lines/session). Cost: one cheap string-chain call per builder call (~µs × 7200 at boot). Turns "did the mod change anything?" into a log grep. | Accepted 2026-09-03 |
| D9 | Signature | Cross-build | `build_ssq_path` = the 32-byte entry AOB (RE §7.1; unique on all 4 builds), in `required_signatures`. The `"%s%s_%c.ssq"`-xref derivation (RE §7.2) is documented but NOT implemented now — no build needs it. | Accepted 2026-09-03 |
| D10 | `chart_length.rs` | The one DLL-side path builder outside the detour | **Out of scope** — the LENGTH readout for split songs is computed from the base file's easy charts, which share the song's length in practice. Recorded as a follow-up; the resolver's public `resolve(basename, d)` makes it a one-line change later. | Accepted 2026-09-03 |
| D11 | Level check semantics | Mode-agnostic builder | A `_N` file "contains level d" if it has a type-3 chunk with `param2 >> 8 == {0x04,0x01,0x02,0x03,0x06}[d]` for EITHER mode (`14`/`18` low byte). Matches the on-disk shape (each `_N` holds both modes). | Assumed |
| D12 | Hot-path shape | ~7200 calls in one frame | Index = `HashMap<Box<[u8]>, [Option<u8>; 5]>` keyed on the raw basename bytes; per call: bounded `strlen` (≤ 0x20), one lookup, one bounded `write!` into the 0x100 buffer, NUL-terminate. No allocation, no logging on the common path (divergence log only when differing, deduped). | Assumed |
| D13 | Pure/impure split | Host-testable per repo convention | `resolver.rs` (pure: `build_index(listing: &[(basename, N, levels)]) -> Index`, `Index::resolve(basename, d) -> Choice`) with host tests that reproduce the RE §4.1 stock table from the RE §6 file listing; `discovery.rs` (impure: dir scan + chunk-header read via `core/ssq`); `mod.rs` (lifecycle + detour). Multi-file ⇒ `src/mods/split_ssq_auto_discovery/`. | Assumed |
| D14 | fast_bootup interplay | Cache correctness | No change: the analysis cache is keyed per registered path; a path that differs from the previous boot's is a per-item miss (stock analysis + re-capture). Self-heals; no invalidator change. | Assumed |
| D15 | Boot-pass thread | Detour runs on the game thread inside `onInit` | Pure map lookup + stack formatting; no AVS calls, no locks beyond a `OnceLock`/`RwLock` read of the index. Index built on the enabling thread before the detour installs. | Assumed |

## Readiness Confirmed 2026-09-03

Register approved wholesale by the maintainer (D1–D10 Accepted, D11–D15 Assumed). No research step required — `docs/split_ssq_research.md` answers every unknown. Proceeding to design.

## Detail

### D1 — Hook mechanism
The RE (`docs/split_ssq_research.md` §7.3) established nothing past the
prologue is stable (`0x3A9`→`0x70F` body). Calling the original then "fixing up"
its answer was considered and rejected: for the target case (song unknown to the
binary) the original returns the base file, so we would need the index anyway;
replacement is simpler and the original still serves as the diagnostic oracle (D8).

### D2 — Rule A
See `docs/split_ssq_research.md` §6.1 for the simulation over the 39 installed
split files. Rule A's content check is the safety property: it can never name a
file lacking the requested level, which is the one outcome that raises the
boot-blocking corruption error.

### D3 — toho
`DancePlaySequence::onSetup` / `MatchingDancePlaySequence::onSetup` /
`PlayerCourseWork::prepare` all do `snprintf(buf, 8, "toho%d", (rand&3)+1)` for
mcode `0x939D` before the builder call (RE §8). The resolver must not assume the
basename is a `musicdb.xml` entry — and does not.

### D8 — Divergence log
Expected steady state on a matched binary/data pair: zero lines (or exactly the
`sabm d=4: _5 vs stock _3` line). On the target scenario (old binary, new data):
one line per newly-discovered split (song, difficulty). Both are the validation
signal for the cabinet deploy.
