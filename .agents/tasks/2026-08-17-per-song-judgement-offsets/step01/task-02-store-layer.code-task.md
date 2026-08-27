# Task: Store layer — session/baseline maps, merge semantics, wire codec

## Description
Implement `src/mods/per_song_judgement_offsets/store.rs`: the in-memory state
for per-song judgement offsets — a CSV-derived baseline plus two per-side
session maps — with the merge operations, the network wire-string
encode/decode, and the pure decision helpers the UI and gameplay layers will
call. Host-tested, no game APIs.

## Background
The design's merge model: at boot each side's *session map* equals the CSV
*baseline* column; a server profile load **replaces** that side's session map
(CSV untouched); explicit options-menu edits update the session map and upsert
the CSV; card-in resets a side back to baseline before any new server data is
applied. Session maps may hold codes the local CSV has never seen (server data
from another cabinet) — those round-trip on the wire but never touch the CSV
unless locally edited.

The wire string is `code|offset|code|offset|...`, entries sorted by code,
offsets as decimal integers (with `-`), empty map = empty string. Cap 2000
entries on both encode and decode. Values clamp to −100..+100.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-17-per-song-judgement-offsets/design/detailed-design.md
  (sections: Detailed Requirements 1, 6, 7; Data Models → In-memory, Wire
  string; Error Handling table)

**Note:** Read the design document before beginning implementation.

## Technical Requirements
1. `store.rs` defines the pure state type (design shape):
   ```rust
   pub struct Store {
       baseline: HashMap<String, [Option<i8>; 2]>,
       session:  [HashMap<String, i8>; 2],
       armed:    bool,
   }
   ```
   plus a module-level `Mutex<Store>` accessor for later consumers (the pure
   logic must be testable on `Store` directly, without the global).
2. Operations (all pure methods on `Store`):
   - `load_baseline(doc: &csv::CsvDoc)` — sets baseline from the CSV document,
     resets both session maps to their baseline columns, sets `armed`.
   - `reset_to_baseline(side)` — session map = baseline column.
   - `apply_server_string(side, s: &str) -> DecodeStats` — decode + replace
     the side's session map (replace even when `s` is empty).
   - `set_entry(side, code, value: i8)` / `clear_entry(side, code)` /
     `lookup(side, code) -> Option<i8>`.
   - `encode_side(side) -> String` — sorted, capped, deterministic.
3. Wire decode: iterate `|`-separated tokens as (code, offset) pairs; skip a
   pair when the offset is non-integer or out of range after clamp policy
   (design says clamp on CSV read but SKIP malformed wire pairs — follow the
   design: out-of-range wire values are clamped, non-integer tokens skip the
   pair); a dangling trailing code is dropped; `DecodeStats` counts
   skipped/clamped for the caller's aggregated WARN. Cap: decode stops
   accepting entries beyond 2000 (stats note truncation).
4. Pure decision helpers (used by Step 4's UI and Step 5's arming, host-tested
   now):
   - `row_seed(&Store, side, code) -> (i32 /*parent 0|1*/, i32 /*child*/)` —
     parent 1 + value when an entry exists, else (0, 0).
   - `arm_decision(&Store, side_entered: bool, course_mode: bool, code:
     Option<&str>) -> Option<i8>` — Some(offset) only when entered, not
     course, code known, and an entry exists.
5. `encode_side` of an un-armed store returns nothing usable — expose
   `is_armed()` so the persistence closure can return `None` (omit field)
   before boot completes.
6. No logging, no `unsafe`, no game APIs in this file. `cargo fmt` /
   `cargo check --target x86_64-pc-windows-msvc` / `cargo test` clean.

## Dependencies
- task-01-csv-layer (module gate exists; `csv::CsvDoc` is the baseline input
  type).

## Implementation Approach
1. Define `Store` + `DecodeStats`; implement baseline/session operations with
   tests for each merge rule from the design's requirement 7.
2. Implement encode/decode with round-trip tests first, then the malformed /
   cap / empty-string edge tests.
3. Add the two decision helpers with table-driven tests.
4. Add the module-level `Mutex` accessor last (a thin wrapper; no logic).

## Acceptance Criteria

1. **Wire round-trip**
   - Given a session map with negative values, a zero value, and >2 entries
   - When encoded and decoded back
   - Then the map is identical and the encoding is sorted by code and stable
     across runs

2. **Empty-string semantics**
   - Given a side with entries
   - When `apply_server_string(side, "")` runs
   - Then the side's session map is empty (server-cleared), and
     `encode_side` of an empty map returns `""`

3. **Malformed wire tolerance**
   - Given `"puty|11|bad|xx|aaaa|999|dangling"`
   - When decoded
   - Then puty=11 loads, the `bad|xx` pair is skipped, aaaa clamps to 100,
     `dangling` is dropped, and `DecodeStats` reflects each

4. **Merge semantics**
   - Given a baseline {A:5} and a session edit adding B, then a server string
     "C|3" applied to side 0
   - When each step runs in order
   - Then after the edit side 0 = {A:5, B:…}; after the server apply side 0 =
     {C:3} exactly; side 1 remains at baseline; `reset_to_baseline(0)`
     restores {A:5}

5. **Unknown-code preservation**
   - Given a server string containing a code absent from the baseline
   - When applied and re-encoded
   - Then the unknown code round-trips in the wire string

6. **Cap enforcement**
   - Given 2001 entries
   - When encoding and when decoding
   - Then exactly 2000 survive deterministically and stats note the truncation

7. **Decision helpers**
   - Given the design's arm conditions (entered/course/code/entry) as a truth
     table
   - When `arm_decision` and `row_seed` run over the table
   - Then outputs match the design (including: entry value 0 → parent ON,
     child 0; no entry → (0, 0))

## Metadata
- **Complexity**: Medium
- **Labels**: rust, pure-layer, state, wire-codec, foundation
- **Required Skills**: Rust
- **Generated By**: code-task-generator 2026-08-17
- **Source Plan**: .agents/planning/2026-08-17-per-song-judgement-offsets/implementation/plan.md
- **Plan Step**: Step 1
