# Orientation: Enable Background Dancers

Written: 2026-09-16 (Step 2 blind-spot pass). Sources: `docs/background_dancers_feasibility.md`
(the design-input record), `docs/3d_model_format_research.md`, `docs/custom_shader_backgrounds_research.md`,
`docs/bm2d_background_preview_research.md`, `docs/custom_resolution.md` §3a, `docs/chart_strip_hud_research.md`,
`scripts/anm_dump.py`, `scripts/ktmdl_dump.py`, `tools/blender_ddr_addon/`, and the Rust services named below.
Addresses are file-relative to `gamemdx.dll` @ `0x180000000`, build 20260825 unless noted.

## 1. What the idea actually asks for

A single-toggle mod. When ON, every GAMEPLAY song shows a randomly chosen A3 stage with
randomly chosen A3 dancer(s) animating behind the lane, using the data already in the stock
World install. No per-player rows, no persistence.

The feasibility doc settled the architecture ("Option A — hybrid"): the engine's whole 3D
pipeline survives in World (KTMDL loader, DDS/ANM callbacks, model shaders, the four
`MODEL:*` passes attached and running with an empty item list, `SceneGraph`/`SceneGraphManager`,
the camera object). Konami deleted only the game-side scene layer (node types, ANM evaluator,
pose chain, `CharaActor`/`StageActor`/`CameraActor`). The mod supplies that layer.

The user's "random stage + random dancer" constraint REMOVES two things the feasibility doc
planned for: the `musicdb.xml` `<bgstage>` song→stage table, and any character-selection UI.

## 2. Findings that change or sharpen the idea

### 2.1 Integration surfaces that already exist (reuse, don't rebuild)

| Need | Existing surface | Notes |
|---|---|---|
| Register an arc with the game FileManager so its `.model`/`.dds`/`.anm` members are loaded by the stock callbacks | `src/services/asset_loader.rs` — `load(arc_path, tex_name) -> Option<AssetHandle>` wraps `agcs::FileManager::Load(this, path) -> i32` (async; refcounted; MUST run on the render thread); `release(handle)` → `FileManager::Free` | `Load` takes ONLY a path — there is no category argument, so A3's `"3d-motion"` category question is moot for the load call itself. Readiness probe today is TEXTURE-only (`resource_manager_get_texture_data`); a **model-registry lookup** (`FUN_180202d50` family) is new signature work |
| Read `startup.arc` rlists / raw `.anm` bytes DLL-side | `src/core/arc.rs` — `parse`, `extract`, `ArcArchive::from_bytes` (avslz aware) | rlists are in `startup.arc` (`data/chara/chara_resources.rlist`, `data/map/map_resources.rlist`, `data/camera/{stage,music}_camera_resources.rlist`) |
| LayeredFS path resolution for mod-folder overrides | `avs_layeredfs/mod_paths.rs::find_first_modfile(norm_path)` (data-relative path, e.g. `arc/pl_emi00.arc`) | Locks `STATE` — never call from an `extern "C"` frame |
| Suppress the background movie for the song | `src/services/movie_policy.rs` — `MovieSuppressor { NonNativeOs, SongRate }` + `set_suppressed(source, bool)`; `fake_opened` writes player `+0x8 = 3`, leaves `+0x14 = 0` | Adding a `Dancers` variant = enum arm + `AtomicBool` + OR term in `should_suppress` (~6 lines). song_rate sets it tentatively at scene-26 arm and confirms at commit |
| Song time (content domain) | `song_reset::current_raw_music_count()` = `GamePlayActor+0x178` (sanity-ranged); valid only after `song_reset::first_anchored_frame()`. Under a committed song rate the Q31 stub makes `+0x178` CONTENT-domain already; under the audio clock it is the DAC-corrected count | No conversion needed for animation time: `t = mc/1000 s`, `frame = t·60`. `on_song_reset(cb)` exists but is unnecessary if animation time is re-derived from the count every frame |
| Scene callbacks | `scene_manager::on_scene_change(Box<dyn Fn(prev, next)>)` (0-indexed, fires BEFORE `createNextSequence`, outside the lock, in `catch_unwind`) | Constants in `src/types/scenes.rs`: SONG_TO_STAGE_INTERSTITIAL 26, STAGE_INDICATOR 27, GAMEPLAY 28, STAGE_RESULT (post-song loader) 29, ATTRACT_DEMO 16 |
| Per-frame game-thread callback | `input_manager::on_frame(Arc<dyn Fn()>)` — runs pre-original inside the layer-dispatcher detour; idle path must be O(1) | For enqueuing/polling only; the per-frame 3D work belongs in our node's `visit` (job-graph worker) |
| Render-thread scheduling | `widget_renderer::run_on_render_thread(FnOnce)` — never hold a mutex across it | FileManager load/free calls go through this |
| Entered sides | `stage_records::side_entered(side) -> Option<bool>` (`PlayerWork+0x4`); `stage_records::game_work()`; `multiplayer_bot::is_bot_side(side)` | The bot side reads as entered during a bot song |
| Mod-owned vtable objects | `custom_options/rows.rs::build_mod_vtable` (donor clone, COL at `[-1]`, `memory::alloc_zeroed` RWX) and the pure `foot_panel_swap/layout.rs::bot_vtable_image` shape with `offset_of!` layout tests | Our scene node has NO donor (World deleted `ModelNode`): slot 0 dtor + slot 1 `visit` are entirely ours; COL can be null unless something `__RTDynamicCast`s the node (learnings `:196-216`) |
| Signatures / derivations | `core/signatures.rs` — `SignatureDefinition{name,pattern,description}`, `resolve_all` → `resolve_derived` (`derive_*` fns), `publish_value`/`published_value` for non-address results, `find_vtable_by_rtti`, `xrefs_to` | Template for "all CALL sites must agree on a disp32" = `derive_smarvelous_burst`; all-or-nothing + identity gate = `derive_bottom_text` |
| Memory | `memory::alloc_zeroed` (VirtualAlloc RWX), `is_readable(ptr,len)` (VirtualQuery), `read_*/write_*` unaligned | Render items may be mod-owned (the engine never frees them); only bone TEXTURE handles are engine-owned |
| Mod skeleton | `src/mods/mod_trait.rs` (`Mod` trait, `DEFAULT_OFF_MODS`, `is_active` = "CAN work"), `hide_bottom_text.rs` as the thin-contributor template, registration list in `src/lib.rs:160-203` | |

### 2.2 Things that do NOT exist yet (all new work)

1. Model-resource registry lookup by name hash (World `FUN_180202d50`-family) — the readiness probe for `.model` members.
2. Engine dynamic-texture **create** on 20260825 (chart-strip HUD documents 20260721's `FUN_1802488e0(w, h, mips, fmt, usage)`; the model pass's own **lock/unlock** are `FUN_18024a1f0`/`FUN_18024a620` on 20260825) and — undocumented anywhere — **how such a handle is released**. Bone textures are `4×bone_count` `A32B32G32R32F` (fmt 0x74), usage 0x2001, two per skinned item (frame parity).
3. `SceneGraphManager` global (`DAT_1806f2d08`) + camera slot 0 layout (`graph+0x38`, 0x3F8 stride) + root insertion under the manager lock + the deferred-destroy vector (`mgr+0x08`).
4. The render-item ABI (0xC8 bytes, table in feasibility §5.1.1) and the node protocol (`+0x00` vtable, `+0x08` flags, `+0x0C` pass mask, `+0x10/+0x18/+0x20` tree links, `+0x78` render item).
5. ANM/CAMANM evaluator + pose chain + part attachment in Rust (pure; Python reference = `scripts/anm_dump.py::evaluate_pose`/`sample_track`/`decode_q48` and `tools/blender_ddr_addon/import_anm.py::game_camera_half_tangent`; ~800 LOC).
6. A way to make the 2D gameplay background transparent (see 2.4).
7. A shared `is_play_scene` helper (currently duplicated privately in `audio_clock/game.rs` and `s_marvelous/mod.rs`).

### 2.3 Data inventory (stock World install, verified this session)

- `data/arc/`: 115 `pl_*.arc` (bodies, parts, `pl_shadow00`), 26 `mapset_*.arc`, 24 `mc_*.arc`
  (`mc_male`/`mc_female` generic pools, `mc_bpm120`, 10 song-specific pairs + `mc_male_lesa`),
  `camera/stage_camera.arc` (93 `.camanm`) + 11 `camera/camera_music_*.arc`.
- `chara_resources.rlist`: 26 rows `key → [pl, sex M/F, class A/B/C, model_scale, shadow_scale, unlock_id]`.
  13 rows have `unlock_id 0.0` (the A3 no-card pool), 4 are event unlocks (16–19), 9 are `-1.0`
  (incl. `emi00`, `concent00`, `zukin00`, `pix00`). Class B rows are the small mascots
  (`babylon00` scale 0.4, `pix00` 0.4).
- `map_resources.rlist`: 34 rows; 7 are `dummy00` (never a real stage), `boom00` appears twice
  (row 0 without and row 32 with the `footpanel` part), `monitor00` twice, `replicant00..05` six.
  Every row's two colour fields are `000000`. Parts carry `:N` low-priority suffixes (`bg:-2`, `stage:-1`, …).
- `stage_camera_resources.rlist`: 34 rows parallel to the stage rows, each listing ~10–13 set names
  (`stNNN_stNN`, `stNNN_nonNN`, `stNNNx2_stNN`, `floor_stNN`); each `.camanm` is ~450 frames = 7.5 s.
- `music_camera_resources.rlist`: 12 song rows (`[num, num, num, music_<song>]` or an inline
  `name:time` cue list for `mawa`). The three numerics are untraced.
- Generic choreography: `mc_female.arc` = 13 `*_exec.anm` (~1133 frames ≈ 18.9 s each) + `ne01_loop.anm`
  (241 frames ≈ 4 s idle); `mc_male.arc` = 16 exec + loop (adds `ht04`, `br03`, `tu01` — `tu01` is absent
  from the feasibility doc's pool list). `mc_bpm120.arc` = a body model + `start01_exec`/`between01_exec`
  clips — never explained by any doc.
- Member ORDER inside the stock arcs is `.model` first, `.dds` after (the feasibility doc's "dds first"
  premise is wrong as written; whether the FileManager dispatches in table order is unknown).
- A3 `musicdb.xml` (bottle sibling install): 1099/1221 songs carry `<bgstage>`; 77 % use stages
  2/3/14/15/16/17. Irrelevant under the random-stage requirement but useful as a "which stages look
  good" prior.

### 2.4 The compositing problem, sharpened

The 3D passes render into the RENDER target (viewport 0x66) BEFORE RENDER_2D (0x68); RENDER_2D
clears depth only, so the 3D colour survives. Dancers are therefore automatically the frame
floor — but only if the 2D gameplay background above them is transparent or absent:

- **Movie** (`BgMovieActor`, owned by World's `SceneManageActor`): a `MovieSuppressor::Dancers`
  contributor gives the `fake_opened` no-movie state. With no config knob this means **movie songs
  lose their movie whenever the mod is ON** (Option A cannot draw dancers OVER a movie).
- **AFP background** (customize `background_gameplay`, `Customize+0x14`, backdrop manager
  `FUN_18003dfa0` → `bg_root` clip stored in `owner+0x140`): two candidate mechanisms, neither
  implemented:
  1. **Transparent placeholder arc** (the shader-background plan's §4.1a with alpha 0): ship a
     `background_09NN.arc` and temporarily write its id into `Customize+0x14` for the song, restoring
     at exit. Doc-recommended ("strongly preferred over hiding the layer"), but: touches a
     player-persisted field (restore must be guaranteed before the logout save), needs an authored
     AFP asset, and the placeholder's package-ready gate must never soft-lock the DPS.
  2. **Hide the live clip** — zero the colour alpha of the clip at `owner+0x140` via the existing
     `bm2d_api::layer_set_color_raw` once it exists (per-frame poll during GAMEPLAY). Zero assets,
     zero persisted-state risk, but needs the backdrop-owner global derived on all four builds, and
     the "known crash class" warning in the docs is about destroying/releasing packages under live
     layers — an alpha write does neither, but this has never been tried.
  Needs one Ghidra session on 20260825 (when `+0x14` is read; the owner global) before choosing.
- **"1st STAGE" banner** (`StageFrameActor`): leave — A3 showed it over the 3D too.

### 2.5 Unknowns that decide whether it "looks like A3" (not just "renders")

1. **Choreography sequencing.** Generic `_exec` clips are ~19 s; songs are ~2 min. How A3 chains
   them (loop one? random next? idle `ne01_loop` between? `mc_bpm120`'s `start`/`between` clips?)
   is untraced (A3 `SceneManageActor::onUpdate` `FUN_180060460`, `anim_player_apply_frame`
   `FUN_180158ad0`, model handle play `FUN_18001c750`).
2. **Tempo.** `mc_bpm120` strongly suggests clips are authored at 120 BPM and played back at
   `song_bpm/120` — `ne01_loop` = 241 frames ≈ 4.0 s = exactly 8 beats at 120. If A3 scales clip
   time by BPM, "dancing to the music" REQUIRES it; if not, dancers are visibly off-beat on any
   song not near 120 BPM. Untraced; decides whether the mod needs the song's BPM (available:
   music-DB entry core BPM, `song_rate::real_speed` reads it).
3. **Camera set switching.** Each stage lists ~10 sets of 7.5 s; A3's `CameraActor`
   (`FUN_180059a60`, set choice `FUN_180059d60`) cue rules are untraced. A random walk through
   the stage's sets on clip end is a plausible v0.
4. **Dancer spacing / placement** (`(i − (n−1)/2)·spacing`, spacing constant untraced) and
   whether dancers stand at the stage origin.
5. **Render-item ABI acceptance on a real cabinet** — the spike question. Everything read says
   yes; nothing has run.
6. **CrossOver / D3DMetal vertex-texture fetch** for the skinning VS — never exercised under World.
7. **Load latency** (~5–10 MB of arcs per song) and whether the KTMDL conversion's GPU uploads
   happen on the FileManager worker or the render thread — decides where the "ready" poll lives
   and whether arcs should be requested at scene 26/27 rather than 28.

Items 1–4 are A3 RE (one to three Ghidra sessions on `gamemdx_20240402_A3_Final.dll`, which is in the
connected `DDRWorld_Ghidra` project). Items 5–7 are cabinet/spike questions.

### 2.6 Constraints inherited from the repo

- No panics across FFI; every `extern "C"` (our node's `visit`/dtor) in `catch_unwind` or panic-free.
- One detour per target — this feature needs ZERO new detours for rendering (construct objects,
  hand them to the engine); the only hooks are existing scene/frame callbacks and the movie
  contributor. A background-hide mechanism may add one derivation, not a detour.
- No hardcoded offsets: every engine offset the item/node builders use must be pinned by an AOB
  or derived, then swept with `scripts/validate_signatures.sh` + `shape_diff.py` over the
  consumer functions (`FUN_180263430`/`FUN_180261780`/`FUN_180262670` twins) on all four builds.
- Allocator discipline: render items/nodes may be `memory::alloc_zeroed` (engine never frees
  them; we remove nodes only via the manager's deferred-destroy vector); bone textures via the
  engine texture API and released by us.
- Threading: `SceneGraph::update` (our `visit`) runs on a job-graph worker; the passes run later
  on the render walker under the item's frame stamp. All writes to item/node memory happen inside
  `visit`; the game thread only enqueues.
- `is_active()` must report "CAN work" (sites resolved), never "rendered this boot".
- Never commit Konami arcs; read from the install, LayeredFS-aware (`find_first_modfile`).
- Random-only selection means NO musicdb dependency and NO basename dependency — unless song-specific
  choreography/camera (`mc_<sex>_<song>`, `camera_music_<song>`, 11 songs) is included.

## 3. Proposed sequence

Clarification first (the register below is mostly product/scope decisions the user can settle
without RE), with two research tracks in parallel where a decision genuinely depends on them:
(a) A3 choreography sequencing + tempo + camera-set rules (Ghidra, A3 Final); (b) World backdrop
manager read site + owner global (Ghidra, 20260825) to pick the background-transparency mechanism.
Then readiness → design → plan, with the plan's first steps structured as the spike (static prop →
bind-pose skinned → animated) and an explicit go/no-go before the game-integration steps.
