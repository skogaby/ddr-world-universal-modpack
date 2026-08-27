# Plan — Task 01: Hoist Note-Record Helpers

**Status: Approved** (auto mode — approval inherited from the verified approved plan/design chain;
see context.md)

## Test scenarios

No unit-test harness exists in this repo. Verification, per the task's own ACs:

1. **AC1/AC2/AC3 (structure):** inspect the new files and grep — `src/types/game_note.rs` holds the
   reading half and imports nothing from `src/mods/`; `notes_vec.rs` holds `GameNotesVec` +
   `NotesVecError`; no `note_types_expansion::game_note` path remains anywhere.
2. **AC4 (nothing changed but location):** `git diff` review — every constant, signature and doc
   comment byte-identical apart from module paths and location-referencing sentences.
3. **AC6 (gates):** `cargo check --target x86_64-pc-windows-msvc` → `cargo fmt` → `./build.sh`, all
   clean. (Gates run once at the end of the three-task Step 3 sequence for `build.sh`; `cargo check`
   after this task.)
4. **AC5 (mines still work):** maintainer's live check on a mine chart — reported, not agent-run.

## Implementation approach

1. Create `src/types/game_note.rs` by moving text: file header's layout-provenance portion, the
   `GameNote` struct + const assert, `panel`/`state`/`kind`/`result` modules, `for_each_result`,
   `actor_results_range`, `impl GameNote`. Only `use std::mem;` is needed.
2. Create `src/mods/note_types_expansion/notes_vec.rs` with the injection half: header's
   allocator/injection portion, `GameNotesVec`, `NotesVecError`; `use std::mem; use std::ptr;
   use crate::types::game_note::GameNote;`.
3. Delete the old `game_note.rs`; update `mod.rs`'s module declaration.
4. Update the five importers per the table in context.md.
5. Add `game_note` to `src/types/mod.rs` (header bullet + `#[allow(dead_code)] pub mod`).
6. `cargo check`, then diff review for AC4.
