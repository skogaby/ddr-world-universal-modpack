# Split SSQ Auto-Discovery — Implementation Plan

Status: Approved 2026-09-03

Design: `design/detailed-design.md`. RE: `docs/split_ssq_research.md`.

- [x] Step 1: Pure resolver (`resolver.rs`) + host harness reproducing the stock table
- [x] Step 2: Discovery scan, signature, mod skeleton with detour — end-to-end on the cabinet
- [x] Step 3: Divergence diagnostics, docs, AGENTS.md row, readiness gates

## Step 1: Pure resolver + host harness

**Objective.** Land the whole decision core as a dependency-free module with tests
that pin it to the game's own table before any hook exists.

**Guidance.** Create `src/mods/split_ssq_auto_discovery/{mod.rs,resolver.rs}`
(`mod.rs` may be a stub exporting `resolver` only for now — or land it with the
skeleton in Step 2; either way the module must compile under `cargo check`).
Implement `LEVEL_HIGH_BYTES`, `SplitFile`, `Choice`, `Index::{empty,build,resolve,
song_count,describe}`, `levels_in_blob`, `parse_split_filename`, `format_path`
per the design. `levels_in_blob` walks headers exactly like
`src/core/ssq/ssq_chunk.rs::find_chunk` (length-0 terminator, `0xFFFF` sentinel,
malformed ⇒ stop) but must not depend on it if the harness is to mount
`resolver.rs` alone — copy the two read helpers. Add
`scripts/validate_split_ssq.sh` cloned from `scripts/validate_auto_calibration.sh`
mounting `resolver.rs`.

**Tests (in `resolver.rs`, `#[cfg(test)]`).** Filename parsing accept/reject set;
`levels_in_blob` on synthetic blobs (sentinel, terminator, truncation, both
modes); **stock-table fixture**: the 39 installed files' level sets from
`docs/split_ssq_research.md` §6 → assert every `(song, d)` of RE §4.1 where the
chart exists, `sabm d=4 → Split(5)` exception, `toho1..4` and unknown ⇒ `Base`,
`acef` fully split, `rabb` `[B,B,B,4,5]`, `hkhk` Basic ⇒ Base despite the redundant
copy in `_3`; `format_path` exact bytes + overflow refusal.

**Integration.** None yet (no callers). `cargo check --target
x86_64-pc-windows-msvc` clean with the module declared in `src/mods/mod.rs`.

**Demo.** `./scripts/validate_split_ssq.sh` green: the resolver reproduces the
game's 35-entry table from file contents alone.

## Step 2: Discovery, signature, mod skeleton, detour

**Objective.** The feature works end-to-end on the cabinet: enable scans the
disk, builds the index, and the detour answers every `build_ssq_path` call.

**Guidance.** Add the `build_ssq_path` `SignatureDefinition` to
`src/core/signatures.rs` (pattern per design; comment with the four per-build
addresses). Implement `discovery.rs::scan()` (stock dir + `mod_paths::available_mods()`
mod dirs; content via `find_first_modfile` else stock; per-file WARN on read
failure; `Err` only if the stock dir is unlistable). Implement `mod.rs`:
`SplitSsqAutoDiscoveryMod` (id `split-ssq-auto-discovery`, name "Split SSQ
Auto-Discovery"), `required_signatures = ["build_ssq_path"]`, statics `HOOK`,
`HOOK_INSTALLED`, `ACTIVE`, `INDEX: RwLock<Option<Arc<Index>>>`, the callback per
the design MINUS the oracle compare (Step 3), `enable()` (scan → build → store →
INFO summary + per-song mapping lines → install once via `hooks::install_enabled`
→ `ACTIVE=true`), `disable()` (`ACTIVE=false`), `is_active()`. Register in
`src/lib.rs` next to `anytime_speedmod`. Follow `src/mods/announcer_mute.rs` for
the detour/static shape.

**Tests.** `discovery.rs` gets a small host-testable inner function
`collect_from_listing(names: impl Iterator<Item=&[u8]>) -> Vec<(basename, n)>`
covered in the harness (dedupe across sources, ignores non-matching names). The
callback itself is validated on the cabinet.

**Integration.** Consumes Step 1's resolver. `cargo check` clean → `cargo fmt` →
`./build.sh` → `./scripts/validate_signatures.sh ~/Desktop/ddr_modules` ALL GREEN
(new signature must hit exactly once on all four builds; `shape_diff.py` not
needed — the consumer reads nothing at `match+N`).

**Demo.** Cabinet boot log: `SplitSsqAutoDiscovery: indexed 32 split songs from 39
files`, per-song mapping lines, zero `INVALID SSQ`/`ME1529`; `casr` Expert and a
`toho` chart play normally.

## Step 3: Divergence diagnostics, docs, gates

**Objective.** Make the mod's effect observable in the log and finish the
repo-side documentation.

**Guidance.** Add the R6 oracle to the callback: call the original into a
0x100 scratch, compare NUL-terminated strings, dedup via
`Mutex<HashSet<(Vec<u8>, u8)>>` + `AtomicUsize` cap 64, INFO
`"SplitSsqAutoDiscovery: <basename> d=<d>: ours=<path> stock=<path>"`. Add the
AGENTS.md Key Entry Points row (mechanism, rule A, toho, fail-open, oracle log,
`chart_length.rs` follow-up). Add `.agents/planning/.../progress.md` deploy log
entry template. Run the readiness gates again.

**Tests.** Harness: a pure `diverges(ours: &[u8], stock: &[u8]) -> bool`
NUL-aware compare helper in `resolver.rs` with cases (equal, differ, missing NUL).

**Integration.** Callback now complete per design. `cargo check` → `cargo fmt` →
`./build.sh` clean.

**Demo.** Stock-data boot: log shows either no divergence lines or exactly
`sabm d=4: ours=..._5.ssq stock=..._3.ssq`. Simulated new split song (copy
`casr*.ssq` to `zzzt*.ssq` + a `musicdb.merged.xml` entry) ⇒ one divergence line
per affected difficulty and the Expert chart loads.
