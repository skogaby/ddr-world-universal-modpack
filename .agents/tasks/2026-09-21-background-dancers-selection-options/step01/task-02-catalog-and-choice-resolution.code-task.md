# Task: Pure catalog + choice resolution (`catalog.rs`, `selection::resolve_choice`)

## Description
Implement the dependency-free half of the option rows: the sorted, labelled catalog of dancers
and distinct stages derived from the existing candidate tables (`split_key`, `label_for`,
`build_catalog`, `clamp_to_catalog`), and the pure `resolve_choice` that turns the rows' choices
into a `(StageCandidate, Vec<DancerCandidate>)` for `assemble_pick`. Host-tested via the
temp-crate harness (`scripts/validate_background_dancers.sh`), which mounts these files at the
crate root beside `selection.rs` — so they MUST NOT contain `crate::` imports (reach `selection`
via `super::selection`, exactly as `schedule.rs` does).

## Background
The Background Dancers mod already builds `Vec<StageCandidate>` / `Vec<DancerCandidate>` from the
`startup.arc` rlists (`selection::stage_candidates` / `dancer_candidates`). Keys are the arc stems
(`emi01`, `boom06`, `crystaldium00`). The rows' values are indices into a catalog sorted by key
(0 = RANDOM, `k ≥ 1` = `catalog[k−1]`); labels are `UPPER(alpha prefix)` + ` #(digits+1)` only
when that prefix has > 1 variant (design §4.2, §5.1 — `emi00 → EMI #1`, `babylon00 → BABYLON`,
`replicant05 → REPLICANT #6`). Stages collapse duplicate rlist rows to distinct keys
(`boom00` ×2, `monitor00` ×2) and never include `dummy00` (already excluded by
`stage_candidates`). Every label must be ≤ 15 bytes (the scalar value SSO budget).

The plan places these functions in `options.rs`; because that file will also hold the engine-facing
registration (which needs `crate::`), the pure functions live in a sibling `catalog.rs` so the
harness can mount them. `resolve_choice` belongs in `selection.rs` (already mounted, already pure).

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-21-background-dancers-selection-options/design/detailed-design.md` (§4.2, §4.3, §5.1, §7)

**Additional References (if relevant to this task):**
- `src/mods/background_dancers/selection.rs` — `StageCandidate`, `DancerCandidate`, `pick_stage`, `pick_dancers`, `distinct_stage_keys`, `apply_pin`, the `real_map_rows()` test fixture (the full stock stage key list)
- `.agents/planning/2026-09-21-background-dancers-selection-options/research/orientation.md` §1.1 (the 26 dancer keys / 25 stage keys)
- `scripts/validate_background_dancers.sh` (harness mount list)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. New file `src/mods/background_dancers/catalog.rs` (std-only, no `crate::`):
   - `pub const RANDOM: i32 = 0;` `pub const MAX_LABEL_BYTES: usize = 15;`
   - `pub struct CatalogEntry { pub key: String, pub label: String }`
   - `pub struct Catalog { pub dancers: Vec<CatalogEntry>, pub stages: Vec<CatalogEntry> }` with
     `label(kind, value) -> Option<&str>`, `key(kind, value) -> Option<&str>`,
     `count(kind) -> usize` helpers; `pub enum Kind { Dancer, Stage }`.
   - `pub fn split_key(key: &str) -> (String, u32)` — alphabetic prefix upper-cased, trailing
     digits + 1 (no digits ⇒ 1). Non-alphanumeric prefix characters are dropped.
   - `pub fn label_for(prefix: &str, variant: u32, variants_in_family: usize) -> String`.
   - `pub fn build_catalog(stages: &[StageCandidate], dancers: &[DancerCandidate]) -> Catalog`:
     sorted by key (byte order), stages distinct by key, family variant counts computed over the
     catalog's own entries; labels truncated to `MAX_LABEL_BYTES` defensively (a truncation is a
     data bug; the test suite pins that the stock set never needs it).
   - `pub fn clamp_to_catalog(value: i32, count: usize) -> i32` — `0..=count` passes, else `RANDOM`.
2. `selection.rs`: `pub fn resolve_choice(rng: &mut Rng, stages, dancers, stage_key: Option<&str>,
   dancer_keys: &[Option<&str>]) -> Option<(StageCandidate, Vec<DancerCandidate>)>`:
   `Some(k)` stage ⇒ uniform over the rows with key `k` (unknown key ⇒ `None`); `None` ⇒
   `pick_stage`; each `dancer_keys[i]`: `Some(k)` ⇒ that candidate (unknown ⇒ `None`), `None` ⇒ one
   uniform pick. Returns `None` for an empty `dancer_keys` or empty tables (caller falls back).
3. Mount `catalog.rs` in `scripts/validate_background_dancers.sh` (`MODULE_NAMES` + paths) and
   add `pub mod catalog;` to `background_dancers/mod.rs`.

## Dependencies
- None (task-03 consumes both).

## Implementation Approach
1. Tests first in `catalog.rs`: `split_key` cases (`emi01`→`("EMI",2)`, `crystaldium00`→
   `("CRYSTALDIUM",1)`, `abc`→`("ABC",1)`, `st001x2`-style mixed ⇒ documented behaviour);
   `label_for`; `build_catalog` over the full stock lists (26 dancers via a fixture rows table of
   the 26 keys from orientation §1.1 through `dancer_candidates`, 25 stages via `real_map_rows()`
   — expose that fixture as `pub(crate) fn` under `#[cfg(test)]` in `selection.rs` or duplicate the
   key list) asserting: sorted, 25/26 counts, `dummy00` absent, `boom00` once, `EMI #1/#2/#3`,
   `RAGE #1/#2`, `BABYLON`, `BOOM #1..#7`, `REPLICANT #1..#6`, `CRYSTALDIUM`, every label
   ≤ 15 bytes; `clamp_to_catalog` edges.
2. Tests in `selection.rs` for `resolve_choice`: chosen stage key ⇒ only that key's rows, both
   `boom00` rows reachable over many seeds; chosen dancer per slot; mixed `[Some, None]`; unknown
   stage/dancer key ⇒ `None`; all-RANDOM equals `pick_stage` + `pick_dancers` under the same seed
   (same RNG draw order — implement RANDOM legs by calling those functions).
3. Implement; run `./scripts/validate_background_dancers.sh` and `cargo check --target
   x86_64-pc-windows-msvc`; `cargo fmt`.

## Acceptance Criteria

1. **Label derivation**
   - Given the stock dancer keys
   - When `build_catalog` runs
   - Then labels include `EMI #1`, `EMI #2`, `EMI #3`, `RAGE #1`, `RAGE #2`, `BABYLON`, `ZERO`, and
     the entries are in byte-sorted key order

2. **Stage collapsing**
   - Given the 34-row stock `map_resources` table (7 `dummy00`, `boom00`×2, `monitor00`×2)
   - When `build_catalog` runs
   - Then `stages.len() == 25`, `boom00` appears once, `dummy00` is absent, `BOOM #1`..`BOOM #7`,
     `REPLICANT #1`..`REPLICANT #6`, `CRYSTALDIUM`, `LOVESWEETS` are present

3. **SSO budget**
   - Given the full stock catalog
   - When every label's byte length is checked
   - Then all are ≤ 15

4. **Load clamp**
   - Given `count = 25`
   - When `clamp_to_catalog` sees −1, 0, 25, 26
   - Then it returns 0, 0, 25, 0

5. **Choice resolution**
   - Given `stage_key = Some("boom00")` and `dancer_keys = [Some("emi01"), None]`
   - When `resolve_choice` runs over many seeds
   - Then the stage is always `boom00` (both rows 0 and 32 observed), dancer 0 is always `emi01`,
     dancer 1 varies; `Some("nope")` anywhere ⇒ `None`; all-`None` reproduces
     `pick_stage`/`pick_dancers` for the same seed

## Metadata
- **Complexity**: Low
- **Labels**: background-dancers, pure, catalog, selection
- **Required Skills**: Rust, host-harness testing
- **Generated By**: code-task-generator 2026-09-21
- **Source Plan**: `.agents/planning/2026-09-21-background-dancers-selection-options/implementation/plan.md`
- **Plan Step**: Step 1: Option rows end-to-end
