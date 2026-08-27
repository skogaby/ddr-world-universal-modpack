# Design: NoteTypesExpansion — Mines (ITG/StepMania)

**Requirements**: [requirements.md](requirements.md)
**Parent SIM**: none (personal project)

---

## Overview

Add a new mod, `NoteTypesExpansion`, that introduces a first ITG-style note type — **mines** — into DDR World by teaching the game's own note pipeline about a new `kind` value and feeding mines through the vanilla render and judge paths. The mod is designed as a framework with pluggable note-type implementations so future additions (lifts, rolls) extend the same scaffold.

Mines live in a new SSQ chunk (`kind = 20`, `MINE_DATA`) that vanilla DDR silently skips. On modded DDR we parse that chunk during the existing `IStepReader::Analyze` post-processing pass, inject one `step::Note` per mine into the game's own `Notes` vector with a new `kind` value, and let the game's native rendering pipeline draw them — inheriting scroll speed, boost, reverse, appearance, and any future scroll transforms Konami adds.

Judge-time handling is done by marking mine `Results` as "already judged" (setting `entry+0xC` to a non-`0xFF` sentinel) before the vanilla judge runs, so the judge iterates past them. A sidecar `Vec<MineEntry>` keyed by `(musicCount, panel)` drives mine-hit penalties from a post-judge pass. The same "mark as judged" trick also causes `AutoFootPanel::update` to naturally avoid mine panels (US-6 falls out for free).

Mine-note allocation uses the game's **`agcs::ApplicationHeap`** allocator — the same heap `agcs::stl::vector<Note>` itself uses — to avoid heap-mismatch crashes at chart end when the game's vector destructor runs. A single bulk append is performed once per chart load; there is no per-entry grow path.

To allow multiple mods to hook `GamePlayActor::judgeNotes` safely, this feature introduces a shared `services::judge_hook` dispatcher. The existing `autoplay` mod is refactored to use it. NoteTypesExpansion registers its pre/post-judge callbacks through the same service.

Sprite delivery goes through the existing AVS LayeredFS service — no new custom ARC packaging — continuing the team's migration away from `register_arc` codepaths.

---

## Architecture Decisions

### Decision 1: Inject mines into the game's `Notes` vector with a new `kind`, not as a sidecar-only render pass

**Problem**: Mines need to scroll, render, and time-sync identically to regular arrows (same speed/boost/reverse/appearance transforms, same playhead-relative scroll math, same draw pass ordering). Two viable approaches: (A) inject mines into the game's `Notes` vector as entries with a new `kind` value and hook the renderer's `kind` filter to let them through; (B) keep mines exclusively in a mod-side sidecar table and hook the renderer to issue a parallel draw pass using the game's sprite infrastructure.

**Decision**: Option A. Mines become entries in the game's own `Notes` / `Results` vectors with `kind = MINE` (a new value outside the vanilla enum). The renderer's `kind != ARROW` filter in `collect_render_notes` (`FUN_1800240C0`) is widened via a hook to also accept `kind == MINE`, and texture binding for mine-kind entries is rebound to the mine sprite. Judge-time hiding is handled separately (see Decision 5).

**Rationale**:
- The game's renderer computes scroll Y, appearance alpha, speed/boost multipliers, reverse transforms, and every other per-note visual transform from fields on the `Note` struct directly. Putting mines into that struct means every transform applies to them for free — including any future transforms Konami adds in later builds.
- The alternative (B) requires us to re-drive the draw pipeline: read `speed`, `boost`, `musicCount`, `offsetY`, and appearance parameters off the ArrowRenderer, call `getOffsetY`/`calcAppearanceAlpha`/`renderSprite` in the correct sequence, and push quads into the right sprite pool. That parallel path would silently diverge every time Konami touches the render internals, and we've already seen Konami add new scroll transforms across builds.
- Injection is a bulk append into the game's `agcs::stl::vector<Note>` at a well-defined post-parse moment (`IStepReader::Analyze`, `FUN_1801C6D80`). The vector has a 0x60 byte stride. Allocation must use the **app heap allocator** the game uses for this vector (see Decision 9), NOT `game_malloc` (CRT malloc). The injected bytes are pre-computed and appended in one pass — no per-mine grow path.

**Alternatives Considered**:
- **Sidecar-only (Option B)**: safer failure profile (if our render hook fails, mines are invisible rather than mis-judged), but duplicates the scroll/transform pipeline and drifts against future game updates. Rejected on maintainability.
- **Reinterpret shock arrow bytes**: feasibility doc dismissed this early — vanilla DDR emits a shock arrow on the marker byte before we can intercept. Rejected up front.

**Tradeoffs**: Judge-skip is now a hard dependency — if our pre-judge hook fails, vanilla judge sees mine `Results` with `state[panel]=1` and would treat them as 1-panel arrows. Mitigated by the `required_signatures()` mechanism: the mod refuses to register if the `judge_notes` signature can't be resolved.

### Decision 2: New SSQ chunk type `kind = 20` (`MINE_DATA`), keyed per-difficulty via `param2`

**Problem**: Need a chart-file representation for mines that is (a) forward-compatible on vanilla DDR (zero behavioral impact on unmodded cabinets), (b) carries exactly the information the DLL needs (tick + panel), (c) supports unique mine placements per difficulty within a single song (parity with how regular step data works — each difficulty has its own step chunk), (d) leaves room for future note types without further chunk proliferation.

**Decision**: A new chunk type, `kind = 20` (`0x14`), with one chunk per difficulty per song. The chunk's `param2` header field carries the same **difficulty+style code** that vanilla step chunks use (per `docs/ssq_format.md §5.1`): `0x0114` = Single Basic, `0x0214` = Single Difficult, …, `0x0618` = Double Challenge. A song with mines on only some difficulties emits mine chunks only for those difficulties. Future note types (lifts, rolls) get their own chunk kinds (21, 22, …), each with the same per-difficulty `param2` convention. Chunk body is an array of fixed-size 8-byte entries: `{i32 beatCount, u8 panels, u8 flags, u16 reserved}` where `panels` is the bitmask from `docs/ssq_format.md §5.3`.

**Rationale**:
- **Per-difficulty parity with regular notes.** A chart author can design mine placements that work with that difficulty's stepchart — e.g. sparse mines on Basic, denser traps on Challenge. Without per-difficulty keying, either all difficulties share mines (breaks chart design) or the entire mod can't express the common case.
- **Zero new semantics** — the `param2`-as-difficulty-code convention is already established by step chunks (kind=3). The DLL's Analyze hook receives `(mode, difficulty)` as function arguments on every invocation, converts them to the standard 16-bit code, and does a `find_chunk(kind=20, param2=<that code>)` lookup. Exactly the same pattern the vanilla step parser uses.
- Chunk kinds observed in DDR World's 1523-file sample: `1, 2, 3, 4, 5, 9, 17`. Value 20 is above the legacy range and leaves clearance for Konami to add kinds 10–19.
- The SSQ parser's chunk walker advances by `chunk.length` regardless of whether the `kind` is recognized — unknown kinds are silently skipped. Verified behavior across all 1523 shipped charts.
- Fixed 8-byte entry stride keeps parsing trivial, pads to dword alignment, and leaves headroom in `flags` + `reserved` for future per-mine variants (cold mines, silent mines, etc.) without a schema bump. `param2` is NOT the per-mine-variant escape hatch — it's the per-difficulty key — so mine subtypes land in `flags`.
- Per-type chunks (vs. one generic "custom notes" chunk) keep each note type's wire format self-describing.

**Alternatives Considered**:
- **Single MINE_DATA chunk per file, `param2 = 0`, mines shared across all difficulties**: simpler format but breaks the common case where different difficulties want different mine sets. Rejected.
- **Generic custom-notes chunk with per-entry type tag and per-entry difficulty field**: more compact for songs with few mines, but conflates concerns, forces all note types to share one wire format, and introduces a redundant per-entry difficulty tag when the existing step-chunk convention already uses chunk-level `param2`. Rejected.
- **Reuse an existing kind (e.g., 4 or 5) with overloaded params**: types 4 and 5 have established legacy semantics in 96 charts. Overloading risks confusion for any third-party tool. Rejected.
- **Higher kind value (50, 100)**: more conservative against Konami encroachment, but arbitrary. 20 is chosen for compactness.

**Tradeoffs**:
- If Konami adds an official chunk with `kind = 20` in a future DDR update, our chunks collide. Mitigation: if Konami's chunk uses `param2` outside the step-difficulty-code set, the DLL's lookup continues to work (we only match on valid difficulty codes). If Konami's chunk collides on a valid difficulty code, migration to a new kind (21+) is a `ddr-chart-tools` + DLL release.
- `param2` is no longer available as a subtype discriminator within MINE_DATA. Per-mine variants instead live in the entry-level `flags` byte. This is actually cleaner — subtype info belongs per-entry, not per-chunk — and was codified in the spec's forward-compat section.

### Decision 3: Mine `Note` records use `state[panel] = 1` on exactly one panel

**Problem**: The vanilla shock classifier checks `state[0..3]` or `state[4..7]` all == `TRG` (1). A mine covering all four panels would be mis-classified as a shock arrow. Also, `state[dir]` carries semantic meaning (0=NONE, 1=TRG, 2=REC, 4=GEN) that downstream code reads.

**Decision**: Each mine `Note` has `state[panel] = 1` on exactly one panel (the mine's panel) and `state[*] = 0` elsewhere. Simultaneous mines on different panels at the same tick become multiple `Note` entries at the same `musicCount`, each with a single bit set. `length[*] = 0` always — mines have no duration.

**Rationale**:
- Guarantees the renderer's shock classifier returns false for mines — single set bit cannot match the four-in-a-row pattern.
- Matches the renderer's expectation of one render entry per `state[dir] != NONE` — the renderer's inner loop walks `state[]` and emits a render quad per set bit, which is exactly what we want for a single-panel mine.
- The judge never sees mines (pre-judge hook marks them "already judged" per Decision 5), so the single set bit is never interpreted as a 1-panel arrow.

**Alternatives Considered**:
- **New `state` value (e.g., 5 = MINE)**: every `state` check in the renderer would need widening. Rejected — too large a change surface.
- **Mines with all `state` zero, discriminated by `kind` alone**: the renderer's `state[dir] != NONE` gate in the inner loop means zero-state mines wouldn't render at all. Rejected.

**Tradeoffs**: Relies on the judge-skip hook being installed. If our hook fails and a mine gets into the vanilla judge loop, it'd be treated as a single-panel arrow (graceful degradation — no crash, wrong score).

### Decision 4: Sidecar table for mine-hit judgment, keyed by `(musicCount, panel)`

**Problem**: The post-judge hook needs to know "is there a mine at panel P within the timing window of current musicCount?" Reading this from the game's `Results` vector every frame means re-iterating game memory on the hot path, mixed in with all regular arrows.

**Decision**: When mines are injected during `Analyze`, we also build a mod-owned `Vec<MineEntry>` sorted by `musicCount`. The post-judge hook binary-searches the narrow window `[musicCount - window, musicCount + window]` and runs mine-hit logic from there. The `Note` entry in the game's vector is for rendering only; the sidecar is the source of truth for mine state (consumed, expired).

**Rationale**:
- O(log N) lookup per frame, independent of regular arrow count.
- Decouples mod-owned judgment state (consumed, expired) from the game-owned `Results` vector — we don't mutate `Results` beyond the brief "mark as judged" pre/post-pass.
- Rebuilt once per chart load, torn down at chart end — same lifecycle as the game's `Notes` vector.

**Alternatives Considered**:
- **Re-scan `Results` at judge time**: O(N) in total notes per frame. Wasteful on large charts. Rejected.
- **Mutate the game's `Results` entries persistently to track mine state**: writes to game memory beyond what we own. Rejected — we only touch `entry+0xC` for the pre/post-judge sentinel flip.

**Tradeoffs**: Two structures to keep consistent (game `Notes` for rendering, sidecar for judging). Both built together in the same `Analyze` hook pass, both torn down together at chart end. Invariant is straightforward.

### Decision 5: Hide mines from the vanilla judge via a "mark as already-judged" sentinel

**Problem**: The vanilla judge reads `state[]` but never `kind`. If a mine `Note` has `state[panel]=1`, the judge will match on a panel press and treat it as a 1-panel arrow — wrong behavior. We need to prevent the judge from processing mine entries without patching the judge function itself.

**Decision**: Before the vanilla judge runs each frame, iterate `Results` and set `entry+0xC = 0xFE` (any non-`0xFF` value) on entries whose underlying `Note.kind == MINE`. The judge loop's `*(int*)(entry + 0xc) == 0xff` gate skips them. After the vanilla judge returns, iterate `Results` again and restore `entry+0xC = 0xFF` on those same entries so they can be revisited on the next frame.

**Rationale**:
- No mid-function patching of the judge. Clean entry/exit wrapping via the shared dispatcher (Decision 6).
- The sentinel is a value the judge loop already handles gracefully — we're using the game's own "already processed" fast-path to bypass processing. No new judge semantics.
- **AutoFootPanel::update also filters on `entry+0xC == 0xFF`** (per `docs/autoplay.md`). Same sentinel causes autoplay to naturally avoid mine panels — US-6 (autoplay avoidance) falls out for free with no separate autoplay-mask hook.
- Value `0xFE` is chosen because it's `!= 0xFF` (so judge skips) and not a valid `step::judge::KIND` enum value (0..5=grades, 6=OK, 7=NG, 0xFF=INVALID), so no code path confuses it for a real grade.

**Alternatives Considered**:
- **Mid-function patch in `judgeNotes` to skip `kind != ARROW`**: requires a stable AOB at a mid-function instruction. More fragile across game updates than a pre/post-wrapping hook.
- **Clear `state[]` on mine entries during judge, restore after**: also works, but touches a larger field and could interact badly with `collect_render_notes` if render runs between pre-judge and post-judge (it doesn't — render and judge are sequential in `GamePlayActor::update` state 4 — but wouldn't want to rely on that invariant).

**Tradeoffs**: Two iterations of `Results` per frame (once pre, once post). `Results` is small (one entry per active note in the judge window) — overhead is negligible.

### Decision 6: Shared `judge_hook` service — single detour on `judgeNotes` with priority-ordered callbacks

**Problem**: Both `AutoplayMod` (existing) and `NoteTypesExpansionMod` (new) need to intercept `GamePlayActor::judgeNotes`. The existing pattern installs independent `retour::GenericDetour` handles per mod. Retour doesn't compose nested detours on the same function cleanly — the second-installed detour's trampoline captures the first detour's jmp as the "original," so one mod's callback silently bypasses the other depending on install order.

**Decision**: Introduce `services::judge_hook` as a shared dispatcher. One `GenericDetour` is installed on `judgeNotes` during service init. Mods register pre-judge and post-judge callbacks with a priority. The detour dispatches:
1. All pre-judge callbacks in ascending priority order
2. The original `judgeNotes` function
3. All post-judge callbacks in ascending priority order

Priorities are coarse: `Priority::Early`, `Priority::Normal`, `Priority::Late`. Two mods at the same priority run in registration order (documented non-guarantee; priorities exist specifically to avoid relying on it).

**Rationale**:
- Correctness: single detour install point means no retour stacking issues.
- Deterministic ordering independent of mod-registration order.
- Extensible: any future mod that needs to intercept the judge uses the same mechanism.
- Each callback receives `(actor: *mut u8, music_count: i32)` — enough state to do anything the mod needs. No type-erasure in the callback shape.

**Callback ordering for mines + autoplay** (both enabled):
```
1. NoteTypesExpansion pre(Early):  mark mine Results as "already judged" (entry+0xC = 0xFE)
2. Autoplay            pre(Late):  swap foot panel to AutoFootPanel, call AutoFootPanel::update
                                   (AutoFootPanel skips marked mines — autoplay avoidance for free)
3. Real judgeNotes runs:            processes regular arrows, skips marked mines
4. Autoplay            post(Early): restore foot panel pointer
5. NoteTypesExpansion  post(Late):  un-mark mine Results (entry+0xC = 0xFF)
                                   scan sidecar for mines in timing window → detect hits from
                                   current (user or auto) foot panel state → apply penalties
```

**Alternatives Considered**:
- **Attempt to compose nested retour detours**: investigated; retour's trampoline mechanism doesn't preserve the call chain when detours are installed on the same function in sequence. The second `call()` bypasses the first callback. Dead end without retour-internals patches.
- **Leave autoplay with its own detour and make NoteTypesExpansion tolerate whatever happens**: brittle, silently broken, no way to enforce ordering. Rejected.

**Tradeoffs**: This is a scope expansion — we're refactoring `autoplay.rs` as part of this feature. The autoplay refactor is small (~30 LOC, mechanical: move hook body into two closures registered with the service) and behavior-preserving (we verify autoplay still works before layering mines on top). Implementation order: ship the shared service + autoplay refactor as the first tasks, then layer NoteTypesExpansion on top.

### Decision 7: Sprite delivery via LayeredFS (no custom ARC)

**Problem**: Mines need a visually distinct on-screen sprite. Existing texture-packaged mods use `asset_loader::register_arc`, which requires building a custom ARC + IFS. Per team direction, that path is being deprecated in favor of LayeredFS serving raw files from disk.

**Decision**: The mine sprite is served via AVS LayeredFS from `data_mods/note_types_expansion/`. The render hook, when it encounters a `kind == MINE` entry in `collect_render_notes`, rebinds the quad's texture to the mine sprite (resolved by name via the existing `texture_resolver` service). The mine texture can be authored as a PNG and dropped into the mod folder under the appropriate `data/textures/` path — LayeredFS + the existing IFS texture injection pipeline handle PNG→BGRA/DXT5 conversion on load.

**Rationale**:
- No ARC build step. Artist updates a PNG and restarts the game.
- Consistent with `FolderExpansionMod`, the team's reference example for LayeredFS-based asset injection.
- Falls back gracefully if the texture is absent: the render hook leaves the default arrow-atlas binding in place and logs a warning. Mines visible as regular-arrow variants — degraded but non-breaking.

**Alternatives Considered**:
- **Custom ARC via `asset_loader::register_arc`**: deprecated. Rejected.
- **Clip-and-reuse shock_effect sprite at single-panel width**: listed as an acceptable v1 shortcut in requirements. On inspection, it's not meaningfully simpler than a dedicated sprite (either way we rebind texture + UV in the hook). Dedicated sprite chosen for cleaner visual identity.

**Tradeoffs**: Requires a one-time art task to produce the mine PNG before visual acceptance testing. Small.

### Decision 8: Framework is a `NoteType` trait with a `NoteTypeRegistry`

**Problem**: Requirements call for a framework, not a mine-specific mod. The design needs to name the extension points now even though only mines ship in v1.

**Decision**: A `NoteType` trait with one impl per type. Each impl owns: parse (read its SSQ chunk), inject (build `step::Note` records, append to game `Notes`, populate sidecar), render binding (declare `kind` value + texture name + UV for the render hook to look up), judge (own per-frame logic driven by the sidecar), and reset (per-chart teardown). A `NoteTypeRegistry` held by the mod invokes registered types at each hook dispatch point.

Autoplay avoidance is not a separate trait method for mines, because the "mark as already-judged" sentinel handles it automatically (per Decision 5). Future note types with different semantics (e.g., lifts triggered on release, not press) may need an explicit autoplay-mask method — the trait is extended at that point.

**Rationale**:
- Each note type's behavior is colocated in one module, not scattered across parse/render/judge.
- Adding a new note type is a new `NoteType` impl + a `register()` call. No rewrites of the hook scaffolding.
- Trait-based dispatch keeps the registry generic; the registry module doesn't need to know about mines.

**Alternatives Considered**:
- **Monolithic mine mod, refactor framework later**: defers structure cost but violates US-9. Rejected.
- **Compile-time enum of note types vs. `Box<dyn NoteType>`**: implementation detail, leave to the implementer.

**Tradeoffs**: One-implementation framework risks under-specifying extension points for future types. Mitigated by grounding the trait methods in what lifts/rolls concretely need based on DDR's observed judge/render behavior (e.g., judge method receives `foot_panel` pointer so lifts can query release state, not just press state).

### Decision 9: Allocate mine notes via the game's app-heap allocator, not CRT `malloc`

**Problem**: The game's `Notes` vector is allocator-aware — it routes allocations through the game's own application heap rather than the CRT heap. When the vector is destroyed at chart end, its destructor calls free on that specific heap. If we grew the vector's buffer via a different allocator (e.g., CRT `malloc` / the existing `game_malloc` signature), the free path walks the wrong heap's free list → heap mismatch → crash or silent corruption.

Ghidra confirms the layout: the Measures vector grow path (`FUN_180032B80`) calls `FUN_18023AF40(DAT_180460058, size, 0, ...)`. `FUN_18023AF40` is the single allocator entry point (its first argument is a heap-handle global pointer). `DAT_180460058` is the application-heap handle global. The Notes vector uses the same allocator routed through the same handle — both are compiled from the same `std::vector<T>` allocator-aware template.

**Decision**: Resolve two new signatures:

- `agcs_heap_malloc` → `FUN_18023AF40`. Signature: `fn(heap_handle: *const u8, size: usize, align: u32, ...) -> *mut u8`
- `app_heap_handle` → `DAT_180460058`. A `*const *const u8` — dereference once to get the `agcs::Heap*` the allocator expects.

A matching `agcs_heap_free` is needed to release the old buffer after a grow. Its signature is simpler: `fn(ptr: *mut u8) -> void` — AGCS free looks up the heap from the pointer's alloc header. (Exact function to be identified during implementation; candidates visible in the cleanup path at `FUN_180027678C` and the std::map teardown code in `Analyze`.)

All three resolve via AOB at startup. If any fail, the mod declines to register.

**Rationale**:
- Allocating with the same heap the vector's destructor will free from is the only correct approach. Anything else is a latent crash.
- `FUN_18023AF40` is called hundreds of times across the binary for every AGCS-allocator allocation — the function is core to the engine and unlikely to change across DDR World builds. Stable AOB target.
- Pre-sizing once (Decision 1) means we call the allocator exactly once per chart load, freeing the old buffer once. No per-entry `push_back` grow path — simpler failure model.

**Alternatives Considered**:
- **CRT `malloc` via existing `game_malloc` signature**: wrong heap. Would crash at chart end. Rejected.
- **Don't grow the vector — append mines into a separate parallel vector**: breaks Decision 1 (mines must be in `Results` for the renderer to pick them up). Rejected.
- **Allocate via `agcs_heap_malloc` but skip the free-old-buffer step (leak the pre-grow buffer)**: avoids the free-path signature but leaks `chart_count × old_notes_size` bytes per session. On short sessions it's bounded (~MB range), but accumulates over a multi-hour operator shift. Rejected — free signature is easy enough to resolve.

**Tradeoffs**: Two new signatures to maintain (allocator function, heap handle pointer) + one more for free. If any regress in a future DDR build, the mod won't register — graceful degradation. Free function in particular may be harder to AOB-identify because AGCS free is less distinctive than the malloc path; mitigation is to use the **game's own `std::vector` copy-reserve helper** if we can locate one, which encapsulates the malloc+memcpy+free sequence in a single call and sidesteps needing a standalone free signature.

### Decision 10: Pre-size the vector once — no per-entry grow path

**Problem**: Even with correct heap-matching, calling the allocator once per mine entry is wasteful and increases the window where a heap-mismatch bug could surface.

**Decision**: In the Analyze hook, perform exactly one bulk grow operation per chart load:
1. Read current `notes.size()` from `(end - begin) / 0x60`.
2. Parse MINE_DATA chunk header to get mine count `N`.
3. If `end + N * 0x60 > end_capacity`: allocate one new buffer of size `(current + N) * 0x60` bytes (with some headroom, matching the game's 1.5× growth factor), memcpy existing notes into it, append our mines in-place, update the vector's three pointers (`begin`, `end`, `end_capacity`) atomically, free the old buffer.
4. Else (unlikely — vector usually sized exactly to input): memcpy mines into `end`, bump `end` by `N * 0x60`.

All pointer writes happen after the new buffer is fully populated — if any step fails (allocation returns null), we abort without touching the vector's pointers. The original vector remains valid.

**Rationale**:
- Matches what `agcs::stl::vector::insert(end, N items)` would do internally. This isn't reinventing — it's open-coding a known operation.
- Single allocator call per chart → smaller blast radius for any allocator bug.
- Atomic pointer update means the game's destructor always sees a consistent vector state.

**Alternatives Considered**:
- **Per-mine `push_back` via a helper**: many allocator calls, many grow-path invocations, larger exposure to allocator mismatch. Rejected.
- **Call the game's own `std::vector::reserve` / `insert` helpers**: if we can locate stable signatures for them, this becomes a one-liner. Mentioned as an optional refinement during implementation — not gating.

**Tradeoffs**: The mod briefly holds the vector in a partially-mutated state (new buffer allocated but old pointers still live) during the memcpy+update sequence. No other thread reads the vector during Analyze (it's called on the game thread during chart load, before gameplay starts), so this is safe.

### Decision 11: Defaults for score penalty and gauge damage — deferred to implementation

**Problem**: Pick numeric defaults for `score_penalty_per_hit` and `gauge_damage_per_hit`. Requirements defer to PE.

**Decision**: Defaults are set at implementation time, derived empirically by measuring DDR World's shock-arrow penalty on a live cabinet (score delta + gauge delta on a confirmed shock-NG) and scaling by the StepMania/ITG convention ratio. README.md documents the chosen values with justification per US-8.

**Rationale**: Any value I pick here without a live measurement is speculation. The config knobs are user-overridable, so defaults can be tuned post-ship without code changes. This is an implementation-time empirical task, not an architectural decision.

**Tradeoffs**: Design isn't numeric-complete. Mitigated by the config surface — values are user-tunable from day one.

---

## Component Design

### New Components — Service Layer

| Component | Location | Responsibility |
|-----------|----------|----------------|
| `judge_hook` service | `src/services/judge_hook.rs` | Single `GenericDetour` on `judgeNotes`; priority-ordered pre/post callback dispatch; stable callback registration API |

### New Components — NoteTypesExpansion Mod

| Component | Location | Responsibility |
|-----------|----------|----------------|
| `NoteTypesExpansionMod` | `src/mods/note_types_expansion/mod.rs` | Mod trait impl; lifecycle; owns `NoteTypeRegistry`; registers hook callbacks on enable, unregisters on disable |
| `NoteType` (trait) | `src/mods/note_types_expansion/note_type.rs` | Trait contract for any new note type — parse, inject, render binding, judge, reset |
| `NoteTypeRegistry` | `src/mods/note_types_expansion/registry.rs` | Holds `Vec<Box<dyn NoteType>>`; dispatches to them from hook callbacks; enforces `kind` uniqueness across registered types |
| `MineNoteType` | `src/mods/note_types_expansion/mines.rs` | `NoteType` impl for mines; owns `Vec<MineEntry>` sidecar |
| `MineEntry` (struct) | `src/mods/note_types_expansion/mines.rs` | `{ music_count: i32, panel: u8, consumed: bool }` |
| SSQ chunk walker | `src/mods/note_types_expansion/ssq_chunk.rs` | Pure function: given SSQ blob + target chunk kind, return iterator over chunk body byte slices |
| Tempo converter | `src/mods/note_types_expansion/timing.rs` | Pure function: `beatCount → musicCount` via the chart's tempo chunk (linear interp, integer math) |
| Hook callbacks | `src/mods/note_types_expansion/hooks.rs` | `extern "C"` static functions registered with `judge_hook` service + `retour::GenericDetour` on `IStepReader::Analyze` and `collect_render_notes` |
| Config | `src/mods/note_types_expansion/config.rs` | `NoteTypesExpansionConfig` struct with `serde::Deserialize` defaults |

### New Signatures (`src/core/signatures.rs`)

| Signature name | Resolution method | Purpose |
|---------------|-------------------|---------|
| `step_reader_analyze` | AOB on `FUN_1801C6D80` prologue (distinctive prologue with specific stack layout + vtable[1] call pattern) | Hook point for mine injection after vanilla parse builds `Notes` |
| `collect_render_notes` | AOB on `FUN_1800240C0` prologue | Hook point for render kind-filter widening and texture rebinding |
| `agcs_heap_malloc` | AOB on `FUN_18023AF40` prologue | AGCS heap allocator — `fn(heap_handle, size, align, ...) → *mut u8`. Used for app-heap-matched allocation when growing the Notes vector (Decision 9). |
| `app_heap_handle` | Derive from `DAT_180460058` — the heap handle pointer seen at `FUN_180032B80` (vector<Measure> grow path). RIP-relative LEA from the grow function's call site. | The `agcs::ApplicationHeap*` used by `agcs::stl::vector<Note>`. |
| `agcs_heap_free` | AOB on the STL cleanup path function (candidate `FUN_180027678C`). Or: if we can locate a game-side `std::vector::reserve` equivalent that encapsulates malloc+memcpy+free, prefer that and skip this signature. | Free the pre-grow Notes-vector buffer after a grow (Decision 9). |
| `get_offset_y` | AOB on `FUN_180023F80` prologue | Scroll Y computation — not strictly needed for mines but exposed for future note types; optional |

Existing signatures depended on (already resolved):
- `judge_notes` (used by `autoplay` today, becomes shared via `judge_hook` service)
- `auto_foot_panel_update` (used by `autoplay`, unchanged)
- `game_malloc` — **NOT used for Notes vector growth** (see Decision 10). Reserved for any future allocation the mod needs that genuinely targets CRT heap (none currently planned).

### Modified Existing Components

| Component | Change | Reason |
|-----------|--------|--------|
| `src/core/signatures.rs` | Add 2–3 new `SignatureDefinition` entries; any derive helpers needed | New hook points |
| `src/services/mod.rs` | `pub mod judge_hook;` | Expose new service |
| `src/lib.rs` | Call `judge_hook::init(&signatures)` in init sequence; register `NoteTypesExpansionMod` in the mod registry | Standard wiring |
| `src/mods/autoplay.rs` | Replace direct `GenericDetour` with two callbacks registered through `judge_hook::register_pre` and `register_post` | Required for coexistence with NoteTypesExpansion (Decision 6) |
| `src/mods/mod.rs` | `pub mod note_types_expansion;` | Expose new module |
| `src/mods/config.rs` | Add `pub note_types_expansion: Option<NoteTypesExpansionConfig>` to `ConfigFile` | Surface new config section |
| `mod-config.json` | Add `"note-types-expansion": true` to `mods` section; add `"note_types_expansion"` top-level section | Ship default enabled + default values |
| `README.md` | New section describing the mod, config knobs, and link to the MINE_DATA chunk spec | US-8 doc requirement + US-10 handoff pointer |

### New Documentation

| Document | Location | Purpose |
|----------|----------|---------|
| MINE_DATA chunk spec | `docs/ssq_mine_chunk_spec.md` | Self-contained authoring spec for `ddr-chart-tools` handoff (US-10) |

### Component Interactions

**Chart load** (once per song):

```
SSQ blob on disk
      ↓
SsqReader::SsqReader(data, size)    [FUN_1801CA230, vanilla]
      ↓
IStepReader::Analyze(&notes, &measures, &result, ...)    [FUN_1801C6D80, vanilla]
  ├── vtable[1] → SsqReader::analyze → walks FOOTSTEP, builds Notes vector
  ├── freeze post-processing
  ├── shock counting
  └── [HOOK — post-call] NoteTypesExpansionMod::on_analyze_complete
        ├── locate chart's tempo chunk (for beatCount→musicCount conversion)
        ├── for each registered NoteType:
        │     ├── parse its SSQ chunk(s) from the blob via ssq_chunk walker
        │     ├── convert chunk entries → step::Note records
        │     ├── append notes to game's Notes vector (via game_malloc on grow)
        │     └── populate NoteType's sidecar
        └── return to game — Analyze finalizes (options, groove radar, Results built)
```

**Each frame during gameplay** (state 4 of `GamePlayActor::update`):

```
1. footPanel->update(foot_panel, &note_list, note_count, music_count)    [vtable[1]]
     (AutoFootPanel reads Results and generates auto-inputs, skipping entries
      whose entry+0xC != 0xFF — so mines marked in step 3a are auto-avoided)

2. FUN_18005F050(actor)    [vanilla pre-judge setup]

3. GamePlayActor::judgeNotes(actor, music_count)    [FUN_18005F270, shared hook]
     │
     ├── [HOOK — pre-dispatch] judge_hook runs pre-callbacks by priority:
     │     ├── Priority::Early  → NoteTypesExpansion.pre_judge
     │     │     └── scan Results; for each whose note.kind == MINE, write entry+0xC = 0xFE
     │     └── Priority::Late   → Autoplay.pre_judge
     │           ├── save original foot_panel pointer from actor+0x278
     │           ├── write AutoFootPanel pointer into actor+0x278
     │           └── call AutoFootPanel::update (reads Results, skips marked mines)
     │
     ├── [ORIGINAL judgeNotes runs]
     │     └── iterates Results [actor+0xB0, actor+0xB8); processes only entries
     │         with entry+0xC == 0xFF (unjudged) — mines are skipped; regular
     │         arrows and shocks judge normally
     │
     └── [HOOK — post-dispatch] judge_hook runs post-callbacks by priority:
           ├── Priority::Early  → Autoplay.post_judge
           │     └── restore original foot_panel pointer to actor+0x278
           └── Priority::Late   → NoteTypesExpansion.post_judge
                 ├── scan Results; restore entry+0xC = 0xFF on mine-kind entries
                 └── for each registered NoteType:
                       └── on_judge_tick(actor, music_count, foot_panel)
                             └── (mines) binary-search sidecar for entries in
                                 [music_count - window, music_count + window];
                                 for each pending (not consumed), check pressed
                                 panels via foot_panel->vtable[2] (wasJustPressed);
                                 on hit: submit judgment via FUN_180060330 with
                                 mine-NG code, apply score/gauge penalty, mark consumed

4. ArrowRenderer::onDraw    [FUN_180025FA0, vanilla]
     └── FUN_180026050 (outer render)
           └── FUN_1800240C0 (collect_render_notes)    [HOOK]
                 └── for each Result entry:
                       ├── vanilla: filter by *note.kind == 0 (ARROW)
                       ├── [HOOK — widen] also accept *note.kind == MINE
                       │   and look up texture binding from NoteTypeRegistry;
                       │   stash binding for the quad-emit phase
                       └── rest of collect_render_notes runs unchanged:
                           scroll Y computed via getOffsetY, appearance alpha applied,
                           quad pushed to sprite pool
           └── sprite pool flushed to ScreenCommandList by vanilla code;
               our stashed binding causes mine quads to bind the mine texture
               instead of the arrow atlas
```

**Chart end** (scene exits gameplay):

```
Game destroys Notes / Results vectors (frees memory it owns)
      ↓
[HOOK — scene manager callback] NoteTypesExpansionMod::on_chart_end
      └── NoteTypeRegistry.reset_all() → each NoteType clears its sidecar
```

---

## Integration Points

**Existing services consumed**:
- `core::memory` — `write_ptr`, `write_u32`, etc. for the entry+0xC sentinel writes
- `core::signatures::SignatureStore` — resolves `step_reader_analyze`, `collect_render_notes`, plus reuses `judge_notes`, `game_malloc`, `auto_foot_panel_update`
- `core::hooks::HookManager` — registers the retour detours for `Analyze` and `collect_render_notes`
- `core::scanner` — shared AOB / RIP-relative decode helpers for any new signature resolution
- `services::judge_hook` (new) — pre/post callback registration for both Autoplay and NoteTypesExpansion
- `services::scene_manager` — scene-change callback for chart-end cleanup
- `services::avs_layeredfs` — serves the mine texture file from `data_mods/note_types_expansion/` (no runtime API — purely file-replacement)
- `mods::config` — reads `"note_types_expansion"` section from `mod-config.json`

**External services**: None. The mod is self-contained within the DLL.

**Data Storage**: None persistent. All mod state is in-process, rebuilt per chart.

**Configuration** (new):

```
mod-config.json → "note_types_expansion": {
  "mines": {
    "score_penalty_per_hit": <int, default TBD at implementation>,
    "gauge_damage_per_hit":  <int, default TBD at implementation>
  }
}
```

Invalid or negative values fall back to defaults with a warning log (per US-8).

---

## Public Contracts (Signatures Only — No Implementations)

### `services::judge_hook`

```rust
// src/services/judge_hook.rs

/// Ordering bucket for pre/post callbacks. Within the same priority, order
/// is registration order — do NOT rely on it; use a distinct priority if
/// ordering matters.
pub enum Priority {
    Early,
    Normal,
    Late,
}

/// Callback signature for both pre-judge and post-judge callbacks.
/// `actor` is the GamePlayActor pointer; `music_count` is the current playhead.
pub type JudgeCallback = fn(actor: *mut u8, music_count: i32);

/// Initialize the service — installs the retour detour on judgeNotes.
/// Must be called once during lib.rs init sequence, after signatures resolve.
pub fn init(signatures: &SignatureStore) -> bool;

/// Register a callback to run BEFORE the original judgeNotes. Returns true
/// if registered; false if the service wasn't initialized.
pub fn register_pre(priority: Priority, callback: JudgeCallback) -> bool;

/// Register a callback to run AFTER the original judgeNotes.
pub fn register_post(priority: Priority, callback: JudgeCallback) -> bool;

/// Availability check for dependents.
pub fn is_available() -> bool;
```

### `NoteType` trait

```rust
// src/mods/note_types_expansion/note_type.rs

/// Sentinel `kind` values start at 20 and go up — outside the vanilla
/// Note::kind range (0, 1, 2, negative control values).
pub const MINE_KIND: i8 = 20;
// pub const LIFT_KIND: i8 = 21;   (future)
// pub const ROLL_KIND: i8 = 22;   (future)

/// Implemented by each note-type. Each impl owns its SSQ chunk format,
/// sidecar, and per-hook logic.
pub trait NoteType: Send {
    /// Unique identifier (e.g., "mines"). Used in config and logs.
    fn id(&self) -> &'static str;

    /// The step::Note::kind value this type uses. Must be unique across
    /// registered types; registry enforces this at register() time.
    fn note_kind(&self) -> i8;

    /// Parse this type's SSQ chunk(s), append Notes to the game's vector,
    /// populate internal sidecar. Called from Analyze hook with tempo
    /// converter already set up for the chart. Returns count of notes injected.
    fn on_chart_loaded(
        &mut self,
        ssq_blob: &[u8],
        tempo: &TempoConverter,
        notes_vec: &mut GameNotesVec,
    ) -> Result<usize, NoteTypeError>;

    /// Called once per frame from post_judge. Check sidecar against
    /// music_count + foot_panel state, apply type-specific judgments.
    fn on_judge_tick(
        &mut self,
        actor: *mut u8,
        music_count: i32,
        foot_panel: *mut u8,
    );

    /// Render binding for this type's kind. Render hook calls this once
    /// per frame (cached) and uses it when it encounters a matching kind.
    fn render_binding(&self) -> RenderBinding;

    /// Clear all per-chart state. Called at chart end.
    fn reset(&mut self);
}

/// Render-time sprite binding.
pub struct RenderBinding {
    pub texture_name: &'static str,   // e.g., "note_types_mine00"
    pub uv: [f32; 4],                 // left, top, right, bottom
}

/// Opaque helper passed to on_chart_loaded. Wraps the chart's tempo chunk
/// data for integer-accurate beatCount → musicCount conversion.
pub struct TempoConverter { /* opaque */ }
impl TempoConverter {
    pub fn beat_to_music_count(&self, beat_count: i32) -> i32;
}

/// Safe wrapper around the game's agcs::stl::vector<Note>. Uses the app-heap
/// allocator (Decision 9) to match the vector's allocator — critical for
/// heap safety when the vector is destroyed at chart end.
///
/// The constructor takes the raw vector pointer (from IStepReader::Analyze's
/// second argument) and the resolved app_heap_handle. append_bulk reserves
/// once for the full mine count and appends all entries in one pass — no
/// per-entry grow (Decision 10).
pub struct GameNotesVec { /* opaque */ }
impl GameNotesVec {
    pub fn new(vec_ptr: *mut u8, app_heap: *const u8) -> Self;

    /// Reserve (once) for `additional` more entries, then append them.
    /// Allocates a new buffer via agcs_heap_malloc, memcpys existing entries,
    /// appends new entries, updates vector pointers atomically, frees old buffer.
    /// Returns Err if allocation fails — vector left unmodified.
    pub fn append_bulk(&mut self, notes: &[GameNote]) -> Result<(), NotesVecError>;
}

/// Raw layout of step::Note — confirmed by Ghidra observations of the Analyze
/// post-processing loops (iteration stride 0x60, per-panel dword arrays at
/// +0x1C and +0x3C). 0x60 byte stride.
#[repr(C)]
pub struct GameNote {
    pub kind: i8,          // +0x00
    _pad1: [u8; 3],
    pub beat_count: i32,   // +0x04
    pub music_count: i32,  // +0x08
    _pad2: [u8; 0x10],
    pub state: [i32; 8],   // +0x1C
    pub length: [i32; 8],  // +0x3C
    _pad3: [u8; 0x04],     // to reach 0x60
}
// Total size: 0x60 bytes (verified via Ghidra on FUN_1801C6D80 iteration stride)

pub enum NoteTypeError { /* parse errors, alloc failures, etc. */ }
pub enum NotesVecError { AllocFailed, VectorPointerInvalid, HeapHandleInvalid }
```

### `NoteTypeRegistry`

```rust
// src/mods/note_types_expansion/registry.rs

pub struct NoteTypeRegistry { /* Vec<Box<dyn NoteType>> */ }

impl NoteTypeRegistry {
    pub fn new() -> Self;

    /// Register a NoteType impl. Panics if note_kind() collides with an
    /// already-registered type — caught in dev, never reaches prod.
    pub fn register(&mut self, nt: Box<dyn NoteType>);

    /// Dispatch on_chart_loaded to all registered types.
    pub fn on_chart_loaded(
        &mut self,
        ssq_blob: &[u8],
        tempo: &TempoConverter,
        notes_vec: &mut GameNotesVec,
    );

    /// Dispatch on_judge_tick to all registered types.
    pub fn on_judge_tick(&mut self, actor: *mut u8, music_count: i32, foot_panel: *mut u8);

    /// Look up render binding by Note::kind. Returns None if no registered
    /// type handles this kind.
    pub fn binding_for_kind(&self, kind: i8) -> Option<&RenderBinding>;

    /// Return true if any registered type uses this kind value.
    pub fn handles_kind(&self, kind: i8) -> bool;

    /// Reset all registered types.
    pub fn reset_all(&mut self);
}
```

### Autoplay refactor

```rust
// src/mods/autoplay.rs — new shape (behavior preserved)

impl Mod for AutoplayMod {
    fn required_signatures(&self) -> &[&str] {
        // judge_notes is now consumed via judge_hook service instead of directly,
        // but we still declare it so mod registration fails gracefully if unresolved.
        &["judge_notes", "auto_foot_panel_vtable", "auto_foot_panel_update"]
    }

    fn enable(&mut self) {
        // Register pre/post callbacks with the shared judge_hook service
        // instead of installing a GenericDetour directly.
        judge_hook::register_pre(Priority::Late, autoplay_pre_judge);
        judge_hook::register_post(Priority::Early, autoplay_post_judge);
    }

    fn disable(&mut self) {
        // (future: judge_hook exposes unregister; for v1 disable just stops the callback from acting via a flag)
    }
}

// The callbacks contain the same logic that currently lives inside
// autoplay::judge_notes_hook — split into entry and exit halves.
```

The disable path needs care: `judge_hook` v1 may need to expose an `unregister` API, or callbacks check a mod-local `enabled` atomic flag to no-op when disabled. Implementation detail — the mod-trait `disable()` contract is preserved either way.

---

## Changes to Existing Code

### `src/core/signatures.rs`
- **Change**: Add `SignatureDefinition` entries for `step_reader_analyze` and `collect_render_notes`
- **Reason**: Hook points required by the new mod
- **Impact**: Additive — existing signatures and callers unchanged

### `src/services/mod.rs`
- **Change**: `pub mod judge_hook;`
- **Reason**: Expose the new service
- **Impact**: None

### `src/services/judge_hook.rs` (new file)
- **Change**: New service — see Public Contracts section
- **Reason**: Decision 6 — prevent retour-stacking breakage
- **Impact**: Gives any future mod a safe way to hook `judgeNotes`

### `src/mods/autoplay.rs`
- **Change**: Replace direct `GenericDetour<JudgeNotesFn>` install with two `judge_hook::register_{pre,post}` calls; move existing hook body into two callback functions
- **Reason**: Decision 6 — coexistence with NoteTypesExpansion
- **Impact**: Behavior-preserving. Callers of `AutoplayMod` (only `ModRegistry::register`) unaffected. `required_signatures()` list unchanged so graceful-degradation behavior unchanged.

### `src/mods/config.rs`
- **Change**: Add `pub note_types_expansion: Option<NoteTypesExpansionConfig>` field to `ConfigFile`
- **Reason**: Surface new config section through the centralized store
- **Impact**: Additive — `#[serde(default)]` already applied pattern, missing sections still parse cleanly

### `src/mods/mod.rs`
- **Change**: `pub mod note_types_expansion;`
- **Reason**: Expose new module
- **Impact**: None

### `src/lib.rs`
- **Change**:
  - Add `judge_hook::init(&signatures)` call in the init sequence (after signature scan, before mod registration)
  - Add `reg.register(Box::new(mods::note_types_expansion::NoteTypesExpansionMod::new()), &ctx);` in the mod registration block
- **Reason**: Standard new-service + new-mod wiring
- **Impact**: Additive

### `mod-config.json`
- **Change**: Add `"note-types-expansion": true` to `mods`; add `"note_types_expansion": { "mines": { "score_penalty_per_hit": <N>, "gauge_damage_per_hit": <N> } }` top-level
- **Reason**: Enable the new mod by default; ship default config values
- **Impact**: None on existing keys

### `README.md`
- **Change**: New section describing the mod (purpose, enable/disable, config knobs with defaults and justification per US-8), with a link to `docs/ssq_mine_chunk_spec.md`
- **Reason**: US-8 documentation; US-10 handoff pointer
- **Impact**: Additive

---

## New Documentation Files

### `docs/ssq_mine_chunk_spec.md`

Self-contained SSQ MINE_DATA chunk format spec for the `ddr-chart-tools` handoff (Requirements US-10). Outline:

1. **Chunk kind value**: 20 (0x14). Rationale with reference to `docs/ssq_format.md §1` chunk-kind observations across 1523 files.
2. **Chunk header**: `type=0x14`, `param2=0` (reserved), `param3=N` (entry count), `param4=0`.
3. **Body layout**: N fixed 8-byte entries, little-endian: `(i32 beat_count, u8 panels, u8 flags, u16 reserved)`. Size invariant: `chunk_length = 12 + 8*N`.
4. **Panel bitmask**: references `docs/ssq_format.md §5.3` — same bit layout as step bytes.
5. **Tick space**: same 4096-per-whole-note as FOOTSTEP. Converted to `musicCount` at load time via tempo chunk.
6. **Sorting**: entries must be sorted ascending by `beat_count`. Duplicate beat_counts permitted (multiple mines at same tick on different panels — use separate entries, each with one bit in `panels`).
7. **Co-existence rules**: mines and arrows at the same tick on different panels OK; same tick + same panel → arrow takes priority per US-3 AC.
8. **Forward-compat**: vanilla DDR's chunk walker skips `kind=20` cleanly (verified behavior); modded DDR with mod disabled also ignores.
9. **StepMania mapping**: `M` note in `#NOTES` block → one MINE_DATA entry with `panels = 1 << panel_index`.
10. **Worked example**: small mine-enabled SSQ with annotated hex bytes.
11. **Validation rules**: tick ordering, `panels != 0`, `flags = 0`, `reserved = 0`.

---

## Deployment Sequence

Implementation order (relevant for task breakdown):

1. **Shared `judge_hook` service** + **autoplay refactor**. Ship together and validate autoplay still works (attract-mode demo exhibits correct autoplay behavior). No NoteTypesExpansion code yet. This establishes the foundation without the feature's risk on top.
2. **NoteTypesExpansion scaffolding**: Mod trait impl, `NoteType` trait, `NoteTypeRegistry`, config surface, hook registrations (no-op bodies). Verify the mod registers cleanly, appears in the mod menu, enables/disables without errors.
3. **SSQ chunk parsing + `Analyze` hook**: parse MINE_DATA chunk, log what was parsed. No injection yet. Verify against a hand-authored test SSQ.
4. **Note injection + sidecar population**: append mine Notes to the game's vector, build sidecar. No rendering or judgment yet. Verify via logs that the counts match and the Notes vector didn't crash on chart end.
5. **Render hook + texture binding**: widen kind filter, rebind texture. Mines become visible (may look placeholder until dedicated sprite is authored).
6. **Pre/post-judge sentinel + mine-hit detection**: mark Results as judged, detect hits in post-pass, apply penalties.
7. **Autoplay verification with mines**: enable Autoplay + NoteTypesExpansion simultaneously, verify autoplay avoids mine panels and doesn't rack up penalties.
8. **Dedicated mine sprite**: author PNG, serve via LayeredFS, verify visual distinctness.
9. **Empirical defaults tuning**: measure DDR shock-arrow penalty, pick score/gauge defaults, document in README.md.
10. **Vanilla-compat verification**: load mine-enabled SSQ on unmodded DDR World (mod DLL removed or disabled), confirm clean play (US-11).
11. **MINE_DATA chunk spec doc**: author `docs/ssq_mine_chunk_spec.md` for `ddr-chart-tools` handoff (US-10). Can be authored in parallel with step 3, does not block.

**Test-chart authoring**: For early phases (before `ddr-chart-tools` mine support lands), hand-build minimal mine-enabled SSQs with a hex editor. A single 2-minute song with a handful of mines on known panels is enough to validate all hook points.

**Rollback**: set `"note-types-expansion": false` in `mod-config.json` and restart. Game reverts to vanilla behavior with zero residual impact. No on-disk file needs to be touched. If the shared `judge_hook` service itself has a regression, the fallback is revert the DLL commit — autoplay would revert with it.

---

## Risks and Mitigations

| Risk | Impact | Likelihood | Mitigation |
|------|--------|------------|------------|
| Pre-judge sentinel hook fails to run; mines leak to vanilla judge loop | High (mines judged as 1-panel arrows; scoring broken) | Low | `judge_hook` service + `NoteTypesExpansion` both gate on `judge_notes` signature resolution. Mod won't register if the signature is missing. Judge signature is shared with Autoplay — regression surfaces loudly across multiple mods at once. |
| Autoplay refactor regresses autoplay behavior | Medium | Low-Medium | Refactor is behavior-preserving (same logic, different call shape). Validation gate: deployment step 1 requires attract-mode autoplay to still work before any mine code is added. Attract mode runs continuously during testing → fast feedback. |
| Two mods register conflicting priorities or the priority ordering doesn't produce the intended sequence | Medium | Low | Priorities are documented per-callback (Early/Normal/Late) with the exact intended frame ordering in this design. Integration test: dry-run the callback sequence on a mine-enabled chart with both mods enabled and verify log output matches the documented order. |
| `agcs_heap_malloc` signature fails to resolve, or `app_heap_handle` points to the wrong global | High (mod can't inject mines; or worse, silent heap mismatch → crash at chart end) | Low-Medium | Mod gates on both signatures resolving at register time. `app_heap_handle` is additionally validated at first use: dereference must yield a non-null pointer whose first few qwords look like an `agcs::Heap` vtable (sanity check). Failure → refuse to enable, log warning. Mines invisible rather than crashy. |
| Heap mismatch (allocate via CRT malloc, free via AGCS heap, or vice versa) | Critical (crash at chart end when the game's destructor runs) | Very low (given Decision 9 explicitly uses AGCS allocator) | Decision 9 names the exact allocator + heap. `GameNotesVec::append_bulk` is the only path that touches the Notes vector's buffer. Unit-testable in isolation against a mocked vector layout before deploying. If `agcs_heap_free` signature is fragile, alternate path (Decision 9's tradeoffs section) is to use a game-side `std::vector::reserve` helper that encapsulates the sequence. |
| Bulk grow partially fails mid-memcpy | Low | Very low | Sequence is: allocate new → memcpy existing → memcpy mines → swap pointers → free old. No pointer state is committed until the new buffer is fully populated. If allocation fails, `append_bulk` returns Err and the vector is untouched. Memcpy can't fail. |
| Mine render hook widens the kind filter but texture rebinding doesn't take effect | Medium (mines render as regular arrows) | Medium | Render hook integration is the most intricate part. Mitigation: deployment step 5 ships rendering as a distinct phase with its own validation (visible mines in-game before wiring judgment in). If texture rebinding proves fragile, fallback is to ship with the arrow atlas texture and iterate — mines would be visible (just as regular arrows) while players learn where they are. Non-blocking for judgment correctness. |
| `beatCount → musicCount` conversion drifts from the game's own math | Medium (mines judge at slightly off times) | Low-Medium | Use integer linear interpolation across tempo entries, exact same logic as vanilla `CalcMusicCount`. Validation: after chart load, for each mine's musicCount, cross-check against what the game computes for a regular note at the same beatCount (pick a regular note near each mine and log its musicCount from `Results`). Should be within ±1 tick. Deferred-to-impl: unit-testable in isolation with a canned tempo chunk. |
| Konami adds an official chunk with `kind=20` in a future DDR update | Medium (our charts misinterpreted by vanilla) | Very low | `param2` is reserved in our layout and can become a subtype discriminator. Or migrate to a new kind. Chunk-kind set has been stable for 10+ years. |
| Autoplay hook registration fails but mod registration succeeds | Medium (silent autoplay regression) | Low | `judge_hook::register_pre/post` returns `bool`. Autoplay checks the return value, refuses to enable with a warning log on false. Mod stays registered but inactive — standard `ModRegistry` failure-mode semantics. |
| Mine texture missing from LayeredFS at runtime | Low (mines render as default arrow) | Low-Medium | Render hook falls back to default arrow atlas binding if texture_resolver can't find the mine texture. Warning logged. Degraded but non-crashing. |
| Two NoteType impls collide on `note_kind()` value (future concern) | Low (v1 has one type) | Very low | `NoteTypeRegistry::register` asserts kind uniqueness at register time; duplicates panic registration (loud failure, caught in dev). |
| `ddr-chart-tools` implements the chunk spec incorrectly, produces malformed chunks | Medium (mine charts don't load right) | Medium | Chunk parser validates: `chunk_length == 12 + 8*N`, entries sorted by `beat_count`, `panels != 0`, `flags == 0`, `reserved == 0`. Malformed chunks log a warning and are treated as zero mines — chart still plays normally. US-10 spec doc is the cross-team handoff contract. |
| Per-chart state not cleared on scene change; mine sidecar leaks across songs | Medium (wrong mines appear on next song) | Low | Scene manager callback on transition out of gameplay triggers `NoteTypeRegistry::reset_all()`. Already used pattern elsewhere in the codebase. |

---

## Open Questions

1. **`judge_hook` unregister semantics on mod disable.** If Autoplay (or NoteTypesExpansion) is disabled via the mod menu at runtime, how do we stop its callbacks from firing? Two options: (a) service exposes `unregister(handle)` with callback handles returned from `register_pre/post`, (b) callbacks check an atomic flag owned by the mod and no-op when disabled. Option (b) is simpler and matches the existing hook-removal pattern (`HookManager::remove_all` at disable) in spirit. Decision deferred to implementation. No impact on external behavior either way.

2. **Default values for `score_penalty_per_hit` and `gauge_damage_per_hit`.** Deferred per Decision 9 — requires empirical measurement of DDR World's shock-arrow penalty on a live cabinet. Implementation task with README.md as the deliverable.

3. **Exact texture name(s) and path(s) under `data_mods/note_types_expansion/`.** Depends on which name the render hook uses for the rebind lookup, which is resolved during Render-hook implementation (deployment step 5). Artist-facing surface, not architectural.

4. **Should the mine chunk's `param2` carry a format version for future evolution, or remain `0`?** Recommendation: remain `0` for v1, document as reserved-for-subtype in the spec. Revisit only if a second mine-format revision becomes necessary.

5. **Should empty mines (`panels == 0`) be a warning or a hard parse error?** Recommendation: warning + skip the entry. Tolerant parsing keeps charts with minor authoring-tool bugs playable. Documented in the spec.

6. **Should gameplay log a summary of mines parsed per chart (count, panels)?** US-12 asks for informative logs. Recommendation: yes, log at INFO on chart load: "MineNoteType: parsed N mines from MINE_DATA chunk in {chart_identity}". Per-hit logs at DEBUG to avoid spam.

7. **`agcs_heap_free` signature vs. game-side `std::vector::reserve` helper.** Decision 10 calls for a free signature to release the pre-grow buffer. An alternative is to locate the game's own `std::vector<Note>::reserve`-equivalent helper, which encapsulates malloc+memcpy+free in one call and sidesteps needing a standalone free signature. Search during implementation: xref analysis on `agcs_heap_malloc` usages within the STL cleanup paths may surface a stable reserve helper. If found, prefer it and remove `agcs_heap_free` from the signature list. If not, resolve `agcs_heap_free` directly. Either way, no architectural impact.
