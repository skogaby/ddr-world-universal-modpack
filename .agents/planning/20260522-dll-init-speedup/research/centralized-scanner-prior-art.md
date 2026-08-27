# Centralized Scanner Prior Art

Research for `.agents/planning/20260522-dll-init-speedup/`.
Goal: identify prior art for a single-pass, multi-pattern AOB scanner that
installs hooks as soon as their addresses resolve, in a Rust hook DLL injected
into a live Windows game process.

All sources cited below are public (this is a personal project; no Amazon
internal tools used). Items I could not verify are flagged inline.

---

## 1. Existing frameworks (game modding / DLL hooking)

### 1.1 spice2x (the closest analog)

- Repo: <https://github.com/spice2x/spice2x.github.io>
- License: GPL-3.0
- Language: C++ (continuation of SpiceTools)
- Relevance: spice2x is the same problem domain — it is the loader our DDR
  hook DLL is injected by — so its sigscan code is a direct prior art.

How patterns are registered: there is no registration phase. Each game
backend (`src/spice2x/games/<game>/*.cpp`) calls
`util::sigscan::find_pattern(...)` directly when it needs an address. There
is also a JSON-RPC API (`src/spice2x/api/modules/memory.cpp`) that lets
external clients do one-off `signature(dll_name, signature, replacement,
offset, usage)` requests, mutex-serialized via a file-scoped
`MEMORY_LOCK`.

Single-pass multi-pattern? **No.** The header at
`src/spice2x/util/sigscan.h` exposes `find_pattern`,
`find_pattern_from`, `replace_pattern`. Each takes exactly one
`pattern`/`mask`. The implementation in `sigscan.cpp` uses
`std::search` (naive O(n*m)) per pattern, and **copies the entire
module image into a fresh `std::vector<uint8_t>` on every call**. K
patterns means K image copies + K naive scans. No SIMD, no threading
inside the scan, only a single global mutex around the API entry
point. So if anything, spice2x is a worked example of what NOT to do
for our use case.

Sync or async install: synchronous; each game backend's entry point
walks its sigscans in order at startup.

### 1.2 Microsoft Detours

- Repo: <https://github.com/microsoft/Detours>
- License: MIT
- Wiki: <https://github.com/microsoft/Detours/wiki/OverviewInterception>

Pattern registration: not applicable — Detours hooks named/imported
functions, not AOB-scanned addresses. Relevant only for the *install*
side of the problem.

Batched installs: **yes, transactional.** The
`DetourTransactionBegin` / `DetourAttach` / `DetourTransactionCommit` /
`DetourTransactionAbort` API lets you queue multiple attaches between
Begin and Commit and apply them atomically (Commit suspends threads
once for the whole batch). The wiki explicitly lists these and
`DetourUpdateThread`. Couldn't pull the per-function reference page
(failed to render), but the API names plus the "Transaction" naming
make the batching contract clear.

Sync or async install: sync within a transaction. The whole transaction
commits at one point in time.

### 1.3 MinHook

- Repo: <https://github.com/TsudaKageyu/minhook>
- License: BSD-2-Clause
- Used by spice2x (vendored in `src/spice2x/external/minhook`) and many
  other tools.

Pattern registration: none — like Detours, hooks raw addresses.

Batched installs: **yes.** `MH_QueueEnableHook` /
`MH_QueueDisableHook` / `MH_ApplyQueued` exist precisely so you can
register many hooks individually then enable them in one
suspend-resume-all-threads pass. README explicitly recommends
queue+apply for multi-hook scenarios because every individual
enable/disable suspends and resumes all threads. `MH_ALL_HOOKS`
constant also affects every created hook in one call.

Sync or async install: sync, but the queue model lets you decouple
"create" from "apply", which fits our "register early, install when
resolved" pattern.

### 1.4 SafetyHook

- Repo: <https://github.com/cursey/safetyhook>
- License: BSL-1.0
- C++23, x86/x64, Zydis-based disassembly.

Batched/transactional installs: **not advertised** in the README. Hooks
are created individually with `safetyhook::create_inline(...)`. Notable
safety features it does have: pauses other threads during creation/
deletion, fixes IPs that landed inside the original prologue, widens
short branches, fixes RIP-relative displacements in relocated bytes.

AOB scanning: not built-in.

Relevance: a model for *safe* per-hook installation, not for batched
install. Closely matches what `retour` already does for our DLL.

### 1.5 REFramework (Capcom RE Engine mod loader)

- Repo: <https://github.com/praydog/REFramework>
- License: MIT (per repo metadata; not stated in the README excerpt I
  saw — verify before borrowing code).
- Inspired by and uses code from Kanan (cursey/kanan-new).

Pattern registration / scanner architecture: **could not verify from
the README alone.** README only describes high-level features. To get
a real answer would require reading `src/`, which I did not do here.
Flagging as "likely has a centralized sigscan helper" but not
confirmed.

### 1.6 BepInEx

- Repo: <https://github.com/BepInEx/BepInEx>
- License: LGPL-2.1
- Targets Unity Mono / IL2CPP / .NET. C# managed-side patching via
  HarmonyX, MonoMod, Cecil.

AOB scanning: **not part of the framework.** Patching is at the IL
level, not native memory. Plugins that need native hooks bring their
own scanner. Listed for completeness — not directly applicable to a
native Rust hook DLL.

### 1.7 Other game mod loaders investigated and dropped

- `Pizzaboxer/ryu-mod-manager`, `SilicaAndPina/Yakuza-Mod-Loader`,
  `Hexware-Land/yakuza-mod-loader`, `Wunkolo/Hookshot`,
  `SamboyCoding/AssemblyUnhollower` — all returned 404 at the URLs I
  tried. Either renamed, private, or my guesses were wrong. Did not
  pursue further; the four frameworks above already give us a complete
  picture of the design space.

### 1.8 findpattern-bench (benchmark, not a framework)

- Repo: <https://github.com/learn-more/findpattern-bench>
- License: MIT (project), individual `patterns/` entries vary.
- 16 different findpattern implementations benchmarked head-to-head
  against the same haystack with single-pattern + IDA-style wildcard
  patterns.

Useful as a sanity check on algorithm tradeoffs (see Section 3). Top
results from `results.txt` (2 patterns x 1024 iterations, lower is
better):

| Implementation                | Pattern 0 | Pattern 1 |
|-------------------------------|-----------|-----------|
| DarthTon v2                   | 7.4 ms    | (failed)  |
| Forza (SIMD With OpenMP)      | 7.9 ms    | 8.0 ms    |
| Forza (Boyer-Moore Variant)   | 33.7 ms   | 18.6 ms   |
| mrexodia (horspool)           | 59.8 ms   | 46.6 ms   |
| Michael                       | 117 ms    | 116 ms    |
| superdoc1234                  | 115 ms    | 115 ms    |
| fdsasdf                       | 115 ms    | 115 ms    |
| atom0s                        | 205 ms    | 206 ms    |
| DarthTon                      | 205 ms    | 203 ms    |
| dom1n1k_Patrick               | 247 ms    | 247 ms    |
| learn_more v2                 | 221 ms    | 229 ms    |
| stevemk14ebr                  | 433 ms    | 455 ms    |
| atom0s (mrexodia mod)         | 770 ms    | 773 ms    |

Takeaway: a SIMD scanner is ~30x faster than a Horspool variant and
~75x faster than the median naive scanner *for a single pattern*. A
Horspool variant is itself ~5x faster than naive. Multi-pattern was
not benchmarked here.

---

## 2. Rust crates for pattern scanning

### 2.1 `aobscan`

- crates.io: <https://crates.io/crates/aobscan> (page is JS-rendered;
  use lib.rs)
- lib.rs:    <https://lib.rs/crates/aobscan>
- GitHub:    <https://github.com/sonodima/aobscan>
- License:   MIT
- Last push: 2022-11-17 (low activity, 15 stars)

Features: Multi-format pattern parser — IDA-style with `?`/`??`,
code-style `\xAB\xCD` + mask `"x?x"`, hex string `"488b??????"`.
Builder API; `with_all_threads()` partitions the haystack across
threads. Optional `object-scan` feature for scanning a section of an
object file directly.

Wildcards: yes. Multi-pattern in single pass: **no.** One `Pattern`
per scan. SIMD: not advertised.

Performance claim from README: 10.17 GB/s avg / 12.41 GB/s peak
multi-threaded on Apple M1 Pro 10-core; 1.42 GB/s single-threaded.
The 10x MT speedup at 10 cores suggests this is naive-scan-with-thread-
parallelism, not a sophisticated algorithm. (For a hook DLL where we
want to be lean and avoid spawning threads early in the game's
lifecycle, that ratio is the wrong tradeoff.)

### 2.2 `patternscan`

- crates.io: <https://crates.io/crates/patternscan>
- lib.rs:    <https://lib.rs/crates/patternscan>
- GitHub:    <https://github.com/lewisclark/patternscan>
- License:   MIT
- Last release: 1.2.0, 2021-01-23 (effectively dormant)

Features: tiny (18 KB / 295 LOC), no runtime deps, `Read`-trait based.
Wildcards: yes, single `?` token only (e.g. `8d 11 ? ? 8f`).
Multi-pattern: **no.** SIMD: no. Threading: no.

Verdict: simple and dependency-free, but no advantage over what
`core/scanner.rs` already provides in this codebase.

### 2.3 `aho-corasick`

- crates.io: <https://crates.io/crates/aho-corasick>
- docs.rs:   <https://docs.rs/aho-corasick>
- GitHub:    <https://github.com/BurntSushi/aho-corasick>
- License:   MIT OR Unlicense (dual)
- Maintainer: BurntSushi (very active)

Features: linear-time multi-pattern matcher. Supports overlapping
matches, leftmost-first / leftmost-longest semantics, streaming.

Wildcards: **no.** Aho-Corasick is a literal multi-pattern matcher.
This is the central limitation — AOB patterns have `??` wildcards, so
the crate is not a drop-in solution. (Workaround: split each AOB into
its longest contiguous wildcard-free run, register those as needles
with Aho-Corasick to *prefilter* candidate positions, then verify each
candidate against the full pattern with mask. See Section 3.)

Multi-pattern: yes, that is its purpose.
SIMD: yes — uses the `packed` submodule (Teddy algorithm) as a
prefilter for small pattern counts.

### 2.4 `aho-corasick::packed` (Teddy)

- docs:      <https://docs.rs/aho-corasick/latest/aho_corasick/packed/>
- Algorithm: Teddy (originally from Hyperscan; ported to Rust by
  BurntSushi).

Features: SIMD-accelerated multi-substring search using PSHUFB-style
table lookups. Typically 10x faster than Aho-Corasick when applicable.

Wildcards: no. Multi-pattern: yes.
SIMD: yes (x86_64 + aarch64 only). Pattern count limit: ~100; build
becomes fallible when too many. Construction also fails on zero
patterns, any empty pattern, or when heuristics predict poor
performance — so it must be treated as a "best-effort" fast path.

Relevance: this is the right tool for the prefilter step (Section 3).

### 2.5 `memchr` / `memmem`

- docs:    <https://docs.rs/memchr>
- License: MIT OR Unlicense

Features: SIMD-accelerated single-byte (`memchr`), 2-byte (`memchr2`),
3-byte (`memchr3`), and single-substring (`memmem::Finder`) search.
SIMD on x86_64 / aarch64 / wasm32. No multi-needle, no wildcards.

Relevance: useful primitive — `memmem::Finder::new(needle)` is fast
enough that "build a Finder per pattern, run them all serially" beats
naive `std::search` by orders of magnitude. Could be an MVP step if
Aho-Corasick prefiltering proves overkill.

### 2.6 `pelite`

- docs:    <https://docs.rs/pelite>
- GitHub:  <https://github.com/CasualX/pelite>
- License: MIT

Features: zero-allocation PE32/PE32+ parser. Includes a `pattern`
module with rich syntax: exact bytes, skips/wildcards, "follow this
1-byte signed jump", "follow this 4-byte signed jump", pointer
following, return-from-jump-and-continue, and a save/capture array
(see `STACK_SIZE`, `save_len`). `pattern!` macro parses at compile
time.

Wildcards: yes. Multi-pattern: **no** (single pattern with optional
captures). SIMD: not advertised.

Relevance: the *pattern syntax* in pelite is roughly the kind of DSL
we'd want for a richer registration API (e.g. "scan for this AOB,
then RIP-relative-decode at +5, then capture the resulting address" —
see open question in `rough-idea.md`). We probably want our own
syntax driven by `core/signatures.rs` patterns, but pelite's
`Atom` enum is a useful design reference.

### 2.7 `goblin` (object-file parser)

Not directly relevant to the pattern-scanning side — listed in the
brief for completeness. `goblin` parses PE/ELF/Mach-O headers; it's
not a pattern matcher. We'd use it (or `pelite`) only if we wanted to
restrict scans to specific PE sections (`.text` only, etc.) instead of
the full module image. That's a real optimization but orthogonal to
the multi-pattern question.

### 2.8 Hyperscan / vectorscan (FFI option)

- Hyperscan:  <https://github.com/intel/hyperscan> — BSD-3, last
  release 5.4.2 (Apr 2023), now closed-source past 5.4.
- vectorscan: <https://github.com/VectorCamp/vectorscan> — BSD-3,
  open-source fork; ABI/API compatible with Hyperscan 5.4. Supports
  ARM NEON/ASIMD, Power VSX, ARM SVE2 in progress, SIMDe emulation
  for non-SIMD targets.

Features: PCRE-style regex multi-pattern matching, hybrid automata,
"up to tens of thousands of regular expressions" simultaneously,
streaming mode. Heavily SIMD (AVX2/AVX512/AVX512VBMI/NEON/SVE/VSX).

Wildcards: yes (PCRE `.`, `\xNN`, character classes, etc.).
Multi-pattern: yes, that is the design goal.

Relevance / concerns:
1. C library — would need FFI bindings (some `hyperscan-rs` crates
   exist on crates.io but I did not validate them here; flag for
   verification).
2. Build system: pulling Hyperscan into a `cargo-xwin` cross-compiled
   `cdylib` is a real engineering cost. The whole point of this DLL
   is to be small and load fast.
3. Pattern compilation is up-front and not free; for ~25-50 patterns
   compiled once at DLL init, it's fine, but it's overkill versus an
   in-tree scanner.

Verdict: powerful but heavy. Would only consider if the in-tree
approach proves insufficient.

---

## 3. Multi-pattern algorithms

For our use case (~25-50 AOB patterns, each 8-32 bytes typical, some
50+ bytes, sparse `??` wildcards, scanning ~5 module images of a few
MB to maybe 20-30 MB each), the candidate algorithms are:

### 3.1 Aho-Corasick with literal-prefix prefilter

The classic adaptation of Aho-Corasick to wildcard patterns: split
each AOB into its longest contiguous run of literal bytes, register
those literal runs as needles in a single Aho-Corasick automaton,
walk the haystack once, and at every needle hit do a full pattern
verification (literal bytes + mask) against the original AOB at the
implied start offset. Linear-time prefilter + O(matches * pattern_len)
verification.

Tradeoffs: Most AOB patterns have a fairly long contiguous literal
run (typical x64 instruction sigs have `??` only over operands —
8-16 contiguous literal bytes is the common case). For 25-50 patterns
the automaton fits in L2 cache. Verification false-positive rate is
low because longer literal substrings are rare. Best fit for this
codebase.

Implementation paths: (a) build it on top of the `aho-corasick`
crate (pre-existing SIMD via Teddy when ≤ ~100 patterns), (b)
hand-roll a simpler version using `memmem::Finder` per literal run
(works fine if patterns are few — *no* automaton, just a list of
finders, but you walk the haystack once per finder).

### 3.2 Teddy (SIMD packed multi-substring)

Teddy uses PSHUFB-based table lookups to test 16-32 bytes against a
small set of 1-4-byte-prefix needles per cycle. It is the hot core of
Hyperscan and the `aho-corasick::packed` submodule. Works only for
literal needles; same wildcard-handling story as 3.1 (split AOBs into
literal runs, use Teddy as the prefilter).

Tradeoffs: An order of magnitude faster than scalar Aho-Corasick on
modern x86_64 *when applicable*. Capacity capped at ~100 patterns,
prefers needles that share short prefixes for table density. Build
can fail (so we'd fall back to scalar Aho-Corasick, or to a
serial-finder loop, on the slow path). Available essentially for
free via `aho-corasick`'s prefilter — we don't need to call `packed`
directly.

### 3.3 Boyer-Moore-Horspool with mask

Per-pattern: precompute the bad-character shift table treating wildcard
positions as "any" (which forces shift=1 contributions for those
bytes — wildcards reduce skip distance). Skip-table lookup per
window, full compare on candidate.

Tradeoffs: Single-pattern, so K patterns = K passes over the haystack.
Excellent constant factors on patterns dominated by literal bytes
(see findpattern-bench: Forza Boyer-Moore variant ~5x faster than
naive on a single pattern; mrexodia Horspool ~3-4x faster). Very
simple to implement in safe Rust. **Loses the single-pass property
we want** but is a solid baseline.

### 3.4 SIMD multi-needle (custom, AVX2)

Pick a discriminating literal byte from each pattern (e.g. the rarest
byte in the pattern by frequency), build an AVX2 search that checks
32 input bytes against all of those discriminating bytes per cycle,
verify candidates. Essentially what Forza's "SIMD With OpenMP" entry
does in findpattern-bench (7-8 ms vs 200+ ms for naive — ~30x).

Tradeoffs: highest throughput, most code, cross-platform fragile
(requires runtime CPU feature detection and a scalar fallback). For
our use case this is what `aho-corasick::packed` (Teddy) gives us
already, with someone else maintaining it.

### 3.5 Rabin-Karp (multi-pattern hashing)

Rolling hash, K patterns, single pass; classic textbook approach.

Tradeoffs: not competitive in practice with either Aho-Corasick or
SIMD. Mentioned for completeness; do not pursue.

---

## 4. Async install patterns

How other tools handle "install hook X as soon as its address resolves,
even if other patterns are still scanning":

### 4.1 Detours / MinHook — transaction or queue, no streaming

Both batch installs but apply them at one synchronization point.
Detours' `DetourTransactionCommit` and MinHook's `MH_ApplyQueued`
are explicitly atomic-batch APIs because each install requires
suspending all threads. From the user's perspective there is no
"install as soon as resolved" — you're choosing the *single moment*
when all queued hooks become live, to minimize the number of
suspend/resume cycles.

Implication: if our scanner finishes all patterns in <5 ms anyway,
the right model is the same — collect addresses, install in one
batch, suspend threads once. The "race" we care about
(`musicdb.xml` allocation hook) reduces to "make the scan phase
finish before the game touches the allocation site" rather than
"install hook A before hook B's pattern is even scanned."

### 4.2 spice2x — sequential, per-game

No async at all. Each game backend runs its scans serially in the
init thread; install happens immediately after each scan resolves.
This is what we are doing today and what we want to improve on.

### 4.3 retour (Rust) — no batching

`retour::GenericDetour::enable()` is a per-hook call. There is no
transaction API in the documented surface. If we want a batched
install we either layer one on top, or accept that we install K
hooks one at a time and pay K * suspend-resume cost. (For ~50 hooks
this is still fast — single-digit ms — so practical impact is
small, but worth modeling.)

Verified by reading docs.rs/retour.

### 4.4 SafetyHook — per-hook with thread-pause hardening

Each `safetyhook::create_inline` already pauses other threads,
adjusts in-flight IPs, etc. But also no batched install. Same
tradeoff as retour.

### 4.5 General observation

I could not find a popular Windows hooking framework that exposes a
true "streaming install as patterns resolve" model. The dominant
patterns are:

1. **Batch-then-commit** (Detours, MinHook): collect, install at one
   point in time.
2. **Eager per-hook** (retour, SafetyHook): install when caller is
   ready, accept the per-hook overhead.

For our problem, model (1) plus a fast multi-pattern scan probably
dominates model (2) plus a streaming install — because the
suspend-resume cost amortizes better when batched, and "fastest
possible scan" is a more impactful lever than "first-result-first
install" given how short the scan window already is.

---

## 5. Recommendations for our use case

Inputs assumed: ~25-50 patterns, ~5 modules, each ~few MB to ~20-30
MB, AOB patterns 8-32 bytes typical with sparse `??` wildcards,
running inside the game's main thread shortly after DLL injection.

### 5.1 Strong recommendation: build it, don't import it

None of the off-the-shelf Rust crates fits cleanly:

- `aobscan` and `patternscan` are single-pattern.
- `aho-corasick` is multi-pattern but has no wildcard support.
- `pelite` has rich pattern syntax but is single-pattern.
- Hyperscan/vectorscan is heavy FFI.

The right shape for this codebase is: **registry of (pattern, mask,
post-match derivation, install callback) entries; one pass per
module using Aho-Corasick over wildcard-stripped literal runs as a
prefilter; verify on candidate hits; invoke the install callback as
each pattern resolves.** That's straightforward to implement in
~200-400 lines of Rust on top of the existing `core/scanner.rs`
primitives.

### 5.2 Prefilter algorithm: Aho-Corasick on literal runs

Use the `aho-corasick` crate (already MIT/Unlicense, tiny, no
problematic deps) for the prefilter. For each registered AOB, split
into contiguous literal runs, pick the longest one as the prefilter
needle, register a `(pattern_id, run_offset_into_pattern)` tuple.
Walk the module image once per module. On each Aho-Corasick hit,
back-compute the candidate pattern start, full-verify against the
pattern + mask, and if it matches, fire the install callback.

This gets us:
- Single-pass per module.
- Free SIMD via Teddy when pattern count ≤ ~100.
- Graceful scalar fallback when Teddy can't be built.
- Readable, no `unsafe` in the prefilter itself.

### 5.3 Install model: batch per module, not streaming

After all patterns for a given module are resolved, do one
`retour::GenericDetour::enable()` pass through them. Don't attempt
true streaming-install — the per-hook suspend-resume overhead at our
scale is small (sub-ms per hook), and the model is much simpler. If
profiling later shows install cost dominating, revisit by either
(a) wrapping a Detours-like transaction layer over `retour`, or (b)
moving to MinHook for `MH_QueueEnableHook` / `MH_ApplyQueued`. Both
are an extra dependency we don't need until measured.

The "install as soon as resolved" wording in `rough-idea.md` is
probably better restated as: **finish scanning module X before the
game touches the code path that depends on hook X**, not "install
hook X within microseconds of its match." The race we're losing
isn't "scanning vs. installing" — it's "scanning vs. the game
running ahead." Faster scan dominates.

### 5.4 Don't bother with multi-threaded scan

`aobscan` shows ~10x speedup with 10 threads. But we're scanning
~25-30 MB of code total; on modern hardware a single-threaded
Aho-Corasick + Teddy prefilter should clear that in well under 50 ms
total (Teddy throughput is multi-GB/s). Spawning a thread pool
during DLL init adds latency (TLS init, scheduler warm-up) and
fights spice2x's own init for cores. Single-threaded with a fast
algorithm wins on simplicity *and* probably on wall-clock for our
sizes. Revisit only if measurement shows scan time > ~50 ms.

### 5.5 Pattern DSL: keep `core/signatures.rs`'s string format

Don't introduce a pelite-style `Atom` enum unless we have a concrete
need (e.g. "follow this rel32 jump and continue matching"). For now,
the IDA-style `"48 8B ?? ?? ?? ?? 48 89"` format is universally
understood by reverse-engineers, parses trivially, and is what
`core/scanner.rs::scan_pattern` already accepts. *Post-match
derivation* (RIP-relative decode, RTTI walk, etc.) belongs in the
registration entry as a Rust closure, not in the pattern DSL.

### 5.6 Risks and tradeoffs to flag for design phase

1. **Teddy build fallibility.** The `aho-corasick::packed` builder
   can fail at runtime (zero patterns, empty pattern, too many
   patterns, heuristic). Make sure the registration layer never
   surfaces a build failure as a hard error — fall back to scalar
   Aho-Corasick automatically.

2. **Verification cost when many patterns share literal runs.** If
   two AOBs both have `48 8B 05` as their longest literal run, every
   `48 8B 05` in the haystack triggers verification for both. Not a
   real risk at our pattern count but worth noting during pattern
   curation.

3. **Module load timing.** The proposal assumes "scan when
   `gamemdx.dll` is detected and loaded." We need to be sure the PE
   image is fully loaded (all sections committed) before scanning,
   or pattern misses become a flaky failure mode. Today this is
   handled by `core/module_resolver.rs` polling. Keep that.

4. **`unsafe` discipline at the install boundary.** The install
   callback runs while other patterns may still be resolving. It
   must not assume the registry is locked or that other addresses
   are populated. Either install callbacks take only the resolved
   address, or we batch-install per-module after all of that
   module's patterns resolve (recommended; see 5.3).

5. **Dependency budget.** `aho-corasick` is small and well-vetted.
   Adding it for the prefilter is low risk. Avoid adding `pelite`,
   `goblin`, `hyperscan-sys`, etc. unless something forces it.

6. **Mod dependency graph.** `rough-idea.md` flags "one mod needs
   another mod's resolved address before it can install its own
   hook." A registry-keyed-by-pattern-id design naturally supports
   this: a derivation step can look up another already-resolved
   address by id (or take a callback that runs after a named
   pattern completes). Out-of-scope for the scanner itself, but
   shape the registration API so this is expressible.

---

## Sources verified

- <https://lib.rs/crates/aobscan>
- <https://github.com/sonodima/aobscan>
- <https://lib.rs/crates/patternscan>
- <https://github.com/lewisclark/patternscan>
- <https://docs.rs/memchr/latest/memchr/>
- <https://docs.rs/aho-corasick/latest/aho_corasick/>
- <https://docs.rs/aho-corasick/latest/aho_corasick/packed/index.html>
- <https://github.com/BurntSushi/aho-corasick>
- <https://docs.rs/pelite/latest/pelite/> (overview only;
  pattern syntax inferred from `pattern` module index)
- <https://docs.rs/pelite/latest/pelite/pattern/index.html>
- <https://docs.rs/retour/latest/retour/>
- <https://github.com/spice2x/spice2x.github.io>
- <https://raw.githubusercontent.com/spice2x/spice2x.github.io/main/src/spice2x/util/sigscan.h>
  (read via WebFetch, summarized)
- <https://raw.githubusercontent.com/spice2x/spice2x.github.io/main/src/spice2x/util/sigscan.cpp>
  (read via WebFetch, summarized)
- <https://raw.githubusercontent.com/spice2x/spice2x.github.io/main/src/spice2x/api/modules/memory.cpp>
  (read via WebFetch, summarized)
- <https://github.com/microsoft/Detours>
- <https://github.com/microsoft/Detours/wiki/OverviewInterception>
  (per-function reference pages did not render; transaction API
  inferred from sidebar listing)
- <https://github.com/TsudaKageyu/minhook>
- <https://github.com/cursey/safetyhook>
- <https://github.com/intel/hyperscan>
- <https://github.com/VectorCamp/vectorscan>
- <https://github.com/BepInEx/BepInEx>
- <https://github.com/learn-more/findpattern-bench>
- <https://raw.githubusercontent.com/learn-more/findpattern-bench/master/results.txt>

## Sources I could not verify

- REFramework's internal scanner architecture
  (`praydog/REFramework`): README only, did not read source.
- Yakuza mod loaders (`Pizzaboxer/ryu-mod-manager`,
  `SilicaAndPina/Yakuza-Mod-Loader`,
  `Hexware-Land/yakuza-mod-loader`): all returned 404.
- `Wunkolo/Hookshot`, `SamboyCoding/AssemblyUnhollower`: 404.
- Microsoft Detours per-function pages: failed to render in the
  wiki; transaction batching contract inferred from API names.
- Existence and quality of any `hyperscan-rs` / `vectorscan-rs`
  crate: not validated.
