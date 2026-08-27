# Task: Hoist the Shared Note-Record Helpers into `types/`

## Description

Move the game's per-note record layout and the two helpers that read it out of the
`note_types_expansion` mod and into a shared module, so a second mod can consume them without
depending on an unrelated mod's internals. Pure refactor: no behaviour change, no new functionality.

The assist-tick mod (the next task) needs exactly this knowledge — the note record's fields, the
note-kind values, the per-panel state values, and the walk over a gameplay actor's Results vector.
Today all of it lives inside `note_types_expansion`, whose *other* half is mine **injection**
machinery that nothing else will ever call. Hoisting the reading half is what makes the shared
knowledge shared, rather than reaching sideways into a mod.

## Background

**Working directory: this repository** (the DDR World hook DLL / modpack).

`src/mods/note_types_expansion/game_note.rs` (431 lines) contains two unrelated halves:

| Half | Contents | Consumers |
|---|---|---|
| **Reading** the game's notes | the `GameNote` `#[repr(C)]` layout, the `panel` / `state` / `kind` / `result` constant modules, `actor_results_range`, `for_each_result`, and `GameNote::mine` | `note_types_expansion` **and** the incoming assist-tick mod |
| **Injecting** notes | `GameNotesVec`, `NotesVecError` — a wrapper over the game's allocator-aware note vector, parameterised by the app-heap allocator function pointers | `note_types_expansion` only |

Only the first half is shared. The second is bound to the app-heap allocator and is squarely a
mine-injection concern.

Two details that decide the boundary:

- **`GameNote::mine` must travel with the struct.** It writes the struct's private `_pad` fields, so
  it cannot live in a different module from the type once the type moves.
- **The destination is `src/types/`, not `src/core/`.** `core/` is documented as *game-agnostic*
  low-level infrastructure; a DDR note-record layout is the opposite of that. `types/` is documented
  as "shared type definitions used across mods and services", which is exactly what this is.
  (Maintainer decision, 2026-07-26.)

The layout constants in this file were read directly off the game's disassembly and are load-bearing
for two mods. **Nothing about them may change in this task** — a silent edit to an offset here would
break mine injection and mine rendering, both of which are working today.

## Reference Documentation

**Required:**
- Design: `.agents/planning/20260725-assist-tick/design/detailed-design.md` — §5.1 lists the
  structures and offsets the assist-tick mod consumes, which is the set that must end up reachable
  from the new module. §4.2.2 names `actor_results_range` and the Results-entry walk as "existing
  helpers", i.e. the things being hoisted

**Additional References (if relevant to this task):**
- `.agents/planning/20260725-assist-tick/research/note-taxonomy-and-actors.md` — §"Note-record
  kinds", §"`state[]` values" and §"Results-vector coverage" are the authority for what these
  constants mean; useful for judging whether a doc comment survives the move intact
- `CLAUDE.md` rule 10 (module layout) — why `types/` and not `core/`
- `src/types/mod.rs` — the module header lists each submodule with a one-line description; the new
  module needs an entry in the same style

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. Create `src/types/game_note.rs` holding the **reading** half: the `GameNote` struct with its
   `#[repr(C)]` layout and its `size_of` compile-time assertion, the `panel`, `state`, `kind` and
   `result` constant modules, `for_each_result`, `actor_results_range`, and `GameNote::mine`.
2. Move the doc comments **verbatim**. They carry the Ghidra provenance for every offset (which
   instruction was observed, which debug string confirmed a field name) and that provenance is the
   reason the constants can be trusted. Reword only where a sentence names the old location.
3. Leave `GameNotesVec` and `NotesVecError` in `note_types_expansion`, in their own file, with a
   name that says what they are (they are a note-*injection* vector, not a note record). Update that
   mod's `mod.rs` accordingly.
4. Register the new module in `src/types/mod.rs`, with a header bullet matching the existing
   `scenes` / `buttons` style.
5. Update every importing site — there are five files inside `note_types_expansion` — to the new
   path. Do not leave a re-export shim in the old location: one canonical path, or the next reader
   cannot tell which is authoritative.
6. **No behaviour change of any kind.** No constant edited, no signature changed, no logic altered,
   nothing added. If something looks wrong while moving it, report it rather than fixing it here.
7. Visibility should be no wider than it needs to be, but the new module's items are consumed from
   both `mods/` and (later) another mod, so `pub` on the moved items is correct.
8. `src/types/game_note.rs` must not depend on anything under `src/mods/`, or the hoist has achieved
   nothing.

## Dependencies

- `src/mods/note_types_expansion/game_note.rs` — the file being split
- `src/mods/note_types_expansion/{mod,hooks,mine_render,mines,note_type,registry}.rs` — the
  importers to update
- `src/types/mod.rs` — the module list
- No new crate dependencies

## Implementation Approach

1. Read the whole of `game_note.rs` first and decide the exact cut line. The `impl GameNote` block
   holding `mine` goes with the struct; `GameNotesVec` / `NotesVecError` stay.
2. Create the new module by moving text, not retyping it, so the doc comments and constants cannot
   drift.
3. Split the remainder into its own file under `note_types_expansion`, importing the type from its
   new home.
4. Fix the five import sites; `cargo check` will find any you miss.
5. Confirm by inspection that the only differences in the moved code are the module path and any
   sentence that referred to the old location.

## Acceptance Criteria

1. **The reading half is shared and mod-independent**
   - Given the completed change
   - When `src/types/game_note.rs` is inspected
   - Then it contains the `GameNote` layout, the four constant modules, `for_each_result`,
     `actor_results_range` and `GameNote::mine`, and it imports nothing from `src/mods/`

2. **The injection half stayed behind**
   - Given the completed change
   - When `src/mods/note_types_expansion/` is inspected
   - Then `GameNotesVec` and `NotesVecError` live there, in a file named for what they do, and the
     mod's `mod.rs` declares it

3. **There is exactly one path to each item**
   - Given the completed change
   - When the repository is searched for the moved item names
   - Then the old `note_types_expansion::game_note` module no longer exists and no re-export shim
     stands in for it

4. **Nothing changed but the location**
   - Given the diff
   - When it is reviewed
   - Then every layout constant, function signature and doc comment is byte-identical to before
     apart from module paths and location references — no offset, value or behaviour is altered

5. **Note types still work**
   - Given the built DLL installed in the local game
   - When the game boots and a chart with mod-injected mines is loaded
   - Then the boot log shows `note-types-expansion` initialising as before and mines behave exactly
     as they did prior to this change (this is the change's only real risk, and it is the maintainer's
     to confirm on a mine chart)

6. **The build gates pass**
   - Given the completed change
   - When `cargo check --target x86_64-pc-windows-msvc`, then `cargo fmt`, then `./build.sh` are run
   - Then all three complete cleanly, with no new warnings

## Metadata
- **Complexity**: Low
- **Labels**: refactor, module-layout, note-records, no-behaviour-change
- **Required Skills**: Rust module organisation and visibility; the discipline to move
  reverse-engineered constants without "improving" them
- **Generated By**: code-task-generator 2026-07-26
- **Source Plan**: `.agents/planning/20260725-assist-tick/implementation/plan.md`
- **Plan Step**: Step 3 — `mods::assist_tick` — end-to-end ticking on the dispatched actor
