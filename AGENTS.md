# AGENTS.md

## Project Summary

Rust hook DLL (`cdylib`, `x86_64-pc-windows-msvc`) for DanceDanceRevolution World arcade. Loaded into the live game process via spice2x's `-k` flag. Installs inline function hooks and checked byte patches against `gamemdx.dll` and sibling Konami DLLs (`libavs-win64`, `libafp-win64`, `arkmdxbio2`, `ess`, `xactengine2_10`) to add mods that render through the game's own UI and 3D pipelines — widgets, option rows and the 3D background are native game objects, not overlays.

Almost every interesting constraint comes from running inside someone else's process: allocator matching, FFI calling conventions, no panics across `extern "C"`, no hardcoded offsets (everything is AOB-scanned, RTTI-walked or derived so it survives game updates).

The architecture knowledge base is `.agents/summary/index.md` — start there for layers, the hook-ownership map, config schema and workflows. Per-feature mechanisms are documented in each module's `//!` header; RE investigations are in `docs/`.

## Directory Map

```
src/
├── lib.rs        # DllMain + the load-bearing init() sequence + splash
├── core/         # game-agnostic: scanner, signatures, hooks, memory patching, frame pump, formats (arc/ifs/afp/ap2/ssq/xact/anm)
├── services/     # game-system integrations; each hooked game function has exactly one owning service
├── mods/         # one module per mod (Mod trait) + config.rs (mod-config.json)
├── widgets/      # TextWidget / ImageWidget (native game objects)
└── types/        # scene ids, buttons, GameNote
updater/          # standalone auto-updater crate (NOT a workspace member)
tools/            # blender_ddr_addon, bot_sim (offline bot simulator), fxc (pinned HLSL compiler)
shaders/src/      # HLSL → committed blobs in data_mods/shader_fixes/blobs/
scripts/          # validate_* host harnesses, sig_harness, gen_* asset generators, format tools, game_nav, release
data_mods/        # shipped runtime assets (LayeredFS mod folders)
docs/             # RE research notes (addresses file-relative to 0x180000000)
.agents/          # summary (generated), steering, learnings, planning (PDD), tasks, scratchpad
```

## Where to Start

| Task | Start here |
|---|---|
| Add a mod | `src/mods/mod_trait.rs`, then construct/register in `src/lib.rs`; `DEFAULT_OFF_MODS` for default-off |
| Add a service | `src/services/mod.rs`, init call in `src/lib.rs` (order is load-bearing) |
| Add/change a signature or derivation | `src/core/signatures.rs`, then the signature sweep below |
| Hook something already hooked | `.agents/summary/interfaces.md` → Hook Ownership; subscribe to / `acquire` from the owner |
| Per-player option row | `src/services/custom_options/api.rs`; labels via `scripts/option_strings.py` → `scripts/gen_option_labels.py` |
| Cabinet-wide overlay-menu row | `src/mods/mod_menu/rows.rs` |
| Config section | `src/mods/config.rs` (schema + writers); ownership table in `.agents/summary/data_models.md` |
| File replacement / textures / shaders | `src/services/avs_layeredfs/` |
| Score-submission policy | `src/services/score_guard.rs` + `src/services/custom_options_persistence.rs` |
| Auto-updater | `updater/src/main.rs` |
| One specific feature | its row in `.agents/summary/components.md` → the module's `//!` doc → the named `docs/` note |

## Build & Validation

```bash
cargo check --target x86_64-pc-windows-msvc      # fast type check (works on any host)
./build.sh                                       # release DLL (cargo-xwin)
./build_win7.sh                                  # Windows 7 build (-Z build-std)
./scripts/deploy.sh                              # build + scp to cabinet (/tmp/ssh{host,user,pass})
./scripts/build_release_archive.sh               # release/: zip + bare updater exe + tester install .bat
cargo test --manifest-path updater/Cargo.toml    # updater tests (separate crate, runs natively)
./scripts/validate_<area>.sh                     # host tests for pure modules (see below)
./scripts/validate_signatures.sh ~/Desktop/ddr_modules   # offline sweep over every supported gamemdx build
```

- **Plain `cargo test` does not build the DLL crate on ARM hosts** (`retour` has no aarch64 backend). Pure logic is tested by `scripts/validate_*.sh`, which build a throwaway crate that `#[path]`-mounts dependency-free source files — so harness-mounted files must not use `crate::` imports. Some legs need `$DDR_WORLD_INSTALL` or sibling `ddr-chart-tools` / `bemaniutils` checkouts.
- Engine-facing code has no harness: validation is a cabinet deploy + log observation (spice2x `log.txt`, `ddr_hook_crash.log`). `scripts/game_nav/` automates a CrossOver cabinet over SpiceAPI.
- **Signature sweep** after touching `signatures.rs` or any consumer-side fixed offset: `validate_signatures.sh` must be green (misses only where a `_vN` alternate or `scripts/sig_harness/report.py::ALT_GROUPS` covers them), and run `scripts/sig_harness/shape_diff.py` for anything that reads bytes at `match+N` — an AOB matching every build proves nothing about the bytes after it.
- **Readiness gate** before handing a build back: `cargo check` clean → `cargo fmt` (whole crate — never pass file args; a targeted `cargo fmt -- <file>` still formats everything) → `./build.sh` clean → sweep green if signatures changed.
- A DLL-only deploy does not carry `data_mods/`: new option-row label textures, shader blobs or art must be copied too, or labels render blank / features degrade.

## Patterns That Deviate From Defaults

- **`static mut` for hook state** — intentional; `extern "C"` callbacks can't capture. Access via `std::ptr::addr_of!` with null guards.
- **`unsafe impl Send`** — raw pointers to game memory are valid for the process lifetime.
- **`#![allow(dead_code)]`** crate-wide — many items are only reached from hook callbacks.
- **No `println!`/`eprintln!`** — log through `log_info!`/`log_warn!`/`log_error!`/`log_debug!` (→ `OutputDebugStringA`, visible in spice2x's log).
- **One detour per target function, ever.** Shared targets are owned by a dispatcher/acquire service (`judge_hook`, `render_notes_hook`, `analyze_hook`, `hud_layout_hooks`, `combo_hooks`, `call_voice_hooks`, `bottom_text`, `movie_policy`, `foot_panel_swap`, …); full map in `.agents/summary/interfaces.md`. If a target is privately owned by one mod and a second consumer appears, promote it to a service.
- **Three allocator heaps** — game CRT (`game_malloc`), AGCS app heap (`agcs_heap_malloc`), our own VirtualAlloc (`memory::alloc_zeroed`/`alloc_near`). Whoever frees decides; a mismatch crashes in `RtlFreeHeap`.
- **Pure logic in harness-mountable files** — decision/format code lives in separate `crate::`-free files (`*_math.rs`, `*_logic.rs`, `model.rs`, `plan.rs`, …) so `scripts/validate_*.sh` can test it.
- **`Mod::is_active` means "this mod CAN work", never "its effect landed this boot".** The registry records `enabled = is_active()` after `enable()`. Boot-only mods (custom-resolution, fps-unlock, gameplay-timing-fixes) must return `true` whenever their sites resolved — including when being turned ON from the menu for the next launch — or the menu persists the toggle as off. The registry tracks `requested` (intent, persisted) separately from `enabled` (effective).

## Cross-Cutting Gotchas

- **Init order in `src/lib.rs` is load-bearing.** `ident_override::init()` is the very first call (must beat the launcher to `ea3_boot`); config + LayeredFS install before the gamemdx wait and the AOB scan (the game reads `shader.arc`/`musicdb.xml` within ms of gamemdx loading). Never move LayeredFS back below `resolve_all`.
- **Build-dependent layouts are derived, never hardcoded.** Several actor/field offsets differ between supported `gamemdx` builds; use the typed accessors in `signatures.rs` or add a derivation (`publish_value` for non-address values). Before dereferencing a pointer read from a game object whose layout no AOB pins, probe `memory::is_readable` — a vtable identity check on a pointer read from a wrong offset protects nothing.
- **Scene ids are 0-indexed** everywhere except `agcs::Sequence::finish` (`sequence_finish`), which takes 1-indexed ids. Scene callbacks fire *before* `createNextSequence` builds the next sequence — the new scene's objects don't exist yet.
- **Bot phantom side.** During a multiplayer-bot song both sides read as entered; any cabinet-wide policy folding both players' option values must exclude `multiplayer_bot::is_bot_side(side)`.
- **Score integrity is fail-closed.** Anything that lets a player alter a song's outcome must taint through `score_guard`; if sanitisation can't initialize, saves are suppressed rather than forwarded.
- **DLL-written config sections are rewritten whole** by their owner — a writer must emit every key of its section (see `data_models.md`). Operator-only sections are never written.
- **Generated artifacts:** never hand-edit generated option-label PNGs (edit `scripts/option_strings.py` and regenerate) or generated camera clips (edit the generator's table). `data_mods/_cache/` and enable-time `*_ifs/` output are machine-owned — never commit them.

## Detailed Documentation

- `.agents/summary/index.md` — generated knowledge base (architecture, components, hook ownership, data models, workflows, dependencies, review notes). Regenerate with the codebase-summary workflow; don't hand-edit.
- `.agents/steering/rust-hooking.md` — allocator rules, callback patterns, shared_ptr layout, decoding primitives, design rationale.
- `.agents/steering/reverse-engineering.md` — target modules, address conventions, derivation anchors, research-note structure.
- `.agents/learnings/learnings.md` — hard-won project-specific traps. Check it when something feels subtle.
- `.agents/planning/<feature>/` — PDD docs per feature (`_archive/` = completed).
- `docs/` — RE research notes. `README.md` — user/operator documentation.

## Custom Instructions

<!-- This section is maintained by developers and agents during day-to-day work.
     It is NOT auto-generated by codebase-summary and MUST be preserved during refreshes.
     Add project-specific conventions, gotchas, and workflow requirements here. -->

### Workflow

Solo-maintainer repo: sole owner, no code-review system, no CI gate. The maintainer
manages `git commit` / push themselves. When working through a multi-step plan or task
list, don't stop and hand back between tasks waiting for a CR or approval handoff —
continue through the plan in the same session, running each step's validation as you
go. Commit/push only when the maintainer asks.

### Git rules (agents)

- **NEVER run `git commit` (or push) unless the maintainer explicitly asks in the
  current session.** This includes commits mandated by skills/SOPs (e.g. code-assist's
  Commit step): skip that step, leave the work staged or unstaged, and report what
  would have been committed. Where a workflow tracks completion by commit hash, record
  `Status: Complete (uncommitted — maintainer commits manually)` instead.
- When the maintainer DOES authorize a commit, write a plain conventional-commit
  message with **no attribution trailers** — no `🤖 Assisted by …`, no
  `Co-authored-by`, no tool footers.
- **Never write the maintainer's local username or a machine-specific absolute path
  into ANY tracked artifact** — source, docs, planning/research notes, scripts,
  generated reports/tables, fixtures, logs. Not `/Users/<name>/…`, not
  `/home/<name>/…`, not the CrossOver bottle path spelled out. Use `~/…` or `$HOME`
  in prose and tables, repo-relative paths for anything inside the repo, and the
  `DDR_WORLD_INSTALL` env var (with NO hard-coded absolute fallback) for the game
  install; scripts that print paths must print them `~`-relative (invert
  `os.path.expanduser`). Tool output pasted into notes (cargo/build logs, sweep
  reports, backtraces) carries absolute paths — strip them before saving. Before
  handing work back, `git grep -nE "/(Users|home)/[^/ ]+/" -- . ':!target'` must add
  no new hits (pre-existing hits: a few tracked `.agents/scratchpad/**/logs` files
  and the vendored `scripts/arctool` / `spice2x-cli/spice2x-cli` binaries — leave
  those to the maintainer).

### Documentation scope

- AGENTS.md holds only what an agent needs on every task: navigation, repo-specific
  tooling, deviating patterns and cross-cutting rules. Do not add per-feature mechanism
  write-ups, RE findings, deploy histories or decision logs here — mechanisms go in the
  module's `//!` doc, investigations in `docs/`, in-flight feature state in
  `.agents/planning/<feature>/progress.md`, and non-obvious traps in
  `.agents/learnings/learnings.md`.

### Code Navigation (LSP)

This project is configured with a **rust-analyzer LSP** — prefer it over `grep`/`rg`
for code intelligence: `goToDefinition`, `findReferences`, `hover` (types/signatures),
`documentSymbol`, `workspaceSymbol` (find a symbol by name across the crate), and the
call-hierarchy ops. It resolves `static`/trait/`extern "C"` symbols and gives exact
types where text search guesses. Note: rust-analyzer indexes the whole crate on first
use, so the first query in a session may return "server is starting" — retry after a
moment. Keep `grep` for non-symbol scans (AOB byte patterns, comments, log strings).

### Rust Quality Rules

Shaped by being an in-process hook DLL where the engine-facing code has no test harness:

1. **Panics must not cross FFI.** Hook callbacks are `extern "C"` — a panic unwinding into game code is undefined behavior. Wrap fallible callback bodies in `std::panic::catch_unwind` (see `scene_manager` for the pattern) or keep them strictly panic-free. No `unwrap()` / `expect()` / indexing / `unreachable!()` inside a hook callback, render-thread closure, or input callback; reserve those for init-time code, and prefer graceful degradation even there.
2. **Graceful degradation over hard failure.** If a signature doesn't resolve or a service fails to init, log a warning and continue — other mods may still work. Most service functions return `Option<T>`; callers check `is_available()`. Use `require_address()` (panicking) only when a mod literally cannot function without that address, and declare it in `required_signatures()` so `ModRegistry` can skip the mod cleanly.
3. **Respect thread boundaries.** Widget creation/mutation and texture resolution happen on the game's render thread — use `widget_renderer::run_on_render_thread()`. Background threads can read but not mutate. Do not hold a state `Mutex` across a `run_on_render_thread` schedule (the closure deadlocks against your own lock).
4. **Keep hot-path callbacks tight.** `judgeNotes`, the render hook, and `input_manager::poll` run every frame. Work over ~1 ms stutters the game — stash a flag and defer heavy work to another thread or a later render-thread closure.
5. **Use the scanner primitives.** `core/scanner.rs` provides `decode_rip_relative`, `decode_call_rel32`, `scan_first_call_rel32`, `scan_xrefs_to`. Don't reimplement RIP-relative decoding inline; add a new primitive to `scanner.rs` once it has two call sites.
6. **No hardcoded offsets in hook code.** Addresses come from AOB signatures (`core/signatures.rs`), RTTI walks, or RIP-relative derivation from a scanned landmark. Absolute addresses are acceptable only in `docs/` research notes, written file-relative to the module base.
7. **Stay inside the module layout.** New functionality belongs in `core/` (game-agnostic), `services/` (game-system integrations), `widgets/`, `mods/`, or `types/`. Mods that outgrow a single file get a subdirectory (`mods/note_types_expansion/` is the reference).
8. **`unsafe` is expected — discipline isn't optional.** Keep `unsafe` blocks narrow (scoped to the operation, not the function) and structure the surrounding code so the soundness argument is obvious.
9. **Verify after changing a hook.** "It compiles" is a weak signal. After touching `signatures.rs`, a hook callback, or a memory layout, run `cargo check` *and* plan a deploy test — observe the relevant logs and visual behavior. When investigating apparent runtime bugs, ship a diagnostic build with one-shot WARN/INFO logs on every fallback branch before rewriting (see `.agents/learnings/learnings.md`).

### PDD feature progress tracking

For features developed under `.agents/planning/<date>-<name>/`, maintain a `progress.md`
in that feature directory throughout implementation. It is the live resume point — written
so a fresh agent with zero context can pick up the work after a context-window reset by
reading it once. It complements `implementation/plan.md` (the stable plan + step checklist),
it does not replace it.

- **Top of file:** an `Updated:` date, a `Status:` line (`Step N of M — in progress|blocked|
  done`), and a single **`NEXT ACTION:`** line stating the exact next thing to do (file /
  function / command), plus a one-line resume protocol pointer to the plan/design/research.
- **Sections:** `Done` (one line per completed step + outcome), `In flight` (current edit /
  uncommitted work), `Deploy & test log` (each cabinet deploy → observed result; this repo's
  only real validation), `Deviations & open questions`, and a condensed `Key facts for a cold
  resume`.
- **Update it after each implementation step and before any long pause / handoff.** Keep it
  tight — it's a resume aid, not a journal.
- **Per-task working documents belong to the skills that create them.** Skills like
  `code-assist` (invoked directly or via `code-task-generator`) keep their own per-task
  artifacts — context/plan/progress/logs — in their own default working directories and
  use markers like a `Status: Complete <hash>` line in the task's `progress.md` to track
  task/step completion. Let them: those markers are how the tooling decides whether a
  plan step's tasks are done. The artifacts are task-scoped working records; the
  feature-level `progress.md` (planning dir) plus the ticked checklist in
  `implementation/plan.md` remain the cross-session resume points. Don't duplicate one
  into the other — summarize and link instead.
- **Task files** produced by `code-task-generator` live under
  `.agents/tasks/<feature>/step<NN>/`. They are inputs to implementation, not a progress
  record — never track status in them.

### Shared movie hook ownership

- `src/services/movie_policy.rs` owns the sole `DShowPlayer::BuildGraph` detour. `non_native_os_support` only toggles `MovieSuppressor::NonNativeOs`; song-rate suppression has its own contributor, set tentatively at a non-identity scene-26 arm and confirmed at commit (background movies are suppressed for rate-played songs — the DirectShow graph clock cannot follow the XACT rate).
