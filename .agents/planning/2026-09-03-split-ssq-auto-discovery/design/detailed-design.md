# Split SSQ Auto-Discovery — Detailed Design

Status: Approved 2026-09-03

## Overview

DanceDanceRevolution World stores a song's charts in one SSQ file,
`data/mdb_apx/ssq/<basename>.ssq`, except for songs whose difficulties carry
different tempo data: those are split into `<basename>_<N>.ssq` files
(`N` = 1..5 = Beginner, Basic, Difficult, Expert, Challenge), the base file keeping
the easy charts and the `_N` files the hard ones. The decision "which file holds
`(basename, difficulty)`?" is made by a single game function, `build_ssq_path`,
whose body is a hardcoded string-compare chain that grows with every game revision
(19 entries on the 2025-08-05 build, 35 on 2026-07-21). Players pinned to an older
`gamemdx.dll` who load chart data from a newer revision get wrong file choices
for split songs the old binary does not list: the Expert/Challenge chart is read
from a file that does not contain it, which surfaces as an empty chart or as the
boot-blocking `ME1529 FILE CORRUPTION ERROR`.

This mod (`split-ssq-auto-discovery`, "Split SSQ Auto-Discovery") replaces that
hardcoded table with runtime discovery: at enable it scans the SSQ directory (stock
plus every LayeredFS mod folder) for `_N` files, records which chart levels each
one actually contains, and detours `build_ssq_path` to answer from that index.
Every in-game SSQ consumer — the boot-time analysis pass, normal play, matching
play, and course preload — calls this one function, so a single detour covers the
whole game.

## Detailed Requirements

Functional:

1. **R1 — Rule A resolution.** For a request `(basename, d)` with `d ∈ 0..4`, the
   mod names the file `<basename>_<N>.ssq` for the HIGHEST `N ≤ d+1` such that the
   file exists and contains a type-3 (step) chunk of level `d` in either play
   mode; if no such `N` exists it names the unsplit `<basename>.ssq`. On the
   installed stock data this reproduces the game's own choices for every
   `(song, difficulty)` whose chart exists, with one harmless divergence (`sabm`
   Challenge → `_5`, whose Challenge chunks are byte-identical to the stock-chosen
   `_3`'s).
2. **R2 — Basename-opaque.** The resolver looks up exactly the string the game
   passes and never consults `musicdb.xml`. This preserves the `toho` special
   case: the play sequences rewrite that song's basename to a random `toho1..toho4`
   BEFORE calling the builder; with no `tohoN_*.ssq` on disk those resolve to the
   unsplit `tohoN.ssq`, byte-identical to stock.
3. **R3 — Discovery sources.** Split files are discovered in the stock
   `data/mdb_apx/ssq/` directory and in every LayeredFS mod folder's
   `mdb_apx/ssq/`. The content check reads the file LayeredFS would actually
   serve (first mod folder in resolution order, else stock), so the index agrees
   with what the game will load.
4. **R4 — Fail-open.** (a) An unknown `(basename, d)` yields the unsplit path;
   (b) if the index could not be built, every call is forwarded to the original
   function (literal stock behavior) and one WARN is logged; (c) `d ∉ 0..4` is
   forwarded to the original; (d) if the `build_ssq_path` signature does not
   resolve, the mod is not registered.
5. **R5 — Timing.** The index is built synchronously inside `enable()`, before
   the detour becomes effective, so the game's first builder call (the boot pass,
   ~7200 synchronous calls inside `CheckStepDataActor::onInit`) is answered from a
   complete index.
6. **R6 — Divergence diagnostics.** On every call the detour also runs the
   original into a scratch buffer and logs one INFO line per distinct
   `(basename, d)` whose answer differs from the original's, capped at 64 lines
   per session. Expected on a matched binary/data pair (cabinet 2026-09-03): the `sabm d=4`
   line plus one `d=4` line per pattern-E/C song whose split file has NO
   Challenge chart (stock names `_3`/`_5`, Rule A names the base file — both
   lack the chart, both analyses are zero, no `ME1529`); the target scenario adds
   one line per newly-discovered split chart.
7. **R7 — No configuration.** The only control is the `mods` map toggle
   (default ON). Disabling at runtime makes the detour pass through to the
   original; re-enabling rebuilds the index.

Non-functional:

8. **R8 — Hot path.** Per call: bounded `strlen`, one hash lookup, one bounded
   formatted write into the caller's 256-byte buffer, NUL-termination. No heap
   allocation, no locks other than a read of an immutable index, no logging except
   the deduped divergence line.
9. **R9 — Safety.** The detour body is panic-free (no `unwrap`/indexing outside
   bounds-checked helpers); all reads of game-owned memory are bounded.
10. **R10 — Cross-build.** No read at any fixed offset past the function entry;
    the signature is the entry-prologue AOB, unique on all four supported builds
    (20250805, 20260224, 20260721, 20260825).

Assumptions:

- The game only calls `build_ssq_path` for songs in its loaded music DB (boot
  pass iterates the DB; play sequences pass DB entries), so "only songs present in
  musicdb" holds structurally.
- A `_N` file holds both the single and the double chart of its level (confirmed
  on all 39 installed split files); the builder takes no mode argument.
- The fast-bootup analysis cache is keyed per registered path; a path that
  differs from a previous boot is a per-item cache miss and self-heals. No
  invalidator change.
- `src/services/chart_length.rs`, the one DLL-side SSQ path builder outside the
  detour, is out of scope (it computes the song-select LENGTH readout from the base
  file's charts, which share the song's length). The resolver is exposed so it can
  be routed later.

## Architecture Overview

```mermaid
flowchart LR
    subgraph game [gamemdx.dll]
        BOOT[CheckStepDataActor::onInit<br/>5 × songs]
        DPS[DancePlaySequence::onSetup]
        MDPS[MatchingDancePlaySequence::onSetup]
        CW[PlayerCourseWork::prepare]
        BSP[build_ssq_path<br/>out, basename, d]
        FM[FileManager::register → AVS open]
        BOOT & DPS & MDPS & CW --> BSP --> FM
    end
    subgraph dll [ddr_world_hook.dll]
        DET[detour: build_ssq_path_hook]
        IDX[(Index<br/>basename → [Option&lt;N&gt;; 5])]
        DISC[discovery.rs<br/>dir scan + chunk headers]
        RES[resolver.rs<br/>pure: build_index / resolve]
        DISC -->|"at enable()"| RES --> IDX
        DET -->|lookup| IDX
        DET -.->|"oracle (R6)"| ORIG[original build_ssq_path]
    end
    BSP -. GenericDetour .-> DET
    LFS[LayeredFS mod folders<br/>data_mods/*/mdb_apx/ssq] --> DISC
    STOCK[data/mdb_apx/ssq] --> DISC
```

Call sequence at boot:

```mermaid
sequenceDiagram
    participant Init as DLL init
    participant Mod as SplitSsqAutoDiscoveryMod
    participant Disc as discovery
    participant Game as CheckStepDataActor::onInit
    participant Hook as detour
    Init->>Mod: enable()
    Mod->>Disc: scan() → Vec<SplitFile{basename,n,levels}>
    Disc-->>Mod: listing
    Mod->>Mod: resolver::build_index(listing) → store in INDEX
    Mod->>Mod: install detour (once), ACTIVE=true
    loop 5 × songs
        Game->>Hook: build_ssq_path(out, basename, d)
        Hook->>Hook: INDEX.resolve(basename, d)
        Hook->>Hook: write path into out
        Hook->>Hook: original(scratch, basename, d); compare; maybe INFO
    end
```

## Components and Interfaces

All under `src/mods/split_ssq_auto_discovery/`.

### `resolver.rs` (pure, host-tested)

```rust
/// Difficulty index 0..4 = Beginner, Basic, Difficult, Expert, Challenge.
pub const LEVEL_HIGH_BYTES: [u8; 5] = [0x04, 0x01, 0x02, 0x03, 0x06];

/// One discovered `<basename>_<n>.ssq`.
pub struct SplitFile {
    pub basename: Vec<u8>,   // raw bytes as they appear in the filename
    pub n: u8,               // 1..=5
    pub levels: u8,          // bitmask, bit d set ⇔ file has a level-d type-3 chunk (either mode)
}

pub enum Choice { Base, Split(u8 /* n */) }

pub struct Index { map: HashMap<Vec<u8>, [Option<u8>; 5]> }

impl Index {
    pub fn empty() -> Index;
    /// Rule A over the listing. Duplicate (basename, n) entries are merged (OR of levels).
    pub fn build(files: &[SplitFile]) -> Index;
    /// `d` must be 0..4; callers guard. Unknown basename ⇒ Base.
    pub fn resolve(&self, basename: &[u8], d: usize) -> Choice;
    pub fn song_count(&self) -> usize;
    /// For the enable-time INFO: per-basename effective mapping, sorted.
    pub fn describe(&self) -> Vec<(Vec<u8>, [Option<u8>; 5])>;
}

/// Level bitmask from an SSQ blob's type-3 chunks (`param2 >> 8` ∈ LEVEL_HIGH_BYTES).
/// Walks chunk headers only (12-byte stride by `length`); malformed ⇒ what was read so far.
pub fn levels_in_blob(blob: &[u8]) -> u8;

/// Parse `<basename>_<n>.ssq` (case-sensitive `.ssq`, n ∈ '1'..='5', non-empty basename).
pub fn parse_split_filename(name: &[u8]) -> Option<(Vec<u8>, u8)>;

/// Writes `data/mdb_apx/ssq/<basename>[_<n>].ssq` + NUL into `out` (cap bytes).
/// Returns false (and writes nothing) if it would not fit. Pure, allocation-free.
pub fn format_path(out: &mut [u8], basename: &[u8], choice: Choice) -> bool;
```

Rule A in `build`: for each basename, for each `d`, `chosen[d] = max { n : n ≤ d+1 ∧
file(basename, n) exists ∧ levels(n) has bit d }`.

### `discovery.rs` (impure, host `std::fs`)

```rust
/// Scan stock `data/mdb_apx/ssq` + every LayeredFS mod folder
/// (`mod_paths::available_mods()`, each `<mod>/mdb_apx/ssq`) for `*_[1-5].ssq`.
/// For each distinct (basename, n): read the file LayeredFS would serve
/// (`mod_paths::find_first_modfile("mdb_apx/ssq/<name>")`, else the stock path),
/// compute `levels_in_blob`. Files that fail to read are skipped with one WARN each.
/// Returns Err only when the stock directory itself cannot be listed.
pub fn scan() -> Result<Vec<SplitFile>, String>;
```

Reads whole files (split SSQs are tens of KiB; ~40 files) — simpler than partial
reads and still milliseconds.

### `mod.rs` (lifecycle + detour)

- `type BuildSsqPathFn = unsafe extern "C" fn(*mut u8, *const u8, i32);`
- `static mut HOOK: Option<GenericDetour<BuildSsqPathFn>>` — installed once
  (`hooks::install_enabled`, the repo's one-detour-per-target rule); never
  uninstalled. `HOOK_INSTALLED: AtomicBool` drives `is_active()`.
- `static ACTIVE: AtomicBool` — the runtime toggle; `disable()` clears it and the
  callback forwards to the original.
- `static INDEX: RwLock<Option<Arc<Index>>>` — replaced wholesale at each enable;
  the callback takes a read lock, clones the `Arc`, drops the lock (never held
  across the original call).
- `static DIVERGENCE_SEEN: Mutex<HashSet<(Vec<u8>, u8)>>` + `DIVERGENCE_COUNT:
  AtomicUsize` (cap 64) for R6. Only touched on the divergent path (rare).
- Callback:

```rust
unsafe extern "C" fn build_ssq_path_hook(out: *mut u8, basename: *const u8, d: i32) {
    let Some(hook) = HOOK.as_ref() else { return };
    if !ACTIVE.load(Acquire) || out.is_null() || basename.is_null() || !(0..=4).contains(&d) {
        return hook.call(out, basename, d);                      // R4c, R7
    }
    let index = { INDEX.read() ... clone Arc };                 // None ⇒ R4b
    let Some(index) = index else { return hook.call(out, basename, d) };
    let name = bounded_cstr(basename, 0x20);                    // bytes up to NUL, cap 32
    let choice = index.resolve(name, d as usize);
    let mut ours = [0u8; 0x100];
    if !resolver::format_path(&mut ours, name, choice) { return hook.call(out, basename, d) }
    // R6 oracle: original into scratch, compare, dedup-log.
    let mut stock = [0u8; 0x100];
    hook.call(stock.as_mut_ptr(), basename, d);
    if cstr(&ours) != cstr(&stock) { log_divergence_once(name, d, &ours, &stock) }
    copy ours (incl. NUL) into out;
}
```

  The original is called with our scratch buffer rather than `out` so a later
  compare cannot be confused by the original's own write; `out` receives our
  answer last.
- `enable()`: `discovery::scan()` → `Index::build` → store; INFO `"SplitSsqAutoDiscovery:
  indexed N split songs from M files"` plus one compact INFO per song
  (`fizz: [-,-,3,3,3]`); on `Err` store `None` + WARN (R4b); install detour if not yet;
  `ACTIVE=true`.
- `disable()`: `ACTIVE=false`; INFO.
- `required_signatures()`: `["build_ssq_path"]`; `init()` reads it via
  `require_address`.

### `src/core/signatures.rs`

```
name: "build_ssq_path"
pattern: "48 89 74 24 08 57 48 83 EC 30 4C 8B D1 48 8D 3D ?? ?? ?? ?? 48 8B F2 B9 05 00 00 00 F3 A6 0F 84"
```

Entry of the SSQ path builder; wildcards only the `LEA RDI` displacement; the
`0F 84` tail pins the first cell's unconditional-match shape (`acef`). Verified
exactly one hit on 20250805 (`0x18019E8D0`), 20260224 (`0x1801A1730`), 20260721
(`0x1801B43F0`), 20260825 (`0x1801B4090`). Consumer reads nothing at `match+N`.

### Registration

`src/lib.rs` mod list (next to `anytime_speedmod`); `src/mods/mod.rs` module
declaration. Default ON (not in `DEFAULT_OFF_MODS`); not late-binding.

### Documentation

`AGENTS.md` Key Entry Points row; `docs/split_ssq_research.md` already carries the
RE. `scripts/validate_split_ssq.sh` (host harness in the
`validate_auto_calibration.sh` mould) mounting `resolver.rs`.

## Data Models

| Item | Shape | Notes |
|---|---|---|
| `SplitFile` | `{ basename: Vec<u8>, n: u8 (1..=5), levels: u8 bitmask }` | bit `d` ⇔ level `d` present |
| `Index` | `HashMap<Vec<u8>, [Option<u8>; 5]>` | value = chosen `n` per difficulty, `None` = base |
| Game ABI | `void build_ssq_path(char out[0x100], const char* basename, int d)` | MS-x64; basename ≤ 7 chars in stock, we accept ≤ 31 |
| Output path | `data/mdb_apx/ssq/<basename>.ssq` or `..._<n>.ssq` | must match the game's own formats byte-for-byte so FileManager dedupe (FNV over the name) behaves identically |
| Level ↔ chart code | `d` 0..4 ↔ `param2 >> 8` = `04 01 02 03 06`; low byte `14`/`18` = single/double | per `docs/ssq_format.md` §5.1 |

## Error Handling

| Condition | Behavior |
|---|---|
| Signature miss (incl. third-party-modified 20250805 DLLs) | Mod not registered; stock behavior |
| Stock SSQ dir unlistable | `INDEX=None`, one WARN; every call forwarded to original |
| A `_N` file unreadable / malformed | Skipped with one WARN; other files still indexed |
| Detour install failure | WARN; `is_active()=false`; stock behavior |
| `d` out of range / null pointers | Forwarded to original |
| Basename > 31 bytes or path would exceed 0xFF chars | Forwarded to original |
| Divergence log cap reached | Silent thereafter (cap 64) |

Nothing in the callback can panic: all slices are bounds-checked, the map lookup
is infallible, `format_path` returns `bool`.

## Testing Strategy

Host tests (pure `resolver.rs`, run via `scripts/validate_split_ssq.sh` on this
ARM host and via `cargo test` where the crate compiles):

- `parse_split_filename`: accepts `casr_3.ssq`, `dopa2_5.ssq`; rejects `casr.ssq`,
  `casr_6.ssq`, `casr_33.ssq`, `_3.ssq`, `casr_3.SSQ`.
- `levels_in_blob`: synthetic blobs with type-1/2/3 chunks; `0xFFFF` sentinel and
  zero-length terminator honored; truncated header tolerated.
- **Stock-table reproduction**: encode the 39 installed split files' level sets
  (from the RE record §6) as a fixture listing; assert `Index::build(...).resolve`
  equals the game's effective table (RE §4.1) for every `(song, d)` whose chart
  exists, with `sabm d=4 → Split(5)` as the documented exception; assert
  unknown basenames and `toho1..4` resolve to `Base` for all `d`.
- `format_path`: exact bytes for base/split, NUL placement, refusal on overflow.

Cabinet validation (the engine-facing layer has no harness):

1. Stock data, stock-matched binary: boot log shows `indexed 32 split songs`,
   zero `INVALID SSQ`/`ME1529` lines, and either zero divergence lines or exactly
   `sabm d=4`. Play a pattern-E song (e.g. `casr` Expert) and a `toho` chart.
2. Target scenario: on a binary lacking a code (or by simulating: copy an
   existing split song's files under a new basename with a matching
   `musicdb.merged.xml` entry), confirm the divergence log names it and the
   Expert chart loads.
3. Toggle OFF in the mod menu → next boot's log shows the mod disabled and stock
   paths; toggle ON → re-indexed.

## Appendix A — Alternatives Considered

- **Call the original and post-correct its answer** — rejected: for unknown songs
  the original returns the base file, so the index is needed anyway; replacement
  is simpler and keeps the original as a pure diagnostic oracle.
- **Filename-only discovery rules** — "exact `_{d+1}`" is wrong for 25 of 32
  installed split songs (their Expert/Challenge live in `_3`); "highest existing
  `_N`" matches today's data but cannot guarantee the chosen file has the chart,
  and a wrong choice is a boot-blocking error.
- **LayeredFS-level merge into one `<basename>.ssq`** — impossible: split files
  carry different tempo chunks and an SSQ has exactly one.
- **Musicdb-driven index** — would have broken the `toho1..4` randomized
  basenames, which are not musicdb entries.
