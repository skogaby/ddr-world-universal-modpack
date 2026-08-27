# Tasks: Custom Options Support (20260506-custom-options-support)

Tasks are sized to be commit-ready: one shippable unit that builds independently, roughly ~1 day of focused work each. The project has no unit-test harness (per steering docs) — "tests" means `cargo check --target x86_64-pc-windows-msvc` passes AND a deploy-and-observe validation via log output and visual in-game inspection where applicable.

## Workspace Info

**Primary crate**: `ddr_world_hook` (Rust cdylib, `x86_64-pc-windows-msvc` target)
**Sequencing philosophy**:
- Tasks 1–5 land the framework scaffolding, registration API, row allocation, builder detour, and the Page6 Mods tab — the substrate every later task builds on.
- Task 6 migrates the Autoplay POC as the first user-visible mod on the new framework. In-session toggling only — persistence lands in Task 8.
- Task 7 adds the scroll driver so Mods tabs with more rows than viewport capacity stay navigable (validated against autoplay + overflow dummies).
- Task 8 is the polish task: wire persistence (ess.dll save/load + `custom_options.persist` config key) so autoplay survives card-swipe cycles, closing US-4.
- Tasks 9–10 add scalar-row support as an incremental enhancement, explicitly marked optional per Decision 11 of the design. Scalar RE (Task 9) is a research-only task with no code output.
- Every task leaves the project in a buildable state (`cargo check` passes, shipped binary runs in-game without regressing existing mods).

**Shared validation protocol** (referenced by each task):
- **Local**: `cargo check --target x86_64-pc-windows-msvc` must succeed.
- **Deploy**: `./scripts/deploy.sh` builds + SCPs the DLL to the test cabinet.
- **In-game observation**: spice2x logs contain `[DDR-Hook]` lines; use DebugView or spice2x's log file. Visual inspection of options menu on cabinet confirms layout/behavior.

---

## Task 1: Add signatures + scaffold empty service modules + init wiring

**Module(s)**: `src/core/signatures.rs`, `src/services/custom_options/mod.rs` (new), `src/services/custom_options_persistence.rs` (new), `src/services/options_scroll.rs` (new), `src/services/mod.rs`, `src/lib.rs`

**Goal**: Lay the architectural skeleton so later tasks can fill it in without re-plumbing. Every downstream task compiles from day one because the module paths, init calls, and signature entries already exist as stubs. No behavior change in-game — all three new services are `is_available() == false` at this point.

**Scope**:
- Add signature entries (with AOB patterns) to `core::signatures::SIGNATURES` for: `row_builder_fn` (for `FUN_180163970`), `tab_filter_fn` (for `FUN_180168d10`), `option_element_arrowcolor_ctor` (the donor ctor `FUN_180173810`), `option_tab_vtable` (for row allocation vtable writes), `option_form_tab_count_store` (for the `5→6` immediate patch site; exact AOB TBD via CE in Task 5 — ship as a placeholder that resolves to None on current builds, which is fine for the scaffolding task).
- Create `src/services/custom_options/mod.rs` with a minimal `is_available()`, `init()` (logs "custom_options: init" and returns `true`), no other public API yet. Empty submodule files (`api.rs`, `registry.rs`, etc.) are NOT created in this task — they land in Task 2.
- Create `src/services/custom_options_persistence.rs` with the same skeleton.
- Create `src/services/options_scroll.rs` with the same skeleton.
- Register all three in `src/services/mod.rs`.
- Wire `init()` calls into `src/lib.rs` in the documented order: `afp_patcher` → `custom_options` → `options_scroll` → `custom_options_persistence`.
- No hooks installed, no state maintained. Three services simply log their init and return.

**Tests**:
- `cargo check` passes.
- Deploy + launch game: spice2x log shows the three new init lines; no existing mods regress; game behaves identically to pre-feature.

**Dependencies**: None.

**Out of scope**: Any actual behavior (registration API, detours, state). Signatures for scalar-row work (deferred to Task 9).

- [x] 1.1 Add all new signature entries to `core::signatures::SIGNATURES`; AOBs for enum-kind ctor, row builder, tab filter, OptionTab vtable are documented in `docs/custom_player_options_research.md`. For `option_form_tab_count_store`, add a placeholder signature that resolves to None on current builds; resolved in Task 5.
- [x] 1.2 Create `src/services/custom_options/mod.rs` with `init() -> bool` and `is_available() -> bool`.
- [x] 1.3 Create `src/services/custom_options_persistence.rs` with the same skeleton.
- [x] 1.4 Create `src/services/options_scroll.rs` with the same skeleton.
- [x] 1.5 Update `src/services/mod.rs` to export the three new modules.
- [x] 1.6 Wire `init()` calls into `src/lib.rs`'s startup sequence in the correct order.
- [x] 1.7 Validate: `cargo check` passes; deploy + launch; confirm the three init log lines appear; confirm no existing mod regresses.

### Acceptance

**Status**: Approved 2026-05-06
**Notes**: Implementation includes RTTI+LEA+ctor/dtor-disambig derivation for ArrowColor ctor (20 enum ctors share the prologue shape; pure AOB rejected). Quality gates verified: cargo fmt --check, cargo clippy -D warnings, cargo check all clean.

---

## Task 2: Build the `custom_options` registry, API types, and per-player value cache

**Module(s)**: `src/services/custom_options/api.rs` (new), `src/services/custom_options/registry.rs` (new), `src/services/custom_options/mod.rs` (extend)

**Goal**: The framework's public API lands. Mods can call `register_option(...)` and get back a handle (or Err). `get_value(side, id)` returns the current resolved value. Change callbacks are wired but fire from a test path only — no UI or persistence yet. Scalar registrations return `Err(ScalarUnsupported)` per Decision 11.

**Scope**:
- `api.rs`: All the public types from design's "Public Contracts" section — `OptionHandle` (opaque), `UiKind`, `EnumValue`, `ScalarFormat`, `PageTag`, `ShowWhen`, `OnChangeFn`, `RegisterSpec`, `RegisterError`. Plus a `RegisterSpec::bool_toggle()` convenience builder that expands to `UiKind::Enum` with two hardcoded `EnumValue`s pointing at Konami's stock **`seop_op_off`** and **`seop_op_on`** sprites — so mods registering a simple on/off toggle supply only the row label texture (`seop_item_<name>.png`), not value-side PNGs. No alias injection required: `seop_op_*` is the native shared-value-ribbon convention Konami ships in the atlas (132×24 pixels, read at runtime by the `option_item` AFP template's value-text field). Other reusable stock value sprites include `seop_op_normal`/`seop_op_near`/`seop_op_far`/`seop_op_center`/`seop_op_mirror`/`seop_op_low`/`seop_op_medium`/`seop_op_high`/`seop_op_dark`/`seop_op_darker`/`seop_op_darkest`/etc. — mods can point at any of them by name when their enum values map to stock semantics. For bespoke enum values, mods ship their own `seop_op_<name>.png`.
- `registry.rs`: Single mutex-guarded struct holding the option list + per-option per-player value cache (`HashMap<String, [i32; 2]>`). Implement: duplicate-id rejection (logs ERROR, returns `RegisterError::Duplicate`), scalar-variant rejection (logs WARN, returns `RegisterError::ScalarUnsupported`), value-cache initialization with `default_value` at registration time.
- `mod.rs` (extend): Public `register_option`, `get_value`, and `resolve_from_load` (crate-internal, for Task 6). Change-callback invocation: wrap in `std::panic::catch_unwind` per the design's Risk #9 mitigation.
- Duplicate-id detection, scalar rejection, panic-catching are all exercised by a one-time smoke test path that runs at service init (registers two test options, verifies the second duplicate fails, deregisters before returning — this is temporary validation; removed in Task 10).

**Tests**:
- `cargo check` passes.
- Deploy + launch: spice2x log shows smoke-test output (registration success, duplicate rejection, scalar rejection) at init time.
- Remove smoke-test path before the feature ships (cleanup in Task 10).

**Dependencies**: Task 1 (services exist and init).

**Out of scope**: Any row rendering, any hook installation, any persistence. The registry is purely in-memory state at this point.

- [x] 2.1 Define `api.rs` types per the design's Public Contracts section. No function bodies yet.
- [x] 2.2 Implement `RegisterSpec::bool_toggle()` sugar that expands to a `UiKind::Enum` with two hardcoded `EnumValue`s pointing at stock `seop_op_off` / `seop_op_on`. No asset shipping required — these already exist in the game's atlas.
- [x] 2.3 Implement `registry.rs` with mutex-guarded state, duplicate detection, scalar rejection, value-cache init.
- [x] 2.4 Extend `mod.rs` with the public `register_option` / `get_value` / `resolve_from_load` surface.
- [x] 2.5 Wrap change-callback invocation in `panic::catch_unwind`.
- [x] 2.6 Add temporary init-time smoke test registering two options + verifying duplicate & scalar rejection.
- [x] 2.7 Validate: `cargo check`, deploy, confirm smoke-test log lines.

### Acceptance

**Status**: Approved 2026-05-07
**Notes**: Added `RegisterError::UnknownParent` (parent-first contract resolved mid-session) and `RegisterError::NoPages` (design mentioned the invariant but didn't assign a variant). Smoke-test branches A/B/C/D all verified on deploy; addresses for Task 1's RE-derived signatures match predictions exactly.

---

## Task 3: Row allocation + custom vtable with slot-4 override (enum only)

**Module(s)**: `src/services/custom_options/rows.rs` (new), `src/services/custom_options/mod.rs` (extend)

**Goal**: The framework can allocate a 0x330-byte row using `game_malloc`, call the `OptionElement<ArrowColor>` donor ctor on it, overwrite its primary vtable pointer at `+0x00` with a mod-authored vtable whose slot 4 is our own `advanceValue` impl. The row isn't yet registered anywhere the game will render it — that's Task 4 — but the allocation, layout, and vtable patch are all working and survive one-shot validation via Cheat Engine / debug log inspection.

**Scope**:
- `rows.rs`: Module owns the kept-alive storage of mod vtables (one vtable per option, because slot 4 closes over the option id) and row pointers. Public fn `allocate_row_for_option(handle, side) -> *mut u8` returns the ready-to-register row.
- Custom primary vtable: 8 slots, constructed as follows — slot 0/1/2/3/5/6/7 copied verbatim from the donor's primary vtable (read at init via `core::memory::read_ptr`), slot 4 pointing at our `advance_value_enum` function.
- `advance_value_enum`: reads the option's allowed-values list from the framework registry, computes the next value (cycling forward, or playing error SFX at endpoint per slot 4 native behavior), writes new value to the per-player cache, fires the mod's change callback.
- Allocation uses `FUN_180276a34` (game_malloc) to match the game's free discipline when the row is eventually destroyed; store donor ctor address via signature resolution.
- Row's category-tag metadata is written via `FUN_1800038b0` + `FUN_18004c540` per each `PageTag` in the option's registration (single-tag or multi-tag).

**Tests**:
- `cargo check` passes.
- Deploy: at init, for a test option registered via the smoke-test path (Task 2), log the allocated row's pointer, vtable pointer at `+0x00`, vtable[4] value — verify vtable[4] == our function's address, vtable[0/1/2/3/5/6/7] == donor's addresses.
- No in-game visual yet — row isn't in the scene graph.

**Dependencies**: Task 2 (registry exists so `advance_value_enum` has somewhere to read from).

**Out of scope**: Injecting the row into `FUN_180163970`'s OptionTab vector (Task 4). Scalar rows (Task 10).

- [x] 3.1 Resolve donor ctor, donor vtable, and `game_malloc` addresses from `core::signatures`.
- [x] 3.2 Implement `allocate_row_for_option(handle, side)`: allocate, call donor ctor, patch vtable.
- [x] 3.3 Implement `advance_value_enum` slot-4 function: cycle through allowed values, update cache, fire callback. Handle end-of-list with error SFX (reuse `FUN_180173c10`'s SFX primitives if reachable, else accept silent endpoints for v1).
- [x] 3.4 Write `PageTag`s into the row's metadata map via the tag-set primitive.
- [x] 3.5 Keep the row pointer + the synthesized vtable alive in module-level storage (never freed).
- [x] 3.6 Validate: `cargo check`, deploy, inspect logged vtable layout.

### Acceptance

**Status**: Approved 2026-05-07
**Notes**: Live-allocation path (`game_malloc` + donor ctor + mod vtable install + PageN tag) runs correctly but CANNOT be exercised from DLL-init thread — parent-class ctors dereference game globals not yet populated that early (deploy test crashed with EXCEPTION_ACCESS_VIOLATION at gamemdx+0x173810 inside the ArrowColor ctor). Init-time validation was swapped to a read-only `log_vtable_preview` that reads the donor vtable from `.rdata` and logs the synthesized mod vtable layout. Deploy validation confirmed every pointer: donor vtable at gamemdx+0x3772B8 (matches research doc), 7 donor slots all inside gamemdx `.text`, slot-4 override inside our DLL. New project learning saved capturing the "don't invoke native C++ ctors from DLL init" rule. Comment audit pass applied against global Learning 1 (no workflow references in source) and Learning 6 (no build stamps); rows.rs re-staged via two-step new-file workflow for reviewer-friendly `git diff HEAD`.

---

## Task 4: Builder hook detour — inject mod rows into `FUN_180163970`

**Module(s)**: `src/services/custom_options/builder_hook.rs` (new), `src/services/custom_options/mod.rs` (extend)

**Goal**: Mod options start appearing in the options menu. After the native row builder completes, for each active player side, the detour iterates registered options and appends rows via `FUN_180168c70(&shared_ptr, row_ptr)` into the flat OptionTab vector at `(parent + 0x230) + 0x68`.

**Scope**:
- `builder_hook.rs`: `retour::GenericDetour` on `FUN_180163970`. Installation gated on signature resolution success; graceful-degrades to no-op if signature missing.
- Detour body: call original first (natives register), then determine the player side (from `*(param_1 + 0x228)` per research doc), then iterate all registered options whose `pages` list contains at least one Page the active side shows. For each, call `allocate_row_for_option` (Task 3) and register via the same `FUN_180168c70` helper the native loop uses.
- Row registration already handles page tagging per Task 3 — no duplication here.
- No `ShowWhen` predicate evaluation yet (that's part of the framework's visibility service, deferred to after Task 8's end-to-end validation — track as follow-up).

**Tests**:
- `cargo check` passes.
- Deploy: register a single test enum option under `Page1` via the smoke-test path. Enter options menu on cabinet; confirm a new unlabeled/miss-textured row appears at the bottom of the Basic tab. (Visual glitches acceptable — textures don't exist yet; what matters is that the row exists and is pressable.)
- Press left/right on the new row; spice2x log shows the change callback fired with the new value.

**Dependencies**: Task 3 (row allocation works).

**Out of scope**: Page6 support (requires tab-count patch + filter detour — Task 5). Rendering quality (requires textures — Task 8). `ShowWhen` predicate (visibility service — Task 8 or follow-up).

- [x] 4.1 Install the `FUN_180163970` detour via `retour::GenericDetour`.
- [x] 4.2 Detour body: call original, then iterate registered options, call `allocate_row_for_option`, register via `FUN_180168c70`.
- [x] 4.3 Graceful-degrade if signature or hook install fails.
- [x] 4.4 Validate: `cargo check`, deploy, register Page1 test option, confirm row appears + callback fires on input.

### Acceptance

**Status**: Approved 2026-05-07
**Notes**: Added `option_tab_register` AOB signature (30 bytes, no wildcards, Ghidra-verified structurally unique + `aob_check.py` confirms single match on both 20260324 and 20250805 builds). Created `builder_hook.rs` with `retour::GenericDetour` on `row_builder_fn`; detour body calls original, reads active side from `parent+0x228`, iterates the registry, allocates rows via Task 3's path, pushes via the native register helper. Two deploy fixes landed during approval: (1) MSVC RTTI requires a `CompleteObjectLocator` pointer at `vtable[-1]` — our synthesized vtable was missing it, causing a caught `0xE06D7363` C++ exception from `__RTDynamicCast`; fixed by allocating N+1 qwords and copying the donor's `[-1]` slot. (2) Donor slots 6 (`onCreate`) and 7 (`render`) read per-KIND fields the native builder's subscriber wiring populates — our rows don't participate in that wiring, so first-render-tick crashed with an empty-name `afp_mc_load_bitmap` lookup; fixed by overriding both slots with a no-op `extern "C"` trampoline. Row is now invisible but pressable, matching the task's explicit acceptance criterion. Deploy screenshot shows a blank row below ARROW PLACEMENT on Basic tab — the injection landed. Two new project learnings saved: Learning 6 (MSVC vtable `[-1]` COL requirement), Learning 7 (donor slot inheritance unsafe for slots that read per-KIND fields).

---

## Task 5: Page6 tab infrastructure — tab-count patch + filter detour + AFP tab-list patch

**Module(s)**: `src/services/custom_options/filter_hook.rs` (new), `src/services/custom_options/tab_count_patch.rs` (new), `src/services/custom_options/afp_patches.rs` (new), `src/services/custom_options/mod.rs` (extend), `src/core/signatures.rs` (modify — resolve `option_form_tab_count_store`)

**Goal**: Page6 "Mods" tab becomes reachable. Scrolling right past Assist lands on a 6th tab. The tab has its own tab-strip slot (textures will be placeholder until Task 8), and options tagged `"Page6"` appear under it. Tab filtering works correctly for Pages 1–6 plus the `"System"`/`"Disabled"` magic keys.

**Scope**:
- **Tab-count patch**: CE-driven discovery of the `OptionForm+0x08 ← 5` store site. Update `option_form_tab_count_store` signature with the real AOB found at this task's implementation time. Patch the immediate from `5` to `6` using `core::memory::write_u8` (or similar). Graceful-degrade: if patch site can't be located, log WARN and skip Page6 support.
- **Filter detour**: `retour::GenericDetour` on `FUN_180168d10`. Rust reimplementation with `tab_names[1..6] = ["basic", "arrows", "lane", "judge", "assist", "mods"]`. Identical logic to the native handler: Phase 1 (set tab title), Phase 2 (iterate tab strip), Phase 3 (iterate row list, apply System/Disabled/Page-key filtering). FNV-1a hashing matches the native.
- **AFP tab-list patch**: `afp_patcher::register_patch("option_tab_list", ...)`. Callback parses the AFP via `core::afp`, adds a new `AP2PlaceObjectTag` for `tab6_usr` (source_tag_id=24, tx=286, pivot=(26, 11) if going uniform-52-px OR match existing stride if keeping native 58-px width and accepting visual unevenness — decide at implementation time), relabels existing tab6 (Back) to tab7_usr, updates all tx values accordingly.
- **Optional `option_tab_return` patch**: only if unifying the Back tab's width. Skip unless visual QA during implementation deems it necessary.

**Tests**:
- `cargo check` passes.
- Deploy: scroll past Assist in options menu → Mods tab appears (with placeholder art; textures land in Task 8). Scroll mod-tagged option into view to confirm filtering works.
- Register an option tagged `Page6` only (via smoke-test); confirm it appears under Mods but NOT under any other tab.
- Register an option tagged both `Page5` and `Page6`; confirm it appears under both (multi-tag validation).
- CE introspection to confirm `OptionForm+0x08` is now `6` after init.

**Dependencies**: Tasks 1 (signatures + services), 2 (registry), 3 (row allocation), 4 (rows actually inject into the menu).

**Out of scope**: Mods-tab textures (Task 8 ships placeholders). Scroll driver (Task 7). `ShowWhen` predicate (follow-up).

- [x] 5.1 CE session to locate the `OptionForm+0x08 ← 5` store site; update `option_form_tab_count_store` signature with the resolved AOB; patch immediate to `6`.
- [x] 5.2 Implement `filter_hook.rs` — Rust reimplementation of `FUN_180168d10` with 6-tab array.
- [x] 5.3 Implement `afp_patches.rs::patch_option_tab_list` — AFP rewrite adding `tab6_usr` and relabeling Back to `tab7_usr`.
- [x] 5.4 Register the AFP patch via `afp_patcher::register_patch` during `custom_options::init`.
- [x] 5.5 Graceful-degrade on any of the three patch points: log WARN, skip Page6 but keep Pages 1–5 fully functional.
- [x] 5.6 Validate: `cargo check`, deploy, confirm all four test cases (single-tag, multi-tag, tab switch to Mods, CE verification).

### Acceptance

**Status**: Approved 2026-05-09
**Notes**: Approved implicitly via user's task-reorder request. Implementation includes: real AOB for option_form_tab_count_store + scene_layout_flush signature, core::afp::make_place_object_full, tab_count_patch byte flip 5→6, bm2d_api mc_traversal + mc_load_bitmap wrappers, filter_hook detour with 6-tab table + FNV hashes + rb-tree walk + slot-5 onTick + scene_layout_flush, afp_patches uniform-stride option_tab_list rewrite. All gates clean from cargo clean.

---

## Task 6: Autoplay POC migration + placeholder assets (in-session toggling only)

**Module(s)**: `src/mods/autoplay.rs` (modify), `data_mods/.../tex/` (new assets), removed temp smoke-test registration from Task 2

**Goal**: The autoplay mod becomes the first real consumer of the custom options framework. Autoplay is per-player, toggleable in-game from both Mods and Assist tabs, with placeholder art so the tab looks like something. This validates the end-to-end path (registration → row injection → Page6 tab → in-game input → change callback) without persistence — values reset on card-swipe. Temporary smoke-test scaffolding from earlier tasks is removed.

**Scope**:
- `autoplay.rs` conversion:
  - Replace single-flag model with `[AtomicBool; 2]` per-player cache.
  - In `enable()`: call `custom_options::register_option(RegisterSpec::bool_toggle().id("autoplay").label_texture("seop_item_autoplay").pages(vec![Page5, Page6]).default_off().on_change(callback).build())`. The `bool_toggle()` sugar handles the stock `seop_op_off` / `seop_op_on` value-side sprites automatically. Change callback writes the per-player atomic.
  - Pre/post judge-hook callbacks: read the per-player atomic; no-op if the current side's flag is false.
  - `disable()` unregisters the option (or leaves it registered — framework's choice per lifecycle rules from design; decide at implementation time based on whether registrations should track mod lifecycle).
- Placeholder assets shipped to `data_mods/`:
  - `seop_tab_title_mods.png` (~156 × 36), `seop_tab_icon_mods.png` (~88 × 56) — simple solid-color PNGs with text label, approximate dimensions.
  - If design chose per-tab tab backgrounds: `seop_tab_mods_{on,off,on_alt}.png` at ~104 × 44 each. If shared-background model: skip.
  - `seop_item_autoplay.png` (176 × 16) — row label texture for the Autoplay option.
  - **NO value-side textures for autoplay** — the registration uses `RegisterSpec::bool_toggle()` which points at Konami's stock `seop_op_on` / `seop_op_off` sprites (already in the atlas, zero assets to ship).
  - **NO preview-illustration textures** (`seop_image_*`) are shipped for this feature. Autoplay appears on Assist and Mods tabs, neither of which uses a preview-illustration box natively.
  - `texturelist.merged.xml` registering the `seop_tab_*_mods` and `seop_item_autoplay` textures. Follow the existing `folder_expansion` / `series_expansion` pattern.
- Cleanup: remove the temp smoke-test registrations/logs added in Tasks 2, 3, 4.

**Tests**:
- `cargo check` passes.
- Deploy + launch.
- **US-2** validation: enter P1 options menu → confirm autoplay row appears under Assist AND Mods. Enter P2 options menu → confirm independent state (P1 autoplay=On doesn't affect P2's row value). Visual check: placeholder art looks reasonable at correct dimensions.
- **US-5** validation: 6 content tabs + Back visible in both P1 and P2 menus. Scroll through them, confirm Mods tab activates correctly. Confirm tab strip geometry doesn't overflow the 368 px container.
- **US-6** validation: disable `autoplay` in `mod-config.json` → restart → confirm no autoplay row in either tab. Re-enable → confirm row reappears.
- Confirm autoplay toggle works in gameplay (judge-hook integration).

**Dependencies**: Tasks 1–5 (framework scaffolding, row allocation, builder hook, Page6 tab).

**Out of scope**: Persistence (Task 8). Scroll driver (Task 7). Scalar-row support (Tasks 9–10). `ShowWhen` predicate validation (deferred).

- [x] 6.1 Migrate `autoplay.rs` to per-player `AtomicBool[2]`; register option via `custom_options::register_option(RegisterSpec::bool_toggle()...)` with `pages=[Page5, Page6]`.
- [x] 6.2 Update `autoplay_pre_judge` / `autoplay_post_judge` to read per-side atomic and no-op if false.
- [x] 6.3 Create placeholder PNG assets (simple programmatic flats with readable text, OR hand-authored — pick per design Open Question #4).
- [x] 6.4 Write `texturelist.merged.xml` registering all new textures.
- [x] 6.5 Remove all smoke-test / scratch registrations from Tasks 2, 3, 4.
- [x] 6.6 Validate: `cargo check`, deploy, confirm autoplay row visible + toggleable under Assist and Mods tabs, per-player independence, config gating.

### Acceptance

**Status**: Approved 2026-05-09
**Notes**: Position drift fix (bm2d_api set_position passed i32 bits where AFP expects f32; re-added per-frame position pinning in render_enum). Autoplay gating fix (play-side field is at actor+0x84, not +0x08; confirmed via Ghidra + source cross-reference). Both 20250805 and 20260324 builds validated. Task reorder: Tasks 9-10 (scalar foundation) prioritized ahead of Tasks 7-8 (scroll, persistence) to round out foundational capabilities before adding features.

---

## Task 7: Scroll driver — `options_scroll` service + AFP scroll-chrome injection

**Module(s)**: `src/services/options_scroll.rs` (extend), `src/services/custom_options/mod.rs` (extend — cross-service query APIs)

**Goal**: When a tab's row count exceeds viewport capacity, scroll chrome appears and up/down navigation auto-scrolls. Off-screen rows hide (via `"Disabled"` tag or direct position shift). Per-(side, page) scroll state. Graceful-degrades if scroll-chrome AFP injection fails — tab falls back to non-scrolling with clipped off-screen rows.

**Scope**:
- `options_scroll.rs`: Per-(side, page) scroll-state struct: visible capacity, total rows, focus index, scroll offset. Scroll state resets on tab switch (framework subscribes to the tab-switch reactive stream — hook into the same thunk `FUN_18017f030` used by the tab-filter, or observe via a separate mechanism).
- Cross-service query APIs added to `custom_options`:
  - `row_count_for_tab(side, page) -> usize`: walks the per-player registered-option list, returns count of options tagged for that page.
  - `row_handles_for_tab(side, page) -> Vec<RowHandle>`: returns the row pointers + row metadata for visibility manipulation.
- When row count ≤ viewport capacity: no scroll chrome, no behavior change.
- When row count > viewport capacity:
  - Inject scroll chrome into the `option` AFP template via `afp_patcher::register_patch` (adds `scroll_usr`, `move_usr`, `tri_l_usr`/`tri_r_usr` children if not already present).
  - On up/down input events (subscribed via `input_manager` or the same scroll-input path the menu already uses — discover at implementation time): advance focus, clamp to `[0, total-1]`, shift scroll offset if focus crosses viewport edge.
  - Hide off-screen rows: toggle `"Disabled"` metadata tag (triggers re-filter) OR overwrite `this+0x88..+0x90` to off-screen coords. Pick one at implementation time based on which is less invasive.
- Scroll driver only activates when AT LEAST ONE mod-tagged row has pushed row count past the native viewport. Native-only tabs with ≤ native capacity are untouched.

**Tests**:
- `cargo check` passes.
- Deploy with 8+ test options registered on a single tab (enough to exceed native capacity). Confirm scroll chrome appears. Press up/down — focus advances. Confirm rows beyond viewport hide properly. Confirm returning to a tab keeps prior scroll position within the same menu session.
- Register only 2 options on a tab — confirm no scroll chrome appears; tab behaves natively.
- Test P1 and P2 independently: P1 scrolls to row 5 on Page1, P2 stays on row 1 — confirm per-side isolation.
- Autoplay (Task 6) serves as the real-mod baseline row for validation alongside overflow dummies.

**Dependencies**: Tasks 2 (registry), 3 (row allocation), 4 (builder hook), 6 (autoplay provides a real row to test with).

**Out of scope**: Persistence (Task 8). Scroll-chrome placeholder art — reuse native scroll assets or ship simple placeholders inline.

- [x] 7.1 Add cross-service query APIs (`row_count_for_tab`, `row_handles_for_tab`) to `custom_options`.
- [x] 7.2 Implement per-(side, page) scroll state in `options_scroll`.
- [x] 7.3 Tab-switch subscription: reset scroll offset on tab change.
- [x] 7.4 AFP patch registration: inject `scroll_usr`/`move_usr`/`tri_l_usr`/`tri_r_usr` children into the `option` template.
- [x] 7.5 Up/down input handling: advance focus, shift scroll offset.
- [x] 7.6 Off-screen row hiding: pick `"Disabled"` tag OR position overwrite based on implementation-time profiling.
- [x] 7.7 Only activate scroll when row count > capacity for a tab.
- [x] 7.8 Validate: `cargo check`, deploy, test the 3 scenarios (overflow scroll, non-overflow no-scroll, P1/P2 independence).

### Acceptance

**Status**: Approved 2026-05-10
**Notes**: Scroll implemented via +0xB8 active-byte masking (chose position overwrite path for 7.6). AFP scroll-chrome injection (7.4) was unnecessary — the native layout engine repositions visible rows automatically when off-screen rows have +0xB8=0, so no visual scroll indicators needed. Key RE finding: the positional step function FUN_18004a030 (not FUN_18004a3c0) is the actual navigation path for the options GridPanel. The trampoline bypasses native position-based step logic entirely and returns computed vector indices directly, avoiding stale-coordinate issues on freshly-unmasked rows. Tab highlight fix (Bug 2) rebinds seop_tab_on_return/seop_tab_off_return via bm2d on each navigation event.

---

## Task 8: Persistence service — ess.dll detours + libavs-win64 bindings + `custom_options.persist` config key + end-to-end validation

**Module(s)**: `src/services/custom_options_persistence.rs` (extend), `src/mods/config.rs` (modify)

**Goal**: Mod option values round-trip through the backend. Save emits `<mod_{id}>` children under `<option>`; load parses them back and calls `resolve_from_load` into the framework. The `"custom_options": { "persist": true }` config key gates detour installation per Decision 12. This is the polish task that closes US-4 and validates the full feature end-to-end including persistence.

**Scope**:
- `config.rs`: Add `pub custom_options: Option<CustomOptionsConfig>` to `ConfigFile`. New `CustomOptionsConfig { persist: bool }` struct with `persist` defaulting to `true`. Follow existing serde pattern (see `layeredfs: Option<LayeredFsConfig>`).
- `custom_options_persistence.rs`: At init, read `config::get().custom_options.persist` (defaulting true). If `false`, log INFO and return — no detours installed.
- If `persist == true`: resolve ess.dll sender/receiver addresses (dispatcher-table walk primary, AOB fallback). Resolve libavs-win64 ordinals 162/163/175/176 via `GetProcAddress(MAKEINTRESOURCE(n))`.
- Install two detours: `sys_playerdata_save_sender` and `sys_playerdata_load_receiver`.
- Save-side detour: call original, then re-enter `/data → /option` via `Ordinal_162`, iterate `custom_options`'s registered-option snapshot, emit one `Ordinal_163(ctx, option_node, 6, "mod_<id>", &value)` per option for the current PlayerdataEntry's side.
- Load-side detour: call original, then for each registered option call `Ordinal_176(ctx, option_node, "mod_<id>", 6, &scratch, 4)`; on success call `custom_options::resolve_from_load(id, side, value)`. On absent field, value stays at default (already primed by registration).
- All failures log WARN and graceful-degrade (persistence off; in-menu toggling still works).
- Full end-to-end validation of the entire feature (US-1 through US-7) now that all pieces are in place.

**Tests**:
- `cargo check` passes.
- Deploy against a running bemani-buddy instance (operator's choice — note the companion feature may not yet exist; if so, backend silently ignores unknown fields per the assumption in design).
- **US-4** validation: toggle autoplay on P1, remove card, reinsert — confirm state persists IF backend echoes it back.
- Flip `"custom_options": { "persist": false }` in `mod-config.json` — confirm log shows persistence disabled at init, and card-swipe cycles reset the option to default (still toggleable in-menu).
- If backend rejects the save entirely (strict deny_unknown_fields): the operator flag is the escape hatch.
- **Full validation matrix**: confirm US-1 (registration), US-2 (per-player rows), US-3 (dependent visibility via scratch option — removed before commit), US-5 (6 tabs), US-6 (config gating), US-7 (scroll with overflow dummies — removed before commit).

**Dependencies**: Tasks 1–7. This is the ship-ready task that closes the enum-path feature.

**Out of scope**: Scalar-row support (Tasks 9–10). Final production-quality art (follow-on task; this task ships only placeholders). The companion bemani-buddy feature — noted in the design as a separate spec.

- [x] 8.1 Extend `config.rs` with `CustomOptionsConfig { persist: bool }` (default true).
- [x] 8.2 Resolve libavs-win64 ordinals 162/163/175/176 at init via `GetProcAddress(MAKEINTRESOURCE)`.
- [x] 8.3 Resolve ess.dll sender address via dispatcher-table walk (primary) and AOB (fallback).
- [x] 8.4 Install save-side detour: after original runs, emit `Ordinal_163` per registered option for the current side.
- [x] 8.5 Install load-side detour: after original runs, call `Ordinal_176` per registered option and push results into `custom_options::resolve_from_load`.
- [x] 8.6 Honor `"custom_options.persist": false` by skipping detour installation entirely.
- [x] 8.7 All failures degrade gracefully with WARN logs.
- [x] 8.8 Full end-to-end validation walkthrough across all 7 user stories on cabinet.
- [x] 8.9 Remove any scratch/dummy options used for US-3/US-7 validation before final commit.

### Acceptance

**Status**: Pending
**Notes**:

---

## Task 9: Scalar-row RE prerequisite (research only — NO CODE)

**Module(s)**: `docs/custom_player_options_research.md` (extend)

**Goal**: Decode everything needed to implement scalar-row support: the dedicated ctors for `OptionHispeed` / `OptionRealSpeed`, their vtable shape (probably differs from the enum-kind 8-slot vtable), the numeric-input path in their slot-4 equivalent, the display-formatting function that turns a numeric value into text for the clip's text field, and the field-offset layout for scalar-specific state (min/max/step/current). Update the research doc.

**Scope**:
- **RE work only.** No source-code changes in the modpack. No task files modified beyond `docs/custom_player_options_research.md`.
- Ghidra-driven static analysis + (if needed) CE-driven runtime validation, following the same RE protocol the feature's earlier phases used.
- Inputs: the gap items from `requirements.md`'s Dependencies section and the design doc's Decision 11.
- Outputs: a new section in the research doc fully documenting scalar rows, with addresses, offsets, callsites, and code citations from the game binary. Same documentation discipline as the existing sections on enum rows.
- Discuss findings with the user before proceeding to Task 10.

**Tests**: N/A (no code).

**Dependencies**: None (can be done any time — chosen to sit after Task 8 so enum feature ships first, but could run in parallel earlier if you want to front-load the unknown).

**Out of scope**: Any code implementation. Any scope extension beyond what's needed to implement scalar rows (e.g., don't RE all 20 enum kinds' ctors; we only need enough to understand scalar).

- [x] 9.1 Identify `OptionElement<int>::ctor` address (`FUN_180162240`) and its four MI vtables (`0x180373F38/F80/F98/FE0`). Runtime-validated on live Scroll Speed row at `0x226DF960`.
- [x] 9.2 Decode the primary vtable slot-by-slot for `OptionElement<int>` and identify the per-kind divergence at fourth-MI slot 0 (`<int>`=`FUN_180178c50` creator, `<ArrowColor>`=`FUN_18017bf20` teardown, `OptionTab`=`FUN_180153750` no-op).
- [x] 9.3 Map the numeric-input path: `FUN_180162680` (base `<int>` advanceValue) registers four lambdas per press — two per direction (consume/no-consume) split between fine step (`OptionTab+0x08`) and coarse step (`OptionTab+0x0C`). Value-list advance is `FUN_18017ef00`, which walks the 0x10-byte entries between `vec_begin`/`vec_end` updating `current_value` at `OptionTab+0x10`.
- [x] 9.4 Map the display-formatting path: the native path goes through the reactive stream at `row+0x208` where `lambda10` formats the value and calls `textlayer_set_text` (`FUN_1801d2aa0`) on the TextLayer at `row+0x130`. The TextLayer digit-compositor renders `seop_num_*` sprites via its internal `BmpString` renderer at `TextLayer+0x68`, which is lazy-allocated on first tick by `FUN_1801d3400` → `FUN_180029e30` (gated on priority `<7` being registered in the global render-group list at `DAT_1806ebee8` — priority 4 is the one used for row TextLayers).
- [x] 9.5 Field layout confirmed on live row at `0x226DF960`: `+0x118` AFP layer ptr, `+0x120` label TextLayer shared_ptr obj, `+0x128` label TextLayer ctrl block, `+0x130` value TextLayer shared_ptr obj, `+0x138` value TextLayer ctrl block, `+0x140` current-index animator (double), `+0x1F8` OptionTab shared_ptr obj, `+0x200` OptionTab ctrl block. OptionTab internals: `+0x00` current_index, `+0x08` step_left (fine), `+0x0C` step_right (coarse), `+0x10` current_value, `+0x18` flag bytes, `+0x20..+0x28` value vector (each entry 0x10 bytes with pointer at `+0x00`).
- [x] 9.6 Updated `docs/custom_player_options_research.md` with corrected visibility-lifecycle attribution, BmpString lazy-init section, per-kind vtable divergence table, runtime-validation subsection with ground-truth field values, and a concrete Task 10 handoff plan.
- [x] 9.7 User confirmed findings sufficient; approved 2026-05-09.

### Acceptance

**Status**: Approved 2026-05-09
**Notes**: Original handoff from prior RE session had three structurally wrong claims (wrong vtable-slot attribution for `FUN_180178c50`, wrong function role for BmpString creation, wrong row-field offsets). All three were caught during verification against Ghidra static analysis; the corrected picture was cross-validated against a live Scroll Speed row (current_value=480) via Cheat Engine. Two project learnings saved to `.spec/learnings/sdd-reverse-engineer.md` (Learnings 2 and 3) capturing the MI-base-offset decomposition pitfall and the handoff re-verification rule.

---

## Task 10: Scalar-row support — lift the `UiKind::Scalar` Err path, implement scalar rows

**Module(s)**: `src/services/custom_options/rows.rs` (extend), `src/services/custom_options/registry.rs` (modify — remove `ScalarUnsupported` error), `src/services/custom_options/mod.rs` (extend — scalar-specific slot-4 impl)

**Goal**: Mods can register `UiKind::Scalar` options and get working numeric-input rows with correct increment behavior and correct on-screen display. Feature reaches 100% of requirements US-1 and US-2's scalar coverage.

**Scope**:
- Remove the `RegisterError::ScalarUnsupported` early-return path from `registry.rs`.
- Add scalar-specific fields to the framework's per-option state (min, max, step, format).
- Add scalar donor-kind ctor resolution to `core::signatures` (identified in Task 9).
- Extend `rows.rs` with `allocate_scalar_row_for_option` — same donor-vtable pattern as enum, different donor ctor, scalar-specific slot-4 override (`advance_value_scalar`).
- `advance_value_scalar`: read current value from cache, apply `step` in the press direction, clamp to `[min, max]`, write to cache, fire change callback.
- Display formatting: ensure the scalar row's `render()` reads the framework-provided formatter. The exact integration depends on Task 9's findings; may require injecting a second lambda into the AFP display pipeline if the native scalar display formatter closes over kind-specific state.

**Tests**:
- `cargo check` passes.
- Deploy: register a scalar option (e.g. `"test_scalar"` range `0..100` step `5`, format `Integer`) under `Page6`. Enter Mods tab — confirm row renders as numeric input, left/right increments by 5, clamps at 0 and 100, display shows current value as text.
- Remove the scratch scalar option before the task commits; ship scalar support as a framework capability but don't add new production scalar rows in this task (autoplay stays enum-only per US-6).

**Dependencies**: Task 9 (RE findings). Tasks 2, 3, 4 (framework core). Task 5 (Page6 for test scalar option to live under).

**Out of scope**: Any new production scalar option for mods beyond the one-time scratch validation. Those land in future feature work.

- [x] 10.1 Added `option_element_int_ctor` + `option_element_int_primary_vtable` signatures; also added `event_register_no_consume` for the coarse-step register helper. Refactored the ArrowColor ctor+vtable derivation into a kind-parameterized helper so the two donor derivations share a single code path.
- [x] 10.2 Revised `UiKind::Scalar` to carry separate `step_fine` and `step_coarse` fields (replacing the single `step` field the design originally assumed) — mirrors the native scalar row's coarse/fine step mechanic.
- [x] 10.3 Removed `RegisterError::ScalarUnsupported` and the rejection branch in `try_register`.
- [x] 10.4 Added `allocate_scalar_row_for_option` in `rows.rs` using the OptionElement<int> donor so the inherited fourth-MI-slot-0 visibility handler creates the AFP layer + TextLayer shared_ptrs on tab show. Synthesized vtable inherits slots 0/1/2/3/5 verbatim; slot 4 = scalar advance, slot 6 = no-op, slot 7 = custom render.
- [x] 10.5 Implemented `advance_value_scalar_trampoline` with the 4-lambda registration pattern (left-fine, right-fine, left-coarse, right-coarse) against both `FN_EVENT_REGISTER` and `FN_EVENT_REGISTER_NO_CONSUME`. The engine's event-obj gate (`+0x10 == 1` vs `== 2`) automatically picks fine-vs-coarse based on whether Start is held. `press_body` generalizes to both kinds with step+clamp arithmetic for scalar, clamp-at-endpoint no-op to match native behavior.
- [x] 10.6 Implemented display via `render_scalar_trampoline` — per-frame position pinning on the AFP sub-clip, direct TextLayer tick on both label (`row+0x120`) and value (`row+0x130`) shared_ptrs (lazy-allocates BmpString on first tick), then pushes the formatted value via `textlayer_set_text` with a last-pushed-cache debounce. Label-texture binding moved inside `render_scalar` because scalar rows have `row+0x118` null at injection time. `format_scalar_value` supports `ScalarFormat::Integer` and `ScalarFormat::FixedPoint { decimals }`. Atlas cloner extended with row-major rect packing (2-px padding, grow to next power-of-2 up to 4096x4096) so multiple labels sharing `seop_item_appearance` as donor get unique atlas slots. Asset generation refactored to register labels automatically per-option via a derived `seop_item_<id>` naming convention — no centralized hardcoded list.
- [x] 10.7 Validated by deploying a scratch `hello-world` rewrite with a bounded scalar option (`bounce_count`, range 0..100, step_fine=1, step_coarse=10, format=Integer) that drove a swarm of bouncing text widgets. Confirmed: row renders on Mods tab with correct label, left/right steps by 1, Start+left/right steps by 10, clamps at endpoints, value display updates in real time via native digit compositor, scratch mod's count syncs to the scalar in real time. Scratch hello_world rewrite + bounce_count PNG reverted before task closure.

### Acceptance

**Status**: Approved 2026-05-09
**Notes**: End-to-end validation via the `bounce_count` scratch test confirmed every piece of the scalar path: OptionElement<int> donor selection (corrected from ArrowColor per Task 9 RE), 4-lambda fine/coarse registration, native TextLayer digit compositor rendering the formatted value, per-frame label binding on the `option_usr` child (because scalar `row+0x118` is null at injection time — populated later by the inherited visibility handler). Atlas-cloner packing extended with row-major allocation + power-of-2 growth so multiple labels sharing the `seop_item_appearance` donor get unique atlas slots. Two fixes landed during validation: (1) the initial render_scalar_trampoline delegated to the donor's native slot 7, which crashes when `row+0x110` is null — replaced with a direct implementation that ticks TextLayers and pins position without the scroll-speed-specific scene-graph dereferences; (2) `bind_textures` was initially skipped for scalar rows on the incorrect theory that TextLayers handle the label — moved label binding into render_scalar so it re-runs each frame when `row+0x118` is finally populated.

---

## Summary

| # | Task | Size | Dependencies | Ships |
|---|---|---|---|---|
| 1 | Signatures + scaffolding + init wiring | Small | — | Empty service shells in place |
| 2 | Registry + API + cache | Medium | 1 | In-memory registration works |
| 3 | Row allocation + enum slot-4 override | Medium | 2 | Rows allocatable, not yet registered |
| 4 | Builder detour (inject rows) | Medium | 3 | Mod rows visible in native tabs |
| 5 | Page6 infrastructure (tab count + filter + AFP) | Medium | 1, 2, 3, 4 | Mods tab reachable and filterable |
| 6 | Autoplay POC + placeholder assets | Medium | 1–5 | First real mod on the framework |
| 7 | Scroll driver | Medium | 2, 3, 4, 6 | Overflowed tabs scroll cleanly |
| 8 | Persistence + config key + end-to-end validation | Medium | 1–7 | **Feature ships (enum path)** |
| 9 | Scalar-row RE prerequisite | — | — (research only) | Research doc extended |
| 10 | Scalar row implementation | Medium | 2, 3, 4, 5, 9 | **Feature complete** |

After Task 8 the feature is shippable: enum options work, autoplay POC validates end-to-end, Page6 tab works, scroll works, persistence works. Tasks 9 and 10 add scalar support as a follow-on.
