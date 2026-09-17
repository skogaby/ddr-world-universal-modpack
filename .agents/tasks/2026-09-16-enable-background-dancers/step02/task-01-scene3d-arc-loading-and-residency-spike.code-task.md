# Task: `services/scene3d` arc loading + model-registry readiness, and the `background-dancers` mod skeleton with a developer residency harness

## Description
Create the first engine-facing slice of the Background Dancers feature: a `scene3d` service that
registers an A3 arc with the game's `FileManager` and reports when its `.model` members have become GPU
model resources in the `ResourceManager`; a `background-dancers` mod skeleton (id, name, default OFF,
scene + frame callbacks); and a developer-only harness (`DDR_DANCERS_SPIKE=1`, `layeredfs.developer_mode`
gated) that loads `mapset_boom00.arc` at GAMEPLAY entry, polls residency per frame, logs each model's
residency latency and `ResourceView` fields, and frees the arc at window exit. This is spike part 1
(plan Step 2): it proves the stock loader turns an A3 arc into GPU model resources when the DLL registers
it — nothing is rendered yet.

## Background
World's `Application::onBoot` still registers `agcs::ModelFileCallback` / `DdsFileCallback` /
`AnimeFileCallback`, so any arc handed to `FileManager::Load` has its `.model` members converted
(`FUN_180208fc0` → `FUN_1802030b0(kind, stem)` registers the GPU resource under **FNV-1 of the bare file
stem**, e.g. `gm_boom00_footpanel`; plain FNV-1 — NOT the texture hasher's lowercase/underscore-strip
variant). A3's lookup-by-hash was deleted, so `model_registry` walks the ResourceManager's model
`std::map` itself, read-only, under the map's own avs mutex, with every offset coming from the Step 1
`Scene3dSites` bundle (`docs/background_dancers_research.md` §1.5). The lock is libavs-win64 ordinal
16/17 reached through the IAT slots the derivation published (§1.2) — read the loader-patched pointer
out of the slot at call time.

`FileManager::Load` is async (`file_manager_load` description): the handle is valid immediately, the
model conversion lands on a worker thread — hence the per-frame poll. Load is refcounted; every `load`
pairs with exactly one `free` (`asset_loader.rs` is the reference for the load/free call shape and
threading rules: game thread only, never hold a state mutex across a `run_on_render_thread` schedule).

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.2.1 `arc_set`, §4.2.2 `model_registry` + `ResourceView`, §4.3.1 mod identity, §4.4 threading)
- RE record: `docs/background_dancers_research.md` §1.2 (lock protocol), §1.5 (model registry layout + the tree walk pseudo-code)

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 2 (demo line format)
- `src/services/asset_loader.rs` (the FileManager load/free wrapper this generalises — same fn types, same singleton deref, same threading contract)
- `src/services/avs_layeredfs/mod_paths.rs::find_first_modfile` (mod-folder override lookup; takes the `data/`-stripped path)
- `src/mods/hide_bottom_text.rs` (thin mod skeleton), `src/mods/two_player_bpl_mode/mod.rs` lines 285–380 (scene + frame callback registration/removal pattern, `developer_mode` + env-var dev gate, `ARMED`/`ENABLED` atomics)
- `docs/background_dancers_feasibility.md` §5.1.1 (the `res+0x1C/+0x20/+0x24/+0x28/+0x30/+0x48/+0x50/+0x68/+0x78/+0x88` GPU-resource field table `ResourceView` exposes)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/services/scene3d/mod.rs`: `pub fn init(signatures: &SignatureStore) -> bool` capturing `signatures.scene3d_sites()` + the existing `file_manager_load` / `file_manager_free` / `file_manager_singleton` addresses into a `OnceLock`; `pub fn is_available() -> bool`; `pub(crate) fn sites() -> Option<&'static Scene3dSites>`; re-export the submodules. Call `scene3d::init` from `src/lib.rs` after `asset_loader::init` (it needs nothing else). A missing group ⇒ one WARN, `is_available() == false`, no panic.
2. `src/services/scene3d/arc_set.rs` per design §4.2.1: `resolve_path(game_rel: &str) -> Option<String>` (input `data/arc/<file>`; mod-folder override via `find_first_modfile("arc/<file>")` — the `data/`-stripped form — else the stock path relative to the game's working directory; `None` if neither exists on disk — use `std::path::Path::exists`); `pub struct ArcSet` (`Vec<(String, i32)>`, NOT `Clone`); `load(paths: &[&str]) -> ArcSet` (game thread; `FileManager::Load(manager, c_path)` per path — pass the RESOLVED filesystem path when it came from a mod folder, the game-relative path otherwise; skip + WARN on `< 0` or a missing file); `free(set: ArcSet)` (game thread; `FileManager::Free` per handle, consumes the set); `read_bytes(game_rel: &str) -> Option<Vec<u8>>` (`std::fs::read` of the resolved path — any thread). Pure path logic (`stock_path_for`, the mod-relative form) factored so a host test can exercise it with a temp dir (no engine calls).
3. `src/services/scene3d/model_registry.rs` per design §4.2.2: `pub fn fnv1_name_hash(name: &str) -> u32` (offset basis `0x811C9DC5`, prime `0x01000193`, `hash = hash * prime ^ byte` — the ORDER the engine uses, verified in `FUN_1802030b0`: `uVar14 = uVar14 * 0x1000193 ^ byte`); `pub fn model_resource(name: &str) -> Option<*const u8>` = the §1.5 tree walk under the map mutex: `lock(rm + rm_model_mutex_off)` only when `*(i32*)(rm + mutex_off) > 0` (the engine's own gate), depth `+4` INC/DEC, head = `*(rm + map_off + 8)`, root = `*(head + 8)`, loop on `isnil(node + nil_off) == 0`: `key(node + key_off) < hash ? node = *(node + right_off) : { best = node; node = *(node + 0) }`; found iff `best != head && !(hash < key(best))`; value = `*(best + value_off)`; null ⇒ `None`. Every pointer dereferenced is probed with `memory::is_readable` first (a torn map is fail-open, never a fault). `pub struct ResourceView(*const u8)` with `is_skinned() = res+0x1C & 1`, `bone_count() = u32@+0x20`, `draw_record_count() = u32@+0x24`, `material_count() = u32@+0x28`, `palette_count() = u32@+0x30`, `bind() = ptr@+0x48`, `inverse_bind() = ptr@+0x50`, `draw_records() = ptr@+0x68`, `materials() = ptr@+0x78`, `palettes() = ptr@+0x88` — these GPU-resource offsets are the ONE place the design accepts literal engine offsets in this step (they are attested identical on all five builds by the Step 1 consumer shape diff; cite §1.9 in the doc comment and keep them as named `const`s at the top of the file so a future derivation can replace them).
4. The lock helper `pub(crate) unsafe fn with_avs_mutex<R>(mutex_field: *const i32, depth_field: *mut i32, f: impl FnOnce() -> R) -> R` in `scene3d/mod.rs` (shared later by `scene_graph.rs`): reads `*(sites.mutex_lock_iat as *const *const u8)` / `_unlock_iat` at call time, casts to `unsafe extern "C" fn(i32)`, calls lock iff `*mutex_field > 0`, INC depth, runs `f`, DEC depth, unlock iff `> 0` — the exact `FUN_180024250` / `FUN_180203b60` sequence. Both IAT pointers are validated non-null and `is_readable` before the first use (cache the check in an `AtomicU8` tri-state).
5. `src/mods/background_dancers/mod.rs`: `BackgroundDancersMod` — id `background-dancers`, name `Enable Background Dancers`, description `Random A3 3D stage + dancers behind the lane, rendered by the game's own model passes`; `required_signatures` = `&["scene3d_scene_graph_manager", "file_manager_load", "file_manager_free", "file_manager_singleton"]` (listing the group's anchor name is enough — the getter is all-or-nothing); `init` returns `scene3d::is_available()`; `enable` registers ONE scene callback + ONE frame callback (the two_player_bpl shape) and sets `ENABLED`; `disable` removes both and tears down any live spike state; `is_active() == scene3d::is_available()` ("CAN work"). Add the id to `DEFAULT_OFF_MODS`, `pub mod background_dancers;` in `src/mods/mod.rs`, and the `Box::new(...)` line in `src/lib.rs`'s `mods_to_register` (after `two_player_bpl_mode`).
6. `src/mods/background_dancers/spike.rs` (developer-only, removed in Step 7): armed iff `DDR_DANCERS_SPIKE` is set (read once at `enable`, logged; the developer_mode gate was dropped by the maintainer — env-var only). On scene → GAMEPLAY (prev ≠ GAMEPLAY): `arc_set::load(&["data/arc/mapset_boom00.arc"])` via `widget_renderer::run_on_render_thread`, record `Instant::now()`, set `POLLING`. Per frame while polling: for each of `gm_boom00_{bg,ripple,sp,spot,stage,footpanel}` not yet resident, `model_registry::model_resource(name)`; on first `Some` log `scene3d: <name> resident after <ms> ms (bones=N records=N materials=N palettes=N skinned=B)`; when all six are resident log `scene3d: mapset_boom00 fully resident after <ms> ms` and stop polling; 20 s without full residency ⇒ one WARN naming the missing models. On leaving {26,27,28}: `arc_set::free` via `run_on_render_thread`, log `scene3d: mapset_boom00 freed`, reset. The `ArcSet` lives in a `Mutex<Option<ArcSet>>`; nothing here may panic (`Mutex::lock().ok()` everywhere, no `unwrap`).
7. Logging: `scene3d::init` logs one INFO `scene3d: available (mgr=+0x…, rm=+0x…, tex create/release=+0x…/+0x…)` or one WARN; the mod's `enable` logs `BackgroundDancers: enabled (spike harness ON|off)`.
8. Rust Quality Rules: no `unwrap`/`expect`/indexing in the frame callback or scene callback; `unsafe` blocks narrow; all engine calls on the game thread; thread-safety via `OnceLock`/atomics/`Mutex` held only within a call.

## Dependencies
- Step 1 complete (`Scene3dSites` + `scene3d_sites()` in `src/core/signatures.rs`) — done, uncommitted in the working tree
- Existing: `file_manager_load` / `file_manager_free` / `file_manager_singleton` signatures, `avs_layeredfs::mod_paths::find_first_modfile`, `input_manager::on_frame`, `scene_manager::on_scene_change`, `widget_renderer::run_on_render_thread`, `memory::is_readable`
- Inferred: the game's working directory is the game folder (every other stock-path consumer in the DLL relies on it — `asset_loader` passes `data/arc/...` relative paths straight to the FileManager)

## Implementation Approach
1. `scene3d/mod.rs` with `init`/`is_available`/`sites`/`with_avs_mutex`; register in `services/mod.rs` and call from `lib.rs`.
2. `arc_set.rs` — pure path helpers + the FileManager wrappers (copy `asset_loader`'s fn types and singleton deref).
3. `model_registry.rs` — FNV-1 + the guarded tree walk + `ResourceView`.
4. Mod skeleton + `spike.rs`; wire into `mods/mod.rs`, `lib.rs`, `DEFAULT_OFF_MODS`.
5. Host test for the pure path resolution (temp dir; mod-folder-over-stock precedence, missing ⇒ None) and for `fnv1_name_hash` against a known vector (`fnv1("a") == 0x050C5D7E`, `fnv1("") == 0x811C9DC5`).
6. `cargo check` → `cargo fmt` (whole crate) → `./build.sh`.

## Acceptance Criteria

1. **Service resolves on a supported build**
   - Given a boot on any of the four supported builds
   - When `lib.rs` runs `scene3d::init`
   - Then the log carries exactly one `scene3d: available …` INFO (or one WARN and `is_available() == false` — never a panic)

2. **Harness proves residency**
   - Given `DDR_DANCERS_SPIKE=1` in the process environment and the mod ON
   - When a song is entered
   - Then `log.txt` shows six `scene3d: gm_boom00_<part> resident after N ms (…)` lines with plausible field values (footpanel: `bones=1 records≥1 skinned=false`; sp: `skinned=false`, materials ≥ 1) and one `fully resident` line; at song exit one `mapset_boom00 freed` line; three consecutive songs produce the same shape with no WARN and no growth in latency

3. **Fail-open without the group**
   - Given `scene3d_sites()` returns `None` (simulate by a corrupted pattern in a scratch build, or observe on a build where the harness reports the group missing)
   - When the mod is enabled
   - Then it reports `is_active() == false`, `enable` logs one WARN, and no callback is registered

4. **Host tests**
   - Given `cargo test` on the host
   - When the `scene3d` path and hash tests run
   - Then they pass (mod-over-stock precedence; missing ⇒ `None`; the two FNV-1 vectors)

5. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh` run
   - Then all are clean and no file outside `src/services/scene3d/`, `src/services/mod.rs`, `src/mods/background_dancers/`, `src/mods/mod.rs`, `src/mods/mod_trait.rs` (DEFAULT_OFF_MODS), `src/lib.rs` changed

## Metadata
- **Complexity**: Medium
- **Labels**: scene3d, background-dancers, step-2, spike, engine-facing
- **Required Skills**: Rust FFI in this codebase (the `asset_loader` idioms), the repo's mod/service registration conventions
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 2: `services/scene3d` arc loading and model-registry readiness (spike part 1)
