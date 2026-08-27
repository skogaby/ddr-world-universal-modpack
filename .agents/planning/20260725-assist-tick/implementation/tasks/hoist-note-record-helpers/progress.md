# Progress — Task 01: Hoist Note-Record Helpers

- [x] Create `src/types/game_note.rs` (reading half, text moved verbatim via `git mv` + carve)
- [x] Create `src/mods/note_types_expansion/notes_vec.rs` (injection half)
- [x] Delete old `game_note.rs`, update `mod.rs` (`pub mod notes_vec;`)
- [x] Update 5 importers (mines, registry, mine_render, note_type, hooks)
- [x] Register in `src/types/mod.rs` (header bullet + `#[allow(dead_code)] pub mod game_note;`)
- [x] `cargo check` clean; `cargo fmt` no-op on moved code; `./build.sh` clean

## Verification record

- **AC4 proven mechanically:** `diff` of the moved reading half (orig lines 25–218 vs new 24–217)
  and the moved injection half (orig 220–431 vs new 16–227) — **both byte-identical**. The only
  non-move differences are the split file headers (the original `//!` header's summary sentence and
  its injection paragraph were distributed to the file each describes) and the import lines.
- **AC1:** `src/types/game_note.rs` imports only `std::mem`; nothing from `src/mods/`.
- **AC2:** `notes_vec.rs` holds `GameNotesVec` + `NotesVecError`; declared in the mod's `mod.rs`.
- **AC3:** `rg "note_types_expansion::game_note"` → no matches; no re-export shim.
- **AC5 (mines behave identically on a mine chart):** maintainer's live check — outstanding,
  bundled with the Step 3 listening pass.
- **AC6:** `cargo check` exit 0, `cargo fmt` clean, `./build.sh` exit 0 (logs/ has both).

No deviations. Commit deliberately not made (repo convention: maintainer owns commits).

Status: Complete
