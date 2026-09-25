//! Signature Store — Centralized registry of known function signatures.
//!
//! Each signature maps a logical function name to an AOB pattern.
//! After AOB scanning, `resolve_derived()` computes additional addresses
//! from the found signatures (RIP-relative operands, RTTI walks, string refs).

use crate::core::module_resolver::GameModule;
use crate::core::scanner::{
    decode_call_rel32, decode_rip_relative, find_function_entry, scan_first_call_rel32,
    scan_pattern, scan_pattern_all, scan_patterns_batch, scan_xrefs_to,
};
use crate::{log_info, log_warn};
use std::collections::HashMap;

/// Addresses the Custom Resolution mod needs before `resolve_derived` runs —
/// see [`SignatureStore::custom_resolution_anchors`].
#[derive(Clone, Copy, Debug, Default)]
pub struct CustomResolutionAnchors {
    pub aa_config_imm: Option<*const u8>,
    pub graphics_init: Option<*const u8>,
    pub render_surfaces_global: Option<*const u8>,
    pub screen_w_global: Option<*const u8>,
    pub screen_h_global: Option<*const u8>,
}

pub struct SignatureDefinition {
    pub name: &'static str,
    pub pattern: &'static str,
    pub description: &'static str,
}

pub struct ResolveResult {
    pub found: usize,
    pub total: usize,
    pub missing: Vec<String>,
}

/// Build-dependent `GamePlayActor` field offsets (see
/// `SignatureStore::derive_gameplay_actor_layout`). All are byte offsets
/// from the actor; the speed cluster is f32/f32/(unused)/i32, the gauge
/// cluster five f32s, the death flags two bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GamePlayActorLayout {
    /// f32 current speed multiplier (ctor 1.0).
    pub speed_current: usize,
    /// f32 lerp-target multiplier (ctor 1.0).
    pub speed_target: usize,
    /// i32 ×100 integer copy of the multiplier (ctor 100).
    pub speed_int: usize,
    /// Gauge-percent tracking: min (ctor 1.0), max, last, accumulated
    /// loss, accumulated gain (ctor 0.0).
    pub gauge_min: usize,
    pub gauge_max: usize,
    pub gauge_last: usize,
    pub gauge_loss: usize,
    pub gauge_gain: usize,
    /// Instant-death gauge gate byte (`m_canInstantDeath`-equivalent).
    pub death_gate: usize,
    /// Death-result flag byte set by the `gauge::DEAD` handler.
    pub death_result: usize,
}

/// Build-dependent `ShutterActor` layout (see
/// `SignatureStore::derive_shutter_actor_layout`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutterActorLayout {
    /// i32 active shutter kind (-1 = none).
    pub active_kind: usize,
    /// i32 pending shutter kind (-1 = none).
    pub pending_kind: usize,
    /// The kind id of the stage-jacket "READY?" panel.
    pub stage_kind: i32,
}

/// Every address and offset DDR SELECTION's `LayoutActor` per-package helper
/// replacement consumes (see `SignatureStore::derive_ddr_selection`).
/// Addresses are absolute; offsets are byte offsets from the `LayoutActor`
/// (records / load list) or from `GameWork` (skin).
#[derive(Clone, Copy, Debug)]
pub struct DdrSelectionSites {
    /// The helper itself (detour target; also called as the original).
    pub package_helper: *const u8,
    /// `bool(const char* dir, const char* name)` — the arc version probe.
    pub probe: *const u8,
    /// `void(map* records, const char* key, const RecordValue* value)`.
    pub record_insert: *const u8,
    /// `void(vector<string>* list, const string* name)`.
    pub load_list_push: *const u8,
    /// The game's own `"bm2d"` literal (the probe's dir argument).
    pub bm2d_dir: *const u8,
    /// Global holding a pointer to the `GameWork` pointer (double hop).
    pub game_work_global: *const u8,
    /// Shared (side 2) record map.
    pub records_shared_off: usize,
    /// Side-0 record map; side 1 = + `records_side_stride`.
    pub records_side_off: usize,
    pub records_side_stride: usize,
    /// The load list every registered name is pushed onto.
    pub load_list_off: usize,
    /// `GameWork` i32 skin id (0 = World UI).
    pub gamework_skin_off: usize,
}

/// ddr_selection's legacy stage-panel sites (`derive_ddr_sel_panel`).
#[derive(Clone, Copy, Debug)]
pub struct DdrSelPanelSites {
    /// `ShutterActor::onUpdate` (RTTI vtable slot 6) — the detour target.
    pub shutter_update: *const u8,
    /// World's stage-voice function (state 2, stage kind only).
    pub stage_voice: *const u8,
    /// The default kind table's stage row (`{pkg, root, SE in, SE out, …}`).
    pub stage_row: *const u8,
    /// 0x40 (20260721+) / 0x30 (20250805, 20260224).
    pub row_stride: usize,
    /// Pending song basename `std::string` (written by the kind-3 fill).
    pub basename_off: usize,
    /// Active jacket-name `std::string` (copied at the swap).
    pub jacket_off: usize,
    /// World's state machine can host A3's legacy root on this build.
    pub host_ok: bool,
    /// Old layout only (20250805 / 20260224): the un-null-checked
    /// `CALL CMovieClip::SetVisible` on `find("jacket_usr")` in update state 2
    /// (5 bytes, return value unused) — NOPed while A3's root is hosted.
    pub jacket_vis_call: Option<*const u8>,
    /// The CLEARED / FAILED rows of the same table (stage row + 1 / + 2
    /// strides, content-gated) and their kind ids (stage kind + 1 / + 2) —
    /// the legacy end banners. `None` when the rows are not the stock rows.
    pub banner_rows: Option<DdrSelBannerRows>,
}

/// The two end-banner rows of the ShutterActor's default kind table.
#[derive(Clone, Copy, Debug)]
pub struct DdrSelBannerRows {
    pub cleared_row: *const u8,
    pub failed_row: *const u8,
    pub cleared_kind: i32,
    pub failed_kind: i32,
}

/// ddr_selection's `_sel` movie sites (`derive_ddr_sel_movie`).
#[derive(Clone, Copy, Debug)]
pub struct DdrSelMovieSites {
    /// `SceneManageActor::onInitialize` (RTTI vtable slot 4) — the detour
    /// target (it creates the song's MovieActor).
    pub sma_init: *const u8,
    /// The music-info lookup by basename (`const char*` → entry or null).
    pub music_lookup: *const u8,
    /// The entry's two movie bytes the gate reads (`+0x141` first, `+0x140`
    /// when that one is 5): 0 / 5 = no movie.
    pub movie_kind_off: usize,
    pub movie_kind2_off: usize,
    /// The entry's movie-name override `std::string` (empty ⇒ vslot 1).
    pub movie_name_off: usize,
    /// SceneManageActor: song basename / movie suffix `std::string`s, VIDEO
    /// SIZE (1 FULLSCREEN, 2 ON), the created MovieActor pointer.
    pub sma_basename_off: usize,
    pub sma_suffix_off: usize,
    pub sma_video_size_off: usize,
    pub sma_movie_off: usize,
    /// MovieActor: RTTI vtable, the `_sel`-first flag byte, the found movie
    /// path `std::string`.
    pub movie_actor_vtable: *const u8,
    pub sel_flag_off: usize,
    pub movie_path_off: usize,
}

/// The gameplay HUD layout builder / marker setter (`derive_hud_layout`) —
/// consumed by `services::hud_layout_hooks` (center-arrows' lane shift,
/// ddr_selection's legacy marker post-pass).
#[derive(Clone, Copy, Debug)]
pub struct HudLayoutSites {
    /// `LayoutActor`'s marker builder `void(LayoutActor*)`.
    pub builder: *const u8,
    /// Marker setter `void(parent, const char* key, const i32 coord[6])`.
    pub setter: *const u8,
    /// The builder's per-side extras (all-or-nothing): style
    /// (`+style + side*4`: 0 single / 1 double / 2 skipped), the reverse flag
    /// byte World latched (`+reverse + side*0x48`), and the Option vslot of
    /// `judge_position`. `None` when any failed.
    pub side: Option<HudLayoutSideSites>,
}

#[derive(Clone, Copy, Debug)]
pub struct HudLayoutSideSites {
    pub style_off: usize,
    pub reverse_off: usize,
    pub judge_pos_vslot: usize,
}

/// ddr_selection's legacy life-gauge sites (`derive_ddr_sel_gauge`). The
/// percent family (Normal / Grade / Flare / Immortal) shares one init and
/// one update; LifeGaugeActor has its own init.
#[derive(Clone, Copy, Debug)]
pub struct DdrSelGaugeSites {
    /// Percent-family `onInitialize` (RTTI slot 4) — post-original detour.
    pub gauge_init: *const u8,
    /// `LifeGaugeActor::onInitialize` (slot 4) — post-original detour.
    pub life_init: *const u8,
    /// The percent-family fill (`void(GaugeActor*)`, called by slot 6) — replaced
    /// for legacy actors.
    pub gauge_fill: *const u8,
    /// `LEA R8,[rip+"dance_gauge"]` — the clip-create export name, in each init.
    pub gauge_export_lea: *const u8,
    pub life_export_lea: *const u8,
    /// Per-side layout parent pointer (`**(actor+side_off)` = side).
    pub side_off: usize,
    /// Percent family: record skin, clip (`CMovieClip*`), displayed value
    /// (f32 0..1), state, and the state → label vslot.
    pub gauge_skin_off: usize,
    pub gauge_clip_off: usize,
    pub gauge_value_off: usize,
    pub gauge_state_off: usize,
    pub gauge_label_vslot: usize,
    /// LifeGaugeActor: record skin and clip.
    pub life_skin_off: usize,
    pub life_clip_off: usize,
    /// `CMovieClip`: the root MovieClip id (`+0x110`) and the SetScale vslot
    /// (`vt+0xC0`, `(this, f32 sx, f32 sy)`).
    pub clip_root_mc_off: usize,
    pub clip_set_scale_vslot: usize,
}

/// ddr_selection's A3 option icons (`derive_ddr_sel_option_icons`): World's
/// `sequence::dance::OptionIconActor`, World's `ddr::player::Option` field
/// offsets (each verified by its `MOV EAX,[RCX+off]; RET` getter stub) and
/// the game's `BM2D::CSprite` pool (RE:
/// `.agents/planning/2026-09-22-ddr-selection/research/option-icons.md`).
#[derive(Clone, Copy, Debug)]
pub struct DdrSelOptionIconSites {
    /// `onInitialize` (slot 4) / `onUpdate` (slot 6).
    pub init: *const u8,
    pub update: *const u8,
    /// The actor's side holder (`**(actor + holder_off)` = side).
    pub holder_off: usize,
    /// `record* (holder, const char* base)` / `marker* (holder, const char* key)`.
    pub record_fn: *const u8,
    pub marker_fn: *const u8,
    /// `Option* resolver(table[side], 0)` and the table.
    pub option_resolver: *const u8,
    pub option_table: *const u8,
    /// RTTI `ddr::player::Option` vtable.
    pub option_vtable: *const u8,
    pub fields: OptionFieldOffsets,
    /// RTTI `BM2D::CSprite` vtable, the pool (`count` × `stride`) and
    /// `void CSprite::Create(CSprite*, const char* texture, int priority)`.
    pub sprite_vtable: *const u8,
    pub sprite_pool: *const u8,
    pub sprite_count: usize,
    pub sprite_stride: usize,
    pub sprite_create: *const u8,
}

/// World `ddr::player::Option` field offsets (bytes).
#[derive(Clone, Copy, Debug)]
pub struct OptionFieldOffsets {
    pub speed_type: usize,
    pub hispeed: usize,
    pub speed_derived: usize,
    pub gauge: usize,
    pub scroll: usize,
    pub visibility: usize,
    pub lane_cover: usize,
    pub stepzone: usize,
    pub boost: usize,
    pub turn: usize,
    pub color: usize,
    pub cut: usize,
    pub freeze: usize,
    pub jump: usize,
    pub flare: usize,
}

/// World's `sequence::dance::CallVoiceActor` — the in-game announcer
/// (`derive_call_voice`, from the RTTI vtable). The actor keeps A3's field
/// layout on every build (checked by the derivation, see there).
#[derive(Clone, Copy, Debug)]
pub struct CallVoiceSites {
    /// `onUpdate` (vtable slot 6) — the per-frame announcer
    /// (== the `announcer_dispatcher` AOB).
    pub update: *const u8,
    /// `bool is_playing(AudioManager*, u32 handle)` (the voice guard's call).
    pub is_playing: *const u8,
    /// The audio-manager pointer global the guard passes.
    pub audio_manager_global: *const u8,
    /// The AVS lock-id global (`> 0` ⇒ lock / unlock around sound calls).
    pub lock_count: *const u8,
    /// IAT slots of libavs ordinals 16 / 17 (`lock(id)` / `unlock(id)`).
    pub lock_iat: *const u8,
    pub unlock_iat: *const u8,
}

/// World's `ComboActor` (`derive_combo_actor`, from the RTTI vtable) — the
/// four functions `services::combo_hooks` detours and the counter fields.
#[derive(Clone, Copy, Debug)]
pub struct ComboActorSites {
    /// `onInitialize` (slot 4), `onFinalize` (slot 5), `onUpdate` (slot 6),
    /// `onMessage` (slot 8).
    pub init: *const u8,
    pub finalize: *const u8,
    pub update: *const u8,
    pub msg: *const u8,
    /// i32 combo count and worst-grade index (0..=3; 0xFF = none) — written by
    /// the msg-0x1033 case, the worst grade saved at finalize.
    pub combo_off: usize,
    pub worst_off: usize,
    /// u8 set by msg 0x103C (game over), read by the update.
    pub gameover_off: usize,
}

/// ddr_selection's legacy-combo sites inside World's `ComboActor::onInitialize`
/// (`derive_ddr_sel_combo`; `research/legacy-combo.md` §5).
#[derive(Clone, Copy, Debug)]
pub struct DdrSelComboSites {
    pub actor: ComboActorSites,
    /// `MOV R15D,2; LEA R13D,[R15+1]; LEA RBP,[R14+disp32]` (17 bytes) — the
    /// three-root loop head (count imm at +2, first-root disp at +13).
    pub loop_head: *const u8,
    /// `LEA RDX,[rip+"dance_combo_root%d"]` (7 bytes) — after `MOV R8D,0x12`.
    pub root_fmt_lea: *const u8,
    /// `record_value* (side_holder, const char* base)` — the LayoutActor
    /// record lookup (skin at value+0x28).
    pub record_fn: *const u8,
    /// `const i32* (side_holder, const char* key)` — the marker lookup.
    pub marker_fn: *const u8,
    /// Actor field holding the side holder (`**(actor+off)` = side).
    pub side_off: usize,
    /// First root (root1) and last root (root3) fields; stride 8.
    pub root1_off: usize,
    pub root3_off: usize,
    /// `CMovieClip` vslots: SetPosition `(this, i32, i32)`, SetColor
    /// `(this, f32 a, f32 r, f32 g, f32 b)`; root MovieClip id field.
    pub clip_set_position_vslot: usize,
    pub clip_set_color_vslot: usize,
    pub clip_root_mc_off: usize,
}

/// ddr_selection's legacy-score sites in World's `ScoreActor`
/// (`derive_ddr_sel_score`; `research/legacy-score.md` §5).
#[derive(Clone, Copy, Debug)]
pub struct DdrSelScoreSites {
    /// `onInitialize` (slot 4), the digit refresh (slot 7), `onMessage` (slot 8).
    pub init: *const u8,
    pub digits: *const u8,
    pub msg: *const u8,
    /// The three clip creates in the init: `MOV R9D,7; LEA R8,[rip+name]`
    /// (13 bytes; priority imm at +2, name disp32 at +9) for `dance_score`,
    /// `dance_difficulty`, `dance_name`.
    pub create_score: *const u8,
    pub create_difficulty: *const u8,
    pub create_name: *const u8,
    /// `record_value* (side_holder, const char* base)` — the LayoutActor
    /// record lookup the init calls first (skin at value+0x28).
    pub record_fn: *const u8,
    /// Actor fields: side holder, record skin, level, score target, displayed
    /// score (`-1` = repaint all), difficulty, the three clips, EX flag.
    pub side_off: usize,
    pub skin_off: usize,
    pub level_off: usize,
    pub target_off: usize,
    pub displayed_off: usize,
    pub difficulty_off: usize,
    pub score_clip_off: usize,
    pub difficulty_clip_off: usize,
    pub name_clip_off: usize,
    pub ex_off: usize,
}

/// ddr_selection's legacy song-info patch site (`derive_ddr_sel_song_info`):
/// in `SongInfoActor::onInitialize`, `LEA RAX,["dance_song_info_single"]; LEA
/// R8,["dance_song_info_double"]; TEST; CMOVNE; MOV [RSP+0x20],1; MOV R9D,5`
/// (35 bytes: single disp32 at +3, double disp32 at +10, priority imm at +31).
#[derive(Clone, Copy, Debug)]
pub struct DdrSelSongInfoSites {
    pub site: *const u8,
    /// Skins 3–5 (A3's `dance_song_info0000_v2` panel with title / artist
    /// text) — `None` when that optional group did not resolve.
    pub panel: Option<DdrSelSongInfoPanelSites>,
}

/// ddr_selection's legacy song-info PANEL sites (skins 3–5,
/// `derive_ddr_sel_song_info_panel`) — every place World's SongInfoActor /
/// SongInfoChild differs from A3's for the `0000` panel (RE:
/// `.agents/planning/2026-09-22-ddr-selection/research/legacy-score.md` §7).
#[derive(Clone, Copy, Debug)]
pub struct DdrSelSongInfoPanelSites {
    /// The imm32 of `MOV R9D,4` (the SongInfoChild ctor's font id; A3: 3).
    pub font_imm: *const u8,
    /// The `JNZ rel8` after `TEST r8,r8` (style single ⇒ skip the white text
    /// colour write).
    pub color_jcc: *const u8,
    /// The child's `LEA reg,[rip+name]` loads: ctor `music_usr` ×2,
    /// `artist_usr` ×2, then update (vtable slot 6) the same (disp32 at +3).
    pub name_leas: [*const u8; 8],
    /// The text-create helper's `MOV [reg+0xA8],<zero reg>` (7 bytes — the
    /// horizontal alignment; A3: 1 = centred).
    pub align_store: *const u8,
    /// The helper's `SUB r32,r32` (2 bytes — World moves the text box one
    /// placeholder width left; A3 centres it on the placeholder).
    pub x_offset_sub: *const u8,
}

/// ddr_selection's legacy stage-frame patch sites (`derive_ddr_sel_stage_frame`).
#[derive(Clone, Copy, Debug)]
pub struct DdrSelStageFrameSites {
    /// `LEA R8,[rip+"dance_stage"]` (7 bytes) — the export-name load.
    pub export_lea: *const u8,
    /// `MOV R8D,0xB; LEA RDX,[rip+"dast_stage_"]` (13 bytes) — the texture
    /// prefix assign.
    pub texture_site: *const u8,
}

/// Every engine address and struct offset the Background Dancers 3D scene
/// service (`services::scene3d`) consumes, derived all-or-nothing by
/// `SignatureStore::derive_scene3d` (RE record:
/// `docs/background_dancers_research.md` §1). `scene3d_sites()` returns
/// `None` unless EVERY field resolved on this build.
///
/// Addresses are absolute; offsets are byte offsets from the object named in
/// the field's doc comment. Nothing here is hardcoded — each value is decoded
/// from the instruction stream of the World function that reads it.
#[derive(Clone, Copy, Debug)]
pub struct Scene3dSites {
    // ── SceneGraphManager / SceneGraph ──────────────────────────────────
    /// Global holding the `SceneGraphManager*` (`DAT_1806f2d08`): `*global`
    /// = mgr, `*(mgr + 0)` = the `SceneGraph` (which IS the root node).
    pub scene_graph_manager: *const u8,
    /// mgr: `std::vector<Node*>` deferred-destroy begin (end = +8).
    pub mgr_destroy_vec_off: usize,
    /// mgr: i32 avs mutex id (> 0 ⇒ lock live).
    pub mgr_mutex_off: usize,
    /// mgr: i32 lock nesting depth (INC/DEC inside the lock).
    pub mgr_depth_off: usize,
    /// mgr: f32 playback rate multiplied into the update dt.
    pub mgr_rate_off: usize,
    /// IAT slot holding the loader-patched `avs_mutex_lock(i32)` pointer
    /// (libavs-win64 ordinal 16 — the exact slot the flush calls).
    pub mutex_lock_iat: *const u8,
    /// IAT slot holding `avs_mutex_unlock(i32)` (ordinal 17).
    pub mutex_unlock_iat: *const u8,
    /// graph: u32 flags, bit0 = enabled (set by DPS step 5).
    pub graph_flags_off: usize,
    /// graph: first-child pointer (head-insertion point for our nodes).
    pub graph_root_child_off: usize,
    /// graph: u32 misc flags, bit0 ⇒ the visible vector is sorted.
    pub graph_sort_flag_off: usize,
    /// graph: camera vector begin (end = +8), `camera_stride` bytes each.
    pub graph_camera_vec_off: usize,
    /// node: render-item pointer read by `SceneGraph::update`'s item push.
    pub node_item_off: usize,
    /// node: i32 sort key read by the visible-vector std::sort.
    pub node_sort_key_off: usize,
    /// `SceneGraph::update` — identity/shape only, never called by the DLL.
    pub scene_graph_update: *const u8,
    // ── Camera slot (me::scene::camera::Camera, `camera_stride` bytes) ──
    pub camera_stride: usize,
    /// u8 active flag per camera slot.
    pub camera_active_off: usize,
    /// f32[16] view matrix (rebuilt by the tick).
    pub camera_view_off: usize,
    /// u8 projection-dirty byte polled by the tick.
    pub camera_proj_dirty_off: usize,
    pub cam_eye_off: usize,
    pub cam_target_off: usize,
    pub cam_up_off: usize,
    /// f32 `w` (1.0 ⇒ perspective), then l / r / b / t / near / far.
    pub cam_w_off: usize,
    pub cam_l_off: usize,
    pub cam_r_off: usize,
    pub cam_b_off: usize,
    pub cam_t_off: usize,
    pub cam_near_off: usize,
    pub cam_far_off: usize,
    /// u8 view-dirty request byte (write 1 after changing eye/target/up).
    pub cam_view_dirty_off: usize,
    /// u8 projection-dirty request byte (write 1 after changing the frustum).
    pub cam_proj_req_off: usize,
    // ── ResourceManager model registry ──────────────────────────────────
    /// Global holding the `ResourceManager*` (`DAT_1806f2f68`).
    pub resource_manager: *const u8,
    /// rm: i32 avs mutex id guarding the model map (depth at +4).
    pub rm_model_mutex_off: usize,
    /// rm: the model `std::map` object (head pointer at +8).
    pub rm_model_map_off: usize,
    /// map node: u8 isnil, u32 key (name hash), right child, value, refcount.
    pub rm_node_nil_off: usize,
    pub rm_node_key_off: usize,
    pub rm_node_right_off: usize,
    pub rm_node_value_off: usize,
    pub rm_node_refcount_off: usize,
    // ── Engine texture API ──────────────────────────────────────────────
    /// `u32 create(w, h, mips, fmt, usage)`.
    pub texture_create: *const u8,
    /// `i32 release(u32 handle)` — refcount after, or -1 when stale.
    pub texture_release: *const u8,
    // ── 2D background ───────────────────────────────────────────────────
    /// Global holding the `BgMovieActor*` singleton (`DAT_1806f2d38`).
    pub bgmovie_actor: *const u8,
    /// BgMovieActor: `BackgroundFrame*` (shared_ptr object pointer).
    pub bgframe_off: usize,
    /// BackgroundFrame: the live `bg_root` CMovieClip (shared_ptr object ptr).
    pub bg_clip_slot_off: usize,
    /// The 0x400-slot CMovieClip pool (`DAT_1806f9b20`), stride, count.
    pub cmovieclip_pool: *const u8,
    pub cmovieclip_pool_stride: usize,
    pub cmovieclip_pool_count: usize,
    // ── gs texture registry (OPTIONAL sub-group, see `Scene3dTextureLookup`) ──
    /// `None` when `texture_lookup_site` did not derive: the render-item
    /// builder then keeps the converter's material-texture pointers as-is
    /// (fail-open — only the re-resolve of not-yet-registered DDS members is
    /// lost, RE `docs/background_dancers_research.md` §2.1).
    pub texture_lookup: Option<Scene3dTextureLookup>,
    /// `gs::Shader* fn(u32 fnv1_name_hash)` — the shader registry lookup the
    /// converter resolves material shaders with (null on miss). OPTIONAL
    /// (`model_shader_select_site`): without it the render items keep the
    /// converter's shader objects and the whole-scene restyle is off.
    pub shader_lookup: Option<*const u8>,
    /// The viewport-pass compositor's sites (the Background Dancers option
    /// PREVIEWS). OPTIONAL (`scene3d_resolve_viewport`, nine AOBs,
    /// all-or-nothing): `None` ⇒ the option rows work, no live 3D preview.
    pub viewport: Option<Scene3dViewportSites>,
}

/// Everything `scene3d::viewport_pass` needs to attach mod-owned clones of
/// the MODEL passes (+ a clear viewport) into the RENDER_2D target list with
/// their own D3D viewport rect and camera matrices (design §4.5 / research
/// `preview-compositing.md`). Offsets are bytes from the object named in the
/// field's doc; every value is decoded from the engine's own instruction
/// stream (`derive_scene3d`'s viewport sub-group) and cross-checked across
/// the nine sites.
#[derive(Clone, Copy, Debug)]
pub struct Scene3dViewportSites {
    /// Global holding the display object pointer (`DAT_1806f2ef0`).
    pub display: *const u8,
    /// display: the RENDER_2D target list pointer (`+0x38`) — the list the
    /// clones attach to (its viewports draw after every AFP layer).
    pub render2d_list_off: usize,
    /// display: the RENDER-3D target list pointer (`+0x28`, informational —
    /// where the stock passes live).
    pub render3d_list_off: usize,
    /// `void attach(list, viewport, u32 prio)` — push + sort (`FUN_1802666c0`).
    pub attach: *const u8,
    /// `void detach(list, viewport)` — erase (`FUN_1802667d0`).
    pub detach: *const u8,
    /// target list: u32 flags (bit0 disabled, bit1 clear-at-start).
    pub list_flags_off: usize,
    /// target list: the target surface pointer (u16 dims at
    /// `target_w_off` / `target_h_off`) — the render-target pixel size.
    pub list_target_off: usize,
    pub target_w_off: usize,
    pub target_h_off: usize,
    /// The four stock pass globals in ctor order: DISTANTVIEW, OPACITY,
    /// LOWPRIO_TRANS, TRANS (`*global` = the 0xF8-byte pass object).
    pub pass_globals: [*const u8; 4],
    /// Pass object size (0xF8) — the clone's allocation.
    pub pass_size: usize,
    /// pass: the `gs::Renders::Model::Viewport<Render>` sub-object (the
    /// pointer the engine attaches / calls; `+0x30`).
    pub sub_off: usize,
    /// Its vftable (slot 0 = render(viewport, workerCtx), slot 1 = dtor) —
    /// the identity gate before cloning a stock pass.
    pub pass_vftable: *const u8,
    /// pass: u32 sort mode (`+0x28`), u32 node-mask FILTER (`+0x2C`).
    pub pass_sort_off: usize,
    pub pass_filter_off: usize,
    /// pass: i32 rect `{x, y, w, h}` (`+0x38`), f32 minZ / maxZ.
    pub pass_rect_off: usize,
    pub pass_minz_off: usize,
    pub pass_maxz_off: usize,
    /// pass: u32 name hash (`+0x50`), u32 flags (`+0x54`: bit0 DISABLED,
    /// bit1 skip camera upload).
    pub pass_name_off: usize,
    pub pass_flags_off: usize,
    /// pass: f32[16] projection (`+0x58`) and view (`+0x98`) — uploaded by
    /// the worker before the render callback iff flags bit1 is clear.
    pub pass_proj_off: usize,
    pub pass_view_off: usize,
    /// pass: self back-pointer (`+0xE0`), render-item list (`+0xE8`),
    /// callback block (`+0xF0`).
    pub pass_self_off: usize,
    pub pass_items_off: usize,
    pub pass_callbacks_off: usize,
    /// Viewport sub-object: the rect (`+8`) and flags (`+0x24`) the worker /
    /// dispatcher read (== `pass_rect_off − sub_off`, `pass_flags_off − sub_off`).
    pub vp_rect_off: usize,
    pub vp_flags_off: usize,
    /// Render worker ctx: the gd write pointer (`+0x218`) a render callback
    /// appends records at.
    pub gd_write_off: usize,
    /// The gd Clear record header (`0x00140000` = tag 0, size 0x14) and size.
    pub clear_tag: u32,
    pub clear_record_size: usize,
}

/// The gs texture registry's lookup trio, derived from the model converter's
/// texture-table fill (`texture_lookup_site`, RE §2.5). Lets the render-item
/// builder re-resolve material textures the converter resolved to the
/// DEFAULT texture because the DDS was registered after the `.model`.
#[derive(Clone, Copy, Debug)]
pub struct Scene3dTextureLookup {
    /// `gs::TextureData* fn(u32 gs_hash)` — null on miss. Must be called
    /// while holding `spin` exactly like the converter does (it lazily
    /// sorts the registry vector on the first lookup).
    pub lookup: *const u8,
    /// Global holding the default `TextureData*` (what the converter
    /// substitutes on a miss — comparing against it is how "unresolved" is
    /// recognised without a hash compare).
    pub default_texture: *const u8,
    /// The u32 spin flag: `while fetch_add(1) != 0 { SwitchToThread }` …
    /// `store(0)`.
    pub spin: *const u8,
}

/// Decode one `MOV [base+disp32], src` store at `p` whose primary opcode is
/// `opcode` (0x88 = r/m8,r8; 0x89 = r/m32,r32; 0xC6 = r/m8,imm8; 0xC7 =
/// r/m32,imm32 — or r/m16,imm16 with `want_66`). Accepts an optional REX
/// prefix and a SIB byte (so RDI- and R12-based actors decode alike), and
/// requires ModRM mod=10 (disp32). Returns `(instruction length, disp32)`.
///
/// # Safety
/// `p..p+16` must be readable.
unsafe fn decode_mem_store_disp32(
    p: *const u8,
    opcode: u8,
    want_66: bool,
) -> Option<(usize, usize)> {
    let mut i = 0usize;
    if want_66 {
        if *p != 0x66 {
            return None;
        }
        i += 1;
    }
    if (*p.add(i) & 0xF0) == 0x40 {
        i += 1; // REX
    }
    if *p.add(i) != opcode {
        return None;
    }
    i += 1;
    let modrm = *p.add(i);
    if (modrm & 0xC0) != 0x80 {
        return None;
    }
    i += 1;
    if (modrm & 7) == 4 {
        i += 1; // SIB
    }
    let disp = (p.add(i) as *const u32).read_unaligned() as usize;
    i += 4;
    i += match opcode {
        0xC6 => 1,
        0xC7 => {
            if want_66 {
                2
            } else {
                4
            }
        }
        _ => 0,
    };
    Some((i, disp))
}

/// Decode one scalar-single SSE instruction with a `[base + disp32]` memory
/// operand at `p`: `F3 [REX] 0F op ModRM(mod=10) disp32`. Returns
/// `(op, base_register, disp32, length)` — `op` is the second opcode byte
/// (`0x10` MOVSS load, `0x5C` SUBSS, `0x59` MULSS, …), `base_register`
/// includes the REX.B extension (1 = RCX, 7 = RDI). Anything else ⇒ `None`.
///
/// # Safety
/// `p..p+10` must be readable.
unsafe fn decode_sse_scalar_mem(p: *const u8) -> Option<(u8, u8, u32, usize)> {
    if *p != 0xF3 {
        return None;
    }
    let mut i = 1usize;
    let mut rex_b = 0u8;
    if (*p.add(i) & 0xF0) == 0x40 {
        rex_b = *p.add(i) & 1;
        i += 1;
    }
    if *p.add(i) != 0x0F {
        return None;
    }
    let op = *p.add(i + 1);
    let modrm = *p.add(i + 2);
    if (modrm & 0xC0) != 0x80 || (modrm & 7) == 4 {
        return None; // not mod=10, or a SIB form
    }
    let base = (modrm & 7) | (rex_b << 3);
    let disp = (p.add(i + 3) as *const u32).read_unaligned();
    Some((op, base, disp, i + 7))
}

/// Walk `body..body+len` instruction-agnostically (every byte position) and
/// collect the scalar-SSE memory operands in address order.
///
/// # Safety
/// `body..body+len+10` must be readable.
unsafe fn scene3d_sse_mem_ops(body: *const u8, len: usize) -> Vec<(usize, u8, u8, u32)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 10 <= len {
        if let Some((op, base, disp, n)) = decode_sse_scalar_mem(body.add(i)) {
            out.push((i, op, base, disp));
            i += n;
        } else {
            i += 1;
        }
    }
    out
}

/// Camera fields from the view-rebuild body (`me::scene::camera::Camera`,
/// World `FUN_180220b80`): `(view_dirty, eye, target, up)`. See
/// `docs/background_dancers_research.md` §1.7 for the attested shape.
fn scene3d_camera_view_fields(
    body: *const u8,
    len: usize,
) -> Result<(usize, usize, usize, usize), String> {
    const RCX: u8 = 1;
    unsafe {
        // First `CMP byte [rcx+disp32],0` within the prologue = view-dirty gate.
        let dirty = scan_pattern(body, 0x40, "80 B9 ?? ?? ?? ?? 00")
            .map(|r| (r.address.add(2) as *const u32).read_unaligned() as usize)
            .ok_or("view rebuild: no view-dirty CMP in the prologue")?;
        // The projection-request byte (dirty+1) and the `0x0101` re-arm store
        // (dirty+2) must both appear in the body, any base register.
        let mut saw_req = false;
        let mut saw_rearm = false;
        for r in scan_pattern_all(body, len, "80 ?? ?? ?? ?? ?? 00") {
            let modrm = *r.address.add(1);
            if (0xB8..=0xBF).contains(&modrm)
                && (r.address.add(2) as *const u32).read_unaligned() as usize == dirty + 1
            {
                saw_req = true;
            }
        }
        for r in scan_pattern_all(body, len, "66 C7 ?? ?? ?? ?? ?? 01 01") {
            let modrm = *r.address.add(2);
            if (0x80..=0x87).contains(&modrm)
                && (r.address.add(3) as *const u32).read_unaligned() as usize == dirty + 2
            {
                saw_rearm = true;
            }
        }
        if !saw_req || !saw_rearm {
            return Err(format!(
                "view rebuild: dirty byte chain not attested (req {} rearm {})",
                saw_req, saw_rearm
            ));
        }
        let ops = scene3d_sse_mem_ops(body, len);
        let eye = ops
            .iter()
            .find(|(_, op, base, _)| *op == 0x10 && *base == RCX)
            .map(|(_, _, _, d)| *d as usize)
            .ok_or("view rebuild: no MOVSS [rcx+disp32]")?;
        let target = ops
            .iter()
            .find(|(_, op, base, _)| *op == 0x5C && *base == RCX)
            .map(|(_, _, _, d)| *d as usize)
            .ok_or("view rebuild: no SUBSS [rcx+disp32]")?;
        if target != eye + 0xC {
            return Err(format!(
                "view rebuild: target 0x{:X} is not eye 0x{:X} + 0xC",
                target, eye
            ));
        }
        let up = target + 0xC;
        if !ops
            .iter()
            .any(|(_, op, _, d)| *op == 0x10 && *d as usize == up)
        {
            return Err(format!(
                "view rebuild: no MOVSS load of the up vector at 0x{:X}",
                up
            ));
        }
        if dirty <= up || dirty >= 0x1000 {
            return Err(format!(
                "view rebuild: implausible dirty 0x{:X} vs up 0x{:X}",
                dirty, up
            ));
        }
        Ok((dirty, eye, target, up))
    }
}

/// Camera frustum fields from the projection-rebuild body (World
/// `FUN_1802376e0`): the first seven `MOVSS xmm,[rcx+disp32]` loads in
/// order = `(w, near, far, l, r, b, t)`.
fn scene3d_camera_proj_fields(
    body: *const u8,
    len: usize,
) -> Result<(usize, usize, usize, usize, usize, usize, usize), String> {
    const RCX: u8 = 1;
    let loads: Vec<usize> = unsafe { scene3d_sse_mem_ops(body, len) }
        .into_iter()
        .filter(|(_, op, base, _)| *op == 0x10 && *base == RCX)
        .map(|(_, _, _, d)| d as usize)
        .take(7)
        .collect();
    if loads.len() != 7 {
        return Err(format!(
            "proj rebuild: expected 7 MOVSS [rcx] loads, found {}",
            loads.len()
        ));
    }
    let (w, near, far, l, r, b, t) = (
        loads[0], loads[1], loads[2], loads[3], loads[4], loads[5], loads[6],
    );
    if far != near + 4 || r != l + 4 || b != r + 4 || t != b + 4 {
        return Err(format!(
            "proj rebuild: frustum layout not contiguous (l/r/b/t {:X}/{:X}/{:X}/{:X} near/far {:X}/{:X})",
            l, r, b, t, near, far
        ));
    }
    if w >= 0x1000 || t >= 0x1000 || far >= 0x1000 {
        return Err("proj rebuild: implausible field offset".into());
    }
    Ok((w, near, far, l, r, b, t))
}

const SONG_RATE_CLOCK_ANCHOR_PATTERN: &str = "48 63 89 84 00 00 00 48 8D 35 ?? ?? ?? ?? 33 D2 48 8B 0C CE E8 ?? ?? ?? ?? 48 8B 10 48 8B C8 FF 92 48 02 00 00 44 8D 34 18 4C 8D 67 58 41 0F B7 54 24 2A";
// Pre-20260324 codegen (20250805 @ 0x1800598D5, 20260224 @ 0x180058915): the
// Option accessor takes no second argument, so the `33 D2` XOR EDX,EDX is
// absent and the redirect window sits at match+0x23 instead of +0x25. The
// eight redirect bytes and their register semantics are identical.
const SONG_RATE_CLOCK_ANCHOR_V1_PATTERN: &str = "48 63 89 84 00 00 00 48 8D 35 ?? ?? ?? ?? 48 8B 0C CE E8 ?? ?? ?? ?? 48 8B 10 48 8B C8 FF 92 48 02 00 00 44 8D 34 18 4C 8D 67 58 41 0F B7 54 24 2A";
const SONG_RATE_WAVEBANK_CREATE_PATTERN: &str = "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 28 FF FF FF 48 81 EC B0 01 00 00 48 C7 45 90 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 85 A0 00 00 00 48 63 F1 4C 8B 35 ?? ?? ?? ?? 49 8B 56 68 49 8B 46 70";
const SONG_RATE_WAVEBANK_UNREGISTER_PATTERN: &str = "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B 05 ?? ?? ?? ?? 48 8B 35 ?? ?? ?? ?? 48 63 F9 48 8D 14 BF 41 B8 03 00 00 00 48 C1 E2 05 48 03 50 28 0F B6 82 8F 00 00 00 48 8D 4C 10 11 48 8D 15 ?? ?? ?? ?? E8 ?? ?? ?? ?? 85 C0 75 ??";
const SONG_RATE_CLOCK_PATCH_OFFSET: usize = 0x25;
const SONG_RATE_CLOCK_PATCH_OFFSET_V1: usize = 0x23;
const SONG_RATE_CLOCK_EXPECTED: [u8; 8] = [0x44, 0x8d, 0x34, 0x18, 0x4c, 0x8d, 0x67, 0x58];
// Audio-manager ctor callback-registration region: `lookAheadTime = 0xFA`
// immediate followed by three `LEA RAX,[rip+disp32] / MOV [RBP+disp8],RAX`
// pairs (notification, readFile, getOverlappedResult). LEA disp32s and frame
// disp8s wildcarded; the 0xFA immediate and instruction shape are literal.
const SONG_RATE_IO_CALLBACK_REGSITE_PATTERN: &str = "C7 45 ?? FA 00 00 00 48 8D 05 ?? ?? ?? ?? 48 89 45 ?? 48 8D 05 ?? ?? ?? ?? 48 89 45 ?? 48 8D 05 ?? ?? ?? ?? 48 89 45 ??";
// disp32 positions of the second and third LEAs inside the regsite match
// (readFile and getOverlappedResult callback entries, RIP-decoded).
const SONG_RATE_IO_READFILE_LEA_DISP: usize = 21;
const SONG_RATE_IO_OVERLAPPED_LEA_DISP: usize = 32;
// The readFile callback body's literal prologue up to (and including) its
// first CALL opcode at entry+0x21 — `E8 rel32` to the handle→file_id lookup
// helper. Byte-identical on all four supported builds except the rel32.
const SONG_RATE_IO_READFILE_PREFIX: [u8; 34] = [
    0x48, 0x89, 0x5C, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x48, 0x89, 0x7C, 0x24, 0x20, 0x41,
    0x54, 0x48, 0x83, 0xEC, 0x40, 0x49, 0x8B, 0xD9, 0x41, 0x8B, 0xF0, 0x4C, 0x8B, 0xE2, 0x48, 0x8B,
    0xF9, 0xE8,
];
const SONG_RATE_IO_READFILE_CALL_OFFSET: usize = 0x21;
// Inside the `song_rate_wavebank_unregister` match: `MOV RAX,[rip+disp32]`
// at match+15 loads the audio file-table global (disp32 at match+18). The
// pattern's literal bytes already pin the access shape that global feeds
// (`+0x28` path-rows load, 0xA0-stride row math, `+0x11` path offset).
const SONG_RATE_IO_FILE_TABLE_MOV_OFFSET: usize = 15;
const SONG_RATE_IO_FILE_TABLE_MOV_OPCODE: [u8; 3] = [0x48, 0x8B, 0x05];
const SONG_RATE_IO_FILE_TABLE_DISP: usize = 18;

/// All known AOB signatures for DDR World (gamemdx.dll).
const SIGNATURES: &[SignatureDefinition] = &[
    SignatureDefinition {
        name: "timer_update_jz",
        pattern: "2B F9 33 F6 85 FF 7F ? 8B FE EB ? B8 ? ? ? ? ? ? 0F 4F ? ? ? ? ? ? ? 0F 84",
        description: "TimerActor update — JZ at +28 skips timer display update",
    },
    // ── TimerActor state-1 "show" site ────────────────────────────────
    // `sequence::common::TimerActor::onUpdate` (FUN_18003c790 on 20260616),
    // state 1's bottom block: the ONLY place in the binary that makes the
    // actor's `timer_root` layer visible. The layer is created hidden; when
    // the scene arms the timer (byte at actor+0xBC), this block plays the
    // "in"/"hazard_in" label and calls the play+set-visible helper
    // (layer_play(1.0) + afp_layer_set_attribute(id, 1, visible)) with that
    // byte as the visible flag. The timer-reset path (msg 0x1003 →
    // FUN_18003d3a0 → state 1) re-enters through this same instruction.
    //
    //   80 BD BC 00 00 00 00   CMP  byte [RBP+0xBC], 0     (armed gate)
    //   0F 84 rel32            JZ   epilogue
    //   8B 85 B0 00 00 00      MOV  EAX, [RBP+0xB0]        (total seconds)
    //   48 8D 0D d32           LEA  RCX, ["hazard_in"]
    //   48 8D 15 d32           LEA  RDX, ["in"]
    //   3B 85 B4 00 00 00      CMP  EAX, [RBP+0xB4]        (hazard threshold)
    //   48 0F 4F D1            CMOVG RDX, RCX
    //   41 B1 01 / 45 0F B6 C1 (label-play args)
    //   48 8B 8D A8 00 00 00   MOV  RCX, [RBP+0xA8]        (timer_root wrapper)
    //   E8 rel32               CALL SetFrameLabel helper
    //   0F B6 95 BC 00 00 00   MOVZX EDX, byte [RBP+0xBC]  <- patch at +62
    //
    // The timer-freeze mod rewrites the MOVZX at match+62 to XOR EDX,EDX +
    // 5 NOPs so the helper is always called with visible=0: the timer layer
    // (frame art + digit children — display collection skips the whole clip
    // tree when the layer attribute bit is clear) never shows, while the
    // state machine, countdown, and timeout semantics stay stock. Unique
    // single match on 20260526 (0x18003c162), 20260616 (0x18003cb72) and
    // 20260721 (0x18003c0b2), byte-identical apart from the wildcarded
    // displacements.
    SignatureDefinition {
        name: "timer_show_call",
        pattern: "80 BD BC 00 00 00 00 0F 84 ?? ?? ?? ?? 8B 85 B0 00 00 00 48 8D 0D ?? ?? ?? ?? 48 8D 15 ?? ?? ?? ?? 3B 85 B4 00 00 00 48 0F 4F D1 41 B1 01 45 0F B6 C1 48 8B 8D A8 00 00 00 E8 ?? ?? ?? ?? 0F B6 95 BC 00 00 00",
        description: "TimerActor onUpdate state-1 show site — MOVZX EDX,[actor+0xBC] at +62 feeds the layer set-visible helper; timer-freeze zeroes it to hide the timer display",
    },
    // Same site, pre-20260324 codegen (20250805 @ 0x18003C3AD, 20260224 @
    // 0x18003BCCD, unique on both): the `visible=1` argument is materialised
    // as `41 B0 01` (MOV R8B,1) instead of `41 B1 01 45 0F B6 C1`
    // (MOV R9B,1; MOVZX R8D,R9B), so the MOVZX patch site is at +58.
    SignatureDefinition {
        name: "timer_show_call_v1",
        pattern: "80 BD BC 00 00 00 00 0F 84 ?? ?? ?? ?? 8B 85 B0 00 00 00 48 8D 0D ?? ?? ?? ?? 48 8D 15 ?? ?? ?? ?? 3B 85 B4 00 00 00 48 0F 4F D1 41 B0 01 48 8B 8D A8 00 00 00 E8 ?? ?? ?? ?? 0F B6 95 BC 00 00 00",
        description: "TimerActor onUpdate state-1 show site, pre-20260324 codegen — MOVZX EDX,[actor+0xBC] at +58. timer-freeze uses it when `timer_show_call` misses.",
    },
    SignatureDefinition {
        name: "premium_free_stage_inc",
        pattern: "48 8B 08 FF 41 0C",
        description: "Per-frame stage counter increment — MOV RCX,[RAX]; INC dword [RCX+0xc]. Patch site at +3 (the 3-byte INC).",
    },
    // ── Per-stage play-record accessor ────────────────────────────────
    // `getStageRecord(side, stage)` — a tiny leaf accessor that returns
    // `PlayerWork + <base> + stage*<stride>` (or the course-mode record when
    // `GameWork+<course_off> != 0`). One match on builds 20260526/20260616,
    // byte-identical apart from the two RIP disp32s. The premium-free mod
    // decodes everything it needs from the matched bytes (bm2d_package
    // precedent — no hardcoded layout constants):
    //
    //   +0   MOV RAX,[rip+d32]        ; d32 at +3  -> game-work ptr global
    //   +7   MOV R8,[RAX]             ; ptr -> GameWork (double indirection)
    //   +10  MOVSXD RAX,ECX
    //   +13  LEA RCX,[rip+d32]        ; d32 at +16 -> player_work_table
    //   +20  CMP qword [R8+d8],0      ; d8 at +23  -> course-mode field (0x70)
    //   +25  MOV RAX,[RCX+RAX*8]      ; table[side] = wrapper*
    //   +29  MOV RAX,[RAX]            ; *wrapper   = PlayerWork*
    //   +32  JZ +7
    //   +34  ADD RAX,imm32            ; course record offset (0x2D8)
    //   +40  RET
    //   +41  MOVSXD RCX,EDX
    //   +44  IMUL RCX,RCX,imm32       ; imm32 at +47 -> record stride (0x2B8)
    //   +51  LEA RAX,[RAX+RCX+d32]    ; d32 at +55  -> record base (0x590)
    //   +59  RET
    SignatureDefinition {
        name: "stage_record_accessor",
        pattern: "48 8B 05 ?? ?? ?? ?? 4C 8B 00 48 63 C1 48 8D 0D ?? ?? ?? ?? 49 83 78 ?? 00 48 8B 04 C1 48 8B 00 74 07 48 05 ?? ?? 00 00 C3 48 63 CA 48 69 C9 ?? ?? 00 00 48 8D 84 08 ?? ?? 00 00 C3",
        description: "getStageRecord(side, stage) accessor — sources the game-work ptr global, player-work table, course-mode field offset, per-stage record stride and base. Consumed by the premium-free stale-record fix.",
    },
    // Same accessor, the OLDER codegen shape seen on 20250805 (0x1800AE7A0) and
    // 20260224 (0x1800B10E0) — one match each, none on 20260324+. The course
    // branch is taken FIRST (JZ skips it) and the stage record is computed as
    // `(stage + skew) * stride + *table[side]` with no separate base:
    //
    //   +0   MOV RAX,[rip+d32]        ; d32 at +3  -> game-work ptr global
    //   +7   MOV R8,[RAX]
    //   +10  MOVSXD RAX,ECX
    //   +13  LEA RCX,[rip+d32]        ; d32 at +16 -> player_work_table
    //   +20  CMP qword [R8+d8],0      ; d8 at +23  -> course-mode field (0x70)
    //   +25  JZ +14
    //   +27  MOV RAX,[RCX+RAX*8]
    //   +31  MOV RAX,[RAX]
    //   +34  ADD RAX,imm32            ; imm32 at +36 -> course record offset (0x2B8)
    //   +40  RET
    //   +41  MOV RCX,[RCX+RAX*8]
    //   +45  MOVSXD RAX,EDX
    //   +48  ADD RAX,imm8             ; imm8 at +51  -> stage skew (2)  => base = skew*stride
    //   +52  IMUL RAX,RAX,imm32       ; imm32 at +55 -> record stride (0x2B8)
    //   +59  ADD RAX,[RCX]
    //   +62  RET
    SignatureDefinition {
        name: "stage_record_accessor_v1",
        pattern: "48 8B 05 ?? ?? ?? ?? 4C 8B 00 48 63 C1 48 8D 0D ?? ?? ?? ?? 49 83 78 ?? 00 74 ?? 48 8B 04 C1 48 8B 00 48 05 ?? ?? 00 00 C3 48 8B 0C C1 48 63 C2 48 83 C0 ?? 48 69 C0 ?? ?? 00 00 48 03 01 C3",
        description: "getStageRecord(side, stage) accessor, pre-20260324 codegen (course record via ADD imm32, stage record via (stage+skew)*stride). stage_records decodes it when `stage_record_accessor` misses.",
    },
    // ── Premium Free ghost cache (same-credit PB ghost under a frozen stage) ──
    // `sequence::dance::GhostActor` init (20260721 `FUN_180056ad0`; 20260616
    // `0x180056b00`, 20260825 `0x180056a40` — match+0x0D..). Resolves the ghost
    // id via the score-DB lookup, then either copies a LOCAL stage slot's
    // grade stream (negative id → `PlayerWork[side] + 0x590 + stage*0x2B8 +
    // 0xB8`), kicks the network load (positive id), or leaves the vector
    // empty (id 0). Fields it pins (byte-identical on the 2026 builds):
    //
    //   40 53 48 83 EC 40          PUSH RBX; SUB RSP,0x40
    //   48 8B 05 ?? ?? ?? ??       MOV RAX,[security cookie]
    //   48 33 C4 48 89 44 24 30    cookie xor/store
    //   48 8B D9                   MOV RBX,RCX            (actor)
    //   8B 89 84 00 00 00          MOV ECX,[RCX+0x84]     (side)
    //   E8 ?? ?? ?? ??             CALL ghost-id lookup
    //   4C 8B D8                   MOV R11,RAX
    //   48 89 83 90 00 00 00       MOV [RBX+0x90],RAX     (ghost id)
    //   48 85 C0 75                TEST RAX,RAX; JNZ
    //
    // Detoured post-original by premium_free's ghost cache: when the game
    // resolved an EMPTY ghost vector (the frozen-stage slot was virginised
    // + re-prepared at song select), inject the cached same-chart stream.
    SignatureDefinition {
        name: "ghost_actor_init",
        pattern: "40 53 48 83 EC 40 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 44 24 30 48 8B D9 8B 89 84 00 00 00 E8 ?? ?? ?? ?? 4C 8B D8 48 89 83 90 00 00 00 48 85 C0 75",
        description: "sequence::dance::GhostActor init — ghost id lookup + local-slot / network ghost resolution. Detoured by the premium-free ghost cache.",
    },
    // The local-slot copy site INSIDE `ghost_actor_init` (match+0x1A1 on
    // 20260721): `IMUL R8,R8,0x2B8; MOV RCX,[RAX]; LEA RDX,[R8+RCX+d32];
    // LEA RCX,[RBX+0x98]; CALL vector<u8>::assign`. d32 = record base + 0xB8
    // (the record's grade stream): 0x648 = 0x590+0xB8 on 20260324+, 0x628 =
    // 0x570+0xB8 on 20250805/20260224 (PlayerWork grew 0x20 in between) — so
    // it is wildcarded; the runtime reads the stream through `stage_records`'
    // decoded layout, never this literal. The CALL rel32 at +25 resolves the
    // game's own `vector<u8>` copy-assign (`ghost_vec_copy`, derived) — the
    // allocator-correct way to fill `actor+0x98`. Unique on 20250805/20260224/
    // 20260616/0721/0825; the derivation pins it inside `ghost_actor_init`.
    SignatureDefinition {
        name: "ghost_local_slot_copy_site",
        pattern: "4D 69 C0 B8 02 00 00 48 8B 08 49 8D 94 08 ?? ?? 00 00 48 8D 8B 98 00 00 00 E8",
        description: "GhostActor init local-slot copy site — CALL at +25 is the game's vector<u8> copy-assign (derived as ghost_vec_copy).",
    },
    // ── Multiplayer Bot "Target Score" — the GamePlayActor's GhostActor field ──
    // `GamePlayActor::onUpdate` state 2 WAITS on its GhostActor before the
    // actor may advance toward the judging state (20260825 `FUN_18005cc70`
    // @ `0x18005d186`; 20260224 `0x180058dc6`; 20250805 `0x180059d86`):
    //
    //   48 8B 8F d32       MOV  RCX,[RDI+ghost_actor]   ; GamePlayActor+0x1F8 (2026-03+)
    //   48 85 C9           TEST RCX,RCX                 ;               +0x1F0 (20250805/20260224)
    //   74 ??              JZ   advance
    //   E8 rel32           CALL GhostActor::isReady     ; state[idx] == 2 || TIMEOUT_GHOST
    //   84 C0              TEST AL,AL
    //   0F 84 ...          JZ   keep_waiting
    //
    // The disp32 is the ONLY attested source of the GhostActor field: the
    // GamePlayActor layout forks at `+0x1F0` (the old builds sit 8 bytes
    // lower from here on), so `derive_ghost_actor_probe` publishes it as
    // `gpa_ghost_actor_off` — gated on the CALL target being the isReady
    // body (its prologue re-attests the GhostActor state layout). Consumer:
    // multiplayer_bot's ghost_source (the human's ghost vector at
    // `GhostActor+0x98`, read once per song — no detour). Unique on
    // 20250805 / 20260224 / 20260825 (Ghidra); 20260721 via the sweep.
    SignatureDefinition {
        name: "gpa_ghost_actor_probe",
        pattern: "48 8B 8F ?? ?? 00 00 48 85 C9 74 ?? E8 ?? ?? ?? ?? 84 C0 0F 84",
        description: "GamePlayActor::onUpdate state-2 GhostActor wait — disp32 at +3 = the GhostActor field (published as gpa_ghost_actor_off), CALL at +12 = GhostActor::isReady (identity gate).",
    },
    // The song-end result commit — GamePlayActor vtable +0x28 (20260721
    // `FUN_18005d970`, 20260526 `FUN_18005d180`). Copies the actor's live
    // judge counters / score cluster / grade decision / note + grade + ms
    // streams / gauge map into `PlayerWork + 0x590 + (GameWork+0xC)*0x2B8`
    // with REPLACE semantics. Two early-outs skip the whole commit:
    //
    //   40 53 56 57 48 81 EC 80 00 00 00   prologue
    //   80 B9 ?? ?? 00 00 00               CMP byte [RCX+d32],0     (skip flag 1; d32 at +13:
    //                                        0x280 on 20260324+, 0x278 on 20250805/20260224)
    //   48 8B F1 0F 85 ?? ?? ?? ??         MOV RSI,RCX; JNZ skip
    //   8B 81 94 01 00 00 03 81 9C 01 00 00  taps + shocks judged
    //   75 ??                              JNZ (else "MDX1529" no-judge report)
    //   48 8D 0D ?? ?? ?? ?? 33 D2 FF 15 ?? ?? ?? ??
    //   48 83 BE ?? ?? 00 00 00            CMP qword [RSI+d32],0    (skip flag 2 at +56; d32 at
    //                                        +59: 0x288 / 0x280 — always skip1 + 8)
    //
    // Both GamePlayActor skip-flag displacements are wildcarded (the actor grew
    // 8 bytes before 20260324); premium_free's diag decodes them from the match
    // at +13 / +59 instead of hardcoding 0x280/0x288. Unique on 20250805/
    // 20260224/20260526/20260721.
    //
    // Detoured post-original by premium_free's ghost cache (snapshot the
    // committed grade stream) + the bug-1 diagnostic (log the early-outs).
    SignatureDefinition {
        name: "result_commit",
        pattern: "40 53 56 57 48 81 EC 80 00 00 00 80 B9 ?? ?? 00 00 00 48 8B F1 0F 85 ?? ?? ?? ?? 8B 81 94 01 00 00 03 81 9C 01 00 00 75 ?? 48 8D 0D ?? ?? ?? ?? 33 D2 FF 15 ?? ?? ?? ?? 48 83 BE ?? ?? 00 00 00",
        description: "GamePlayActor result commit (vtable +0x28) — writes the per-stage play record at song end. Detoured by the premium-free ghost cache + diagnostic.",
    },
    // The in-song speed-mod adjustment window's kill gate, inside
    // `sequence::dance::ControlSpeedActor`'s message handler (vtable+0x40).
    // Each frame the gameplay sequence broadcasts msg 0x1045 with the elapsed
    // song time; at payload+0x8 >= 10000 ms the actor self-destructs, which is
    // the ONLY thing that ends the stock speed-adjust window:
    //
    //   41 81 78 08 10 27 00 00   CMP dword [R8+0x8], 0x2710  ; elapsed ms vs 10000
    //   0F 8C                     JL  <window still open>
    //
    // Every byte is structurally fixed (payload layout + the 10000 ms game
    // constant); the JL rel32 is excluded. Unique single match on builds
    // 20260421/20260526/20260616/20260721 (handler entry+0x4F on all four).
    // The anytime-speedmod mod rewrites the imm32 at match+4 to 0x7FFFFFFF so
    // the actor lives until the normal msg-0x104A song-end kill (untouched).
    // RE notes: docs/anytime_speedmod_research.md
    SignatureDefinition {
        name: "speedmod_window_gate",
        pattern: "41 81 78 08 10 27 00 00 0F 8C",
        description: "ControlSpeedActor msg-0x1045 self-destruct gate — CMP [R8+8],10000ms; JL. Anytime-speedmod patches the imm32 at +4.",
    },
    SignatureDefinition {
        name: "fps_target_imm32",
        pattern: "C7 44 24 ?? 3C 00 00 00 75 08 C7 44 24 ?? 4B 00 00 00",
        description: "Fullscreen display-refresh ('FPS') target in Application::onBoot — MOV dword [RSP+d],0x3C (default 60); JNZ +8; MOV dword [RSP+d],0x4B (75 if MachineType==1). The patchable imm32 is at match+4 (the 0x3C). FPS-unlock mod overwrites it (u32) before onBoot consumes it. Value is latched into the D3D device once at boot (never re-read). Unique single match, byte-identical on builds 20250805/20260324/20260526.",
    },
    // Landmark in the timing-init publisher: the four consecutive
    // `MOV EDX,[RBP+d]; LEA RCX,[rip+key]; CALL set_int` pairs that publish
    // SOUND/INPUT/RENDER/BOMB_FRAME offsets into the runtime config map. The
    // FIRST match is the SOUND_OFFSET set-pair; the CALL at match+0xA targets
    // the config-map int setter, which `timing_config_set_int` derives via
    // decode_call_rel32. Resolved this way (not by the setter prologue) because
    // the setter shares a byte-identical prologue with a sibling config-map
    // setter for a different map — only the publisher call-site disambiguates
    // it. The timing-offsets mod hooks the derived setter. Pattern matches the
    // overlapping 4-call run (3 hits) only inside the publisher on both builds.
    SignatureDefinition {
        name: "timing_set_call_landmark",
        pattern: "8B 55 ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 8B 55 ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ??",
        description: "Timing-init publisher config-set landmark: consecutive MOV EDX,[RBP+d]; LEA RCX,[rip+OFFSET_key]; CALL set_int pairs. First match = SOUND_OFFSET pair; CALL at +0xA derives timing_config_set_int (the config-map int setter). Used by the timing-offsets mod.",
    },
    SignatureDefinition {
        name: "hud_layout_builder",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 28 FE FF FF 48 81 EC B0 02 00 00 48 C7 45 20 FE FF FF FF",
        description: "Gameplay HUD/lane layout builder (entry). RCX = builder_root (a LayoutActor). Per-side layout parent at root+0xE0+side*0x48 (the `parent` the layout setter receives). Center-arrows mod hooks this to capture the builder root. NOTE: this prologue bakes in the stack-frame constants (LEA RBP disp / SUB RSP imm / cookie slot), which vary per build — it matches 20260324/0616/0721/0825 but NOT 20250805 (-0x1D8/0x2A0/+0x18) or 20260224 (-0x1C8/0x2A0/+0x18). `hud_layout_builder_style_cluster` is the build-stable fallback anchor.",
    },
    // Gameplay HUD/lane layout builder — the lane-name selection cluster 0x1DC
    // into the function body (byte-identical apart from the string disp32s):
    //   CMP dword [R13+0x84],1     ; P1 style == double?
    //   JNZ +9
    //   LEA RDX,["double_lane_usr"]
    //   JMP +0x18
    //   CMP dword [R13+0x88],1     ; P2 style == double?
    //   LEA RDX,[...]
    //   JZ +7
    //   LEA RDX,[...]
    //   CALL find_child
    // R13 = builder_root (RCX at entry). Unique single match on 20250805
    // (0x18006880C), 20260224 (0x18006789C), 20260324 (0x18006C40C), 20260616
    // (0x18006BD8C), 20260721 (0x18006BF6C), 20260825 (0x18006BF1C); the
    // function entry is exactly match-0x1DC on every one. The center-arrows mod
    // derives the entry from this match via a backward scan for the
    // frame-size-agnostic prologue head `MOV RAX,RSP; PUSH RBP; PUSH R12..R15`.
    SignatureDefinition {
        name: "hud_layout_builder_style_cluster",
        pattern: "41 83 BD 84 00 00 00 01 75 09 48 8D 15 ?? ?? ?? ?? EB 18 41 83 BD 88 00 00 00 01 48 8D 15 ?? ?? ?? ?? 74 07 48 8D 15 ?? ?? ?? ?? E8",
        description: "HUD layout builder lane-name style cluster (CMP [R13+0x84],1 / CMP [R13+0x88],1 selecting the lane clip name). Build-stable anchor for deriving the `hud_layout_builder` entry (backward prologue scan) when the frame-constant-bearing prologue AOB misses.",
    },
    SignatureDefinition {
        name: "hud_layout_setter",
        pattern: "4C 8B DC 56 57 41 54 41 55 41 56 48 83 EC 60 48 C7 44 24 20 FE FF FF FF 49 89 5B 18 49 89 6B 20 48 8B 05",
        description: "Named-layout setter: void(parent /*RCX*/, name /*RDX, C-string*/, coord /*R8, 6xi32; [0]=X,[1]=Y*/). Center-arrows mod detours this to shift coord[0] for the active 1P side's lane-relative keys. Pattern ends at the stack-cookie LEA opcode (the differing displacement is excluded); verified to match one site on both supported builds.",
    },
    // HUD layout builder per-side loop head (FUN_18006bd40+0x5A0 on
    // 20260825): `MOV EAX,[R13+RBX*4+style]; CMP EAX,2; JZ next_side` — the
    // per-side play style (0 single, 1 double, 2 = side skipped, no key
    // written). derive_hud_layout requires the match inside the builder and
    // publishes the disp32 (+4) as `hud_layout_style_off` (0x84 on every
    // build; cross-checked against the style cluster's literal 0x84).
    // Unique on all five builds.
    SignatureDefinition {
        name: "hud_layout_side_loop",
        pattern: "41 8B 84 9D ?? ?? ?? ?? 83 F8 02 0F 84",
        description: "HUD layout builder per-side loop head (MOV EAX,[R13+RBX*4+style]; CMP EAX,2; JZ skip). derive_hud_layout publishes the style disp32 as `hud_layout_style_off`. Consumer: ddr_selection's legacy marker post-pass (lane name per side).",
    },
    // The builder's per-side scroll-reverse latch (FUN_18006bd40+0x5C7 on
    // 20260825): `MOV RDX,[RAX]; MOV RCX,RAX; CALL [RDX+0x2F8] (Option::
    // isReverse); MOV [RSP+x],AL; LEA RCX,[RBX+RBX*8]; MOV [R13+RCX*8+rev],AL`
    // — World stores each side's reverse flag at LayoutActor + rev +
    // side*0x48 (the per-side marker parent + 4). derive_hud_layout
    // publishes the disp32 (+24) as `hud_layout_reverse_off` (0xE4 on every
    // build). Unique on all five builds.
    SignatureDefinition {
        name: "hud_layout_reverse_store",
        pattern: "48 8B 10 48 8B C8 FF 92 ?? ?? ?? ?? 88 44 24 ?? 48 8D 0C DB 41 88 84 CD ?? ?? ?? ??",
        description: "HUD layout builder: the per-side reverse flag store MOV [R13+RCX*8+0xE4],AL after the Option isReverse vcall. derive_hud_layout publishes `hud_layout_reverse_off`. Consumer: ddr_selection's legacy marker post-pass (reverse lane / difficulty variants).",
    },
    // The builder's judge-group lane variant (FUN_18006bd40+0x1839 on
    // 20260825): `MOV RCX,[RBP+x] (the side's Option); MOV R11,[RCX]; CALL
    // [R11+0x298]; CMP EAX,1; SETZ AL` — Option `judge_position` (+0x4C);
    // XORed with reverse it picks `lane_*_{normal,reverse}` for judge / combo
    // / fast_slow / filter / score_compare. derive_hud_layout publishes the
    // vslot (+10) as `hud_layout_judge_pos_vslot` (0x298 on every build).
    // Unique on all five builds.
    SignatureDefinition {
        name: "hud_layout_judge_pos_call",
        pattern: "48 8B 4D ?? 4C 8B 19 41 FF 93 ?? ?? ?? ?? 83 F8 01 0F 94 C0",
        description: "HUD layout builder: the judge_position Option vcall (CALL [R11+0x298]; CMP EAX,1; SETZ) that picks the judge-group lane variant. derive_hud_layout publishes `hud_layout_judge_pos_vslot`. Consumer: ddr_selection's legacy marker post-pass.",
    },
    // Song-info card builder — the branch cluster that picks the card style:
    //   CMP dword [RBP+0xC4],EDI   ; card style field: 0 = single, 1 = double
    //   SETZ R13B                  ; R13B = 1 when single
    //   LEA RAX,["dance_song_info_single"]
    //   LEA R8, ["dance_song_info_double"]
    //   TEST R13B,R13B
    //   CMOVNZ R8,RAX
    // R13B also gates the doubles dark-tint color write at the function tail
    // (TEST R13B / JNZ skip). The community hex patch for 20250805 (file offset
    // 476947: 41 0F 94 C5 -> 41 B5 00 90) forces R13B=0 here so 1P play gets
    // the dark transparent doubles card that doesn't occlude a centered lane.
    // The center-arrows mod reproduces that effect at runtime by detouring the
    // containing function (entry derived via backward prologue scan from this
    // match) and transiently flipping the +0xC4 style field for gated calls.
    // Unique single match on builds 20250805 (0x18007530D), 20260324
    // (0x18007951D), 20260616 (0x18007882D), 20260721 (0x180078C2D);
    // byte-identical apart from the wildcarded string LEA disp32s.
    SignatureDefinition {
        name: "song_info_card_style",
        pattern: "39 BD C4 00 00 00 41 0F 94 C5 48 8D 05 ?? ?? ?? ?? 4C 8D 05 ?? ?? ?? ?? 45 84 ED 4C 0F 45 C0",
        description: "Song-info card builder style branch: CMP [RBP+0xC4],EDI; SETZ R13B; LEA single/double card names; CMOVNZ. Center-arrows mod derives the builder entry from this match (backward prologue scan) and detours it to force the dark doubles card during centered 1P play.",
    },
    SignatureDefinition {
        name: "player_array_anchor",
        pattern: "48 8B 05 ?? ?? ?? ?? 66 C7 05 ?? ?? ?? ?? 00 FF 66 C7 05 ?? ?? ?? ?? 00 FF 66 C7 05 ?? ?? ?? ?? 00 FF",
        description: "Small lamp-state accessor whose first insn `MOV RAX,[RIP+disp32]` loads the 2-elem player-object array (P1=[0], P2=[1] at +8). The center-arrows mod RIP-decodes disp32 at +3 to get the array, then tests `*(*(*slot) + 4)` (per-side 'is playing' bool) for single-player detection. Several near-identical accessors match this pattern; all reference the same array global, so the first match's disp is authoritative.",
    },
    SignatureDefinition {
        name: "wrapper_render",
        pattern: "48 83 EC 28 48 8B 49 18 48 8B 41 08 48 89 05 ? ? ? ? 48 8B 41 10 48 89 05 ? ? ? ? 48 8B 41 18 48 8B 09 48 89 05 ? ? ? ? 48 8B 01 FF 50 08",
        description: "agcs::BmpString vtable[5] — sets font globals before render",
    },
    SignatureDefinition {
        name: "render_function",
        pattern: "4C 8B DC 55 53 49 8D AB 68 FF FF FF 48 81 EC 88 01 00 00 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 C8 48 8B 41 08 48 8B D9 80 78 49 00",
        description: "kt::BmpfontSimpleString vtable[1] — text rendering",
    },
    SignatureDefinition {
        name: "widget_factory",
        pattern: "40 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 40 48 89 6C 24 48 48 89 74 24 50 41 8B",
        description: "Creates kt::BmpfontSimpleString instances",
    },
    SignatureDefinition {
        name: "constructor",
        pattern: "48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 50 48 8B F9 48 8D 05 ? ? ? ? 48 89 01 33 DB 48 89 59 08 45 33 C0 BA C0 00 00 00",
        description: "kt::BmpfontSimpleString constructor",
    },
    SignatureDefinition {
        name: "series_mapper_bounds",
        pattern: "40 0F B6 C6 FF C8 83 F8 14 0F 87",
        description: "Series mapper bounds check — movzx eax,sil; dec eax; cmp eax,0x14; ja default",
    },
    SignatureDefinition {
        name: "version_predicate_lea",
        pattern: "48 8B 50 08 48 8B 0A 48 3B CA 74 ? 4C 8D 05",
        description: "Version filter predicate — MOV+MOV+CMP+JZ before LEA R8 table base, LEA at offset 12",
    },
    SignatureDefinition {
        name: "ui_entry_loop",
        pattern: "BE 08 00 00 00 48 8D 1D",
        description: "FilterButton creation loop — MOV ESI,8 + LEA RBX,[last_entry_key]. Count at offset 1, LEA at offset 5.",
    },
    SignatureDefinition {
        name: "thumbnail_arc_loop",
        pattern: "48 FF C6 48 83 FE 15 0F 86",
        description: "Thumbnail ARC loading loop bound — INC RSI; CMP RSI,0x15; JBE. Series limit at offset 6.",
    },
    // Leaf function `fn(category: u32) -> u32` returning the entry count for a
    // filtersort category. Drives the filtersort selection<->bitfield round-trip
    // (the `version` u64 saved to the profile): the save mask-builder and the
    // load apply-loop both bound their per-entry bit loops by this count. The
    // VERSION category is index 1, whose count is hardcoded to 9 (the stock
    // entry count) — so custom series entries (selection-map index >= 9) never
    // get a bit on save and are never restored on load. series_expansion detours
    // this to return 9 + n_custom for category 1. Match is at function entry.
    // `cmp ecx,0xC; ja; lea rdx,[jumptable]; movsxd rax,ecx`.
    SignatureDefinition {
        name: "filter_entry_count_table",
        pattern: "83 F9 0C 77 ?? 48 8D 15 ?? ?? ?? ?? 48 63 C1",
        description: "Per-category filtersort entry-count leaf fn (category:u32)->u32. VERSION=category 1, hardcoded 9. Detour target for the version-bitfield persistence fix.",
    },
    SignatureDefinition {
        name: "filter_button_panel_config",
        pattern: "48 8B C4 55 57 41 54 48 8D 68 A1 48 81 EC B0 00 00 00 48 C7 45 E7 FE FF FF FF 48 89 58 10 48 89 70 18",
        description: "FilterButton panel config — called for EVERY FilterButton (groups + versions). Sets category at +0xF0, finds BM2D template. Params: (RCX=FilterButton*, EDX=category_index).",
    },
    SignatureDefinition {
        name: "bm2d_pool_iter",
        pattern: "FF C3 48 81 C7 40 02 00 00 81 FB 00 04 00 00",
        description: "BM2D pool iteration — INC EBX; ADD RDI,0x240; CMP EBX,0x400. LEA Rxx,[pool_base] is within 64 bytes before match.",
    },
    SignatureDefinition {
        name: "filter_panel_builder",
        pattern: "48 8B C4 55 41 54 41 55 48 8D 68 B8 48 81 EC 30 01 00 00 48 C7 44 24 68 FE FF FF FF",
        description: "Filter category panel builder — unique prologue with stack cookie at [RSP+0x68]. Creates filter_switch_base BM2D template and renders FilterButton entries.",
    },

    // FilterButton::~FilterButton (destructor body). Fires per filter button as
    // the filter category panel tears down on filter-menu close — the moment the
    // game frees the button objects that series_filter_scroll tracks by raw
    // pointer. The bare dtor prologue is the generic MSVC two-vtable shape (4
    // matches), so the signature extends through the body: two
    // `FilterButton::vftable` LEA/writes (`[RCX]` and `[RCX+0x28]`), then the
    // distinctive `CALL <panel release>; NOP; MOV RCX,[RBX+0x1B0]` tail. Wildcards
    // cover the 3 vtable-LEA/CALL disp32s. Verified unique + cross-version:
    // 20260421 (FUN_180134260) and 20260526 (FUN_180133ba0).
    SignatureDefinition {
        name: "filterbutton_dtor",
        pattern: "48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 48 48 89 6C 24 50 48 89 74 24 58 48 8B D9 48 8D 05 ? ? ? ? 48 89 01 48 8D 05 ? ? ? ? 48 89 41 28 E8 ? ? ? ? 90 48 8B 8B B0 01 00 00",
        description: "FilterButton::~FilterButton. Signature: fn(this). Fires per filter button on filter-menu close; series_filter_scroll detours it to drop its tracked panel pointers before they dangle.",
    },
    SignatureDefinition {
        name: "filter_label_builder_count",
        pattern: "48 89 44 24 20 4C 8D 4D AF 4C 8D 45 8F BA 09 00 00 00 48 8D 4D",
        description: "Filter label builder — MOV [RSP+0x20]; LEA R9; LEA R8; MOV EDX,9; LEA RCX. Count byte at offset 13. LEA RCX [table_base] is 0x64 bytes before the count. Multiple instances exist (one per filter category).",
    },
    // Per-song version display name lookup. Two structurally distinct forms
    // emitted by the compiler across builds; whichever resolves is used.
    //
    // Both forms compute `string_table[ raw_series_u8 ]` via:
    //   MOVZX r32, byte ptr [thisreg + 0x138]  ; raw u8 at song_property+0x138
    //   LEA   rreg, [string_table]              ; RIP-relative
    //   MOV   rreg, [rreg + r*8]                ; OOB-prone for u8 >= 22
    //
    // The table at `string_table` only has 22 entries (one per vanilla series
    // value); custom series values >= 22 read past the end and the resulting
    // garbage pointer crashes sprintf_s in the "Version / %s" filter chip
    // builder. Hook target for series-expansion is the LEA's disp32: redirect
    // it to a 256-entry table the mod owns.
    //
    // Standalone form (newer builds): the lookup lives in its own 19-byte
    // function (slot 21 of the song-property accessor vtable). The LEA is
    // 7 bytes into the match.
    SignatureDefinition {
        name: "series_label_lookup_standalone",
        pattern: "0F B6 81 38 01 00 00 48 8D 0D ?? ?? ?? ?? 48 8B 04 C1 C3",
        description: "Standalone song-property accessor (vtable slot 21). MOVZX EAX,[RCX+0x138]; LEA RCX,[table]; MOV RAX,[RCX+RAX*8]; RET. LEA at match+7. disp32 at match+10.",
    },
    // Inlined form (older builds): the same lookup is inlined into the
    // version-label sprintf builder. The MOVZX target is R8D (REX.R prefix),
    // the LEA writes RAX, and the indexed MOV uses R8 (REX.RX). The LEA is
    // 8 bytes into the match.
    SignatureDefinition {
        name: "series_label_lookup_inlined",
        pattern: "44 0F B6 80 38 01 00 00 48 8D 05 ?? ?? ?? ?? 4E 8B 04 C0",
        description: "Inlined per-song version label lookup (older builds). MOVZX R8D,[RAX+0x138]; LEA RAX,[table]; MOV R8,[RAX+R8*8]. LEA at match+8. disp32 at match+11.",
    },
    // ── Flare-skill series classification (CalcFlareSkill) ─────────────
    // The inlined classification walk inside ddr::player::Record::
    // CalcFlareSkill that maps a song's RAW series byte (vtable+0xA0
    // accessor — NOT the mapped value from series_mapper) into a flare-skill
    // version category: >=18 GOLD(3), >=14 WHITE(2), >=1 CLASSIC(1), else 0.
    // The GOLD test has NO upper bound, so custom series (>= 22) count
    // toward GOLD. Both table operands are module-BASE-relative disp32s
    // (RVAs added to a LEA-materialized base register), not RIP-relative.
    //
    //   +0   CALL qword [RDX+slot]       ; raw <series> u8 -> AL (vtable slot
    //                                    ;  0xA0 on 20260324+, 0x88 on
    //                                    ;  20250805/20260224 — wildcarded)
    //   +6   MOVZX R8D,AL
    //   +10  XOR ECX,ECX                 ; walk index = 0
    //   +12  NOP dword [RAX+0]
    //   +16  MOV EDX,[RCX+R13+catRVA]    ; disp32 at +20 (category table)
    //   +24  CMP [RCX+R13+thrRVA],R8D    ; disp32 at +28 (threshold table)
    //   +32  JLE +0xC                    ; classified
    //   +34  SUB RCX,4
    //   +38  CMP RCX,-8                  ; imm8 loop bound at +41 (0xF8)
    //   +42  JGE loop
    //   +44  XOR EDX,EDX                 ; fallthrough -> category 0
    //
    // Wildcards cover the two data-layout-dependent disp32s and the raw-series
    // accessor's vtable slot (the only byte that differs on the pre-20260324
    // builds); register allocation and branch displacements verified
    // byte-identical on 20250805/20260224/20260324/20260616/20260721 (unique
    // match on all five). The stock tables are validated at init (ascending
    // thresholds, categories 1..=3), so a wrong-site match fails closed. Full
    // RE: docs/flare_ranking_research.md.
    SignatureDefinition {
        name: "flare_skill_classifier",
        pattern: "FF 92 ?? 00 00 00 44 0F B6 C0 33 C9 0F 1F 40 00 42 8B 94 29 ?? ?? ?? ?? 46 39 84 29 ?? ?? ?? ?? 7E 0C 48 83 E9 04 48 83 F9 F8 7D E4 33 D2",
        description: "CalcFlareSkill series->category walk. Cat-table disp32 at +20, threshold-table disp32 at +28, loop-bound imm8 at +41. series_expansion redirects both disp32s at a 4-entry extended table (adds 'series >= 22 -> category 0') and widens the bound -8 -> -12.",
    },
    // ── Folder Expansion signatures ─────────────────────────────────
    // Only folder_register and folder_has_songs are AOB-scanned.
    // All other folder functions are derived from folder_register xrefs
    // (see derive_folder_functions).
    SignatureDefinition {
        name: "afp_layer_init_wrapper",
        pattern: "48 89 5C 24 10 56 48 83 EC 40 41 8B F1 48 8B D9 48 85 D2",
        description: "gamemdx.dll wrapper around libafp stream lookup + afp_layer_create_with_property. Receives stream name in R8. Hook target for AFP redirect.",
    },
    SignatureDefinition {
        name: "folder_register",
        pattern: "40 55 48 8B EC 48 83 EC 60 48 C7 45 C0 FE FF FF FF 48 89 5C 24 78",
        description: "Pushes FolderProperty into folder list (older builds). Hook target for custom folder injection.",
    },
    // Newer builds (20260526+): compiler emits extra RSI/RDI saves before
    // the frame pointer setup, and saves RBX to [RSP+0x88] instead of [RSP+0x78].
    SignatureDefinition {
        name: "folder_register_v2",
        pattern: "40 55 56 57 48 8B EC 48 83 EC 60 48 C7 45 C0 FE FF FF FF 48 89 9C 24 88 00 00 00",
        description: "Pushes FolderProperty into folder list (20260526+ builds). Same function, different prologue.",
    },
    SignatureDefinition {
        name: "folder_has_songs",
        pattern: "48 8B 05 ? ? ? ? 48 63 51 08 48 8B 08 48 8B 05 ? ? ? ? 44 8B 41 04 41 83 F8 01",
        description: "Has-songs predicate — reads bit_index from functor+0x8, checks count array. Hook target.",
    },
    // ── Gameplay object allocation ───────────────────────────────────
    // The gameplay sequence object has a fixed-size shared_ptr array (one slot per
    // non-ALL_MUSIC folder). Custom folders overflow this. We find the allocation
    // to patch the size and the constructor to zero extra bytes.
    // Pattern: MOV ECX,<size>; CALL malloc; MOV [RBP+??],RAX; TEST RAX,RAX; JZ; MOV RCX,RAX; CALL ctor; JMP short
    // The trailing EB (JMP short) distinguishes this from the 0x400 alloc in the same function.
    SignatureDefinition {
        name: "gameplay_obj_alloc",
        pattern: "B9 ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 89 45 ?? 48 85 C0 74 ?? 48 8B C8 E8 ?? ?? ?? ?? EB",
        description: "Gameplay sequence object allocation — MOV ECX,<size>; CALL malloc; null check; CALL ctor; JMP. Size imm32 at +1, ctor CALL at +20.",
    },
    // ── Scene transition (advanceToScene) ─────────────────────────────
    // TS::advanceToScene — the vtable-dispatched function that calls
    // createNextSequence, installs the new gosub child, and writes
    // m_currentID. The TEST EDX,EDX; JNZ+7 shape (conditional
    // getNextID call) is structurally unique across the binary.
    SignatureDefinition {
        name: "advance_to_scene",
        pattern: "48 89 5C 24 08 57 48 83 EC 20 8B DA 48 8B F9 85 D2 75 07",
        description: "TS::advanceToScene. Prologue + TEST EDX,EDX; JNZ +7 (skip getNextID). Detour target for fixing m_currentID after scene redirects.",
    },
    // ── agcs::Sequence::finish ─────────────────────────────────────────
    // The engine's single scene-advance primitive. Called as
    // finish(this, nextSceneId_1INDEXED): sends message 0x201 to the parent
    // TransitionSequence — whose handler is advanceToScene (createNextSequence
    // → our scene hook, install gosub child, update m_currentID) — then flags
    // the calling subtree for destruction (flags |= 4). Frees nothing; the
    // reaper runs next frame, so calling it from the frame thread is safe and
    // the transition is synchronous. NOTE the 1-indexed scene id — the hook
    // DLL's scene tracking is 0-indexed everywhere else.
    SignatureDefinition {
        name: "sequence_finish",
        pattern: "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8B 59 08 48 8B F1 8B FA F6 43 20 20",
        description: "agcs::Sequence::finish(this, nextSceneId_1INDEXED) — sends msg 0x201 (advanceToScene on the TransitionSequence parent) then marks the subtree for destruction. Frees nothing; reaper runs next frame. Verified unique on 20260616 (0x18021DB90) and 20260721 (0x18021DF70). Consumed by the quick-logout mod and the quick-restart/fail fast paths.",
    },
    // ── GameWork session-state probe ─────────────────────────────────
    // The 36-byte tail of the final-stage-override test leaf (FUN_1801DD660
    // on 20260721; designed in docs/quick_logout_research.md §8.2). Matches
    // at function start + 7. Every GameWork session-state constant is in the
    // matched bytes (offsets are LITERAL in the pattern — a layout change
    // means no match, which fails the consumers closed):
    //
    //   -7   MOV RAX,[rip+d32]        ; d32 at -4 -> GameWork ptr-ptr global
    //   +0   MOV RCX,[RAX]
    //   +3   CMP qword [RCX+d8],0     ; d8 at +6  -> course field (0x70)
    //   +8   MOV EDX,[RCX+d8]         ; d8 at +10 -> stage counter (0x0C)
    //   +11  JNZ +0x17
    //   +13  MOV EAX,[RCX+d32]        ; d32 at +15 -> event-mode field (0xD0)
    //   +19  CMP EAX,1 / JE / CMP EAX,2 / JE
    //   +29  CMP EDX,[RCX+d8]         ; d8 at +31 -> final-stage override (0x10)
    //   +32  SETZ AL / RET
    //
    // Unique on 20250805 (0x1801c6e47), 20260616 (0x1801dd1b7) and 20260721
    // (0x1801dd667). stage_records decodes + cross-checks the constants; the
    // quick-fail fast path consumes them (session-continues predicate).
    SignatureDefinition {
        name: "final_stage_probe",
        pattern: "48 8B 08 48 83 79 70 00 8B 51 0C 75 17 8B 81 D0 00 00 00 83 F8 01 74 0C 83 F8 02 74 07 3B 51 10 0F 94 C0 C3",
        description: "GameWork session-state probe (final-stage override test leaf, match = entry+7). Yields the event-mode (+0xD0) and final-stage override (+0x10) offsets plus cross-checkable course/stage offsets and the GameWork global. Consumed by stage_records for the quick-fail fast path.",
    },
    // ── ShutterActor close-request wrapper ───────────────────────────
    // FUN_1800334f0 on 20260721: the whole-function pattern of the
    // shutter-close broadcast wrapper (`requestClose(kind)` — sends msg
    // 0x1007 with the kind to the ShutterActor singleton and its children).
    // Matches at function entry:
    //
    //   +0   MOV [RSP+8],ECX / PUSH RBX / SUB RSP,0x20
    //   +9   MOV RBX,[rip+d32]        ; d32 at +12 -> ShutterActor singleton
    //   +16  TEST RBX,RBX / JZ ...
    //   +21  TEST byte [RBX+0x20],0x20 / JNZ ...   ; tree-flags dispatch guard
    //   +27  MOV RAX,[RBX] / LEA R8,[RSP+0x30]
    //   +35  MOV EDX,0x1007           ; the message imm pins this wrapper
    //   +40  MOV RCX,RBX / CALL [RAX+0x18]         ; onMessage(this,0x1007,&kind)
    //
    // The 0x1007 imm disambiguates against the sibling 0x1008/kind-close
    // wrappers (a shorter tail-only pattern matched 2 sites per build).
    // Unique on 20250805 (0x1800337e0), 20260616 (0x180034020) and 20260721
    // (0x1800334f0). Consumed by derive_shutter_actor_global for the
    // quick-restart/fail bannerless fast path (the 0x100c stage-shutter
    // dismiss + the state gates around it).
    SignatureDefinition {
        name: "shutter_close_request",
        pattern: "89 4C 24 08 53 48 83 EC 20 48 8B 1D ?? ?? ?? ?? 48 85 DB 74 ?? F6 43 20 20 75 ?? 48 8B 03 4C 8D 44 24 30 BA 07 10 00 00 48 8B CB FF 50 18",
        description: "ShutterActor close-request wrapper (msg 0x1007 broadcast). MOV RBX,[rip+d32] at +9 yields the ShutterActor singleton global (derived as shutter_actor_global).",
    },
    // ── Gameplay-entry loader ctor mask imms ─────────────────────────
    // createNextSequence case 0x1c/0x34 (the pre-gameplay stage
    // LoadingSequence ctor args): MOV EDX,0x8000 (load mask) followed by
    // MOV R8D,0x32000 (unload mask). The load imm pins the site — the other
    // unload-0x32000 caller (the course loader, case 0x2c) loads 0xD000.
    // Unique on 20250805 (0x18002fabb), 20260616 (0x1800301a0) and 20260721
    // (0x18002fc0b).
    //
    // The unload imm32 at match+7 is byte-patched 0x32000 → 0x30000 by
    // quick-restart-or-fail (select-residency patch): stock gameplay entry
    // evicts the select-music packages (mask 0x2000), which is what made
    // every gameplay → song-select hop (the quick-fail fast path AND the
    // natural post-results return) spend ~5 s reloading them. Keeping them
    // resident makes the 0-idx 24 loader a residency no-op.
    SignatureDefinition {
        name: "gameplay_loader_masks",
        pattern: "BA 00 80 00 00 41 B8 00 20 03 00",
        description: "Stage-loader ctor args in createNextSequence case 0x1c (MOV EDX,0x8000 + MOV R8D,0x32000). Unload imm32 at +7 patched to 0x30000 to keep select-music packages resident through gameplay.",
    },

    // ── In-place song reset (services::song_reset) ───────────────────
    // RE record: .agents/planning/20260812-inplace-restart/research/run_state_re.md
    // (§6 audio, §6 broadcast shape, §9 messages). All five patterns
    // verified this session: unique on 20250805 / 20260616 / 20260721
    // except dps_timing_anchor_site (2 matches on 20250805, both decoding
    // to the SAME tick global — the derivation requires that agreement).
    //
    // Song play-by-bank wrapper (FUN_1801aa5c0 on 20260721): the whole
    // prologue through the profiling-marker guard and the tail-call setup
    // into the inner play routine. Distinctive: the XORPS XMM2 (pan = 0)
    // before forwarding, and the NOP after the profiling FF 15. Called as
    // (slot /*ECX*/, const char* bankName /*RDX*/) -> i32 handle (-1 =
    // fail). Slot 5 = the per-song bank registered by DPS onSetup.
    SignatureDefinition {
        name: "song_play_by_bank",
        pattern: "40 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 40 48 8B DA 8B F9 48 8D 05 ?? ?? ?? ?? 48 89 44 24 50 8B 0D ?? ?? ?? ?? 85 C9 7E 07 FF 15 ?? ?? ?? ?? 90 0F 57 D2 48 8B D3 8B CF E8",
        description: "Song play/prepare wrapper: (slot, bank_name) -> i32 cue handle, -1 on failure. DPS update state 4 calls (5, name) and stores the handle at DPS+0x128. Consumed by song_reset (stop → replay audio rewind).",
    },
    // Song stop-by-handle wrapper (FUN_1801aa7c0 on 20260721): guards
    // handle != -1, computes the manager slot ((handle+5)*0x20 + mgr) and
    // stops via the slot object's vtable. The (handle+5)*0x20 arithmetic
    // (LEA RAX,[RBX+5]; SHL RAX,5; ADD RAX,[rip]) is kept literal — a slot
    // layout change must break the match.
    SignatureDefinition {
        name: "song_stop_by_handle",
        pattern: "40 53 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 8B D9 48 8D 05 ?? ?? ?? ?? 48 89 44 24 48 8B 0D ?? ?? ?? ?? 85 C9 7E 0C FF 15 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? 83 FB FF 74 ?? 48 8D 43 05 48 C1 E0 05 48 03 05",
        description: "Song stop wrapper: (i32 cue handle). DPS state 8 and DPS::leave stop the song with it (handle from DPS+0x128). Consumed by song_reset Phase 1.",
    },
    // Song is-prepared probe (FUN_1801aa630 on 20260721): returns the
    // per-handle prepared byte (*(mgr + handle*0x20 + 0xB0)). The 0x20
    // stride SHL and the literal 0xB0 displacement pin the layout.
    SignatureDefinition {
        name: "song_is_prepared",
        pattern: "40 53 48 83 EC 20 8B D9 8B 0D ?? ?? ?? ?? 85 C9 7E 0C FF 15 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 8B D3 48 C1 E2 05 0F B6 9C 02 B0 00 00 00",
        description: "Song prepared probe: (i32 cue handle) -> bool. DPS state 5 gates song start on it. Consumed by song_reset Phase 2 (poll before re-anchoring).",
    },
    // Optional audio diagnostics. Static 20260825 RE: start manager entry
    // RVA 0x1AB1C0 (wrapper 0x1AA120, called by DPS on 0x1044). This is a
    // REQUEST, not the XACT wave start or audible presentation boundary.
    SignatureDefinition {
        name: "audio_start_prepared",
        pattern: "83 FA FF 74 ?? 53 48 83 EC 20 8B D2 48 8B D9 48 8D 42 05 48 C1 E0 05 48 03 C1 74 ?? 48 C1 E2 05 80 BC 0A B0 00 00 00 00 75 ?? C6 40 11 01 48 8B 09 48 8B 01",
        description: "Optional start-prepared manager entry: void(manager*, i32 handle). Pending-start flag when unprepared, otherwise cue Play then engine DoWork; audio_sync_diag observes only.",
    },
    SignatureDefinition {
        name: "audio_sync_offset_layout",
        pattern: "48 8D 97 6C 01 00 00 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8D 97 70 01 00 00 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8D 97 84 01 00 00 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8D 97 88 01 00 00 48 8D 0D ?? ?? ?? ?? E8",
        description: "Optional GPA timing-field attestation: ctor subscribes SOUND/INPUT/RENDER/BOMB fields at +16C/+170/+184/+188 (20260825 RVA 5B756). Diagnostic validates the four key strings before reading fields.",
    },
    SignatureDefinition {
        name: "audio_sync_raw_count_store",
        pattern: "FF 87 58 01 00 00 44 89 B7 78 01 00 00",
        description: "Optional GPA onUpdate tail: frame counter increment then stored raw count +178 (20260825 RVA 5D8B8). AFTER judgeNotes, hence sampled separately from its current argument.",
    },
    SignatureDefinition {
        name: "audio_sync_offset_layout_v1",
        pattern: "49 8D 94 24 6C 01 00 00 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 49 8D 94 24 70 01 00 00 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 49 8D 94 24 84 01 00 00 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 49 8D 94 24 88 01 00 00 48 8D 0D ?? ?? ?? ?? E8",
        description: "20250805 GPA timing-field attestation, same offsets/keys as audio_sync_offset_layout but R12 base (RVA 583BF); 20-byte cells rather than 19.",
    },
    // Recursive actor-subtree message broadcast (FUN_18022eaa0 on
    // 20260721): broadcast(actor, msg, param, depth) — checks the
    // dispatch-suppressed flag (+0x20 & 0x20), calls the actor's
    // onMessage (vt+0x18), and recurses over first-child/next-sibling.
    // This is the engine's own delivery primitive for every 0x10xx
    // message; DPS states 5/6 use exactly this to send 0x1043/0x1044.
    SignatureDefinition {
        name: "update_broadcast",
        pattern: "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 20 F6 41 20 20 41 8B E9 49 8B F8 8B F2 48 8B D9 75 ?? 48 8B 01 FF 50 18",
        description: "agcs actor-subtree message broadcast (actor, msg, param, 0). Flag-guarded onMessage + child recursion — the engine's own delivery for the 0x1043/0x1044 timing protocol. Consumed by song_reset Phase 2.",
    },
    // DPS update state 6 — the timing-anchor broadcast site
    // (0x180058a91 on 20260721):
    //
    //   +0   MOV RAX,[rip+d32]        ; d32 at +3 -> frame-clock global
    //   +7   MOV RCX,[RAX+0x1268]     ; current frame tick (ms domain)
    //   +14  MOV [RSP+0x58],RCX       ; the 0x1044 payload
    //   +19  TEST byte [RSI+0x20],0x20
    //   +23  JNZ ...
    //   +25  MOV RAX,[RSI] / LEA R8,[RSP+0x58]
    //   +33  MOV EDX,0x1044           ; the timing-anchor message imm
    //
    // The 0x1268 displacement + the 0x1044 imm pin the site. Two matches
    // on 20250805 (a second state machine shares the shape) — both decode
    // to the same global, which derive_frame_tick_global REQUIRES.
    SignatureDefinition {
        name: "dps_timing_anchor_site",
        pattern: "48 8B 05 ?? ?? ?? ?? 48 8B 88 68 12 00 00 48 89 4C 24 58 F6 46 20 20 75 ?? 48 8B 06 4C 8D 44 24 58 BA 44 10 00 00",
        description: "DPS state-6 timing-anchor read+broadcast site. RIP disp32 at +3 yields frame_tick_global (current tick at +0x1268 — the exact clock the 0x1044 anchor must carry). Consumed via derive_frame_tick_global by song_reset.",
    },

    // The input-manager's per-frame tick function (`FUN_1800231F0` on
    // 20260825; the LAST call of the per-frame input poll). Its final two
    // instructions are `CALL [rip+Ordinal_45]` (libavs `XCnbrep700002c`,
    // the game tick) and `MOV [RBP+0x1268],RAX` — the ONE place the frame
    // tick `T` every gameplay clock reads is stored. Matched by its
    // prologue + the `MOV RBP,[rip+d32]` load of the input-state global
    // (d32 at +0x16 — `derive_input_tick_function` REQUIRES it to decode
    // to `frame_tick_global`) and the first two field stores; the tail
    // store is verified by the derivation as well. Consumed by
    // services/audio_clock (post-original detour pairing `T` with QPC).
    SignatureDefinition {
        name: "input_tick_function",
        pattern: "48 8B C4 53 55 56 57 41 54 41 55 41 56 41 57 48 83 EC 28 48 8B 2D ?? ?? ?? ?? 48 8D 50 08 48 8D 48 18 33 FF C7 40 08 01 00 00 00 89 78 18 FF 15",
        description: "Input-manager per-frame tick function (20260224+): prologue + `MOV RBP,[rip+frame_tick_global]` (d32 at +0x16); ends `CALL [Ordinal_45]; MOV [RBP+0x1268],RAX; RET`. derive_input_tick_function verifies both and publishes `input_tick_store` (the tail store site). Consumed by services/audio_clock (T,QPC pairing).",
    },
    SignatureDefinition {
        name: "input_tick_function_v1",
        pattern: "40 53 55 56 57 41 54 41 55 41 56 48 83 EC 20 4C 8B 25 ?? ?? ?? ?? 48 8D 54 24 60 48 8D 4C 24 70 33 FF C7 44 24 60 01 00 00 00 89 7C 24 70 FF 15",
        description: "20250805 shape of input_tick_function: `MOV R12,[rip+frame_tick_global]` (d32 at +0x12); tail `CALL [Ordinal_45]; MOV [R12+0x1268],RAX; ADD RSP,0x20; POP R14`. derive_input_tick_function publishes whichever shape matched as `input_tick_function`.",
    },

    // GamePlayActor's msg-0x1044 rewind worker (`FUN_18005bac0` on
    // 20260721) — the training-mode seek's rebuild-trio anchor. The two
    // stores right after its step gate are unmistakable:
    //
    //   +0   MOV [RCX+0x160],RDX             ; the timing anchor
    //   +7   MOV dword [RCX+0x190],0xFFFFFFFF
    //
    // The +0x160/+0x190 displacements + the -1 imm make the pair unique;
    // the whole region through the three rebuild calls is byte-identical
    // on 20260616/20260721 (only a rip-disp32 inside differs). From the
    // match, the first three CALL rel32 sites are the judge-record trio —
    // clear(vec@actor+0xB0) / reserve(vec, count) / rebuild(out, begin,
    // end, &{actor, playhead}) — the flash-renderer virtual call between
    // is `FF 50 10`, never E8. Consumed by derive_judge_rebuild_trio.
    SignatureDefinition {
        name: "judge_rebuild_anchor",
        pattern: "48 89 91 60 01 00 00 C7 81 90 01 00 00 FF FF FF FF",
        description: "msg-0x1044 rewind worker's anchor stores (MOV [this+0x160],tick + MOV [this+0x190],-1). The first three CALL rel32s after the match are the judge-record rebuild trio (clear/reserve/rebuild), derived as judge_rebuild_clear/reserve/rebuild for seek-to-T.",
    },

    // FlareGaugeActor ctor field-init tail (`FUN_180075490` on 20260721,
    // right after the base GaugeActor ctor CALL) — a pure LAYOUT
    // ATTESTATION for the in-place reset's flare-state restore. Every
    // flare-specific offset the restore touches is pinned as a literal
    // disp32 in this run of stores:
    //
    //   33 C0                      XOR  EAX,EAX
    //   89 9F E8000000             MOV  [RDI+0xE8],EBX      ; side
    //   48 8B 5C 24 ??             MOV  RBX,[RSP+disp]
    //   89 87 E4000000             MOV  [RDI+0xE4],EAX      ; good-judge streak
    //   48 89 87 EC000000          MOV  [RDI+0xEC],RAX      ; per-grade judge
    //   48 89 87 F4000000          MOV  [RDI+0xF4],RAX      ;   history counters
    //   48 89 87 FC000000          MOV  [RDI+0xFC],RAX      ;   (8 dwords,
    //   48 89 87 04010000          MOV  [RDI+0x104],RAX     ;   0xEC..0x108)
    //   48 B8 1027000010270000     MOV  RAX,0x2710_00002710
    //   C6 87 E0000000 00          MOV  byte [RDI+0xE0],0   ; course-carry flag
    //   4C 8D 1D ????????          LEA  R11,[vftable]       ; rip disp32
    //   4C 89 1F                   MOV  [RDI],R11
    //   48 89 87 0C010000          MOV  [RDI+0x10C],RAX     ; per-level gauge
    //                                                       ;   array head
    //                                                       ;   (11 dwords of
    //                                                       ;   10000)
    //
    // NOTE the class-name/ctor swap vs the run_state_re.md §5 table: the
    // REAL FlareGaugeActor is the 0x138-byte class built for gauge
    // options 1..0xB (1 = FLOATING, 2..10 = FLARE I..IX, 0xB = FLARE EX);
    // option 0xE builds the 0xE8-byte GradeGaugeActor. The CURRENT flare
    // level does NOT live on the actor — it is ddr::player::Option+0x7C
    // (setter vt+0x1A0 / getter vt+0x310, plain field accessors), reached
    // via the derived player_option_table. Verified to match exactly once
    // on 20260324 / 20260421 / 20260526 / 20260616 / 20260721; MISSES on
    // 20250805 by design (older layout, no course-carry fields — 2025
    // builds are unsupported). Fail-open: unresolved ⇒ song_reset refuses
    // in-place resets whenever a FlareGaugeActor is live, and the caller's
    // scene-jump fallback (which re-runs onSetup) restores flare state
    // the slow way.
    SignatureDefinition {
        name: "flare_gauge_ctor_layout",
        pattern: "33 C0 89 9F E8 00 00 00 48 8B 5C 24 ?? 89 87 E4 00 00 00 48 89 87 EC 00 00 00 48 89 87 F4 00 00 00 48 89 87 FC 00 00 00 48 89 87 04 01 00 00 48 B8 10 27 00 00 10 27 00 00 C6 87 E0 00 00 00 00 4C 8D 1D ?? ?? ?? ?? 4C 89 1F 48 89 87 0C 01 00 00",
        description: "FlareGaugeActor ctor field-init tail — layout attestation for song_reset's floating-flare restore (streak +0xE4, per-grade history counters +0xEC..+0x108, per-level array +0x10C..+0x134, side +0xE8). Presence attests the 2026 flare layout; the address itself is unused.",
    },
    // The OLDER FlareGaugeActor layout (20250805 @ 0x18007212E, 20260224 @
    // 0x18007129E — unique on both, absent on 20260324+): no course-carry
    // fields at all. Ctor tail:
    //
    //   48 8D 05 ????????          LEA  RAX,[vftable]
    //   48 89 07                   MOV  [RDI],RAX
    //   89 9F E0000000             MOV  [RDI+0xE0],EBX      ; side
    //   33 C0                      XOR  EAX,EAX
    //   48 89 87 E4000000          MOV  [RDI+0xE4],RAX      ; per-grade judge
    //   48 89 87 EC000000          MOV  [RDI+0xEC],RAX      ;   history counters
    //   48 89 87 F4000000          MOV  [RDI+0xF4],RAX      ;   (8 dwords,
    //   48 89 87 FC000000          MOV  [RDI+0xFC],RAX      ;   0xE4..0x100)
    //
    // calcJudgePoint on these builds bumps `[this+0xE4+grade*4]` and
    // demotes via Option vt+0x1A0 exactly like the 2026 code, so the
    // floating-flare restore only needs to zero those 8 counters (no
    // streak, no per-level array). song_reset selects the layout by which
    // attestation matched.
    SignatureDefinition {
        name: "flare_gauge_ctor_layout_v1",
        pattern: "48 8D 05 ?? ?? ?? ?? 48 89 07 89 9F E0 00 00 00 33 C0 48 89 87 E4 00 00 00 48 89 87 EC 00 00 00 48 89 87 F4 00 00 00 48 89 87 FC 00 00 00",
        description: "FlareGaugeActor ctor field-init tail, pre-20260324 layout (side +0xE0, per-grade history counters +0xE4..+0x100, no streak / per-level array). Layout attestation for song_reset's floating-flare restore on 20250805 / 20260224.",
    },

    // GradeGaugeActor ctor field-init tail (`FUN_180075270` on 20260721,
    // option 0xE) — layout attestation for song_reset's grade-watermark
    // reset, same shape as flare_gauge_ctor_layout:
    //
    //   4C 8D 1D ????????                LEA  R11,[vftable]     ; rip disp32
    //   C7 83 E0000000 00 00 00 80       MOV  dword [RBX+0xE0],0x80000000
    //   4C 89 1B                         MOV  [RBX],R11
    //
    // +0xE0 is the best-EX-score watermark (ctor INT_MIN sentinel): the
    // grade calcJudgePoint (FUN_180075360) multiplies the miss penalty
    // while the current EX score has not grown past it, and clamps /
    // rewrites it on every judge. It survives an in-place reset (EX
    // score restarts at 0, watermark keeps the pre-reset value) —
    // early misses on the restarted run get over-penalized until the
    // first good judge rewrites it. The reset writes the ctor sentinel
    // back. Verified to match exactly once on 20260324 / 20260421 /
    // 20260526 / 20260616 / 20260721; fail-open like the flare AOB
    // (unresolved ⇒ resets refuse while a GRADE gauge is live).
    SignatureDefinition {
        name: "grade_gauge_ctor_layout",
        pattern: "4C 8D 1D ?? ?? ?? ?? C7 83 E0 00 00 00 00 00 00 80 4C 89 1B",
        description: "GradeGaugeActor ctor field-init tail (vftable store + best-EX watermark seed [this+0xE0] = 0x80000000) — layout attestation for song_reset's grade-watermark reset. Presence attests the offset + sentinel; the address itself is unused.",
    },

    // ── CRT functions ───────────────────────────────────────────────
    // MSVC's operator new — statically linked CRT, identical bytes across game versions.
    // Complete function from prologue to RET: retry loop calling _malloc_base then _callnewh.
    SignatureDefinition {
        name: "game_malloc",
        pattern: "53 48 83 EC 40 48 8B D9 EB ?? 48 8B CB E8 ?? ?? ?? ?? 85 C0 74 ?? 48 8B CB E8 ?? ?? ?? ?? 48 85 C0 74 ?? 48 83 C4 40 5B C3",
        description: "MSVC operator new (CRT malloc). Takes size in RCX, returns pointer. Uses HeapAlloc on the game's CRT heap.",
    },
    // ── AGCS heap allocator (app heap — NOT CRT) ─────────────────────
    // Used by the game's allocator-aware STL containers and any agcs::*::new
    // allocation. Distinct from game_malloc (CRT). Memory allocated here must
    // be freed via agcs_heap_free — mixing allocators causes heap mismatch
    // crashes.
    //
    // Byte 30 is wildcarded: MSVC emits either `4C 8B D8` (REX.R MOV RBX,R8)
    // or `49 8B D8` (REX.B MOV RBX,R8) depending on toolchain version — same
    // instruction, different encoding. The wildcard covers both.
    SignatureDefinition {
        name: "agcs_heap_malloc",
        pattern: "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 54 48 83 EC 20 48 8B 01 ?? 8B D8 48 8B F2 48 8B F9 FF 50 20 4C 8B 1F",
        description: "AGCS heap allocator entry point. Signature: fn(heap_handle, size, align, _unused) -> *mut u8. Prepends a 0x20-byte tracking header; pair with agcs_heap_free.",
    },
    // Two bytes wildcarded: the function reads the tracking header at [ptr-0x18]
    // and the heap-object vtable at [ptr]. Depending on toolchain version the
    // compiler keeps ptr in RCX (48 8B 59 E8, 48 8B 07) or moves RCX → RDI first
    // (48 8B 5F E8, 48 8B 01). Same behavior either way; the wildcards cover
    // both.
    SignatureDefinition {
        name: "agcs_heap_free",
        pattern: "48 83 EC 28 48 85 C9 74 ?? 48 89 5C 24 30 48 8B ?? E8 48 89 7C 24 20 48 8B 79 E0 48 8B CF 48 8B ?? FF 50 20",
        description: "AGCS heap free. Signature: fn(ptr) — reads tracking header at ptr-0x18/ptr-0x20 to locate heap. Pair with agcs_heap_malloc.",
    },
    // ── App-heap-allocated std::vector<T>::reserve (stride 12) anchor ────
    // Used only as a landmark to derive app_heap_handle (via MOV RCX,[RIP+disp32]
    // at +0x7B) and cross-check agcs_heap_malloc (via CALL at +0x82). The 12-byte
    // stride is identified by the 0x1555555555555555 = SIZE_MAX/12 overflow check
    // constant, which is unique to 12-byte-element reserve functions.
    SignatureDefinition {
        name: "app_heap_reserve_anchor",
        pattern: "41 54 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 40 48 89 74 24 50 48 89 7C 24 58 4C 8B C2 48 8B D9 48 B8 55 55 55 55 55 55 55 15",
        description: "std::vector<T>::reserve for 12-byte-stride T (Measure etc). Landmark for deriving app_heap_handle + agcs_heap_malloc.",
    },
    // ── step::IStepReader::Analyze (post-parse hook point) ─────────────
    // Non-virtual member function on step::IStepReader (concrete runtime
    // type SsqReader per RTTI).
    //   RCX = this (reader pointer)
    //   RDX = per-note-record vector pointer
    //   R8  = per-measure-record vector pointer
    //   R9  = optional result-struct pointer (may be null)
    //   [RSP+0x28] = groove-radar struct pointer
    //   [RSP+0x30] = mode (i32)
    //   [RSP+0x38] = difficulty (i32)
    // Reader member layout: data-blob pointer at +0x08, blob size at +0x10.
    SignatureDefinition {
        name: "step_reader_analyze",
        pattern: "4C 89 4C 24 20 55 53 56 57 41 54 41 55 41 56 48 8D 6C 24 F9 48 81 EC F0 00 00 00",
        description: "step::IStepReader::Analyze (public non-virtual) prologue. Hook point for post-parse mine injection.",
    },
    // GamePlayActor::judgeNotes submits a judgment result for a single
    // note via this helper. Called with:
    //   RCX = GamePlayActor*
    //   RDX = per-active-note result-record pointer
    //   R8D = judge code
    //   R9  = scratch pointer
    // Judge codes observed from the call sites inside judgeNotes:
    //   0x1028+grade  -- normal grade judgment (grade 0..5)
    //   0x102d        -- MISS
    //   0x1030        -- shock MISS (player stepped on shock)
    //   0x1031        -- shock NG (the result's grade dword at +0xC is
    //                    pre-set to 7 before the call)
    //   0x1046        -- cancel / reset
    // Mines reuse the shock-NG code path (0x1031 + grade=7) which already
    // drives combo break, NG display, and life gauge damage via event
    // dispatch. Prologue is distinctive: 4-push frame, stack cookie load,
    // then MOVZX [RCX+0x1e8] — the judgment-suppression byte on
    // GamePlayActor.
    SignatureDefinition {
        name: "judge_submit",
        pattern: "55 57 41 55 41 56 48 8B EC 48 83 EC 78 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 D0 48 8B 02 48 89 74 24 68 48 8B F9 80 38 02 0F B6 89 E8 01 00 00",
        description: "Judgment submitter. Takes (actor, result, judge_code, scratch); dispatches score/gauge/display updates.",
    },
    // ComboActor digit-refresh (s-marvelous combo tint/digits). Prologue
    // anchored; the inline tint-immediates run (marvelous pair
    // 0xA9FEEC/0xDFA6EF written to locals) pins uniqueness. RIP disp of the
    // security-cookie load wildcarded. Event-driven (init + combo-changed
    // msg with combo >= 4) — never per-frame.
    SignatureDefinition {
        name: "combo_digit_refresh",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D 68 98 48 81 EC 40 01 00 00 48 C7 44 24 50 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 0F 29 70 C8 0F 29 78 B8 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 18 48 89 4C 24 38 C7 45 F8 EC FE A9 00 C7 45 FC EF A6 DF 00",
        description: "sequence::dance::ComboActor digit refresh (this in RCX). Repaints digit art per layer/place + applies the per-grade tint pairs.",
    },
    // NoteResultActor msg handler (`FUN_18007B300` on 20260721), grade case
    // 0x1028..0x102D: the FAST/SLOW indicator's show/hide gate. After the
    // judgement word is driven, the `dance_fast_slow` clip (this+0xA8) is
    // HIDDEN when either the ms delta (this+0x98) is 0 or the grade
    // (this+0x94) is 0 = Marvelous; otherwise shown at in_fast/in_slow:
    //
    //   83 BF 98 00 00 00 00   CMP dword [RDI+0x98], 0   ; delta == 0 → hide
    //   74 ??                  JZ  hide
    //   83 BF 94 00 00 00 00   CMP dword [RDI+0x94], 0   ; grade == 0 → hide
    //   74 ??                  JZ  hide
    //
    // The s-marvelous mod rewrites the second CMP's imm8 (match+15, `00` →
    // `FF`): grade is 0..=5 in this branch so `grade == -1` never holds and
    // the JZ is never taken — Marvelous judgements show FAST/SLOW like every
    // other grade (the delta==0 hide stays — an exactly-on-time step is
    // neither). Single-byte store, no tearing. Both JZ rel8s wildcarded
    // (7B/72 on 20250805/20260616/20260721/20260825, but structurally
    // free). Unique single match, byte-identical on all four.
    SignatureDefinition {
        name: "note_result_fast_slow_gate",
        pattern: "83 BF 98 00 00 00 00 74 ?? 83 BF 94 00 00 00 00 74 ??",
        description: "NoteResultActor grade-case FAST/SLOW gate — CMP [RDI+0x98],0; JZ; CMP [RDI+0x94],0; JZ. S-Marvelous rewrites the grade CMP imm8 at match+15 (0→-1) so Marvelous shows FAST/SLOW.",
    },
    // Song-select header-card refresh (s-marvelous S-MFC lamp). Prologue
    // (`MOV RAX,RSP; PUSH RBP/R12-R15; LEA RBP,[RAX-frame]; SUB RSP,size`)
    // + the body head through `CMP [RCX+0xD0],Rn` (the card's layer-object
    // field the lamp block draws into). Wildcarded: frame constants, the
    // security-cookie RIP + stack slot, the `-2` marker slot, and the callee-
    // saved register the compiler picked for `this` / the zero register
    // (R15/R12 on 20260324+, R12/R13 on 20250805/20260224), and the `DL` spill slot (0x30 on 20250805, 0x28 later).
    // fn(this, flag: u8); the lamp block near the end formats
    // `fullcombo_%dp_usr` / `muca_card_%s` from a clearkind-indexed table.
    SignatureDefinition {
        name: "selectmusic_card_refresh",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 ?? ?? ?? ?? 48 81 EC ?? ?? ?? ?? 48 C7 45 ?? FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 0F 29 70 C8 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 85 ?? ?? ?? ?? 0F B6 F2 88 54 24 ?? 4C 8B ?? 48 89 4C 24 ?? 45 33 ?? 44 89 ?? 24 20 4C 39 ?? D0 00 00 00",
        description: "Song-select header-card refresh (FUN_18015a450 on 20260825): fn(this, flag). Post-original detour target for the S-MFC lamp swap — layer object at this+0xD0, widget `fullcombo_%dp_usr`, texture `muca_card_%s` from the clearkind table.",
    },
    // Song-select DifficultyPanel::Reflesh (s-marvelous S-MFC lamp, the
    // per-difficulty-row CLEAR RANK lamps). Prologue (six XMM saves — the
    // discriminator against the header-card refresh's single XMM6 save) +
    // the two early-out field tests `CMP [RCX+0xD0],Rn; JZ; CMP [RCX+0xC0],Rn`
    // (music-info holder / layer object). Frame constants, cookie RIP/slot,
    // the `-2` marker slot, register choices and the spill slot wildcarded.
    // fn(this); rows widget `difficulty%dp_usr/dif%02d_usr`, lamp child
    // `fc_usr`, texture `muca_dif_%s` from the clearkind table.
    SignatureDefinition {
        name: "selectmusic_difficulty_panel_refresh",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 ?? ?? ?? ?? 48 81 EC ?? ?? ?? ?? 48 C7 85 ?? ?? ?? ?? FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 0F 29 70 C8 0F 29 78 B8 44 0F 29 40 A8 44 0F 29 48 98 44 0F 29 50 88 44 0F 29 98 78 FF FF FF 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 85 ?? ?? ?? ?? 4C 8B ?? 45 33 ?? 41 8B ?? 44 89 ?? 24 ?? 48 39 ?? D0 00 00 00 0F 84 ?? ?? ?? ?? 48 39 ?? C0 00 00 00",
        description: "sequence::selectmusic::DifficultyPanel::Reflesh (FUN_180115ea0 on 20260825): fn(this). Post-original detour target for the S-MFC lamp swap on the CLEAR RANK column — layer object at this+0xC0, rows vector<int> at this+0x1B8..0x1C0, widget `difficulty%dp_usr/dif%02d_usr/fc_usr`, texture `muca_dif_%s`.",
    },
    // Same function, 20250805 / 20260224 shape: the `-2` marker store uses
    // the disp8 ModRM form (`48 C7 45 xx`) and `this` is ALSO spilled to
    // `[RSP+x]` before the zero-register setup.
    SignatureDefinition {
        name: "selectmusic_difficulty_panel_refresh_v1",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 ?? ?? ?? ?? 48 81 EC ?? ?? ?? ?? 48 C7 45 ?? FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 0F 29 70 C8 0F 29 78 B8 44 0F 29 40 A8 44 0F 29 48 98 44 0F 29 50 88 44 0F 29 98 78 FF FF FF 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 85 ?? ?? ?? ?? 4C 8B ?? 48 89 4C 24 ?? 45 33 ?? 41 8B ?? 44 89 ?? 24 ?? 48 39 ?? D0 00 00 00 0F 84 ?? ?? ?? ?? 48 39 ?? C0 00 00 00",
        description: "sequence::selectmusic::DifficultyPanel::Reflesh on 20250805/20260224 (FUN_180109750 / FUN_18010c190). Same layout (+0xC0 layer, +0xD0 info holder, +0x1B8 rows).",
    },
    // Song-select RecordPanel::Refresh (s-marvelous S-MFC lamp — the
    // always-visible side-info DIFFICULTY / LEVEL / BEST SCORE / CLEAR RANK
    // table, `side_%dp_usr/info_%dp_usr/item_%02d_usr/fc_usr`, textures
    // `musi_dif_%s`). Prologue + the body head through the two field tests
    // `CMP [RCX+0x118],RSI; JZ` (layer) and the side/song loads
    // `MOV EBX,[RCX+0x140]` … `CMP [RCX+0x148],RSI` (side, highlighted
    // song). Frame constants, cookie RIP/slot, JZ rel32 and the model-global
    // RIP wildcarded; register allocation is identical on all four builds.
    SignatureDefinition {
        name: "selectmusic_record_panel_refresh",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 ?? ?? ?? ?? 48 81 EC ?? ?? ?? ?? 48 C7 45 ?? FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 85 ?? ?? ?? ?? 4C 8B F1 33 F6 44 8B EE 89 74 24 30 48 39 B1 18 01 00 00 0F 84 ?? ?? ?? ?? 4C 8B 25 ?? ?? ?? ?? 4C 89 64 24 50 8B 99 40 01 00 00 89 5C 24 34 44 8D 7B 01 48 39 B1 48 01 00 00 74 29",
        description: "sequence::selectmusic::RecordPanel::Refresh (FUN_18019b9f0 on 20260825): fn(this). Post-original detour target for the S-MFC lamp swap on the side-info table — layer at this+0x118 (id at layer+0x08), side at this+0x140, rows = difficulties 0..4 directly (`item_%02d` = row+1), texture `musi_dif_%s`.",
    },
    // FullcomboActor::onMessage (s-marvelous FC splash). Prologue-anchored;
    // the `CMP EDX,0x1034` (its only handled message) pins uniqueness —
    // `81 FA 34 10 00 00` is module-unique on 20260721.
    SignatureDefinition {
        name: "fullcombo_actor_on_message",
        pattern: "40 55 56 57 48 83 EC 60 48 C7 44 24 30 FE FF FF FF 48 89 9C 24 80 00 00 00 0F 29 74 24 50 0F 29 7C 24 40 49 8B F0 48 8B D9 81 FA 34 10 00 00 0F 85",
        description: "sequence::dance::FullcomboActor message handler (this, msg, payload). Handles only 0x1034: SE + splash label goto + play/visible.",
    },
    // PlaydataTab populate/update (s-marvelous results score tab, vslot 7 —
    // runs every frame while a judgement-count tab is visible; the heavy
    // populate is gated on the dirty byte this+0x151, consumed at its
    // start). Prologue-anchored; the giant frame (SUB RSP,0xB70) plus the
    // 0x151/0x110 field reads pin uniqueness — the string
    // "marvelous_num_usr" has exactly one xref, inside this fn
    // (0x1800F6BC0 on 20260721). Security-cookie RIP disp wildcarded.
    SignatureDefinition {
        name: "playdata_tab_update",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 68 F5 FF FF 48 81 EC 70 0B 00 00 48 C7 45 50 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 48 8B 05 ? ? ? ? 48 33 C4 48 89 85 60 0A 00 00 4C 8B E1 4C 8B 81 10 01 00 00",
        description: "sequence::result::PlaydataTab populate/update (this). Populates the judgement-count rows behind the +0x151 dirty byte, then lays out every widget in this+0x158 per frame.",
    },
    // PlaydataTab row-write helper (s-marvelous results score tab): builds a
    // sequence::SpriteLayer number widget — make_shared, glyph conversion
    // via "scre_tab_num_%s", parent = ctx wrapper, anchor-name assign,
    // set-names, then PUSHES it into the tab's widget vector (tab+0x158) so
    // the game owns layout + destruction. (ctx, out_shared_ptr, name_str,
    // text_str) -> out_shared_ptr; ctx = {wrapper*, tab*} pair
    // (0x1800F8370 on 20260721). Prologue-anchored, unique on the frame
    // size + homing sequence.
    SignatureDefinition {
        name: "playdata_row_write",
        pattern: "40 53 55 56 57 41 54 41 55 48 81 EC 98 00 00 00 48 C7 44 24 50 FE FF FF FF 48 8B 05 ? ? ? ? 48 33 C4 48 89 84 24 80 00 00 00 49 8B D9 49 8B F0 48 8B EA 4C 8B E1 48 89 54 24 48 45 33 ED 44 89 6C 24 20",
        description: "PlaydataTab row-write helper (ctx {wrapper,tab}, out shared_ptr, anchor-name string, text string). Creates a SpriteLayer row widget and pushes it into tab+0x158.",
    },
    // Same helper, pre-20260324 codegen (20250805 @ 0x1800EBA00, 20260224 @
    // 0x1800EDC00 — byte-identical prologue, unique on both, absent on
    // 20260324+): RBP-framed with a 0xD0 frame and RSI/RDI/R14 saves, and
    // the parent+anchor-name write goes through a small setter
    // (`sl+0x60 = wrapper; sl+0x68 = name`) instead of inline stores. Same
    // ABI (RCX=ctx {wrapper,tab}, RDX=out shared_ptr, R8=anchor name,
    // R9=text), same SpriteLayer field writes (+0x94/+0x9C/+0xA0/+0xD8/
    // +0xE0), same `tab+0x158..0x168` push — Ghidra-verified on both builds.
    // `derive_smarvelous_results_fallbacks` publishes it under the primary
    // name so the results_score consumer stays build-agnostic.
    SignatureDefinition {
        name: "playdata_row_write_v1",
        pattern: "40 55 53 56 57 41 54 41 55 41 56 48 8D 6C 24 D9 48 81 EC D0 00 00 00 48 C7 45 EF FE FF FF FF 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 1F 49 8B D9 49 8B F8 4C 8B E2 48 8B F1 48 89 55 BF 45 33 F6",
        description: "PlaydataTab row-write helper, pre-20260324 prologue (20250805 / 20260224). Same ABI and layout as playdata_row_write.",
    },
    // GraphTab per-frame rebuild (s-marvelous judgement graph, vslot 7 —
    // clears + rebuilds all charts/legend texts every frame). Prologue
    // anchored; the giant 0x12E0 chkstk frame pins uniqueness. The chkstk
    // call rel32 AND the EH-marker slot displacement are wildcarded (the
    // slot is [RBP+0x7E8] on 20260324+ but [RBP+0x890] on 20250805 /
    // 20260224). Verified exactly-once on 20250805 @0x1800E1520, 20260224
    // @0x1800E32D0, 20260324 @0x1800EE240, 20260616 @0x1800ED1B0, 20260721
    // @0x1800ED610, 20260825 @0x1800ED9F0; the GraphTab layout the detours
    // consume (+0x110/+0x120 wrappers, +0x138 page, +0x148/+0x14C side/
    // stage, +0x1C4 has_data, series vectors +0x538/+0x5D8/+0x5F8, legend
    // ctx {rect*,cursor*,tab*}, ColorCallable {vft,rgba,..,impl}) is
    // decompile-identical on the two old builds.
    SignatureDefinition {
        name: "graph_tab_rebuild",
        pattern: "40 55 41 54 41 55 41 56 41 57 48 8D AC 24 20 EE FF FF B8 E0 12 00 00 E8 ? ? ? ? 48 2B E0 48 C7 85 ? ? 00 00 FE FF FF FF",
        description: "sequence::result::GraphTab rebuild (this). Rebuilds charts (tab+0x178) and legend texts (tab+0x1A0) per frame while the tab is visible.",
    },
    // Chart single-color series append (s-marvelous judgement graph):
    // (chart, vector<double>*, callable {vft, rgba u32, pad, impl_ptr}) —
    // DEEP-COPIES the data, CONSUMES the callable. Unique on 20260721
    // @0x1801CFF60 / 20260616 @0x1801CF410; cookie disp wildcarded.
    SignatureDefinition {
        name: "graph_chart_append",
        pattern: "40 55 53 56 57 41 54 48 8D 6C 24 C9 48 81 EC 00 01 00 00 48 C7 45 E7 FE FF FF FF 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 2F 49 8B F8 48 8B F1",
        description: "Graph chart series append (chart, &vector<double>, &color callable). Copies the series data into the chart; the callable supplies the bar color.",
    },
    // Chart TWO-COLOUR series append (s-marvelous judgement graph — the
    // shimmer gradient): (chart, vector<double>*, callable {vft, c0 u32
    // @+8, c1 u32 @+0xC, pad, impl_ptr}) — the sibling 0x80 bytes BEFORE
    // `graph_chart_append`. Same data deep-copy; the callable is pushed
    // into `chart+0xF0` DIRECTLY (no lambda8 adapter) because it already
    // has the renderer's outer type `function<function<uint(double,
    // double)>(int bucket, int count)>`: per cell the renderer asks it for
    // an inner colour functor and evaluates that at the quad's four corners
    // (u,v ∈ {0,1}) — vertex colours, so the rasterizer interpolates. The
    // stock ALL-MARVELOUS series (`tab+0x5F8`) rides it with the `lambda17`
    // functor (`c0 = 0xA9FEECFF` cyan, `c1 = 0xDEA7EFFF` pink; inner
    // `lambda47` returns `v > 0.5 ? c0 : c1` — c1 at the top of the bar,
    // c0 at the bottom = the pearlescent vertical gradient). The mod
    // captures that functor's vftable live and clones `{vft, light, violet}`
    // for the pure-S-Marvelous seconds. Unique on 20250805 @0x1801BA120
    // and 20260825 @0x1801D01C0 (Ghidra); cookie disp + the two internal
    // CALL rel32s wildcarded, the `+0xD0`/`+0xF0` chart offsets pinned.
    SignatureDefinition {
        name: "graph_chart_append_2c",
        pattern: "40 57 48 83 EC 40 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 68 48 8B 05 ? ? ? ? 48 33 C4 48 89 44 24 30 49 8B F8 48 8B D9 4C 89 44 24 28 48 81 C1 D0 00 00 00 E8 ? ? ? ? 48 8D 8B F0 00 00 00 48 8B D7",
        description: "Graph chart two-colour series append (chart, &vector<double>, &outer colour callable). The stock all-Marvelous shimmer gradient rides it; the callable is pushed without the single-colour adapter.",
    },
    // GraphTab legend text helper (s-marvelous judgement graph):
    // (ctx {rect block*, cursor*, tab*}, &string, rgba) — creates a scaled
    // 0.6 text object, tints it, pushes into tab+0x1A0, advances the
    // cursor by the text width. Unique on 20260721 @0x1800F15E0 /
    // 20260616 @0x1800F1180 (exact bytes, no relocs in the window).
    SignatureDefinition {
        name: "graph_legend_text",
        pattern: "48 8B C4 55 57 41 54 48 8D 68 A1 48 81 EC 90 00 00 00 48 C7 45 E7 FE FF FF FF 48 89 58 10 48 89 70 18 41 8B D8 4C 8B E1 48 8B 41 08 44 8B 08 41 FF C1",
        description: "GraphTab legend-line helper (ctx, string, rgba). Appends one colored legend text to the tab and advances the layout cursor.",
    },
    // GraphTab TIMING-page axis half-max (s-marvelous timing graph): the
    // rebuild calls it once per frame with `&{tab*, judge_axis_max}` and
    // uses the int it returns as the ±y range of BOTH timing bar charts and
    // the gridline extent. Stock scans the 8 drawn timing series
    // (tab+0x398..+0x3F8 FAST, +0x438..+0x498 SLOW — NOT the grade-0/6
    // series at +0x418, which is never drawn) for the tallest per-second
    // stack, floors it at judge_max/2, and rounds up to even
    // (`2*ceil(x/2)`). The mod's post-original detour folds its Marvelous
    // FAST/SLOW series into the same max so the taller stacks never draw
    // past the chart box. Byte-identical (displacements included) and
    // exactly-once on 20250805 @0x1800E5940 and 20260825 @0x1800F1EB0.
    SignatureDefinition {
        name: "graph_timing_axis_max",
        pattern: "48 83 EC 38 48 8B 01 66 0F 57 DB 4C 8B C1 48 8B 90 80 03 00 00 F2 0F 11 5C 24 40 66 0F 28 D3 48 2B 90 78 03 00 00 48 C1 FA 03 48 85 D2",
        description: "GraphTab timing-page axis half-max (&{tab, judge_max}) -> int. Tallest FAST/SLOW per-second stack, rounded up to even; feeds both timing charts' y range.",
    },
    // Results window build (s-marvelous FC emblems, Step 9 — runs ONCE at
    // results-scene build). Drives the per-stage clear-kind emblem: suffix
    // from the DAT_180486410 table ([10]="mfc"), refer
    // "player_%dp_info_usr/fc_usr", `afp_mc_op(mc, 0xF09, "loop_"+suffix)`.
    // Prologue-anchored; the frame displacement −0xA18 + SUB RSP,0xAF0 +
    // the 0x200 EH slot pin uniqueness — verified exactly-once on 20260721
    // @0x1800B8AA0 AND 20260616 @0x1800B88A0, byte-verified in the on-disk
    // cabinet DLL (the "scre_rank_%s" string's only xref is inside).
    // Security-cookie RIP disp (past the pattern) excluded.
    SignatureDefinition {
        name: "result_window_build",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 E8 F5 FF FF 48 81 EC F0 0A 00 00 48 C7 85 00 02 00 00 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20",
        description: "sequence::result results-window builder (this). One-shot scene build: rank/emblem/flare bitmaps + fc_usr loop_<kind> label goto per side.",
    },
    // Same builder, pre-20260324 frame (20250805 @ 0x1800B0CB0, 20260224 @
    // 0x1800B35F0 — byte-identical prologue, unique on both, absent on
    // 20260324+): −0x978 frame displacement + SUB RSP,0xA50 + the 0x1B8 EH
    // slot. Layout the emblem detour consumes is decompile-identical on
    // both: `this+0x108` result_root layer (id at +8), `this+0xEC` stage,
    // record `+0x54` clear kind → suffix table ([10] = "mfc") → `fc_usr`
    // `0xF09 "loop_<suffix>"`.
    SignatureDefinition {
        name: "result_window_build_v1",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 88 F6 FF FF 48 81 EC 50 0A 00 00 48 C7 85 B8 01 00 00 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20",
        description: "sequence::result results-window builder, pre-20260324 prologue (20250805 / 20260224). Same layout as result_window_build.",
    },
    // Total-results populate (s-marvelous FC emblems, Step 9). Builds the
    // per-stage "total_result" pane layers (actor+0x1B0+pane*8) and loads
    // the clear-kind badge bitmap "scre_total_player_%s" (suffix table
    // DAT_180486E80, [10]="fc_mfc") into the fullcombo_usr leaves under
    // total_p%d_top_usr. Prologue-anchored; frame displacement −0x5A8 +
    // SUB RSP,0x680 + the XMM spill run pin uniqueness — verified
    // exactly-once on 20260721 @0x1800CB090 AND 20260616 @0x1800CB170,
    // byte-verified in the on-disk cabinet DLL (anchors: the only
    // "total_result" / "fullcombo_usr" xrefs are inside).
    SignatureDefinition {
        name: "total_result_populate",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 58 FA FF FF 48 81 EC 80 06 00 00 48 C7 85 40 02 00 00 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 0F 29 70 C8 0F 29 78 B8",
        description: "sequence::result total-results populate (this). Builds per-stage panes and loads the per-side clear-kind badge into fullcombo_usr.",
    },
    // Same populate, pre-20260324 frame (20250805 @ 0x1800C1840, 20260224 @
    // 0x1800C3FC0 — byte-identical prologue, unique on both, absent on
    // 20260324+): −0x5D8 frame displacement + SUB RSP,0x6B0 + the 0x230 EH
    // slot. Layout the emblem detour consumes is decompile-identical on
    // both: `this+0x9C` primary side, `this+0x1B0 + pane*8` total_result
    // pane layers (one pane per non-virgin stage, in order), record `+0x54`
    // clear kind → suffix table ([10] = "fc_mfc") → `fullcombo_usr` under
    // `total_p%d_top_usr`.
    SignatureDefinition {
        name: "total_result_populate_v1",
        pattern: "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8 28 FA FF FF 48 81 EC B0 06 00 00 48 C7 85 30 02 00 00 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 0F 29 70 C8 0F 29 78 B8",
        description: "sequence::result total-results populate, pre-20260324 prologue (20250805 / 20260224). Same layout as total_result_populate.",
    },
    // CalcCalorieActor per-frame tick (vtable slot 6, shared Single/Double).
    // Reads the current measurement-window index (+0x92) and its closed flag
    // (+0x68 + idx*8); when closed, calls vtable slot 9 (+0x48) for the
    // per-window kcal increment and accumulates it into the running per-stage
    // kcal total at +0x94. PowerUserStatistics detours this to cache the live
    // per-side kcal for the realtime-calorie display. Body is byte-identical on
    // 20260324 (@0x180053a50) and 20260616 (@0x180053470); the two short-jump
    // displacements are wildcarded. See docs/calorie_weight_profile_research.md §3.1.
    SignatureDefinition {
        name: "calc_calorie_tick",
        pattern: "40 53 48 83 EC 20 0F B7 91 92 00 00 00 48 8B D9 8B 54 D1 68 FF CA 75 ?? 48 8B 01 FF 50 48 85 C0 74 ?? 01 83 94 00 00 00",
        description: "CalcCalorieActor per-frame tick (this). Accumulates per-window kcal into actor+0x94 (the running per-stage calorie total).",
    },
    // ── Training Mode strip HUD (chart-strip timeline, Step 6) ───────
    // The tap quantization→palette-row selector called from the arrow
    // fill (`FUN_180028130` @ 20260616 ≡ `0x180027d10` @ 20260721 ≡
    // `0x180027650` @ 20260324): reads the color-option field at
    // renderer+0xE8 — values 0/5 select beat-DIVISION rows (4th/8th/16th
    // = rows 1/3/2, else 4), anything else the beat-CYCLING mode. The
    // strip synthesis CALLS it with the live ArrowRenderer so a future
    // quantization-granularity hack propagates for free (maintainer
    // constraint — never replicate its math). Signature: fastcall
    // `u32 selector(ArrowRenderer* rcx, i32 beat edx)` — a pure leaf
    // (arithmetic + one field read), safe on the game thread. The body
    // is fully position-independent; this 44-byte head matches UNIQUELY
    // and byte-identically on 20260324/20260616/20260721. RE:
    // docs/chart_strip_hud_research.md §4.
    SignatureDefinition {
        name: "arrow_row_selector",
        pattern: "44 8B 81 E8 00 00 00 8B C2 32 D2 25 FF 03 00 00 45 85 C0 0F B6 CA 41 B9 01 00 00 00 41 0F 44 C9 41 83 F8 05 0F B6 D1 41 0F 44 D1 84 D2",
        description: "Quantization -> palette-row selector (ArrowRenderer* this, i32 beat) -> row 1..4. Called per note by the strip synthesis with the live renderer (never replicated).",
    },
    // ── File / Resource Manager (texture loading) ────────────────────
    // Used by NoteTypesExpansion to load mine PNG textures via the
    // engine's file pipeline (agcs::FileManager dispatches to
    // PngFileCallback, which registers the texture in the resource
    // system). The FileManager singleton pointer is derived from
    // xrefs to file_manager_load.
    SignatureDefinition {
        name: "file_manager_load",
        pattern: "40 53 56 57 48 81 EC F0 00 00 00 48 8B 05 ? ? ? ? 48 33 C4 48 89 84 24 E0 00 00 00 48 8B F1 48 8D 4C 24 40",
        description: "agcs::FileManager member that loads a file by path and returns an i32 handle. Dispatches to registered callbacks by file extension (PngFileCallback for .png). Async — handle is valid immediately but resource registration completes on a worker thread.",
    },
    // The Free counterpart to file_manager_load: enqueues a loaded file's
    // table index onto the same FileManager's release queue (member +0x98),
    // guarded by the identical +0x150/+0x154 busy lock the loader uses. The
    // engine drains the queue on a worker thread; for a registered PNG this
    // fires the callback's OnDetach, which calls ReleaseTextureData(stem).
    // Load is refcounted (loading an already-resident file bumps its +0x24
    // refcount instead of re-reading), so each load handle pairs with exactly
    // one free. Signature: (FileManager* /*RCX*/, i32 index /*EDX*/) -> void.
    // Used by asset_loader to evict on-demand preview textures.
    SignatureDefinition {
        name: "file_manager_free",
        pattern: "48 89 5C 24 08 48 89 74 24 18 89 54 24 10 57 48 83 EC 20 48 8B F9 8B 89 50 01 00 00 8B F2 85 C9 7E 06",
        description: "agcs::FileManager member that releases a file by its i32 handle (the index returned by file_manager_load). Enqueues the index onto the manager's release queue (+0x98); the engine drains it async, firing PngFileCallback::OnDetach → ReleaseTextureData(stem) for registered textures. Args: (this, index).",
    },
    SignatureDefinition {
        name: "resource_manager_get_texture_hash_value",
        pattern: "48 89 5C 24 10 48 89 74 24 18 57 48 83 EC 70 48 8B 05 ? ? ? ? 48 33 C4 48 89 44 24 60 48 8B F1 33 C0 48 83 C9 FF",
        description: "Hashes a texture name string (lowercase + strip underscores + FNV) and returns a u32 hash. Static function — no this pointer. The hash vtable is at a global resolved via the indirect CALL at the function's tail.",
    },
    SignatureDefinition {
        name: "resource_manager_get_texture_data",
        pattern: "48 89 5C 24 08 57 48 83 EC 20 48 8B 1D ? ? ? ? 8B F9 8B 8B 50 01 00 00 85 C9 7E 06 FF 15 ? ? ? ? FF 83 54 01 00 00 48 8B 8B B8 00 00 00 8B 93 54 01 00 00 48 8B 41 08 80 78 41 00 75 17",
        description: "Looks up a gs::TextureData pointer by hash. Returns null if the hash is not registered. Loads the ResourceManager singleton from a global at the function's first RIP-relative MOV.",
    },
    // ── BM2D data manager (on-demand arc → package load) ─────────────
    // `bm2d::data::(anonymous namespace)::Manager` (name from its own log
    // string) owns the registry of loaded BM2D packages (the CFileData
    // vector global, `DAT_1806f1d68` on 20260526 / `DAT_1806ebce8` on
    // 20260324) and the on-demand load path the game itself uses for
    // backgrounds, HUD clips, etc. Used by the background preview overlay
    // to load `background_%04d.arc` packages on demand. All three entry
    // points verified byte-identical (single match) on both supported
    // builds. See `.agents/planning/20260708-background-preview-overlay/
    // progress.md` for the full Ghidra derivation.
    //
    // Request-load: `bool f(const char* dir /*e.g. "custom/background"*/,
    // const char* name /*e.g. "background_0001"*/, u32 flag /*game: 0*/)`.
    // Dedups by name against the registry, resolves the arc-name variant
    // (`%s_v3` → `%s_lite` → `%s` — machine-type-gated), opens the arc via
    // the arc manager and appends a registry entry with a NULL package ptr.
    // The manager's per-frame Update (pumped by the engine main loop)
    // creates the package once every queued arc finishes loading.
    SignatureDefinition {
        name: "bm2d_data_request_load",
        pattern: "48 8B C4 55 57 41 54 41 55 41 56 48 8D 68 A1 48 81 EC E0 00 00 00 48 C7 45 97 FE FF FF FF 48 89 58 18 48 89 70 20 48 8B 05 ? ? ? ? 48 33 C4 48 89 45 27 45 8B E0 48 8B F2 4C 8B E9 48 8B 3D",
        description: "bm2d::data Manager request-load. Args: (dir, name, flag). Non-blocking: queues the arc + appends a pending registry entry; poll bm2d_data_is_ready for completion.",
    },
    // Is-ready: `bool f(const char* name)` — true once the registry entry
    // exists AND its package pointer is non-null. Also the anchor for two
    // derived addresses: the registry global (RIP-decoded from the
    // `MOV RAX,[rip+disp32]` at +6, disp at +9) and the name-lookup helper
    // (`Entry* lookup(Entry* begin, Entry* end, const char* name)`,
    // decoded from the `CALL rel32` at +26). The final instruction
    // `MOV RCX,[RAX+disp8]` reads the entry's package pointer — its disp8
    // (at +39, wildcarded; 0x30 on both 2026 builds) is the entry-layout
    // offset `bm2d_package::init` reads from the matched bytes, so a
    // layout-only change survives instead of desyncing.
    SignatureDefinition {
        name: "bm2d_data_is_ready",
        pattern: "40 53 48 83 EC 20 48 8B 05 ? ? ? ? 4C 8B C1 48 8B 58 08 48 8B 08 48 8B D3 E8 ? ? ? ? 48 3B C3 74 10 48 8B 48 ?",
        description: "bm2d::data Manager is-ready check. Args: (name) -> bool. Anchor for bm2d_package_registry + bm2d_package_lookup derivation + the entry package-pointer offset (disp8 at +39).",
    },
    // Release: `void f(const char* name)` — destroys the package and erases
    // the registry entry. Only dynamic entries (index >= 72) are erased;
    // the manager's 72 permanent common entries are protected, so releasing
    // an on-demand background is always safe.
    SignatureDefinition {
        name: "bm2d_data_release",
        pattern: "40 53 41 54 41 56 48 83 EC 30 4C 8B 25 ? ? ? ? 4C 8B F1 49 8B 1C 24 49 3B 5C 24 08 0F 84 ? ? ? ? 48 89 6C 24 50",
        description: "bm2d::data Manager release-by-name. Args: (name). Destroys the package and erases the registry entry (dynamic entries only).",
    },
    // ── Arrow render pipeline (mine render pass) ─────────────────────
    // The outer per-frame note-rendering member on the arrow renderer
    // object. Runs the shock-arrow pass (silver glyph + lightning overlay)
    // then the normal-arrow pass. Hook target for appending a dedicated
    // mine pass after both vanilla passes complete.
    SignatureDefinition {
        name: "render_notes",
        pattern: "48 8B C4 55 53 56 57 41 54 41 55 41 56 41 57 48 8D A8 28 FF FF FF 48 81 EC 98 01 00 00",
        description: "Arrow renderer's per-frame note draw function. RCX = ArrowRenderer this. Calls the per-pass note collector twice (shock then normal) and emits sprite batches via inlined CommandList writes.",
    },
    // The per-frame LAYER DISPATCHER: iterates the 11-entry layer table
    // (global at the RIP disp32 at match+13, entries {override_ptr,
    // layer_object, list_index} stride 0x18), sets the ScreenRenderer
    // state's active-list index (+0x68 — its ONLY writer), and walks each
    // enabled layer (vtbl+0x28) to record its display quads. Called once
    // per frame unconditionally from the render orchestrator. The
    // overlay-draw animated-background emitter detours this and appends
    // its quad to the widget layer's list PRE-original so the quad sits
    // beneath the menu's own widgets but above every lower layer in every
    // scene. Verified unique on 20260721 (0x18002af10) and 20260616
    // (0x18002b530); the `LEA EDI,[RBX+0xB]` 11-count shortly after is the
    // structural confirmation. RE: docs/overlay_draw_research.md.
    SignatureDefinition {
        name: "layer_dispatcher",
        pattern: "48 89 5C 24 08 57 48 83 EC 60 48 8B 15 ? ? ? ? 4C 8B 05 ? ? ? ? 0F 29 74 24 50 0F 57 F6",
        description: "Per-frame layer dispatcher (walks the 11-entry layer table, writes the active-list index)",
    },
    // The receptor-row (spot) renderer's per-frame draw. Emits one
    // SetShader (the spot shader object @ this+0xA0) + one 4/8-quad
    // ROTATESPRITE batch (mode @ this+0x98) through the shared per-quad
    // fill. Hook target for the player-perspective mod's receptor pass
    // rewrite (the receptors span ~96 px of track depth, so the hallway
    // map foreshortens them like a note at the row).
    SignatureDefinition {
        name: "spot_render",
        pattern: "53 56 57 41 55 41 57 48 83 EC 60 48 8B 15 ?? ?? ?? ?? 83 B9 98 00 00 00 01",
        description: "SpotRenderer per-frame receptor draw. RCX = SpotRenderer this (ArrowSprite base: posX/posY @ +0x30/+0x34, mode @ +0x98, shader @ +0xA0).",
    },
    // JudgeEffectRenderer per-frame draw: ages the effect records
    // (vector @ +0xA0..+0xA8), then emits its OWN tag-0x13 SetShader
    // (shader object @ this+0x98, program hardcoded 0) + one quad batch
    // into the global command list. Draws BOTH the tap hit-burst and the
    // freeze-hold glow (arrow-sheet cells at the receptor row) — the pass
    // player_perspective rewrites to the judge container's perspective
    // program. Pattern = prologue + the two structural vector-field loads
    // (member offsets, no relocatable bytes); verified unique on 20260324
    // (0x1800279b0), 20260616 (0x180028490), 20260721 (0x180028070).
    SignatureDefinition {
        name: "judge_effect_render",
        pattern: "48 89 5C 24 10 57 48 83 EC 40 48 8B 99 A8 00 00 00 4C 8B 89 A0 00 00 00 48 8B F9",
        description: "JudgeEffectRenderer per-frame draw. RCX = renderer this (ArrowSprite base: posX/posY @ +0x30/+0x34, shader @ +0x98, records vector @ +0xA0/+0xA8).",
    },
    // JudgeEffectRenderer record pusher: `void push(this, u8 lane_bits,
    // int type)` — builds `{t0 = this+0x94 (the renderer's clock), lanes,
    // type}` and `vector::push_back`s it into `this+0xA0` (the game's own
    // allocator). The ONLY two stock callers are `judgeNotes` (types 1/2/3
    // = Perfect/Great/Good burst) and the freeze-hold tick (type 4); types
    // 0/5/6 (the 150 ms "flash" path) have NO pusher on any supported
    // build, and nothing reads the type except the renderer's own prune +
    // emit. The s_marvelous receptor burst calls this with type 7 (outside
    // both draw-time type classes: Perfect's 200 ms / 1.25-grow geometry
    // with the base greyscale `(f,f,f)` colour) and recolours the resulting
    // white quads violet at the fill. Prologue anchored (`40 53` REX-prefixed PUSH
    // RBX), cookie disp wildcarded, then the `+0x94` clock read + register
    // setup. Unique on 20250805 (0x1800279D0) and 20260825 (0x180027EC0).
    SignatureDefinition {
        name: "judge_effect_push",
        pattern: "40 53 48 83 EC 40 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 44 24 38 8B 81 94 00 00 00 45 33 C9 41 8B D8 4C 8B D9 44 0F B6 D2",
        description: "JudgeEffectRenderer::push(this, u8 lane_bits, int type). Appends {this+0x94, lanes, type} to the records vector @ this+0xA0.",
    },
    // The final overload of the per-sprite filler on the arrow sprite
    // base class. Takes explicit UV, rotation, and color — does not read
    // member UV/twist state. Handles appearance alpha, reverse, and
    // rotation math, then writes vertex positions + UV + color into a
    // ROTATESPRITE entry (0x34 bytes).
    SignatureDefinition {
        name: "render_sprite_final",
        pattern: "48 8B C4 53 48 81 EC C0 00 00 00 F3 0F 10 61 6C",
        description: "ArrowSprite per-quad filler (final overload). Args: (this, &sprite, x, y, w, h, &uv[4], twist, &color). Writes 0x34-byte ROTATESPRITE.",
    },
    // Sets the rotation angle on the arrow sprite base class from a
    // panel direction index (0=left, 1=down, 2=up, 3=right). Maps
    // direction to a quarter-turn twist value stored on the object.
    SignatureDefinition {
        name: "set_direction",
        pattern: "40 53 48 83 EC 30 45 33 C0 81 E2 03 00 00 80",
        description: "ArrowSprite direction setter. Args: (this, dir). Converts dir%4 to a rotation angle and stores it.",
    },
    // ── Playfield styling: lane clip capture (CMovieClip helpers) ────
    // Two generic CMovieClip helpers the playfield-styling mod detours to
    // scale the lane background + lane cover (both AFP-layer clips that do
    // NOT flow through render_sprite_final). Neither is Create'd by lane
    // name: the lane bg is a find-child of the gameplay `dance_root` movie;
    // the lane cover is created via a pool-slot wrapper around
    // CMovieClip::Create. Both are hooked directly (collision-free —
    // overlay-element-styling owns CMovieClip::Create itself, which these
    // bypass). Prologues verified unique on builds 20260616 + 20260324.
    SignatureDefinition {
        name: "cmovieclip_pool_create",
        pattern: "48 89 6C 24 10 48 89 74 24 18 48 89 7C 24 20 41 54 48 83 EC 30 41 8B E9 49 8B F8 4C 8B E2 48 8B F1",
        description: "CMovieClip pool-slot create-from-package wrapper (pool, package, name /*R8, C-string*/, priority, mode) -> clip slot (layer id at slot+0x08), or null. Wraps CMovieClip::Create. Playfield styling captures the lane cover (hidden_cover_* / sudden_cover_*) here.",
    },
    // NoteResultActor setup (RCX = actor). Creates the per-panel hit-flash
    // clips (`dance_effect`, via afp_layer_create_with_property — NOT through
    // Create/pool-create, so the other two hooks miss them) and stores them
    // in the actor's `vector<CMovieClip*>` at actor+0xE8 (begin) / +0xF0
    // (end); each element's AFP layer id is at clip+0x08. Play mode is at
    // actor+0x90 (0 = single/4 panels, else double/8). Playfield styling
    // hooks this to scale the receptor hit flashes. Prologue unique on builds
    // 20260616 (0x18007A230) + 20260324 (0x18007AF20).
    SignatureDefinition {
        name: "note_result_setup",
        pattern: "48 89 4C 24 08 53 55 56 57 41 54 41 55 41 56 41 57 48 83 EC 68 48 8B 81 88 00 00 00 48 8B E9",
        description: "NoteResultActor setup (this). Builds the judge/fast_slow/score_compare clips + the per-panel receptor hit-flash clips (dance_effect) into the actor's vector<CMovieClip*> @ +0xE8..+0xF0. Playfield styling walks that vector to scale the hit flashes.",
    },
    // Scroll-Y computation shared by the shock and normal render
    // passes. Args: (dBeatCount, speed, boost, musicCount) — speed is
    // stored on the arrow renderer as an integer (speed*100), so the
    // formula is: d = (dBeatCount * speed * 96) / 100, then the result
    // is adjusted by the boost/brake/wave enum.
    SignatureDefinition {
        name: "get_offset_y",
        pattern: "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 66 0F EF C0 48 63 C2 41 8B F1 41 8B F8 48 0F AF C1 48 8D 04 40 48 C1 E0 05",
        description: "Scroll-Y pure function. Returns pixel offset from the spot row as a float.",
    },
    // ── Per-side player-work table anchor ────────────────────────────
    // A short accessor function whose first two instructions load the
    // global player-work table for the 1P and 2P sides back-to-back.
    // The two loads in sequence are distinctive enough to make this
    // anchor unique across the whole binary.
    //
    // Used for reading the player's Option struct (inlined on the
    // PlayerWork object at +0xE0). The Option's arrow-shape field sits
    // at +0x60 inside that inlined struct. The path from a gameplay
    // actor is:
    //     actor[+0x84]               = playSide (i32)
    //     player_work_table[playSide] = wrapper*
    //     *wrapper                   = PlayerWork*
    //     PlayerWork[+0xE0]          = Option (inlined)
    //     Option[+0x60]              = arrow_shape (i32 in 0..7)
    //
    // The first RIP-relative MOV (at offset +3 of the anchor) resolves
    // to the player-work table global; derived via
    // `derive_player_work_table` below.
    SignatureDefinition {
        name: "player_work_table_anchor",
        pattern: "48 8B 05 ? ? ? ? 48 8B 08 80 79 04 00 74 09 66 C7 05 ? ? ? ? 7F 04 48 8B 05 ? ? ? ? 48 8B 08 80 79 04 00 74 09 66 C7 05 ? ? ? ? 7F 04",
        description: "Accessor whose first two instructions load slots [0] and [1] of the per-side player-work table global. Landmark for deriving player_work_table via RIP-relative decode.",
    },
    // ── Custom player options framework ──────────────────────────────
    // Signatures for sequence::selectmusic subsystem hook points.
    // Research: docs/custom_player_options_research.md "Signatures — Verified Cross-Version".

    // Row-builder anchor. The prologue itself is not unique (shared shape with
    // 2 unrelated large functions), so we anchor on an internal landmark at
    // The 21-row OptionForm builder has a unique prologue: 5 callee-saved
    // registers + a large stack frame (LEA RBP,[RSP-0x1A40..0x1A20]) + a
    // __chkstk allocation of ~0x1B00..0x1B40. The `?? E5 FF FF B8` sequence
    // (LEA frame byte + SUB size prefix) is structurally unique across all
    // known game versions.
    SignatureDefinition {
        name: "row_builder_fn_prologue",
        pattern: "40 55 41 54 41 55 41 56 41 57 48 8D AC 24 ? E5 FF FF B8",
        description: "21-row OptionForm builder function entry (direct prologue match, all versions).",
    },

    // Tab-state re-renderer FUN_180168d10. Seven-register save + specific frame
    // sizes (0x260/0x360/0x78) are structurally unique to this function in both
    // builds. Prologue-anchored (match address IS function entry).
    SignatureDefinition {
        name: "tab_filter_fn",
        pattern: "40 55 56 57 41 54 41 55 41 56 41 57 48 8D AC 24 A0 FD FF FF 48 81 EC 60 03 00 00 48 C7 44 24 78 FE FF FF FF 48 89 9C 24 B0 03 00 00",
        description: "Tab-state re-renderer (FUN_180168d10). Detour target for Page6 filter reimplementation; iterates all rows and show/hides each based on its PageN metadata tag.",
    },

    // Metadata-set insert. Inserts a hashed std::string key into the
    // metadata-set inlined in an OptionElement at `+0x08..+0x28`. The game's
    // row builder calls this to tag each OptionElement with its `"PageN"`
    // category string. The 4 wildcards cover the internal CALL rel32; the
    // remaining 35 bytes are prologue + shadow-store + first few arg setup
    // moves, structurally invariant across compiler toolchain drift.
    //
    // Signature: fn(OptionElement* row, std::string* key) -> OptionElement*.
    // Calling convention: Microsoft x64 (RCX=row, RDX=key).
    // Side effect: hashes the std::string's contents (FNV-1a) and inserts
    // into the rb-tree at row+0x08.
    SignatureDefinition {
        name: "metadata_insert",
        pattern: "48 89 5C 24 10 57 48 83 EC 30 48 8B F9 48 8B CA E8 ? ? ? ? 48 8B 5F 18 48 8D 4F 08 4C 8D 44 24 40 48 8D 54 24 20",
        description: "OptionElement metadata-set insert. Signature: fn(OptionElement* row, std::string* key) -> OptionElement*. Hashes the key's contents (FNV-1a) and inserts into the rb-tree at row+0x08.",
    },

    // OptionTab row-register helper. Wraps a bare element pointer in a
    // shared_ptr and appends it to the flat row vector at
    // `(parent+0x230)+0x68`, then writes the scene-graph anchor back into
    // the row at `row+0x60` and dispatches the IResourceSharing cleanup
    // lambda for rows that implement that interface.
    //
    // The 30-byte prologue is structurally unique: no wildcards, no other
    // function in gamemdx dereferences a caller-supplied pointer-to-pointer
    // and reads `[+0x230]` this early in its prologue.
    //
    // Signature: fn(&parent_ptr: *mut *mut u8, row: *mut u8) -> *mut u8.
    // Calling convention: Microsoft x64 (RCX = address of a stack-local
    // holding the parent pointer; RDX = row).
    SignatureDefinition {
        name: "option_tab_register",
        pattern: "48 89 5C 24 10 48 89 74 24 18 57 48 83 EC 50 48 8B 01 48 8B FA 48 8B F1 48 8B 98 30 02 00 00",
        description: "OptionTab row-register helper. Signature: fn(*mut *mut u8 parent_slot, *mut u8 row) -> *mut u8. Wraps row in shared_ptr and appends to the flat row vector at (*parent_slot + 0x230) + 0x68.",
    },

    // OptionForm destructor. Fires once per carded-in side when the options
    // overlay closes — the moment the game frees that side's option rows. The
    // custom_options framework detours it to drop its stale RowSlot pointers
    // (clear_side) before any +0xB8-writing path can dereference freed rows.
    //
    // The bare dtor prologue is the generic MSVC 3-vtable shape (shared with
    // 2 unrelated dtors), so the signature extends through the body: three
    // vtable-pointer writes, `ADD RCX, 0xC0`, the sub-object release CALL, then
    // `MOV RBX, [RSI+0x238]` (shared_ptr release of the +0x238 field) — that
    // tail is what makes it unique. Wildcards cover the 3 vtable-LEA disp32s and
    // the CALL rel32. Verified unique on both 20260526 (FUN_18018dda0) and
    // 20250805 stock (FUN_1801786b0); player side is read at OptionForm+0x228.
    SignatureDefinition {
        name: "optionform_dtor",
        pattern: "48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 48 48 89 6C 24 50 48 89 74 24 58 48 8B F1 48 8D 05 ? ? ? ? 48 89 01 48 8D 05 ? ? ? ? 48 89 41 28 48 8D 05 ? ? ? ? 48 89 81 C0 00 00 00 48 81 C1 C0 00 00 00 E8 ? ? ? ? 90 48 8B 9E 38 02 00 00",
        description: "OptionForm::~OptionForm. Signature: fn(this). Fires per carded-in side on options-overlay close; player side at this+0x228. Detoured by custom_options to invalidate stale row pointers.",
    },

    // Component visibility toggle. Used by the tab-filter detour's Phase 3
    // to toggle per-row visibility on tab switch. Writes the visible byte
    // to row+0xB8 (the isActive flag) and propagates to children via the
    // child vector at row+0x68..+0x70.
    //
    // Signature: fn(Component* row, bool visible).
    // Calling convention: Microsoft x64 (RCX=row, DL=visible).
    SignatureDefinition {
        name: "component_set_visible",
        pattern: "48 89 5C 24 10 57 48 83 EC 20 0F B6 FA 48 8B 51 60 48 8B D9 48 85 D2",
        description: "Component visibility toggle. Signature: fn(Component* row, bool visible). Writes visible to row+0xB8 and propagates to children.",
    },

    // Event-callback registration. Native slot-4 (`FUN_180173c10`,
    // `advanceValue`) calls a sibling (`FUN_18017dc40`) that registers four
    // per-direction lambdas against the current input event via repeated
    // calls to this function. The dispatcher (`FUN_180048a90`) also calls
    // this function directly for navigation types (3=up, 4=down). For every
    // input event, the engine calls this function once per registered
    // handler with `(event_obj, type, &lambda)`; only the type matching the
    // event currently in flight fires its lambda. Mod rows reuse this
    // mechanism so left/right are type-gated exactly like native rows.
    //
    // Signature: fn(event_obj: *mut u8, event_type: i32, lambda: *mut u8).
    //   RCX = event_obj (stack-local from the dispatcher; same pointer across
    //         every call in a given dispatch window)
    //   EDX = event_type (1=left, 2=right, 3=up, 4=down, 0=stop)
    //   R8  = lambda (std::tr1::_Impl_no_alloc0 frame, 32 bytes on stack or
    //         16 bytes on the CRT heap, first qword = vtable pointer)
    //
    // 38-byte prologue: MOV [RSP+0x18],R8; PUSH RSI; SUB RSP,0x50;
    // MOV [RSP+0x20], -2; MOV [RSP+0x68],RBX; MOV RBX,R8; MOV R8D,EDX;
    // MOV RSI,RCX; CMP [RCX+0x10], 1; JZ +5 byte. No wildcards needed;
    // every byte is structurally mandated by the calling convention and
    // the control-flow shape. Verified unique on 20260324 and 20250805.
    SignatureDefinition {
        name: "event_register",
        pattern: "4C 89 44 24 18 56 48 83 EC 50 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 68 49 8B D8 44 8B C2 48 8B F1 48 83 79 10 01",
        description: "Event-callback registration (FUN_180045b70). Signature: fn(event_obj: *mut u8, event_type: i32, lambda: *mut u8). Registers a type-gated lambda against the current input event when the Start-modifier is NOT held (event_obj+0x10 == 1). Used by enum rows and by scalar-row fine-step lambdas.",
    },

    // Twin of event_register used by scalar rows to register the coarse-step
    // (Start-held) variant of left/right advance lambdas. Identical registration
    // shape to event_register, but gated on event_obj+0x10 == 2 instead of 1.
    // The third parameter is an auxiliary predicate that the native scalar
    // slot-4 (FUN_180162680) always passes as 0; same behavior is fine for mod
    // rows.
    //
    // 38-byte prologue with no wildcards: two shadow-store MOVs, SUB RSP 0x50,
    // MOV [RSP+0x28], -2, two register saves, three register copies
    // (RBX/R9D/RDI), CMP [RCX+0x10], 2. Verified unique on 20260324 and 20250805.
    SignatureDefinition {
        name: "event_register_no_consume",
        pattern: "4C 89 4C 24 20 57 48 83 EC 50 48 C7 44 24 28 FE FF FF FF 48 89 5C 24 68 49 8B D9 44 8B CA 48 8B F9 48 83 79 10 02",
        description: "Event-callback registration, Start-held variant (FUN_180051130). Signature: fn(event_obj: *mut u8, event_type: i32, predicate_arg: u32, lambda: *mut u8). Registers a lambda that only fires when the Start modifier is held (event_obj+0x10 == 2). Used by scalar-row coarse-step lambdas.",
    },

    // Scene-graph layout flush called at the tail of the native tab-filter.
    // Applies the scene root's pending (x, y) offsets to every row in the
    // flat row vector at `scene_root+0x68..+0x70`, running any pending
    // scroll-position easing in the process. The tab-filter detour calls
    // this as its final step so row positions stay in sync with the
    // scene's layout state after a visibility refresh.
    //
    // Signature: fn(SceneRoot* root, bool commit_immediately).
    // Calling convention: Microsoft x64 (RCX=root, DL=commit_immediately).
    //
    // 19-byte prologue: MOV [RSP+0x20], RBX; PUSH R12; SUB RSP, 0x70;
    // MOVZX R12D, DL; MOV RBX, RCX; CALL rel32. The trailing CALL's 4-byte
    // disp32 is wildcarded because it points at a sibling helper whose
    // address shifts between builds.
    SignatureDefinition {
        name: "scene_layout_flush",
        pattern: "48 89 5C 24 20 41 54 48 83 EC 70 44 0F B6 E2 48 8B D9 E8",
        description: "Scene layout flush. Signature: fn(SceneRoot* root, bool commit_immediately). Walks the row vector at root+0x68..+0x70 and applies scroll offsets; called as the final step of the native tab-filter.",
    },

    // Options-menu focus-advance core. Takes the layout container and a
    // direction (-1 = up, +1 = down) and returns the new focus index
    // (caller writes it to container+0x168). Invoked by BOTH step-up
    // (FUN_1800495a0 passes EDX=-1) and step-down (FUN_180049670 passes
    // EDX=+1) entrypoints, so a single detour here intercepts all
    // cursor-driven focus advances.
    //
    // Detour body can:
    //   - Read EDX to determine direction.
    //   - Pre-advance our scroll window so the target row has +0xB8=1
    //     before the native walk starts (otherwise the native's
    //     `+0xB8 != 0` filter skips hidden rows and the cursor wraps).
    //
    // Signature: fn(container: *mut u8, direction: i32) -> i32.
    // Positional focus-advance: FUN_18004a030 (20260324). The spatial
    // step function called by GridPanel's own up/down navigation lambdas
    // (at container+0x178 / +0x198) when the mode flag at
    // *(lambda+0x08)+0xC0 == 0. Iterates all rows in the vector looking
    // for the nearest selectable row (checking +0xB8 != 0) in the given
    // direction, comparing positions. Returns the target focus index.
    // The caller writes the return value into container+0x168.
    // Calling convention: Microsoft x64 (RCX=container, EDX=direction
    // where +1=down, -1=up). Returns i32 (new focus index).
    SignatureDefinition {
        name: "grid_positional_step_fn",
        pattern: "89 54 24 10 55 53 41 ? 48 8D 6C 24 B9 48 81 EC ? 00 00 00 48 8B D9 48 8B 49 68 4C 8B ? 70 4C",
        description: "GridPanel positional focus-advance. fn(container: *mut u8, direction: i32) -> i32. Returns next selectable focus index.",
    },

    // ── TextLayer (value display via native text pipeline) ───────────────
    // TextLayer objects render digit-composed or bitmap-composed value
    // strings through the game's own UI pipeline rather than per-frame
    // mc_load_bitmap calls. Three functions are needed:
    //
    //   textlayer_ctor      — constructs a TextLayer object in-place;
    //                         matches function start directly.
    //   textlayer_bind      — binds the TextLayer to a parent MC and a
    //                         named child path. AOB match is at fn+0x33;
    //                         function entry derived by subtracting 0x33.
    //   textlayer_set_text  — sets the text/bitmap key on the layer each
    //                         frame; matches function start directly.
    SignatureDefinition {
        name: "textlayer_ctor",
        pattern: "48 83 EC 28 66 0F 57 C0 C6 41 08 00 45 33 C0 48 BA 00 00 00 00 00 00 F0 3F",
        description: "TextLayer constructor. Signature: fn(this: *mut u8) -> *mut u8. Initializes a 0x150-byte TextLayer object; match is at function start.",
    },
    SignatureDefinition {
        name: "textlayer_bind_anchor",
        pattern: "C6 41 60 01 48 89 51 70 48 8D 79 78 49 83 C9 FF 45 33 C0",
        description: "Internal landmark at textlayer_bind+0x33 (older builds). Function entry derived by subtracting 0x33.",
    },
    // Newer builds (20260526+): the function was simplified — no security
    // cookie, 0x20 frame, store-to-+0x70 moved before set-+0x60.
    SignatureDefinition {
        name: "textlayer_bind_direct",
        pattern: "48 89 5C 24 08 57 48 83 EC 20 49 8B C0 48 89 51 70 48 8D 79 78",
        description: "textlayer_bind function entry (20260526+ builds). Direct prologue match.",
    },
    SignatureDefinition {
        name: "textlayer_set_text",
        pattern: "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 20 48 8D 99 A8 00 00 00 41 8B F0 48 8B F9",
        description: "TextLayer set-text function. Signature: fn(this: *mut u8, text_sso: *mut u8, mode: i32) -> *mut u8. Sets the text/bitmap key on the layer; match is at function start.",
    },
    SignatureDefinition {
        name: "pacemaker_render_input",
        pattern: "49 63 76 08 48 8B 97 B0 00 00 00",
        description: "Score-render pacemaker case (0x1036) — movsxd rsi,[r14+8]; mov rdx,[rdi+0xb0]. 11-byte patch site for ms-error swap.",
    },
    SignatureDefinition {
        name: "real_speed_bpm_anchor",
        pattern: "F2 0F 5E 01 48 8D 4C 24 40",
        description: "ddr::player::Option::SetScrollSpeed — divsd xmm0,[rcx]; lea rcx,[rsp+0x40]. Anchor for R24/R25/R26 BPM divisor swap patches.",
    },
    // NOTE: the former `real_speed_logf_anchor` (R15/R16 logf-guard) was
    // retired 2026-09-01: its AOB (`0F 28 C7 E8 ? ? ? ? F3 0F 58 C6`)
    // actually lands inside NoteResultActor::onMessage case 0x1036 — the
    // PACEMAKER readout, not any scroll-speed code (single match, same
    // function, on 20250805/20260616/20260721/20260825). Its R15 byte
    // rewrote the pacemaker zero-branch JMP and broke the exact-0 digit
    // render. See src/mods/real_speed_fix/mod.rs.
    // ── Music Wheel Song Length signatures ──────────────────────────
    // sequence::SpriteLayer — the game's "row of bitmaps by texture name"
    // widget class that renders the song-select header's BPM digits (and
    // dozens of other bitmap strings). The mod constructs one instance of
    // its own and drives it through these two functions; the per-frame
    // layout call goes through the constructed object's own vtable slot 0.
    // RE notes: .agents/planning/2026-08-16-music-wheel-song-length/research.md §3.
    SignatureDefinition {
        name: "spritelayer_ctor",
        pattern: "33 D2 48 8D 05 ? ? ? ? 48 89 01 48 89 51 48 48 89 51 50 48 89 51 08 48 89 51 10 48 89 51 18 48 89 51 28 48 89 51 30 48 89 51 38",
        description: "sequence::SpriteLayer constructor (FUN_1801d2e00 on 20260721). fn(this: *mut u8) -> *mut u8; pure field init on a 0xF8 struct (vftable LEA wildcarded). Unique single hit on 20260324/20260526/20260616/20260721.",
    },
    SignatureDefinition {
        name: "spritelayer_set_names",
        pattern: "41 56 41 57 48 81 EC 88 00 00 00 4C 8B F1 48 83 C1 28 E8 ? ? ? ? 49 8B CE E8 ? ? ? ? 4D 8B 46 28 49 8B 56 30 49 2B D0 48 B8 67 66 66 66",
        description: "sequence::SpriteLayer::SetBitmaps (FUN_1801d3070 on 20260721). fn(this, names: *const StdStringVec) -> *mut u8; COPY-assigns the names vector (source stays caller-owned), releases old CBitmaps, allocates new ones from the CBitmap pool by texture name, ends with a virtual layout call (vtable slot 0). Unique single hit on all four builds.",
    },
    SignatureDefinition {
        name: "selectmusic_model_anchor",
        pattern: "4C 8B 1D ? ? ? ? 48 C7 44 24 38 00 00 00 00 33 F6 48 89 74 24 40 49 8B 93 ? ? 00 00 4D 8B 83 ? ? 00 00",
        description: "MusicCard per-frame tick (FUN_180160910 on 20260721) reading the select-music model global: MOV R11,[rip+d32] then the highlighted-song shared_ptr loads at [R11+slot+8] (ctrl, disp32 at match+26) / [R11+slot] (obj, disp32 at match+33). slot = 0x1B0 on 20260324+, 0x190 on 20250805 / 20260224 — derived as `selectmusic_highlight_slot` (the two disps must be exactly 8 apart). d32 at match+3 → `selectmusic_model` derived global. Unique single hit on all six inspected builds.",
    },
    // ── Overlay Element Styling signatures ──────────────────────────
    // BM2D CMovieClip pool-wrapper methods (see
    // docs/gameplay_overlay_elements_research.md §6).
    //
    // NOTE: `cmovieclip_create` is deliberately NOT in this batch array. Its
    // longest literal run (the 20-byte prologue) shares its first 18 bytes
    // with the existing `afp_layer_init_wrapper` signature — they resolve the
    // SAME function (CMovieClip::Create @ FUN_180257770). The batch scanner
    // builds one Aho-Corasick automaton over all needles and iterates
    // NON-overlapping matches, so at that address the shorter (afp) needle is
    // consumed first and the longer (create) needle is never reported —
    // `cmovieclip_create` would spuriously report "pattern not found". It is
    // instead resolved standalone in `derive_cmovieclip_create` (a
    // single-needle scan can't collide), with full-pattern verification.
    //
    // Wrapper SetPosition (CMovieClip vtable +0x38):
    // `fn(this, x: i32, y: i32)` — converts to floats, forwards to
    // afp_layer_set_position. Complete function; wildcards cover the
    // security-cookie load disp32, the afp_layer_set_position IAT disp32,
    // and the __security_check_cookie CALL rel32. Unique match on both
    // builds: 0x180258DE0 (20260616) / 0x18021CD20 (20260324) —
    // Ghidra-verified 2026-07-12. Used for versus side-binding (first-
    // position x-discrimination); non-fatal if missing.
    SignatureDefinition {
        name: "cmovieclip_set_position",
        pattern: "48 83 EC 38 48 8B 05 ? ? ? ? 48 33 C4 48 89 44 24 28 8B 49 08 66 0F 6E C2 66 41 0F 6E C8 48 8D 54 24 20 0F 5B C0 0F 5B C9 F3 0F 11 44 24 20 F3 0F 11 4C 24 24 FF 15 ? ? ? ? 48 8B 4C 24 28 48 33 CC E8 ? ? ? ? 48 83 C4 38 C3",
        description: "CMovieClip wrapper SetPosition (vtable +0x38) — fn(this, x:i32, y:i32). Versus side-binding detour target for overlay-element-styling.",
    },
    // ── Non-Native OS Support: background-movie DirectShow graph builder ──
    // `me::movie::impl::DShowPlayer::BuildGraph(this, request)` — the ONLY
    // function in gamemdx that touches DirectShow: CoCreateInstance(
    // CLSID_FilterGraph, IID_IGraphBuilder) → AddFilter(custom renderer) →
    // IGraphBuilder::RenderFile (vtbl+0x68). Under Wine/CrossOver, RenderFile's
    // intelligent-connect enumerates audio renderers via devenum → builtin
    // winmm, which access-violates (crash RA = match+0x2B0, right after the
    // RenderFile call). The non-native-os-support mod detours the function
    // entry and, WITHOUT calling the original, fakes the success epilogue's
    // one observable side effect — player state dword +0x8 = 3 ("opened") —
    // then returns 0. The state write is load-bearing: Dx9Movie::update's
    // status machine only advances past "opening" when getState() reads 3,
    // and the demo/gameplay sequences poll that status before starting the
    // song (a plain error-returning stub soft-locks the attract demo at a
    // black screen — live-tested 2026-07-21). The `opened` byte (+0x14) stays
    // 0, so every per-frame path keeps to its guarded early-return and none
    // of the (null) COM interface pointers is ever touched.
    //
    // Pattern = complete prologue + home-stores + the request-flag extraction
    // (`[RDX+0x14] bit0 → this+0x16, bit2 → this+0x17; LEA RSI,[RCX+0x48]`).
    // Every byte is structural (opcodes + struct-offset immediates); no
    // relocations, calls, or jumps in range. Unique single match on both
    // supported builds: 0x18023AE40 (20260616) / 0x180256EB0 (20260324) —
    // Ghidra-verified 2026-07-21. RE record:
    // .agents/planning/20260721-non-native-os-support/.
    SignatureDefinition {
        name: "movie_build_graph",
        pattern: "4C 8B DC 56 57 41 54 41 55 41 56 48 83 EC 40 48 C7 44 24 30 FE FF FF FF 49 89 5B 18 49 89 6B 20 48 8B EA 48 8B F9 8B 42 14 24 01 88 41 16 8B 42 14 C1 E8 02 24 01 88 41 17 48 8D 71 48",
        description: "DShowPlayer::BuildGraph — sole DirectShow filter-graph builder (CLSID_FilterGraph + RenderFile). Detoured by non-native-os-support to fake a successful open (player state 3, no COM) so movies are skipped without crashing Wine (builtin winmm AVs during audio-renderer enumeration) and without soft-locking the movie-status pollers.",
    },
    // ── Background Dancers: Background Movies = STAGE SCREENS ─────────
    // (docs/background_dancers_research.md §8). DDR A3 routed a "monitor"
    // song's movie into layer-table entry 10 — a 1280×1280 canvas whose
    // private command list renders into the OFFSCREEN1 render target the
    // engine publishes at boot as the named texture `offscreen1`, which the
    // screen materials of the monitor*/replicant* stages sample. World kept
    // the whole chain but its `MovieActor` layer choice (20260825
    // `FUN_18007cf90`; 20250805 `FUN_180079280`) never selects entry 10:
    //
    //   48 8B 15 d32       MOV   RDX,[rip+layer_table]   ; match − 0x1B
    //   45 33 C0           XOR   R8D,R8D
    //   C7 40 0C FF FF FF 7F MOV dword [RAX+0xC],0x7FFFFFFF ; match − 0x11 (draw prio)
    //   48 85 D2 / 75 05 / 41 8B D0 / EB 19   (null-table guard)
    //   44 38 81 48 01 00 00 CMP  byte [RCX+0x148],R8B   ; the THUMBNAIL flag
    //   B8 09 00 00 00     MOV   EAX,9                   ; ← imm8 at match+8
    //   49 0F 45 C0        CMOVNZ RAX,R8                 ; thumbnail ⇒ entry 0
    //   48 8D 04 40        LEA   RAX,[RAX+RAX*2]
    //   48 8B 54 C2 08     MOV   RDX,[RDX+RAX*8+8]       ; entry.layer (stride 0x18)
    //
    // `derive_movie_screen_route` publishes the imm byte's address as
    // `movie_layer_select_imm`; the mod rewrites it `09 → 0A` for a routed
    // song only (checked, restored at window exit). Gates: unique; the byte
    // reads `09`; the `MOV RDX,[rip]` at match − 0x1B decodes to the derived
    // `layer_table`; the draw-priority store at match − 0x11. Unique +
    // byte-identical on all five builds (capstone sweep 2026-09-23):
    // 20250805 0x1800792a2 / 20260224 0x1800783e2 / 20260721 0x18007cbd2 /
    // 20260825 0x18007cfb2 / 20260915 0x18007d122.
    SignatureDefinition {
        name: "movie_layer_select",
        pattern: "44 38 81 48 01 00 00 B8 09 00 00 00 49 0F 45 C0 48 8D 04 40 48 8B 54 C2 08",
        description: "MovieActor layer-table entry choice (`thumbnail ? 0 : 9`) — imm8 at +8 (published as movie_layer_select_imm) is the fullscreen entry the STAGE SCREENS route rewrites to 10 (OFFSCREEN1).",
    },
    // The MovieActor's 0x1045 (per-frame, song clock) case (20260825
    // `FUN_18007d250`; the fit call `FUN_18007d030(this, &size, &origin)`
    // runs only while the actor's StackStep is 2):
    //
    //   83 7C C1 58 02     CMP   dword [RCX+RAX*8+0x58],2   ; StackStep == 2
    //   0F 85 rel32        JNZ
    //   0F 10 81 d32       MOVUPS XMM0,[RCX+origin]         ; d32 @ +14 (0x108)
    //   F2 0F 10 89 d32    MOVSD XMM1,[RCX+origin+0x10]     ; d32 @ +22 (0x118)
    //   4C 8D 44 24 20 / 48 8D 54 24 40 / 0F 29 44 24 20
    //   0F 10 81 d32       MOVUPS XMM0,[RCX+size]           ; d32 @ +44 (0x120)
    //   F2 0F 11 4C 24 30
    //   F2 0F 10 89 d32    MOVSD XMM1,[RCX+size+0x10]       ; d32 @ +58 (0x130)
    //
    // Origin = f64 (x, y, z), size = f64 (w, h, d), written only by the ctor
    // (`FUN_18007c960`, from the SceneManageActor's marker rect). Published
    // as `movie_fit_origin_off` / `movie_fit_size_off` (values); the STAGE
    // SCREENS route writes origin (0,0) / size (1280,1280) while the step is
    // ≤ 2 — A3's monitor fit. Gates: unique; +22 == +14 + 0x10; +58 == +44 +
    // 0x10; both below the actor's 0x150 allocation. Unique + identical
    // displacements on all five builds: 20250805 0x180079570 / 20260224
    // 0x1800786b0 / 20260721 0x18007cea0 / 20260825 0x18007d280 / 20260915
    // 0x18007d3f0.
    SignatureDefinition {
        name: "movie_actor_fit_case",
        pattern: "83 7C C1 58 02 0F 85 ?? ?? ?? ?? 0F 10 81 ?? ?? ?? ?? F2 0F 10 89 ?? ?? ?? ?? 4C 8D 44 24 20 48 8D 54 24 40 0F 29 44 24 20 0F 10 81 ?? ?? ?? ?? F2 0F 11 4C 24 30 F2 0F 10 89 ?? ?? ?? ??",
        description: "MovieActor 0x1045 case (step-2 fit) — d32 at +14 = fit origin f64[3] (movie_fit_origin_off), d32 at +44 = fit size f64[3] (movie_fit_size_off); +22/+58 are the +0x10 z/depth halves (identity gate).",
    },
    // ── Announcer / in-game voice dispatcher ──────────────────────────
    // The per-frame announcer body (docs/hex_edit_porting.md Hack 1,
    // 32-bit analog FUN_10047ab0): plays combo callouts
    // (`vo_ingame_combo_%04d`/`_other`), score-state cues
    // (`vo_ingame_state_NN_*`) and stage-clear cheers (`se_kansei_*`) from
    // a single cabinet-wide instance (it reads BOTH sides' combo counters
    // via max()). The announcer-mute mod detours the entry and, when mute
    // is effective, returns without calling the original — silencing every
    // family the research enumerates in one place.
    //
    // Pattern = full entry prologue (MOV RAX,RSP; PUSH RBP; LEA
    // RBP,[RAX-0x5F]; SUB RSP,0xE0; gap-slot store; home stores; XMM6/7
    // saves) + security-cookie load (disp32 wildcarded) + the distinctive
    // state guard `MOVZX EAX,word [RCX+0x82]; MOV ECX,[RCX+RAX*8+0x58];
    // TEST ECX,ECX; JZ`. Unique single match on all four supported builds:
    // 0x180055A50 (20260324 — matches the research doc's dispatcher
    // exactly), 0x180054C80 (20260526), 0x180055470 (20260616),
    // 0x180055430 (20260721). Ghidra-verified 2026-08-16.
    SignatureDefinition {
        name: "announcer_dispatcher",
        pattern: "48 8B C4 55 48 8D 68 A1 48 81 EC E0 00 00 00 48 C7 45 C7 FE FF FF FF 48 89 58 10 48 89 70 18 48 89 78 20 0F 29 70 E8 0F 29 78 D8 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 45 2F 48 8B D9 0F B7 81 82 00 00 00 8B 4C C1 58 85 C9 0F 84",
        description: "In-game announcer/voice dispatcher entry — combo callouts, score-state cues and stage-clear cheer SFX. Detoured by the announcer-mute mod (conditional early-return).",
    },
    // ── Bottom-text (system HUD line) renderer + its blank loop ────────
    // The bottom-of-screen status readouts — centre: network status
    // (ONLINE / CHECKING… / MAINTENANCE / OFFLINE MODE / LOCAL MODE …),
    // the CREDIT / FREE PLAY / EVENT MODE line and the COIN/TOKEN count;
    // corners: P1 / P2 PASELI balances; centre-above (attract idle): the
    // SOFTWARE / SYSTEM / HARDWARE ID lines — are EIGHT persistent text
    // objects held in one 8-slot pointer array, all owned by a single
    // per-frame "system HUD" tick (FUN_18000a9a0 on 20260825). Every
    // frame in ark system status ∈ {3,5,6} the tick calls the RENDERER
    // (FUN_180009630 — the function docs/hex_edit_porting.md Hack 3 names
    // FUN_180009680), which recomposes slots 0/1/2/3/7 and writes each
    // through the text object's `set_text(inner, str, 1)` vcall; in every
    // other status the tick's else-branch writes the game's own "" literal
    // into all eight slots instead. The text objects PERSIST between
    // frames (the else-branch exists precisely to clear them), so hiding
    // = detour the renderer and run that same blank loop in its place
    // (an early-return alone would freeze the last-drawn strings).
    //
    // `bottom_text_render` = the renderer's full relocation-free prologue
    // (MOV R11,RSP; home R12; PUSH RBP; LEA RBP,[R11-0xA8]; SUB
    // RSP,0x1A0; XMM6 save; security cookie load (disp wildcarded); home
    // RBX/RDI; the `memset(buf+1, 0, 0xFF)` arg setup + `MOV byte
    // [RBP-0x80],0`) through the CALL opcode. Ghidra-verified single hit:
    // 0x180009220 (20250805), 0x1800096A0 (20260224), 0x180009630
    // (20260825); byte-identical body shape on all three.
    SignatureDefinition {
        name: "bottom_text_render",
        pattern: "4C 8B DC 4D 89 63 20 55 49 8D AB 58 FF FF FF 48 81 EC A0 01 00 00 41 0F 29 73 E8 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 85 80 00 00 00 49 89 5B 08 48 8D 4D 81 33 D2 41 B8 FF 00 00 00 49 89 7B 18 C6 45 80 00 E8",
        description: "Bottom-of-screen status-text renderer entry (credit/coin/PASELI/EVENT MODE/FREE PLAY + the centre network-status line; FUN_180009630 on 20260825). Detoured by services::bottom_text — when any contributor asks to hide, the callback skips the original and blanks the eight text objects with the game's own \"\" literal instead.",
    },
    // The system-HUD tick's else-branch — `TEST BPL,BPL; JZ +7; CALL
    // renderer; JMP epilogue; LEA RBX,[slots]; MOV EDI,8; loop { MOV
    // RAX,[RBX]; TEST; JZ; MOV RAX,[RAX+0x18]; MOV RCX,[RAX]; MOV
    // RAX,[RCX]; MOV R8D,R14D(=1); LEA RDX,[""]; CALL [RAX+0x10]; ADD
    // RBX,8; DEC RDI; JNZ }`. derive_bottom_text reads: the CALL rel32 at
    // match+5 (must equal `bottom_text_render` — identity cross-check that
    // the AOB'd renderer IS the function this tick feeds), the RIP disp32
    // at match+15 → `bottom_text_slots` (the 8 × ptr array,
    // DAT_1806f2c90 on 20260825), the imm32 at match+20 (must be 8) and
    // the RIP disp32 at match+48 → `bottom_text_empty_str` (the game's ""
    // literal, DAT_1802dda70). The `+0x18` holder offset, the `[RCX]`
    // inner deref, the `+0x10` vtable slot and the `1` flag are LITERAL in
    // the pattern, so the service's replica of the loop is attested by the
    // match itself. Single hit: 0x18000A86C (20250805), 0x18000ACEC
    // (20260224), 0x18000AC7C (20260825).
    SignatureDefinition {
        name: "bottom_text_blank_loop",
        pattern: "40 84 ED 74 07 E8 ?? ?? ?? ?? EB 5E 48 8D 1D ?? ?? ?? ?? BF 08 00 00 00 48 8B 03 48 85 C0 74 17 48 8B 40 18 48 8B 08 48 8B 01 45 8B C6 48 8D 15 ?? ?? ?? ?? FF 50 10 48 83 C3 08 48 FF CF 75 D8",
        description: "System-HUD tick else-branch that blanks all eight bottom-text objects (`set_text(**(slot+0x18), \"\", 1)` × 8). Source of the derived `bottom_text_slots` (RIP at +15) and `bottom_text_empty_str` (RIP at +48); the CALL at +5 cross-checks `bottom_text_render`.",
    },
    // ── Split SSQ Auto-Discovery: the SSQ path builder ────────────────
    // `void build_ssq_path(char out[0x100], const char* basename, int
    // difficulty)` — the ONE function that decides which `<basename>[_N].ssq`
    // file holds a chart, via a hardcoded per-build `repe cmpsb` song table
    // (RE: `docs/split_ssq_research.md`). Pattern = prologue (`MOV
    // [RSP+8],RSI; PUSH RDI; SUB RSP,0x30; MOV R10,RCX`) + the first table
    // cell (`LEA RDI,["acef"]` disp32 wildcarded; `MOV RSI,RDX; MOV ECX,5;
    // REPE CMPSB`) + `JZ rel32` opcode — the only unconditional-match cell
    // (`acef` splits at every difficulty), present since 20250805. The
    // inner `48 8B F2 B9 05 ...` fragment repeats ~30× in the body, so the
    // `4C 8B D1` prefix is load-bearing. Exactly one hit on all four builds:
    // 0x18019E8D0 (20250805), 0x1801A1730 (20260224), 0x1801B43F0
    // (20260721), 0x1801B4090 (20260825). The consumer detours the entry
    // and reads NOTHING at match+N (body size varies 0x3A9..0x70F with the
    // table). Third-party hex-edited 20250805 DLLs rewrite this prologue —
    // a miss there skips the mod cleanly.
    SignatureDefinition {
        name: "build_ssq_path",
        pattern: "48 89 74 24 08 57 48 83 EC 30 4C 8B D1 48 8D 3D ?? ?? ?? ?? 48 8B F2 B9 05 00 00 00 F3 A6 0F 84",
        description: "SSQ path builder entry (basename + difficulty → data/mdb_apx/ssq/<basename>[_N].ssq via a hardcoded split-song table). Detoured by split-ssq-auto-discovery (full replacement).",
    },
    // ── Song playback speed: identity-only runtime transaction ────────
    // Byte-level evidence and per-build addresses live in
    // `.agents/planning/2026-08-05-song-playback-speed/research/runtime-integration.md`.
    // `derive_song_rate_runtime_sites` re-scans all three for uniqueness and
    // validates the clock's exact eight-byte redirect window before publishing
    // the derived patch address. These signatures are inert until Step 3's
    // later installation tasks consume them.
    SignatureDefinition {
        name: "song_rate_clock_anchor",
        pattern: SONG_RATE_CLOCK_ANCHOR_PATTERN,
        description: "Per-frame authoritative music_count calculation. The exact eight-byte `LEA R14D,[RAX+RBX]; LEA R12,[RDI+0x58]` redirect window is at match+0x25 and is derived only after literal-byte validation.",
    },
    SignatureDefinition {
        name: "song_rate_clock_anchor_v1",
        pattern: SONG_RATE_CLOCK_ANCHOR_V1_PATTERN,
        description: "song_rate_clock_anchor without the XOR EDX,EDX (20250805 / 20260224 codegen); redirect window at match+0x23. `derive_song_rate_runtime_sites` publishes whichever shape matched as `song_rate_clock_anchor`.",
    },
    SignatureDefinition {
        name: "song_rate_wavebank_create",
        pattern: SONG_RATE_WAVEBANK_CREATE_PATTERN,
        description: "audio wavebank_create(i32 file_id) -> bool entry. Returns true only after native open, XACT streaming-bank acceptance, manager insertion, and DoWork.",
    },
    SignatureDefinition {
        name: "song_rate_wavebank_unregister",
        pattern: SONG_RATE_WAVEBANK_UNREGISTER_PATTERN,
        description: "Manager-level wave-bank unregister(int file_id) entry called by audio::XwbFileCallback's unload vslot after file-manager unload; releases XACT/native bookkeeping for the exact file id.",
    },
    // ── Song playback speed: XACT file-IO callback registration site ──
    // The audio-manager constructor builds XACT_RUNTIME_PARAMETERS with
    // `lookAheadTime = 0xFA` (250 ms) followed by three LEA/MOV pairs
    // storing the notification, readFile, and getOverlappedResult callback
    // pointers. The streaming rate engine (design req 9) detours the second
    // and third — RIP-decoded from this match by
    // `derive_song_rate_io_callbacks`, which also derives the handle→file_id
    // lookup helper (first CALL inside the readFile body) and the audio
    // file-table global (from the unregister match). Verified to match
    // EXACTLY ONCE on four builds (20260324 0x1801a81a4 / 20260421
    // 0x1801a8e74 / 20260616 0x1801a9ca4 / 20260721 0x1801aad34) —
    // Ghidra-verified 2026-08-10. Cross-version table + evidence:
    // `docs/xact_streaming_research.md` §6. Fail-open: never in the required
    // set — unresolved means the streaming integration stays structurally
    // absent and the DLL boots stock.
    SignatureDefinition {
        name: "song_rate_io_callback_regsite",
        pattern: SONG_RATE_IO_CALLBACK_REGSITE_PATTERN,
        description: "Audio-manager ctor XACT callback registration (lookAheadTime 0xFA + 3x LEA/MOV). LEAs 2+3 RIP-decode to the readFile (FUN_1801aa250 on 20260721) and getOverlappedResult (FUN_1801aa350) callbacks — the streaming rate engine's detour pair (design req 9).",
    },
    // ── Audio (XACT 2) — assist-tick bank registration + cue playback ──
    //
    // The game's audio is COM-instantiated Microsoft XACT 2, wrapped by an
    // in-house "audio manager" singleton owning six sound-bank slots plus a
    // small "play a sound effect" façade. `services::game_audio` parks a
    // mod-owned sound bank in a free slot and plays cues through the façade.
    //
    // All three patterns verified to match EXACTLY ONCE on four builds
    // (20260721 / 20260616 / 20260421 / 20260324); `derive_game_audio_addresses`
    // re-checks the match count at boot. Wildcards are deliberate: every
    // address, RIP displacement, stack-frame displacement and branch
    // displacement is `??`, while semantic immediates are literal so a
    // meaningful change to the game breaks the match instead of silently
    // mis-resolving. RE record (byte-level authority for these patterns, with
    // per-build match addresses):
    // .agents/planning/20260725-assist-tick/research/bank-slot-and-anchors.md
    // → "Proposed signatures" (S1/S2/S3) and "Derivation chains" (A/B/C).
    SignatureDefinition {
        name: "se_play",
        pattern: "40 57 48 83 EC 40 48 C7 44 24 ?? FE FF FF FF 48 89 5C 24 ?? 0F 29 74 24 ?? 0F 28 F2 48 8B FA 8B D9 44 8B C1 41 FF C8 74 ?? 41 83 F8 04 74 ?? C7 44 24 ?? 05 00 00 00 48 8D 4C 24 ?? FF 15",
        description: "Public sound-effect play façade — u32(i32 bank_id /*ECX*/, const char* cue /*RDX*/, f32 pan /*XMM2, NOT R8D*/); returns a handle, 0xFFFFFFFF on failure. Match is the function entry. The distinctive core is the `bank_id ∈ {1,5} skips the SE mute filter` ladder (DEC R8D; JZ; CMP R8D,4; JZ) followed by the literal mute-filter state 5; pattern stops at the `FF 15` opcode so the mute-filter disp32 is excluded. Cross-checked by decoding its first CALL rel32, which must land on the derived se_play_inner.",
    },
    SignatureDefinition {
        name: "se_play_inner_body",
        pattern: "48 8B 35 ?? ?? ?? ?? 48 63 F9 0F 29 74 24 ?? 48 8D 47 01 0F 28 F2 48 03 C0 48 8B 1C C6 48 85 DB 74 ?? 48 8B 03 48 8B CB FF 10 B9 FF FF 00 00 66 3B C1 74 ?? 4C 8B 13 48 8D 4C 24 ?? 45 33 C9 48 89 4C 24 ?? 45 33 C0 0F B7 D0 48 8B CB 48 C7 44 24 ?? 00 00 00 00 41 FF 52 20",
        description: "Landmark inside se_play_inner (match = entry + 0xF), the sole anchor for the audio-manager global: `MOV RSI,[rip+audio_manager]` with the disp32 at match+3 — that global's absolute address MOVES ON EVERY GAME BUILD, so it must be RIP-decoded from here and never scanned or hardcoded. Also yields se_play_inner itself at match-0xF. The pattern must run all the way to `41 FF 52 20` (SoundBank::Play, vtable +0x20): the neighbouring se_prepare_inner is byte-for-byte identical for its first ~0x65 bytes apart from that displacement and its vtable index (`41 FF 52 18`), so a shorter pattern matches both. The `48 8D 47 01 / 48 03 C0 / 48 8B 1C C6` triple encodes the 0x10 slot-array stride and is kept literal so a layout change fails the match instead of mis-indexing.",
    },
    SignatureDefinition {
        name: "bank_slot_of_file_loop",
        pattern: "4C 8B 0E 48 83 C9 FF 33 C0 49 8B F9 48 8B D5 F2 AE 48 F7 D1 4C 8D 41 FF 49 8B C9 E8 ?? ?? ?? ?? 85 C0 74 ?? FF C3 48 83 C6 10 83 FB ?? 72 ?? B8 05 00 00 00",
        description: "Name-match loop of bank_slot_of_file(file_id) -> slot, which maps a loaded bank file's basename to a manager slot and returns {0,1,2,3} on a hit or the literal fallback 5 on a miss — so slot 4 is unreachable, which is what makes it claimable. The imm8 at match+0x2C is the NUMBER OF NAMED BANKS and must read 4: a build that added a fifth named bank would map it to slot 4 and silently collide with our bank. Read as a boot-time safety gate (guard G1) rather than assumed. The `B8 05 00 00 00` fallback is literal so a change there breaks the match.",
    },
    // ── Per-side player Option (assist-tick JUDGMENT TIMING) ─────────
    // The per-frame gameplay count computation (FUN_18005f100 on 20260324)
    // reaches each side's `ddr::player::Option` exactly like this:
    //
    //   MOVSXD RCX,[RBP+0x84]              ; actor's play side (0/1)
    //   MOV    [RBP+0x168],EAX             ; beat count store
    //   MOV    EAX,[RBP+0x184]             ; RENDER_OFFSET
    //   LEA    R12,[rip+disp32]            ; -> the MODULE BASE (validated)
    //   SUB    EAX,[RBP+0x170]             ; − INPUT_OFFSET
    //   XOR    EDX,EDX
    //   ADD    EAX,ESI
    //   MOV    [RBP+0x17C],EAX             ; dispMusicCount
    //   MOV    RCX,[R12+RCX*8+disp32]      ; ctx-table[side]; disp32 = table RVA
    //   CALL   option_accessor             ; returns *ctx + 0xE0 (the Option)
    //
    // The actor-field displacements (0x84/0x168/0x170/0x17C/0x184) are kept
    // literal — they are the same layout facts the timing-offsets mod rests
    // on, and a layout change SHOULD fail this match. The LEA/table/call
    // displacements are wildcarded (they move every build); the derivation
    // (`derive_player_option_table`) validates the LEA target against the
    // module base before trusting the table RVA. Verified to match exactly
    // once on 20260324 / 20260421 / 20260616 / 20260721.
    //
    // 20250805 (0x18005B58E) and 20260224 (0x18005A5CE) emit the identical
    // sequence WITHOUT the `33 D2` XOR EDX,EDX between the SUB and the ADD —
    // `player_option_ctx_load_v1` below. Same table (it is the
    // player_work_table — `*(table[side]) + 0xE0` is PlayerWork's inlined
    // Option), table disp32 at match+44 instead of +46.
    SignatureDefinition {
        name: "player_option_ctx_load",
        pattern: "48 63 8D 84 00 00 00 89 85 68 01 00 00 8B 85 84 01 00 00 4C 8D 25 ?? ?? ?? ?? 2B 85 70 01 00 00 33 D2 03 C6 89 85 7C 01 00 00 49 8B 8C CC ?? ?? ?? ?? E8",
        description: "Per-side context-table load inside the per-frame count computation. Yields the derived player_option_table (base + the MOV's disp32); each side's ddr::player::Option is *(table[side]) + 0xE0, JUDGMENT TIMING (timing_music, ±100 ms) at Option+0x24. RE record: .agents/planning/20260729-assist-tick-premixed-track/research/ra-rb-timing-chain.md.",
    },
    SignatureDefinition {
        name: "player_option_ctx_load_v1",
        pattern: "48 63 8D 84 00 00 00 89 85 68 01 00 00 8B 85 84 01 00 00 4C 8D 25 ?? ?? ?? ?? 2B 85 70 01 00 00 03 C6 89 85 7C 01 00 00 49 8B 8C CC ?? ?? ?? ?? E8",
        description: "player_option_ctx_load without the XOR EDX,EDX (20250805 / 20260224 codegen). Table disp32 at match+44. Consumed by derive_player_option_table when the primary anchor misses.",
    },
    // ── Song-select preview restart (preview design §Components 5–6) ──
    // The four addresses the live-edit restart executor is built on: two
    // vftable identity gates (View / AudioLoader — the runtime guards that
    // make the compile-time struct offsets fail-closed across builds) and
    // two stock functions the executor calls in their stock roles (the
    // cue-handle stop and the load-completion create router, whose XWB arm
    // lands on the detoured wavebank_create so the re-create composes with
    // the preview bind branch for free).
    //
    // All four patterns verified to match EXACTLY ONCE on four builds
    // (20260721 / 20260616 / 20260421 / 20260324); `derive_preview_restart`
    // re-checks the match counts at boot. Wildcards per house style: every
    // RIP disp32, CALL rel32, stack-frame displacement and branch
    // displacement is `??`; semantic immediates and struct-field offsets
    // stay literal so a layout change breaks the match instead of silently
    // mis-resolving. Byte-level authority (annotated disassembly, per-build
    // match table): .agents/planning/2026-08-15-song-preview-rate/research/
    // preview-retrigger-re.md §9. Any miss disables only the preview
    // feature's restart half (declared through `preview::init_restart`,
    // never a mod's `required_signatures` — design R9/R11).
    SignatureDefinition {
        name: "audio_loader_ctor",
        pattern: "48 8D 05 ?? ?? ?? ?? 48 89 01 48 C7 41 08 FF FF FF FF C7 41 10 FF FF FF FF C6 41 14 00 0F B6 45 ?? 88 41 15 89 51 18",
        description: "Field-init cluster inside sequence::AudioLoader's ctor (match = entry+0x3F on 20260721): the vftable install LEA (disp32 at match+3 — the derivation's RIP-decode source) followed by the loader-layout facts the restart executor's constants rest on, kept literal as the layout gate: XWB/XSB file ids = -1,-1 (+0x08 qword), cue handle = -1 (+0x10), failed = 0 (+0x14), mode from the stack arg (+0x15, its RBP disp8 wildcarded), slot (+0x18). Yields the derived audio_loader_vftable (ONE virtual slot: the per-frame tick that fires se_play exactly once and re-arms when the handle is set back to -1).",
    },
    SignatureDefinition {
        name: "selectmusic_view_ctor",
        pattern: "48 89 5C 24 ?? 57 48 83 EC 20 48 8B D9 E8 ?? ?? ?? ?? 33 FF 48 8D 05 ?? ?? ?? ?? 4C 8D 1D ?? ?? ?? ?? 48 8D 8B E8 01 00 00 48 89 43 28 4C 89 1B 48 89 BB C0 00 00 00 48 8D 05 ?? ?? ?? ?? 48 89 83 C8 00 00 00 48 89 BB D0 00 00 00 C6 83 D8 00 00 00 01 48 C7 83 F8 00 00 00 0F 00 00 00",
        description: "sequence::selectmusic::View ctor head (match = entry). The View vftable is the SECOND LEA (`4C 8D 1D`, R11, disp32 at match+30) stored bare to [RBX] by `4C 89 1B` — the FIRST LEA (disp32 at match+23) is an inner interface vftable stored at +0x28, not the View's own. Literal layout pins: the +0x28 store, the [RBX+0x1E8] member LEA, the +0xC0/+0xD0 pointer clears, the third LEA's store to +0xC8 — the embedded sequence::AudioPlayer, THE load-bearing offset the loader-chain walk (child+<derived selectmusic_view_child_offset> -> View -> +0xC8+0x08 loader) rests on — plus +0xD8=1 and the +0xF8=0xF string-capacity init. Yields the derived selectmusic_view_vftable (the walk's identity gate) and, via its single CALL site, the derived selectmusic_view_child_offset (the View* field on the SelectMusicSequence: +0x90 through 20260324, +0xB8 from 20260421).",
    },
    SignatureDefinition {
        name: "cue_handle_stop",
        pattern: "40 53 48 83 EC 30 48 C7 44 24 ?? FE FF FF FF 8B D9 48 8D 05 ?? ?? ?? ?? 48 89 44 24 ?? 8B 0D ?? ?? ?? ?? 85 C9 7E ?? FF 15 ?? ?? ?? ?? 8B 0D ?? ?? ?? ?? 83 FB FF 74 ?? 48 8D 43 05 48 C1 E0 05 48 03 05 ?? ?? ?? ?? 74 ?? 4C 8B 00 4D 85 C0 74 ?? 49 8B 00 33 D2 49 8B C8 FF 50 08 EB ?? 48 83 78 08 00 74 ?? 48 8B 48 08 48 8B 01 BA 01 00 00 00 FF 50 10",
        description: "cue_handle_stop(i32 handle) entry — the game's own teardown stop (AudioLoader::release uses it on the stored handle; a dead/stale handle is a safe no-op inside it). Distinctive body kept literal: the `CMP EBX,-1` guard, the handle-table indexing `LEA RAX,[RBX+5]; SHL RAX,5` ((h+5)*0x20 against the rip-loaded table global, disp wildcarded), and both dispatch arms — live cue => vt+0x08 Stop(0) (`FF 50 08`), dead entry => soundbank vt+0x10 with flags=1 (`BA 01 00 00 00` + `FF 50 10`). The lock prologue's globals/import disps are wildcarded. Restart executor step 2.",
    },
    SignatureDefinition {
        name: "sound_bank_create_router",
        pattern: "40 53 48 83 EC 30 48 C7 44 24 ?? FE FF FF FF 48 63 D9 48 8D 05 ?? ?? ?? ?? 48 89 44 24 ?? 8B 0D ?? ?? ?? ?? 85 C9 7E ?? FF 15 ?? ?? ?? ?? 90 48 8D 0C 9B 48 C1 E1 05 48 8B 05 ?? ?? ?? ?? 48 03 48 28 0F B6 81 8F 00 00 00 48 8D 4C 08 11 41 B8 03 00 00 00 48 8D 15 ?? ?? ?? ?? E8 ?? ?? ?? ?? 8B CB 85 C0 75 ?? E8 ?? ?? ?? ?? 0F B6 D8 EB ?? E8 ?? ?? ?? ?? 0F B6 D8",
        description: "sound_bank_create_router(i32 file_id) entry — the FileManager 'sound'-category load-completion callback's bank creator: path extension 'xsb' => sound-bank create, anything else => wavebank_create (the detoured entry — calls into the router land on the patched function, so the restart's re-create re-qualifies through the preview bind branch for free). Distinctive FileManager row walk kept literal: `LEA RCX,[RBX+RBX*4]; SHL RCX,5` (0xA0 row stride), rows base at [mgr+0x28], path length byte at row+0x8F, the extension backset LEA (+0x11), and the strncmp('xsb', 3) setup; both dispatch CALL rel32s and every global disp wildcarded (the post-lock `90` NOP is present on all four builds). Restart executor step 4.",
    },
    // ── Custom Resolution (mods/custom_resolution; planning
    //    .agents/planning/2026-09-05-arbitrary-resolution) ──────────────
    SignatureDefinition {
        name: "display_backbuffer_dims",
        pattern: "80 79 12 00 48 8B F1 74 16 C7 05 ?? ?? ?? ?? 00 05 00 00 C7 05 ?? ?? ?? ?? D0 02 00 00 EB 14 C7 05 ?? ?? ?? ?? 80 02 00 00 C7 05 ?? ?? ?? ?? E0 01 00 00",
        description: "Back-buffer size selector inside the display-init function (FUN_1801ef6d0 on 20260616): `CMP byte [RCX+0x12],0` (the onBoot display struct's HD flag = machineType ∉ {0,1}) then the HD branch `MOV [screen_w],0x500; MOV [screen_h],0x2d0` and the SD else-branch `0x280/0x1e0`. These two globals are the ONLY origin of the physical resolution (D3DPRESENT_PARAMETERS BackBufferWidth/Height, the display surface, the DISPLAY viewport, the SYSTEM/DEBUG_DIALOG lists, the letterbox dst rect). Consumers read: imm32 at match+15/+25 (HD w/h) and +37/+47 (SD w/h) — custom_resolution rewrites all four to the OUTPUT dims so the machine type stops mattering; RIP disp32 at match+11/+21 → derived screen_w_global / screen_h_global. Unique on 20250805/20260224/20260721/20260825 + the live install.",
    },
    SignatureDefinition {
        name: "window_client_size",
        pattern: "66 C7 45 ?? 01 00 C7 45 ?? 00 05 00 00 C7 45 ?? D0 02 00 00 48 89 7D ?? 48 89 7D ?? C7 45 ?? 00 00 01 00",
        description: "The application window descriptor init in the game's main (FUN_180003bf0 on 20260825): `MOV word [RBP+d],1` then the CLIENT size `MOV dword [RBP+d],0x500; MOV dword [RBP+d],0x2d0` (imm32 at match+9 / +16), followed by the 0x10000 flags word. The descriptor reaches `AdjustWindowRectEx` + `CreateWindowExW` (FUN_18021e120) BEFORE display init, so it is the one place the window's client size is decided; the fullscreen path later restyles the window to the desktop, but under spice2x `-w` (which swallows every later SetWindowPos for MDX) this IS the presented client size — a non-720p back-buffer gets stretched into it unless custom_resolution rewrites both imms to the OUTPUT dims (its `window_client_sites`). RBP disp8s wildcarded.",
    },
    SignatureDefinition {
        name: "render_surface_hoist",
        pattern: "45 33 C9 45 8D 41 15 41 BF 00 05 00 00 41 8B D7 41 8B CF E8",
        description: "Inside the render-surface object ctor (FUN_1801f01a0 on 20260616, the 0x170-byte DAT_1806f1ef0): the compiler hoisted `MOV R15D,0x500` (imm at match+9) and, at match+0x2B, `MOV ESI,0x2d0` (imm +0x2C), then reuses both for EVERY 1280/720 surface-create argument (depth A/B, colour, render_color, render_depth, RENDER msaa pair, resolve, D24R readable depth; OFFSCREEN1 = R15D×R15D). The `E8` at match+0x13 is the first surface create `(R15D, R15D, 0x15)` = `FUN_180250950(u16 w, u16 h, u32 fmt, u32 msaa, opts*)`. The ctor body after the match also carries the RT-struct dim immediates custom_resolution::sites scans for (`C7 41 14 00 05 D0 02` ×3, `C7 41 14 00 05 00 05` ×1, `C7 40 16 D0 02 00 00` ×2 — first = PRESENT rt[0x10]). Unique on all four builds.",
    },
    SignatureDefinition {
        name: "list_viewport_table",
        pattern: "44 0F 28 2D ?? ?? ?? ?? 48 8D 05 ?? ?? ?? ?? 48 89 85 ?? ?? 00 00 C7 85 ?? ?? 00 00 00 05 00 00 C7 85 ?? ?? 00 00 D0 02 00 00",
        description: "Start of the eight-entry `{name*, w, h}` stack table in the ScreenCommandList viewport builder (FUN_1801f5d10 on 20260616): the MOVAPS XMM13 constant load that precedes entry 0 (`LEA RAX,[name]; MOV [RBP+d],RAX; MOV dword [RBP+d+8],0x500; MOV dword [RBP+d+0xC],0x2d0`). The MOVAPS prefix is what makes the hit unique — the bare store pair also matches entry 1. custom_resolution::sites::viewport_pairs scans the ≤0x140-byte window after the match for every `C7 85 disp32 imm32` with imm ∈ {0x500,0x2d0} paired by (disp, disp+4): 5 wide pairs (FRONT/MIDDLE/BACK/OFFSCREEN0/RENDER_CAPTURE) + 1 square pair (OFFSCREEN1); SYSTEM and DEBUG_DIALOG take the screen dims from registers and are untouched. Unique on all four builds.",
    },
    SignatureDefinition {
        name: "letterbox_rect_fn",
        pattern: "48 89 5C 24 08 48 89 74 24 10 48 89 7C 24 18 44 8B 0D ?? ?? ?? ?? 4C 8B 05 ?? ?? ?? ?? 33 C0 44 8B DA 48 8B D9 44 8B D0 45 85 C9 74 ?? 45 8B 10 41 8B 48 04 BA 00 05 00 00 8B F8 8B F0 44 8B C8 41 B8 01 00 00 00 44 3B D2 74 ?? 45 3B D8 75 ?? B8 A0 00 00 00 BA 60 04 00 00",
        description: "Entry of the present-chain letterbox/crop rect function `(this, int mode)` (FUN_1801f3f60 on 20260616) — the engine's own SD scaler: `screen_w == 0x500` ⇒ 1:1 POINT copy; mode 1 ⇒ 960-px centre crop (src x 0xA0..0x460, LINEAR); else width-fit letterbox. Called by the present-chain ctor, by the TEST-menu entry with mode 0, and with mode 1 at every scene transition (FUN_18002e7b0 ×13) — so an operator-chosen mode must be re-asserted by a detour on THIS entry, never by a one-shot write. Consumers read: detour target = match; `MOV EDX,0x500` imm at match+0x35 (src x1 AND the equality comparand — patched to the render width so the 1:1 branch fires exactly when render == output); `MOV dword [RBX+0x298],0x2d0` (src y1) at match+0xDD via sites::letterbox_sites. Unique on all four builds.",
    },
    SignatureDefinition {
        name: "scissor_handler",
        pattern: "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 20 48 8B 01 33 F6 48 8B FA 48 8B D9 8D 6E 18 48 39 70 30 74 ?? 44 8B 40 24 45 85 C0 74 ?? 48 8B 49 08 8B 50 20 48 81 C1 08 02 00 00 E8",
        description: "Entry of the ScreenCommandList walker's tag-0x0C (scissor) handler `(walker_ctx**, record*)` (FUN_180269080 on 20260616): emits gd 0x11 (scissor-enable render state) then gd 0x18 with the record's `{u16 x,y,w,h}` (record+6..+0xE) copied VERBATIM — render-target pixels, no canvas scaling — into what becomes `SetScissorRect`. The 2D draw handlers convert canvas→NDC via the walker's context (`*ctx`: +0x00 offset/rt, +0x10 1/canvas) and the segment header's viewport dims (`ctx[1]+0x144/+0x146`), so with a render target ≠ 1280×720 every scissored layer clips wrong; custom_resolution::scissor detours this entry and rescales the record before the original runs. Prologue kept literal incl. the `LEA EBP,[RSI+0x18]` (gd tag 0x18 constant), the +0x30/+0x24 walker checks and the `ADD RCX,0x208` emitter offset. Unique on all four builds.",
    },
    SignatureDefinition {
        name: "debug_font_scale",
        pattern: "48 89 5C 24 10 57 48 83 EC 20 49 8B D8 48 8B FA 85 C9 0F 84 ?? ?? ?? ?? FF C9 74 ?? FF C9 74 ?? C7 02 00 00 80 3F 41 C7 00 00 00 80 3F",
        description: "Entry of the ark draw-callback API's font-scale chooser `(int size_class, float* sx, float* sy)` (FUN_1800066d0 on 20260825), called ONLY by that API's createFont (which stores the pair into the agcs text object's scale fields +0x58/+0x5C). Body: class 0/1/2 each ask arkMDXGetMachineType and pick a 480-line table value (machine 0/1 = SD cabinet: 0.95/0.8/0.75×0.7) or a 720-line one (1.5/1.2/1.0); any other class = 1.0/1.0 — the `C7 02 / 41 C7 00 0x3F800000` default pair kept literal. The pixel size is thus fixed per machine type and never tracks the back-buffer: custom_resolution::debug_ui detours the entry and multiplies both outputs by output_h/ref_h so the TEST menu / hardware check / error screens stay the same on-screen proportion at every output height. Unique on all four builds.",
    },
    SignatureDefinition {
        name: "debug_sprite_create",
        pattern: "4C 8B DC 55 56 57 48 83 EC 70 48 C7 44 24 38 FE FF FF FF 49 89 5B 18 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 44 24 68 48 8B F2 48 8B E9 49 C7 43 D0 0F 00 00 00 33 DB 49 89 5B C8 88 5C 24 40 33 C0 48 83 C9 FF 48 8B FD F2 AE 48 F7 D1 4C 8D 41 FF 48 8B D5 49 8D 4B B8 E8 ?? ?? ?? ?? 90 4C 8D 44 24 40 48 8D 54 24 28 48 8B 0D ?? ?? ?? ?? E8",
        description: "Entry of the ark draw-callback API's createSprite `(const char* name, sprite** out)` (FUN_180007250 on 20260825): std::string(name) → equal_range in the debug-texture map (global disp wildcarded) → 0x10 sprite `{texture*, float scale}`; the scale at +0x08 is 1.0 (720-line table) or 0.8 when arkMDXGetMachineType ∈ {0,1} (SD table) — unless the name is `screenCheck`, always 1.0. drawSprite sizes the quad `texture_px × scale` in PHYSICAL pixels, so like the fonts it never tracks the back-buffer; custom_resolution::debug_ui detours the entry and multiplies `(*out)->scale` post-original. `*out` is NULL for an unknown name (the detour checks). Prologue + inline strlen (`REPNE SCASB`) + the two CALL rel32s kept with wildcards; unique on all four builds.",
    },
    // ── 2-Player BPL Mode (two_player_bpl_mode) ─────────────────────────
    SignatureDefinition {
        name: "battle_frame_ctor",
        pattern: "48 89 4C 24 08 56 57 41 54 41 55 41 56 48 83 EC 40 48 C7 44 24 30 FE FF FF FF 48 89 5C 24 78 48 89 AC 24 80 00 00 00 45 0F B6 D1 49 8B D8 4C 8B DA 4C 8B F1 33 ED 48 89 69 08 48 89 69 10 48 89 69 18 48 89 69 20 89 69 28 40 88 69 2C",
        description: "Entry of `sequence::dance::MatchingBattleFrameActor::MatchingBattleFrameActor(this, layoutDesc, GamePlayActor** actors, u8 isEx /*R9B*/, i32 isDouble, i32 mcode, i32 diff)` (FUN_180071740 on 20260825, FUN_18006df80 on 20250805 — byte-identical): the relocation-free prologue through the agcs::Actor base zeroing (`MOVZX R10D,R9B` = isEx, `MOV RBX,R8` = actors, `MOV R11,RDX` = layoutDesc). Consumers (two_player_bpl_mode) call the match as the ctor; derive_two_player_bpl reads the SECOND `48 8D 05 disp32` in the body (the class vftable store, match+0x78) as an identity cross-check against the RTTI-resolved `battle_frame_actor_vtable`, and the first `48 63 05 disp32` (`MOVSXD RAX,[rip]` = CNetworkManager's local cabinet index, match+0x228) → `matching_local_cabinet_idx`. Stock's only caller is MatchingDancePlaySequence::onUpdate state 2; the mod calls it from the NORMAL DancePlaySequence with a mod-owned vtable clone installed afterwards. Unique on all four builds.",
    },
    SignatureDefinition {
        name: "actor_add_child",
        pattern: "48 3B CA 74 63 48 85 D2 74 5E 48 83 7A 08 00 75 57 48 83 7A 10 00 75 50 48 8B 41 18 45 33 C0 48 85 C0 74 16 44 8B 4A 28 44 39 48 24 76 0C 4C 8B C0 48 8B 40 10 48 85 C0 75 EE",
        description: "Entry of `agcs::Actor::addChild(parent RCX, child RDX)` (FUN_18021f230 on 20260825): refuses as a silent no-op when child == parent, child NULL, child already has a parent (+0x08) or a next sibling (+0x10); otherwise walks the parent's child list (+0x18 first / +0x10 next) comparing the sibling's effective priority (+0x24) against the child's requested priority (+0x28) and splices the child BEFORE the first sibling whose priority is ≤ its own (newest-first among equals), then sets child+0x08 = parent and child+0x24 = child+0x28. Consumers (two_player_bpl_mode) call the match to attach the re-hosted MatchingBattleFrameActor to the live DancePlaySequence — the same call the matching sequence makes — and verify child+0x08 == parent afterwards because the refusals are silent. Whole body up to the splice kept literal; unique on all four builds.",
    },
    SignatureDefinition {
        name: "dance_matching_slot_probe",
        pattern: "48 8B 05 ?? ?? ?? ?? 48 8B 08 4C 8B A9 ?? ?? ?? ?? 48 8B 05",
        description: "Inside `MatchingBattleFrameActor::onInitialize` (FUN_180071ce0 on 20260825, match at +0x8E): the package-map-miss default `MOV RAX,[rip+scene_resource_manager]; MOV RCX,[RAX]; MOV R13,[RCX+slot_off]` that fetches the resident `dance_matching` package pointer (scene-resource slot 31, `slot_off` = 0x7F0 on 20260825), immediately followed by the GameWork load `MOV RAX,[rip+…]`. derive_two_player_bpl requires the match inside `[vtable[4], +0x200)` of the RTTI-resolved frame vtable, RIP-decodes match+3 → `scene_resource_manager` (global holding the manager object pointer; the manager's FIRST field is the slot array) and publishes the imm32 at match+13 as `dance_matching_slot_off`. Consumers pre-check `*(**scene_resource_manager + slot_off) != 0` — THREE loads, exactly the stock chain (the first cabinet build missed the middle hop and read past the 0x28-byte manager object, 2026-09-10) before letting the stock onInitialize run — a NULL package makes stock NULL-deref on the failed `main_single` clip create. Unique on all four builds.",
    },
    SignatureDefinition {
        name: "gpa_score_select",
        pattern: "80 B8 ?? ?? 00 00 00 74 08 48 05 ?? ?? 00 00 EB 06 48 05 ?? ?? 00 00 8B 00",
        description: "The per-frame score read in `MatchingDancePlaySequence::onUpdate` state 0xB (FUN_180061cc0+0xC28 on 20260825): `CMP byte [GamePlayActor+isEx],0; JZ; ADD RAX,exScore; JMP; ADD RAX,moneyScore; MOV EAX,[RAX]` — the game's own \"which score counter does this side display\" selector. derive_two_player_bpl publishes the three imm32s as `gpa_is_ex_off` (match+2, 0x1D0 = the cached use-EX-score byte), `gpa_ex_score_off` (match+11, 0x1D8) and `gpa_money_score_off` (match+19, 0x1D4) so two_player_bpl_mode's onUpdate replacement reads exactly what stock BPL reads, with the offsets attested per build instead of hardcoded (all three sit below the +0x208 GamePlayActor layout fork and match song_reset's GPA_SCORE_OFFSET/GPA_EX_SCORE_OFFSET). Unique on all four builds.",
    },
    // ── DDR SELECTION (ddr_selection) ───────────────────────────────────
    SignatureDefinition {
        name: "layout_package_helper",
        pattern: "40 55 53 56 57 41 54 41 55 41 56 48 8B EC 48 83 EC 70 48 C7 45 B0 FE FF FF FF 48 8B 05 ?? ?? ?? ?? 48 33 C4 48 89 45 F0 45 8B F1 49 8B D8 4C 63 EA 4C 8B E1 48 C7 45 D0 0F 00 00 00 48 C7 45 C8 00 00 00 00 C6 45 B8 00 44 89 4D E0 33 C0 48 83 C9 FF 49 8B F8 F2 AE 48 F7 D1 4C 8D 41 FF 48 8B D3 48 8D 4D B8 E8 ?? ?? ?? ?? 48 8D 3D ?? ?? ?? ?? 48 8B F3 B9 0E 00 00 00 F3 A6 75 05 45 85 F6 74 26",
        description: "Entry of `sequence::dance::LayoutActor`'s per-package helper `void(LayoutActor* this, int side /*0,1; 2 = shared*/, const char* base, int skin, bool shared)` (FUN_18006b710 on 20260825, FUN_180068000 on 20250805 — byte-shape-identical on all five builds apart from rel32/RIP displacements). Builds the record value `{std::string name @[RBP-0x48]; int skin @[RBP-0x20] (= value+0x28)}` from `base`, formats `\"%04d\"` with the skin into a DEAD 8-byte stack buffer (A3 appended it to the name here — World removed the append), probes `FUN_1801ac3f0(\"bm2d\", name)` (skin := 0 on a miss), then — when `!shared || skin != 0` — inserts `records[side][base] = value` and pushes the name on the LayoutActor load list. Pattern = prologue through the `\"dance_message\"` compare (LEA RDI at match+106, disp at +109). Consumer: ddr_selection::package_helper detours the MATCH (full replacement: stock packages call the original with skin 0, legacy skin packages register `<arc_base>000N` A3-style); derive_ddr_selection reads the helper's own body for its callees and record/list offsets. Unique on all five builds.",
    },
    SignatureDefinition {
        name: "dps_skin_table_read",
        pattern: "C7 45 ?? 01 00 00 00 C7 45 ?? 02 00 00 00 C7 45 ?? 03 00 00 00 C7 45 ?? 04 00 00 00 48 C7 45 ?? 05 00 00 00 48 8B 05 ?? ?? ?? ?? 48 8B 08 48 63 81 ?? ?? ?? ?? 44 8B 4C 85",
        description: "Inside `DancePlaySequence::onInitialize` (FUN_1800573d0+0x727 on 20260825): the A3 skin identity table `int t[6] = {0,1,2,3,4,5}` stored on the stack, then `MOV RAX,[rip+game_work_global]; MOV RCX,[RAX]; MOVSXD RAX,[RCX+skin_off]; MOV R9D,[RBP+RAX*4+t]` — the skin id handed as the 4th argument to the LayoutActor ctor (which stores it at +0x190). derive_ddr_selection RIP-decodes match+39 (the GameWork global, cross-checked against stage_record_accessor's) and publishes the disp32 at match+49 as `gamework_skin_off` (0xA8 on every supported build; A3 had +0xB0). Nothing in World writes the field except the per-credit reset; ddr_selection writes the armed skin there. Unique on all five builds.",
    },
    SignatureDefinition {
        name: "afp_sound_callback_play",
        pattern: "48 89 5C 24 08 57 48 83 EC 20 8B 59 08 48 8B CA 48 8B FA E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 0F 57 D2 80 B9 C4 20 00 00 00 74 ?? 85 DB 74 ?? FF CB 75 ?? F3 0F 10 15",
        description: "Entry of `bm2d::SoundCallback::play(this, const char* label)` (FUN_1801ad8e0 on 20260825, 0x180197fe0 on 20250805; RTTI `.?AVSoundCallback@bm2d@@` vtable slot 1) — the game's handler for AFP-embedded sounds (`asdlib.sound_play(\"<cue>\")` in a clip's bytecode; libafp passes only the name). Body: `slot = classify(label)` (`vo_*` or a 25-name A3 voice table → 3, else 2), pan 0 / −1 / +1 from `this+8` (side) when the audio manager's versus-pan byte `+0x20C4` is set, then tail-JMP `se_play(slot, label, pan)` — ONE bank, a miss is silent. The legacy DDR SELECTION clips name A3 cues that only the mod's `dsel` bank holds, so ddr_selection::sound::afp_route detours the match (pre-original: route its names to the dsel slot). derive_afp_sound_callback cross-checks the RTTI vtable slot 1 == match and the tail `E9` rel32 (disp at match+0x55) → the `se_play` match; any miss un-resolves the name. Unique on all five builds (+ A3 20240402).",
    },
    // The code-played full-combo SE inside `FullcomboActor::onMessage`
    // (FUN_180069c00+0xB7 on 20260825 — the handler is byte-shape-identical
    // from +0x95 to +0x13B on all five builds): the inlined play
    //   MOV RBP,[rip+audio_mgr]; MOV RDI,[RBP+0x30] (slot-2 bank);
    //   TEST RDI,RDI; JZ release;               ← match+0x0E
    //   MOV RAX,[RDI]; LEA RDX,["se_game_fullcombo"]; MOV RCX,RDI;
    //   CALL [RAX] (GetCueIndex); CMP AX,0xFFFF; JZ; … CALL [R10+0x20] (Play);
    //   … MOVAPS XMM3,XMM6; MOV R8D,2; … CALL register_handle
    // A3's FullcomboActor played no code SE (its only full-combo sound is the
    // legacy clip's embedded `XAC_full_combo2`), so on a legacy full-combo
    // song ddr_selection::sound::code_se flips the null-bank JZ to JMP
    // (`74` → `EB`, rel8 kept) — World's own "no se_normal bank" path: the
    // play is skipped, the AVS lock is still released. derive_ddr_sel_code_se
    // gates the LEA (disp at match+0x16) on the string, the JZ opcode and its
    // target (`TEST ECX,ECX; JLE` = the lock release) and — when resolved —
    // the site lying inside `fullcombo_actor_on_message`; publishes the JZ as
    // `ddr_sel_fullcombo_se_jz`. Unique on all five builds (the first 35
    // bytes alone also hit one unrelated inlined play; the tail pins it).
    SignatureDefinition {
        name: "ddr_sel_fullcombo_se_site",
        pattern: "48 8B 2D ?? ?? ?? ?? 48 8B 7D 30 48 85 FF 74 ?? 48 8B 07 48 8D 15 ?? ?? ?? ?? 48 8B CF FF 10 B9 FF FF 00 00 66 3B C1 74 ?? 48 C7 84 24 98 00 00 00 00 00 00 00 4C 8B 17 48 8D 8C 24 98 00 00 00 48 89 4C 24 20 45 33 C9 45 33 C0 0F B7 D0 48 8B CF 41 FF 52 20 85 C0 78 ?? 0F 28 DE 41 B8 02 00 00 00",
        description: "FullcomboActor::onMessage's inlined `se_game_fullcombo` play from the slot-2 bank (FUN_180069c00+0xB7 on 20260825). ddr_selection flips the null-bank JZ at match+0x0E to JMP while a legacy full-combo package is loaded (A3 played only the clip's own XAC_full_combo2). Identity-gated by derive_ddr_sel_code_se (LEA at +0x13 → \"se_game_fullcombo\").",
    },
    // DancePlaySequence::onUpdate step 5's READY? dwell gate (FUN_180057e10
    // +0xA94 on 20260825): `MOVSS XMM0,[RSI+timer]; COMISS XMM0,[rip+5.0f];
    // JB wait; MOV RDX,[rip+shutter_global]; TEST RDX,RDX; JZ; MOVZX EAX,
    // word [RDX+0x82]` — the song waits until 5.0 s have passed since the
    // DPS was created. A3 had no dwell (its stage panel held itself), so
    // while a legacy intro is armed ddr_selection seeds the timer past the
    // threshold every pre-song frame (the quick-restart technique).
    // derive_ddr_sel_intro publishes the timer disp32 (match+4, 0x130 on
    // every build) as `ddr_sel_dps_ready_timer_off`, gated on the threshold
    // float (rip at match+11) being a plausible dwell and the shutter global
    // (rip at match+24) equalling `shutter_actor_global`. Unique on all five
    // builds (the bare MOVSS/COMISS prefix also hits a +0x158 twin on
    // 20250805/20260224; the shutter load pins it).
    SignatureDefinition {
        name: "ddr_sel_dps_ready_dwell",
        pattern: "F3 0F 10 86 ?? ?? 00 00 0F 2F 05 ?? ?? ?? ?? 0F 82 ?? ?? 00 00 48 8B 15 ?? ?? ?? ?? 48 85 D2 74 ?? 0F B7 82 82 00 00 00",
        description: "DancePlaySequence step-5 READY? dwell gate (MOVSS [RSI+0x130]; COMISS [rip 5.0f]; JB) followed by the ShutterActor global load. ddr_selection seeds the DPS timer past the threshold while a legacy intro is armed (A3 had no dwell).",
    },
    // ── DDR SELECTION legacy stage panel (ShutterActor kind-3 host) ─────
    //
    // RE: `.agents/planning/2026-09-22-ddr-selection/research/stage-panel.md`.
    // All four feed derive_ddr_sel_panel (all-or-nothing, publishes on every
    // build; `ddr_sel_panel_host_ok` = 1 only where World's state machine can
    // host A3's legacy root).
    SignatureDefinition {
        name: "ddr_sel_shutter_swap",
        pattern: "8B 8E ?? ?? 00 00 8B 86 ?? ?? 00 00 89 86 ?? ?? 00 00 89 8E ?? ?? 00 00 48 8D 8E ?? ?? 00 00 48 8D 96 ?? ?? 00 00 E8 ?? ?? ?? ?? 48 8D 96 ?? ?? 00 00 48 8D 9E ?? ?? 00 00 48 8B CB E8",
        description: "ShutterActor::onUpdate state 2 (FUN_180033f60+0x4E5 on 20260825): the active/pending kind swap `MOV ECX,[RSI+active]; MOV EAX,[RSI+pending]; MOV [RSI+active],EAX; MOV [RSI+pending],ECX` followed by the two std::string copies basename `+0x318 <- +0x340` and jacket name `+0x368 <- +0x390`. derive_ddr_sel_panel cross-checks the kind offsets (+2/+8/+14/+20) against shutter_actor_layout and publishes `ddr_sel_shutter_basename_off` (pending basename, disp at +34) and `ddr_sel_shutter_jacket_off` (active jacket name, disp at +53). Unique on all five builds.",
    },
    SignatureDefinition {
        name: "ddr_sel_shutter_stage_tail",
        pattern: "48 8D 15 ?? ?? ?? ?? 48 8B 8E ?? ?? 00 00 E8 ?? ?? ?? ?? 48 85 C0 74 0A 33 D2 48 8B C8 E8 ?? ?? ?? ?? E8 ?? ?? ?? ??",
        description: "ShutterActor::onUpdate state 2, stage kind only: `LEA RDX,[\"choice_stage_usr\"]; MOV RCX,[RSI+stage_layer]; CALL find; TEST RAX,RAX; JZ; XOR EDX,EDX; MOV RCX,RAX; CALL CMovieClip::Pause; CALL stage_voice` (FUN_180033f60+0x640 on 20260825). derive_ddr_sel_panel checks the LEA string, the layer slot (+10) == 0x88 + stage_kind*0x10, publishes the stage-voice function (CALL at +34) and whether the preceding `jacket_usr` SetVisible is null-checked (`48 85 C0 74 0A` at -15: 20260721+ yes; 20250805/20260224 no — there the unchecked `CALL SetVisible` at -5 is shape-checked from -29 and published as `ddr_sel_shutter_jacket_vis_call`, which ddr_selection NOPs while it hosts A3's root: that root has no `jacket_usr`). Unique on all five builds.",
    },
    SignatureDefinition {
        name: "ddr_sel_shutter_kind_table",
        pattern: "48 63 C2 48 C1 E0 06 83 F9 0A 75 ?? 48 8D 0D ?? ?? ?? ?? 0F 10 84 08 ?? ?? ?? ?? 0F 10 94 08 ?? ?? ?? ?? 0F 10 9C 08 ?? ?? ?? ?? 0F 10 A4 08 ?? ?? ?? ?? EB ?? 83 F9 09 48 8D 0D ?? ?? ?? ?? 75 ?? 0F 10 84 08 ?? ?? ?? ?? 0F 10 94 08 ?? ?? ?? ?? 0F 10 9C 08 ?? ?? ?? ?? 0F 10 A4 08 ?? ?? ?? ?? EB ?? 0F 10 84 08 ?? ?? ?? ??",
        description: "ShutterActor kind-art loader (FUN_180035420+0x200 on 20260825, 20260721+): the per-kind 0x40-byte row copy `{pkg, root, SE in, SE out, voice in, voice out, …}` from the dan (mode 10) / galaxy-brave (mode 9) / default tables; the default table is the module-relative disp32 at +103. derive_ddr_sel_panel publishes the stage row (`ddr_sel_shutter_stage_row`, gated: pkg NULL, root \"shutter_play\", SE in \"se_start_game\"). ddr_selection rewrites that row while a legacy panel is hosted (pkg \"common_choice_v2\", root \"shutter_choice_hd_root\") so World's own named-package path loads A3's root. 20250805/20260224 have the 0x30-stride row getter instead (`_v1`).",
    },
    SignatureDefinition {
        name: "ddr_sel_shutter_kind_table_v1",
        pattern: "48 8D 0C 40 48 03 C9 49 8B 84 C8 ?? ?? ?? ?? 49 89 01 49 8B 84 C8 ?? ?? ?? ?? 49 89 41 08 49 8B 84 C8 ?? ?? ?? ?? 49 89 41 10",
        description: "Old-layout (20250805/20260224) ShutterActor kind-row getter (0x1800351e0+0x70 on 20250805): the default 0x30-stride, 6-pointer table's module-relative disp32 at +11. derive_ddr_sel_panel publishes the stage row (kind 1) from it; the first three pointers (pkg, root, SE in) are laid out like the 0x40 rows, and the old kind-art loader (0x180034fc0 on 20250805) has the same named-package branch.",
    },
    // ── DDR SELECTION `_sel` background movies ───────────────────────────
    //
    // RE: `.agents/planning/2026-09-22-ddr-selection/research/
    // end-banners-sel-movies.md` §4. Both feed derive_ddr_sel_movie
    // (all-or-nothing; a miss only means World's own movie rules).
    SignatureDefinition {
        name: "ddr_sel_sma_movie_gate",
        pattern: "E8 ?? ?? ?? ?? 48 85 C0 74 27 0F B6 88 ?? ?? ?? ?? 80 F9 05 75 10 0F B6 88 ?? ?? ?? ?? 80 F9 05 0F 84 ?? ?? ?? ?? 0F B6 C1 84 C0 0F 84 ?? ?? ?? ??",
        description: "SceneManageActor::onInitialize's movie gate (FUN_18007d700+0x33 on 20260825): `CALL music_info_lookup(basename); TEST RAX,RAX; JZ create; MOVZX ECX,byte [RAX+0x141]; CMP CL,5; JNZ; MOVZX ECX,byte [RAX+0x140]; CMP CL,5; JZ skip; MOVZX EAX,CL; TEST AL,AL; JZ skip` — World creates the song's MovieActor only for a null entry or movie bytes other than 0 / 5 (then VIDEO SIZE `+0xE4` 1 or 2). derive_ddr_sel_movie checks the match lies in the RTTI slot-4 function, decodes the lookup (CALL), both movie-byte disps (+13 / +25), the VIDEO SIZE disp (+51, the instruction the null-entry JZ lands on) and the basename LEA 0x16 before the CALL. ddr_selection (the `_sel` movies) makes a movie-less legacy song pass this gate for the one call. Unique and byte-identical on all five builds.",
    },
    SignatureDefinition {
        name: "ddr_sel_movie_sel_test",
        pattern: "48 83 B8 ?? ?? ?? ?? 00 75 0E 48 8B 10 48 8B C8 FF 52 08 48 8B D8 EB 11 48 8D 98 ?? ?? ?? ?? 48 83 7B 18 10 72 03 48 8B 1B 80 BF ?? ?? ?? ?? 00 74 ?? 4C 8D 87 ?? ?? ?? ?? 48 8D 8F ?? ?? ?? ?? 48 8B D3 E8",
        description: "MovieActor::onInitialize's path search (FUN_18007cd70+0x67 on 20260825): the movie name = the music-info override string (`+0x148`, empty ⇒ entry vslot 1), then `CMP byte [RDI+0x149],0; JZ; LEA R8,[RDI+0xB0]; LEA RCX,[RDI+0xD8]; MOV RDX,RBX; CALL try_sel` — World's dormant `_sel` flag (nothing writes it). derive_ddr_sel_movie gates the CALL target on its `\"_sel\"` string and the match on lying 0x100 into the first CALL of MovieActor vtable slot 4, publishes the flag (+43), the override string (+27, its size at +3) and the found-path string (+60). Unique and byte-identical on all five builds.",
    },
    // ── DDR SELECTION legacy stage frame ("1st STAGE" …) ────────────────
    //
    // RE: `.agents/planning/2026-09-22-ddr-selection/research/
    // hud-layout-stage-frame.md` §4 / §8. Both feed derive_ddr_sel_stage_frame
    // (all-or-nothing; a miss keeps World's stage frame).
    SignatureDefinition {
        name: "ddr_sel_stage_frame_export",
        pattern: "4C 8D 05 ?? ?? ?? ?? 41 B9 05 00 00 00 48 8D 3C C0 48 8B D6",
        description: "StageFrameActor::onInitialize's clip create (init+0xC8 on every build, FUN_18007a190 on 20260825): `LEA R8,[\"dance_stage\"] (the EXPORT name — the init's earlier \"dance_stage\" LEA is the record key); MOV R9D,5; … CALL CMovieClip::Create`. ddr_selection points the disp32 (+3) at a near-allocated \"stage_frame\" (A3's export in `dance_stage_frame000N`) while the current LayoutActor's dance_stage record is legacy. derive_ddr_sel_stage_frame checks the match lies in the RTTI slot-4 function and the string. Unique on all five builds.",
    },
    SignatureDefinition {
        name: "ddr_sel_stage_frame_texture",
        pattern: "41 B8 0B 00 00 00 48 8D 15 ?? ?? ?? ?? 49 8D 4B ?? E8 ?? ?? ?? ?? 90 48 8B 1D ?? ?? ?? ?? 48 8B 13 80 7A",
        description: "StageFrameActor's stage-texture fn (texture fn+0x46 on every build; the fn is the CALL at msg+0x24 of RTTI slot 8, FUN_18007a390 on 20260825): `MOV R8D,0xB; LEA RDX,[\"dast_stage_\"]; LEA RCX,[R11-0x40]; CALL string::assign` — the texture-name prefix World's stage suffix (01..05 / final / extra / …) is appended to. ddr_selection rewrites imm32 (+2) and disp32 (+9) to A3's `stage_frame000N_stage_` (22 chars) while the dance_stage record is legacy. derive_ddr_sel_stage_frame checks the fn, the imm and the string. Unique on all five builds (the 17-byte head alone also hits a `music_title` assign).",
    },
    // ── Multiplayer Bot (multiplayer_bot) ───────────────────────────────
    SignatureDefinition {
        name: "extra_stage_grant",
        pattern: "48 83 EC 38 48 8B 05 ?? ?? ?? ?? 48 8B 10 80 7A 59 00 0F 85 ?? ?? ?? ?? 85 C9 0F 85 ?? ?? ?? ?? 48 83 7A 70 00 0F 85 ?? ?? ?? ?? 83 7A 04 01 0F 84",
        description: "Entry of the extra-stage grant `void(int arg)` (FUN_1801ddcd0 on 20260825; 0x1801c6970 on 20250805, 0x1801ca7e0 on 20260224): `SUB RSP,38; MOV RAX,[rip+game_work_global]; MOV RDX,[RAX]; CMP byte [RDX+0x59],0 (already granted); JNZ out; TEST ECX,ECX (arg must be 0); JNZ out; CMP qword [RDX+0x70],0 (course); JNZ out; CMP dword [RDX+0x4],1 (double); JZ out` — then `max_stage + 1 == 3` and, for EVERY side with `PlayerWork+0x4 != 0`: `record[0]+0x50 >= 0xF` (AAA), `PlayerWork+0x1710 == 0`, gauge option in {0, 0xC}, `record[0]+0x270 != 7`; on success `GameWork+0x59 = 1`. Called from `ResultSequence::onUpdate` case 0x16 (results window-out) when the stage counter is 0 — INSIDE the Multiplayer Bot's play window, so a low-level bot that did not AAA would block the human's extra stage. Consumer: `multiplayer_bot::extra_stage_guard` detours the MATCH address (nothing read at match+N) and clears the bot side's entered byte around the original while an impersonation is active. Soft consumer (`get_address`): a miss leaves the stock rule + one WARN. GameWork disp32 and the four JCC rel32s wildcarded; unique on all four builds.",
    },
    // ── Background Dancers (scene3d) ────────────────────────────────────
    //
    // Every anchor the 3D scene service needs. All twelve feed ONE
    // all-or-nothing derivation (`derive_scene3d`, RE record
    // `docs/background_dancers_research.md` §1.7): a miss or a failed identity
    // gate un-resolves the whole `scene3d_*` group AND these raw names, so no
    // consumer can pick up a half-derived layout. Hit counts below are the
    // attested values on 20250805 / 20260224 / 20260721 / 20260825 / 20260915.
    SignatureDefinition {
        name: "sg_enable_bit_site",
        pattern: "48 8B 05 ?? ?? ?? ?? 48 8B 08 83 49 08 01",
        description: "`MOV RAX,[rip+SceneGraphManager]; MOV RCX,[RAX]; OR dword [RCX+8],1` — the SceneGraph ENABLE-bit set at `DancePlaySequence::onUpdate` step 5 (FUN_180057e10+0xB4F on 20260825, A3's 0x1046 song-start edge) and its two MatchingDancePlaySequence twins. 3 hits on every build; derive_scene3d requires ALL hits to decode the SAME global (RIP disp32 at match+3 → `scene3d_scene_graph_manager`, the pointer-to-manager global; `*global` = mgr, `*mgr` = graph) and publishes the imm8 at match+12 as `scene3d_graph_flags_off` (0x08, bit0 = enabled).",
    },
    SignatureDefinition {
        name: "sg_manager_tick",
        pattern: "40 57 48 83 EC 20 E8 ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B F8 48 85 C0 0F 84 ?? ?? ?? ?? 48 8B C8 48 89 5C 24 30 48 89 74 24 38 E8 ?? ?? ?? ?? 48 8B CF 48 8D 77 08 E8 ?? ?? ?? ?? 80 BF ?? ?? ?? ?? 00 74 ?? 48 8B CF E8",
        description: "Prologue of the SceneGraphManager per-frame tick (FUN_180023fb0 on 20260825; the DebugRenderJob's run(this=mgr)): `CALL destroy_flush; CALL active_camera; TEST RAX; JZ; CALL view_rebuild(cam); LEA RSI,[RDI+8]; CALL view_rebuild (again); CMP byte [RDI+0x2B3],0; JZ; MOV RCX,RDI; CALL proj_rebuild` then the view/projection memcpy fan-out into the four MODEL passes. derive_scene3d reads: CALL rel32 at match+6 (must land on the `sg_destroy_flush` match), at match+11 (must land on the `sg_active_camera` match), at match+41 (camera view rebuild, FUN_180220b80) and match+70 (camera projection rebuild, FUN_1802376e0 — the two callees the camera field block is decoded from), imm8 at match+52 (`scene3d_camera_view_off` = 0x08) and disp32 at match+60 (`scene3d_camera_proj_dirty_off` = 0x2B3). Unique on every build.",
    },
    SignatureDefinition {
        name: "sg_active_camera",
        pattern: "48 8B 05 ?? ?? ?? ?? 45 33 C0 4C 8B 10 48 B8 41 20 10 08 04 02 81 40 49 8B 4A ?? 49 2B 4A ?? 48 F7 E9",
        description: "The active-camera finder (FUN_1800243a0 on 20260825): `MOV RAX,[rip+mgr]; XOR R8D; MOV R10,[RAX] (graph); MOV RAX,0x4081020408102041 (the 1/0x3F8 reciprocal); MOV RCX,[R10+0x40]; SUB RCX,[R10+0x38]; IMUL` — walks the camera vector for the first slot whose active byte is set. derive_scene3d requires the RIP global at match+3 to equal `sg_enable_bit_site`'s and the tick's CALL at match+11 to land here; publishes imm8 at match+30 as `scene3d_graph_camera_vec_off` (0x38; the imm8 at match+26 must be +8), then scans forward for `IMUL RAX,RAX,imm32` (`48 69 C0`) → `scene3d_camera_stride` (0x3F8) and `CMP byte [RAX+RCX+disp32],0` (`80 BC 08`) → `scene3d_camera_active_off` (0x3F4). Unique on every build.",
    },
    SignatureDefinition {
        name: "sg_destroy_flush",
        pattern: "40 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 48 48 89 6C 24 50 48 89 74 24 58 48 8B E9 48 8D 79 ?? 48 89 7C 24 40 8B 0F 85 C9 7E ?? FF 15 ?? ?? ?? ?? FF 47 ?? 48 8B 5D ?? 48 3B 5D ??",
        description: "Prologue of the SceneGraphManager deferred-destroy flush (FUN_180024250 on 20260825, `this` = mgr): `LEA RDI,[RCX+0x28] (avs mutex id); MOV ECX,[RDI]; TEST; JLE; CALL [rip+IAT avs_mutex_lock] (libavs-win64 ordinal 16); INC [RDI+4] (depth); MOV RBX,[RBP+8]; CMP RBX,[RBP+0x10] (destroy vector begin/end)`; the body unlinks each queued node's children (link clears only), unlinks it from its parent and calls `(*node->vtable[0])(node, 1)` — OUR node dtor — then `CALL [rip+IAT avs_mutex_unlock]` (ordinal 17). derive_scene3d requires the tick's CALL at +6 to land here; reads imm8 at match+36 (`scene3d_mgr_mutex_off` 0x28), RIP disp32 at match+50 (`scene3d_mutex_lock_iat` — the IAT SLOT, read at call time), imm8 at match+56 (+mutex → `scene3d_mgr_depth_off` 0x2C), imm8 at match+60 (`scene3d_mgr_destroy_vec_off` 0x08; match+64 must be +8); the dtor call shape `48 8B 01 BA 01 00 00 00 FF 10` and exactly one further `FF 15` (→ `scene3d_mutex_unlock_iat`) must follow within 0x180 bytes. Unique on every build.",
    },
    SignatureDefinition {
        name: "sg_update_job_run",
        pattern: "48 83 EC 28 48 85 D2 74 ?? F6 42 08 01 74 ?? 80 3D ?? ?? ?? ?? 00 74 ?? F3 0F 10 0D ?? ?? ?? ?? F3 0F 59 0D ?? ?? ?? ?? EB ?? F3 0F 10 0D ?? ?? ?? ?? F3 0F 59 0D ?? ?? ?? ?? 48 8B 05 ?? ?? ?? ?? 48 8B CA F3 0F 59 48 ?? E8",
        description: "GraphUpdateJob::run(this, graph) (FUN_180024430 on 20260825): `TEST RDX; JZ; TEST byte [RDX+8],1 (graph enabled); JZ; dt = frameDelta × …; MOV RAX,[rip+mgr]; MULSS XMM1,[RAX+0x38] (playback rate); CALL SceneGraph::update(graph, dt)`. derive_scene3d requires the RIP global at match+61 to equal `sg_enable_bit_site`'s, publishes imm8 at match+72 as `scene3d_mgr_rate_off` (0x38) and the CALL target at match+73 as `scene3d_scene_graph_update` (FUN_180214570) — whose prologue must read `40 53 56 57 41 54 41 56 41 57 48 83 EC 48 48 8B 59 ??` (imm8 → `scene3d_graph_root_child_off` 0x18); inside its body the item push `MOV RDX,[RDX+0x78]; CALL` (`48 8B 52 ?? E8`) → `scene3d_node_item_off` and the sort gate `TEST byte [R12+0x28],1` (`41 F6 44 24 ?? 01 74`) → `scene3d_graph_sort_flag_off`, followed by the CALL to the std::sort dispatcher. Unique on every build.",
    },
    SignatureDefinition {
        name: "sg_insertion_sort",
        pattern: "48 8B 3B 48 8B 06 4C 8B C3 44 8B 8F ?? ?? ?? ?? 44 3B 88 ?? ?? ?? ?? 7D",
        description: "The insertion-sort leaf of `SceneGraph::update`'s visible-node std::sort (FUN_180215830 on 20260825): `MOV RDI,[RBX]; MOV RAX,[RSI]; MOV R8,RBX; MOV R9D,[RDI+0xE8]; CMP R9D,[RAX+0xE8]; JGE` — the comparator reads an i32 at node+0xE8 from EVERY visible node, so the mod-owned node MUST carry a valid sort key there. The match is the comparator LOOP at entry+0x40, not the function entry: derive_scene3d requires a CALL rel32 inside the sort dispatcher (reached from `SceneGraph::update`'s sort gate) whose target lies within 0x100 bytes BEFORE the match, and publishes the disp32 at match+12 (== the one at match+19) as `scene3d_node_sort_key_off`. Unique on every build.",
    },
    SignatureDefinition {
        name: "model_registry_release",
        pattern: "40 55 56 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 50 8B F9 48 8B 35 ?? ?? ?? ?? 48 8D 9E ?? ?? ?? ?? 48 89 5C 24 68 8B 0B 85 C9 7E ?? FF 15 ?? ?? ?? ?? FF 43 04 48 8D 6E ?? 48 8B 4D 08 48 8B 41 08 80 78 ?? 00 75 ?? 0F 1F 40 00 39 78 ?? 73 ?? 48 8B 40 ?? EB ?? 48 8B C8 48 8B 00 80 78 ?? 00 74",
        description: "Prologue + red-black-tree walk of a ResourceManager `release(hash)` (FUN_180203b60 = the MODEL registry's on 20260825): `MOV RSI,[rip+ResourceManager]; LEA RBX,[RSI+0x130] (map mutex); … CALL [IAT avs_mutex_lock]; INC [RBX+4]; LEA RBP,[RSI+0x30] (std::map); MOV RCX,[RBP+8] (head); MOV RAX,[RCX+8] (root); CMP byte [RAX+0x39],0 (isnil); CMP [RAX+0x18],EDI (key); MOV RAX,[RAX+0x10] (right) / MOV RAX,[RAX] (left)`. The shape matches the three raw-buffer registries too (3 hits per build); derive_scene3d selects the ONE whose body `LEA RCX,[rip+vftable]` names `agcs::Resource::GpuResource<gs::ModelData>` (RTTI) and whose lock IAT slot equals the scene-graph flush's. Reads: RIP disp32 at match+0x1B (`scene3d_resource_manager`), disp32 at match+0x22 (`scene3d_rm_model_mutex_off` 0x130), imm8 at +0x3D (`scene3d_rm_model_map_off` 0x30), imm8 at +0x48 and +0x63 (`scene3d_rm_node_nil_off` 0x39, must agree), imm8 at +0x52 (`scene3d_rm_node_key_off` 0x18), imm8 at +0x58 (`scene3d_rm_node_right_off` 0x10), then forward `48 8B 77 ??` (`scene3d_rm_node_value_off` 0x28) and `FF 4F ??` (`scene3d_rm_node_refcount_off` 0x30). World deleted A3's lookup-by-hash, so `scene3d::model_registry` walks the tree itself with these offsets under the same mutex.",
    },
    SignatureDefinition {
        name: "texture_create_site",
        pattern: "BA 20 00 00 00 48 8B ?? 44 8D 4A F5 44 8D 42 E1 B9 00 01 00 00 C7 44 24 20 02 20 00 00 E8",
        description: "The ArrowPalette factory's dynamic-texture create (FUN_180024d00+0x11 on 20260825): `MOV EDX,0x20; MOV RBX,RCX; LEA R9D,[RDX-0xB] (fmt 0x15); LEA R8D,[RDX-0x1F] (mips 1); MOV ECX,0x100; MOV [RSP+0x20],0x2002 (usage); CALL create` — the engine texture API `u32 create(w, h, mips, fmt, usage)` (FUN_180249c20). derive_scene3d publishes the CALL target at match+29 as `scene3d_texture_create`; bone textures are created as `(4, bone_count, 1, 0x74 A32B32G32R32F, 0x2001)` — WIDTH 4 texels, one bone per ROW (the upload strides `data + i*pitch`; RE docs/background_dancers_research.md §2.8). Unique on every build.",
    },
    SignatureDefinition {
        name: "texture_release",
        pattern: "40 53 48 83 EC 20 8B D9 B8 01 00 00 00 F0 0F C1 05 ?? ?? ?? ?? 85 C0 74 ?? 0F 1F 80 00 00 00 00 FF 15 ?? ?? ?? ?? 41 BB 01 00 00 00 F0 44 0F C1 1D ?? ?? ?? ?? 45 85 DB 75 ?? 85 DB 74 ?? 48 8B C3 48 C1 E8 11 48 8D 0C 80 48 C1 E1 05 48 03 0D ?? ?? ?? ?? 39 19 75 ?? E8",
        description: "Engine texture release `i32 release(u32 handle)` (FUN_18024a170 on 20260825 — the World twin of the A3 render-item dtor's bone-texture release): spin on the registry flag (`XADD.LOCK [rip+DAT_1806f1a60]`), slot = `(handle>>17)*0xA0 + [rip+registry]`, `CMP [slot],EBX` (stale-handle guard), `CALL` refcount-decrement/free (FUN_180249570 == A3 FUN_180166560). `8B D9` (by value) excludes the by-pointer twin FUN_1801f4c30 (`8B 19`). derive_scene3d publishes the match as `scene3d_texture_release` and requires its spin-flag global (RIP at match+17) to equal the first `F0 0F C1 05` in the create body. Unique on every build.",
    },
    SignatureDefinition {
        name: "texture_lookup_site",
        pattern: "8B CF E8 ?? ?? ?? ?? 48 85 C0 48 0F 44 05 ?? ?? ?? ?? 33 C9 87 0D ?? ?? ?? ?? 48 89 43 08 FF C6 48 83 C5 50 48 83 C3 10",
        description: "Tail of the model converter's texture-table fill (FUN_180273e20+0xC7 on 20260825): `MOV ECX,EDI (gs hash); CALL FUN_18026f9e0 (gs texture-registry lookup, TextureData* or null); TEST RAX; CMOVZ RAX,[rip+DAT_1806f3298] (default texture); XOR ECX,ECX; XCHG [rip+DAT_1806f2090],ECX (spin release); MOV [RBX+8],RAX (entry.ptr); INC ESI; ADD RBP,0x50; ADD RBX,0x10`. The lookup's ONLY caller in World — A3's setModel-time re-resolve (FUN_180175b80) has no twin, so a material whose DDS registered after its .model converted holds the default texture forever; scene3d::render_item re-resolves into its own material copies with these three sites. OPTIONAL sub-group of derive_scene3d: publishes `scene3d_texture_lookup` (CALL @+2; its prologue must be `40 53 48 83 EC 20 48 8B 05 ?? ?? ?? ?? 8B D9 80 B8 80 00 00 00 00 75 2A`), `scene3d_texture_default` (RIP @+14) and `scene3d_texture_spin` (RIP @+22, must equal both `LOCK XADD [rip]` globals at match-0x23 and match-0x0C). Unique + byte-shape identical on every build; a miss only disables the re-resolve. RE: docs/background_dancers_research.md §2.1/§2.5.",
    },
    SignatureDefinition {
        name: "model_shader_select_site",
        pattern: "44 8B D0 41 8B CA E8 ?? ?? ?? ?? 48 85 C0 75 2A 38 43 2C 74 0C 8D 50 19 48 8D 0D ?? ?? ?? ?? EB 0C BA 10 00 00 00 48 8D 0D ?? ?? ?? ?? FF 15 ?? ?? ?? ?? 8B C8 E8 ?? ?? ?? ??",
        description: "The model converter's material→shader selection (FUN_1802745b0+0x81 on 20260825, World twin of A3 FUN_18018af30): `MOV R10D,EAX (FNV-1 of the material's debug-info shader name); MOV ECX,R10D; CALL shader_registry_lookup; TEST RAX; JNZ done; CMP byte [RBX+0x2C],AL (mesh has BLENDWEIGHT); LEA EDX,[RAX+0x19]; LEA RCX,[\"gs_model_skinning_default\"]; JMP; MOV EDX,0x10; LEA RCX,[\"gs_model_default\"]; CALL [rip+hasher]; MOV ECX,EAX; CALL shader_registry_lookup`. OPTIONAL sub-group of derive_scene3d: publishes the CALL target at match+6 as `scene3d_shader_lookup` — `gs::Shader* fn(u32 fnv1_name_hash)` (FUN_18025f8f0: spins the registry flag, binary-searches the sorted object vector on `*(u32*)obj == hash`, null on miss; its prologue must be `40 53 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 8B D9`) — identity-gated on BOTH CALLs sharing one target and the two LEAs pointing at those exact strings. The DLL's render items re-point their PRIVATE material copies (`mat+0x20` = the object the converter stored here) at synthesized style-variant containers looked up through it — the whole-scene stock/lit/cel switch without a detour (RE docs/background_dancers_research.md §4.7). Unique + byte-shape identical on every build; a miss only disables the restyle.",
    },
    SignatureDefinition {
        name: "bgmovie_readiness",
        pattern: "40 53 48 83 EC 20 48 8B 05 ?? ?? ?? ?? 48 8B 58 ?? 48 85 DB 74 ?? 48 8D 8B ?? ?? ?? ?? E8 ?? ?? ?? ?? 84 C0 75 ?? 32 C9 EB ?? 48 8D 8B",
        description: "`sequence::common::BgMovieActor` readiness (FUN_1800320a0 on 20260825): `MOV RAX,[rip+DAT_1806f2d38] (the BgMovieActor singleton); MOV RBX,[RAX+0x58] (BackgroundFrame); TEST; JZ; LEA RCX,[RBX+0x150]; CALL AnimationLoader::ready; …`. The match IS the function entry (`40 53` = REX-prefixed PUSH RBX — the DPS poll's CALL target is compared against it). derive_scene3d publishes RIP at match+9 as `scene3d_bgmovie_actor` and imm8 at match+16 as `scene3d_bgframe_off` (0x58); the live `bg_root` clip the background hide targets is `*(frame + scene3d_bg_clip_slot_off)`. Unique on every build.",
    },
    SignatureDefinition {
        name: "bgmovie_ready_call_site",
        pattern: "48 83 3D ?? ?? ?? ?? 00 74 ?? E8 ?? ?? ?? ?? 84 C0 0F 84",
        description: "`CMP qword [rip+global],0; JZ; CALL fn; TEST AL,AL; JZ far` — the DancePlaySequence::onUpdate case-3 readiness poll (`if (BgMovieActor && !FUN_1800320a0()) wait`) among 4 look-alikes per build. Identity only: derive_scene3d requires at least one hit whose CMP global (RIP disp32 at match+3, PLUS ONE — the instruction is 8 bytes) equals `scene3d_bgmovie_actor` AND whose CALL target (match+10) equals the `bgmovie_readiness` match. Nothing else is read.",
    },
    SignatureDefinition {
        name: "bg_root_create_site",
        pattern: "4C 8D 35 ?? ?? ?? ?? 49 8B FE 0F 1F 40 00 48 8B 17 48 63 C3 48 8D 0C C0 48 C1 E1 06 49 03 CE FF 92 ?? ?? ?? ?? 83 CE FF 84 C0 74 ?? FF C3 48 81 C7 ?? ?? ?? ?? 81 FB ?? ?? ?? ?? 7C",
        description: "The CMovieClip pool walk inside the `bg_root` creator (FUN_18003e5b0+0x122 on 20260825, reached from the BackgroundFrame's AnimationLoader<int> ReactiveAction): `LEA R14,[rip+DAT_1806f9b20] (0x400 × 0x240-byte CMovieClip pool); loop { CALL [vt+0x138] (slot-free probe); ADD RDI,0x240; CMP EBX,0x400 }`, then `CALL CMovieClip::Create(slot, pkg, \"bg_root\", 0)` and `MOV RCX,[frame_ptr]; ADD RCX,0x140; CALL store_shared_ptr` — the clip slot inside `sequence::BackgroundFrame`. derive_scene3d publishes RIP at match+3 as `scene3d_cmovieclip_pool`, the `48 81 C7 imm32` as `scene3d_cmovieclip_pool_stride` (0x240), the `81 FB imm32` as `scene3d_cmovieclip_pool_count` (0x400), and — in the forward window — the `48 81 C1 imm32` after the create CALL as `scene3d_bg_clip_slot_off` (0x140); the CALL rel32 preceded by `LEA R8,[rip+\"bg_root\"]` must equal the already-derived `cmovieclip_create` (identity gate). Unique on every build.",
    },
    // ── Background Dancers option PREVIEWS: the `scene3d_viewport` OPTIONAL
    // sub-group (design .agents/planning/2026-09-21-background-dancers-
    // selection-options §4.5/§4.9; research/preview-compositing.md). Nine AOBs
    // decoded all-or-nothing by `scene3d_resolve_viewport`; a miss leaves
    // `Scene3dSites.viewport == None` (rows work, no live 3D preview). NEVER in
    // any `required_signatures`. Every consumer reads `match+N` — sweep with
    // shape_diff.py.
    SignatureDefinition {
        name: "render_graph_boot_attach",
        pattern: "48 8B 15 ?? ?? ?? ?? 48 83 C2 ?? 41 B8 66 00 00 00 48 8B 0D ?? ?? ?? ?? 48 8B 49 ?? E8 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 83 C2 ?? 41 B8 67 00 00 00 48 8B 0D ?? ?? ?? ?? 48 8B 49 ?? E8 ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 83 C2 ?? 41 B8 68 00 00 00 48 8B 0D ?? ?? ?? ?? 48 8B 49 ?? E8",
        description: "Render-graph boot (FUN_1801f2c30+0x2E5 on 20260825): the three MODEL pass attaches `MOV RDX,[rip+pass]; ADD RDX,0x30 (Viewport sub-object); MOV R8D,0x66/0x67/0x68 (priority); MOV RCX,[rip+display]; MOV RCX,[RCX+0x28] (RENDER-3D target list); CALL attach(list, viewport, prio)` for OPACITY / LOWPRIO_TRANS / TRANS. scene3d_resolve_viewport reads per 33-byte block: RIP at +3 (`scene3d_vp_pass_opacity/lowprio/trans` globals), imm8 at +10 (`scene3d_vp_sub_off` 0x30 — all three must agree), RIP at +20 (`scene3d_vp_display` — all must agree), imm8 at +27 (the RENDER-3D list offset, informational), CALL at +28 (`scene3d_vp_attach`, FUN_1802666c0 — all must agree; its body `48 89 5C 24 08 57 48 83 EC 30 48 8B FA 48 8B D9 48 85 D2 74 06 4C 8D 4A ?? EB 03 45 33 C9 48 8B 41 ?? …` yields `scene3d_vp_rect_off` (LEA disp8, 8), `scene3d_vp_list_target_off` (0x38) and the u16 dims `scene3d_vp_target_w_off/h_off` (0x14/0x16) from the two MOVZX word loads). Unique on every build (the 2D-list attaches use a different shape).",
    },
    SignatureDefinition {
        name: "render_graph_2d_attach",
        pattern: "41 B8 65 00 00 00 48 8B 15 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 48 8B 49 ?? E8 ?? ?? ?? ?? 41 B8 66 00 00 00 48 8B 15 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 48 8B 49 ?? E8 ?? ?? ?? ?? 41 B8 67 00 00 00 48 8B 15 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 48 8B 49 ?? E8",
        description: "Render-graph boot (FUN_1801f2c30+0x3AA on 20260825): the three 2D-layer-list attaches into RENDER_2D — `MOV R8D,0x65/0x66/0x67; MOV RDX,[rip+2D list viewport]; MOV RCX,[rip+display]; MOV RCX,[RCX+0x38] (RENDER_2D target list); CALL attach`. scene3d_resolve_viewport reads per 29-byte block: RIP at +16 (display — must equal `render_graph_boot_attach`'s), imm8 at +23 (`scene3d_vp_render2d_list_off` 0x38 — all three must agree AND differ from the RENDER-3D offset) and CALL at +24 (must equal the attach fn). The DLL attaches its pass clones + clear viewport into THIS list at priorities ≥ 0x68 so they draw above every AFP layer. The RENDER-3D list's own 0x65 attach is a single block (followed by an `ADD RDX,0x188` shape), so the two-block run is unique on every build.",
    },
    SignatureDefinition {
        name: "viewport_detach",
        pattern: "48 8B 0D ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8B 49 ?? 48 81 C2 ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8B 49 ?? 48 83 C2 ?? E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8B 49 ?? 48 83 C2 ?? E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 48 8B 15 ?? ?? ?? ?? 48 8B 49 ?? 48 83 C2 ?? E8",
        description: "Render-graph shutdown (FUN_1801f30b0+0x100 on 20260825): the RENDER-3D list detach of the packet pass at `+0x188` (`MOV RCX,[rip+display]; MOV RDX,[rip+packets]; MOV RCX,[RCX+0x28]; ADD RDX,0x188 (imm32 form — the anchor); CALL detach`) followed by the three MODEL pass detaches `MOV RCX,[rip+display]; MOV RDX,[rip+pass]; MOV RCX,[RCX+0x28]; ADD RDX,0x30; CALL detach(list, viewport)` for OPACITY / LOWPRIO / TRANS. scene3d_resolve_viewport reads per 27-byte MODEL block (k = 0..2 at +30 + 27k): RIP at +3 (display, must equal), RIP at +10 (the pass globals, must equal the boot's in order), imm8 at +17 (list off, must equal the boot's RENDER-3D off), imm8 at +21 (sub off, must equal), CALL at +22 (`scene3d_vp_detach`, FUN_1802667d0 — all three must agree; its body must start `4C 8B 41 08 48 8B 01 4C 8B CA 4C 8B D1 49 3B C0 74 ?? 48 39 10 74 ?? 48 83 C0 10` — the erase walk over 16-byte `{viewport*, prio}` elements). The leading imm32 block is what makes this unique: the three-block run alone also matches one block later (the `ADD RDX,0x8` detach that follows TRANS shares the imm8 shape).",
    },
    SignatureDefinition {
        name: "model_pass_ctor",
        pattern: "B9 ?? ?? ?? ?? E8 ?? ?? ?? ?? 48 85 C0 74 ?? 48 8B 0B 48 89 48 08 48 8B 4B 08 48 89 48 10 48 8B 4B 10 48 89 48 18 48 8B 4B 18 89 70 ?? 48 89 48 20 44 89 70 ?? 4C 89 70 ?? 4C 89 70 ?? 44 89 70 ?? C7 40 ?? 00 00 80 3F 48 89 80 ?? ?? ?? ?? 4C 89 B0 ?? ?? ?? ?? 48 8D 0D ?? ?? ?? ?? 48 89 48 ?? 48 89 98 ?? ?? ?? ?? 89 78 ?? 89 68 ??",
        description: "The MODEL pass object constructor's store block (FUN_1801f6510+0x159 on 20260825, run once per pass DISTANTVIEW/OPACITY/LOWPRIO_TRANS/TRANS): `MOV ECX,0xF8; CALL alloc; TEST RAX; JZ; copy the four callbacks [RBX..RBX+0x18] → [RAX+8..+0x20]; MOV [RAX+0x28],ESI (sort mode); MOV [RAX+0x54],R14D (flags, zeroed); MOV [RAX+0x38],R14; MOV [RAX+0x40],R14 (viewport rect x,y,w,h — zero ⇒ attach fills w/h); MOV [RAX+0x48],R14D (minZ); MOV [RAX+0x4C],1.0f (maxZ); MOV [RAX+0xE0],RAX (self back-pointer); MOV [RAX+0xE8],R14 (render-item list, set later by the SceneGraphManager); LEA RCX,[rip+gs::Renders::Model::Viewport<Render> vftable]; MOV [RAX+0x30],RCX (the Viewport sub-object); MOV [RAX+0xF0],RBX (callback block); MOV [RAX+0x50],EDI (name hash); MOV [RAX+0x2C],EBP (node-mask FILTER)`. scene3d_resolve_viewport reads: imm32 at +1 (`scene3d_vp_pass_size` 0xF8), disp8 at +44 (`scene3d_vp_pass_sort_off` 0x28), +52 (`scene3d_vp_pass_flags_off` 0x54), +56 (`scene3d_vp_pass_rect_off` 0x38), +64 (`scene3d_vp_pass_minz_off` 0x48), +67 (`scene3d_vp_pass_maxz_off` 0x4C), disp32 at +75 (`scene3d_vp_pass_self_off` 0xE0), +82 (`scene3d_vp_pass_items_off` 0xE8), RIP at +89 (`scene3d_vp_pass_vftable` — the clone identity gate; slot 0 = render FUN_1801f68a0 `48 83 EC 28 4C 8B 81 ?? ?? ?? ?? 4D 85 C0 74 0C 48 8B 89 ?? ?? ?? ?? E8` whose two disp32s must equal items_off − sub_off and self_off − sub_off), disp8 at +96 (sub off, must equal the boot's 0x30), disp32 at +100 (`scene3d_vp_pass_callbacks_off` 0xF0), disp8 at +106 (`scene3d_vp_pass_name_off` 0x50), +109 (`scene3d_vp_pass_filter_off` 0x2C). The DLL byte-clones the OPACITY and TRANS objects (pass_size bytes), patches self/rect/filter and writes view/proj per frame. Unique on every build.",
    },
    SignatureDefinition {
        name: "model_pass_enable_tail",
        pattern: "48 8B 05 ?? ?? ?? ?? 83 60 ?? FE 48 8B 05 ?? ?? ?? ?? 83 60 ?? FE 48 8B 05 ?? ?? ?? ?? 83 60 ?? FE 48 8B 05 ?? ?? ?? ?? 83 60 ?? FE 83 25",
        description: "Tail of the MODEL pass constructor (FUN_1801f6510+0x2C1 on 20260825): `MOV RAX,[rip+pass]; AND dword [RAX+0x54],~1` for the four passes in ctor order (DISTANTVIEW, OPACITY, LOWPRIO_TRANS, TRANS — DAT_1806f1528/1530/1538/1540) then `AND dword [rip+DAT_1806f8244],~1` — flags bit0 = DISABLED cleared at construction. scene3d_resolve_viewport reads the RIP globals at +3/+14/+25/+36 (`scene3d_vp_pass_distant` + the three that must equal `render_graph_boot_attach`'s OPACITY/LOWPRIO/TRANS in that order) and the disp8 at +9 (all four must equal the ctor's flags off). The DISTANTVIEW global exists only for the free-node-mask-bit check (the four live filters must leave 0x08 and 0x20 clear). TWO hits on every build — the SceneGraphManager ctor (FUN_1800238a0+0xFA) re-clears the bit after storing `pass+0xE8 = graph->items` for the same four passes; the derivation accepts 1..=2 hits and requires every hit to decode the same four globals.",
    },
    SignatureDefinition {
        name: "scene_manager_camera_copy",
        pattern: "48 8B 0D ?? ?? ?? ?? 48 8B D6 48 81 C1 ?? ?? ?? ?? 41 B8 40 00 00 00 E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 48 8B D7 48 83 C1 ?? 41 B8 40 00 00 00 E8 ?? ?? ?? ?? 48 8B 0D ?? ?? ?? ?? 48 8B D7 48 83 C1 ?? 41 B8 40 00 00 00 E8",
        description: "SceneGraphManager tick (FUN_180023fb0+0xAD on 20260825): the TRANS pass VIEW copy `MOV RCX,[rip+TRANS]; MOV RDX,RSI (cam+0x08 view); ADD RCX,0x98; MOV R8D,0x40; CALL memcpy` followed by the DISTANTVIEW and OPACITY PROJ copies `MOV RCX,[rip+pass]; MOV RDX,RDI (cam+0x1C8 proj); ADD RCX,0x58; MOV R8D,0x40; CALL memcpy`. scene3d_resolve_viewport reads RIP at +3 (must equal TRANS), imm32 at +13 (`scene3d_vp_pass_view_off` 0x98), RIP at +31 (must equal DISTANTVIEW), imm8 at +41 (`scene3d_vp_pass_proj_off` 0x58), RIP at +56 (must equal OPACITY), imm8 at +66 (must equal proj off); the 0x40 matrix sizes are pinned. Stock passes get camera slot 0's matrices here every frame — a CLONE is never touched, so the DLL writes its own view/proj at these offsets. Unique on every build.",
    },
    SignatureDefinition {
        name: "viewport_setup_rect",
        pattern: "8B 47 ?? F3 0F 10 47 ?? F3 0F 10 4F ?? 44 8B 4F ?? 44 8B 47 ?? 8B 17 F3 0F 11 44 24 30 F3 0F 11 4C 24 28 49 8B CD 89 44 24 20 E8 ?? ?? ?? ?? F6 47 ?? 02 45 8D 77 08 0F 85 ?? ?? ?? ?? 0F 10 5F ?? 0F 10 57 ?? 0F 10 4F ??",
        description: "Per-viewport render setup (FUN_18026cec0+0x5A on 20260825; `(workerCtx, rect = viewport+8, list)`): `MOV EAX,[RDI+0xC] (h); MOVSS XMM0,[RDI+0x14] (maxZ); MOVSS XMM1,[RDI+0x10] (minZ); MOV R9D,[RDI+8] (w); MOV R8D,[RDI+4] (y); MOV EDX,[RDI] (x); … CALL set_viewport(ctx,x,y,w,h,minZ,maxZ) (D3D SetViewport — the rect IS honoured per pass); TEST byte [RDI+0x1C],2 (flags bit1 = skip camera upload); JNZ tail; MOVUPS XMM3,[RDI+0x20] … (PROJ rows) … [RDI+0x60] … (VIEW rows)`. scene3d_resolve_viewport reads disp8s at +2/+7/+12/+16/+20 (must be 0xC/0x14/0x10/8/4 — the `{x,y,w,h,minZ,maxZ}` rect order), +49 (`scene3d_vp_vp_flags_off` = rect off + 0x1C, must equal the ctor's flags off − sub off), +64 (proj within rect, must equal the tick's proj off − sub off − rect off) and scans forward for the first `0F 10 5F ??` after +70 (view within rect, 0x60). The DLL's clear viewport sets bit1 so only the rect is applied. Unique on every build.",
    },
    SignatureDefinition {
        name: "worker_gd_write",
        pattern: "48 8D 53 ?? 48 85 DB 75 03 48 8B D6 48 8B CF E8 ?? ?? ?? ?? 4C 8B 1B 48 8B D7 48 8B CB 41 FF 13 48 8B 87 ?? ?? ?? ?? C7 00 3A 00 04 00 48 83 C0 04 48 89 87 ?? ?? ?? ??",
        description: "Render worker loop (FUN_180272d30+0x187 on 20260825): `LEA RDX,[RBX+8] (viewport rect); TEST RBX; JNZ; MOV RDX,RSI; MOV RCX,RDI (workerCtx); CALL setup (FUN_18026cec0); MOV R11,[RBX]; MOV RDX,RDI; MOV RCX,RBX; CALL [R11] (viewport vtable slot 0 = render(viewport, workerCtx)); MOV RAX,[RDI+0x218] (gd write pointer); MOV dword [RAX],0x4003A (per-viewport terminator); ADD RAX,4; MOV [RDI+0x218],RAX`. scene3d_resolve_viewport reads disp8 at +3 (must equal the attach fn's rect off), CALL at +15 (must lie in `[viewport_setup_rect match − 0x100, match)` and start `40 53 57 41 55 41 56 41 57 48 81 EC`), disp32 at +35 (`scene3d_vp_gd_write_off` 0x218; must equal the disp32 at +52). The DLL's clear viewport's slot-0 callback writes its Clear record at `*(ctx + gd_write_off)` and advances it. Unique on every build.",
    },
    SignatureDefinition {
        name: "target_list_clear",
        pattern: "F6 42 ?? 01 48 8B FA 48 8B F1 0F 85 ?? ?? ?? ?? 48 8B 52 ?? 48 85 D2 0F 84 ?? ?? ?? ?? 48 89 5C 24 60 48 89 6C 24 68 E8 ?? ?? ?? ?? 33 ED F6 47 ?? 02 74 ?? 0F B6 47 ?? 0F B6 57 ?? 48 8B 4E 10 F3 0F 10 47 ?? 44 8B 47 ?? C1 E2 08 0B D0 0F B6 47 ?? C1 E2 08 0B D0 0F B6 47 ?? C1 E2 08 0B D0 8B 47 ?? C7 01 ?? ?? ?? ?? F3 0F 11 41 0C 89 41 04 48 8D 41 ?? 89 51 08 44 89 41 10 48 89 46 10",
        description: "Target-list render (FUN_180272600+0x07 on 20260825; `(dispatchCtx, list)`): `TEST byte [RDX+0x40],1 (list disabled); … MOV RDX,[RDX+0x38] (target surface); TEST; JZ; … TEST byte [RDI+0x40],2 (clear at start); JZ; MOVZX EAX,byte [RDI+0x24] (R); MOVZX EDX,byte [RDI+0x27] (A); MOV RCX,[RSI+0x10] (gd write); MOVSS XMM0,[RDI+0x28] (z); MOV R8D,[RDI+0x2C] (stencil); compose D3DCOLOR ARGB from bytes 0x27/0x24/0x25/0x26; MOV EAX,[RDI+0x20] (D3DCLEAR flags); MOV dword [RCX],0x00140000 (tag 0, size 0x14); MOVSS [RCX+0xC],z; MOV [RCX+4],flags; LEA RAX,[RCX+0x14]; MOV [RCX+8],color; MOV [RCX+0x10],stencil; MOV [RSI+0x10],RAX`. scene3d_resolve_viewport reads disp8 at +2 and +48 (`scene3d_vp_list_flags_off` 0x40, must agree), +19 (list target off, must equal the attach fn's), the payload offsets +55/+59/+68/+72/+98 (0x24/0x27/0x28/0x2C/0x20 — the list's clear params, informational), imm32 at +101 (`scene3d_vp_clear_tag` 0x00140000 — the record header the DLL's clear viewport emits) and disp8 at +116 (`scene3d_vp_clear_record_size` 0x14). Attests the gd Clear record `{u16 tag 0, u16 size, u32 flags, u32 D3DCOLOR, f32 z, u32 stencil}`. Unique on every build.",
    },
];

pub struct SignatureStore {
    base: *const u8,
    size: usize,
    resolved: HashMap<String, *const u8>,
    /// Cache of `CALL rel32` xrefs into the module, keyed by the call's
    /// target address. Populated once at the start of `resolve_derived`
    /// for the targets that derivation methods will look up. Allows
    /// O(N×targets×M) work to collapse into O(M).
    xref_cache: HashMap<*const u8, Vec<*const u8>>,
}

unsafe impl Send for SignatureStore {}
unsafe impl Sync for SignatureStore {}

impl SignatureStore {
    pub fn new(game_module: &GameModule) -> Self {
        Self {
            base: game_module.base,
            size: game_module.size,
            resolved: HashMap::new(),
            xref_cache: HashMap::new(),
        }
    }

    /// Look up cached `CALL rel32` xrefs to `target`, or scan the module
    /// if the target wasn't pre-batched. Derivation methods should call
    /// this instead of `scan_xrefs_to` directly so a single batched walk
    /// services every consumer.
    fn xrefs_to(&self, target: *const u8) -> Vec<*const u8> {
        if let Some(cached) = self.xref_cache.get(&target) {
            return cached.clone();
        }
        unsafe { scan_xrefs_to(self.base, self.size, target) }
    }

    /// Scan for all known signatures. Call once at startup.
    ///
    /// All ~50 patterns are resolved in a single pass over the module
    /// using the multi-pattern Aho-Corasick engine. Per-signature
    /// `[+]/[-]` log lines are emitted in `SIGNATURES` array order so
    /// log output is comparable across boots.
    pub fn resolve_all(&mut self) -> ResolveResult {
        let pattern_pairs: Vec<(&str, &str)> =
            SIGNATURES.iter().map(|s| (s.name, s.pattern)).collect();
        let mut results = scan_patterns_batch(self.base, self.size, &pattern_pairs);

        let mut missing = Vec::new();
        for sig in SIGNATURES {
            match results.remove(sig.name) {
                Some(result) => {
                    self.resolved.insert(sig.name.to_string(), result.address);
                    log_info!("  [+] {} @ +0x{:X}", sig.name, result.offset);
                }
                None => {
                    missing.push(sig.name.to_string());
                    log_warn!("  [-] {} -- pattern not found", sig.name);
                }
            }
        }

        ResolveResult {
            found: self.resolved.len(),
            total: SIGNATURES.len(),
            missing,
        }
    }

    /// Resolve derived addresses from already-found signatures.
    pub fn resolve_derived(&mut self) {
        // Promote version-specific alternatives to their canonical names
        // so downstream code can look up a single stable name.
        if self.get_address("folder_register").is_none() {
            if let Some(addr) = self.get_address("folder_register_v2") {
                self.resolved.insert("folder_register".into(), addr);
            }
        }

        self.populate_xref_cache();

        self.find_sprite_vtable();
        self.find_check_step_data_actor();
        self.derive_ultrafast_boot();
        self.find_scene_transition();
        self.find_auto_foot_panel();
        self.find_judge_notes();
        self.find_gameplay_actor_vtable();
        self.find_dance_play_sequence_vtable();
        self.find_movie_backdrop_vtables();
        self.derive_folder_functor_ctors();
        self.derive_gameplay_obj_addresses();
        self.derive_app_heap_handle();
        self.derive_file_manager_singleton();
        self.derive_render_globals();
        self.derive_layer_table();
        // Cross-checks its table load against `layer_table` — keep after it.
        self.derive_movie_screen_route();
        self.derive_player_work_table();
        self.derive_max_stage_global();
        self.derive_shutter_actor_global();
        self.derive_selectmusic_model();
        self.derive_row_builder_fn();
        self.find_option_tab_vtable();
        self.derive_option_element_ctor(
            ".?AV?$OptionElement@W4KIND@ArrowColor@option@player@ddr@@@selectmusic@sequence@@",
            "option_element_arrowcolor_ctor",
            "option_element_arrowcolor_primary_vtable",
        );
        self.derive_option_element_ctor(
            ".?AV?$OptionElement@H@selectmusic@sequence@@",
            "option_element_int_ctor",
            "option_element_int_primary_vtable",
        );
        self.derive_string_assign_via_pair();
        self.derive_event_lambda_vtable_slots();
        self.derive_textlayer_bind();
        self.derive_smarvelous_results_fallbacks();
        self.derive_customize_offset();
        self.derive_timing_config_setter();
        self.derive_bm2d_package_addresses();
        self.derive_cmovieclip_create();
        self.derive_cmovieclip_color_twins();
        self.derive_playfield_styling();
        self.derive_game_audio_addresses();
        self.derive_player_option_table();
        self.derive_gameplay_actor_layout();
        self.derive_shutter_actor_layout();
        self.derive_strip_hud_anchors();
        self.derive_frame_tick_global();
        self.derive_input_tick_function();
        self.derive_custom_resolution();
        self.find_gauge_vtables();
        self.derive_judge_rebuild_trio();
        self.derive_song_rate_runtime_sites();
        // Must follow derive_song_rate_runtime_sites: consumes the
        // uniqueness-revalidated wavebank_unregister match.
        self.derive_song_rate_io_callbacks();
        self.derive_preview_restart();
        self.derive_smarv_results_course_gate();
        self.derive_ghost_vec_copy();
        self.derive_two_player_bpl();
        self.derive_ddr_selection();
        self.derive_music_series_vslot();
        self.derive_afp_sound_callback();
        self.derive_ddr_sel_code_se();
        self.derive_ddr_sel_vo_ready();
        self.derive_ddr_sel_intro();
        // Consumes shutter_actor_layout (derived above).
        self.derive_ddr_sel_panel();
        // Consumes the SceneManageActor / MovieActor RTTI vtables
        // (find_movie_backdrop_vtables, above).
        self.derive_ddr_sel_movie();
        self.derive_hud_layout();
        self.derive_ddr_sel_stage_frame();
        // Consumes the gauge-family RTTI vtables (find_gauge_vtables, above).
        self.derive_ddr_sel_gauge();
        self.derive_combo_actor();
        self.derive_call_voice();
        // Consumes derive_combo_actor.
        self.derive_ddr_sel_combo();
        // Consumes score_actor_vtable (find_gauge_vtables, above).
        self.derive_ddr_sel_score();
        self.derive_ddr_sel_song_info();
        self.derive_ddr_sel_option_icons();
        self.derive_ddr_sel_option_force();
        self.derive_smarvelous_burst();
        self.derive_bottom_text();
        self.derive_ghost_actor_probe();
        // Consumes `cmovieclip_create` (identity gate of the bg_root site) —
        // must stay after derive_cmovieclip_create.
        self.derive_scene3d();
    }

    /// Derive the bottom-text service's two data addresses from the
    /// system-HUD tick's blank loop (`bottom_text_blank_loop`):
    ///
    /// * `bottom_text_slots` — RIP disp32 at match+15 (`LEA RBX,[rip]`): the
    ///   8 × pointer array of persistent text objects (`DAT_1806f2c90` on
    ///   20260825 — slot 0 centre CREDIT/FREE PLAY/EVENT MODE line, 1 COIN
    ///   count, 2/3 P1/P2 PASELI corners, 4–6 SOFTWARE/SYSTEM/HARDWARE ID,
    ///   7 centre network status).
    /// * `bottom_text_empty_str` — RIP disp32 at match+48 (`LEA RDX,[rip]`):
    ///   the game's own `""` literal the loop writes (`DAT_1802dda70`).
    ///
    /// Gates (all-or-nothing — a miss publishes NOTHING, and also
    /// un-resolves `bottom_text_render` so a consumer cannot install a
    /// hide-that-freezes detour without the blank capability):
    /// * both AOBs resolved, and each unique in the module;
    /// * the `CALL rel32` at match+5 decodes to `bottom_text_render` — the
    ///   AOB'd renderer IS the function this tick feeds (identity check);
    /// * the loop bound imm32 at match+20 is exactly 8;
    /// * both derived addresses lie inside the module, and the "" literal
    ///   really is a NUL byte.
    fn derive_bottom_text(&mut self) {
        const TAG: &str = "bottom_text";
        const CALL_OFF: usize = 5;
        const SLOTS_DISP_OFF: usize = 15;
        const COUNT_IMM_OFF: usize = 20;
        const EMPTY_DISP_OFF: usize = 48;
        const EXPECTED_SLOT_COUNT: u32 = 8;

        let (Some(render), Some(blank)) = (
            self.get_address("bottom_text_render"),
            self.get_address("bottom_text_blank_loop"),
        ) else {
            self.resolved.remove("bottom_text_render");
            log_warn!("  [-] {} -- renderer / blank-loop AOB unresolved", TAG);
            return;
        };
        for name in ["bottom_text_render", "bottom_text_blank_loop"] {
            let hits = self.get_all_matches(name);
            if hits.len() != 1 {
                self.resolved.remove("bottom_text_render");
                log_warn!(
                    "  [-] {} -- {} expected exactly 1 match, found {}",
                    TAG,
                    name,
                    hits.len()
                );
                return;
            }
        }
        unsafe {
            if *blank.add(CALL_OFF) != 0xE8 {
                self.resolved.remove("bottom_text_render");
                log_warn!("  [-] {} -- expected CALL rel32 at blank-loop+5", TAG);
                return;
            }
            let call_target = decode_call_rel32(blank.add(CALL_OFF));
            if call_target != render {
                self.resolved.remove("bottom_text_render");
                log_warn!(
                    "  [-] {} -- tick calls +0x{:X}, renderer AOB is +0x{:X} (identity mismatch)",
                    TAG,
                    (call_target as usize).wrapping_sub(self.base as usize),
                    (render as usize).wrapping_sub(self.base as usize)
                );
                return;
            }
            let count = (blank.add(COUNT_IMM_OFF) as *const u32).read_unaligned();
            if count != EXPECTED_SLOT_COUNT {
                self.resolved.remove("bottom_text_render");
                log_warn!(
                    "  [-] {} -- slot count imm32 is {} (expected {})",
                    TAG,
                    count,
                    EXPECTED_SLOT_COUNT
                );
                return;
            }
            let slots = decode_rip_relative(blank.add(SLOTS_DISP_OFF));
            let empty = decode_rip_relative(blank.add(EMPTY_DISP_OFF));
            let slots_off = (slots as usize).wrapping_sub(self.base as usize);
            let empty_off = (empty as usize).wrapping_sub(self.base as usize);
            // The array is 8 × 8 bytes; it must fit entirely in the module.
            if slots_off >= self.size || slots_off + 8 * 8 > self.size {
                self.resolved.remove("bottom_text_render");
                log_warn!("  [-] {} -- slot array outside module", TAG);
                return;
            }
            if empty_off >= self.size || *empty != 0 {
                self.resolved.remove("bottom_text_render");
                log_warn!(
                    "  [-] {} -- empty-string literal outside module or not NUL",
                    TAG
                );
                return;
            }
            // Everything validated — publish the pair.
            self.resolved.insert("bottom_text_slots".into(), slots);
            self.resolved.insert("bottom_text_empty_str".into(), empty);
            log_info!("  [+] bottom_text_slots (derived) @ +0x{:X}", slots_off);
            log_info!("  [+] bottom_text_empty_str (derived) @ +0x{:X}", empty_off);
        }
    }

    /// Derive the Background Movies = STAGE SCREENS route
    /// (docs/background_dancers_research.md §8) from `movie_layer_select`
    /// and `movie_actor_fit_case`:
    ///
    /// * `movie_layer_select_imm` (address) — the `MOV EAX,9` imm8 of the
    ///   MovieActor's layer choice (match+8);
    /// * `movie_fit_origin_off` / `movie_fit_size_off` (published values) —
    ///   the MovieActor fit rectangle fields (d32 at +14 / +44).
    ///
    /// All-or-nothing for the three names. Gates: both AOBs unique; the
    /// imm8 reads `09`; the `MOV RDX,[rip+disp32]` at select − 0x1B decodes
    /// to the derived `layer_table` (or at least into the module when that
    /// derivation failed); the `MOV dword [RAX+0xC],0x7FFFFFFF` draw-priority
    /// store at select − 0x11; the fit's +22 / +58 displacements are its
    /// +14 / +44 ones + 0x10 (the f64 z / depth halves); both offsets end
    /// inside the MovieActor's 0x150-byte allocation. Consumer:
    /// `mods::background_dancers::screen_route` (optional — a miss degrades
    /// STAGE SCREENS to THUMBNAIL with one WARN).
    fn derive_movie_screen_route(&mut self) {
        const TAG: &str = "movie_screen_route";
        const IMM_OFF: usize = 8;
        const STOCK_IMM: u8 = 0x09;
        const TABLE_LOAD_BACK: usize = 0x1B;
        const TABLE_LOAD: [u8; 3] = [0x48, 0x8B, 0x15];
        const PRIO_STORE_BACK: usize = 0x11;
        const PRIO_STORE: [u8; 7] = [0xC7, 0x40, 0x0C, 0xFF, 0xFF, 0xFF, 0x7F];
        const ORIGIN_DISP: usize = 14;
        const ORIGIN_Z_DISP: usize = 22;
        const SIZE_DISP: usize = 44;
        const SIZE_D_DISP: usize = 58;
        const FIT_LEN: usize = 62;
        /// `MovieActor` allocation size (SceneManageActor::onInitialize
        /// `MOV ECX,0x150`, research §7.1).
        const MOVIE_ACTOR_SIZE: usize = 0x150;
        /// The three f64s of a fit field.
        const FIELD_LEN: usize = 0x18;

        let (Some(select), Some(fit)) = (
            self.get_address("movie_layer_select"),
            self.get_address("movie_actor_fit_case"),
        ) else {
            log_warn!("  [-] {} -- layer-select / fit-case AOB unresolved", TAG);
            return;
        };
        for name in ["movie_layer_select", "movie_actor_fit_case"] {
            let hits = self.get_all_matches(name);
            if hits.len() != 1 {
                log_warn!(
                    "  [-] {} -- {} expected exactly 1 match, found {}",
                    TAG,
                    name,
                    hits.len()
                );
                return;
            }
        }
        let base = self.base as usize;
        let inside = |p: usize, len: usize| {
            p.wrapping_sub(base) < self.size
                && p.wrapping_sub(base).saturating_add(len) <= self.size
        };
        let select_addr = select as usize;
        if select_addr.wrapping_sub(base) < TABLE_LOAD_BACK
            || !inside(select_addr - TABLE_LOAD_BACK, TABLE_LOAD_BACK + 25)
            || !inside(fit as usize, FIT_LEN)
        {
            log_warn!("  [-] {} -- match window outside module", TAG);
            return;
        }
        unsafe {
            let imm = *select.add(IMM_OFF);
            if imm != STOCK_IMM {
                log_warn!(
                    "  [-] {} -- layer-select imm reads 0x{:02X} (expected 0x{:02X})",
                    TAG,
                    imm,
                    STOCK_IMM
                );
                return;
            }
            let load = select.sub(TABLE_LOAD_BACK);
            if std::slice::from_raw_parts(load, 3) != TABLE_LOAD {
                log_warn!("  [-] {} -- no MOV RDX,[rip] at layer-select − 0x1B", TAG);
                return;
            }
            let table = decode_rip_relative(load.add(3));
            match self.get_address("layer_table") {
                Some(t) if t != table => {
                    log_warn!(
                        "  [-] {} -- layer-select loads +0x{:X}, layer_table is +0x{:X} (identity mismatch)",
                        TAG,
                        (table as usize).wrapping_sub(base),
                        (t as usize).wrapping_sub(base)
                    );
                    return;
                }
                Some(_) => {}
                None => {
                    if !inside(table as usize, 8) {
                        log_warn!("  [-] {} -- layer-table global outside module", TAG);
                        return;
                    }
                }
            }
            if std::slice::from_raw_parts(select.sub(PRIO_STORE_BACK), PRIO_STORE.len())
                != PRIO_STORE
            {
                log_warn!(
                    "  [-] {} -- draw-priority store absent at layer-select − 0x11",
                    TAG
                );
                return;
            }
            let disp = |o: usize| (fit.add(o) as *const i32).read_unaligned();
            let (origin, origin_z, size, size_d) = (
                disp(ORIGIN_DISP),
                disp(ORIGIN_Z_DISP),
                disp(SIZE_DISP),
                disp(SIZE_D_DISP),
            );
            let plausible = |d: i32| d > 0 && (d as usize) + FIELD_LEN <= MOVIE_ACTOR_SIZE;
            if origin_z != origin.wrapping_add(0x10)
                || size_d != size.wrapping_add(0x10)
                || !plausible(origin)
                || !plausible(size)
            {
                log_warn!(
                    "  [-] {} -- fit displacements implausible (origin 0x{:X}/0x{:X}, size 0x{:X}/0x{:X})",
                    TAG,
                    origin,
                    origin_z,
                    size,
                    size_d
                );
                return;
            }
            // Everything validated — publish the three names.
            let imm_addr = select.add(IMM_OFF);
            self.resolved
                .insert("movie_layer_select_imm".into(), imm_addr);
            log_info!(
                "  [+] movie_layer_select_imm (derived) @ +0x{:X}",
                (imm_addr as usize).wrapping_sub(base)
            );
            self.publish_value("movie_fit_origin_off", origin as usize);
            self.publish_value("movie_fit_size_off", size as usize);
        }
    }

    /// MovieActor fit-origin field (f64 x, y, z) offset — see
    /// `derive_movie_screen_route` — or `None`.
    pub fn movie_fit_origin_off(&self) -> Option<usize> {
        self.published_value("movie_fit_origin_off")
    }

    /// MovieActor fit-size field (f64 w, h, d) offset — see
    /// `derive_movie_screen_route` — or `None`.
    pub fn movie_fit_size_off(&self) -> Option<usize> {
        self.published_value("movie_fit_size_off")
    }

    // ── Background Dancers: the `scene3d` group ─────────────────────────
    //
    // RE record: docs/background_dancers_research.md §1 (the byte-level spec
    // every offset below is decoded from). One all-or-nothing derivation over
    // twelve AOBs: a miss or a failed identity gate publishes NOTHING and also
    // un-resolves the raw AOB names, so `services::scene3d` either gets the
    // whole `Scene3dSites` bundle or reports unavailable.

    /// Raw AOB names feeding `derive_scene3d` (un-resolved on any failure).
    const SCENE3D_RAW: &'static [&'static str] = &[
        "sg_enable_bit_site",
        "sg_manager_tick",
        "sg_active_camera",
        "sg_destroy_flush",
        "sg_update_job_run",
        "sg_insertion_sort",
        "model_registry_release",
        "texture_create_site",
        "texture_release",
        "bgmovie_readiness",
        "bgmovie_ready_call_site",
        "bg_root_create_site",
    ];

    /// Every name `derive_scene3d` publishes (addresses and values alike).
    const SCENE3D_PUBLISHED: &'static [&'static str] = &[
        "scene3d_scene_graph_manager",
        "scene3d_mgr_destroy_vec_off",
        "scene3d_mgr_mutex_off",
        "scene3d_mgr_depth_off",
        "scene3d_mgr_rate_off",
        "scene3d_mutex_lock_iat",
        "scene3d_mutex_unlock_iat",
        "scene3d_graph_flags_off",
        "scene3d_graph_root_child_off",
        "scene3d_graph_sort_flag_off",
        "scene3d_graph_camera_vec_off",
        "scene3d_node_item_off",
        "scene3d_node_sort_key_off",
        "scene3d_scene_graph_update",
        "scene3d_camera_stride",
        "scene3d_camera_active_off",
        "scene3d_camera_view_off",
        "scene3d_camera_proj_dirty_off",
        "scene3d_cam_eye_off",
        "scene3d_cam_target_off",
        "scene3d_cam_up_off",
        "scene3d_cam_w_off",
        "scene3d_cam_l_off",
        "scene3d_cam_r_off",
        "scene3d_cam_b_off",
        "scene3d_cam_t_off",
        "scene3d_cam_near_off",
        "scene3d_cam_far_off",
        "scene3d_cam_view_dirty_off",
        "scene3d_cam_proj_req_off",
        "scene3d_resource_manager",
        "scene3d_rm_model_mutex_off",
        "scene3d_rm_model_map_off",
        "scene3d_rm_node_nil_off",
        "scene3d_rm_node_key_off",
        "scene3d_rm_node_right_off",
        "scene3d_rm_node_value_off",
        "scene3d_rm_node_refcount_off",
        "scene3d_texture_create",
        "scene3d_texture_release",
        "scene3d_bgmovie_actor",
        "scene3d_bgframe_off",
        "scene3d_bg_clip_slot_off",
        "scene3d_cmovieclip_pool",
        "scene3d_cmovieclip_pool_stride",
        "scene3d_cmovieclip_pool_count",
    ];

    /// Derive the whole `scene3d` group or nothing (see the module comment
    /// above and `docs/background_dancers_research.md` §1.7 for the per-site
    /// decode table and identity gates).
    fn derive_scene3d(&mut self) {
        match self.scene3d_resolve() {
            Ok(s) => {
                let base = self.base as usize;
                let rel = |p: *const u8| (p as usize).wrapping_sub(base);
                let mut addr = |name: &str, p: *const u8| {
                    self.resolved.insert(name.into(), p);
                    log_info!("  [+] {} (derived) @ +0x{:X}", name, rel(p));
                };
                addr("scene3d_scene_graph_manager", s.scene_graph_manager);
                addr("scene3d_mutex_lock_iat", s.mutex_lock_iat);
                addr("scene3d_mutex_unlock_iat", s.mutex_unlock_iat);
                addr("scene3d_scene_graph_update", s.scene_graph_update);
                addr("scene3d_resource_manager", s.resource_manager);
                addr("scene3d_texture_create", s.texture_create);
                addr("scene3d_texture_release", s.texture_release);
                addr("scene3d_bgmovie_actor", s.bgmovie_actor);
                addr("scene3d_cmovieclip_pool", s.cmovieclip_pool);
                for (name, v) in [
                    ("scene3d_mgr_destroy_vec_off", s.mgr_destroy_vec_off),
                    ("scene3d_mgr_mutex_off", s.mgr_mutex_off),
                    ("scene3d_mgr_depth_off", s.mgr_depth_off),
                    ("scene3d_mgr_rate_off", s.mgr_rate_off),
                    ("scene3d_graph_flags_off", s.graph_flags_off),
                    ("scene3d_graph_root_child_off", s.graph_root_child_off),
                    ("scene3d_graph_sort_flag_off", s.graph_sort_flag_off),
                    ("scene3d_graph_camera_vec_off", s.graph_camera_vec_off),
                    ("scene3d_node_item_off", s.node_item_off),
                    ("scene3d_node_sort_key_off", s.node_sort_key_off),
                    ("scene3d_camera_stride", s.camera_stride),
                    ("scene3d_camera_active_off", s.camera_active_off),
                    ("scene3d_camera_view_off", s.camera_view_off),
                    ("scene3d_camera_proj_dirty_off", s.camera_proj_dirty_off),
                    ("scene3d_cam_eye_off", s.cam_eye_off),
                    ("scene3d_cam_target_off", s.cam_target_off),
                    ("scene3d_cam_up_off", s.cam_up_off),
                    ("scene3d_cam_w_off", s.cam_w_off),
                    ("scene3d_cam_l_off", s.cam_l_off),
                    ("scene3d_cam_r_off", s.cam_r_off),
                    ("scene3d_cam_b_off", s.cam_b_off),
                    ("scene3d_cam_t_off", s.cam_t_off),
                    ("scene3d_cam_near_off", s.cam_near_off),
                    ("scene3d_cam_far_off", s.cam_far_off),
                    ("scene3d_cam_view_dirty_off", s.cam_view_dirty_off),
                    ("scene3d_cam_proj_req_off", s.cam_proj_req_off),
                    ("scene3d_rm_model_mutex_off", s.rm_model_mutex_off),
                    ("scene3d_rm_model_map_off", s.rm_model_map_off),
                    ("scene3d_rm_node_nil_off", s.rm_node_nil_off),
                    ("scene3d_rm_node_key_off", s.rm_node_key_off),
                    ("scene3d_rm_node_right_off", s.rm_node_right_off),
                    ("scene3d_rm_node_value_off", s.rm_node_value_off),
                    ("scene3d_rm_node_refcount_off", s.rm_node_refcount_off),
                    ("scene3d_bgframe_off", s.bgframe_off),
                    ("scene3d_bg_clip_slot_off", s.bg_clip_slot_off),
                    ("scene3d_cmovieclip_pool_stride", s.cmovieclip_pool_stride),
                    ("scene3d_cmovieclip_pool_count", s.cmovieclip_pool_count),
                ] {
                    self.publish_value(name, v);
                }
                // Optional sub-group: the gs texture-registry lookup trio. A
                // miss here never un-resolves the group — the render-item
                // builder just keeps the converter's texture pointers.
                match self.scene3d_resolve_texture_lookup() {
                    Ok(t) => {
                        let rel = |p: *const u8| (p as usize).wrapping_sub(base);
                        for (name, p) in [
                            ("scene3d_texture_lookup", t.lookup),
                            ("scene3d_texture_default", t.default_texture),
                            ("scene3d_texture_spin", t.spin),
                        ] {
                            self.resolved.insert(name.into(), p);
                            log_info!("  [+] {} (derived) @ +0x{:X}", name, rel(p));
                        }
                    }
                    Err(reason) => {
                        for name in Self::SCENE3D_TEXTURE_LOOKUP {
                            self.resolved.remove(*name);
                        }
                        log_warn!(
                            "  [-] scene3d_texture_lookup (optional) -- {} -- material textures keep the converter's pointers",
                            reason
                        );
                    }
                }
                // Optional sub-group: the shader-registry lookup (whole-scene
                // restyle). A miss only disables the material re-point.
                match self.scene3d_resolve_shader_lookup() {
                    Ok(p) => {
                        self.resolved.insert("scene3d_shader_lookup".into(), p);
                        log_info!("  [+] scene3d_shader_lookup (derived) @ +0x{:X}", rel(p));
                    }
                    Err(reason) => {
                        for name in Self::SCENE3D_SHADER_LOOKUP {
                            self.resolved.remove(*name);
                        }
                        log_warn!(
                            "  [-] scene3d_shader_lookup (optional) -- {} -- scene restyle (lit/cel) unavailable",
                            reason
                        );
                    }
                }
                // Optional sub-group: the viewport-pass compositor (option
                // previews). A miss only disables the live 3D previews.
                match self.scene3d_resolve_viewport() {
                    Ok(v) => self.publish_viewport(&v),
                    Err(reason) => {
                        for name in Self::SCENE3D_VIEWPORT {
                            self.resolved.remove(*name);
                        }
                        log_warn!(
                            "  [-] scene3d_viewport (optional) -- {} -- 3D option previews unavailable",
                            reason
                        );
                    }
                }
            }
            Err((site, reason)) => {
                for name in Self::SCENE3D_PUBLISHED
                    .iter()
                    .chain(Self::SCENE3D_RAW)
                    .chain(Self::SCENE3D_TEXTURE_LOOKUP)
                    .chain(Self::SCENE3D_SHADER_LOOKUP)
                    .chain(Self::SCENE3D_VIEWPORT)
                {
                    self.resolved.remove(*name);
                }
                log_warn!("  [-] scene3d -- {}: {}", site, reason);
            }
        }
    }

    /// The optional viewport-pass sub-group's names (raw AOBs + published
    /// addresses + published values).
    const SCENE3D_VIEWPORT: &'static [&'static str] = &[
        "render_graph_boot_attach",
        "render_graph_2d_attach",
        "viewport_detach",
        "model_pass_ctor",
        "model_pass_enable_tail",
        "scene_manager_camera_copy",
        "viewport_setup_rect",
        "worker_gd_write",
        "target_list_clear",
        "scene3d_vp_display",
        "scene3d_vp_attach",
        "scene3d_vp_detach",
        "scene3d_vp_pass_distant",
        "scene3d_vp_pass_opacity",
        "scene3d_vp_pass_lowprio",
        "scene3d_vp_pass_trans",
        "scene3d_vp_pass_vftable",
        "scene3d_vp_render2d_list_off",
        "scene3d_vp_render3d_list_off",
        "scene3d_vp_list_flags_off",
        "scene3d_vp_list_target_off",
        "scene3d_vp_target_w_off",
        "scene3d_vp_target_h_off",
        "scene3d_vp_pass_size",
        "scene3d_vp_sub_off",
        "scene3d_vp_pass_sort_off",
        "scene3d_vp_pass_filter_off",
        "scene3d_vp_pass_rect_off",
        "scene3d_vp_pass_minz_off",
        "scene3d_vp_pass_maxz_off",
        "scene3d_vp_pass_name_off",
        "scene3d_vp_pass_flags_off",
        "scene3d_vp_pass_proj_off",
        "scene3d_vp_pass_view_off",
        "scene3d_vp_pass_self_off",
        "scene3d_vp_pass_items_off",
        "scene3d_vp_pass_callbacks_off",
        "scene3d_vp_vp_rect_off",
        "scene3d_vp_vp_flags_off",
        "scene3d_vp_gd_write_off",
        "scene3d_vp_clear_tag",
        "scene3d_vp_clear_record_size",
    ];

    /// Publish a resolved viewport sub-group (addresses into `resolved`,
    /// offsets/values via `publish_value`) with the boot-log lines.
    fn publish_viewport(&mut self, v: &Scene3dViewportSites) {
        let base = self.base as usize;
        let rel = |p: *const u8| (p as usize).wrapping_sub(base);
        for (name, p) in [
            ("scene3d_vp_display", v.display),
            ("scene3d_vp_attach", v.attach),
            ("scene3d_vp_detach", v.detach),
            ("scene3d_vp_pass_distant", v.pass_globals[0]),
            ("scene3d_vp_pass_opacity", v.pass_globals[1]),
            ("scene3d_vp_pass_lowprio", v.pass_globals[2]),
            ("scene3d_vp_pass_trans", v.pass_globals[3]),
            ("scene3d_vp_pass_vftable", v.pass_vftable),
        ] {
            self.resolved.insert(name.into(), p);
            log_info!("  [+] {} (derived) @ +0x{:X}", name, rel(p));
        }
        for (name, value) in [
            ("scene3d_vp_render2d_list_off", v.render2d_list_off),
            ("scene3d_vp_render3d_list_off", v.render3d_list_off),
            ("scene3d_vp_list_flags_off", v.list_flags_off),
            ("scene3d_vp_list_target_off", v.list_target_off),
            ("scene3d_vp_target_w_off", v.target_w_off),
            ("scene3d_vp_target_h_off", v.target_h_off),
            ("scene3d_vp_pass_size", v.pass_size),
            ("scene3d_vp_sub_off", v.sub_off),
            ("scene3d_vp_pass_sort_off", v.pass_sort_off),
            ("scene3d_vp_pass_filter_off", v.pass_filter_off),
            ("scene3d_vp_pass_rect_off", v.pass_rect_off),
            ("scene3d_vp_pass_minz_off", v.pass_minz_off),
            ("scene3d_vp_pass_maxz_off", v.pass_maxz_off),
            ("scene3d_vp_pass_name_off", v.pass_name_off),
            ("scene3d_vp_pass_flags_off", v.pass_flags_off),
            ("scene3d_vp_pass_proj_off", v.pass_proj_off),
            ("scene3d_vp_pass_view_off", v.pass_view_off),
            ("scene3d_vp_pass_self_off", v.pass_self_off),
            ("scene3d_vp_pass_items_off", v.pass_items_off),
            ("scene3d_vp_pass_callbacks_off", v.pass_callbacks_off),
            ("scene3d_vp_vp_rect_off", v.vp_rect_off),
            ("scene3d_vp_vp_flags_off", v.vp_flags_off),
            ("scene3d_vp_gd_write_off", v.gd_write_off),
            ("scene3d_vp_clear_tag", v.clear_tag as usize),
            ("scene3d_vp_clear_record_size", v.clear_record_size),
        ] {
            self.publish_value(name, value);
        }
    }

    /// Decode the nine viewport-pass AOBs into one cross-checked bundle
    /// (research `preview-compositing.md`; every offset is read from the
    /// engine's own loads/stores and each site's reading of a shared field
    /// must agree with every other site's).
    fn scene3d_resolve_viewport(&self) -> Result<Scene3dViewportSites, String> {
        let inside = |p: *const u8, len: usize| self.scene3d_inside(p, len);
        // Unaligned little-endian readers over module memory (the AOB match
        // guarantees the pattern's own bytes are mapped; anything reached
        // through a decoded pointer is probed with `inside` first).
        let u8_at = |p: *const u8, off: usize| unsafe { *p.add(off) } as usize;
        let u32_at =
            |p: *const u8, off: usize| unsafe { (p.add(off) as *const u32).read_unaligned() };
        let rip_at = |p: *const u8, off: usize| unsafe { decode_rip_relative(p.add(off)) };
        let call_at = |p: *const u8, off: usize| unsafe { decode_call_rel32(p.add(off)) };
        let unique = |name: &str| -> Result<*const u8, String> {
            let hits = self.get_all_matches(name);
            match hits.len() {
                1 => Ok(hits[0]),
                n => Err(format!("{name}: expected exactly 1 match, found {n}")),
            }
        };
        let agree = |what: &str, a: usize, b: usize| -> Result<(), String> {
            if a == b {
                Ok(())
            } else {
                Err(format!("{what} disagree (0x{a:X} vs 0x{b:X})"))
            }
        };
        let agree_p = |what: &str, a: *const u8, b: *const u8| -> Result<(), String> {
            agree(what, a as usize, b as usize)
        };
        /// Compare module bytes against a masked template (`None` = wildcard).
        fn shape_ok(body: &[u8], want: &[Option<u8>]) -> bool {
            body.len() >= want.len()
                && want
                    .iter()
                    .zip(body)
                    .all(|(w, got)| w.is_none_or(|w| w == *got))
        }

        // ── 1. Boot: the three MODEL pass attaches (RENDER-3D) ────────────
        let boot = unique("render_graph_boot_attach")?;
        let mut pass3 = [std::ptr::null::<u8>(); 3];
        let mut display = std::ptr::null::<u8>();
        let mut attach = std::ptr::null::<u8>();
        let mut sub_off = 0usize;
        let mut render3d_list_off = 0usize;
        for k in 0..3 {
            let b = unsafe { boot.add(33 * k) };
            let pass = rip_at(b, 3);
            let sub = u8_at(b, 10);
            let disp = rip_at(b, 20);
            let list = u8_at(b, 27);
            let fun = call_at(b, 28);
            if k == 0 {
                display = disp;
                attach = fun;
                sub_off = sub;
                render3d_list_off = list;
            } else {
                agree_p("boot display globals", disp, display)?;
                agree_p("boot attach callees", fun, attach)?;
                agree("boot viewport sub offsets", sub, sub_off)?;
                agree("boot RENDER-3D list offsets", list, render3d_list_off)?;
            }
            pass3[k] = pass;
        }
        if pass3[0] == pass3[1] || pass3[1] == pass3[2] || pass3[0] == pass3[2] {
            return Err("boot pass globals are not distinct".into());
        }
        if !inside(display, 8) || !inside(attach, 0x60) {
            return Err("boot display/attach outside the module".into());
        }
        for p in pass3 {
            if !inside(p, 8) {
                return Err("boot pass global outside the module".into());
            }
        }
        // Attach callee identity + the list/target/rect facts it reads.
        //   48 89 5C 24 08 57 48 83 EC 30 48 8B FA 48 8B D9 48 85 D2 74 06
        //   4C 8D 4A ?? (rect = viewport+8) EB 03 45 33 C9 48 8B 41 ?? (target)
        //   48 85 C0 74 ?? 41 83 39 00 75 ?? 41 83 79 04 00 75 ?? 41 83 79 08
        //   00 75 ?? 41 83 79 0C 00 75 ?? 0F B7 40 ?? (w) 41 89 41 08
        //   48 8B 41 ?? 0F B7 48 ?? (h) 41 89 49 0C
        const ATTACH_SHAPE: &[Option<u8>] = &[
            Some(0x48),
            Some(0x89),
            Some(0x5C),
            Some(0x24),
            Some(0x08),
            Some(0x57),
            Some(0x48),
            Some(0x83),
            Some(0xEC),
            Some(0x30),
            Some(0x48),
            Some(0x8B),
            Some(0xFA),
            Some(0x48),
            Some(0x8B),
            Some(0xD9),
            Some(0x48),
            Some(0x85),
            Some(0xD2),
            Some(0x74),
            Some(0x06),
            Some(0x4C),
            Some(0x8D),
            Some(0x4A),
            None,
            Some(0xEB),
            Some(0x03),
            Some(0x45),
            Some(0x33),
            Some(0xC9),
            Some(0x48),
            Some(0x8B),
            Some(0x41),
            None,
            Some(0x48),
            Some(0x85),
            Some(0xC0),
            Some(0x74),
            None,
            Some(0x41),
            Some(0x83),
            Some(0x39),
            Some(0x00),
            Some(0x75),
            None,
            Some(0x41),
            Some(0x83),
            Some(0x79),
            Some(0x04),
            Some(0x00),
            Some(0x75),
            None,
            Some(0x41),
            Some(0x83),
            Some(0x79),
            Some(0x08),
            Some(0x00),
            Some(0x75),
            None,
            Some(0x41),
            Some(0x83),
            Some(0x79),
            Some(0x0C),
            Some(0x00),
            Some(0x75),
            None,
            Some(0x0F),
            Some(0xB7),
            Some(0x40),
            None,
            Some(0x41),
            Some(0x89),
            Some(0x41),
            Some(0x08),
            Some(0x48),
            Some(0x8B),
            Some(0x41),
            None,
            Some(0x0F),
            Some(0xB7),
            Some(0x48),
            None,
            Some(0x41),
            Some(0x89),
            Some(0x49),
            Some(0x0C),
        ];
        let attach_body = unsafe { std::slice::from_raw_parts(attach, ATTACH_SHAPE.len()) };
        if !shape_ok(attach_body, ATTACH_SHAPE) {
            return Err("attach callee body is not the target-list push+sort".into());
        }
        let vp_rect_off = u8_at(attach, 24);
        let list_target_off = u8_at(attach, 33);
        agree("attach target loads", u8_at(attach, 77), list_target_off)?;
        let target_w_off = u8_at(attach, 69);
        let target_h_off = u8_at(attach, 81);
        if target_h_off != target_w_off + 2 {
            return Err("target dims are not adjacent u16s".into());
        }

        // ── 2. Boot: the three 2D-list attaches (RENDER_2D) ───────────────
        let boot2d = unique("render_graph_2d_attach")?;
        let mut render2d_list_off = 0usize;
        for k in 0..3 {
            let b = unsafe { boot2d.add(29 * k) };
            agree_p("2D attach display global", rip_at(b, 16), display)?;
            agree_p("2D attach callee", call_at(b, 24), attach)?;
            let list = u8_at(b, 23);
            if k == 0 {
                render2d_list_off = list;
            } else {
                agree("2D attach list offsets", list, render2d_list_off)?;
            }
        }
        if render2d_list_off == render3d_list_off {
            return Err("RENDER_2D list offset equals the RENDER-3D one".into());
        }

        // ── 3. Shutdown: the three MODEL pass detaches ────────────────────
        let shut = unique("viewport_detach")?;
        let mut detach = std::ptr::null::<u8>();
        // Skip the leading 30-byte `ADD RDX,imm32` anchor block.
        for k in 0..3 {
            let b = unsafe { shut.add(30 + 27 * k) };
            agree_p("detach display global", rip_at(b, 3), display)?;
            agree_p("detach pass global", rip_at(b, 10), pass3[k])?;
            agree("detach list offset", u8_at(b, 17), render3d_list_off)?;
            agree("detach sub offset", u8_at(b, 21), sub_off)?;
            let fun = call_at(b, 22);
            if k == 0 {
                detach = fun;
            } else {
                agree_p("detach callees", fun, detach)?;
            }
        }
        if !inside(detach, 0x30) {
            return Err("detach callee outside the module".into());
        }
        const DETACH_SHAPE: &[Option<u8>] = &[
            Some(0x4C),
            Some(0x8B),
            Some(0x41),
            Some(0x08),
            Some(0x48),
            Some(0x8B),
            Some(0x01),
            Some(0x4C),
            Some(0x8B),
            Some(0xCA),
            Some(0x4C),
            Some(0x8B),
            Some(0xD1),
            Some(0x49),
            Some(0x3B),
            Some(0xC0),
            Some(0x74),
            None,
            Some(0x48),
            Some(0x39),
            Some(0x10),
            Some(0x74),
            None,
            Some(0x48),
            Some(0x83),
            Some(0xC0),
            Some(0x10),
        ];
        let detach_body = unsafe { std::slice::from_raw_parts(detach, DETACH_SHAPE.len()) };
        if !shape_ok(detach_body, DETACH_SHAPE) {
            return Err("detach callee body is not the target-list erase".into());
        }

        // ── 4. The pass constructor's store block ─────────────────────────
        let ctor = unique("model_pass_ctor")?;
        let pass_size = u32_at(ctor, 1) as usize;
        let pass_sort_off = u8_at(ctor, 44);
        let pass_flags_off = u8_at(ctor, 52);
        let pass_rect_off = u8_at(ctor, 56);
        agree("ctor rect second half", u8_at(ctor, 60), pass_rect_off + 8)?;
        let pass_minz_off = u8_at(ctor, 64);
        let pass_maxz_off = u8_at(ctor, 67);
        let pass_self_off = u32_at(ctor, 75) as usize;
        let pass_items_off = u32_at(ctor, 82) as usize;
        let pass_vftable = rip_at(ctor, 89);
        agree("ctor viewport sub offset", u8_at(ctor, 96), sub_off)?;
        let pass_callbacks_off = u32_at(ctor, 100) as usize;
        let pass_name_off = u8_at(ctor, 106);
        let pass_filter_off = u8_at(ctor, 109);
        agree(
            "ctor rect vs attach rect",
            pass_rect_off,
            sub_off + vp_rect_off,
        )?;
        if pass_minz_off != pass_rect_off + 0x10 || pass_maxz_off != pass_minz_off + 4 {
            return Err("ctor minZ/maxZ do not follow the rect".into());
        }
        if !inside(pass_vftable, 16) {
            return Err("pass vftable outside the module".into());
        }
        // vftable slot 0 = render(viewport, ctx): `SUB RSP,0x28; MOV R8,[RCX+
        // items−sub]; TEST R8; JZ; MOV RCX,[RCX+self−sub]; CALL collect`.
        let render_fn = unsafe { (pass_vftable as *const *const u8).read_unaligned() };
        if !inside(render_fn, 0x24) {
            return Err("pass vftable slot 0 outside the module".into());
        }
        const RENDER_SHAPE: &[Option<u8>] = &[
            Some(0x48),
            Some(0x83),
            Some(0xEC),
            Some(0x28),
            Some(0x4C),
            Some(0x8B),
            Some(0x81),
            None,
            None,
            None,
            None,
            Some(0x4D),
            Some(0x85),
            Some(0xC0),
            Some(0x74),
            Some(0x0C),
            Some(0x48),
            Some(0x8B),
            Some(0x89),
            None,
            None,
            None,
            None,
            Some(0xE8),
        ];
        let render_body = unsafe { std::slice::from_raw_parts(render_fn, RENDER_SHAPE.len()) };
        if !shape_ok(render_body, RENDER_SHAPE) {
            return Err("pass vftable slot 0 is not the MODEL pass render fn".into());
        }
        agree(
            "render fn items load vs ctor items off",
            u32_at(render_fn, 7) as usize,
            pass_items_off - sub_off,
        )?;
        agree(
            "render fn outer load vs ctor self off",
            u32_at(render_fn, 19) as usize,
            pass_self_off - sub_off,
        )?;
        for (what, off) in [
            ("self", pass_self_off),
            ("items", pass_items_off),
            ("callbacks", pass_callbacks_off),
        ] {
            if off + 8 > pass_size {
                return Err(format!("ctor {what} offset 0x{off:X} beyond pass size"));
            }
        }

        // ── 5. The ctor tail: the four pass globals in ctor order ─────────
        // Two sites share the shape (the pass ctor's tail and the
        // SceneGraphManager ctor, which re-clears the bit after storing the
        // item list); both must decode the same four globals.
        let tails = self.get_all_matches("model_pass_enable_tail");
        if tails.is_empty() || tails.len() > 2 {
            return Err(format!(
                "model_pass_enable_tail: expected 1..=2 matches, found {}",
                tails.len()
            ));
        }
        let mut pass_globals = [std::ptr::null::<u8>(); 4];
        for (i, tail) in tails.iter().enumerate() {
            for k in 0..4 {
                let b = unsafe { tail.add(11 * k) };
                let g = rip_at(b, 3);
                if i == 0 {
                    pass_globals[k] = g;
                } else {
                    agree_p("enable-tail pass globals across sites", g, pass_globals[k])?;
                }
                agree("enable-tail flags offset", u8_at(b, 9), pass_flags_off)?;
            }
        }
        agree_p("ctor tail OPACITY", pass_globals[1], pass3[0])?;
        agree_p("ctor tail LOWPRIO", pass_globals[2], pass3[1])?;
        agree_p("ctor tail TRANS", pass_globals[3], pass3[2])?;
        if !inside(pass_globals[0], 8) || pass3.contains(&pass_globals[0]) {
            return Err("DISTANTVIEW pass global invalid".into());
        }

        // ── 6. The manager tick's view/proj memcpys ───────────────────────
        let cam = unique("scene_manager_camera_copy")?;
        agree_p("camera copy TRANS global", rip_at(cam, 3), pass3[2])?;
        let pass_view_off = u32_at(cam, 13) as usize;
        agree_p(
            "camera copy DISTANT global",
            rip_at(cam, 31),
            pass_globals[0],
        )?;
        let pass_proj_off = u8_at(cam, 41);
        agree_p("camera copy OPACITY global", rip_at(cam, 56), pass3[0])?;
        agree("camera copy proj offsets", u8_at(cam, 66), pass_proj_off)?;
        if pass_view_off + 0x40 > pass_size || pass_proj_off + 0x40 > pass_size {
            return Err("view/proj matrices beyond pass size".into());
        }

        // ── 7. The per-viewport setup: rect order, flags bit, matrix spots ─
        let setup = unique("viewport_setup_rect")?;
        if [(2usize, 0xCusize), (7, 0x14), (12, 0x10), (16, 8), (20, 4)]
            .iter()
            .any(|&(at, want)| u8_at(setup, at) != want)
        {
            return Err("setup rect field order is not {x,y,w,h,minZ,maxZ}".into());
        }
        let vp_flags_off = vp_rect_off + u8_at(setup, 49);
        agree(
            "setup flags vs ctor flags",
            vp_flags_off,
            pass_flags_off - sub_off,
        )?;
        agree(
            "setup proj vs tick proj",
            vp_rect_off + u8_at(setup, 64),
            pass_proj_off - sub_off,
        )?;
        // The VIEW rows start at the first `MOVUPS XMM3,[RDI+disp8]` after
        // the projection block.
        let mut view_in_rect = None;
        if !inside(setup, 0xC0) {
            return Err("setup match at module edge".into());
        }
        for off in 73..0xB0 {
            if u8_at(setup, off) == 0x0F
                && u8_at(setup, off + 1) == 0x10
                && u8_at(setup, off + 2) == 0x5F
            {
                view_in_rect = Some(u8_at(setup, off + 3));
                break;
            }
        }
        let Some(view_in_rect) = view_in_rect else {
            return Err("setup view rows not found".into());
        };
        agree(
            "setup view vs tick view",
            vp_rect_off + view_in_rect,
            pass_view_off - sub_off,
        )?;

        // ── 8. The worker: gd write pointer + the setup CALL ──────────────
        let worker = unique("worker_gd_write")?;
        agree("worker rect offset", u8_at(worker, 3), vp_rect_off)?;
        let setup_fn = call_at(worker, 15);
        let setup_rel = (setup as usize).wrapping_sub(setup_fn as usize);
        if setup_fn.is_null() || setup_rel >= 0x100 {
            return Err("worker's setup callee does not contain the setup match".into());
        }
        const SETUP_PROLOGUE: &[u8] = &[
            0x40, 0x53, 0x57, 0x41, 0x55, 0x41, 0x56, 0x41, 0x57, 0x48, 0x81, 0xEC,
        ];
        if !inside(setup_fn, SETUP_PROLOGUE.len())
            || unsafe { std::slice::from_raw_parts(setup_fn, SETUP_PROLOGUE.len()) }
                != SETUP_PROLOGUE
        {
            return Err("worker's setup callee prologue mismatch".into());
        }
        let gd_write_off = u32_at(worker, 35) as usize;
        agree(
            "worker gd write offsets",
            u32_at(worker, 52) as usize,
            gd_write_off,
        )?;

        // ── 9. The target-list render: flags/target offsets + Clear shape ─
        let tl = unique("target_list_clear")?;
        let list_flags_off = u8_at(tl, 2);
        agree("list flags offsets", u8_at(tl, 48), list_flags_off)?;
        agree(
            "list target offset vs attach",
            u8_at(tl, 19),
            list_target_off,
        )?;
        let clear_tag = u32_at(tl, 101);
        let clear_record_size = u8_at(tl, 116);
        if clear_tag & 0xFFFF != 0 || (clear_tag >> 16) as usize != clear_record_size {
            return Err(format!(
                "Clear record header 0x{clear_tag:08X} vs size 0x{clear_record_size:X}"
            ));
        }
        if clear_record_size != 0x14 {
            return Err(format!("Clear record size 0x{clear_record_size:X} != 0x14"));
        }

        Ok(Scene3dViewportSites {
            display,
            render2d_list_off,
            render3d_list_off,
            attach,
            detach,
            list_flags_off,
            list_target_off,
            target_w_off,
            target_h_off,
            pass_globals,
            pass_size,
            sub_off,
            pass_vftable,
            pass_sort_off,
            pass_filter_off,
            pass_rect_off,
            pass_minz_off,
            pass_maxz_off,
            pass_name_off,
            pass_flags_off,
            pass_proj_off,
            pass_view_off,
            pass_self_off,
            pass_items_off,
            pass_callbacks_off,
            vp_rect_off,
            vp_flags_off,
            gd_write_off,
            clear_tag,
            clear_record_size,
        })
    }

    /// Read back the viewport sub-group; `None` unless every name resolved.
    pub fn scene3d_viewport_sites(&self) -> Option<Scene3dViewportSites> {
        let a = |n: &str| self.get_address(n);
        let v = |n: &str| self.published_value(n);
        Some(Scene3dViewportSites {
            display: a("scene3d_vp_display")?,
            render2d_list_off: v("scene3d_vp_render2d_list_off")?,
            render3d_list_off: v("scene3d_vp_render3d_list_off")?,
            attach: a("scene3d_vp_attach")?,
            detach: a("scene3d_vp_detach")?,
            list_flags_off: v("scene3d_vp_list_flags_off")?,
            list_target_off: v("scene3d_vp_list_target_off")?,
            target_w_off: v("scene3d_vp_target_w_off")?,
            target_h_off: v("scene3d_vp_target_h_off")?,
            pass_globals: [
                a("scene3d_vp_pass_distant")?,
                a("scene3d_vp_pass_opacity")?,
                a("scene3d_vp_pass_lowprio")?,
                a("scene3d_vp_pass_trans")?,
            ],
            pass_size: v("scene3d_vp_pass_size")?,
            sub_off: v("scene3d_vp_sub_off")?,
            pass_vftable: a("scene3d_vp_pass_vftable")?,
            pass_sort_off: v("scene3d_vp_pass_sort_off")?,
            pass_filter_off: v("scene3d_vp_pass_filter_off")?,
            pass_rect_off: v("scene3d_vp_pass_rect_off")?,
            pass_minz_off: v("scene3d_vp_pass_minz_off")?,
            pass_maxz_off: v("scene3d_vp_pass_maxz_off")?,
            pass_name_off: v("scene3d_vp_pass_name_off")?,
            pass_flags_off: v("scene3d_vp_pass_flags_off")?,
            pass_proj_off: v("scene3d_vp_pass_proj_off")?,
            pass_view_off: v("scene3d_vp_pass_view_off")?,
            pass_self_off: v("scene3d_vp_pass_self_off")?,
            pass_items_off: v("scene3d_vp_pass_items_off")?,
            pass_callbacks_off: v("scene3d_vp_pass_callbacks_off")?,
            vp_rect_off: v("scene3d_vp_vp_rect_off")?,
            vp_flags_off: v("scene3d_vp_vp_flags_off")?,
            gd_write_off: v("scene3d_vp_gd_write_off")?,
            clear_tag: v("scene3d_vp_clear_tag")? as u32,
            clear_record_size: v("scene3d_vp_clear_record_size")?,
        })
    }

    /// The optional shader-lookup pair's names (raw AOB + published).
    const SCENE3D_SHADER_LOOKUP: &'static [&'static str] =
        &["model_shader_select_site", "scene3d_shader_lookup"];

    /// Decode `model_shader_select_site` (RE `docs/background_dancers_research.md`
    /// §4.7): the shared CALL target of the two registry lookups,
    /// identity-gated on the two fallback-name LEAs and the callee prologue.
    fn scene3d_resolve_shader_lookup(&self) -> Result<*const u8, String> {
        let hits = self.get_all_matches("model_shader_select_site");
        let site = match hits.len() {
            1 => hits[0],
            n => return Err(format!("expected exactly 1 match, found {}", n)),
        };
        if !self.scene3d_inside(site, 0x3A) {
            return Err("match at module edge".into());
        }
        // SAFETY: the window was probed inside the module above.
        unsafe {
            // CALL rel32 @+6 (E8 @+6 → rel32 @+7) and @+53 (E8 @+53, after
            // `FF 15 disp32` @+45 and `8B C8` @+51).
            let call_a = decode_call_rel32(site.add(6));
            let call_b = decode_call_rel32(site.add(53));
            if call_a != call_b {
                return Err("the two registry-lookup CALLs disagree".into());
            }
            // LEA RCX,[rip+disp32] @+24 (disp @+27) and @+38 (disp @+41).
            let skinning = decode_rip_relative(site.add(27));
            let plain = decode_rip_relative(site.add(41));
            if !self.scene3d_inside(skinning, 0x1A) || !self.scene3d_inside(plain, 0x11) {
                return Err("fallback-name LEAs outside module".into());
            }
            let s_skin = std::slice::from_raw_parts(skinning, 0x1A);
            let s_plain = std::slice::from_raw_parts(plain, 0x11);
            if s_skin != b"gs_model_skinning_default\0" || s_plain != b"gs_model_default\0" {
                return Err("fallback-name LEAs do not name gs_model_(skinning_)default".into());
            }
            if !self.scene3d_inside(call_a, 0x40) {
                return Err("lookup target outside module".into());
            }
            const LOOKUP_PROLOGUE: &[u8] = &[
                0x40, 0x53, 0x48, 0x83, 0xEC, 0x30, 0x48, 0xC7, 0x44, 0x24, 0x20, 0xFE, 0xFF, 0xFF,
                0xFF, 0x8B, 0xD9,
            ];
            let head = std::slice::from_raw_parts(call_a, LOOKUP_PROLOGUE.len());
            if head != LOOKUP_PROLOGUE {
                return Err("lookup target prologue mismatch".into());
            }
            Ok(call_a)
        }
    }

    /// The optional texture-lookup trio's published names + its raw AOB.
    const SCENE3D_TEXTURE_LOOKUP: &'static [&'static str] = &[
        "texture_lookup_site",
        "scene3d_texture_lookup",
        "scene3d_texture_default",
        "scene3d_texture_spin",
    ];

    /// Decode `texture_lookup_site` (RE `docs/background_dancers_research.md`
    /// §2.5): the CALL target (identity-gated by its prologue), the default
    /// texture global and the spin flag (which must equal both `LOCK XADD`
    /// globals earlier in the loop body).
    fn scene3d_resolve_texture_lookup(&self) -> Result<Scene3dTextureLookup, String> {
        let hits = self.get_all_matches("texture_lookup_site");
        let site = match hits.len() {
            1 => hits[0],
            n => return Err(format!("expected exactly 1 match, found {}", n)),
        };
        // The pattern reads back to match-0x23 and forward to match+0x1A.
        if !self.scene3d_inside(unsafe { site.sub(0x23) }, 0x23 + 0x20) {
            return Err("match at module edge".into());
        }
        // SAFETY: the window was probed inside the module above.
        // (`decode_rip_relative` takes the address of the DISP32 itself:
        // CMOVZ `48 0F 44 05 disp32` @+10 → disp @+14; XCHG `87 0D disp32`
        // @+20 → disp @+22.)
        unsafe {
            let lookup = decode_call_rel32(site.add(2));
            let default_texture = decode_rip_relative(site.add(14));
            let spin = decode_rip_relative(site.add(22));
            // `LOCK XADD dword [rip+disp32], r32` = F0 0F C1 05 disp32 (EAX) /
            // F0 0F C1 15 disp32 (EDX): the RIP target = next instr + disp32.
            let xadd_target = |p: *const u8, modrm: u8| -> Option<*const u8> {
                let head = std::slice::from_raw_parts(p, 4);
                if head != [0xF0, 0x0F, 0xC1, modrm] {
                    return None;
                }
                let disp = (p.add(4) as *const i32).read_unaligned() as isize;
                Some(p.add(8).offset(disp))
            };
            let xadd_a = xadd_target(site.sub(0x23), 0x05)
                .ok_or_else(|| "LOCK XADD [rip],EAX not found at match-0x23".to_string())?;
            let xadd_b = xadd_target(site.sub(0x0C), 0x15)
                .ok_or_else(|| "LOCK XADD [rip],EDX not found at match-0x0C".to_string())?;
            if xadd_a != spin || xadd_b != spin {
                return Err("spin-flag globals disagree (XADD vs XCHG)".into());
            }
            if !self.scene3d_inside(lookup, 0x40)
                || !self.scene3d_inside(default_texture, 8)
                || !self.scene3d_inside(spin, 4)
            {
                return Err("decoded site outside module".into());
            }
            const LOOKUP_PROLOGUE: &[Option<u8>] = &[
                Some(0x40),
                Some(0x53),
                Some(0x48),
                Some(0x83),
                Some(0xEC),
                Some(0x20),
                Some(0x48),
                Some(0x8B),
                Some(0x05),
                None,
                None,
                None,
                None,
                Some(0x8B),
                Some(0xD9),
                Some(0x80),
                Some(0xB8),
                Some(0x80),
                Some(0x00),
                Some(0x00),
                Some(0x00),
                Some(0x00),
                Some(0x75),
                Some(0x2A),
            ];
            let body = std::slice::from_raw_parts(lookup, LOOKUP_PROLOGUE.len());
            let prologue_ok = LOOKUP_PROLOGUE
                .iter()
                .zip(body)
                .all(|(want, got)| want.is_none_or(|w| w == *got));
            if !prologue_ok {
                return Err("CALL target is not the gs texture-registry lookup".into());
            }
            Ok(Scene3dTextureLookup {
                lookup,
                default_texture,
                spin,
            })
        }
    }

    /// Read back the `scene3d` bundle; `None` unless every field resolved.
    pub fn scene3d_sites(&self) -> Option<Scene3dSites> {
        let a = |n: &str| self.get_address(n);
        let v = |n: &str| self.published_value(n);
        Some(Scene3dSites {
            scene_graph_manager: a("scene3d_scene_graph_manager")?,
            mgr_destroy_vec_off: v("scene3d_mgr_destroy_vec_off")?,
            mgr_mutex_off: v("scene3d_mgr_mutex_off")?,
            mgr_depth_off: v("scene3d_mgr_depth_off")?,
            mgr_rate_off: v("scene3d_mgr_rate_off")?,
            mutex_lock_iat: a("scene3d_mutex_lock_iat")?,
            mutex_unlock_iat: a("scene3d_mutex_unlock_iat")?,
            graph_flags_off: v("scene3d_graph_flags_off")?,
            graph_root_child_off: v("scene3d_graph_root_child_off")?,
            graph_sort_flag_off: v("scene3d_graph_sort_flag_off")?,
            graph_camera_vec_off: v("scene3d_graph_camera_vec_off")?,
            node_item_off: v("scene3d_node_item_off")?,
            node_sort_key_off: v("scene3d_node_sort_key_off")?,
            scene_graph_update: a("scene3d_scene_graph_update")?,
            camera_stride: v("scene3d_camera_stride")?,
            camera_active_off: v("scene3d_camera_active_off")?,
            camera_view_off: v("scene3d_camera_view_off")?,
            camera_proj_dirty_off: v("scene3d_camera_proj_dirty_off")?,
            cam_eye_off: v("scene3d_cam_eye_off")?,
            cam_target_off: v("scene3d_cam_target_off")?,
            cam_up_off: v("scene3d_cam_up_off")?,
            cam_w_off: v("scene3d_cam_w_off")?,
            cam_l_off: v("scene3d_cam_l_off")?,
            cam_r_off: v("scene3d_cam_r_off")?,
            cam_b_off: v("scene3d_cam_b_off")?,
            cam_t_off: v("scene3d_cam_t_off")?,
            cam_near_off: v("scene3d_cam_near_off")?,
            cam_far_off: v("scene3d_cam_far_off")?,
            cam_view_dirty_off: v("scene3d_cam_view_dirty_off")?,
            cam_proj_req_off: v("scene3d_cam_proj_req_off")?,
            resource_manager: a("scene3d_resource_manager")?,
            rm_model_mutex_off: v("scene3d_rm_model_mutex_off")?,
            rm_model_map_off: v("scene3d_rm_model_map_off")?,
            rm_node_nil_off: v("scene3d_rm_node_nil_off")?,
            rm_node_key_off: v("scene3d_rm_node_key_off")?,
            rm_node_right_off: v("scene3d_rm_node_right_off")?,
            rm_node_value_off: v("scene3d_rm_node_value_off")?,
            rm_node_refcount_off: v("scene3d_rm_node_refcount_off")?,
            texture_create: a("scene3d_texture_create")?,
            texture_release: a("scene3d_texture_release")?,
            bgmovie_actor: a("scene3d_bgmovie_actor")?,
            bgframe_off: v("scene3d_bgframe_off")?,
            bg_clip_slot_off: v("scene3d_bg_clip_slot_off")?,
            cmovieclip_pool: a("scene3d_cmovieclip_pool")?,
            cmovieclip_pool_stride: v("scene3d_cmovieclip_pool_stride")?,
            cmovieclip_pool_count: v("scene3d_cmovieclip_pool_count")?,
            texture_lookup: match (
                a("scene3d_texture_lookup"),
                a("scene3d_texture_default"),
                a("scene3d_texture_spin"),
            ) {
                (Some(lookup), Some(default_texture), Some(spin)) => Some(Scene3dTextureLookup {
                    lookup,
                    default_texture,
                    spin,
                }),
                _ => None,
            },
            shader_lookup: a("scene3d_shader_lookup"),
            viewport: self.scene3d_viewport_sites(),
        })
    }

    /// The unique match of a `scene3d` AOB, or the failure reason.
    fn scene3d_unique(&self, name: &'static str) -> Result<*const u8, (&'static str, String)> {
        let hits = self.get_all_matches(name);
        match hits.len() {
            1 => Ok(hits[0]),
            n => Err((name, format!("expected exactly 1 match, found {}", n))),
        }
    }

    /// `p..p+len` lies inside the module.
    fn scene3d_inside(&self, p: *const u8, len: usize) -> bool {
        let off = (p as usize).wrapping_sub(self.base as usize);
        off < self.size && off.saturating_add(len) <= self.size
    }

    /// The whole group as one fallible computation (nothing is published
    /// here). Each `Err` names the site and the reason for the boot log.
    fn scene3d_resolve(&self) -> Result<Scene3dSites, (&'static str, String)> {
        type Fail = (&'static str, String);
        let fail = |site: &'static str, why: String| -> Fail { (site, why) };
        let inside = |p: *const u8, len: usize| self.scene3d_inside(p, len);
        let rel = |p: *const u8| (p as usize).wrapping_sub(self.base as usize);
        // Unaligned little-endian readers over module memory.
        let u8_at = |p: *const u8, off: usize| unsafe { *p.add(off) } as usize;
        let u32_at = |p: *const u8, off: usize| unsafe {
            (p.add(off) as *const u32).read_unaligned() as usize
        };
        let rip_at = |p: *const u8, off: usize| unsafe { decode_rip_relative(p.add(off)) };
        let call_at = |p: *const u8, off: usize| unsafe { decode_call_rel32(p.add(off)) };
        let bytes_eq = |p: *const u8, expect: &[u8]| -> bool {
            inside(p, expect.len())
                && unsafe { std::slice::from_raw_parts(p, expect.len()) } == expect
        };

        // ── A. SceneGraphManager global (every enable-bit site must agree) ──
        let enable_sites = self.get_all_matches("sg_enable_bit_site");
        if enable_sites.is_empty() {
            return Err(fail("sg_enable_bit_site", "pattern not found".into()));
        }
        let mut mgr_global: Option<*const u8> = None;
        let mut graph_flags_off: Option<usize> = None;
        for site in &enable_sites {
            if !inside(*site, 16) {
                return Err(fail("sg_enable_bit_site", "match at module edge".into()));
            }
            let g = rip_at(*site, 3);
            let f = u8_at(*site, 12);
            if !inside(g, 8) {
                return Err(fail("sg_enable_bit_site", "global outside module".into()));
            }
            match (mgr_global, graph_flags_off) {
                (None, None) => {
                    mgr_global = Some(g);
                    graph_flags_off = Some(f);
                }
                (Some(pg), Some(pf)) if pg == g && pf == f => {}
                _ => {
                    return Err(fail(
                        "sg_enable_bit_site",
                        format!(
                            "{} sites disagree on the global/flags offset",
                            enable_sites.len()
                        ),
                    ));
                }
            }
        }
        let scene_graph_manager =
            mgr_global.ok_or_else(|| fail("sg_enable_bit_site", "no sites".into()))?;
        let graph_flags_off = graph_flags_off.unwrap_or(0);

        // ── B. Manager tick → flush / active camera / camera rebuilds ──────
        let tick = self.scene3d_unique("sg_manager_tick")?;
        if !inside(tick, 0x40) {
            return Err(fail("sg_manager_tick", "match at module edge".into()));
        }
        let flush = self.scene3d_unique("sg_destroy_flush")?;
        let active_cam = self.scene3d_unique("sg_active_camera")?;
        if call_at(tick, 6) != flush {
            return Err(fail(
                "sg_manager_tick",
                "CALL@+6 is not the destroy-flush match".into(),
            ));
        }
        if call_at(tick, 11) != active_cam {
            return Err(fail(
                "sg_manager_tick",
                "CALL@+11 is not the active-camera match".into(),
            ));
        }
        let view_rebuild = call_at(tick, 41);
        let proj_rebuild = call_at(tick, 70);
        if !inside(view_rebuild, 0x700) || !inside(proj_rebuild, 0x200) {
            return Err(fail(
                "sg_manager_tick",
                "camera rebuild callee outside module".into(),
            ));
        }
        let camera_view_off = u8_at(tick, 52);
        let camera_proj_dirty_off = u32_at(tick, 60);
        if camera_proj_dirty_off >= 0x1000 {
            return Err(fail(
                "sg_manager_tick",
                "implausible proj-dirty offset".into(),
            ));
        }

        // ── C. Active camera: camera vector offset / stride / active byte ──
        if !inside(active_cam, 0x80) || rip_at(active_cam, 3) != scene_graph_manager {
            return Err(fail(
                "sg_active_camera",
                "manager global disagrees with the enable sites".into(),
            ));
        }
        let graph_camera_vec_off = u8_at(active_cam, 30);
        if u8_at(active_cam, 26) != graph_camera_vec_off + 8 {
            return Err(fail(
                "sg_active_camera",
                "camera vector end is not begin+8".into(),
            ));
        }
        let camera_stride = scan_pattern(active_cam, 0x60, "48 69 C0 ?? ?? ?? ??")
            .map(|r| u32_at(r.address, 3))
            .ok_or_else(|| fail("sg_active_camera", "no IMUL stride".into()))?;
        let camera_active_off = scan_pattern(active_cam, 0x60, "80 BC 08 ?? ?? ?? ?? 00")
            .map(|r| u32_at(r.address, 3))
            .ok_or_else(|| fail("sg_active_camera", "no active-byte CMP".into()))?;
        if !(0x200..=0x800).contains(&camera_stride) || camera_active_off + 4 != camera_stride {
            return Err(fail(
                "sg_active_camera",
                format!(
                    "implausible stride 0x{:X} / active 0x{:X}",
                    camera_stride, camera_active_off
                ),
            ));
        }

        // ── D. Destroy flush: mutex / depth / vector / IAT slots ───────────
        if !inside(flush, 0x200) {
            return Err(fail("sg_destroy_flush", "match at module edge".into()));
        }
        let mgr_mutex_off = u8_at(flush, 36);
        let mutex_lock_iat = rip_at(flush, 50);
        let mgr_depth_off = mgr_mutex_off + u8_at(flush, 56);
        let mgr_destroy_vec_off = u8_at(flush, 60);
        if u8_at(flush, 64) != mgr_destroy_vec_off + 8 {
            return Err(fail(
                "sg_destroy_flush",
                "destroy vector end is not begin+8".into(),
            ));
        }
        if !inside(mutex_lock_iat, 8) {
            return Err(fail(
                "sg_destroy_flush",
                "lock IAT slot outside module".into(),
            ));
        }
        let body = unsafe { flush.add(65) };
        if scan_pattern(body, 0x180, "48 8B 01 BA 01 00 00 00 FF 10").is_none() {
            return Err(fail(
                "sg_destroy_flush",
                "no `dtor(node, 1)` vcall in the body".into(),
            ));
        }
        let unlocks = scan_pattern_all(body, 0x180, "FF 15 ?? ?? ?? ??");
        if unlocks.len() != 1 {
            return Err(fail(
                "sg_destroy_flush",
                format!(
                    "expected exactly 1 unlock CALL [rip], found {}",
                    unlocks.len()
                ),
            ));
        }
        let mutex_unlock_iat = rip_at(unlocks[0].address, 2);
        if !inside(mutex_unlock_iat, 8) || mutex_unlock_iat == mutex_lock_iat {
            return Err(fail("sg_destroy_flush", "unlock IAT slot invalid".into()));
        }

        // ── E. Update job → SceneGraph::update layout + sort key ───────────
        let update_job = self.scene3d_unique("sg_update_job_run")?;
        if !inside(update_job, 0x50) || rip_at(update_job, 61) != scene_graph_manager {
            return Err(fail(
                "sg_update_job_run",
                "manager global disagrees with the enable sites".into(),
            ));
        }
        let mgr_rate_off = u8_at(update_job, 72);
        let scene_graph_update = call_at(update_job, 73);
        const UPDATE_PROLOGUE: [u8; 16] = [
            0x40, 0x53, 0x56, 0x57, 0x41, 0x54, 0x41, 0x56, 0x41, 0x57, 0x48, 0x83, 0xEC, 0x48,
            0x48, 0x8B,
        ];
        if !inside(scene_graph_update, 0x400) || !bytes_eq(scene_graph_update, &UPDATE_PROLOGUE) {
            return Err(fail(
                "sg_update_job_run",
                "callee is not SceneGraph::update (prologue)".into(),
            ));
        }
        if u8_at(scene_graph_update, 16) != 0x59 {
            return Err(fail(
                "sg_update_job_run",
                "SceneGraph::update root-child load shape".into(),
            ));
        }
        let graph_root_child_off = u8_at(scene_graph_update, 17);
        let item_loads = scan_pattern_all(scene_graph_update, 0x400, "48 8B 52 ?? E8");
        if item_loads.len() != 1 {
            return Err(fail(
                "sg_update_job_run",
                format!(
                    "expected 1 item-push load in SceneGraph::update, found {}",
                    item_loads.len()
                ),
            ));
        }
        let node_item_off = u8_at(item_loads[0].address, 3);
        let sort_gates = scan_pattern_all(scene_graph_update, 0x400, "41 F6 44 24 ?? 01 74");
        if sort_gates.len() != 1 {
            return Err(fail(
                "sg_update_job_run",
                format!(
                    "expected 1 sort gate in SceneGraph::update, found {}",
                    sort_gates.len()
                ),
            ));
        }
        let graph_sort_flag_off = u8_at(sort_gates[0].address, 4);
        let sort_dispatcher = unsafe { scan_first_call_rel32(sort_gates[0].address.add(7), 0x30) }
            .ok_or_else(|| fail("sg_update_job_run", "no sort CALL after the gate".into()))?;
        let insertion_sort = self.scene3d_unique("sg_insertion_sort")?;
        if !inside(insertion_sort, 0x20) || u32_at(insertion_sort, 12) != u32_at(insertion_sort, 19)
        {
            return Err(fail(
                "sg_insertion_sort",
                "the two key displacements disagree".into(),
            ));
        }
        let node_sort_key_off = u32_at(insertion_sort, 12);
        // The AOB pins the comparator LOOP (entry+0x40 on 20260825), so the
        // dispatcher's CALL lands shortly BEFORE the match, not on it.
        let reached = inside(sort_dispatcher, 0x120)
            && (0..0x120usize - 5).any(|i| {
                let p = unsafe { sort_dispatcher.add(i) };
                if u8_at(p, 0) != 0xE8 {
                    return false;
                }
                let t = call_at(p, 0) as usize;
                let m = insertion_sort as usize;
                t <= m && m - t < 0x100
            });
        if !reached {
            return Err(fail(
                "sg_insertion_sort",
                "not reached from the update's sort dispatcher".into(),
            ));
        }

        // ── F. Camera field block from the two rebuild callees ─────────────
        let (cam_view_dirty_off, cam_eye_off, cam_target_off, cam_up_off) =
            scene3d_camera_view_fields(view_rebuild, 0x700)
                .map_err(|e| fail("sg_manager_tick", e))?;
        let cam_proj_req_off = cam_view_dirty_off + 1;
        let (cam_w_off, cam_near_off, cam_far_off, cam_l_off, cam_r_off, cam_b_off, cam_t_off) =
            scene3d_camera_proj_fields(proj_rebuild, 0x200)
                .map_err(|e| fail("sg_manager_tick", e))?;
        for (label, v) in [
            ("view", camera_view_off),
            ("eye", cam_eye_off),
            ("w", cam_w_off),
            ("far", cam_far_off),
            ("dirty", camera_proj_dirty_off),
        ] {
            if v >= camera_stride {
                return Err(fail(
                    "sg_manager_tick",
                    format!("camera field `{}` past the slot stride", label),
                ));
            }
        }

        // ── G. Model registry: the ONE release whose vtable is GpuResource<ModelData> ──
        let model_vt = self
            .find_vtable_by_rtti(
                ".?AV?$GpuResource@VModelData@gs@@@Resource@agcs@@",
                "scene3d_model_gpu_resource_vtable",
            )
            .ok_or_else(|| {
                fail(
                    "model_registry_release",
                    "GpuResource<ModelData> RTTI vtable not found".into(),
                )
            })?;
        let release_hits = self.get_all_matches("model_registry_release");
        let mut model_release: Option<*const u8> = None;
        for hit in &release_hits {
            if !inside(*hit, 0x100) {
                continue;
            }
            let leas = scan_pattern_all(*hit, 0x100, "48 8D 0D ?? ?? ?? ??");
            if leas.iter().any(|l| rip_at(l.address, 3) == model_vt) {
                if model_release.is_some() {
                    return Err(fail(
                        "model_registry_release",
                        "two releases name the ModelData vtable".into(),
                    ));
                }
                model_release = Some(*hit);
            }
        }
        let model_release = model_release.ok_or_else(|| {
            fail(
                "model_registry_release",
                format!(
                    "none of {} shape hits names the ModelData vtable",
                    release_hits.len()
                ),
            )
        })?;
        if !bytes_eq(unsafe { model_release.add(0x31) }, &[0xFF, 0x15])
            || rip_at(model_release, 0x33) != mutex_lock_iat
        {
            return Err(fail(
                "model_registry_release",
                "lock IAT slot differs from the scene graph's".into(),
            ));
        }
        let resource_manager = rip_at(model_release, 0x1B);
        let rm_model_mutex_off = u32_at(model_release, 0x22);
        let rm_model_map_off = u8_at(model_release, 0x3D);
        let rm_node_nil_off = u8_at(model_release, 0x48);
        if u8_at(model_release, 0x63) != rm_node_nil_off {
            return Err(fail(
                "model_registry_release",
                "the two isnil displacements disagree".into(),
            ));
        }
        let rm_node_key_off = u8_at(model_release, 0x52);
        let rm_node_right_off = u8_at(model_release, 0x58);
        let tail = unsafe { model_release.add(0x60) };
        let rm_node_value_off = scan_pattern(tail, 0x60, "48 8B 77 ??")
            .map(|r| u8_at(r.address, 3))
            .ok_or_else(|| {
                fail(
                    "model_registry_release",
                    "no value load in the release tail".into(),
                )
            })?;
        let rm_node_refcount_off = scan_pattern(tail, 0x60, "FF 4F ??")
            .map(|r| u8_at(r.address, 2))
            .ok_or_else(|| {
                fail(
                    "model_registry_release",
                    "no refcount DEC in the release tail".into(),
                )
            })?;
        if !inside(resource_manager, 8) || rm_model_mutex_off >= 0x1000 {
            return Err(fail(
                "model_registry_release",
                "implausible manager global / mutex offset".into(),
            ));
        }

        // ── H. Texture create / release (same registry spin flag) ──────────
        let create_site = self.scene3d_unique("texture_create_site")?;
        let texture_create = call_at(create_site, 29);
        let texture_release = self.scene3d_unique("texture_release")?;
        if !inside(texture_create, 0x80) || !inside(texture_release, 0x40) {
            return Err(fail(
                "texture_create_site",
                "texture API outside module".into(),
            ));
        }
        let flag_release = rip_at(texture_release, 17);
        let flag_create = scan_pattern(texture_create, 0x80, "F0 0F C1 05 ?? ?? ?? ??")
            .map(|r| rip_at(r.address, 4))
            .ok_or_else(|| {
                fail(
                    "texture_create_site",
                    "no registry spin flag in the create body".into(),
                )
            })?;
        if flag_release != flag_create {
            return Err(fail(
                "texture_release",
                "registry spin flag differs from the create's".into(),
            ));
        }

        // ── I. Background objects ──────────────────────────────────────────
        let readiness = self.scene3d_unique("bgmovie_readiness")?;
        if !inside(readiness, 0x30) {
            return Err(fail("bgmovie_readiness", "match at module edge".into()));
        }
        let bgmovie_actor = rip_at(readiness, 9);
        let bgframe_off = u8_at(readiness, 16);
        if !inside(bgmovie_actor, 8) {
            return Err(fail("bgmovie_readiness", "global outside module".into()));
        }
        // `48 83 3D disp32 imm8` is an 8-byte instruction: the RIP base is one
        // past the displacement's end.
        let poll_ok = self
            .get_all_matches("bgmovie_ready_call_site")
            .iter()
            .filter(|s| inside(**s, 0x20))
            .any(|s| unsafe { rip_at(*s, 3).add(1) } == bgmovie_actor && call_at(*s, 10) == readiness);
        if !poll_ok {
            return Err(fail(
                "bgmovie_ready_call_site",
                "no DPS poll names both the global and the readiness fn".into(),
            ));
        }
        let cmovieclip_create = self
            .get_address("cmovieclip_create")
            .ok_or_else(|| fail("bg_root_create_site", "cmovieclip_create unresolved".into()))?;
        let bg_site = self.scene3d_unique("bg_root_create_site")?;
        if !inside(bg_site, 0x100) {
            return Err(fail("bg_root_create_site", "match at module edge".into()));
        }
        let cmovieclip_pool = rip_at(bg_site, 3);
        let cmovieclip_pool_stride = scan_pattern(bg_site, 0x60, "48 81 C7 ?? ?? ?? ??")
            .map(|r| u32_at(r.address, 3))
            .ok_or_else(|| fail("bg_root_create_site", "no pool stride ADD".into()))?;
        let cmovieclip_pool_count = scan_pattern(bg_site, 0x60, "81 FB ?? ?? ?? ??")
            .map(|r| u32_at(r.address, 2))
            .ok_or_else(|| fail("bg_root_create_site", "no pool count CMP".into()))?;
        // The create CALL: the first `E8` after a `LEA R8,[rip+"bg_root"]`.
        let mut create_call: Option<*const u8> = None;
        for lea in scan_pattern_all(bg_site, 0x100, "4C 8D 05 ?? ?? ?? ??") {
            let s = rip_at(lea.address, 3);
            if bytes_eq(s, b"bg_root\0") {
                if let Some(off) = (7..0x18).find(|o| u8_at(lea.address, *o) == 0xE8) {
                    create_call = Some(unsafe { lea.address.add(off) });
                    break;
                }
            }
        }
        let create_call = create_call.ok_or_else(|| {
            fail(
                "bg_root_create_site",
                "no CALL after the \"bg_root\" LEA".into(),
            )
        })?;
        if call_at(create_call, 0) != cmovieclip_create {
            return Err(fail(
                "bg_root_create_site",
                "create CALL is not cmovieclip_create (identity)".into(),
            ));
        }
        let bg_clip_slot_off =
            scan_pattern(unsafe { create_call.add(5) }, 0x100, "48 81 C1 ?? ?? ?? ??")
                .map(|r| u32_at(r.address, 3))
                .ok_or_else(|| {
                    fail(
                        "bg_root_create_site",
                        "no clip-slot ADD after the create".into(),
                    )
                })?;
        if !inside(cmovieclip_pool, cmovieclip_pool_stride)
            || !(0x100..=0x400).contains(&cmovieclip_pool_stride)
            || !(0x100..=0x1000).contains(&cmovieclip_pool_count)
            || !(0x40..=0x400).contains(&bg_clip_slot_off)
        {
            return Err(fail(
                "bg_root_create_site",
                format!(
                    "implausible pool @+0x{:X} stride 0x{:X} count 0x{:X} slot 0x{:X}",
                    rel(cmovieclip_pool),
                    cmovieclip_pool_stride,
                    cmovieclip_pool_count,
                    bg_clip_slot_off
                ),
            ));
        }

        // ── Plausibility of every small offset ─────────────────────────────
        for (label, v) in [
            ("graph_flags", graph_flags_off),
            ("root_child", graph_root_child_off),
            ("sort_flag", graph_sort_flag_off),
            ("camera_vec", graph_camera_vec_off),
            ("node_item", node_item_off),
            ("node_sort_key", node_sort_key_off),
            ("mgr_destroy_vec", mgr_destroy_vec_off),
            ("mgr_mutex", mgr_mutex_off),
            ("mgr_depth", mgr_depth_off),
            ("mgr_rate", mgr_rate_off),
            ("rm_map", rm_model_map_off),
            ("rm_nil", rm_node_nil_off),
            ("rm_key", rm_node_key_off),
            ("rm_right", rm_node_right_off),
            ("rm_value", rm_node_value_off),
            ("rm_refcount", rm_node_refcount_off),
            ("bgframe", bgframe_off),
        ] {
            if v >= 0x1000 {
                return Err(fail(
                    "scene3d",
                    format!("implausible offset `{}` = 0x{:X}", label, v),
                ));
            }
        }

        Ok(Scene3dSites {
            scene_graph_manager,
            mgr_destroy_vec_off,
            mgr_mutex_off,
            mgr_depth_off,
            mgr_rate_off,
            mutex_lock_iat,
            mutex_unlock_iat,
            graph_flags_off,
            graph_root_child_off,
            graph_sort_flag_off,
            graph_camera_vec_off,
            node_item_off,
            node_sort_key_off,
            scene_graph_update,
            camera_stride,
            camera_active_off,
            camera_view_off,
            camera_proj_dirty_off,
            cam_eye_off,
            cam_target_off,
            cam_up_off,
            cam_w_off,
            cam_l_off,
            cam_r_off,
            cam_b_off,
            cam_t_off,
            cam_near_off,
            cam_far_off,
            cam_view_dirty_off,
            cam_proj_req_off,
            resource_manager,
            rm_model_mutex_off,
            rm_model_map_off,
            rm_node_nil_off,
            rm_node_key_off,
            rm_node_right_off,
            rm_node_value_off,
            rm_node_refcount_off,
            texture_create,
            texture_release,
            bgmovie_actor,
            bgframe_off,
            bg_clip_slot_off,
            cmovieclip_pool,
            cmovieclip_pool_stride,
            cmovieclip_pool_count,
            // Filled in by `derive_scene3d` after the optional sub-derivation;
            // `scene3d_sites()` reads it back from the published names.
            texture_lookup: None,
            shader_lookup: None,
            viewport: None,
        })
    }

    /// Derive `results_course_gate_global` — the global the PlaydataTab
    /// populate consults to pick the record it displays (`DAT_1806F14F8`
    /// on 20260721): `**global + 0x70 != 0` ⇒ the tab reads the COURSE
    /// record (`PlayerWork+0x2D8`), else the per-stage array
    /// (`PlayerWork+0x590 + stage*0x2B8`). The s_marvelous results row
    /// replicates the exact branch so its recompute always reads the SAME
    /// record the tab renders (S-Marvelous is mode-agnostic — normal,
    /// course/Dan, training all show it).
    ///
    /// Derivation: inside the first 0x100 bytes of `playdata_tab_update`,
    /// the (single) sequence
    ///   MOV RAX,[rip+disp32]  ; 48 8B 05 ..    the gate global
    ///   MOV RCX,[RAX]         ; 48 8B 08
    ///   CMP [RCX+0x70],RDI    ; 48 39 79 70
    /// — verified byte-identical shape on 20260616/20260721. Fails closed.
    fn derive_smarv_results_course_gate(&mut self) {
        let populate = match self.get_address("playdata_tab_update") {
            Some(a) => a,
            None => {
                log_warn!("  [-] results_course_gate_global -- playdata_tab_update unresolved");
                return;
            }
        };
        const PATTERN: &str = "48 8B 05 ? ? ? ? 48 8B 08 48 39 79 70";
        let hits = scan_pattern_all(populate, 0x100, PATTERN);
        if hits.len() != 1 {
            log_warn!(
                "  [-] results_course_gate_global -- expected 1 gate match, found {}",
                hits.len()
            );
            return;
        }
        unsafe {
            let global = decode_rip_relative(hits[0].address.add(3));
            let off = global.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!("  [-] results_course_gate_global -- derived address outside module");
                return;
            }
            self.resolved
                .insert("results_course_gate_global".into(), global);
            log_info!("  [+] results_course_gate_global (derived) @ +0x{:X}", off);
        }
    }

    /// `ghost_vec_copy` — the game's `std::vector<u8>` copy-assign, decoded
    /// from the CALL rel32 at `ghost_local_slot_copy_site + 25`. Cross-checked
    /// to sit inside `ghost_actor_init`'s body (the site is that function's
    /// local-slot branch), and the target must lie inside the module. Any miss
    /// leaves the ghost cache fail-open (no injection).
    fn derive_ghost_vec_copy(&mut self) {
        let (Some(site), Some(init)) = (
            self.get_address("ghost_local_slot_copy_site"),
            self.get_address("ghost_actor_init"),
        ) else {
            log_warn!("  [-] ghost_vec_copy -- copy site / GhostActor init unresolved");
            return;
        };
        // The copy site must be a forward reference inside the init function
        // (its body is ~0x2B7 bytes on 20260721; allow generous slack).
        let rel = (site as usize).wrapping_sub(init as usize);
        if rel == 0 || rel > 0x800 {
            log_warn!(
                "  [-] ghost_vec_copy -- copy site {:p} not inside GhostActor init {:p}",
                site,
                init
            );
            return;
        }
        unsafe {
            if *site.add(25) != 0xE8 {
                log_warn!("  [-] ghost_vec_copy -- expected CALL rel32 at site+25");
                return;
            }
            let target = decode_call_rel32(site.add(25));
            let off = (target as usize).wrapping_sub(self.base as usize);
            if off >= self.size {
                log_warn!("  [-] ghost_vec_copy -- derived target outside module");
                return;
            }
            self.resolved.insert("ghost_vec_copy".into(), target);
            log_info!("  [+] ghost_vec_copy (derived) @ +0x{:X}", off);
        }
    }

    /// Derive everything the 2-Player BPL Mode re-host needs beyond its
    /// four AOBs (`battle_frame_ctor`, `actor_add_child`,
    /// `dance_matching_slot_probe`, `gpa_score_select`):
    ///
    /// * `battle_frame_actor_vtable` / `layout_actor_vtable` — RTTI walks
    ///   (`.?AVMatchingBattleFrameActor@dance@sequence@@`,
    ///   `.?AVLayoutActor@dance@sequence@@`). The frame vtable has 9 slots;
    ///   slot 4 = onInitialize, 6 = onUpdate (the two the mod clones over).
    /// * ctor identity cross-check — the ctor's SECOND `LEA RAX,[rip+disp32]`
    ///   (`48 8D 05`) within its first 0x100 bytes must decode to the RTTI
    ///   vtable (the first is the agcs::Actor base vftable). Refuses the whole
    ///   set otherwise: a ctor that constructs a different class would be
    ///   handed a mismatched vtable clone.
    /// * `battle_frame_rank_fn` — stock onUpdate (vtable[6]) tail-jumps to the
    ///   rank/diff function: the FIRST `5E E9 rel32` (`POP RSI; JMP rel32`)
    ///   inside `[vtable[6], +0x200)`, target inside the module.
    /// * `matching_local_cabinet_idx` — the first `48 63 05 disp32`
    ///   (`MOVSXD RAX,[rip]`) inside `[ctor, +0x300)`: CNetworkManager's
    ///   local cabinet index (`DAT_1806f391c` on 20260825), −1 while no
    ///   matching session exists. The stock ctor indexes the cabinet-block
    ///   array with it WITHOUT a null check, so the mod gates on −1.
    /// * `scene_resource_manager` (RIP at probe+3) and the published
    ///   `dance_matching_slot_off` (imm32 at probe+13) — the probe must sit
    ///   inside `[vtable[4], +0x200)`.
    /// * published `gpa_is_ex_off` / `gpa_ex_score_off` / `gpa_money_score_off`
    ///   from `gpa_score_select` (+2 / +11 / +19), each required < 0x400.
    ///
    /// All-or-nothing: any check failing leaves EVERY derived key of this
    /// group unresolved (the mod's `required_signatures` then skips it).
    fn derive_two_player_bpl(&mut self) {
        const TAG: &str = "two_player_bpl";
        let (Some(ctor), Some(probe), Some(select)) = (
            self.get_address("battle_frame_ctor"),
            self.get_address("dance_matching_slot_probe"),
            self.get_address("gpa_score_select"),
        ) else {
            log_warn!(
                "  [-] {} -- ctor / slot probe / score select unresolved",
                TAG
            );
            return;
        };
        let Some(frame_vt) = self.find_vtable_by_rtti(
            ".?AVMatchingBattleFrameActor@dance@sequence@@",
            "battle_frame_actor_vtable",
        ) else {
            return;
        };
        let Some(layout_vt) =
            self.find_vtable_by_rtti(".?AVLayoutActor@dance@sequence@@", "layout_actor_vtable")
        else {
            return;
        };
        let base = self.base as usize;
        let inside = |p: *const u8| (p as usize).wrapping_sub(base) < self.size;

        unsafe {
            // Ctor identity: second `48 8D 05 disp32` decodes to the frame vtable.
            let mut leas = Vec::new();
            let mut i = 0usize;
            while i + 7 <= 0x100 && leas.len() < 2 {
                let p = ctor.add(i);
                if *p == 0x48 && *p.add(1) == 0x8D && *p.add(2) == 0x05 {
                    leas.push(decode_rip_relative(p.add(3)));
                    i += 7;
                } else {
                    i += 1;
                }
            }
            if leas.len() != 2 || leas[1] != frame_vt {
                log_warn!(
                    "  [-] {} -- ctor's class vftable LEA does not match RTTI vtable ({} LEAs)",
                    TAG,
                    leas.len()
                );
                return;
            }

            // vtable slots must be readable, inside the module.
            let slot = |n: usize| *(frame_vt as *const *const u8).add(n);
            let on_init = slot(4);
            let on_update = slot(6);
            if !inside(on_init) || !inside(on_update) {
                log_warn!("  [-] {} -- frame vtable slots 4/6 outside module", TAG);
                return;
            }

            // Rank fn: first `5E E9 rel32` in onUpdate's window.
            let mut rank_fn: Option<*const u8> = None;
            for off in 0..0x200usize {
                let p = on_update.add(off);
                if *p == 0x5E && *p.add(1) == 0xE9 {
                    let t = decode_call_rel32(p.add(1));
                    if inside(t) {
                        rank_fn = Some(t);
                    }
                    break;
                }
            }
            let Some(rank_fn) = rank_fn else {
                log_warn!(
                    "  [-] {} -- no `POP RSI; JMP rel32` tail in stock onUpdate",
                    TAG
                );
                return;
            };

            // Slot probe must lie inside onInitialize.
            let probe_rel = (probe as usize).wrapping_sub(on_init as usize);
            if probe_rel >= 0x200 {
                log_warn!("  [-] {} -- slot probe not inside stock onInitialize", TAG);
                return;
            }
            let scene_res_mgr = decode_rip_relative(probe.add(3));
            if !inside(scene_res_mgr) {
                log_warn!(
                    "  [-] {} -- scene resource manager global outside module",
                    TAG
                );
                return;
            }
            let slot_off = std::ptr::read_unaligned(probe.add(13) as *const u32) as usize;
            if slot_off == 0 || slot_off > 0x2000 || slot_off % 8 != 0 {
                log_warn!(
                    "  [-] {} -- implausible dance_matching slot offset 0x{:X}",
                    TAG,
                    slot_off
                );
                return;
            }

            // Local cabinet index: first `48 63 05 disp32` in the ctor body.
            let mut cab_idx: Option<*const u8> = None;
            for off in 0..0x300usize {
                let p = ctor.add(off);
                if *p == 0x48 && *p.add(1) == 0x63 && *p.add(2) == 0x05 {
                    let g = decode_rip_relative(p.add(3));
                    if inside(g) {
                        cab_idx = Some(g);
                    }
                    break;
                }
            }
            let Some(cab_idx) = cab_idx else {
                log_warn!(
                    "  [-] {} -- no `MOVSXD RAX,[rip]` (local cabinet idx) in ctor",
                    TAG
                );
                return;
            };

            // GamePlayActor score-select offsets.
            let rd = |o: usize| std::ptr::read_unaligned(select.add(o) as *const u32) as usize;
            let (is_ex, ex_score, money) = (rd(2), rd(11), rd(19));
            if [is_ex, ex_score, money]
                .iter()
                .any(|&v| v == 0 || v >= 0x400)
                || ex_score == money
            {
                log_warn!(
                    "  [-] {} -- implausible GamePlayActor score offsets (0x{:X}/0x{:X}/0x{:X})",
                    TAG,
                    is_ex,
                    ex_score,
                    money
                );
                return;
            }

            // Everything validated — publish the group.
            let rel = |p: *const u8| (p as usize).wrapping_sub(base);
            self.resolved
                .insert("battle_frame_actor_vtable".into(), frame_vt);
            log_info!(
                "  [+] battle_frame_actor_vtable (RTTI) @ +0x{:X}",
                rel(frame_vt)
            );
            self.resolved
                .insert("layout_actor_vtable".into(), layout_vt);
            log_info!("  [+] layout_actor_vtable (RTTI) @ +0x{:X}", rel(layout_vt));
            self.resolved.insert("battle_frame_rank_fn".into(), rank_fn);
            log_info!(
                "  [+] battle_frame_rank_fn (derived) @ +0x{:X}",
                rel(rank_fn)
            );
            self.resolved
                .insert("matching_local_cabinet_idx".into(), cab_idx);
            log_info!(
                "  [+] matching_local_cabinet_idx (derived) @ +0x{:X}",
                rel(cab_idx)
            );
            self.resolved
                .insert("scene_resource_manager".into(), scene_res_mgr);
            log_info!(
                "  [+] scene_resource_manager (derived) @ +0x{:X}",
                rel(scene_res_mgr)
            );
            self.publish_value("dance_matching_slot_off", slot_off);
            self.publish_value("gpa_is_ex_off", is_ex);
            self.publish_value("gpa_ex_score_off", ex_score);
            self.publish_value("gpa_money_score_off", money);
        }
    }

    /// Published `dance_matching` scene-resource slot offset (see
    /// `derive_two_player_bpl`), or `None`.
    pub fn dance_matching_slot_off(&self) -> Option<usize> {
        self.published_value("dance_matching_slot_off")
    }

    /// Published GamePlayActor score-select offsets `(is_ex, ex_score,
    /// money_score)` (see `derive_two_player_bpl`), or `None`.
    pub fn gpa_score_offsets(&self) -> Option<(usize, usize, usize)> {
        Some((
            self.published_value("gpa_is_ex_off")?,
            self.published_value("gpa_ex_score_off")?,
            self.published_value("gpa_money_score_off")?,
        ))
    }

    /// Derive everything DDR SELECTION's package-helper replacement needs
    /// beyond its two AOBs (`layout_package_helper`, `dps_skin_table_read`).
    /// All values are read from the helper's OWN body, whose shape is
    /// identical on every supported build (RE: the 2026-09-22 DDR SELECTION
    /// planning notes; `docs/ddr_selection_research.md` §3.1):
    ///
    /// * identity gates — the LEA at match+106 decodes to `"dance_message"`,
    ///   the `45 8B CE 4C 8D 05` LEA to `"%04d"` (the dead skin format), the
    ///   probe's `48 8D 0D` LEA to `"bm2d"`;
    /// * `ddr_sel_pkg_probe` — `FUN_1801ac3f0(dir, name) -> bool`, the arc
    ///   version probe (`_v3`, `_v0`, `_lite`, bare);
    /// * from the insert/push tail
    ///   `LEA R8,[RBP-0x48]; MOV RDX,RBX; CMP R13D,2; JNZ; LEA RCX,[R12+shared];
    ///   JMP; LEA RCX,[R13+R13*8]; LEA RCX,[R12+RCX*8+side]; CALL insert;
    ///   LEA RCX,[R12+list]; LEA RDX,[RBP-0x48]; CALL push` —
    ///   `ddr_sel_record_insert` (`void(map*, const char* key, value*)`, copies
    ///   the string and value+0x28), `ddr_sel_load_list_push`
    ///   (`void(vector<string>*, const string*)`, copies) and the published
    ///   offsets `ddr_sel_records_shared_off` / `ddr_sel_records_side_off` /
    ///   `ddr_sel_load_list_off` (per-side stride 0x48 is pinned by the
    ///   pattern's literal `R13+R13*8` / `*8` encoding);
    /// * from `dps_skin_table_read`: `ddr_sel_game_work_global` (RIP at
    ///   match+39, must equal stage_record_accessor's GameWork global when
    ///   that resolved) and the published `gamework_skin_off` (match+49).
    ///
    /// All-or-nothing: any failure publishes nothing (the mod lists these
    /// names in `required_signatures` and is skipped cleanly).
    fn derive_ddr_selection(&mut self) {
        const TAG: &str = "ddr_selection";
        let (Some(helper), Some(skin_site)) = (
            self.get_address("layout_package_helper"),
            self.get_address("dps_skin_table_read"),
        ) else {
            log_warn!(
                "  [-] {} -- package helper / skin table read unresolved",
                TAG
            );
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let inside = |p: *const u8| (p as usize).wrapping_sub(base) < size;
        // NUL-terminated string at `p` equals `want`?
        let cstr_is = |p: *const u8, want: &[u8]| -> bool {
            if !inside(p) || (p as usize - base) + want.len() + 1 > size {
                return false;
            }
            unsafe { std::slice::from_raw_parts(p, want.len()) == want && *p.add(want.len()) == 0 }
        };
        const WINDOW: usize = 0x200;
        let find = |pattern: &str| -> Option<*const u8> {
            let hits = scan_pattern_all(helper, WINDOW, pattern);
            if hits.len() == 1 {
                Some(hits[0].address as *const u8)
            } else {
                None
            }
        };

        unsafe {
            if !cstr_is(decode_rip_relative(helper.add(109)), b"dance_message") {
                log_warn!(
                    "  [-] {} -- helper LEA at +106 is not \"dance_message\"",
                    TAG
                );
                return;
            }
            let Some(fmt) = find("45 8B CE 4C 8D 05 ?? ?? ?? ?? 8D 50 08 48 8D 4D E8 E8") else {
                log_warn!("  [-] {} -- helper skin-format site not unique", TAG);
                return;
            };
            if !cstr_is(decode_rip_relative(fmt.add(6)), b"%04d") {
                log_warn!("  [-] {} -- helper format string is not \"%04d\"", TAG);
                return;
            }
            let Some(probe_site) = find("48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 84 C0 75") else {
                log_warn!("  [-] {} -- helper probe site not unique", TAG);
                return;
            };
            let dir = decode_rip_relative(probe_site.add(3));
            if !cstr_is(dir, b"bm2d") {
                log_warn!("  [-] {} -- helper probe dir is not \"bm2d\"", TAG);
                return;
            }
            let probe = decode_call_rel32(probe_site.add(7));
            let Some(tail) = find(
                "4C 8D 45 B8 48 8B D3 41 83 FD 02 75 0A 49 8D 8C 24 ?? ?? ?? ?? EB 0D 4B 8D 4C ED 00 49 8D 8C CC ?? ?? ?? ?? E8 ?? ?? ?? ?? 49 8D 8C 24 ?? ?? ?? ?? 48 8D 55 B8 E8 ?? ?? ?? ??",
            ) else {
                log_warn!("  [-] {} -- helper insert/push tail not unique", TAG);
                return;
            };
            let rd = |p: *const u8| std::ptr::read_unaligned(p as *const u32) as usize;
            let shared_off = rd(tail.add(17));
            let side_off = rd(tail.add(32));
            let list_off = rd(tail.add(45));
            let insert = decode_call_rel32(tail.add(36));
            let push = decode_call_rel32(tail.add(53));
            if !inside(probe) || !inside(insert) || !inside(push) {
                log_warn!("  [-] {} -- helper callees outside module", TAG);
                return;
            }
            // Record maps come first (shared, then 2 × 0x48 per side), the
            // load list after them — the ctor's field order.
            if !(shared_off < side_off && side_off + 2 * 0x48 <= list_off && list_off < 0x400) {
                log_warn!(
                    "  [-] {} -- implausible LayoutActor offsets (shared 0x{:X}, side 0x{:X}, list 0x{:X})",
                    TAG,
                    shared_off,
                    side_off,
                    list_off
                );
                return;
            }

            let game_work_global = decode_rip_relative(skin_site.add(39));
            let skin_off = rd(skin_site.add(49));
            if !inside(game_work_global) || !(0x40..0x400).contains(&skin_off) {
                log_warn!(
                    "  [-] {} -- implausible GameWork global / skin offset 0x{:X}",
                    TAG,
                    skin_off
                );
                return;
            }
            let accessor = self
                .get_address("stage_record_accessor")
                .or_else(|| self.get_address("stage_record_accessor_v1"));
            if let Some(acc) = accessor {
                if decode_rip_relative(acc.add(3)) != game_work_global {
                    log_warn!(
                        "  [-] {} -- skin-table GameWork global disagrees with stage_record_accessor",
                        TAG
                    );
                    return;
                }
            }

            let rel = |p: *const u8| (p as usize).wrapping_sub(base);
            for (name, p) in [
                ("ddr_sel_pkg_probe", probe),
                ("ddr_sel_record_insert", insert),
                ("ddr_sel_load_list_push", push),
                ("ddr_sel_bm2d_dir", dir),
                ("ddr_sel_game_work_global", game_work_global),
            ] {
                self.resolved.insert(name.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", name, rel(p));
            }
            self.publish_value("ddr_sel_records_shared_off", shared_off);
            self.publish_value("ddr_sel_records_side_off", side_off);
            self.publish_value("ddr_sel_load_list_off", list_off);
            self.publish_value("gamework_skin_off", skin_off);
        }
    }

    /// Publish `music_series_vslot` — the music-DB entry's "raw musicdb
    /// `<series>`" virtual slot, read from `flare_skill_classifier`'s own
    /// `CALL qword [RDX+disp32]` (match+2): 0xA0 on 20260324+, 0x88 on
    /// 20250805 / 20260224. The object is the same entry `find_music_by_mcode`
    /// returns (both walk the `music_db_global` vector). Consumer:
    /// ddr_selection's AUTO trigger. Soft: a miss publishes nothing.
    fn derive_music_series_vslot(&mut self) {
        let Some(site) = self.get_address("flare_skill_classifier") else {
            log_warn!("  [-] music_series_vslot -- flare_skill_classifier unresolved");
            return;
        };
        let slot = unsafe { std::ptr::read_unaligned(site.add(2) as *const u32) } as usize;
        if slot % 8 != 0 || !(0x40..0x200).contains(&slot) {
            log_warn!("  [-] music_series_vslot -- implausible slot 0x{:X}", slot);
            return;
        }
        self.publish_value("music_series_vslot", slot);
    }

    /// Validate `afp_sound_callback_play`: the RTTI `bm2d::SoundCallback`
    /// vtable's slot 1 must be the match, and the tail `JMP rel32` (opcode at
    /// match+0x54, disp at +0x55) must land on the `se_play` match. On any
    /// failure the AOB is un-resolved so no consumer can detour a function of
    /// the wrong shape.
    fn derive_afp_sound_callback(&mut self) {
        const NAME: &str = "afp_sound_callback_play";
        let Some(play) = self.get_address(NAME) else {
            return;
        };
        let fail = |this: &mut Self, why: &str| {
            this.resolved.remove(NAME);
            log_warn!("  [-] {} -- {}; un-resolved", NAME, why);
        };
        let Some(se_play) = self.get_address("se_play") else {
            return fail(self, "se_play unresolved");
        };
        let Some(vt) =
            self.find_vtable_by_rtti(".?AVSoundCallback@bm2d@@", "sound_callback_vtable")
        else {
            return fail(self, "RTTI vtable not found");
        };
        unsafe {
            let slot1 = *(vt as *const *const u8).add(1);
            if slot1 != play {
                return fail(self, "RTTI vtable slot 1 is not the match");
            }
            if *play.add(0x54) != 0xE9 || decode_rip_relative(play.add(0x55)) != se_play {
                return fail(self, "tail JMP does not reach se_play");
            }
        }
        log_info!("  [+] {} verified (RTTI slot 1, tail -> se_play)", NAME);
    }

    /// Validate `ddr_sel_fullcombo_se_site` and publish the null-bank JZ of
    /// FullcomboActor::onMessage's inlined `se_game_fullcombo` play as
    /// `ddr_sel_fullcombo_se_jz` (the byte ddr_selection flips `74` → `EB`).
    ///
    /// Gates: the `LEA RDX,[rip]` at match+0x13 names `"se_game_fullcombo"`;
    /// match+0x0E is `JZ rel8` and lands on `TEST ECX,ECX; JLE` (the AVS lock
    /// release — the path World takes when the slot-2 bank is absent, so the
    /// JMP keeps the lock balanced); when `fullcombo_actor_on_message`
    /// resolved, the site lies within its first 0x200 bytes. Any failure
    /// un-resolves the AOB (the full-combo SE then doubles on legacy songs —
    /// cosmetic, one WARN at the consumer).
    fn derive_ddr_sel_code_se(&mut self) {
        const NAME: &str = "ddr_sel_fullcombo_se_site";
        const JZ_OFF: usize = 0x0E;
        const LEA_DISP_OFF: usize = 0x16;
        let Some(site) = self.get_address(NAME) else {
            return;
        };
        let fail = |this: &mut Self, why: &str| {
            this.resolved.remove(NAME);
            log_warn!("  [-] {} -- {}; un-resolved", NAME, why);
        };
        let base = self.base as usize;
        let size = self.size;
        unsafe {
            let label = decode_rip_relative(site.add(LEA_DISP_OFF));
            let want = b"se_game_fullcombo\0";
            let off = (label as usize).wrapping_sub(base);
            if off + want.len() > size || std::slice::from_raw_parts(label, want.len()) != want {
                return fail(self, "LEA does not name \"se_game_fullcombo\"");
            }
            let jz = site.add(JZ_OFF);
            if *jz != 0x74 {
                return fail(self, "null-bank branch is not JZ rel8");
            }
            let target = jz.add(2).offset(*jz.add(1) as i8 as isize);
            if std::slice::from_raw_parts(target, 3) != [0x85, 0xC9, 0x7E] {
                return fail(self, "null-bank JZ does not reach the lock release");
            }
            if let Some(handler) = self.get_address("fullcombo_actor_on_message") {
                if (site as usize).wrapping_sub(handler as usize) >= 0x200 {
                    return fail(self, "site is outside fullcombo_actor_on_message");
                }
            }
            self.resolved.insert("ddr_sel_fullcombo_se_jz".into(), jz);
            log_info!(
                "  [+] ddr_sel_fullcombo_se_jz (derived) @ +0x{:X}",
                (jz as usize).wrapping_sub(base)
            );
        }
    }

    /// The ShutterActor update's two inlined `vo_ingame_ready` plays (state 5
    /// = the `stage_out` reveal, state 7 = the `0x100c` drain when the reveal
    /// never ran; FUN_180033f60+0x78C / +0x94E on 20260825), each
    /// `MOV Rb,[rip+audio_mgr]; MOV RDI,[Rb+0x40] (slot-3 voice bank); TEST
    /// RDI,RDI; JZ release; MOV RAX,[RDI]; LEA RDX,["vo_ingame_ready"]; …
    /// GetCueIndex; … Play; … MOV R8D,3; CALL register_handle`. A legacy
    /// `dance_message000N` READY clip plays its own era voice, so
    /// ddr_selection::sound::code_se flips both null-bank `JZ`s to `JMP`
    /// while the legacy intro is armed. Pattern starts at the `MOV RDI` (the
    /// preceding manager load and the bank register differ between 20250805 /
    /// 20260224 and 20260721+); exactly two hits on every build, both must
    /// name the string, both `JZ rel8` (match+7) must land on the AVS lock
    /// release `TEST ECX,ECX; JLE`, and they must sit within 0x400 bytes of
    /// each other. Publishes `ddr_sel_vo_ready_jz_0` / `_1`. Soft: any
    /// failure publishes nothing (World's voice then doubles the legacy one).
    const VO_READY_SITE: &'static str = "?? 8B ?? 40 48 85 FF 74 ?? 48 8B 07 48 8D 15 ?? ?? ?? ?? 48 8B CF FF 10 0F B7 D0 B9 FF FF 00 00 66 3B C1 74 ?? ?? 89 ?? ?? 48 8B 07 48 8D 4D ?? 48 89 4C 24 20 45 33 C9 45 33 C0 48 8B CF FF 50 20 85 C0 78 ?? 0F 57 DB 41 B8 03 00 00 00";

    fn derive_ddr_sel_vo_ready(&mut self) {
        const LABEL: &str = "ddr_sel_vo_ready_jz";
        const JZ_OFF: usize = 7;
        const LEA_DISP_OFF: usize = 15;
        let base = self.base as usize;
        let size = self.size;
        let hits = scan_pattern_all(self.base, self.size, Self::VO_READY_SITE);
        if hits.len() != 2 {
            log_warn!(
                "  [-] {} -- expected 2 inlined vo_ingame_ready plays, found {}",
                LABEL,
                hits.len()
            );
            return;
        }
        let a = hits[0].address as usize;
        let b = hits[1].address as usize;
        if a.abs_diff(b) > 0x400 {
            log_warn!(
                "  [-] {} -- the two sites are 0x{:X} apart",
                LABEL,
                a.abs_diff(b)
            );
            return;
        }
        let mut jzs = Vec::with_capacity(2);
        unsafe {
            for h in &hits {
                let site = h.address;
                let label = decode_rip_relative(site.add(LEA_DISP_OFF));
                let want = b"vo_ingame_ready\0";
                let off = (label as usize).wrapping_sub(base);
                if off + want.len() > size || std::slice::from_raw_parts(label, want.len()) != want
                {
                    log_warn!(
                        "  [-] {} -- a site's LEA does not name \"vo_ingame_ready\"",
                        LABEL
                    );
                    return;
                }
                let jz = site.add(JZ_OFF);
                if *jz != 0x74 {
                    log_warn!("  [-] {} -- null-bank branch is not JZ rel8", LABEL);
                    return;
                }
                let target = jz.add(2).offset(*jz.add(1) as i8 as isize);
                if std::slice::from_raw_parts(target, 3) != [0x85, 0xC9, 0x7E] {
                    log_warn!(
                        "  [-] {} -- null-bank JZ does not reach the lock release",
                        LABEL
                    );
                    return;
                }
                jzs.push(jz);
            }
        }
        for (i, jz) in jzs.into_iter().enumerate() {
            let name = format!("ddr_sel_vo_ready_jz_{i}");
            log_info!(
                "  [+] {} (derived) @ +0x{:X}",
                name,
                (jz as usize).wrapping_sub(base)
            );
            self.resolved.insert(name, jz);
        }
    }

    /// Validate `ddr_sel_dps_ready_dwell` and publish the DancePlaySequence
    /// READY?-dwell timer offset as `ddr_sel_dps_ready_timer_off` (see the
    /// signature's comment). Gates: disp32 at match+4 in 0x40..0x400 and
    /// 4-aligned; the COMISS operand (rip at match+11) a float in (0.5, 30);
    /// the shutter global (rip at match+24) equal to `shutter_actor_global`
    /// when that resolved. Any failure un-resolves the AOB.
    fn derive_ddr_sel_intro(&mut self) {
        const NAME: &str = "ddr_sel_dps_ready_dwell";
        let Some(site) = self.get_address(NAME) else {
            return;
        };
        let fail = |this: &mut Self, why: &str| {
            this.resolved.remove(NAME);
            log_warn!("  [-] {} -- {}; un-resolved", NAME, why);
        };
        unsafe {
            let off = std::ptr::read_unaligned(site.add(4) as *const u32) as usize;
            if !(0x40..0x400).contains(&off) || off % 4 != 0 {
                return fail(self, "implausible timer offset");
            }
            let threshold = decode_rip_relative(site.add(11));
            let t_off = (threshold as usize).wrapping_sub(self.base as usize);
            if t_off + 4 > self.size {
                return fail(self, "threshold outside the module");
            }
            let t = std::ptr::read_unaligned(threshold as *const f32);
            if !(t > 0.5 && t < 30.0) {
                return fail(self, "threshold is not a plausible dwell");
            }
            if let Some(global) = self.get_address("shutter_actor_global") {
                if decode_rip_relative(site.add(24)) != global {
                    return fail(self, "shutter global disagrees with shutter_actor_global");
                }
            }
            self.publish_value("ddr_sel_dps_ready_timer_off", off);
            log_info!("  [+] {} verified (dwell {:.2} s)", NAME, t);
        }
    }

    /// The DancePlaySequence READY?-dwell timer offset, or `None`.
    pub fn ddr_sel_dps_ready_timer_off(&self) -> Option<usize> {
        self.published_value("ddr_sel_dps_ready_timer_off")
    }

    /// Derive everything ddr_selection's legacy stage panel needs from World's
    /// ShutterActor (RE: `.agents/planning/2026-09-22-ddr-selection/research/
    /// stage-panel.md`). All-or-nothing; publishes on every build:
    ///
    /// * `ddr_sel_shutter_update` — RTTI `ShutterActor` vtable slot 6; the
    ///   swap and stage-tail matches must lie inside its first 0x1000 bytes;
    /// * `ddr_sel_shutter_basename_off` / `ddr_sel_shutter_jacket_off` — from
    ///   the swap's two string copies (kind offsets cross-checked against
    ///   `shutter_actor_layout`);
    /// * `ddr_sel_shutter_stage_voice` — the stage-voice function (the CALL
    ///   ending the stage tail; the tail's layer slot must be
    ///   `0x88 + stage_kind * 0x10`);
    /// * `ddr_sel_stage_voice_jz` — the voice mute-filter `JZ rel8` inside it
    ///   (`MOV [rsp+x],5; CALL [filter]; CMP [rsp+x],6; JZ skip`), gated on the
    ///   skipped block holding the slot-3 play (`MOV ECX,3; CALL`) — the byte
    ///   ddr_selection::sound::code_se flips while the legacy panel is hosted;
    /// * `ddr_sel_shutter_stage_row` + `ddr_sel_shutter_row_stride` — the
    ///   default kind table's stage row (pkg NULL, root `"shutter_play"`, SE in
    ///   `"se_start_game"`);
    /// * `ddr_sel_shutter_jacket_vis_guard` — new layout only, informational:
    ///   the `TEST RAX,RAX; JZ` null check of `find("jacket_usr")`;
    /// * `ddr_sel_shutter_jacket_vis_call` — old layout only: the state-2
    ///   `CALL SetVisible` on the un-null-checked `find("jacket_usr")`
    ///   (shape-checked: `"jacket_usr"` LEA, the stage layer slot, the same
    ///   `find` as the stage tail, `MOV DL,1; MOV RCX,RAX; CALL`);
    /// * `ddr_sel_panel_host_ok` — 1 when World can host A3's root: the
    ///   `jacket_usr` SetVisible is null-checked (20260721+), or the old
    ///   unchecked CALL was recognised (20250805 / 20260224, NOPed while
    ///   hosted); else 0.
    fn derive_ddr_sel_panel(&mut self) {
        const TAG: &str = "ddr_sel_panel";
        let (Some(swap), Some(tail)) = (
            self.get_address("ddr_sel_shutter_swap"),
            self.get_address("ddr_sel_shutter_stage_tail"),
        ) else {
            log_warn!("  [-] {} -- swap / stage tail unresolved", TAG);
            return;
        };
        let table_new = self.get_address("ddr_sel_shutter_kind_table");
        let table_old = self.get_address("ddr_sel_shutter_kind_table_v1");
        let Some(layout) = self.shutter_actor_layout() else {
            log_warn!("  [-] {} -- shutter_actor_layout underived", TAG);
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let inside = |p: *const u8| (p as usize).wrapping_sub(base) < size;
        let cstr_is = |p: *const u8, want: &[u8]| -> bool {
            if !inside(p) || (p as usize - base) + want.len() + 1 > size {
                return false;
            }
            unsafe { std::slice::from_raw_parts(p, want.len()) == want && *p.add(want.len()) == 0 }
        };
        let Some(vt) = self.find_vtable_by_rtti(
            ".?AVShutterActor@shutter@common@sequence@@",
            "shutter_actor_vtable",
        ) else {
            log_warn!("  [-] {} -- ShutterActor RTTI vtable not found", TAG);
            return;
        };
        unsafe {
            let update = *(vt as *const *const u8).add(6);
            let within = |p: *const u8| (p as usize).wrapping_sub(update as usize) < 0x1000;
            if !inside(update) || !within(swap) || !within(tail) {
                log_warn!(
                    "  [-] {} -- swap / stage tail outside ShutterActor::onUpdate",
                    TAG
                );
                return;
            }
            let rd = |p: *const u8| std::ptr::read_unaligned(p as *const u32) as usize;
            let (act, pend) = (rd(swap.add(2)), rd(swap.add(8)));
            if act != layout.active_kind
                || pend != layout.pending_kind
                || rd(swap.add(14)) != act
                || rd(swap.add(20)) != pend
            {
                log_warn!(
                    "  [-] {} -- swap kind offsets disagree with shutter_actor_layout",
                    TAG
                );
                return;
            }
            let basename_off = rd(swap.add(34));
            let jacket_off = rd(swap.add(53));
            if rd(swap.add(27)) + 0x28 != basename_off
                || rd(swap.add(46)) != jacket_off + 0x28
                || !(0x200..0x800).contains(&basename_off)
                || !(0x200..0x800).contains(&jacket_off)
            {
                log_warn!("  [-] {} -- implausible basename / jacket offsets", TAG);
                return;
            }

            if !cstr_is(decode_rip_relative(tail.add(3)), b"choice_stage_usr") {
                log_warn!(
                    "  [-] {} -- stage tail LEA is not \"choice_stage_usr\"",
                    TAG
                );
                return;
            }
            let stage_kind = layout.stage_kind.max(0) as usize;
            if rd(tail.add(10)) != 0x88 + stage_kind * 0x10 {
                log_warn!(
                    "  [-] {} -- stage tail layer slot disagrees with the stage kind",
                    TAG
                );
                return;
            }
            let voice = decode_call_rel32(tail.add(34));
            if !inside(voice) {
                log_warn!("  [-] {} -- stage voice call outside module", TAG);
                return;
            }
            let guard =
                std::slice::from_raw_parts(tail.sub(15), 5) == [0x48, 0x85, 0xC0, 0x74, 0x0A];
            // Old builds (20250805 / 20260224): the `jacket_usr` SetVisible is
            // NOT null-checked — `LEA RDX,["jacket_usr"]; MOV RCX,[RSI+slot];
            // CALL find; MOV DL,1; MOV RCX,RAX; CALL SetVisible` ends right at
            // the tail. The legacy root has no direct `jacket_usr`, so that
            // CALL (return value unused) is what ddr_selection NOPs while it
            // hosts A3's root (`ddr_sel_shutter_jacket_vis_call`).
            let jacket_vis_call = if guard {
                None
            } else {
                let pre = tail.sub(29);
                let find_call = pre.add(14);
                let shape_ok = std::slice::from_raw_parts(pre, 3) == [0x48, 0x8D, 0x15]
                    && cstr_is(decode_rip_relative(pre.add(3)), b"jacket_usr")
                    && std::slice::from_raw_parts(pre.add(7), 3) == [0x48, 0x8B, 0x8E]
                    && rd(pre.add(10)) == 0x88 + stage_kind * 0x10
                    && *find_call == 0xE8
                    && decode_call_rel32(find_call) == decode_call_rel32(tail.add(14))
                    && std::slice::from_raw_parts(pre.add(19), 6)
                        == [0xB2, 0x01, 0x48, 0x8B, 0xC8, 0xE8]
                    && inside(decode_call_rel32(pre.add(24)));
                if !shape_ok {
                    log_warn!(
                        "  [-] {} -- unchecked jacket_usr SetVisible not recognised (old layout)",
                        TAG
                    );
                    return;
                }
                Some(pre.add(24))
            };

            let mute = scan_pattern_all(
                voice,
                0x300,
                "C7 44 24 ?? 05 00 00 00 48 8D 4C 24 ?? FF 15 ?? ?? ?? ?? 83 7C 24 ?? 06 74 ??",
            );
            if mute.len() != 1 {
                log_warn!(
                    "  [-] {} -- stage voice mute gate not unique ({} hits)",
                    TAG,
                    mute.len()
                );
                return;
            }
            let jz = mute[0].address.add(24);
            let skipped = std::slice::from_raw_parts(jz.add(2), *jz.add(1) as usize);
            if *jz != 0x74
                || !skipped
                    .windows(6)
                    .any(|w| w == [0xB9, 0x03, 0x00, 0x00, 0x00, 0xE8])
            {
                log_warn!(
                    "  [-] {} -- stage voice mute JZ does not skip the play",
                    TAG
                );
                return;
            }

            let (table, stride) = match (table_new, table_old) {
                (Some(t), None) => (self.base.offset(rd(t.add(103)) as i32 as isize), 0x40usize),
                (None, Some(t)) => (self.base.offset(rd(t.add(11)) as i32 as isize), 0x30usize),
                _ => {
                    log_warn!("  [-] {} -- kind table not uniquely resolved", TAG);
                    return;
                }
            };
            let row = table.add(stage_kind * stride);
            let field = |i: usize| *(row as *const *const u8).add(i);
            if !inside(row)
                || !field(0).is_null()
                || !cstr_is(field(1), b"shutter_play")
                || !cstr_is(field(2), b"se_start_game")
                || !cstr_is(field(3), b"")
            {
                log_warn!(
                    "  [-] {} -- stage row is not the stock shutter_play row",
                    TAG
                );
                return;
            }

            // The end banners (optional — a miss leaves World's banners):
            // CLEARED / FAILED are the next two kinds after the stage panel
            // on every build (4 / 5 on 20260721+, 2 / 3 on the old layout),
            // and their rows follow the stage row in the same table.
            let banner_rows = {
                let cleared = row.add(stride);
                let failed = row.add(2 * stride);
                let f = |r: *const u8, i: usize| *(r as *const *const u8).add(i);
                let ok = inside(failed.add(6 * 8))
                    && f(cleared, 0).is_null()
                    && cstr_is(f(cleared, 1), b"shutter_cleared")
                    && cstr_is(f(cleared, 2), b"se_game_clear")
                    && cstr_is(f(cleared, 3), b"")
                    && cstr_is(f(cleared, 4), b"vo_stage_clear")
                    && cstr_is(f(cleared, 5), b"")
                    && f(failed, 0).is_null()
                    && cstr_is(f(failed, 1), b"shutter_failed")
                    && cstr_is(f(failed, 2), b"se_game_failed")
                    && cstr_is(f(failed, 3), b"")
                    && cstr_is(f(failed, 4), b"")
                    && cstr_is(f(failed, 5), b"");
                if !ok {
                    log_warn!(
                        "  [-] {} -- CLEARED / FAILED rows are not the stock rows after the stage row (legacy end banners off)",
                        TAG
                    );
                }
                ok.then_some((cleared, failed))
            };

            // Both layouts can host: new builds null-check `jacket_usr`, old
            // builds get the SetVisible CALL NOPed while hosted. The old
            // 0x30-stride rows share the first three pointers (pkg, root, SE
            // in) with the 0x40 rows, and the old loader has the same
            // named-package branch (its mode-9 table uses it).
            let host_ok = guard || jacket_vis_call.is_some();
            let rel = |p: *const u8| (p as usize).wrapping_sub(base);
            for (name, p) in [
                ("ddr_sel_shutter_update", update),
                ("ddr_sel_shutter_stage_voice", voice),
                ("ddr_sel_stage_voice_jz", jz),
                ("ddr_sel_shutter_stage_row", row),
            ]
            .into_iter()
            // Exactly one of the two per build (the sweep's ALT_GROUPS pair):
            // the new layout's null check (informational) or the old layout's
            // CALL the panel NOPs while hosted.
            .chain(guard.then(|| ("ddr_sel_shutter_jacket_vis_guard", tail.sub(15))))
            .chain(jacket_vis_call.map(|p| ("ddr_sel_shutter_jacket_vis_call", p)))
            .chain(banner_rows.map(|(c, _)| ("ddr_sel_shutter_cleared_row", c)))
            .chain(banner_rows.map(|(_, f)| ("ddr_sel_shutter_failed_row", f)))
            {
                self.resolved.insert(name.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", name, rel(p));
            }
            self.publish_value("ddr_sel_shutter_basename_off", basename_off);
            self.publish_value("ddr_sel_shutter_jacket_off", jacket_off);
            self.publish_value("ddr_sel_shutter_row_stride", stride);
            self.publish_value("ddr_sel_panel_host_ok", host_ok as usize);
            if banner_rows.is_some() {
                self.publish_value("ddr_sel_shutter_cleared_kind", stage_kind + 1);
                self.publish_value("ddr_sel_shutter_failed_kind", stage_kind + 2);
            }
        }
    }

    /// Everything [`derive_ddr_sel_panel`] produced, or `None` unless the whole
    /// group resolved.
    pub fn ddr_sel_panel_sites(&self) -> Option<DdrSelPanelSites> {
        Some(DdrSelPanelSites {
            shutter_update: self.get_address("ddr_sel_shutter_update")?,
            stage_voice: self.get_address("ddr_sel_shutter_stage_voice")?,
            stage_row: self.get_address("ddr_sel_shutter_stage_row")?,
            row_stride: self.published_value("ddr_sel_shutter_row_stride")?,
            basename_off: self.published_value("ddr_sel_shutter_basename_off")?,
            jacket_off: self.published_value("ddr_sel_shutter_jacket_off")?,
            host_ok: self.published_value("ddr_sel_panel_host_ok")? == 1,
            jacket_vis_call: self.get_address("ddr_sel_shutter_jacket_vis_call"),
            banner_rows: self.ddr_sel_banner_rows(),
        })
    }

    /// The end-banner rows [`derive_ddr_sel_panel`] published, or `None`.
    fn ddr_sel_banner_rows(&self) -> Option<DdrSelBannerRows> {
        Some(DdrSelBannerRows {
            cleared_row: self.get_address("ddr_sel_shutter_cleared_row")?,
            failed_row: self.get_address("ddr_sel_shutter_failed_row")?,
            cleared_kind: self.published_value("ddr_sel_shutter_cleared_kind")? as i32,
            failed_kind: self.published_value("ddr_sel_shutter_failed_kind")? as i32,
        })
    }

    /// Derive ddr_selection's `_sel` background-movie sites (RE:
    /// `.agents/planning/2026-09-22-ddr-selection/research/
    /// end-banners-sel-movies.md` §4). All-or-nothing:
    ///
    /// * `ddr_sel_sma_init` — RTTI `SceneManageActor` vtable slot 4
    ///   (`onInitialize`); the `ddr_sel_sma_movie_gate` match must lie in its
    ///   first 0x60 bytes;
    /// * `ddr_sel_music_info_lookup` — the gate's CALL (the basename LEA
    ///   `48 8D ?? disp32` 0x16 before it, `[RCX+disp]`, gives
    ///   `ddr_sel_sma_basename_off`);
    /// * `ddr_sel_music_movie_kind_off` / `_kind2_off` — the gate's two
    ///   `MOVZX ECX,byte [RAX+disp32]`; `ddr_sel_sma_video_size_off` — the
    ///   instruction the null-entry JZ lands on (`CMP dword [R+disp],1` /
    ///   `MOV ECX,[R+disp]`);
    /// * `ddr_sel_sma_movie_off` — the unique `CALL ctor; NOP; MOV
    ///   [R+disp32],RAX; MOV RDX,RAX` after the gate, the ctor referencing
    ///   the RTTI `MovieActor` vtable; `ddr_sel_sma_suffix_off` — the
    ///   `LEA R8,[R+disp32]` (same base register) between the two; the
    ///   allocation size (`MOV EDX,imm32` before the pool alloc) must cover
    ///   the flag byte;
    /// * `ddr_sel_movie_sel_flag_off` / `ddr_sel_music_movie_name_off` /
    ///   `ddr_sel_movie_path_off` — from `ddr_sel_movie_sel_test`, gated on
    ///   the tried function's `"_sel"` string and on the match lying within
    ///   0x100 of the first CALL of MovieActor vtable slot 4.
    fn derive_ddr_sel_movie(&mut self) {
        const TAG: &str = "ddr_sel_movie";
        let (Some(gate), Some(test), Some(sma_vt), Some(movie_vt)) = (
            self.get_address("ddr_sel_sma_movie_gate"),
            self.get_address("ddr_sel_movie_sel_test"),
            self.get_address("scene_manage_actor_vtable"),
            self.get_address("movie_actor_vtable"),
        ) else {
            log_warn!(
                "  [-] {} -- gate / _sel test / SceneManageActor or MovieActor vtable unresolved",
                TAG
            );
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let inside = |p: *const u8| (p as usize).wrapping_sub(base) < size;
        let plausible = |v: usize| (0x40..0x400).contains(&v);
        unsafe {
            let rd = |p: *const u8| std::ptr::read_unaligned(p as *const u32) as usize;
            let bytes = |p: *const u8, n: usize| std::slice::from_raw_parts(p, n);

            let sma_init = *(sma_vt as *const *const u8).add(4);
            if !inside(sma_init) || (gate as usize).wrapping_sub(sma_init as usize) >= 0x60 {
                log_warn!(
                    "  [-] {} -- movie gate not in SceneManageActor::onInitialize",
                    TAG
                );
                return;
            }
            let lookup = decode_call_rel32(gate);
            let lea = gate.sub(0x16);
            if !inside(lookup)
                || bytes(lea, 2) != [0x48, 0x8D]
                || *lea.add(2) & 0xC7 != 0x81
                || !plausible(rd(lea.add(3)))
            {
                log_warn!(
                    "  [-] {} -- music-info lookup / basename LEA not recognised",
                    TAG
                );
                return;
            }
            let basename_off = rd(lea.add(3));
            let (kind_off, kind2_off) = (rd(gate.add(13)), rd(gate.add(25)));
            let vs = gate.add(49);
            let vs_ok = (bytes(vs, 2) == [0x83, 0xBF]
                || bytes(vs, 2) == [0x83, 0xBB]
                || bytes(vs, 2) == [0x8B, 0x8B]
                || bytes(vs, 2) == [0x8B, 0x8F])
                && plausible(rd(vs.add(2)));
            if !plausible(kind_off) || !plausible(kind2_off) || !vs_ok {
                log_warn!(
                    "  [-] {} -- movie-byte / VIDEO SIZE fields implausible",
                    TAG
                );
                return;
            }
            let video_size_off = rd(vs.add(2));

            // The MovieActor creation after the gate.
            let body = gate.add(49);
            let stores = scan_pattern_all(
                body,
                0x180,
                "E8 ?? ?? ?? ?? 90 48 89 ?? ?? ?? ?? ?? 48 8B D0",
            );
            if stores.len() != 1 {
                log_warn!(
                    "  [-] {} -- MovieActor store not unique ({} hits)",
                    TAG,
                    stores.len()
                );
                return;
            }
            let store = stores[0].address;
            let modrm = *store.add(8);
            let ctor = decode_call_rel32(store);
            let ctor_has_vt = inside(ctor)
                && scan_pattern_all(ctor, 0x120, "48 8D ?? ?? ?? ?? ??")
                    .iter()
                    .any(|m| {
                        *m.address.add(2) & 0xC7 == 0x05
                            && decode_rip_relative(m.address.add(3)) == movie_vt
                    });
            if modrm & 0xF8 != 0x80 || !ctor_has_vt || !plausible(rd(store.add(9))) {
                log_warn!(
                    "  [-] {} -- MovieActor store / ctor identity not recognised",
                    TAG
                );
                return;
            }
            let movie_off = rd(store.add(9));
            let rm = modrm & 0x07;
            let span = (store as usize - body as usize) + 1;
            let suffix: Vec<_> = scan_pattern_all(body, span, "4C 8D ?? ?? ?? 00 00")
                .into_iter()
                .filter(|m| *m.address.add(2) == 0x80 | rm)
                .collect();
            let allocs = scan_pattern_all(body, span, "BA ?? ?? 00 00 48 8B 0D");
            if suffix.len() != 1 || allocs.len() != 1 || !plausible(rd(suffix[0].address.add(3))) {
                log_warn!(
                    "  [-] {} -- suffix LEA / MovieActor allocation not recognised",
                    TAG
                );
                return;
            }
            let suffix_off = rd(suffix[0].address.add(3));
            let alloc_size = rd(allocs[0].address.add(1));

            // MovieActor::onInitialize's `_sel` search.
            let init = *(movie_vt as *const *const u8).add(4);
            let first_call = if inside(init) {
                scan_pattern_all(init, 0x40, "E8 ?? ?? ?? ??")
                    .first()
                    .map(|m| decode_call_rel32(m.address))
            } else {
                None
            };
            let try_sel = decode_call_rel32(test.add(67));
            // `LEA R8,[rip+"_sel"]` (4C 8D 05) — any RIP-relative LEA.
            let sel_str = inside(try_sel)
                && scan_pattern_all(try_sel, 0x100, "?? 8D ?? ?? ?? ?? ??")
                    .iter()
                    .any(|m| {
                        let rex = *m.address;
                        let p = decode_rip_relative(m.address.add(3));
                        (rex == 0x48 || rex == 0x4C)
                            && *m.address.add(2) & 0xC7 == 0x05
                            && inside(p)
                            && bytes(p, 5) == b"_sel\0"
                    });
            let flag_off = rd(test.add(43));
            let name_off = rd(test.add(27));
            let path_off = rd(test.add(60));
            let near = first_call.is_some_and(|f| (test as usize).wrapping_sub(f as usize) < 0x100);
            if !near
                || !sel_str
                || rd(test.add(3)) != name_off + 0x10
                || !plausible(name_off)
                || !plausible(path_off)
                || !(0x100..alloc_size).contains(&flag_off)
            {
                log_warn!(
                    "  [-] {} -- MovieActor _sel test not recognised (flag +0x{:X}, alloc 0x{:X})",
                    TAG,
                    flag_off,
                    alloc_size
                );
                return;
            }

            let rel = |p: *const u8| (p as usize).wrapping_sub(base);
            for (name, p) in [
                ("ddr_sel_sma_init", sma_init),
                ("ddr_sel_music_info_lookup", lookup),
            ] {
                self.resolved.insert(name.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", name, rel(p));
            }
            for (name, v) in [
                ("ddr_sel_music_movie_kind_off", kind_off),
                ("ddr_sel_music_movie_kind2_off", kind2_off),
                ("ddr_sel_music_movie_name_off", name_off),
                ("ddr_sel_sma_basename_off", basename_off),
                ("ddr_sel_sma_suffix_off", suffix_off),
                ("ddr_sel_sma_video_size_off", video_size_off),
                ("ddr_sel_sma_movie_off", movie_off),
                ("ddr_sel_movie_sel_flag_off", flag_off),
                ("ddr_sel_movie_path_off", path_off),
            ] {
                self.publish_value(name, v);
            }
        }
    }

    /// Everything [`derive_ddr_sel_movie`] produced, or `None`.
    pub fn ddr_sel_movie_sites(&self) -> Option<DdrSelMovieSites> {
        Some(DdrSelMovieSites {
            sma_init: self.get_address("ddr_sel_sma_init")?,
            music_lookup: self.get_address("ddr_sel_music_info_lookup")?,
            movie_kind_off: self.published_value("ddr_sel_music_movie_kind_off")?,
            movie_kind2_off: self.published_value("ddr_sel_music_movie_kind2_off")?,
            movie_name_off: self.published_value("ddr_sel_music_movie_name_off")?,
            sma_basename_off: self.published_value("ddr_sel_sma_basename_off")?,
            sma_suffix_off: self.published_value("ddr_sel_sma_suffix_off")?,
            sma_video_size_off: self.published_value("ddr_sel_sma_video_size_off")?,
            sma_movie_off: self.published_value("ddr_sel_sma_movie_off")?,
            movie_actor_vtable: self.get_address("movie_actor_vtable")?,
            sel_flag_off: self.published_value("ddr_sel_movie_sel_flag_off")?,
            movie_path_off: self.published_value("ddr_sel_movie_path_off")?,
        })
    }

    /// Resolve the gameplay HUD layout builder entry and its per-side extras.
    ///
    /// * `hud_layout_builder_entry` — the `hud_layout_builder` prologue AOB
    ///   when it matched (20260324+), else a backward scan from the
    ///   build-stable `hud_layout_builder_style_cluster` (exactly one match
    ///   required) for the frame-size-agnostic prologue head
    ///   `MOV RAX,RSP; PUSH RBP; PUSH R12..R15; LEA RBP,[RAX+disp32]`
    ///   (entry = cluster − 0x1DC on every inspected build; the full AOB
    ///   bakes in per-build frame constants and misses 20250805 / 20260224).
    ///   Moved here from center_arrows_single (identical logic).
    /// * all-or-nothing extras (RE: `.agents/planning/2026-09-22-ddr-selection/
    ///   research/hud-layout-stage-frame.md` §6): `hud_layout_style_off`,
    ///   `hud_layout_reverse_off`, `hud_layout_judge_pos_vslot`, each read
    ///   from a unique site inside the builder's first 0x2000 bytes; the style
    ///   offset must equal the style cluster's literal 0x84, the reverse byte
    ///   must sit inside the per-side 0x48 block that starts at the record map
    ///   `ddr_sel_records_side_off` when that resolved.
    fn derive_hud_layout(&mut self) {
        const TAG: &str = "hud_layout";
        const PROLOGUE_HEAD: &str = "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D A8";
        const SCAN_BACK: usize = 0x400;
        let clusters = self.get_all_matches("hud_layout_builder_style_cluster");
        let entry = match self.get_address("hud_layout_builder") {
            Some(e) => Some(e),
            None => match clusters.as_slice() {
                [cluster] => {
                    let lo = (*cluster as usize)
                        .saturating_sub(SCAN_BACK)
                        .max(self.base as usize);
                    let span = *cluster as usize - lo;
                    scan_pattern_all(lo as *const u8, span, PROLOGUE_HEAD)
                        .last()
                        .map(|m| m.address as *const u8)
                }
                other => {
                    log_warn!(
                        "  [-] {} -- builder AOB missed and style cluster resolved {} matches (want 1)",
                        TAG,
                        other.len()
                    );
                    None
                }
            },
        };
        let Some(entry) = entry else {
            log_warn!("  [-] {} -- builder entry unresolved", TAG);
            return;
        };
        let base = self.base as usize;
        self.resolved
            .insert("hud_layout_builder_entry".into(), entry);
        log_info!(
            "  [+] hud_layout_builder_entry (derived) @ +0x{:X}",
            (entry as usize).wrapping_sub(base)
        );

        const WINDOW: usize = 0x2000;
        let within = |p: *const u8| (p as usize).wrapping_sub(entry as usize) < WINDOW;
        let (Some(side_loop), Some(rev_store), Some(judge_call)) = (
            self.get_address("hud_layout_side_loop"),
            self.get_address("hud_layout_reverse_store"),
            self.get_address("hud_layout_judge_pos_call"),
        ) else {
            log_warn!(
                "  [-] {} -- side loop / reverse store / judge_position call unresolved",
                TAG
            );
            return;
        };
        if !within(side_loop) || !within(rev_store) || !within(judge_call) {
            log_warn!(
                "  [-] {} -- side loop / reverse store / judge_position call outside the builder",
                TAG
            );
            return;
        }
        let rd = |p: *const u8| unsafe { std::ptr::read_unaligned(p as *const u32) as usize };
        let style_off = unsafe { rd(side_loop.add(4)) };
        let reverse_off = unsafe { rd(rev_store.add(24)) };
        let judge_vslot = unsafe { rd(judge_call.add(10)) };
        if clusters.len() == 1 && style_off != 0x84 {
            log_warn!(
                "  [-] {} -- side-loop style offset 0x{:X} disagrees with the style cluster (0x84)",
                TAG,
                style_off
            );
            return;
        }
        let side_ok = match self.published_value("ddr_sel_records_side_off") {
            Some(side) => (side..side + 0x48).contains(&reverse_off),
            None => (0x80..0x200).contains(&reverse_off),
        };
        if !(0x40..0x200).contains(&style_off)
            || !side_ok
            || judge_vslot % 8 != 0
            || !(0x100..0x800).contains(&judge_vslot)
        {
            log_warn!(
                "  [-] {} -- implausible style 0x{:X} / reverse 0x{:X} / judge_position vslot 0x{:X}",
                TAG,
                style_off,
                reverse_off,
                judge_vslot
            );
            return;
        }
        self.publish_value("hud_layout_style_off", style_off);
        self.publish_value("hud_layout_reverse_off", reverse_off);
        self.publish_value("hud_layout_judge_pos_vslot", judge_vslot);
    }

    /// The HUD layout builder / setter pair (+ the side extras when they
    /// resolved), or `None` when either function is missing.
    pub fn hud_layout_sites(&self) -> Option<HudLayoutSites> {
        let side = (|| {
            Some(HudLayoutSideSites {
                style_off: self.published_value("hud_layout_style_off")?,
                reverse_off: self.published_value("hud_layout_reverse_off")?,
                judge_pos_vslot: self.published_value("hud_layout_judge_pos_vslot")?,
            })
        })();
        Some(HudLayoutSites {
            builder: self.get_address("hud_layout_builder_entry")?,
            setter: self.get_address("hud_layout_setter")?,
            side,
        })
    }

    /// Resolve ddr_selection's legacy stage-frame patch sites (RE:
    /// `.agents/planning/2026-09-22-ddr-selection/research/
    /// hud-layout-stage-frame.md` §8): the StageFrameActor RTTI vtable →
    /// init (slot 4) and msg (slot 8); the export LEA must lie in the init's
    /// first 0x200 bytes and load `"dance_stage"`; the msg fn's `E8` at +0x24
    /// is the texture fn, the texture site must lie in its first 0x100 bytes,
    /// carry imm32 0xB and load `"dast_stage_"`. All-or-nothing:
    /// `ddr_sel_stage_frame_export_lea` / `ddr_sel_stage_frame_texture_site`.
    fn derive_ddr_sel_stage_frame(&mut self) {
        const TAG: &str = "ddr_sel_stage_frame";
        let (Some(export), Some(texture)) = (
            self.get_address("ddr_sel_stage_frame_export"),
            self.get_address("ddr_sel_stage_frame_texture"),
        ) else {
            log_warn!("  [-] {} -- export / texture site unresolved", TAG);
            return;
        };
        let Some(vt) = self.find_vtable_by_rtti(
            ".?AVStageFrameActor@dance@sequence@@",
            "stage_frame_actor_vtable",
        ) else {
            log_warn!("  [-] {} -- StageFrameActor RTTI vtable not found", TAG);
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let inside = |p: *const u8| (p as usize).wrapping_sub(base) < size;
        let cstr_is = |p: *const u8, want: &[u8]| -> bool {
            if !inside(p) || (p as usize - base) + want.len() + 1 > size {
                return false;
            }
            unsafe { std::slice::from_raw_parts(p, want.len()) == want && *p.add(want.len()) == 0 }
        };
        unsafe {
            let init = *(vt as *const *const u8).add(4);
            let msg = *(vt as *const *const u8).add(8);
            if !inside(init) || !inside(msg) {
                log_warn!("  [-] {} -- StageFrameActor slots outside the module", TAG);
                return;
            }
            if (export as usize).wrapping_sub(init as usize) >= 0x200
                || !cstr_is(decode_rip_relative(export.add(3)), b"dance_stage")
            {
                log_warn!(
                    "  [-] {} -- export LEA not \"dance_stage\" inside StageFrameActor::onInitialize",
                    TAG
                );
                return;
            }
            let call = msg.add(0x24);
            if *call != 0xE8 {
                log_warn!("  [-] {} -- msg+0x24 is not the texture-fn CALL", TAG);
                return;
            }
            let tex_fn = decode_call_rel32(call);
            let imm = std::ptr::read_unaligned(texture.add(2) as *const u32);
            if !inside(tex_fn)
                || (texture as usize).wrapping_sub(tex_fn as usize) >= 0x100
                || imm != 0xB
                || !cstr_is(decode_rip_relative(texture.add(9)), b"dast_stage_")
            {
                log_warn!(
                    "  [-] {} -- texture site not the \"dast_stage_\" assign of the texture fn",
                    TAG
                );
                return;
            }
            for (name, p) in [
                ("ddr_sel_stage_frame_export_lea", export),
                ("ddr_sel_stage_frame_texture_site", texture),
            ] {
                self.resolved.insert(name.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", name, (p as usize) - base);
            }
        }
    }

    /// Everything [`derive_ddr_sel_stage_frame`] produced, or `None`.
    pub fn ddr_sel_stage_frame_sites(&self) -> Option<DdrSelStageFrameSites> {
        Some(DdrSelStageFrameSites {
            export_lea: self.get_address("ddr_sel_stage_frame_export_lea")?,
            texture_site: self.get_address("ddr_sel_stage_frame_texture_site")?,
        })
    }

    /// Resolve ddr_selection's legacy life-gauge sites (RE:
    /// `.agents/planning/2026-09-22-ddr-selection/research/legacy-gauge.md`).
    /// All from the gauge-family RTTI vtables, no AOB: every percent-family
    /// vtable (Normal / Grade / Flare / Immortal) must share slot 4 (init) and
    /// slot 6 (update); in each init exactly one `LEA R8,[rip+"dance_gauge"];
    /// MOV R9D,imm` (the clip create — the record-key and error-log LEAs are
    /// not followed by `MOV R9D`); the fill = the one CALL target of the update
    /// whose first 0x80 bytes load `"fill _usr"`; offsets read from the
    /// instructions that use them (record skin store after `MOV r,[RAX+0x28]`,
    /// clip store after the create, `LEA RAX,[RCX+value]` in the fill, the
    /// update's `CALL [RAX+0x50]; MOV ESI,EAX; CMP [RDI+state],EAX` + the
    /// following `CALL [R8+label]`, the init's `MOVSS XMM2,[1.0]; MOV RAX,[RCX];
    /// MOVSS XMM1,XMM2; CALL [RAX+set_scale]`, the skin-3 block's root-MC load).
    /// All-or-nothing.
    fn derive_ddr_sel_gauge(&mut self) {
        const TAG: &str = "ddr_sel_gauge";
        let percent = [
            "normal_gauge_vtable",
            "grade_gauge_vtable",
            "flare_gauge_vtable",
            "immortal_gauge_vtable",
        ];
        let Some(vts) = percent
            .iter()
            .map(|n| self.get_address(n))
            .collect::<Option<Vec<_>>>()
        else {
            log_warn!(
                "  [-] {} -- a percent-family gauge vtable is unresolved",
                TAG
            );
            return;
        };
        let Some(life_vt) = self.get_address("life_gauge_vtable") else {
            log_warn!("  [-] {} -- LifeGaugeActor vtable unresolved", TAG);
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let inside = |p: *const u8| (p as usize).wrapping_sub(base) < size;
        let cstr_is = |p: *const u8, want: &[u8]| -> bool {
            if !inside(p) || (p as usize - base) + want.len() + 1 > size {
                return false;
            }
            unsafe { std::slice::from_raw_parts(p, want.len()) == want && *p.add(want.len()) == 0 }
        };
        unsafe {
            let slot = |vt: *const u8, i: usize| *(vt as *const *const u8).add(i);
            let rd = |p: *const u8| std::ptr::read_unaligned(p as *const u32) as usize;
            let bytes = |p: *const u8, n: usize| std::slice::from_raw_parts(p, n);
            let (init, update) = (slot(vts[0], 4), slot(vts[0], 6));
            if vts
                .iter()
                .any(|v| slot(*v, 4) != init || slot(*v, 6) != update)
                || !inside(init)
                || !inside(update)
            {
                log_warn!(
                    "  [-] {} -- percent-family vtables do not share init / update",
                    TAG
                );
                return;
            }
            let life_init = slot(life_vt, 4);
            if !inside(life_init) {
                log_warn!("  [-] {} -- LifeGaugeActor init outside the module", TAG);
                return;
            }
            // The clip-create export LEA (exactly one per init).
            let export_lea = |f: *const u8| -> Option<*const u8> {
                let hits: Vec<*const u8> =
                    scan_pattern_all(f, 0x200, "4C 8D 05 ?? ?? ?? ?? 41 B9 ?? 00 00 00")
                        .into_iter()
                        .map(|m| m.address as *const u8)
                        .filter(|p| cstr_is(decode_rip_relative(p.add(3)), b"dance_gauge"))
                        .collect();
                (hits.len() == 1).then(|| hits[0])
            };
            let (Some(g_lea), Some(l_lea)) = (export_lea(init), export_lea(life_init)) else {
                log_warn!(
                    "  [-] {} -- clip-create \"dance_gauge\" LEA not unique",
                    TAG
                );
                return;
            };
            // Record skin: `MOV r32,[RAX+0x28]` then `MOV [RDI+disp32],r32`.
            let skin_store = |f: *const u8| -> Option<usize> {
                for i in 0..0x60usize {
                    let p = f.add(i);
                    if *p == 0x8B && (*p.add(1) & 0xC7) == 0x40 && *p.add(2) == 0x28 {
                        let reg = (*p.add(1) >> 3) & 7;
                        for j in 3..0x20usize {
                            let q = p.add(j);
                            if *q == 0x89 && *q.add(1) == (0x87 | (reg << 3)) {
                                return Some(rd(q.add(2)));
                            }
                        }
                    }
                }
                None
            };
            // Clip store after the create: `MOV [RDI+disp32],r64`.
            let clip_store = |lea: *const u8| -> Option<usize> {
                for i in 0..0x90usize {
                    let q = lea.add(i);
                    if *q == 0x48 && *q.add(1) == 0x89 && (*q.add(2) & 0xC7) == 0x87 {
                        return Some(rd(q.add(3)));
                    }
                }
                None
            };
            let (Some(g_skin), Some(l_skin), Some(g_clip), Some(l_clip)) = (
                skin_store(init),
                skin_store(life_init),
                clip_store(g_lea),
                clip_store(l_lea),
            ) else {
                log_warn!("  [-] {} -- skin / clip stores not recognised", TAG);
                return;
            };
            // Side parent: `MOV RCX,[RCX+disp32]` right before the init's
            // record-key LEA `LEA RDX,["dance_gauge"]`.
            let side_off = {
                let mut v = None;
                for i in 0..0x40usize {
                    let p = init.add(i);
                    if bytes(p, 3) == [0x48, 0x8B, 0x89]
                        && bytes(p.add(7), 3) == [0x48, 0x8D, 0x15]
                        && cstr_is(decode_rip_relative(p.add(10)), b"dance_gauge")
                    {
                        v = Some(rd(p.add(3)));
                        break;
                    }
                }
                v
            };
            // The fill: the update's one CALL target loading "fill _usr".
            let mut fills: Vec<*const u8> = Vec::new();
            for i in 0..0xB00usize {
                let p = update.add(i);
                if *p != 0xE8 {
                    continue;
                }
                let t = decode_call_rel32(p);
                if !inside(t) || (t as usize - base) + 0x80 > size {
                    continue;
                }
                let loads = scan_pattern_all(t, 0x80, "48 8D 15 ?? ?? ?? ??")
                    .iter()
                    .any(|m| cstr_is(decode_rip_relative(m.address.add(3)), b"fill _usr"));
                if loads && !fills.contains(&t) {
                    fills.push(t);
                }
            }
            let [fill] = fills.as_slice() else {
                log_warn!(
                    "  [-] {} -- update has {} fill candidates (want 1)",
                    TAG,
                    fills.len()
                );
                return;
            };
            let fill = *fill;
            let value_off = scan_pattern_all(fill, 0x40, "48 8D 81 ?? ?? 00 00")
                .first()
                .map(|m| rd(m.address.add(3)));
            let clip_in_fill = scan_pattern_all(fill, 0x80, "48 8B 89 ?? ?? 00 00")
                .first()
                .map(|m| rd(m.address.add(3)));
            let state = scan_pattern_all(update, 0x300, "FF 50 50 8B F0 39 87 ?? ?? 00 00 0F 84");
            let (state_off, label_vslot) = match state.as_slice() {
                [m] => {
                    let so = rd(m.address.add(7));
                    let lbl = scan_pattern_all(m.address, 0x30, "41 FF 50 ??")
                        .first()
                        .map(|x| *x.address.add(3) as usize);
                    (Some(so), lbl)
                }
                _ => (None, None),
            };
            let scale = scan_pattern_all(
                init,
                0x300,
                "F3 0F 10 15 ?? ?? ?? ?? 48 8B 01 F3 0F 10 CA FF 90 ?? ?? 00 00",
            );
            let scale_vslot = match scale.as_slice() {
                [m] => Some(rd(m.address.add(17))),
                _ => None,
            };
            // Skin-3 block: `CMP [RDI+skin],3` then a `MOV r32,[r64+root]` load.
            let root_off = {
                let mut v = None;
                let cmp = [0x83, 0xBF];
                for i in 0..0x600usize {
                    let p = init.add(i);
                    if bytes(p, 2) == cmp && rd(p.add(2)) == g_skin && *p.add(6) == 3 {
                        for j in 7..0x40usize {
                            let q = p.add(j);
                            if matches!(*q, 0x41 | 0x44 | 0x45)
                                && *q.add(1) == 0x8B
                                && (*q.add(2) & 0xC0) == 0x80
                            {
                                v = Some(rd(q.add(3)));
                                break;
                            }
                        }
                        break;
                    }
                }
                v
            };
            let (
                Some(side_off),
                Some(value_off),
                Some(clip_in_fill),
                Some(state_off),
                Some(label_vslot),
                Some(scale_vslot),
                Some(root_off),
            ) = (
                side_off,
                value_off,
                clip_in_fill,
                state_off,
                label_vslot,
                scale_vslot,
                root_off,
            )
            else {
                log_warn!(
                    "  [-] {} -- side / value / state / label / SetScale / root-MC offsets not recognised",
                    TAG
                );
                return;
            };
            let plausible = |v: usize| (0x40..0x400).contains(&v);
            if clip_in_fill != g_clip
                || ![
                    side_off, g_skin, g_clip, value_off, state_off, l_skin, l_clip, root_off,
                ]
                .iter()
                .all(|v| plausible(*v))
                || label_vslot % 8 != 0
                || scale_vslot % 8 != 0
                || !(0x40..0x400).contains(&scale_vslot)
            {
                log_warn!(
                    "  [-] {} -- implausible offsets (skin 0x{:X}/0x{:X}, clip 0x{:X}/0x{:X}/fill 0x{:X}, value 0x{:X}, state 0x{:X}, label 0x{:X}, scale 0x{:X}, root 0x{:X})",
                    TAG, g_skin, l_skin, g_clip, l_clip, clip_in_fill, value_off, state_off, label_vslot, scale_vslot, root_off
                );
                return;
            }
            for (name, p) in [
                ("ddr_sel_gauge_init", init),
                ("ddr_sel_life_gauge_init", life_init),
                ("ddr_sel_gauge_fill", fill),
                ("ddr_sel_gauge_export_lea", g_lea),
                ("ddr_sel_life_gauge_export_lea", l_lea),
            ] {
                self.resolved.insert(name.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", name, (p as usize) - base);
            }
            for (name, v) in [
                ("ddr_sel_gauge_side_off", side_off),
                ("ddr_sel_gauge_skin_off", g_skin),
                ("ddr_sel_gauge_clip_off", g_clip),
                ("ddr_sel_gauge_value_off", value_off),
                ("ddr_sel_gauge_state_off", state_off),
                ("ddr_sel_gauge_label_vslot", label_vslot),
                ("ddr_sel_life_gauge_skin_off", l_skin),
                ("ddr_sel_life_gauge_clip_off", l_clip),
                ("ddr_sel_clip_root_mc_off", root_off),
                ("ddr_sel_clip_set_scale_vslot", scale_vslot),
            ] {
                self.publish_value(name, v);
            }
        }
    }

    /// Everything [`derive_ddr_sel_gauge`] produced, or `None`.
    pub fn ddr_sel_gauge_sites(&self) -> Option<DdrSelGaugeSites> {
        Some(DdrSelGaugeSites {
            gauge_init: self.get_address("ddr_sel_gauge_init")?,
            life_init: self.get_address("ddr_sel_life_gauge_init")?,
            gauge_fill: self.get_address("ddr_sel_gauge_fill")?,
            gauge_export_lea: self.get_address("ddr_sel_gauge_export_lea")?,
            life_export_lea: self.get_address("ddr_sel_life_gauge_export_lea")?,
            side_off: self.published_value("ddr_sel_gauge_side_off")?,
            gauge_skin_off: self.published_value("ddr_sel_gauge_skin_off")?,
            gauge_clip_off: self.published_value("ddr_sel_gauge_clip_off")?,
            gauge_value_off: self.published_value("ddr_sel_gauge_value_off")?,
            gauge_state_off: self.published_value("ddr_sel_gauge_state_off")?,
            gauge_label_vslot: self.published_value("ddr_sel_gauge_label_vslot")?,
            life_skin_off: self.published_value("ddr_sel_life_gauge_skin_off")?,
            life_clip_off: self.published_value("ddr_sel_life_gauge_clip_off")?,
            clip_root_mc_off: self.published_value("ddr_sel_clip_root_mc_off")?,
            clip_set_scale_vslot: self.published_value("ddr_sel_clip_set_scale_vslot")?,
        })
    }

    /// ddr_selection's A3 option icons (optional, all-or-nothing, never
    /// required; consumer `ddr_selection::option_icons`):
    ///
    /// * RTTI `OptionIconActor@dance` → init (slot 4), update (slot 6); in the
    ///   init `LEA RDX,["dance_option"]; MOV RCX,[RCX+holder]; CALL record`,
    ///   `MOVSXD RCX,[RAX]; LEA RAX,[table]; (XOR EDX,EDX;) MOV RCX,[RAX+RCX*8];
    ///   CALL resolver` (old builds pass one argument) and the CALL after
    ///   `LEA RDX,["option_icon"]` (the marker getter);
    /// * RTTI `Option@player@ddr`: each field getter slot must be the 4-byte
    ///   stub `8B 41 off C3` (speed type 0x208, hispeed 0x220, gauge 0x230,
    ///   scroll 0x238, visibility 0x250, lane cover 0x268, step zone 0x280,
    ///   boost 0x2A8, turn 0x2B0, colour 0x2B8, cut 0x2C8, freeze 0x2D0, jump
    ///   0x2D8, flare level 0x310); the derived real-speed multiplier = the
    ///   other `MOV EAX,[RBX+off]` in the effective-speed getter (0x218, which
    ///   first calls vt+0x208);
    /// * RTTI `CSprite@BM2D` + the SpriteLayer's pool create: `LEA RAX,[pool];
    ///   MOV RBX,RSI; XOR R8D,R8D; MOV RDX,R12; IMUL RBX,RBX,stride; ADD
    ///   RBX,RAX; MOV RCX,RBX; CALL create` (unique on all five builds) with
    ///   `CMP EBX,count` in the free-slot loop before it; create must call
    ///   vt+0xE0 (SetPriority).
    fn derive_ddr_sel_option_icons(&mut self) {
        const TAG: &str = "ddr_sel_option_icons";
        let Some(vt) = self.find_vtable_by_rtti(".?AVOptionIconActor@dance@sequence@@", TAG) else {
            return;
        };
        let Some(opt_vt) = self.find_vtable_by_rtti(".?AVOption@player@ddr@@", TAG) else {
            return;
        };
        let Some(spr_vt) = self.find_vtable_by_rtti(".?AVCSprite@BM2D@@", TAG) else {
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let in_mod = |p: *const u8, n: usize| (p as usize).wrapping_sub(base) + n <= size;
        let cstr_is = |p: *const u8, want: &[u8]| -> bool {
            in_mod(p, want.len() + 1)
                && unsafe {
                    std::slice::from_raw_parts(p, want.len()) == want && *p.add(want.len()) == 0
                }
        };
        unsafe {
            let init = *(vt as *const *const u8).add(4);
            let update = *(vt as *const *const u8).add(6);
            if !in_mod(init, 0x200) || !in_mod(update, 0x40) {
                log_warn!("  [-] {} -- actor functions outside the module", TAG);
                return;
            }
            let body = std::slice::from_raw_parts(init, 0x200);
            // record lookup
            let rec = (0..0x200 - 16).find(|&o| {
                body[o] == 0x48
                    && body[o + 1] == 0x8D
                    && body[o + 2] == 0x15
                    && body[o + 7] == 0x48
                    && body[o + 8] == 0x8B
                    && body[o + 9] == 0x49
                    && body[o + 11] == 0xE8
                    && cstr_is(decode_rip_relative(init.add(o + 3)), b"dance_option")
            });
            let Some(rec) = rec else {
                log_warn!("  [-] {} -- dance_option record lookup", TAG);
                return;
            };
            let holder_off = body[rec + 10] as usize;
            let record_fn = decode_call_rel32(init.add(rec + 11));
            // option resolver
            let mut resolver = None;
            for o in rec..0x200 - 20 {
                if body[o..o + 6] != [0x48, 0x63, 0x08, 0x48, 0x8D, 0x05] {
                    continue;
                }
                let tail = if body[o + 10..o + 12] == [0x33, 0xD2] {
                    o + 12
                } else {
                    o + 10
                };
                if body[tail..tail + 5] == [0x48, 0x8B, 0x0C, 0xC8, 0xE8] {
                    resolver = Some((
                        decode_rip_relative(init.add(o + 6)),
                        decode_call_rel32(init.add(tail + 4)),
                    ));
                }
                break;
            }
            let Some((option_table, option_resolver)) = resolver else {
                log_warn!("  [-] {} -- option resolver call", TAG);
                return;
            };
            // marker getter: the CALL after LEA RDX,["option_icon"] (the
            // whole init; the create block sits between).
            let whole = std::slice::from_raw_parts(init, 0x400.min(size - (init as usize - base)));
            let mut marker_fn = None;
            for o in 0..whole.len() - 7 {
                if whole[o] == 0x48
                    && whole[o + 1] == 0x8D
                    && whole[o + 2] == 0x15
                    && cstr_is(decode_rip_relative(init.add(o + 3)), b"option_icon")
                {
                    marker_fn = (o + 7..(o + 0x14).min(whole.len() - 5))
                        .find(|&q| whole[q] == 0xE8)
                        .map(|q| decode_call_rel32(init.add(q)));
                    break;
                }
            }
            let Some(marker_fn) = marker_fn else {
                log_warn!("  [-] {} -- option_icon marker getter", TAG);
                return;
            };
            if !in_mod(record_fn, 0x10)
                || !in_mod(marker_fn, 0x10)
                || !in_mod(option_resolver, 0x10)
                || !in_mod(option_table, 16)
            {
                log_warn!("  [-] {} -- callees outside the module", TAG);
                return;
            }
            // Option getters.
            let stub = |slot: usize| -> Option<usize> {
                let f = *(opt_vt as *const *const u8).add(slot / 8);
                if !in_mod(f, 4) {
                    return None;
                }
                let b = std::slice::from_raw_parts(f, 4);
                (b[0] == 0x8B && b[1] == 0x41 && b[3] == 0xC3).then_some(b[2] as usize)
            };
            let wanted: [(usize, usize, &str); 14] = [
                (0x208, 0x08, "speed type"),
                (0x220, 0x0C, "hispeed"),
                (0x230, 0x18, "gauge"),
                (0x238, 0x1C, "scroll"),
                (0x250, 0x28, "visibility"),
                (0x268, 0x34, "lane cover"),
                (0x280, 0x40, "step zone"),
                (0x2A8, 0x54, "boost"),
                (0x2B0, 0x58, "turn"),
                (0x2B8, 0x5C, "colour"),
                (0x2C8, 0x64, "cut"),
                (0x2D0, 0x68, "freeze"),
                (0x2D8, 0x6C, "jump"),
                (0x310, 0x7C, "flare level"),
            ];
            let mut offs = [0usize; 14];
            for (i, (slot, want, what)) in wanted.iter().enumerate() {
                match stub(*slot) {
                    Some(o) if o == *want => offs[i] = o,
                    other => {
                        log_warn!(
                            "  [-] {} -- Option getter +0x{:X} ({}) is {:?} (want +0x{:X})",
                            TAG,
                            slot,
                            what,
                            other,
                            want
                        );
                        return;
                    }
                }
            }
            let eff = *(opt_vt as *const *const u8).add(0x218 / 8);
            if !in_mod(eff, 0x40) {
                log_warn!("  [-] {} -- effective-speed getter outside the module", TAG);
                return;
            }
            let eb = std::slice::from_raw_parts(eff, 0x40);
            if !eb.windows(6).any(|w| w == [0xFF, 0x90, 0x08, 0x02, 0, 0]) {
                log_warn!("  [-] {} -- effective-speed getter shape", TAG);
                return;
            }
            let loads: Vec<usize> = (0..0x3D)
                .filter(|&o| eb[o] == 0x8B && eb[o + 1] >> 6 == 1 && eb[o + 1] & 7 != 4)
                .map(|o| eb[o + 2] as usize)
                .collect();
            let derived: Vec<usize> = loads.iter().copied().filter(|&o| o != 0x0C).collect();
            let [speed_derived] = derived.as_slice() else {
                log_warn!("  [-] {} -- derived-speed load {:?}", TAG, loads);
                return;
            };
            if !loads.contains(&0x0C) || *speed_derived != 0x10 {
                log_warn!("  [-] {} -- effective-speed loads {:?}", TAG, loads);
                return;
            }
            // Sprite pool.
            let hits = scan_pattern_all(
                self.base,
                self.size,
                "48 8D 05 ?? ?? ?? ?? 48 8B DE 45 33 C0 49 8B D4 48 69 DB ?? ?? ?? ?? 48 03 D8 48 8B CB E8",
            );
            let [hit] = hits.as_slice() else {
                log_warn!(
                    "  [-] {} -- {} sprite-pool create sites (want 1)",
                    TAG,
                    hits.len()
                );
                return;
            };
            let m = hit.address as *const u8;
            let pool = decode_rip_relative(m.add(3));
            let stride = (m.add(19) as *const u32).read_unaligned() as usize;
            let create = decode_call_rel32(m.add(29));
            let before = std::slice::from_raw_parts(m.sub(0x40), 0x40);
            let count = (0..0x3A)
                .find(|&o| before[o] == 0x81 && before[o + 1] == 0xFB)
                .map(|o| {
                    u32::from_le_bytes([before[o + 2], before[o + 3], before[o + 4], before[o + 5]])
                        as usize
                });
            let Some(count) = count.filter(|c| (1..=0x10000).contains(c)) else {
                log_warn!("  [-] {} -- sprite pool count", TAG);
                return;
            };
            if stride < 0x100
                || stride > 0x1000
                || !in_mod(pool, count * stride)
                || !in_mod(create, 0x80)
            {
                log_warn!("  [-] {} -- sprite pool / create outside the module", TAG);
                return;
            }
            let cb = std::slice::from_raw_parts(create, 0x80);
            let prio = cb.windows(6).any(|w| {
                w[0] == 0xFF && (0x90..=0x97).contains(&w[1]) && w[2..6] == [0xE0, 0, 0, 0]
            }) || cb.windows(7).any(|w| {
                w[0] == 0x41
                    && w[1] == 0xFF
                    && (0x90..=0x97).contains(&w[2])
                    && w[3..7] == [0xE0, 0, 0, 0]
            });
            if !prio {
                log_warn!("  [-] {} -- CSprite::Create shape", TAG);
                return;
            }
            for (n, p) in [
                ("ddr_sel_option_icon_init", init),
                ("ddr_sel_option_icon_update", update),
                ("ddr_sel_option_icon_record_fn", record_fn),
                ("ddr_sel_option_icon_marker_fn", marker_fn),
                ("ddr_sel_option_resolver", option_resolver),
                ("ddr_sel_option_table", option_table),
                ("ddr_sel_option_vtable", opt_vt),
                ("ddr_sel_sprite_vtable", spr_vt),
                ("ddr_sel_sprite_pool", pool),
                ("ddr_sel_sprite_create", create),
            ] {
                self.resolved.insert(n.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", n, p as usize - base);
            }
            self.publish_value("ddr_sel_option_icon_holder_off", holder_off);
            self.publish_value("ddr_sel_sprite_count", count);
            self.publish_value("ddr_sel_sprite_stride", stride);
            let names = [
                "speed_type",
                "hispeed",
                "gauge",
                "scroll",
                "visibility",
                "lane_cover",
                "stepzone",
                "boost",
                "turn",
                "color",
                "cut",
                "freeze",
                "jump",
                "flare",
            ];
            for (n, o) in names.iter().zip(offs.iter()) {
                self.publish_value(&format!("ddr_sel_option_{}_off", n), *o);
            }
            self.publish_value("ddr_sel_option_speed_derived_off", *speed_derived);
        }
    }

    /// DDR SELECTION's 1st-5th option forcing (Step 12): the eleven World
    /// `ddr::player::Option` fields A3 forced on skin 1, each verified by its
    /// `MOV EAX,[RCX+off]; RET` getter stub on the RTTI vtable, plus the
    /// effective-speed getter (`type == 1 ⇒ +0x0C`, so a forced type 1 +
    /// hispeed 100 plays at ×1.00). Publishes `ddr_sel_force_<name>_off`.
    /// Optional and independent of the option-icon group (RE:
    /// `.agents/planning/2026-09-22-ddr-selection/research/option-forcing.md`).
    fn derive_ddr_sel_option_force(&mut self) {
        const TAG: &str = "ddr_sel_option_force";
        let Some(opt_vt) = self.find_vtable_by_rtti(".?AVOption@player@ddr@@", TAG) else {
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let in_mod = |p: *const u8, n: usize| (p as usize).wrapping_sub(base) + n <= size;
        // (getter vslot, field offset, name) — `options_force_logic::FIELDS`.
        const WANTED: [(usize, usize, &str); 11] = [
            (0x208, 0x08, "speed_type"),
            (0x220, 0x0C, "hispeed"),
            (0x2A8, 0x54, "scroll_moving"),
            (0x250, 0x28, "visibility"),
            (0x268, 0x34, "lane_cover"),
            (0x280, 0x40, "stepzone"),
            (0x238, 0x1C, "scroll_direction"),
            (0x2B8, 0x5C, "arrow_color"),
            (0x2C0, 0x60, "arrow_design"),
            (0x260, 0x30, "lane_filter"),
            (0x278, 0x3C, "guideline"),
        ];
        unsafe {
            for (slot, want, name) in WANTED {
                let f = *(opt_vt as *const *const u8).add(slot / 8);
                let got = if in_mod(f, 4) {
                    let b = std::slice::from_raw_parts(f, 4);
                    (b[0] == 0x8B && b[1] == 0x41 && b[3] == 0xC3).then_some(b[2] as usize)
                } else {
                    None
                };
                if got != Some(want) {
                    log_warn!(
                        "  [-] {} -- Option getter +0x{:X} ({}) is {:?} (want +0x{:X})",
                        TAG,
                        slot,
                        name,
                        got,
                        want
                    );
                    return;
                }
            }
            // Effective speed: `CALL [RAX+0x208]; CMP EAX,1; JNE; MOV EAX,[RBX+0x0C]`.
            let eff = *(opt_vt as *const *const u8).add(0x218 / 8);
            if !in_mod(eff, 0x20) {
                log_warn!("  [-] {} -- effective-speed getter outside the module", TAG);
                return;
            }
            let eb = std::slice::from_raw_parts(eff, 0x20);
            let shape = eb.windows(12).any(|w| {
                w[0..6] == [0xFF, 0x90, 0x08, 0x02, 0, 0]
                    && w[6..9] == [0x83, 0xF8, 0x01]
                    && w[9] == 0x75
                    && w[11] == 0x8B
            }) && eb.windows(3).any(|w| w == [0x8B, 0x43, 0x0C]);
            if !shape {
                log_warn!("  [-] {} -- effective-speed getter shape", TAG);
                return;
            }
            self.resolved
                .insert("ddr_sel_force_option_vtable".into(), opt_vt);
            log_info!(
                "  [+] ddr_sel_force_option_vtable (derived) @ +0x{:X}",
                opt_vt as usize - base
            );
            for (_, off, name) in WANTED {
                self.publish_value(&format!("ddr_sel_force_{}_off", name), off);
            }
        }
    }

    /// A field offset [`derive_ddr_sel_option_force`] verified (`name` =
    /// `options_force_logic::Field::name`), or `None`.
    pub fn ddr_sel_option_force_offset(&self, name: &str) -> Option<usize> {
        self.get_address("ddr_sel_force_option_vtable")?;
        self.published_value(&format!("ddr_sel_force_{}_off", name))
    }

    /// Everything [`derive_ddr_sel_option_icons`] produced, or `None`.
    pub fn ddr_sel_option_icon_sites(&self) -> Option<DdrSelOptionIconSites> {
        let f = |n: &str| self.published_value(&format!("ddr_sel_option_{}_off", n));
        Some(DdrSelOptionIconSites {
            init: self.get_address("ddr_sel_option_icon_init")?,
            update: self.get_address("ddr_sel_option_icon_update")?,
            holder_off: self.published_value("ddr_sel_option_icon_holder_off")?,
            record_fn: self.get_address("ddr_sel_option_icon_record_fn")?,
            marker_fn: self.get_address("ddr_sel_option_icon_marker_fn")?,
            option_resolver: self.get_address("ddr_sel_option_resolver")?,
            option_table: self.get_address("ddr_sel_option_table")?,
            option_vtable: self.get_address("ddr_sel_option_vtable")?,
            fields: OptionFieldOffsets {
                speed_type: f("speed_type")?,
                hispeed: f("hispeed")?,
                speed_derived: self.published_value("ddr_sel_option_speed_derived_off")?,
                gauge: f("gauge")?,
                scroll: f("scroll")?,
                visibility: f("visibility")?,
                lane_cover: f("lane_cover")?,
                stepzone: f("stepzone")?,
                boost: f("boost")?,
                turn: f("turn")?,
                color: f("color")?,
                cut: f("cut")?,
                freeze: f("freeze")?,
                jump: f("jump")?,
                flare: f("flare")?,
            },
            sprite_vtable: self.get_address("ddr_sel_sprite_vtable")?,
            sprite_pool: self.get_address("ddr_sel_sprite_pool")?,
            sprite_count: self.published_value("ddr_sel_sprite_count")?,
            sprite_stride: self.published_value("ddr_sel_sprite_stride")?,
            sprite_create: self.get_address("ddr_sel_sprite_create")?,
        })
    }

    /// Derive World's `CallVoiceActor` (the announcer) from its RTTI vtable
    /// (consumer: `services::call_voice_hooks`). All-or-nothing:
    ///
    /// * `onUpdate` = slot 6; when the `announcer_dispatcher` AOB resolved it
    ///   must be the same function;
    /// * A3's field layout, checked by the exact instructions World's
    ///   `onUpdate` uses (each exactly once, identical on 20250805 …
    ///   20260915): step state `MOVZX EAX,word [RCX+0x82]; MOV ECX,[RCX+RAX*8
    ///   +0x58]`, combos `LEA RDX,[RBX+0xB0]` / `LEA RCX,[RBX+0xA4]`,
    ///   milestone `MOV ECX,[RBX+0x98]`, gauges `MOVSS XMM0,[RBX+0xA0]` /
    ///   `COMISS XMM0,[RBX+0xAC]`, mutes `CMP byte [RBX+0x9C]/[RBX+0x9D],0`,
    ///   time `CMP [RBX+0x88],ECX`, was-low `MOV byte [RBX+0x9E],1`, regain
    ///   gate `CMP dword [RBX+0x8C],0x4E20`;
    /// * the voice guard = the CALL right after `LEA reg,["vo_ingame_state_01_
    ///   highest"]`; inside it the one `MOV ECX,[cnt]; TEST; JLE; CALL
    ///   [lock]; NOP; MOV EDX,EDI; MOV RCX,[mgr]; CALL is_playing; MOVZX
    ///   EDI,AL; MOV ECX,[cnt]; TEST; JLE; CALL [unlock]` (both counter loads
    ///   the same global; `mgr` == `audio_manager_global` when that resolved).
    fn derive_call_voice(&mut self) {
        const TAG: &str = "call_voice";
        let Some(vt) = self.find_vtable_by_rtti(".?AVCallVoiceActor@dance@sequence@@", TAG) else {
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let in_mod = |p: *const u8, n: usize| (p as usize).wrapping_sub(base) + n <= size;
        unsafe {
            let update = *(vt as *const *const u8).add(6);
            if !in_mod(update, 0x700) {
                log_warn!("  [-] {} -- onUpdate outside the module", TAG);
                return;
            }
            if let Some(aob) = self.get_address("announcer_dispatcher") {
                if aob != update {
                    log_warn!(
                        "  [-] {} -- onUpdate +0x{:X} is not announcer_dispatcher +0x{:X}",
                        TAG,
                        update as usize - base,
                        aob as usize - base
                    );
                    return;
                }
            }
            let body = std::slice::from_raw_parts(update, 0x700);
            let count = |hex: &[u8]| body.windows(hex.len()).filter(|w| *w == hex).count();
            let layout: [(&str, &[u8]); 11] = [
                (
                    "step state",
                    &[0x0F, 0xB7, 0x81, 0x82, 0, 0, 0, 0x8B, 0x4C, 0xC1, 0x58],
                ),
                ("combo 1", &[0x8D, 0x93, 0xB0, 0, 0, 0]),
                ("combo 0", &[0x8D, 0x8B, 0xA4, 0, 0, 0]),
                ("milestone", &[0x8B, 0x8B, 0x98, 0, 0, 0]),
                ("gauge 0", &[0xF3, 0x0F, 0x10, 0x83, 0xA0, 0, 0, 0]),
                ("gauge 1", &[0x0F, 0x2F, 0x83, 0xAC, 0, 0, 0]),
                ("voice mute", &[0x80, 0xBB, 0x9C, 0, 0, 0, 0]),
                ("se mute", &[0x80, 0xBB, 0x9D, 0, 0, 0, 0]),
                ("time", &[0x39, 0x8B, 0x88, 0, 0, 0]),
                ("was low", &[0xC6, 0x83, 0x9E, 0, 0, 0, 1]),
                (
                    "regain gate",
                    &[0x81, 0xBB, 0x8C, 0, 0, 0, 0x20, 0x4E, 0, 0],
                ),
            ];
            for (what, bytes) in layout {
                let n = count(bytes);
                if n != 1 {
                    log_warn!("  [-] {} -- {} instruction x{} (want 1)", TAG, what, n);
                    return;
                }
            }
            // The voice guard.
            let want: &[u8] = b"vo_ingame_state_01_highest";
            let mut guards: Vec<*const u8> = Vec::new();
            for o in 0..0x700 - 0x20 {
                let p = update.add(o);
                if !((*p == 0x48 || *p == 0x4C) && *p.add(1) == 0x8D && (*p.add(2) & 0xC7) == 0x05)
                {
                    continue;
                }
                let t = decode_rip_relative(p.add(3));
                if !in_mod(t, want.len() + 1)
                    || std::slice::from_raw_parts(t, want.len()) != want
                    || *t.add(want.len()) != 0
                {
                    continue;
                }
                if let Some(q) = (7..0x20).map(|k| p.add(k)).find(|q| **q == 0xE8) {
                    let g = decode_call_rel32(q);
                    if !guards.contains(&g) {
                        guards.push(g);
                    }
                }
            }
            let [guard] = guards.as_slice() else {
                log_warn!("  [-] {} -- {} voice guards (want 1)", TAG, guards.len());
                return;
            };
            if !in_mod(*guard, 0x100) {
                log_warn!("  [-] {} -- voice guard outside the module", TAG);
                return;
            }
            let hits = scan_pattern_all(
                *guard,
                0x100,
                "8B 0D ?? ?? ?? ?? 85 C9 7E 07 FF 15 ?? ?? ?? ?? 90 8B D7 48 8B 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 0F B6 F8 8B 0D ?? ?? ?? ?? 85 C9 7E 06 FF 15 ?? ?? ?? ??",
            );
            let [hit] = hits.as_slice() else {
                log_warn!(
                    "  [-] {} -- {} is-playing blocks in the guard (want 1)",
                    TAG,
                    hits.len()
                );
                return;
            };
            let m = hit.address as *const u8;
            let cnt = decode_rip_relative(m.add(2));
            let lock_iat = decode_rip_relative(m.add(12));
            let mgr = decode_rip_relative(m.add(22));
            let is_playing = decode_call_rel32(m.add(26));
            let cnt2 = decode_rip_relative(m.add(36));
            let unlock_iat = decode_rip_relative(m.add(46));
            if cnt != cnt2 {
                log_warn!("  [-] {} -- lock counters disagree", TAG);
                return;
            }
            if !in_mod(is_playing, 0x40)
                || !in_mod(mgr, 8)
                || !in_mod(cnt, 4)
                || !in_mod(lock_iat, 8)
                || !in_mod(unlock_iat, 8)
            {
                log_warn!("  [-] {} -- guard operands outside the module", TAG);
                return;
            }
            if let Some(g) = self.get_address("audio_manager_global") {
                if g != mgr {
                    log_warn!(
                        "  [-] {} -- guard manager +0x{:X} is not audio_manager_global +0x{:X}",
                        TAG,
                        mgr as usize - base,
                        g as usize - base
                    );
                    return;
                }
            }
            for (n, p) in [
                ("call_voice_update", update),
                ("call_voice_guard", *guard),
                ("call_voice_is_playing", is_playing),
                ("call_voice_audio_manager", mgr),
                ("call_voice_lock_count", cnt),
                ("call_voice_lock_iat", lock_iat),
                ("call_voice_unlock_iat", unlock_iat),
            ] {
                self.resolved.insert(n.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", n, p as usize - base);
            }
        }
    }

    /// Everything [`derive_call_voice`] produced, or `None`.
    pub fn call_voice_sites(&self) -> Option<CallVoiceSites> {
        Some(CallVoiceSites {
            update: self.get_address("call_voice_update")?,
            is_playing: self.get_address("call_voice_is_playing")?,
            audio_manager_global: self.get_address("call_voice_audio_manager")?,
            lock_count: self.get_address("call_voice_lock_count")?,
            lock_iat: self.get_address("call_voice_lock_iat")?,
            unlock_iat: self.get_address("call_voice_unlock_iat")?,
        })
    }

    /// Derive World's `ComboActor` functions and counter fields from its RTTI
    /// vtable (consumer: `services::combo_hooks`). All-or-nothing; the four
    /// functions must sit inside the module and each field is decoded from
    /// the instruction that uses it (identical on 20250805 … 20260915):
    ///
    /// * msg: `SUB EDX,0x1033` (the combo case), `MOV [RDI+combo],EAX` after
    ///   `MOV EAX,[R8+4]`, `MOV ECX,[RDI+worst]; CMP ECX,0xFF`, and the 0x103C
    ///   case's `MOV byte [RCX+gameover],1`;
    /// * update: `CMP byte [RCX+gameover],0` (must agree with the msg).
    fn derive_combo_actor(&mut self) {
        const TAG: &str = "combo_actor";
        let Some(vt) = self.find_vtable_by_rtti(".?AVComboActor@dance@sequence@@", TAG) else {
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let inside = |p: *const u8| (p as usize).wrapping_sub(base) + 0x100 < size;
        unsafe {
            let slot = |i: usize| *(vt as *const *const u8).add(i);
            let (init, finalize, update, msg) = (slot(4), slot(5), slot(6), slot(8));
            if ![init, finalize, update, msg].iter().all(|p| inside(*p)) {
                log_warn!("  [-] {} -- vtable slots outside the module", TAG);
                return;
            }
            let rd = |p: *const u8| std::ptr::read_unaligned(p as *const u32) as usize;
            let one = |f: *const u8, len: usize, pat: &str| -> Option<*const u8> {
                match scan_pattern_all(f, len, pat).as_slice() {
                    [m] => Some(m.address as *const u8),
                    _ => None,
                }
            };
            let case_1033 = one(msg, 0x20, "81 EA 33 10 00 00");
            let combo = one(msg, 0x80, "41 8B 40 04 89 47 ??").map(|p| *p.add(6) as usize);
            let worst = one(msg, 0x80, "8B 4F ?? 81 F9 FF 00 00 00").map(|p| *p.add(2) as usize);
            let go_msg = one(msg, 0x40, "C6 81 ?? ?? 00 00 01").map(|p| rd(p.add(2)));
            let go_upd = one(update, 0x20, "80 B9 ?? ?? 00 00 00").map(|p| rd(p.add(2)));
            let (Some(_), Some(combo), Some(worst), Some(go), Some(go_upd)) =
                (case_1033, combo, worst, go_msg, go_upd)
            else {
                log_warn!(
                    "  [-] {} -- msg / update shape not recognised (0x1033 case, combo / worst / game-over fields)",
                    TAG
                );
                return;
            };
            if go != go_upd || worst != combo + 4 || !(0x40..0x200).contains(&go) {
                log_warn!(
                    "  [-] {} -- implausible fields (combo 0x{:X}, worst 0x{:X}, game over 0x{:X} / 0x{:X})",
                    TAG,
                    combo,
                    worst,
                    go,
                    go_upd
                );
                return;
            }
            self.resolved.insert("combo_actor_vtable".into(), vt);
            for (name, p) in [
                ("combo_actor_init", init),
                ("combo_actor_finalize", finalize),
                ("combo_actor_update", update),
                ("combo_actor_msg", msg),
            ] {
                self.resolved.insert(name.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", name, (p as usize) - base);
            }
            for (name, v) in [
                ("combo_actor_combo_off", combo),
                ("combo_actor_worst_off", worst),
                ("combo_actor_gameover_off", go),
            ] {
                self.publish_value(name, v);
            }
        }
    }

    /// Everything [`derive_combo_actor`] produced, or `None`.
    pub fn combo_actor_sites(&self) -> Option<ComboActorSites> {
        Some(ComboActorSites {
            init: self.get_address("combo_actor_init")?,
            finalize: self.get_address("combo_actor_finalize")?,
            update: self.get_address("combo_actor_update")?,
            msg: self.get_address("combo_actor_msg")?,
            combo_off: self.published_value("combo_actor_combo_off")?,
            worst_off: self.published_value("combo_actor_worst_off")?,
            gameover_off: self.published_value("combo_actor_gameover_off")?,
        })
    }

    /// Derive the sites ddr_selection's legacy combo patches / calls inside
    /// World's `ComboActor::onInitialize` (`research/legacy-combo.md` §5).
    /// All-or-nothing; every site is found by content inside the init
    /// (string-identity-gated LEAs, exact loop head), identical on all five
    /// sweep builds.
    fn derive_ddr_sel_combo(&mut self) {
        const TAG: &str = "ddr_sel_combo";
        let Some(actor) = self.combo_actor_sites() else {
            log_warn!("  [-] {} -- combo_actor unresolved", TAG);
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let init = actor.init;
        const SPAN: usize = 0x400;
        if (init as usize - base) + SPAN + 0x40 > size {
            log_warn!("  [-] {} -- init too close to the module end", TAG);
            return;
        }
        let inside = |p: *const u8| (p as usize).wrapping_sub(base) + 0x40 < size;
        let cstr_is = |p: *const u8, want: &[u8]| -> bool {
            if !inside(p) {
                return false;
            }
            unsafe { std::slice::from_raw_parts(p, want.len()) == want && *p.add(want.len()) == 0 }
        };
        unsafe {
            let rd = |p: *const u8| std::ptr::read_unaligned(p as *const u32) as usize;
            let one = |pat: &str| -> Option<*const u8> {
                match scan_pattern_all(init, SPAN, pat).as_slice() {
                    [m] => Some(m.address as *const u8),
                    _ => None,
                }
            };
            // `LEA RDX,[rip+"<want>"]` followed within 12 bytes by a CALL.
            let lea_call = |want: &[u8]| -> Option<(*const u8, *const u8)> {
                let hits: Vec<(*const u8, *const u8)> =
                    scan_pattern_all(init, SPAN, "48 8D 15 ?? ?? ?? ??")
                        .into_iter()
                        .map(|m| m.address as *const u8)
                        .filter(|p| cstr_is(decode_rip_relative(p.add(3)), want))
                        .filter_map(|p| {
                            (7..19usize)
                                .map(|i| p.add(i))
                                .find(|q| **q == 0xE8)
                                .map(|q| (p, decode_call_rel32(q)))
                        })
                        .collect();
                (hits.len() == 1).then(|| hits[0])
            };
            let loop_head = one("41 BF 02 00 00 00 45 8D 6F 01 49 8D AE ?? ?? 00 00");
            let fmt = one("41 B8 12 00 00 00 48 8D 15 ?? ?? ?? ??")
                .map(|p| p.add(6))
                .filter(|p| cstr_is(decode_rip_relative(p.add(3)), b"dance_combo_root%d"));
            let record = lea_call(b"dance_combo");
            let marker = lea_call(b"combo");
            // Side holder: `MOV RCX,[R14+disp8]` between the record LEA and its CALL.
            let side_off = record.and_then(|(lea, _)| {
                (7..12usize)
                    .map(|i| lea.add(i))
                    .find(|q| std::slice::from_raw_parts(*q, 3) == [0x49, 0x8B, 0x4E])
                    .map(|q| *q.add(3) as usize)
            });
            let set_pos = marker.and_then(|(lea, _)| {
                match scan_pattern_all(lea, 0x40, "44 8B 43 04 8B 13 41 FF 51 ??").as_slice() {
                    [m] => Some(*m.address.add(9) as usize),
                    _ => None,
                }
            });
            let set_color = one("0F 28 CF FF 90 ?? ?? 00 00").map(|p| rd(p.add(5)));
            let root_mc = one("4C 8B 45 00 41 8B 98 ?? ?? 00 00").map(|p| rd(p.add(7)));
            // Loop tail: the refresh call (`combo > 0`), then `DEC R13D; SUB
            // RBP,8; DEC R15; JNS head` — the count the head patch zeroes.
            let steps_back = one("49 8B CE E8 ?? ?? ?? ?? 41 FF CD 48 83 ED 08 49 FF CF 0F 89")
                .filter(|p| {
                    self.get_address("combo_digit_refresh")
                        .is_none_or(|r| decode_call_rel32(p.add(3)) == r)
                });
            let (
                Some(loop_head),
                Some(fmt),
                Some((_, record_fn)),
                Some((_, marker_fn)),
                Some(side_off),
                Some(set_pos),
                Some(set_color),
                Some(root_mc),
                Some(_),
            ) = (
                loop_head, fmt, record, marker, side_off, set_pos, set_color, root_mc, steps_back,
            )
            else {
                log_warn!(
                    "  [-] {} -- init shape not recognised (loop head / \"dance_combo_root%d\" / record / marker / SetPosition / SetColor / root MC / loop tail + refresh call)",
                    TAG
                );
                return;
            };
            let root3 = rd(loop_head.add(13));
            let root1 = root3.wrapping_sub(16);
            let plausible = |v: usize| (0x40..0x200).contains(&v);
            if !inside(record_fn)
                || !inside(marker_fn)
                || !plausible(side_off)
                || !plausible(root1)
                || root1 <= side_off
                || root1 <= actor.worst_off
                || set_pos % 8 != 0
                || set_color % 8 != 0
                || !(0x20..0x200).contains(&set_color)
                || !(0x100..0x200).contains(&root_mc)
            {
                log_warn!(
                    "  [-] {} -- implausible values (side 0x{:X}, roots 0x{:X}..0x{:X}, SetPosition 0x{:X}, SetColor 0x{:X}, root MC 0x{:X})",
                    TAG, side_off, root1, root3, set_pos, set_color, root_mc
                );
                return;
            }
            for (name, p) in [
                ("ddr_sel_combo_loop_head", loop_head),
                ("ddr_sel_combo_root_fmt_lea", fmt),
                ("ddr_sel_combo_record_fn", record_fn),
                ("ddr_sel_combo_marker_fn", marker_fn),
            ] {
                self.resolved.insert(name.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", name, (p as usize) - base);
            }
            for (name, v) in [
                ("ddr_sel_combo_side_off", side_off),
                ("ddr_sel_combo_root1_off", root1),
                ("ddr_sel_combo_root3_off", root3),
                ("ddr_sel_combo_set_position_vslot", set_pos),
                ("ddr_sel_combo_set_color_vslot", set_color),
                ("ddr_sel_combo_root_mc_off", root_mc),
            ] {
                self.publish_value(name, v);
            }
        }
    }

    /// Derive the sites ddr_selection's legacy score port needs in World's
    /// `ScoreActor` (`research/legacy-score.md` §5). All-or-nothing, from the
    /// RTTI vtable (slots 4 / 7 / 8); every field decoded from the
    /// instruction that uses it. Identical on all five sweep builds.
    fn derive_ddr_sel_score(&mut self) {
        const TAG: &str = "ddr_sel_score";
        let Some(vt) = self.get_address("score_actor_vtable") else {
            log_warn!("  [-] {} -- score_actor_vtable unresolved", TAG);
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let inside = |p: *const u8| (p as usize).wrapping_sub(base) + 0x600 < size;
        let cstr_is = |p: *const u8, want: &[u8]| -> bool {
            (p as usize).wrapping_sub(base) + want.len() + 1 < size
                && unsafe {
                    std::slice::from_raw_parts(p, want.len()) == want && *p.add(want.len()) == 0
                }
        };
        unsafe {
            let slot = |i: usize| *(vt as *const *const u8).add(i);
            let (init, digits, msg) = (slot(4), slot(7), slot(8));
            if ![init, digits, msg].iter().all(|p| inside(*p)) {
                log_warn!("  [-] {} -- vtable slots outside the module", TAG);
                return;
            }
            let rd = |p: *const u8| std::ptr::read_unaligned(p as *const u32) as usize;
            let one = |f: *const u8, len: usize, pat: &str| -> Option<*const u8> {
                match scan_pattern_all(f, len, pat).as_slice() {
                    [m] => Some(m.address as *const u8),
                    _ => None,
                }
            };
            // The three creates, in order, each followed by its clip store
            // `MOV [RSI+disp],RBX` (disp8 or disp32).
            let creates: Vec<*const u8> =
                scan_pattern_all(init, 0x500, "41 B9 07 00 00 00 4C 8D 05 ?? ?? ?? ??")
                    .into_iter()
                    .map(|m| m.address as *const u8)
                    .collect();
            let names: [&[u8]; 3] = [b"dance_score", b"dance_difficulty", b"dance_name"];
            let ok_creates = creates.len() == 3
                && creates
                    .iter()
                    .zip(names.iter())
                    .all(|(c, n)| cstr_is(decode_rip_relative(c.add(9)), n));
            let store_after = |c: *const u8| -> Option<usize> {
                for i in 13..0x80usize {
                    let q = c.add(i);
                    if *q == 0x48 && *q.add(1) == 0x89 {
                        match *q.add(2) {
                            0x5E => return Some(*q.add(3) as usize),
                            0x9E => return Some(rd(q.add(3))),
                            _ => {}
                        }
                    }
                }
                None
            };
            // `MOV RCX,[RCX+side]; CALL record; MOV R8,RAX; MOV EAX,[RAX+0x28];
            // MOV [RSI+skin],EAX`.
            let head = one(
                init,
                0x60,
                "48 8B 49 ?? E8 ?? ?? ?? ?? 4C 8B C0 8B 40 28 89 46 ??",
            );
            let ex = one(init, 0x500, "88 86 ?? ?? 00 00").map(|p| rd(p.add(2)));
            let dig = one(
                digits,
                0x40,
                "48 83 79 ?? 00 48 8B F1 0F 84 ?? ?? ?? ?? 44 8B 41 ?? 8B 49 ??",
            );
            let case = one(msg, 0x80, "81 EA 36 10 00 00 0F 84 ?? ?? ?? ?? 83 FA 19");
            let diff = one(
                msg,
                0x100,
                "48 63 43 ?? 48 8B 74 C4 ?? 4C 8B 83 ?? ?? 00 00",
            );
            let level = one(msg, 0x300, "4C 63 43 ?? 44 8B 4B ??");
            let (true, Some(head), Some(ex), Some(dig), Some(_), Some(diff), Some(level)) =
                (ok_creates, head, ex, dig, case, diff, level)
            else {
                log_warn!(
                    "  [-] {} -- ScoreActor shape not recognised (creates / record head / EX store / digit head / 0x1036+0x104F case / difficulty / level)",
                    TAG
                );
                return;
            };
            let side_off = *head.add(3) as usize;
            let skin_off = *head.add(17) as usize;
            let score_clip = store_after(creates[0]);
            let diff_clip = store_after(creates[1]);
            let name_clip = store_after(creates[2]);
            let (Some(score_clip), Some(diff_clip), Some(name_clip)) =
                (score_clip, diff_clip, name_clip)
            else {
                log_warn!("  [-] {} -- clip stores not recognised", TAG);
                return;
            };
            let dig_clip = *dig.add(3) as usize;
            let displayed = *dig.add(17) as usize;
            let target = *dig.add(20) as usize;
            let difficulty = *diff.add(3) as usize;
            let diff_clip_msg = rd(diff.add(12));
            let level_diff = *level.add(3) as usize;
            let level_off = *level.add(7) as usize;
            let plausible = |v: usize| (0x40..0x200).contains(&v);
            if dig_clip != score_clip
                || diff_clip_msg != diff_clip
                || level_diff != difficulty
                || displayed != target + 4
                || ![
                    side_off, skin_off, level_off, target, displayed, difficulty, score_clip,
                    diff_clip, name_clip, ex,
                ]
                .iter()
                .all(|v| plausible(*v))
            {
                log_warn!(
                    "  [-] {} -- implausible fields (side 0x{:X}, skin 0x{:X}, level 0x{:X}, target 0x{:X}, displayed 0x{:X}, difficulty 0x{:X}/0x{:X}, clips 0x{:X}/0x{:X}/0x{:X} (digits 0x{:X}, msg 0x{:X}), EX 0x{:X})",
                    TAG, side_off, skin_off, level_off, target, displayed, difficulty, level_diff,
                    score_clip, diff_clip, name_clip, dig_clip, diff_clip_msg, ex
                );
                return;
            }
            for (name, p) in [
                ("ddr_sel_score_init", init),
                ("ddr_sel_score_digits", digits),
                ("ddr_sel_score_msg", msg),
                ("ddr_sel_score_create_score", creates[0]),
                ("ddr_sel_score_create_difficulty", creates[1]),
                ("ddr_sel_score_create_name", creates[2]),
                ("ddr_sel_score_record_fn", decode_call_rel32(head.add(4))),
            ] {
                self.resolved.insert(name.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", name, (p as usize) - base);
            }
            for (name, v) in [
                ("ddr_sel_score_side_off", side_off),
                ("ddr_sel_score_skin_off", skin_off),
                ("ddr_sel_score_level_off", level_off),
                ("ddr_sel_score_target_off", target),
                ("ddr_sel_score_displayed_off", displayed),
                ("ddr_sel_score_difficulty_off", difficulty),
                ("ddr_sel_score_clip_off", score_clip),
                ("ddr_sel_score_difficulty_clip_off", diff_clip),
                ("ddr_sel_score_name_clip_off", name_clip),
                ("ddr_sel_score_ex_off", ex),
            ] {
                self.publish_value(name, v);
            }
        }
    }

    /// Everything [`derive_ddr_sel_score`] produced, or `None`.
    pub fn ddr_sel_score_sites(&self) -> Option<DdrSelScoreSites> {
        Some(DdrSelScoreSites {
            init: self.get_address("ddr_sel_score_init")?,
            digits: self.get_address("ddr_sel_score_digits")?,
            msg: self.get_address("ddr_sel_score_msg")?,
            create_score: self.get_address("ddr_sel_score_create_score")?,
            create_difficulty: self.get_address("ddr_sel_score_create_difficulty")?,
            create_name: self.get_address("ddr_sel_score_create_name")?,
            record_fn: self.get_address("ddr_sel_score_record_fn")?,
            side_off: self.published_value("ddr_sel_score_side_off")?,
            skin_off: self.published_value("ddr_sel_score_skin_off")?,
            level_off: self.published_value("ddr_sel_score_level_off")?,
            target_off: self.published_value("ddr_sel_score_target_off")?,
            displayed_off: self.published_value("ddr_sel_score_displayed_off")?,
            difficulty_off: self.published_value("ddr_sel_score_difficulty_off")?,
            score_clip_off: self.published_value("ddr_sel_score_clip_off")?,
            difficulty_clip_off: self.published_value("ddr_sel_score_difficulty_clip_off")?,
            name_clip_off: self.published_value("ddr_sel_score_name_clip_off")?,
            ex_off: self.published_value("ddr_sel_score_ex_off")?,
        })
    }

    /// Derive the song-info card-name / priority site in World's
    /// `SongInfoActor::onInitialize` (RTTI slot 4) — ddr_selection's skin-2
    /// legacy band (`research/legacy-score.md` §6). Exactly one match whose
    /// two LEAs name `dance_song_info_single` / `_double` and whose priority
    /// imm is 5. Identical on all five sweep builds.
    fn derive_ddr_sel_song_info(&mut self) {
        const TAG: &str = "ddr_sel_song_info";
        let Some(vt) = self.find_vtable_by_rtti(".?AVSongInfoActor@dance@sequence@@", TAG) else {
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let cstr_is = |p: *const u8, want: &[u8]| -> bool {
            (p as usize).wrapping_sub(base) + want.len() + 1 < size
                && unsafe {
                    std::slice::from_raw_parts(p, want.len()) == want && *p.add(want.len()) == 0
                }
        };
        unsafe {
            let init = *(vt as *const *const u8).add(4);
            if (init as usize).wrapping_sub(base) + 0x400 > size {
                log_warn!("  [-] {} -- init outside the module", TAG);
                return;
            }
            let hits: Vec<*const u8> = scan_pattern_all(
                init,
                0x300,
                "48 8D 05 ?? ?? ?? ?? 4C 8D 05 ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? ?? C7 44 24 20 01 00 00 00 41 B9 05 00 00 00",
            )
            .into_iter()
            .map(|m| m.address as *const u8)
            .filter(|p| {
                cstr_is(decode_rip_relative(p.add(3)), b"dance_song_info_single")
                    && cstr_is(decode_rip_relative(p.add(10)), b"dance_song_info_double")
            })
            .collect();
            let [site] = hits.as_slice() else {
                log_warn!("  [-] {} -- {} card-name sites (want 1)", TAG, hits.len());
                return;
            };
            self.resolved.insert("song_info_actor_vtable".into(), vt);
            self.resolved.insert("ddr_sel_song_info_site".into(), *site);
            log_info!(
                "  [+] ddr_sel_song_info_site (derived) @ +0x{:X}",
                (*site as usize) - base
            );
            self.derive_ddr_sel_song_info_panel(init);
        }
    }

    /// The skins 3–5 song-info panel group (optional, all-or-nothing; the
    /// skin-2 band above does not depend on it). From the SongInfoActor init
    /// (`init`): the one CALL whose target writes the SongInfoChild vtable
    /// (the child ctor) — the `MOV R9D,imm` font before it, the `TEST
    /// r8,r8; JNZ rel8` colour skip after its add-child CALL (the skipped
    /// block must store `[reg+0xA0]`); in the ctor the two `music_usr` and
    /// two `artist_usr` LEAs and the text-create helper (the CALL target that
    /// occurs three times and holds `MOV [reg+0xA8],r12d` next to `MOV
    /// [rcx+0xAC],1`, plus exactly one `CVTTSS2SI r32,xmm; SUB r32(same),r32`);
    /// in the child's update (RTTI slot 6) the same four LEAs.
    /// Byte-shape-identical on all five sweep builds.
    fn derive_ddr_sel_song_info_panel(&mut self, init: *const u8) {
        const TAG: &str = "ddr_sel_song_info_panel";
        let Some(child_vt) = self.find_vtable_by_rtti(".?AVSongInfoChild@dance@sequence@@", TAG)
        else {
            return;
        };
        let base = self.base as usize;
        let size = self.size;
        let in_mod = |p: *const u8, n: usize| (p as usize).wrapping_sub(base) + n <= size;
        let cstr_is = |p: *const u8, want: &[u8]| -> bool {
            in_mod(p, want.len() + 1)
                && unsafe {
                    std::slice::from_raw_parts(p, want.len()) == want && *p.add(want.len()) == 0
                }
        };
        // Every RIP-relative `LEA r64,[rip+disp32]` in `[start, start+len)`
        // whose target is the C string `want`.
        let leas_to = |start: *const u8, len: usize, want: &[u8]| -> Vec<*const u8> {
            let mut out = Vec::new();
            unsafe {
                for off in 0..len.saturating_sub(7) {
                    let p = start.add(off);
                    if (*p == 0x48 || *p == 0x4C)
                        && *p.add(1) == 0x8D
                        && (*p.add(2) & 0xC7) == 0x05
                        && cstr_is(decode_rip_relative(p.add(3)), want)
                    {
                        out.push(p);
                    }
                }
            }
            out
        };
        unsafe {
            if !in_mod(init, 0x400) {
                log_warn!("  [-] {} -- init outside the module", TAG);
                return;
            }
            // The child ctor call: its target writes the child vtable.
            let mut ctor_calls: Vec<(*const u8, *const u8)> = Vec::new();
            for off in 0..0x400 - 5 {
                let p = init.add(off);
                if *p != 0xE8 {
                    continue;
                }
                let t = decode_call_rel32(p);
                if !in_mod(t, 0x80) {
                    continue;
                }
                let writes_vt = (0..0x80 - 7).any(|o| {
                    let q = t.add(o);
                    (*q == 0x48 || *q == 0x4C)
                        && *q.add(1) == 0x8D
                        && (*q.add(2) & 0xC7) == 0x05
                        && decode_rip_relative(q.add(3)) == child_vt
                });
                if writes_vt {
                    ctor_calls.push((p, t));
                }
            }
            let [(call, ctor)] = ctor_calls.as_slice() else {
                log_warn!(
                    "  [-] {} -- {} child-ctor calls (want 1)",
                    TAG,
                    ctor_calls.len()
                );
                return;
            };
            let (call, ctor) = (*call, *ctor);
            // Font: the last `MOV R9D,imm32` in the 0x20 bytes before the call.
            let font = (1..0x20usize)
                .map(|b| call.sub(b))
                .find(|p| *(*p) == 0x41 && *p.add(1) == 0xB9)
                .map(|p| p.add(2));
            let Some(font_imm) = font.filter(|p| (*(*p as *const u32)) < 7) else {
                log_warn!("  [-] {} -- font MOV R9D before the child ctor call", TAG);
                return;
            };
            // Colour: the add-child CALL after the ctor call, then `TEST r8,r8;
            // JNZ rel8` over the `[reg+0xA0]` colour stores.
            let mut color_jcc = None;
            for off in 5..0x20usize {
                let p = call.add(off);
                if *p != 0xE8 {
                    continue;
                }
                let mut q = p.add(5);
                if *q & 0xF0 == 0x40 {
                    q = q.add(1);
                }
                let modrm = *q.add(1);
                if *q == 0x84
                    && modrm >> 6 == 3
                    && (modrm >> 3) & 7 == modrm & 7
                    && *q.add(2) == 0x75
                {
                    let jcc = q.add(2);
                    let skip = *jcc.add(1) as usize;
                    let block = std::slice::from_raw_parts(jcc.add(2), skip.min(0x60));
                    let stores_a0 = block.windows(8).any(|w| {
                        w[0] == 0xF3 && w[1] == 0x0F && w[2] == 0x11 && w[4..8] == [0xA0, 0, 0, 0]
                    });
                    if stores_a0 {
                        color_jcc = Some(jcc);
                    }
                }
                break;
            }
            let Some(color_jcc) = color_jcc else {
                log_warn!("  [-] {} -- colour JNZ after the child add", TAG);
                return;
            };
            if !in_mod(ctor, 0x420) {
                log_warn!("  [-] {} -- child ctor outside the module", TAG);
                return;
            }
            let upd = *(child_vt as *const *const u8).add(6);
            if !in_mod(upd, 0x300) {
                log_warn!("  [-] {} -- child update outside the module", TAG);
                return;
            }
            let mut name_leas = [std::ptr::null::<u8>(); 8];
            let groups = [
                (ctor, 0x420usize, &b"music_usr"[..], 0usize),
                (ctor, 0x420, &b"artist_usr"[..], 2),
                (upd, 0x2F0, &b"music_usr"[..], 4),
                (upd, 0x2F0, &b"artist_usr"[..], 6),
            ];
            for (start, len, want, at) in groups {
                let hits = leas_to(start, len, want);
                let [a, b] = hits.as_slice() else {
                    log_warn!(
                        "  [-] {} -- {} {} LEAs (want 2)",
                        TAG,
                        hits.len(),
                        String::from_utf8_lossy(want)
                    );
                    return;
                };
                name_leas[at] = *a;
                name_leas[at + 1] = *b;
            }
            // The text-create helper: a CALL target seen three times in the
            // ctor that holds the alignment store.
            let mut counts: Vec<(*const u8, usize)> = Vec::new();
            for off in 0..0x420 - 5 {
                let p = ctor.add(off);
                if *p != 0xE8 {
                    continue;
                }
                let t = decode_call_rel32(p);
                match counts.iter_mut().find(|(a, _)| *a == t) {
                    Some(e) => e.1 += 1,
                    None => counts.push((t, 1)),
                }
            }
            let mut helpers = Vec::new();
            for (t, n) in counts {
                if n != 3 || !in_mod(t, 0x200) {
                    continue;
                }
                let body = std::slice::from_raw_parts(t, 0x200);
                // `MOV [r64+0xA8],r12d` (REX.R only, ModRM mod=10 reg=100,
                // rm ∉ {SIB}) then `MOV DWORD [RCX+0xAC],1` within 0x20.
                let align = (0..0x80usize).find(|&o| {
                    body[o] == 0x44
                        && body[o + 1] == 0x89
                        && body[o + 2] >> 6 == 2
                        && (body[o + 2] >> 3) & 7 == 4
                        && body[o + 2] & 7 != 4
                        && body[o + 3..o + 7] == [0xA8, 0, 0, 0]
                        && body[o + 7..o + 0x27]
                            .windows(10)
                            .any(|w| w == [0xC7, 0x81, 0xAC, 0, 0, 0, 1, 0, 0, 0])
                        && body[..o].windows(3).any(|w| w == [0x45, 0x33, 0xE4])
                });
                let Some(align) = align else {
                    continue;
                };
                // `CVTTSS2SI r32,xmm` + `SUB r32(same),r32` (both non-REX).
                let subs: Vec<usize> = (0..0x200 - 6)
                    .filter(|&o| {
                        body[o] == 0xF3
                            && body[o + 1] == 0x0F
                            && body[o + 2] == 0x2C
                            && body[o + 3] >> 6 == 3
                            && body[o + 4] == 0x2B
                            && body[o + 5] >> 6 == 3
                            && (body[o + 5] >> 3) & 7 == (body[o + 3] >> 3) & 7
                    })
                    .collect();
                if let [s] = subs.as_slice() {
                    helpers.push((t, align, *s + 4));
                }
            }
            let [(helper, align, sub)] = helpers.as_slice() else {
                log_warn!("  [-] {} -- {} text helpers (want 1)", TAG, helpers.len());
                return;
            };
            let names = [
                "ddr_sel_song_info_ctor_music_lea_0",
                "ddr_sel_song_info_ctor_music_lea_1",
                "ddr_sel_song_info_ctor_artist_lea_0",
                "ddr_sel_song_info_ctor_artist_lea_1",
                "ddr_sel_song_info_update_music_lea_0",
                "ddr_sel_song_info_update_music_lea_1",
                "ddr_sel_song_info_update_artist_lea_0",
                "ddr_sel_song_info_update_artist_lea_1",
            ];
            let mut found: Vec<(&str, *const u8)> = names
                .iter()
                .copied()
                .zip(name_leas.iter().copied())
                .collect();
            found.push(("ddr_sel_song_info_child_ctor", ctor));
            found.push(("ddr_sel_song_info_font_imm", font_imm));
            found.push(("ddr_sel_song_info_color_jcc", color_jcc));
            found.push(("ddr_sel_song_info_text_helper", *helper));
            found.push(("ddr_sel_song_info_align_store", helper.add(*align)));
            found.push(("ddr_sel_song_info_x_offset_sub", helper.add(*sub)));
            for (n, p) in found {
                self.resolved.insert(n.into(), p);
                log_info!("  [+] {} (derived) @ +0x{:X}", n, p as usize - base);
            }
        }
    }

    /// Everything [`derive_ddr_sel_song_info`] produced, or `None`.
    pub fn ddr_sel_song_info_sites(&self) -> Option<DdrSelSongInfoSites> {
        Some(DdrSelSongInfoSites {
            site: self.get_address("ddr_sel_song_info_site")?,
            panel: self.ddr_sel_song_info_panel_sites(),
        })
    }

    fn ddr_sel_song_info_panel_sites(&self) -> Option<DdrSelSongInfoPanelSites> {
        let lea = |n: &str| self.get_address(n);
        Some(DdrSelSongInfoPanelSites {
            font_imm: self.get_address("ddr_sel_song_info_font_imm")?,
            color_jcc: self.get_address("ddr_sel_song_info_color_jcc")?,
            name_leas: [
                lea("ddr_sel_song_info_ctor_music_lea_0")?,
                lea("ddr_sel_song_info_ctor_music_lea_1")?,
                lea("ddr_sel_song_info_ctor_artist_lea_0")?,
                lea("ddr_sel_song_info_ctor_artist_lea_1")?,
                lea("ddr_sel_song_info_update_music_lea_0")?,
                lea("ddr_sel_song_info_update_music_lea_1")?,
                lea("ddr_sel_song_info_update_artist_lea_0")?,
                lea("ddr_sel_song_info_update_artist_lea_1")?,
            ],
            align_store: self.get_address("ddr_sel_song_info_align_store")?,
            x_offset_sub: self.get_address("ddr_sel_song_info_x_offset_sub")?,
        })
    }

    /// Everything [`derive_ddr_sel_combo`] produced, or `None`.
    pub fn ddr_sel_combo_sites(&self) -> Option<DdrSelComboSites> {
        Some(DdrSelComboSites {
            actor: self.combo_actor_sites()?,
            loop_head: self.get_address("ddr_sel_combo_loop_head")?,
            root_fmt_lea: self.get_address("ddr_sel_combo_root_fmt_lea")?,
            record_fn: self.get_address("ddr_sel_combo_record_fn")?,
            marker_fn: self.get_address("ddr_sel_combo_marker_fn")?,
            side_off: self.published_value("ddr_sel_combo_side_off")?,
            root1_off: self.published_value("ddr_sel_combo_root1_off")?,
            root3_off: self.published_value("ddr_sel_combo_root3_off")?,
            clip_set_position_vslot: self.published_value("ddr_sel_combo_set_position_vslot")?,
            clip_set_color_vslot: self.published_value("ddr_sel_combo_set_color_vslot")?,
            clip_root_mc_off: self.published_value("ddr_sel_combo_root_mc_off")?,
        })
    }

    /// The music-DB entry's raw-series vtable slot (byte offset), or `None`.
    pub fn music_series_vslot(&self) -> Option<usize> {
        self.published_value("music_series_vslot")
    }

    /// Everything [`derive_ddr_selection`] produced, or `None` unless the whole
    /// group resolved.
    pub fn ddr_selection_sites(&self) -> Option<DdrSelectionSites> {
        Some(DdrSelectionSites {
            package_helper: self.get_address("layout_package_helper")?,
            probe: self.get_address("ddr_sel_pkg_probe")?,
            record_insert: self.get_address("ddr_sel_record_insert")?,
            load_list_push: self.get_address("ddr_sel_load_list_push")?,
            bm2d_dir: self.get_address("ddr_sel_bm2d_dir")?,
            game_work_global: self.get_address("ddr_sel_game_work_global")?,
            records_shared_off: self.published_value("ddr_sel_records_shared_off")?,
            records_side_off: self.published_value("ddr_sel_records_side_off")?,
            records_side_stride: 0x48,
            load_list_off: self.published_value("ddr_sel_load_list_off")?,
            gamework_skin_off: self.published_value("gamework_skin_off")?,
        })
    }

    /// Derive `gpa_judge_effect_off` — the GamePlayActor field holding the
    /// `screen::JudgeEffectRenderer*` (`+0x150` on every supported build;
    /// below the `+0x208` layout split). Consumer: the s_marvelous receptor
    /// burst, which pushes a type-7 record through the game's own
    /// `judge_effect_push` for the S-Marvelous lanes.
    ///
    /// Derivation: every stock `CALL judge_effect_push` site loads its
    /// `this` argument with `MOV RCX,[<GamePlayActor reg>+disp32]` within
    /// the preceding 16 bytes (judgeNotes: `MOV R8D,type; MOVZX EDX,CL;
    /// MOV RCX,[R13+0x150]; CALL`; freeze tick: `MOV RCX,[R9+0x150]; MOV
    /// R8D,4; CALL`). Decode the LAST such load before each call (REX.W ∈
    /// {48,49}, opcode 8B, ModRM mod=10/reg=RCX — no SIB), require every
    /// site to agree on one plausible 8-aligned offset, and publish it.
    /// Any disagreement or an unreadable site ⇒ nothing published (the
    /// burst stays unavailable, one WARN).
    fn derive_smarvelous_burst(&mut self) {
        const TAG: &str = "gpa_judge_effect_off";
        let Some(push) = self.get_address("judge_effect_push") else {
            log_warn!("  [-] {} -- judge_effect_push unresolved", TAG);
            return;
        };
        let sites = self.xrefs_to(push);
        if sites.is_empty() {
            log_warn!("  [-] {} -- no CALL sites for judge_effect_push", TAG);
            return;
        }
        let base = self.base as usize;
        let inside = |p: usize| p.wrapping_sub(base) < self.size;

        let mut agreed: Option<usize> = None;
        for &site in &sites {
            // Window: the 16 bytes before the E8 opcode (all inside the
            // module — the call site itself is, and 16 bytes back stays
            // well within the same function body).
            let start = (site as usize).wrapping_sub(16);
            if !inside(start) {
                log_warn!("  [-] {} -- call site window outside module", TAG);
                return;
            }
            let mut found: Option<usize> = None;
            // Candidate `REX.W 8B ModRM disp32` = 7 bytes; scan every
            // position whose 7 bytes end at or before the CALL.
            for off in 0..=9usize {
                let p = unsafe { (start as *const u8).add(off) };
                let rex = unsafe { *p };
                let op = unsafe { *p.add(1) };
                let modrm = unsafe { *p.add(2) };
                if (rex == 0x48 || rex == 0x49)
                    && op == 0x8B
                    && (modrm & 0xC0) == 0x80
                    && (modrm & 0x38) == 0x08
                    && (modrm & 0x07) != 0x04
                {
                    let disp = unsafe { std::ptr::read_unaligned(p.add(3) as *const i32) };
                    if disp > 0 {
                        found = Some(disp as usize); // keep the LAST (closest) match
                    }
                }
            }
            let Some(off) = found else {
                log_warn!(
                    "  [-] {} -- no `MOV RCX,[reg+disp32]` before CALL @ +0x{:X}",
                    TAG,
                    (site as usize).wrapping_sub(base)
                );
                return;
            };
            match agreed {
                None => agreed = Some(off),
                Some(prev) if prev == off => {}
                Some(prev) => {
                    log_warn!(
                        "  [-] {} -- call sites disagree (0x{:X} vs 0x{:X})",
                        TAG,
                        prev,
                        off
                    );
                    return;
                }
            }
        }
        let Some(off) = agreed else {
            return;
        };
        if !(0x40..0x400).contains(&off) || off % 8 != 0 {
            log_warn!("  [-] {} -- implausible offset 0x{:X}", TAG, off);
            return;
        }
        // `publish_value` logs the `(derived) = 0x…` line.
        self.publish_value(TAG, off);
    }

    /// Published GamePlayActor `JudgeEffectRenderer*` field offset (see
    /// `derive_smarvelous_burst`), or `None`.
    pub fn gpa_judge_effect_off(&self) -> Option<usize> {
        self.published_value("gpa_judge_effect_off")
    }

    /// Derive `gpa_ghost_actor_off` — the GamePlayActor field holding the
    /// `sequence::dance::GhostActor*` — from the state-2 wait site
    /// (`gpa_ghost_actor_probe`: `MOV RCX,[RDI+disp32]; TEST; JZ; CALL
    /// isReady; TEST AL,AL; JZ`). `+0x1F8` on 20260324+, `+0x1F0` on
    /// 20250805 / 20260224 — the GamePlayActor layout fork sits at this
    /// field, so it is never hardcoded. Consumer: multiplayer_bot's
    /// `ghost_source` (Target Score replays the human's ghost vector).
    ///
    /// Identity gate: the CALL at match+12 must land on the `isReady` body,
    /// whose prologue is byte-identical on every attested build and pins the
    /// GhostActor state layout the consumer reads:
    ///
    ///   0F B7 81 82 00 00 00   MOVZX EAX,[RCX+0x82]           ; state idx
    ///   48 8B D9               MOV   RBX,RCX
    ///   83 7C C1 58 02         CMP   dword [RCX+RAX*8+0x58],2  ; ready == 2
    ///   74                     JZ
    ///
    /// The run must appear within the callee's first 0x30 bytes. Anything
    /// else ⇒ nothing published (the Target tier falls back per song with
    /// one WARN).
    fn derive_ghost_actor_probe(&mut self) {
        const TAG: &str = "gpa_ghost_actor_off";
        const IS_READY_PROLOGUE: [u8; 16] = [
            0x0F, 0xB7, 0x81, 0x82, 0x00, 0x00, 0x00, 0x48, 0x8B, 0xD9, 0x83, 0x7C, 0xC1, 0x58,
            0x02, 0x74,
        ];
        const CALLEE_WINDOW: usize = 0x30;
        let Some(probe) = self.get_address("gpa_ghost_actor_probe") else {
            log_warn!("  [-] {} -- gpa_ghost_actor_probe unresolved", TAG);
            return;
        };
        let base = self.base as usize;
        let inside = |p: usize, len: usize| {
            p.wrapping_sub(base) < self.size
                && p.wrapping_sub(base).saturating_add(len) <= self.size
        };
        let probe_addr = probe as usize;
        if !inside(probe_addr, 21) {
            log_warn!("  [-] {} -- probe window outside module", TAG);
            return;
        }
        let disp = unsafe { std::ptr::read_unaligned(probe.add(3) as *const i32) };
        if !(0x100..0x400).contains(&disp) || disp % 8 != 0 {
            log_warn!(
                "  [-] {} -- implausible GhostActor field disp 0x{:X}",
                TAG,
                disp
            );
            return;
        }
        let callee = unsafe { decode_call_rel32(probe.add(12)) } as usize;
        if !inside(callee, CALLEE_WINDOW) {
            log_warn!(
                "  [-] {} -- isReady callee +0x{:X} outside module",
                TAG,
                callee.wrapping_sub(base)
            );
            return;
        }
        let body = unsafe { std::slice::from_raw_parts(callee as *const u8, CALLEE_WINDOW) };
        if !body
            .windows(IS_READY_PROLOGUE.len())
            .any(|w| w == IS_READY_PROLOGUE)
        {
            log_warn!(
                "  [-] {} -- CALL target +0x{:X} is not GhostActor::isReady (state-layout prologue absent)",
                TAG,
                callee.wrapping_sub(base)
            );
            return;
        }
        // `publish_value` logs the `(derived) = 0x…` line.
        self.publish_value(TAG, disp as usize);
    }

    /// Published GamePlayActor `GhostActor*` field offset (see
    /// `derive_ghost_actor_probe`), or `None`.
    pub fn gpa_ghost_actor_off(&self) -> Option<usize> {
        self.published_value("gpa_ghost_actor_off")
    }

    fn derive_song_rate_runtime_sites(&mut self) {
        for (name, pattern) in [
            (
                "song_rate_wavebank_create",
                SONG_RATE_WAVEBANK_CREATE_PATTERN,
            ),
            (
                "song_rate_wavebank_unregister",
                SONG_RATE_WAVEBANK_UNREGISTER_PATTERN,
            ),
        ] {
            let matches = scan_pattern_all(self.base, self.size, pattern);
            if matches.len() != 1 {
                self.resolved.remove(name);
                log_warn!(
                    "  [-] {} -- expected exactly one match, found {}",
                    name,
                    matches.len()
                );
                continue;
            }
            self.resolved.insert(name.into(), matches[0].address);
        }

        // The clock anchor has two codegen shapes (with / without the
        // accessor's XOR EDX,EDX); exactly one of them must match exactly
        // once. The shape decides where the redirect window sits.
        self.resolved.remove("song_rate_clock_anchor");
        let primary = scan_pattern_all(self.base, self.size, SONG_RATE_CLOCK_ANCHOR_PATTERN);
        let v1 = scan_pattern_all(self.base, self.size, SONG_RATE_CLOCK_ANCHOR_V1_PATTERN);
        let (anchor, patch_offset) = match (primary.as_slice(), v1.as_slice()) {
            ([m], []) => (m.address, SONG_RATE_CLOCK_PATCH_OFFSET),
            ([], [m]) => {
                log_info!("  [+] song_rate_clock_anchor -- pre-20260324 shape (v1)");
                (m.address, SONG_RATE_CLOCK_PATCH_OFFSET_V1)
            }
            _ => {
                log_warn!(
                    "  [-] song_rate_clock_anchor -- expected exactly one match, found {} (v1: {})",
                    primary.len(),
                    v1.len()
                );
                return;
            }
        };
        self.resolved
            .insert("song_rate_clock_anchor".into(), anchor);

        let anchor_offset = anchor as usize - self.base as usize;
        let Some(patch_end) = anchor_offset
            .checked_add(patch_offset)
            .and_then(|offset| offset.checked_add(SONG_RATE_CLOCK_EXPECTED.len()))
        else {
            self.resolved.remove("song_rate_clock_anchor");
            log_warn!("  [-] song_rate_clock_patch -- derived range overflow");
            return;
        };
        if patch_end > self.size {
            self.resolved.remove("song_rate_clock_anchor");
            log_warn!("  [-] song_rate_clock_patch -- derived range outside module");
            return;
        }
        let patch = unsafe { anchor.add(patch_offset) };
        let actual = unsafe { std::slice::from_raw_parts(patch, SONG_RATE_CLOCK_EXPECTED.len()) };
        if actual != SONG_RATE_CLOCK_EXPECTED {
            self.resolved.remove("song_rate_clock_anchor");
            log_warn!("  [-] song_rate_clock_patch -- redirect bytes changed");
            return;
        }
        self.resolved.insert("song_rate_clock_patch".into(), patch);
        log_info!(
            "  [+] song_rate_clock_patch @ +0x{:X}",
            patch as usize - self.base as usize
        );
    }

    /// Derive the streaming rate engine's IO-callback addresses from the
    /// `song_rate_io_callback_regsite` match (design req 9; evidence chain
    /// and cross-version table in `docs/xact_streaming_research.md` §2/§6):
    ///
    /// - `song_rate_readfile_callback` / `song_rate_overlapped_callback` —
    ///   RIP-decoded from the match's second and third LEAs (the detour
    ///   pair; both or neither, they are only ever installed together).
    /// - `song_rate_handle_lookup` — the stock handle→file_id lookup helper
    ///   (fastcall: HANDLE in RCX, returns file_id in EAX, -1 on miss; takes
    ///   the AVS mutex itself), decoded from the readFile body's first CALL
    ///   at entry+0x21 behind a 34-byte literal-prologue validation. The
    ///   read detour calls it to replicate the stock locked sorted-vector
    ///   walk exactly (design req 11).
    /// - `song_rate_file_table` — the audio file-table global (data rows at
    ///   `[*global+0x8] + file_id*0x40`: buffer ptr +0x8, size u32 +0x14;
    ///   path rows at `[*global+0x28] + file_id*0xA0 + 0x11`), RIP-decoded
    ///   from the already-validated `song_rate_wavebank_unregister` match
    ///   (its literal bytes pin the access shape). Sources the binding's
    ///   `SourceView` (the FileManager RAM copy) and the dance-path check.
    ///
    /// Fail-closed derivation, fail-open feature: any validation failure
    /// removes EVERYTHING this function may publish (plus the regsite name
    /// itself) with one WARN; none of the names is in the required set, so
    /// absence just leaves the streaming integration structurally off and
    /// the DLL booting stock (design req 40).
    ///
    /// MUST run after `derive_song_rate_runtime_sites` — it consumes the
    /// uniqueness-revalidated unregister match published there.
    fn derive_song_rate_io_callbacks(&mut self) {
        const PUBLISHED: [&str; 5] = [
            "song_rate_io_callback_regsite",
            "song_rate_readfile_callback",
            "song_rate_overlapped_callback",
            "song_rate_handle_lookup",
            "song_rate_file_table",
        ];
        macro_rules! fail {
            ($($arg:tt)*) => {{
                for name in PUBLISHED {
                    self.resolved.remove(name);
                }
                log_warn!($($arg)*);
                return;
            }};
        }

        let module_start = self.base as usize;
        let module_end = module_start + self.size;
        let in_module = |p: *const u8| {
            let a = p as usize;
            a >= module_start && a < module_end
        };

        let matches = scan_pattern_all(self.base, self.size, SONG_RATE_IO_CALLBACK_REGSITE_PATTERN);
        if matches.len() != 1 {
            fail!(
                "  [-] song_rate_io_callback_regsite -- expected exactly one match, found {}",
                matches.len()
            );
        }
        let regsite = matches[0].address;

        unsafe {
            let readfile = decode_rip_relative(regsite.add(SONG_RATE_IO_READFILE_LEA_DISP));
            let overlapped = decode_rip_relative(regsite.add(SONG_RATE_IO_OVERLAPPED_LEA_DISP));
            if !in_module(readfile) || !in_module(overlapped) {
                fail!("  [-] song_rate_io_callbacks -- decoded callback outside module");
            }

            // Validate the readFile prologue (ends at the E8 opcode of the
            // handle-lookup CALL) before trusting the rel32 decode.
            let prefix = std::slice::from_raw_parts(readfile, SONG_RATE_IO_READFILE_PREFIX.len());
            if prefix != SONG_RATE_IO_READFILE_PREFIX {
                fail!("  [-] song_rate_handle_lookup -- readFile prologue bytes changed");
            }
            let handle_lookup = decode_call_rel32(readfile.add(SONG_RATE_IO_READFILE_CALL_OFFSET));
            if !in_module(handle_lookup) {
                fail!("  [-] song_rate_handle_lookup -- decoded target outside module");
            }

            // File-table global from the unregister match (uniqueness already
            // revalidated by derive_song_rate_runtime_sites).
            let Some(unregister) = self.get_address("song_rate_wavebank_unregister") else {
                fail!("  [-] song_rate_file_table -- wavebank_unregister unresolved");
            };
            let mov = std::slice::from_raw_parts(
                unregister.add(SONG_RATE_IO_FILE_TABLE_MOV_OFFSET),
                SONG_RATE_IO_FILE_TABLE_MOV_OPCODE.len(),
            );
            if mov != SONG_RATE_IO_FILE_TABLE_MOV_OPCODE {
                fail!("  [-] song_rate_file_table -- global-load opcode changed");
            }
            let file_table = decode_rip_relative(unregister.add(SONG_RATE_IO_FILE_TABLE_DISP));
            if !in_module(file_table) {
                fail!("  [-] song_rate_file_table -- decoded global outside module");
            }

            for (name, addr) in [
                ("song_rate_readfile_callback", readfile),
                ("song_rate_overlapped_callback", overlapped),
                ("song_rate_handle_lookup", handle_lookup),
                ("song_rate_file_table", file_table),
            ] {
                self.resolved.insert(name.into(), addr);
                log_info!(
                    "  [+] {} (derived) @ +0x{:X}",
                    name,
                    addr as usize - module_start
                );
            }
        }
    }

    /// Song-select preview restart derivations (preview design §Components
    /// 6, byte authority: `.agents/planning/2026-08-15-song-preview-rate/
    /// research/preview-retrigger-re.md` §9): re-validate the four preview
    /// patterns' uniqueness, then RIP-decode the two vftable identity gates
    /// (`selectmusic_view_vftable`, `audio_loader_vftable`) from their ctor
    /// matches. The two function signatures (`cue_handle_stop`,
    /// `sound_bank_create_router`) ARE their yields — match = entry — and
    /// need no decode here.
    ///
    /// Fail-closed per piece: the restart executor pokes live game objects
    /// through these addresses, so a non-unique pattern or an out-of-module
    /// decode is refused loudly (one WARN naming the piece, nothing
    /// published) rather than half-trusted. A refused piece disables only
    /// the preview feature's restart half (`preview::init_restart` is
    /// all-or-nothing) — wheel-settle preview binds and the gameplay rate
    /// feature run untouched.
    fn derive_preview_restart(&mut self) {
        /// Offset of the `LEA RAX,[rip+AudioLoader::vftable]` displacement
        /// within the `audio_loader_ctor` match.
        const LOADER_VFT_DISP: usize = 3;
        /// Offset of the `LEA R11,[rip+View::vftable]` displacement within
        /// the `selectmusic_view_ctor` match — the SECOND LEA (`4C 8D 1D`,
        /// stored bare to `[RBX]`): the first LEA (disp at match+23) is an
        /// inner interface vftable stored at `+0x28`, not the View's own.
        const VIEW_VFT_DISP: usize = 30;

        // Uniqueness re-validation (the audio-family style): `resolve_all`
        // has first-match semantics, so a second match anywhere in the
        // module would silently poke the wrong object at restart time.
        let mut unique = true;
        for name in [
            "audio_loader_ctor",
            "selectmusic_view_ctor",
            "cue_handle_stop",
            "sound_bank_create_router",
        ] {
            let count = self.get_all_matches(name).len();
            if count > 1 {
                log_warn!(
                    "  [!] {} matched {} times -- not unique on this build; preview restart derivations refused (verify against preview-retrigger-re.md §9)",
                    name,
                    count
                );
                unique = false;
            }
        }
        if !unique {
            return;
        }

        let module_start = self.base as usize;
        let in_module =
            |p: *const u8| (p as usize) >= module_start && (p as usize) < module_start + self.size;

        // (anchor signature, derived name, disp offset, check slot 0)
        // Both vftables must decode in-module; slot 0 must additionally be
        // an in-module function pointer (the AudioLoader's single virtual
        // slot is the per-frame tick, the View's slot 0 its first virtual)
        // — the same cheap "is this really a vftable" corroboration both
        // identity gates rest on at restart time.
        for (anchor_name, derived_name, disp) in [
            ("audio_loader_ctor", "audio_loader_vftable", LOADER_VFT_DISP),
            (
                "selectmusic_view_ctor",
                "selectmusic_view_vftable",
                VIEW_VFT_DISP,
            ),
        ] {
            let Some(anchor) = self.get_address(anchor_name) else {
                log_warn!(
                    "  [-] {} -- {} anchor unresolved",
                    derived_name,
                    anchor_name
                );
                continue;
            };
            unsafe {
                let vftable = decode_rip_relative(anchor.add(disp));
                if !in_module(vftable) {
                    log_warn!(
                        "  [-] {} -- decoded vftable {:p} outside the module; refusing",
                        derived_name,
                        vftable
                    );
                    continue;
                }
                let slot0 = (vftable as *const *const u8).read_unaligned();
                if !in_module(slot0) {
                    log_warn!(
                        "  [-] {} -- vftable slot 0 {:p} outside the module (not a vftable); refusing",
                        derived_name,
                        slot0
                    );
                    continue;
                }
                self.resolved.insert(derived_name.into(), vftable);
                log_info!(
                    "  [+] {} (derived, ctor LEA) @ +0x{:X}",
                    derived_name,
                    vftable as usize - module_start
                );
            }
        }

        self.derive_selectmusic_view_child_offset();
    }

    /// Derive `selectmusic_view_child_offset` — the byte offset of the
    /// `sequence::selectmusic::View*` field on the SelectMusicSequence (the
    /// active scene child at `TS+0x58` during song select). This is the ONE
    /// parent-struct read in the preview loader-chain walk that no signature
    /// pins, and it is BUILD-DEPENDENT: `+0x90` on 20250805 / 20260224 /
    /// 20260324, `+0xB8` on 20260421+ (the whole `+0x88/+0x90/+0x98` sibling
    /// cluster moved to `+0xB0/+0xB8/+0xC0`). The hardcoded `0xB8` read a
    /// non-pointer on 20260224 and the walk faulted BEFORE its vftable
    /// identity gate could compare anything (crash 2026-09-03).
    ///
    /// Source: the View ctor's single caller — `SelectMusicSequence`'s
    /// setup — stores the ctor result immediately after the CALL:
    ///
    /// ```text
    /// E8 rel32              CALL View::ctor
    /// EB 03                 JMP +3
    /// 48 8B C6              MOV RAX,RSI          (null-alloc arm)
    /// 48 89 87 disp32       MOV [RDI+disp32],RAX  <- the field offset
    /// ```
    ///
    /// Same shape on all six builds inspected (20250805 … 20260825).
    /// Fail-closed: exactly one call site, exactly one `MOV [reg+disp32],RAX`
    /// in the 16 bytes after it, disp 8-aligned and within a small object
    /// range — otherwise nothing is published and `preview::init_restart`
    /// leaves the live-edit restart half disarmed (wheel-settle previews and
    /// the gameplay rate are unaffected).
    fn derive_selectmusic_view_child_offset(&mut self) {
        const NAME: &str = "selectmusic_view_child_offset";
        /// Bytes after the CALL opcode to search for the store.
        const STORE_WINDOW: usize = 16;
        /// Plausible field range on the SelectMusicSequence (the known
        /// values are 0x90 and 0xB8; the object is a few hundred bytes).
        const MIN_DISP: u32 = 0x40;
        const MAX_DISP: u32 = 0x200;

        let Some(ctor) = self.get_address("selectmusic_view_ctor") else {
            log_warn!("  [-] {} -- selectmusic_view_ctor unresolved", NAME);
            return;
        };
        let sites = self.xrefs_to(ctor);
        if sites.len() != 1 {
            log_warn!(
                "  [-] {} -- expected exactly 1 CALL site for the View ctor, found {}; refusing",
                NAME,
                sites.len()
            );
            return;
        }
        let call = sites[0];

        // `48 89 /r` with mod=10 (disp32) and reg=RAX (000): ModRM 0x80..0x87
        // minus 0x84 (SIB form). Any base register is accepted — the
        // sequence pointer's register is a compiler choice.
        let mut found: Option<u32> = None;
        let mut hits = 0usize;
        unsafe {
            let start = call.add(5);
            for i in 0..STORE_WINDOW {
                let p = start.add(i);
                let modrm = *p.add(2);
                if *p == 0x48
                    && *p.add(1) == 0x89
                    && (0x80..=0x87).contains(&modrm)
                    && modrm != 0x84
                {
                    let disp = (p.add(3) as *const u32).read_unaligned();
                    found = Some(disp);
                    hits += 1;
                }
            }
        }
        match (found, hits) {
            (Some(disp), 1) if disp % 8 == 0 && (MIN_DISP..MAX_DISP).contains(&disp) => {
                self.publish_value(NAME, disp as usize);
            }
            (Some(disp), 1) => log_warn!(
                "  [-] {} -- store disp 0x{:X} out of the plausible range; refusing",
                NAME,
                disp
            ),
            (_, n) => log_warn!(
                "  [-] {} -- `MOV [reg+disp32],RAX` not uniquely found after the ctor CALL ({} hit(s)); refusing",
                NAME,
                n
            ),
        }
    }

    /// Byte offset of the `View*` field on the SelectMusicSequence (see
    /// `derive_selectmusic_view_child_offset`), or `None` — the preview
    /// live-edit restart half must then stay disarmed.
    pub fn selectmusic_view_child_offset(&self) -> Option<usize> {
        self.published_value("selectmusic_view_child_offset")
    }

    /// Derive the BM2D package registry global and name-lookup helper from
    /// the `bm2d_data_is_ready` anchor (see the signature's comment). The
    /// anchor's body is exactly:
    ///
    /// ```text
    /// +0   PUSH RBX; SUB RSP,0x20
    /// +6   MOV RAX, [rip+disp32]      ; disp32 at +9 -> registry global
    /// +13  MOV R8, RCX
    /// +16  MOV RBX, [RAX+8]           ; end
    /// +20  MOV RCX, [RAX]             ; begin
    /// +23  MOV RDX, RBX
    /// +26  CALL lookup                ; E8 rel32 -> bm2d_package_lookup
    /// ```
    ///
    /// `bm2d_package_registry` is the address of the global *pointer* to the
    /// heap-allocated registry object ([0]=begin, [8]=end) — dereference at
    /// use time (it is created lazily during boot).
    fn derive_bm2d_package_addresses(&mut self) {
        let anchor = match self.get_address("bm2d_data_is_ready") {
            Some(a) => a,
            None => {
                log_warn!("  [-] bm2d_package_registry/lookup -- anchor unresolved");
                return;
            }
        };
        unsafe {
            let registry = decode_rip_relative(anchor.add(9));
            self.resolved
                .insert("bm2d_package_registry".into(), registry);
            log_info!(
                "  [+] bm2d_package_registry (derived) @ +0x{:X}",
                registry.offset_from(self.base) as usize
            );

            let call_site = anchor.add(26);
            if *call_site != 0xE8 {
                log_warn!(
                    "  [-] bm2d_package_lookup -- expected E8 at anchor+26, got 0x{:02X}",
                    *call_site
                );
                return;
            }
            let lookup = decode_call_rel32(call_site);
            self.resolved.insert("bm2d_package_lookup".into(), lookup);
            log_info!(
                "  [+] bm2d_package_lookup (derived) @ +0x{:X}",
                lookup.offset_from(self.base) as usize
            );
        }
    }

    /// Resolve `cmovieclip_create` (CMovieClip::Create @ FUN_180257770) with a
    /// standalone single-pattern scan.
    ///
    /// It CANNOT go in the batch `SIGNATURES` array: its 20-byte prologue
    /// literal run shares its first 18 bytes with `afp_layer_init_wrapper`
    /// (the same function's shorter, pre-existing signature). The batch
    /// scanner builds one Aho-Corasick automaton and iterates NON-overlapping
    /// matches, so at 0x…257770 the shorter afp needle is consumed first and
    /// this longer needle is never reported. A standalone scan uses a
    /// single-needle automaton (no cross-pattern collision) and still verifies
    /// the full pattern — including the `MOV [RCX+0x23C],EAX` /
    /// `MOV EDX,[RDX+0x314]` anchors that make it specific to Create.
    ///
    /// `Create(this, package*, name: *const c_char /*R8*/, priority: i32,
    /// mode: i32)`. Unique full-pattern match on builds 20260616
    /// (0x180257770) and 20260324 (0x18021B6A0) — Ghidra-verified 2026-07-12.
    fn derive_cmovieclip_create(&mut self) {
        const PATTERN: &str = "48 89 5C 24 10 56 48 83 EC 40 41 8B F1 48 8B D9 48 85 D2 0F 84 ? ? ? ? 4D 85 C0 0F 84 ? ? ? ? 83 79 08 00 0F 85 ? ? ? ? 8B 02 89 81 3C 02 00 00 8B 92 14 03 00 00";
        match scan_pattern(self.base, self.size, PATTERN) {
            Some(r) => {
                self.resolved.insert("cmovieclip_create".into(), r.address);
                log_info!("  [+] cmovieclip_create (standalone) @ +0x{:X}", r.offset);
            }
            None => {
                log_warn!("  [-] cmovieclip_create -- standalone pattern not found");
            }
        }
    }

    /// Resolve the CMovieClip wrapper SetColor detour targets for the
    /// overlay-element-styling mod
    /// (`docs/gameplay_overlay_elements_research.md` §6.2/§6.3).
    ///
    /// Each pattern matches EXACTLY two byte-identical function bodies: the
    /// multiplicative set_color form and its additive set_acolor twin — and
    /// the twin ORDER FLIPS between supported builds (20260616 vs 20260324),
    /// so "first match" would silently pick the wrong one on one build. The
    /// only reliable discriminator is each body's `CALL [RIP+disp32]` IAT
    /// slot: decode it, read the loader-patched function pointer, and compare
    /// against the libafp exports resolved by name (libafp is a static import
    /// of gamemdx, so it is guaranteed loaded — and its IAT slots patched —
    /// by the time `resolve_derived` runs). Publishes:
    ///
    ///   - `cmovieclip_set_color_float` — vtable +0x90,
    ///     `fn(this, a: f32, r: f32, g: f32, b: f32)` — **alpha is the FIRST
    ///     float arg** (forwarded as `afp_layer_set_color(id, r, g, b, a)`).
    ///   - `cmovieclip_set_color_int` — vtable +0xB0,
    ///     `fn(this, a_pct: i32, r: f32, g: f32, b: f32)` (alpha percent,
    ///     divided by 100.0 before forwarding).
    ///
    /// ANY ambiguity (≠2 matches, missing exports, unexpected opcode, an IAT
    /// target matching neither export, or both matches resolving to the same
    /// export) leaves the name unresolved: misidentifying set_color vs
    /// set_acolor would silently write the wrong color-transform channel.
    fn derive_cmovieclip_color_twins(&mut self) {
        // (published name, pattern, `FF 15` opcode offset, disp32 offset).
        // Offsets Ghidra-verified on both builds 2026-07-12.
        const TWINS: &[(&str, &str, usize, usize)] = &[
            (
                "cmovieclip_set_color_float",
                "48 83 EC 38 8B 49 08 0F 28 C3 F3 0F 10 5C 24 60 0F 28 E2 F3 0F 11 4C 24 20 0F 28 D0 0F 28 CC FF 15 ? ? ? ? 48 83 C4 38 C3",
                0x1F,
                0x21,
            ),
            (
                "cmovieclip_set_color_int",
                "48 83 EC 38 8B 49 08 0F 28 CB F3 0F 10 5C 24 60 0F 28 E2 66 0F 6E C2 0F 28 D1 0F 5B C0 0F 28 CC F3 0F 5E 05 ? ? ? ? F3 0F 11 44 24 20 FF 15",
                0x2E,
                0x30,
            ),
        ];

        let set_color = resolve_libafp_export("afp_layer_set_color");
        let set_acolor = resolve_libafp_export("afp_layer_set_acolor");
        let (set_color, set_acolor) = match (set_color, set_acolor) {
            (Some(c), Some(a)) => (c, a),
            _ => {
                log_warn!(
                    "  [-] cmovieclip_set_color_* -- libafp set_color/set_acolor exports unavailable"
                );
                return;
            }
        };

        for &(name, pattern, opcode_off, disp_off) in TWINS {
            let matches = scan_pattern_all(self.base, self.size, pattern);
            if matches.len() != 2 {
                log_warn!(
                    "  [-] {} -- expected exactly 2 twin matches, got {}",
                    name,
                    matches.len()
                );
                continue;
            }

            let mut color_match: Option<*const u8> = None;
            let mut acolor_match: Option<*const u8> = None;
            let mut ambiguous = false;

            for m in &matches {
                let addr = m.address;
                unsafe {
                    if *addr.add(opcode_off) != 0xFF || *addr.add(opcode_off + 1) != 0x15 {
                        log_warn!(
                            "  [-] {} -- expected FF 15 at match+0x{:X}, got {:02X} {:02X}",
                            name,
                            opcode_off,
                            *addr.add(opcode_off),
                            *addr.add(opcode_off + 1)
                        );
                        ambiguous = true;
                        break;
                    }
                    let iat_slot = decode_rip_relative(addr.add(disp_off));
                    let target = (iat_slot as *const *const u8).read_unaligned();
                    if target == set_color {
                        ambiguous |= color_match.replace(addr).is_some();
                    } else if target == set_acolor {
                        ambiguous |= acolor_match.replace(addr).is_some();
                    } else {
                        log_warn!(
                            "  [-] {} -- match +0x{:X}: IAT target {:p} is neither set_color nor set_acolor",
                            name,
                            addr.offset_from(self.base) as usize,
                            target
                        );
                        ambiguous = true;
                    }
                }
            }

            match (ambiguous, color_match, acolor_match) {
                (false, Some(color), Some(acolor)) => {
                    self.resolved.insert(name.into(), color);
                    unsafe {
                        log_info!(
                            "  [+] {} (twin-disambiguated) @ +0x{:X} (acolor sibling @ +0x{:X})",
                            name,
                            color.offset_from(self.base) as usize,
                            acolor.offset_from(self.base) as usize
                        );
                    }
                }
                _ => {
                    log_warn!(
                        "  [-] {} -- twin disambiguation failed (color={}, acolor={}) -- unresolved",
                        name,
                        color_match.is_some(),
                        acolor_match.is_some()
                    );
                }
            }
        }
    }

    /// Derive `timing_config_set_int` — the config-map int setter the timing-
    /// init publisher calls to publish SOUND/INPUT/RENDER/BOMB_FRAME offsets.
    ///
    /// The setter cannot be resolved by its own prologue: it shares a
    /// byte-identical prologue with a sibling FNV-map int setter for a
    /// different config map (they differ only in the RIP-relative map global
    /// loaded near the tail). Instead we anchor on the publisher's first
    /// config-set pair (`timing_set_call_landmark`, whose first match is the
    /// SOUND_OFFSET pair) and decode the `CALL rel32` at landmark+0xA — the
    /// int-setter the game calls to set "SOUND_OFFSET" is, by definition, the
    /// timing setter.
    fn derive_timing_config_setter(&mut self) {
        let landmark = match self.get_address("timing_set_call_landmark") {
            Some(a) => a,
            None => {
                log_warn!("  [-] timing_config_set_int -- landmark unresolved");
                return;
            }
        };
        unsafe {
            // Pair layout: MOV EDX,[RBP+d] (3) + LEA RCX,[rip+disp] (7) = 10
            // bytes, then the CALL. Verify the opcode before decoding.
            let call_site = landmark.add(0x0A);
            if *call_site != 0xE8 {
                log_warn!(
                    "  [-] timing_config_set_int -- expected E8 at landmark+0xA, got 0x{:02X}",
                    *call_site
                );
                return;
            }
            let setter = decode_call_rel32(call_site);
            self.resolved.insert("timing_config_set_int".into(), setter);
            let offset = setter.offset_from(self.base) as usize;
            log_info!("  [+] timing_config_set_int (derived) @ +0x{:X}", offset);
        }
        self.derive_timing_config_map_global();
    }

    /// Derive `timing_config_map_global` — the address of the process-global
    /// pointer to the config-map root that the int setter dereferences (the
    /// `DAT_1806ebcf0`/`DAT_1806f1d70` analog). The boot publisher null-guards
    /// on `*global != 0` before publishing the offsets, so the timing-offsets
    /// mod observes this same pointer to know when the map is live (rather than
    /// only latching off the first hook hit — lets the boot-seed fallback fire
    /// even if hook-install ordering is ever violated).
    ///
    /// Derivation: the setter loads the map root via the first
    /// `MOV RDX, qword ptr [rip+disp32]` (`48 8B 15` ..) in its body — verified
    /// the first such instruction on both supported builds (it sits just past
    /// the inlined FNV-1a key-hash loop). Scan the setter prologue for that
    /// opcode and RIP-decode its displacement to the global's address.
    fn derive_timing_config_map_global(&mut self) {
        let setter = match self.get_address("timing_config_set_int") {
            Some(a) => a,
            None => return, // setter unresolved → nothing to derive from
        };
        unsafe {
            // Scan a generous window covering the prologue + FNV loop; the
            // `MOV RDX,[rip+disp]` map-root load is ~0x46 in on both builds. The
            // instruction is 7 bytes (`48 8B 15` + disp32), so stop 6 bytes shy
            // of the window end to keep the whole candidate inside the window.
            const WINDOW: usize = 0x80;
            for i in 0..(WINDOW - 6) {
                let p = setter.add(i);
                if *p == 0x48 && *p.add(1) == 0x8B && *p.add(2) == 0x15 {
                    let global = decode_rip_relative(p.add(3));
                    self.resolved
                        .insert("timing_config_map_global".into(), global);
                    let offset = global.offset_from(self.base) as usize;
                    log_info!("  [+] timing_config_map_global (derived) @ +0x{:X}", offset);
                    return;
                }
            }
            log_warn!(
                "  [-] timing_config_map_global -- MOV RDX,[rip] not found in setter prologue"
            );
        }
    }

    /// Derive the playfield-styling mod's target set from already-resolved
    /// signatures (`render_notes`, `get_offset_y`) + RTTI. Publishes, on
    /// success:
    ///
    ///   - `note_collector` — the per-pass note collector (called from
    ///     `render_notes`; iterates the judge Results vector).
    ///   - `collector_cull_site` — the `MOVSS XMM15,[RIP+disp32]`
    ///     (`F3 44 0F 10 3D`) instruction inside the collector that loads the
    ///     720.0f top-cull bound. Patch target (disp32 redirect).
    ///   - `guideline_draw` — the measure-guideline draw function.
    ///   - `guideline_cull_site` — the `MOVSS XMM9,[RIP+disp32]`
    ///     (`F3 44 0F 10 0D`) 720.0f load inside the guideline draw.
    ///   - `guideline_bulk_emitter` — the guideline's private bulk sprite
    ///     emitter (writes a tag-0x01 DRAWSPRITES command, 0x14-byte record
    ///     stride; exactly ONE caller module-wide).
    ///   - `arrow_renderer_vtable` / `spot_renderer_vtable` /
    ///     `judge_effect_renderer_vtable` — offset-0 vftables via RTTI walk,
    ///     used by the fill hook to classify renderer instances.
    ///
    /// Every step verifies instruction bytes and content before publishing;
    /// ANY ambiguity (no match, multiple matches, wrong constant value)
    /// leaves the name unresolved so the mod's all-or-nothing gate fails
    /// closed (never patch unverified bytes).
    ///
    /// NOTE (verified in Ghidra on builds 20260616 + 20260324): the naive
    /// "first CALL rel32 in render_notes" heuristic is WRONG for the
    /// collector — stray 0xE8 bytes occur earlier as MOV displacement bytes,
    /// and the true first CALL targets a per-pass helper, not the collector.
    /// The collector is instead identified by content: it is the unique
    /// render_notes callee whose body contains the XMM15-form 720.0f load.
    fn derive_playfield_styling(&mut self) {
        self.derive_note_collector();
        self.derive_guideline_targets();

        // Renderer vtables (offset-0 vftables; each class has exactly one
        // COL with offset 0 and one vtable meta-pointer — Ghidra-verified).
        const RENDERER_VTABLES: &[(&str, &str)] = &[
            (".?AVArrowRenderer@screen@@", "arrow_renderer_vtable"),
            (".?AVSpotRenderer@screen@@", "spot_renderer_vtable"),
            (
                ".?AVJudgeEffectRenderer@screen@@",
                "judge_effect_renderer_vtable",
            ),
        ];
        for &(rtti, name) in RENDERER_VTABLES {
            if let Some(vt) = self.find_vtable_by_rtti(rtti, name) {
                self.resolved.insert(name.into(), vt);
                log_info!(
                    "  [+] {} (RTTI) @ +0x{:X}",
                    name,
                    unsafe { vt.offset_from(self.base) } as usize
                );
            }
            // find_vtable_by_rtti logs its own [-] on failure.
        }
    }

    /// Scan a byte window for `prefix` followed by a RIP-relative disp32
    /// whose target reads `expect` (f32). Returns the matching instruction
    /// addresses (of the first prefix byte). `prefix` is the full opcode
    /// prefix up to (excluding) the disp32.
    fn find_rip_f32_loads(
        &self,
        start: *const u8,
        window: usize,
        prefix: &[u8],
        expect: f32,
    ) -> Vec<*const u8> {
        let mut out = Vec::new();
        let insn_len = prefix.len() + 4;
        let end = (start as usize + window).min(self.base as usize + self.size);
        let window = end.saturating_sub(start as usize);
        if window < insn_len {
            return out;
        }
        unsafe {
            let bytes = std::slice::from_raw_parts(start, window);
            for i in 0..=(window - insn_len) {
                if &bytes[i..i + prefix.len()] != prefix {
                    continue;
                }
                let target = decode_rip_relative(start.add(i + prefix.len()));
                let t = target as usize;
                if t < self.base as usize || t + 4 > self.base as usize + self.size {
                    continue;
                }
                if (target as *const f32).read_unaligned() == expect {
                    out.push(start.add(i));
                }
            }
        }
        out
    }

    /// Derive `note_collector` + `collector_cull_site` (see
    /// [`Self::derive_playfield_styling`]). The collector is the unique
    /// CALL-rel32 target within `render_notes`' first 0x400 bytes whose own
    /// first 0x100 bytes contain the `MOVSS XMM15,[RIP+disp]` 720.0f load.
    fn derive_note_collector(&mut self) {
        /// `MOVSS XMM15, dword ptr [RIP+disp32]` opcode prefix.
        const CULL_PREFIX_XMM15: &[u8] = &[0xF3, 0x44, 0x0F, 0x10, 0x3D];
        const RENDER_NOTES_WINDOW: usize = 0x400;
        const COLLECTOR_WINDOW: usize = 0x100;

        let render_notes = match self.get_address("render_notes") {
            Some(a) => a,
            None => {
                log_warn!("  [-] note_collector -- render_notes unresolved");
                return;
            }
        };

        // Every E8 byte in the window is a candidate CALL opcode; decoding a
        // displacement byte as a call yields an out-of-module target (or one
        // that fails the content check), so candidates self-filter.
        let mut verified: Vec<(*const u8, *const u8)> = Vec::new(); // (fn, cull insn)
        unsafe {
            let mod_lo = self.base as usize;
            let mod_hi = mod_lo + self.size;
            for i in 0..RENDER_NOTES_WINDOW {
                let p = render_notes.add(i);
                if *p != 0xE8 {
                    continue;
                }
                let target = decode_call_rel32(p);
                let t = target as usize;
                if t < mod_lo || t + COLLECTOR_WINDOW > mod_hi || target == render_notes {
                    continue;
                }
                let culls =
                    self.find_rip_f32_loads(target, COLLECTOR_WINDOW, CULL_PREFIX_XMM15, 720.0);
                match culls.len() {
                    0 => {}
                    1 => {
                        if !verified.iter().any(|&(f, _)| f == target) {
                            verified.push((target, culls[0]));
                        }
                    }
                    n => {
                        log_warn!(
                            "  [-] note_collector -- candidate +0x{:X} has {} XMM15 720.0 loads (expected 1)",
                            target.offset_from(self.base) as usize,
                            n
                        );
                        return;
                    }
                }
            }
        }

        if verified.len() != 1 {
            log_warn!(
                "  [-] note_collector -- expected exactly 1 verified callee, got {}",
                verified.len()
            );
            return;
        }
        let (collector, cull_site) = verified[0];
        self.resolved.insert("note_collector".into(), collector);
        self.resolved
            .insert("collector_cull_site".into(), cull_site);
        unsafe {
            log_info!(
                "  [+] note_collector (derived) @ +0x{:X}; collector_cull_site @ +0x{:X}",
                collector.offset_from(self.base) as usize,
                cull_site.offset_from(self.base) as usize
            );
        }
    }

    /// Derive `guideline_draw`, `guideline_cull_site`, and
    /// `guideline_bulk_emitter` (see [`Self::derive_playfield_styling`]).
    ///
    /// The guideline draw's prologue AOB matches 3 functions on both
    /// supported builds, so candidates are classified by content: the real
    /// one (and only it) contains, within its first 0x800 bytes, BOTH the
    /// XMM9-form 720.0f load AND a `CALL get_offset_y`. Its bulk emitter is
    /// then the unique callee in the same window whose body starts with the
    /// verified command-header sequence (`ADD [RCX+0xC],0x10` …) and carries
    /// the tag-0x01 write + `count*0x14` stride math — and which has exactly
    /// ONE CALL xref module-wide (the transform detour assumes a private
    /// caller).
    fn derive_guideline_targets(&mut self) {
        /// Shared prologue of the guideline draw (and 2 unrelated functions):
        /// `MOV RAX,RSP; PUSH RBP/R12..R15; LEA RBP,[RAX-0x68]; SUB RSP,0x140`.
        const GUIDELINE_PROLOGUE: &str =
            "48 8B C4 55 41 54 41 55 41 56 41 57 48 8D 68 98 48 81 EC 40 01 00 00";
        /// `MOVSS XMM9, dword ptr [RIP+disp32]` opcode prefix.
        const CULL_PREFIX_XMM9: &[u8] = &[0xF3, 0x44, 0x0F, 0x10, 0x0D];
        const DRAW_WINDOW: usize = 0x800;
        /// Emitter body opening: `ADD dword [RCX+0xC],0x10; MOV EAX,[RCX+0xC]`.
        const EMITTER_HEAD: &[u8] = &[0x83, 0x41, 0x0C, 0x10, 0x8B, 0x41, 0x0C];
        /// Emitter tag/stride core: `MOV EAX,1; MOV [R10],AX` (command tag
        /// 0x01) + `LEA ECX,[R11+R11*4]; SHL ECX,2` (count*0x14 stride).
        const EMITTER_CORE: &[u8] = &[
            0xB8, 0x01, 0x00, 0x00, 0x00, 0x66, 0x41, 0x89, 0x02, 0x43, 0x8D, 0x0C, 0x9B, 0xC1,
            0xE1, 0x02,
        ];
        const EMITTER_SCAN: usize = 0x40;

        let get_offset_y = match self.get_address("get_offset_y") {
            Some(a) => a,
            None => {
                log_warn!("  [-] guideline_draw -- get_offset_y unresolved");
                return;
            }
        };

        let candidates = scan_pattern_all(self.base, self.size, GUIDELINE_PROLOGUE);
        let mut hits: Vec<(*const u8, *const u8)> = Vec::new(); // (draw fn, cull insn)
        for c in &candidates {
            let draw = c.address;
            let culls = self.find_rip_f32_loads(draw, DRAW_WINDOW, CULL_PREFIX_XMM9, 720.0);
            if culls.len() != 1 {
                continue;
            }
            let calls_offset_y = unsafe {
                let mod_lo = self.base as usize;
                let mod_hi = mod_lo + self.size;
                let end = (draw as usize + DRAW_WINDOW).min(mod_hi) - draw as usize;
                (0..end.saturating_sub(5)).any(|i| {
                    let p = draw.add(i);
                    *p == 0xE8 && decode_call_rel32(p) == get_offset_y
                })
            };
            if calls_offset_y {
                hits.push((draw, culls[0]));
            }
        }

        if hits.len() != 1 {
            log_warn!(
                "  [-] guideline_draw -- expected exactly 1 classified candidate, got {} (of {} prologue matches)",
                hits.len(),
                candidates.len()
            );
            return;
        }
        let (draw, cull_site) = hits[0];

        // Locate the bulk emitter among the draw's CALL targets.
        let mut emitters: Vec<*const u8> = Vec::new();
        unsafe {
            let mod_lo = self.base as usize;
            let mod_hi = mod_lo + self.size;
            let end = (draw as usize + DRAW_WINDOW).min(mod_hi) - draw as usize;
            for i in 0..end.saturating_sub(5) {
                let p = draw.add(i);
                if *p != 0xE8 {
                    continue;
                }
                let target = decode_call_rel32(p);
                let t = target as usize;
                if t < mod_lo || t + EMITTER_SCAN > mod_hi {
                    continue;
                }
                let body = std::slice::from_raw_parts(target, EMITTER_SCAN);
                if body.starts_with(EMITTER_HEAD)
                    && body.windows(EMITTER_CORE.len()).any(|w| w == EMITTER_CORE)
                    && !emitters.contains(&target)
                {
                    emitters.push(target);
                }
            }
        }
        if emitters.len() != 1 {
            log_warn!(
                "  [-] guideline_bulk_emitter -- expected exactly 1 verified callee, got {}",
                emitters.len()
            );
            return;
        }
        let emitter = emitters[0];

        // The transform detour assumes the emitter is private to the
        // guideline draw: require exactly one CALL xref module-wide.
        let xrefs = self.xrefs_to(emitter);
        if xrefs.len() != 1 {
            log_warn!(
                "  [-] guideline_bulk_emitter -- expected exactly 1 caller, got {}",
                xrefs.len()
            );
            return;
        }

        self.resolved.insert("guideline_draw".into(), draw);
        self.resolved
            .insert("guideline_cull_site".into(), cull_site);
        self.resolved
            .insert("guideline_bulk_emitter".into(), emitter);
        unsafe {
            log_info!(
                "  [+] guideline_draw (derived) @ +0x{:X}; guideline_cull_site @ +0x{:X}; guideline_bulk_emitter @ +0x{:X} (1 caller)",
                draw.offset_from(self.base) as usize,
                cull_site.offset_from(self.base) as usize,
                emitter.offset_from(self.base) as usize
            );
        }
    }

    /// Pre-compute `CALL rel32` xrefs to every target that any derivation
    /// chain in `resolve_derived` will need. One pass over the module
    /// instead of one pass per target.
    fn populate_xref_cache(&mut self) {
        // Collect (name, address) pairs for the targets we know derivation
        // methods will look up. Listed here once so adding a new derived
        // method that needs xrefs is a one-liner.
        const XREF_TARGETS: &[&str] = &[
            "folder_register",
            "file_manager_load",
            "metadata_insert",
            "selectmusic_view_ctor",
            "judge_effect_push",
        ];

        let targets: Vec<*const u8> = XREF_TARGETS
            .iter()
            .filter_map(|name| self.get_address(name))
            .collect();

        if targets.is_empty() {
            return;
        }

        let results =
            unsafe { crate::core::scanner::scan_xrefs_to_batch(self.base, self.size, &targets) };

        // Re-zip: targets[i] → results[i]. Some XREF_TARGETS entries may
        // have been filtered out as missing, so iterate XREF_TARGETS and
        // pull from `targets` / `results` only the ones that resolved.
        let mut t_iter = targets.into_iter();
        let mut r_iter = results.into_iter();
        for name in XREF_TARGETS {
            if self.get_address(name).is_some() {
                if let (Some(t), Some(r)) = (t_iter.next(), r_iter.next()) {
                    self.xref_cache.insert(t, r);
                }
            }
        }
    }

    pub fn get_address(&self, name: &str) -> Option<*const u8> {
        self.resolved.get(name).copied()
    }

    /// The scanned module's base address (for `+0x…` log formatting).
    pub fn module_base(&self) -> *const u8 {
        self.base
    }

    pub fn require_address(&self, name: &str) -> *const u8 {
        self.get_address(name)
            .unwrap_or_else(|| panic!("Required signature '{}' was not resolved", name))
    }

    /// Scan for ALL matches of a named signature. Returns empty Vec if not found.
    pub fn get_all_matches(&self, name: &str) -> Vec<*const u8> {
        let sig = match SIGNATURES.iter().find(|s| s.name == name) {
            Some(s) => s,
            None => return Vec::new(),
        };
        scan_pattern_all(self.base, self.size, sig.pattern)
            .into_iter()
            .map(|r| r.address)
            .collect()
    }

    // ── Derived address resolution ──────────────────────────────────

    fn derive_folder_functor_ctors(&mut self) {
        let folder_register = match self.get_address("folder_register") {
            Some(a) => a,
            None => return,
        };

        // Step 1: Find folder_init — the function that calls folder_register the most.
        // The xrefs were pre-computed by `populate_xref_cache` so this is an
        // O(1) HashMap lookup.
        let call_sites = self.xrefs_to(folder_register);

        if call_sites.len() < 6 {
            log_warn!(
                "  [-] folder_init -- expected >=6 calls to folder_register, found {}",
                call_sites.len()
            );
            return;
        }

        // Walk backwards from the first call site to find the function prologue (48 8B C4 = MOV RAX,RSP).
        let folder_init = unsafe {
            let first_site = call_sites[0];
            let mut found: Option<*const u8> = None;
            for back in 0..0x2000usize {
                let ci = first_site.sub(back);
                if ci < self.base {
                    break;
                }
                if *ci == 0x48 && *ci.add(1) == 0x8B && *ci.add(2) == 0xC4 {
                    // Verify it's a real prologue: next byte should be PUSH (0x55, 0x57, or 0x41 5x)
                    let next = *ci.add(3);
                    if next == 0x55 || next == 0x57 || next == 0x41 {
                        found = Some(ci);
                        break;
                    }
                }
            }
            found
        };

        let folder_init = match folder_init {
            Some(addr) => {
                self.resolved.insert("folder_init".into(), addr);
                let offset = unsafe { addr.offset_from(self.base) as usize };
                log_info!(
                    "  [+] folder_init (derived from folder_register xrefs) @ +0x{:X}",
                    offset
                );
                addr
            }
            None => {
                log_warn!("  [-] folder_init -- could not find function prologue");
                return;
            }
        };

        // Step 2: Estimate folder_init size from its epilogue.
        let init_size = {
            let bytes = unsafe { std::slice::from_raw_parts(folder_init, 0x4000) };
            // Look for POP; POP; POP; RET pattern near the end
            let mut size = 0x4000;
            for i in (0..0x4000 - 4).rev() {
                if bytes[i] == 0xC3 && (bytes[i - 1] == 0x5D || bytes[i - 1] == 0x5F) {
                    size = i + 1;
                    break;
                }
            }
            size
        };

        // Step 3: Within folder_init, find folder_store_ptr.
        // It's the CALL target immediately before each folder_register call.
        // Pattern: CALL folder_store_ptr; MOV RDX,RAX; LEA RCX,...; CALL folder_register
        unsafe {
            // For each folder_register call site within folder_init, find the preceding CALL
            for &site in &call_sites {
                let site_off = site.offset_from(folder_init) as usize;
                if site_off >= init_size {
                    continue;
                }

                // Scan backwards from the folder_register call to find the nearest E8 CALL
                for back in 5..64usize {
                    let prev = site.sub(back);
                    if prev < folder_init {
                        break;
                    }
                    if *prev == 0xE8 {
                        let target = decode_call_rel32(prev);
                        if target != folder_register {
                            if self.get_address("folder_store_ptr").is_none() {
                                self.resolved.insert("folder_store_ptr".into(), target);
                                let offset = target.offset_from(self.base) as usize;
                                log_info!("  [+] folder_store_ptr (derived) @ +0x{:X}", offset);
                            }
                            break;
                        }
                    }
                }
                if self.get_address("folder_store_ptr").is_some() {
                    break;
                }
            }

            // Step 4: Find folder_property_ctor.
            // Pattern: TEST RAX,RAX; JZ xx; MOV RCX,RAX; CALL folder_property_ctor
            // Bytes: 48 85 C0 74 ?? 48 8B C8 E8
            for i in 0..init_size.saturating_sub(13) {
                let p = folder_init.add(i);
                if *p == 0x48
                    && *p.add(1) == 0x85
                    && *p.add(2) == 0xC0
                    && *p.add(3) == 0x74
                    && *p.add(5) == 0x48
                    && *p.add(6) == 0x8B
                    && *p.add(7) == 0xC8
                    && *p.add(8) == 0xE8
                {
                    let target = decode_call_rel32(p.add(8));
                    self.resolved.insert("folder_property_ctor".into(), target);
                    let offset = target.offset_from(self.base) as usize;
                    log_info!("  [+] folder_property_ctor (derived) @ +0x{:X}", offset);
                    break;
                }
            }

            // Step 5: Find folder_functor_ctor and folder_filter_functor_ctor.
            // They're called in pairs: CALL functor_ctor; ...; CALL filter_functor_ctor
            // Both are called with the same EDX value (bit_index).
            // For FIRST STEP: XOR EDX,EDX (33 D2) precedes each.
            // Find the first pair of CALLs preceded by XOR EDX,EDX.
            let mut functor_pair: Vec<*const u8> = Vec::new();
            let mut i = 0;
            while i < init_size.saturating_sub(10) && functor_pair.len() < 2 {
                let p = folder_init.add(i);
                // Look for: 33 D2 (XOR EDX,EDX) ... E8 (CALL) within 16 bytes
                if *p == 0x33 && *p.add(1) == 0xD2 {
                    for j in 2..16usize {
                        if i + j + 5 > init_size {
                            break;
                        }
                        if *p.add(j) == 0xE8 {
                            let target = decode_call_rel32(p.add(j));
                            // Skip known targets (string builders, alloc, etc.)
                            let store_ptr = self.get_address("folder_store_ptr");
                            let prop_ctor = self.get_address("folder_property_ctor");
                            if Some(target) != store_ptr
                                && Some(target) != prop_ctor
                                && target != folder_register
                            {
                                functor_pair.push(target);
                                i += j + 5; // skip past this CALL
                            }
                            break;
                        }
                    }
                }
                i += 1;
            }

            if functor_pair.len() >= 2 {
                self.resolved
                    .insert("folder_functor_ctor".into(), functor_pair[0]);
                let offset = functor_pair[0].offset_from(self.base) as usize;
                log_info!("  [+] folder_functor_ctor (derived) @ +0x{:X}", offset);

                self.resolved
                    .insert("folder_filter_functor_ctor".into(), functor_pair[1]);
                let offset = functor_pair[1].offset_from(self.base) as usize;
                log_info!(
                    "  [+] folder_filter_functor_ctor (derived) @ +0x{:X}",
                    offset
                );
            } else {
                log_warn!(
                    "  [-] folder functor ctors -- expected 2 targets after XOR EDX,EDX, found {}",
                    functor_pair.len()
                );
            }
        }
    }

    fn derive_gameplay_obj_addresses(&mut self) {
        let alloc_site = match self.get_address("gameplay_obj_alloc") {
            Some(a) => a,
            None => return,
        };

        unsafe {
            // Size imm32 is at offset +1 from the B9 (MOV ECX, imm32)
            let size_addr = alloc_site.add(1);
            self.resolved
                .insert("gameplay_obj_alloc_size".into(), size_addr);
            let original_size = (size_addr as *const u32).read_unaligned();
            let offset = size_addr.offset_from(self.base) as usize;
            log_info!(
                "  [+] gameplay_obj_alloc_size (derived) @ +0x{:X} (current=0x{:X})",
                offset,
                original_size
            );

            // Constructor CALL is at offset +22 (E8 xx xx xx xx)
            let ctor_call = alloc_site.add(22);
            if *ctor_call != 0xE8 {
                log_warn!(
                    "  [-] gameplay_obj_ctor -- expected E8 at alloc+22, got 0x{:02X}",
                    *ctor_call
                );
                return;
            }
            let ctor_addr = decode_call_rel32(ctor_call);
            self.resolved.insert("gameplay_obj_ctor".into(), ctor_addr);
            let offset = ctor_addr.offset_from(self.base) as usize;
            log_info!("  [+] gameplay_obj_ctor (derived) @ +0x{:X}", offset);
        }
    }

    fn find_sprite_vtable(&mut self) {
        let vtable = match self.find_vtable_by_rtti(".?AVSprite@agcs@@", "sprite_vtable") {
            Some(v) => v,
            None => return,
        };
        self.resolved.insert("sprite_vtable".into(), vtable);
        let offset = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] sprite_vtable (RTTI) @ +0x{:X}", offset);
    }

    fn find_check_step_data_actor(&mut self) {
        let rtti_name = ".?AVCheckStepDataActor@common@sequence@@";
        let vtable = match self.find_vtable_by_rtti(rtti_name, "check_step_data") {
            Some(v) => v,
            None => return,
        };

        self.resolved
            .insert("check_step_data_vtable".into(), vtable);
        let vt_off = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] check_step_data_vtable (RTTI) @ +0x{:X}", vt_off);

        unsafe {
            // vtable[6] = per-frame update function
            let update_func = *(vtable.add(6 * 8) as *const *const u8);
            if update_func.is_null() {
                log_warn!("  [-] check_step_data_update -- vtable[6] is NULL");
                return;
            }
            let fn_off = update_func.offset_from(self.base) as usize;
            if fn_off >= self.size {
                log_warn!("  [-] check_step_data_update -- vtable[6] points outside module");
                return;
            }
            self.resolved
                .insert("check_step_data_update".into(), update_func);
            log_info!("  [+] check_step_data_update (vtable[6]) @ +0x{:X}", fn_off);

            // Find global table pointer via SHL+MOV pattern
            if let Some(global_ptr) = self.find_rip_load_near_shl(update_func, 256) {
                self.resolved
                    .insert("step_data_global_table".into(), global_ptr);
                log_info!(
                    "  [+] step_data_global_table (instruction scan) @ {:p}",
                    global_ptr
                );
            } else {
                log_warn!("  [-] step_data_global_table -- SHL+MOV pattern not found");
            }
        }
    }

    /// Derive the ultrafast-boot replay/pacing addresses from the resolved
    /// `CheckStepDataActor::onUpdate` body. All four are decoded from
    /// instructions inside onUpdate (no hardcoded absolutes) — see
    /// `docs/ultrafast_boot_research.md` §2 and the ultrafast-boot design's
    /// derivation table. Confirmed byte-exact on gamemdx 20260721.
    ///
    /// Soft on every seam: a missing anchor logs `[-]` and inserts nothing.
    /// The ultrafast-boot cache/pacing path gates on presence, so plain
    /// fast-bootup batching is unaffected by any miss here.
    fn derive_ultrafast_boot(&mut self) {
        // onUpdate body is ~0x8DA bytes on 20260721; a 0xC00 window covers it
        // without spilling far into the next function.
        const WINDOW: usize = 0xC00;
        /// `MOV byte ptr [RAX+0x1b0],1` — the corruption-flag write. Unique
        /// in the body; the 5 bytes before it are the `find_music_by_mcode`
        /// CALL.
        const FLAG_WRITE: [u8; 7] = [0xC6, 0x80, 0xB0, 0x01, 0x00, 0x00, 0x01];

        let update = match self.get_address("check_step_data_update") {
            Some(a) => a,
            None => {
                log_warn!("  [-] ultrafast_boot -- check_step_data_update unresolved");
                return;
            }
        };
        let mgr = match self.get_address("step_data_global_table") {
            Some(a) => a,
            None => {
                log_warn!("  [-] ultrafast_boot -- step_data_global_table unresolved");
                return;
            }
        };

        let base = self.base;
        let mod_lo = base as usize;
        let mod_hi = mod_lo + self.size;
        let in_module = |p: *const u8| (p as usize) >= mod_lo && (p as usize) < mod_hi;
        let end = (update as usize + WINDOW).min(mod_hi) - update as usize;

        // Small insert+log helper (keeps the four decodes uniform).
        macro_rules! record {
            ($name:literal, $opt:expr) => {
                match $opt {
                    Some(t) if in_module(t) => {
                        self.resolved.insert($name.into(), t);
                        log_info!("  [+] {} @ +0x{:X}", $name, t.offset_from(base) as usize);
                    }
                    _ => log_warn!("  [-] {} -- derivation anchor not found", $name),
                }
            };
        }

        unsafe {
            // (1) music_db_global = &DAT_1806f2d78: the first `MOV RCX,[rip]`
            //     (48 8B 0D) in the body. The manager global shares this
            //     opcode later on, so "first occurrence, and != manager"
            //     pins the music DB.
            let mut music_db: Option<*const u8> = None;
            for i in 0..end.saturating_sub(7) {
                let p = update.add(i);
                if *p == 0x48 && *p.add(1) == 0x8B && *p.add(2) == 0x0D {
                    let target = decode_rip_relative(p.add(3));
                    if in_module(target) && target != mgr {
                        music_db = Some(target);
                        break;
                    }
                }
            }
            record!("music_db_global", music_db);

            // (2) variable_bpm_threshold = &DAT_180393f40: the unique
            //     `MOVSD XMM8,[rip]` (F2 44 0F 10 05) in the prologue.
            let mut threshold: Option<*const u8> = None;
            for i in 0..end.saturating_sub(9) {
                let p = update.add(i);
                if *p == 0xF2
                    && *p.add(1) == 0x44
                    && *p.add(2) == 0x0F
                    && *p.add(3) == 0x10
                    && *p.add(4) == 0x05
                {
                    threshold = Some(decode_rip_relative(p.add(5)));
                    break;
                }
            }
            record!("variable_bpm_threshold", threshold);

            // (3) find_music_by_mcode = 0x1801b4290: the CALL immediately
            //     preceding the unique 0x1b0 corruption-flag write.
            let mut find_mcode: Option<*const u8> = None;
            'outer: for i in 5..end.saturating_sub(FLAG_WRITE.len()) {
                for (j, b) in FLAG_WRITE.iter().enumerate() {
                    if *update.add(i + j) != *b {
                        continue 'outer;
                    }
                }
                let call = update.add(i - 5);
                if *call == 0xE8 {
                    find_mcode = Some(decode_call_rel32(call));
                }
                break;
            }
            record!("find_music_by_mcode", find_mcode);

            // (4) step_data_release = 0x1801ff1b0: the `MOV RCX,[rip]` whose
            //     target IS the manager global and whose next instruction is
            //     a CALL (the post-side-loop release site).
            let mut release: Option<*const u8> = None;
            for i in 0..end.saturating_sub(12) {
                let p = update.add(i);
                if *p == 0x48
                    && *p.add(1) == 0x8B
                    && *p.add(2) == 0x0D
                    && decode_rip_relative(p.add(3)) == mgr
                    && *p.add(7) == 0xE8
                {
                    release = Some(decode_call_rel32(p.add(7)));
                    break;
                }
            }
            record!("step_data_release", release);

            // (5) entry_has_chart_vslot: onUpdate's own `hasChart(mode,
            //     difficulty)` virtual call on the music-DB entry —
            //     `MOV R8D,EBX; MOV EDI,[RSP+0x48]; MOV EDX,EDI; MOV RCX,RSI;
            //     CALL qword [RAX+disp8]`. The slot is `+0x70` on 20260324+
            //     but `+0x58` on 20250805 / 20260224 (three vtable entries
            //     were inserted; on the old builds `+0x70` is `isShock`, same
            //     argument shape — a silent wrong answer, not a crash). The
            //     replay path must call whatever slot the game itself calls.
            const HAS_CHART_VCALL: [u8; 13] = [
                0x44, 0x8B, 0xC3, 0x8B, 0x7C, 0x24, 0x48, 0x8B, 0xD7, 0x48, 0x8B, 0xCE, 0xFF,
            ];
            let mut vslot: Option<usize> = None;
            let mut vslot_hits = 0usize;
            for i in 0..end.saturating_sub(HAS_CHART_VCALL.len() + 2) {
                let p = update.add(i);
                if (0..HAS_CHART_VCALL.len()).all(|j| *p.add(j) == HAS_CHART_VCALL[j])
                    && *p.add(13) == 0x50
                {
                    vslot = Some(*p.add(14) as usize);
                    vslot_hits += 1;
                }
            }
            match (vslot, vslot_hits) {
                (Some(slot), 1) if slot % 8 == 0 && slot < 0x200 => {
                    self.publish_value("entry_has_chart_vslot", slot);
                }
                (_, n) => log_warn!(
                    "  [-] entry_has_chart_vslot -- hasChart vcall not uniquely found in onUpdate ({} hit(s))",
                    n
                ),
            }
        }
    }

    /// Byte offset of the music-DB entry's `hasChart(mode, difficulty)`
    /// vtable slot as CheckStepDataActor::onUpdate itself calls it, or
    /// `None` (fast_bootup's cache replay must then stay off).
    pub fn entry_has_chart_vslot(&self) -> Option<usize> {
        self.published_value("entry_has_chart_vslot")
    }

    fn find_scene_transition(&mut self) {
        let needle = "sequence::TransitionSequence::createNextSequence";
        if let Some(func_addr) = self.find_function_by_debug_string(needle, "scene_transition") {
            self.resolved.insert("scene_transition".into(), func_addr);
            let offset = unsafe { func_addr.offset_from(self.base) as usize };
            log_info!("  [+] scene_transition (string ref) @ +0x{:X}", offset);
        }
    }

    fn find_auto_foot_panel(&mut self) {
        let vtable = match self.find_vtable_by_rtti(".?AVAutoFootPanel@input@@", "auto_foot_panel")
        {
            Some(v) => v,
            None => return,
        };

        self.resolved
            .insert("auto_foot_panel_vtable".into(), vtable);
        let vt_off = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] auto_foot_panel_vtable (RTTI) @ +0x{:X}", vt_off);

        unsafe {
            let update_func = *(vtable.add(8) as *const *const u8);
            if update_func.is_null() {
                log_warn!("  [-] auto_foot_panel_update -- vtable[1] is NULL");
                return;
            }
            self.resolved
                .insert("auto_foot_panel_update".into(), update_func);
            let fn_off = update_func.offset_from(self.base) as usize;
            log_info!("  [+] auto_foot_panel_update (vtable[1]) @ +0x{:X}", fn_off);
        }
    }

    fn find_judge_notes(&mut self) {
        let needle = "sequence::dance::GamePlayActor::judgeNotes";
        if let Some(func_addr) = self.find_function_by_debug_string(needle, "judge_notes") {
            self.resolved.insert("judge_notes".into(), func_addr);
            let offset = unsafe { func_addr.offset_from(self.base) as usize };
            log_info!("  [+] judge_notes (string ref) @ +0x{:X}", offset);
        }
    }

    /// Find the `GamePlayActor` vtable via RTTI. Used to filter actor-tree
    /// children to GamePlayActor instances when dispatching the
    /// `gauge::GAME_OVER` message for Quick Fail / Quick Restart.
    fn find_gameplay_actor_vtable(&mut self) {
        let rtti_name = ".?AVGamePlayActor@dance@sequence@@";
        let vtable = match self.find_vtable_by_rtti(rtti_name, "gameplay_actor_vtable") {
            Some(v) => v,
            None => return,
        };
        self.resolved.insert("gameplay_actor_vtable".into(), vtable);
        let offset = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] gameplay_actor_vtable (RTTI) @ +0x{:X}", offset);
    }

    /// Find the `sequence::dance::DancePlaySequence` vtable via RTTI — the
    /// identity gate that tells a LIVE DancePlaySequence apart from any other
    /// active TransitionSequence child (the scene callbacks fire BEFORE
    /// `createNextSequence`, so at GAMEPLAY entry the child is still the
    /// stage-indicator sequence for a few frames; `song_reset::dps_step`
    /// refuses to read a step out of it). Optional: a miss only disables the
    /// consumers' DPS-step gates (background dancers stay hidden).
    fn find_dance_play_sequence_vtable(&mut self) {
        let rtti_name = ".?AVDancePlaySequence@dance@sequence@@";
        let vtable = match self.find_vtable_by_rtti(rtti_name, "dance_play_sequence_vtable") {
            Some(v) => v,
            None => return,
        };
        self.resolved
            .insert("dance_play_sequence_vtable".into(), vtable);
        let offset = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] dance_play_sequence_vtable (RTTI) @ +0x{:X}", offset);
    }

    /// Find the `sequence::dance::SceneManageActor` and
    /// `sequence::dance::MovieActor` vtables via RTTI — the identity gates of
    /// the Background Dancers' fullscreen-movie probe
    /// (`background_dancers::movie_backdrop`): the live DancePlaySequence's
    /// SceneManageActor child (created at DPS step 2) owns the song's
    /// MovieActor child whenever the song has a movie and VIDEO SIZE shows
    /// one. Both classes are single-inheritance `agcs::Actor`s on every
    /// supported build. Optional: a miss only disables the dancers'
    /// FULLSCREEN (NO STAGE) movie mode (it degrades to THUMBNAIL).
    /// RE: `docs/background_dancers_research.md` §7.
    fn find_movie_backdrop_vtables(&mut self) {
        for (rtti, name) in [
            (
                ".?AVSceneManageActor@dance@sequence@@",
                "scene_manage_actor_vtable",
            ),
            (".?AVMovieActor@dance@sequence@@", "movie_actor_vtable"),
        ] {
            if let Some(vt) = self.find_vtable_by_rtti(rtti, name) {
                self.resolved.insert(name.into(), vt);
                let offset = unsafe { vt.offset_from(self.base) as usize };
                log_info!("  [+] {} (RTTI) @ +0x{:X}", name, offset);
            }
            // find_vtable_by_rtti logs its own [-] on failure.
        }
    }

    /// Derive `app_heap_handle` from `app_heap_reserve_anchor`.
    ///
    /// The reserve function for 12-byte-stride vectors has a fixed prologue
    /// shape. At `anchor + 0x7B` it does `MOV RCX, [RIP+disp32]` to load the
    /// heap-handle pointer, followed by `CALL agcs_heap_malloc` at `anchor +
    /// 0x82`. We decode the RIP-relative displacement to get the heap handle
    /// global, and cross-check that the CALL target matches the separately
    /// resolved `agcs_heap_malloc` signature.
    fn derive_app_heap_handle(&mut self) {
        let anchor = match self.get_address("app_heap_reserve_anchor") {
            Some(a) => a,
            None => {
                log_warn!("  [-] app_heap_handle -- reserve anchor not resolved");
                return;
            }
        };

        unsafe {
            // Expected instruction at anchor+0x7B: 48 8B 0D disp32 (MOV RCX,[RIP+disp32])
            let mov_site = anchor.add(0x7B);
            if *mov_site != 0x48 || *mov_site.add(1) != 0x8B || *mov_site.add(2) != 0x0D {
                log_warn!(
                    "  [-] app_heap_handle -- expected MOV RCX,[RIP+disp32] at anchor+0x7B, got {:02X} {:02X} {:02X}",
                    *mov_site, *mov_site.add(1), *mov_site.add(2)
                );
                return;
            }
            let handle_addr = decode_rip_relative(mov_site.add(3));
            self.resolved.insert("app_heap_handle".into(), handle_addr);
            let offset = handle_addr.offset_from(self.base) as usize;
            log_info!("  [+] app_heap_handle (derived) @ +0x{:X}", offset);

            // Cross-check: CALL at anchor+0x82 should target agcs_heap_malloc.
            let call_site = anchor.add(0x82);
            if *call_site == 0xE8 {
                let call_target = decode_call_rel32(call_site);
                if let Some(malloc) = self.get_address("agcs_heap_malloc") {
                    if call_target != malloc {
                        log_warn!(
                            "  [!] app_heap_reserve_anchor CALL target ({:p}) != agcs_heap_malloc ({:p}) -- one of the signatures may be mismatched",
                            call_target, malloc
                        );
                    }
                }
            }
        }
    }

    /// Derive the XACT-2 audio addresses `services::game_audio` consumes, from
    /// the three audio anchors. Nothing here calls a game function — it is
    /// address arithmetic over the module's own bytes.
    ///
    /// Three independent stages, each degrading on its own so one missing
    /// anchor cannot cost the others:
    ///
    /// - **match-count diagnostic** — `resolve_all` has first-match-per-name
    ///   semantics, so a second match anywhere in the module would be silent.
    ///   For `se_play_inner_body` that matters more than usual: its neighbour
    ///   `se_prepare_inner` is byte-for-byte identical for ~0x65 bytes, and
    ///   binding Prepare instead of Play would look like "no audio" several
    ///   steps later rather than failing here.
    /// - [`Self::derive_audio_manager_and_play`] — the manager global and the
    ///   inner play entry.
    /// - [`Self::derive_audio_named_bank_count`] — the free-slot safety gate.
    fn derive_game_audio_addresses(&mut self) {
        // Three whole-module single-needle scans. A deliberate boot cost:
        // these patterns' uniqueness is the assumption the rest of the audio
        // binding rests on, and this is the only place it is observable.
        let n_play = self.get_all_matches("se_play").len();
        let n_inner = self.get_all_matches("se_play_inner_body").len();
        let n_slot = self.get_all_matches("bank_slot_of_file_loop").len();
        log_info!(
            "  [+] audio signature match counts: se_play={} se_play_inner_body={} bank_slot_of_file_loop={}",
            n_play, n_inner, n_slot
        );
        // A count of 0 is already reported as `[-]` by `resolve_all`.
        for (name, count) in [
            ("se_play", n_play),
            ("se_play_inner_body", n_inner),
            ("bank_slot_of_file_loop", n_slot),
        ] {
            if count > 1 {
                log_warn!(
                    "  [!] {} matched {} times -- the pattern is not unique on this build and resolve_all took the first match; verify against the per-build table in research/bank-slot-and-anchors.md",
                    name, count
                );
            }
        }

        self.derive_audio_manager_and_play();
        self.derive_audio_named_bank_count();
    }

    /// Chains A and B of the audio derivation (see
    /// `.agents/planning/20260725-assist-tick/research/bank-slot-and-anchors.md`
    /// → "Derivation chains").
    ///
    /// **Chain A.** The `se_play_inner_body` match sits at `se_play_inner + 0xF`,
    /// on the instruction that loads the audio-manager singleton:
    ///
    /// ```text
    /// -0x0F  48 89 5C 24 08 …        ; se_play_inner prologue (15 bytes)
    /// +0x00  MOV RSI,[rip+disp32]    ; disp32 at +3 -> audio_manager_global
    /// +0x07  MOVSXD RDI,ECX          ; bank_id
    /// +0x0F  LEA RAX,[RDI+1] …       ; bank = *(mgr + ((bank_id+1)*2)*8)
    /// +0x56  CALL [R10+0x20]         ; SoundBank::Play — Play-vs-Prepare discriminator
    /// ```
    ///
    /// The manager global's absolute address **moves on every game build** (four
    /// distinct addresses across the four verified builds), so it is RIP-decoded
    /// from this anchor rather than scanned for or hardcoded. The `-0xF` entry
    /// offset is only trusted after the prologue bytes are confirmed there.
    ///
    /// **Chain B.** `se_play`'s first `CALL rel32` must land on the derived inner
    /// entry — the same style of corroboration
    /// [`Self::derive_app_heap_handle`] does on its `CALL` target. Disagreement
    /// means one of the two patterns mis-resolved, and is a warning rather than a
    /// failure because nothing consumes these addresses until `game_audio` asks
    /// for them.
    fn derive_audio_manager_and_play(&mut self) {
        /// `se_play_inner`'s prologue, byte-identical on all four verified builds.
        const INNER_PROLOGUE: &[u8] = &[
            0x48, 0x89, 0x5C, 0x24, 0x08, 0x48, 0x89, 0x74, 0x24, 0x10, 0x57, 0x48, 0x83, 0xEC,
            0x40,
        ];
        /// Distance from the `se_play_inner_body` match back to the function entry.
        const BODY_TO_ENTRY: usize = 0x0F;
        /// Offset of the `MOV RSI,[rip+disp32]` displacement within the match.
        const MGR_DISP: usize = 3;
        /// Window searched for `se_play`'s first `CALL rel32` (it is at entry+0x73).
        const CALL_WINDOW: usize = 0x80;

        let anchor = match self.get_address("se_play_inner_body") {
            Some(a) => a,
            None => {
                log_warn!(
                    "  [-] audio_manager_global/se_play_inner -- se_play_inner_body anchor unresolved"
                );
                return;
            }
        };

        let base = self.base;
        let size = self.size;
        let module_end = (base as usize).saturating_add(size);
        // Plain integer arithmetic, so it stays valid for a pointer that turned
        // out not to be in the module (which is exactly the case being logged).
        let rel = move |p: *const u8| (p as usize).wrapping_sub(base as usize);
        let in_module =
            move |p: *const u8, len: usize| p as usize >= base as usize && rel(p) + len <= size;

        unsafe {
            // The three bytes before the displacement are literal in the
            // pattern, so a match guarantees the instruction shape; what it
            // cannot guarantee is that the displacement lands in the module.
            let mgr = decode_rip_relative(anchor.add(MGR_DISP));
            if !in_module(mgr, std::mem::size_of::<usize>()) {
                log_warn!(
                    "  [-] audio_manager_global -- RIP target {:p} is outside the module [{:p}, 0x{:X})",
                    mgr, base, module_end
                );
                return;
            }
            self.resolved.insert("audio_manager_global".into(), mgr);
            log_info!(
                "  [+] audio_manager_global (derived, se_play_inner_body RIP disp32) @ +0x{:X}",
                rel(mgr)
            );

            let mut verified: Option<*const u8> = None;
            if rel(anchor) >= BODY_TO_ENTRY {
                let candidate = anchor.sub(BODY_TO_ENTRY);
                let actual = std::slice::from_raw_parts(candidate, INNER_PROLOGUE.len());
                if actual == INNER_PROLOGUE {
                    verified = Some(candidate);
                } else {
                    log_warn!(
                        "  [!] se_play_inner -- prologue mismatch at se_play_inner_body-0x{:X}: got {:02X?}; falling back to find_function_entry",
                        BODY_TO_ENTRY, actual
                    );
                }
            }
            let inner = match verified {
                Some(p) => {
                    log_info!(
                        "  [+] se_play_inner (derived, prologue verified) @ +0x{:X}",
                        rel(p)
                    );
                    p
                }
                None => {
                    let p = find_function_entry(anchor, base);
                    log_info!(
                        "  [+] se_play_inner (derived via find_function_entry) @ +0x{:X}",
                        rel(p)
                    );
                    p
                }
            };
            self.resolved.insert("se_play_inner".into(), inner);

            // Chain B. A missing `se_play` is already reported by `resolve_all`,
            // so it gets no second warning here.
            if let Some(se_play) = self.get_address("se_play") {
                match scan_first_call_rel32(se_play, CALL_WINDOW) {
                    Some(target) if target == inner => {}
                    Some(target) => log_warn!(
                        "  [!] se_play (+0x{:X}) first CALL rel32 targets +0x{:X} but se_play_inner derived as +0x{:X} -- one of the two audio signatures has mis-resolved",
                        rel(se_play), rel(target), rel(inner)
                    ),
                    None => log_warn!(
                        "  [!] se_play (+0x{:X}) -- no CALL rel32 within 0x{:X} bytes; cannot corroborate se_play_inner",
                        rel(se_play), CALL_WINDOW
                    ),
                }
            }
        }
    }

    /// Chain C: publish the address of `bank_slot_of_file`'s named-bank count
    /// (the `CMP EBX,imm8` bound of its name-match loop) and report the value.
    ///
    /// The mapper returns `{0,1,2,3}` for the four named banks and the literal
    /// `5` for anything else, which is what leaves slot 4 permanently free and
    /// claimable. A build that added a fifth named bank would map it to slot 4
    /// and silently collide with our bank, so `register_bank` re-reads this byte
    /// and declines when it is not 4 (guard G1). The **address** is published
    /// rather than the value because the store maps names to addresses; keeping
    /// the offset here keeps it out of the service.
    fn derive_audio_named_bank_count(&mut self) {
        /// Offset of the `CMP EBX,imm8` immediate within the match.
        const COUNT_IMM8: usize = 0x2C;
        /// bgm_menu, se_system, se_normal, voice.
        const EXPECTED: u8 = 4;

        let anchor = match self.get_address("bank_slot_of_file_loop") {
            Some(a) => a,
            None => {
                log_warn!(
                    "  [-] audio_named_bank_count_site -- bank_slot_of_file_loop anchor unresolved"
                );
                return;
            }
        };

        let base = self.base;
        unsafe {
            let site = anchor.add(COUNT_IMM8);
            if (site as usize).wrapping_sub(base as usize) >= self.size {
                log_warn!(
                    "  [-] audio_named_bank_count_site -- {:p} is outside the module",
                    site
                );
                return;
            }
            let count = *site;
            self.resolved
                .insert("audio_named_bank_count_site".into(), site);
            log_info!(
                "  [+] audio_named_bank_count_site @ +0x{:X} (named bank count = {})",
                site.offset_from(base) as usize,
                count
            );
            if count != EXPECTED {
                log_warn!(
                    "  [!] named bank count is {}, expected {} -- a game build has added a named sound bank, so the free-slot assumption may no longer hold and assist tick should decline to register its own bank",
                    count, EXPECTED
                );
            }
        }
    }

    /// Resolve the Training-Mode strip HUD's runtime-validation anchors:
    /// the `screen::ArrowPalette` and `screen::ArrowRenderer` vftable
    /// addresses (RTTI walk). The strip's per-song snapshot reads the
    /// GamePlayActor's palette manager (`actor+0x130`) and arrow renderer
    /// (`actor+0x148` — the actor-init decompile's `param_1[0x26]` /
    /// `param_1[0x29]` stores) and requires each object's vptr to equal
    /// the matching vftable before ANY use — offset drift on a future
    /// build shows up as a vtable mismatch (⇒ the flat-color fallback
    /// ladder), never a wild virtual call. Both optional: a miss only
    /// degrades the strip's coloring. RE: docs/chart_strip_hud_research.md
    /// §4 + the 2026-08-14 actor-init decompile (task-02 record).
    fn derive_strip_hud_anchors(&mut self) {
        for (rtti, name) in [
            (".?AVArrowPalette@screen@@", "arrow_palette_vtable"),
            (".?AVArrowRenderer@screen@@", "arrow_renderer_vtable"),
        ] {
            if let Some(vt) = self.find_vtable_by_rtti(rtti, name) {
                self.resolved.insert(name.into(), vt);
                log_info!("  [+] {} (RTTI) @ {:p}", name, vt);
            }
            // find_vtable_by_rtti logs its own [-] on failure.
        }
    }

    /// Derive `player_option_table` — the per-side context table whose
    /// entries lead to each side's `ddr::player::Option` — from the
    /// `player_option_ctx_load` anchor (see the signature's comment for the
    /// instruction sequence and the RE record reference).
    ///
    /// The count function reads the table as `[R12 + side*8 + disp32]` with
    /// R12 pre-loaded to the **module base** via `LEA R12,[rip+disp32]`, so
    /// the table's address is `base + disp32` — but only if that LEA really
    /// resolves to the base. That is validated here rather than assumed: a
    /// compiler change that anchored R12 elsewhere would silently shift
    /// every table read, so a mismatch drops the derivation (and with it the
    /// assist-tick mod, which requires this name — fail-closed, NFR-4).
    ///
    /// Consumption (assist-tick, per song at build time):
    /// `Option(side) = *( *(table + side*8) ) + 0xE0`, JUDGMENT TIMING
    /// (`timing_music`, ±100 ms) at `Option + 0x24`.
    fn derive_player_option_table(&mut self) {
        /// Offset of the `LEA R12,[rip+disp32]` displacement within the match.
        const LEA_DISP: usize = 22;
        /// Offset of the table-load `MOV RCX,[R12+RCX*8+disp32]` displacement
        /// (the `49 8B 8C CC` opcode+ModRM+SIB spans match+42..46; the disp32
        /// follows). Off-by-one here reads the 0xCC SIB byte into the value —
        /// which is precisely what the out-of-module check below caught on
        /// the first deploy of this derivation.
        const TABLE_DISP: usize = 46;
        /// Same for the `_v1` shape, which lacks the 2-byte `XOR EDX,EDX`.
        const TABLE_DISP_V1: usize = 44;

        let (anchor, table_disp) = match self.get_address("player_option_ctx_load") {
            Some(a) => (a, TABLE_DISP),
            None => match self.get_address("player_option_ctx_load_v1") {
                Some(a) => (a, TABLE_DISP_V1),
                None => {
                    log_warn!(
                        "  [-] player_option_table -- player_option_ctx_load anchor unresolved"
                    );
                    return;
                }
            },
        };
        let base = self.base;
        unsafe {
            // Structural guard: the table load's opcode+ModRM+SIB must sit
            // right where the chosen shape says it does.
            let op = std::slice::from_raw_parts(anchor.add(table_disp - 4), 4);
            if op != [0x49, 0x8B, 0x8C, 0xCC] {
                log_warn!(
                    "  [-] player_option_table -- table-load opcode mismatch at match+{}; refusing to derive",
                    table_disp - 4
                );
                return;
            }
            let lea_target = decode_rip_relative(anchor.add(LEA_DISP));
            if lea_target != base {
                log_warn!(
                    "  [-] player_option_table -- the anchor's LEA resolves to {:p}, not the module base {:p}; refusing to derive",
                    lea_target,
                    base
                );
                return;
            }
            let disp = (anchor.add(table_disp) as *const u32).read_unaligned() as usize;
            if disp >= self.size {
                log_warn!(
                    "  [-] player_option_table -- table displacement 0x{:X} is outside the module (size 0x{:X})",
                    disp,
                    self.size
                );
                return;
            }
            let table = base.add(disp);
            self.resolved.insert("player_option_table".into(), table);
            log_info!(
                "  [+] player_option_table (derived, base-validated LEA + disp32) @ +0x{:X}",
                disp
            );

            // The Option's offset inside PlayerWork is NOT a stable layout
            // fact: the accessor the anchor CALLs (`E8` right after the table
            // disp32) returns `*ctx + OFF`, and OFF is 0xE0 on 20260324+ but
            // 0xF0 on 20250805 / 20260224. Decode it from the callee's
            // `MOV RAX,[RCX|R8]; ADD RAX,imm32` return sites (there are one
            // or two — the direct path and the post-debug-check path — and
            // they must agree). Stored as a base-relative pseudo address;
            // read via `player_option_offset()`.
            let call_insn = anchor.add(table_disp + 4);
            if *call_insn != 0xE8 {
                log_warn!("  [-] player_option_offset -- expected CALL after the table load");
                return;
            }
            let callee = decode_call_rel32(call_insn);
            let callee_off = (callee as usize).wrapping_sub(base as usize);
            if callee_off >= self.size.saturating_sub(0x400) {
                log_warn!("  [-] player_option_offset -- accessor callee outside module");
                return;
            }
            let body = std::slice::from_raw_parts(callee, 0x300);
            let mut found: Option<u32> = None;
            let mut consistent = true;
            for i in 0..body.len() - 9 {
                // 48 8B 01 = MOV RAX,[RCX]; 49 8B 00 = MOV RAX,[R8];
                // followed by 48 05 imm32 = ADD RAX,imm32.
                let load_ok = (body[i] == 0x48 && body[i + 1] == 0x8B && body[i + 2] == 0x01)
                    || (body[i] == 0x49 && body[i + 1] == 0x8B && body[i + 2] == 0x00);
                if !load_ok || body[i + 3] != 0x48 || body[i + 4] != 0x05 {
                    continue;
                }
                let imm = u32::from_le_bytes([body[i + 5], body[i + 6], body[i + 7], body[i + 8]]);
                match found {
                    None => found = Some(imm),
                    Some(prev) if prev != imm => consistent = false,
                    _ => {}
                }
            }
            match found {
                Some(off) if consistent && (0x40..=0x400).contains(&off) => {
                    self.resolved
                        .insert("player_option_offset".into(), base.add(off as usize));
                    log_info!(
                        "  [+] player_option_offset (derived from accessor @ +0x{:X}) = PlayerWork+0x{:X}",
                        callee_off,
                        off
                    );
                }
                Some(off) => log_warn!(
                    "  [-] player_option_offset -- accessor return sites disagree or out of range (0x{:X}, consistent={})",
                    off,
                    consistent
                ),
                None => log_warn!(
                    "  [-] player_option_offset -- no `MOV RAX,[ctx]; ADD RAX,imm32` in accessor @ +0x{:X}",
                    callee_off
                ),
            }
        }
    }

    /// Offset of `ddr::player::Option` inside `PlayerWork` (the value the
    /// `player_option_ctx_load` accessor adds to `*ctx`): 0xE0 on 20260324+,
    /// 0xF0 on 20250805 / 20260224. `None` when the derivation failed — every
    /// consumer must then stay inert rather than assume a layout.
    pub fn player_option_offset(&self) -> Option<usize> {
        self.get_address("player_option_offset")
            .map(|p| (p as usize).wrapping_sub(self.base as usize))
    }

    /// Read a published pseudo-address (`base + value`) back as the value.
    fn published_value(&self, name: &str) -> Option<usize> {
        self.get_address(name)
            .map(|p| (p as usize).wrapping_sub(self.base as usize))
    }

    /// Publish a small non-address value under `name` as `base + value` (the
    /// `player_option_offset` convention) so it rides the ordinary
    /// `resolved` map and the boot log.
    fn publish_value(&mut self, name: &str, value: usize) {
        self.resolved
            .insert(name.into(), unsafe { self.base.add(value) });
        log_info!("  [+] {} (derived) = 0x{:X}", name, value);
    }

    // ── GamePlayActor build-dependent layout ────────────────────────────
    //
    // Every GamePlayActor field at or above ~+0x208 sits 8 bytes LOWER on
    // 20250805 / 20260224 than on 20260324+ (the field cluster ≤ +0x1E9 —
    // side, judge counts, combo, is_dead — is identical). Three consumers
    // write into that region (song_rate's Real Speed recompute, the in-place
    // song reset's gauge-cluster + death-result restore, quick restart's
    // fallback death simulation), so the offsets are derived from the ctor's
    // seed block instead of being hardcoded. The ctor tail (byte-identical
    // apart from the register the actor lives in and the displacements):
    //
    //   MOV  dword [A+speed  ], 0x3F800000   ; multiplier current = 1.0
    //   MOV  qword [A+speed+4], 0x3F800000   ; lerp target = 1.0, +8 = 0
    //   MOV  dword [A+speed+C], 0x64         ; ×100 int copy = 100
    //   MOV  qword [A+gauge  ], 0x3F800000   ; gauge min = 1.0, max = 0.0
    //   MOV  qword [A+gauge+8], REG(0)       ; last / loss
    //   MOV  dword [A+gauge+10], REG(0)      ; gain
    //   MOV  RAX,[rip+GameWork]; MOV RCX,[RAX]; CMP qword [RCX+0x70],0; SETNE AL
    //   MOV  byte  [A+course ], AL           ; <- ANCHOR ends here
    //   MOV  word  [A+course+1], 0
    //   MOVZX EAX, byte [RBP+0x97]
    //   MOV  byte  [A+course+3], AL          ; instant-death gauge gate
    //   MOV  byte  [A+course+4], 0           ; death-result flag
    //
    // 20260721: speed 0x290, gauge 0x2A0, course 0x2B4, gate 0x2B7, result
    // 0x2B8 (actor in RDI). 20250805: 0x288 / 0x298 / 0x2AC / 0x2AF / 0x2B0
    // (actor in R12 — SIB-encoded stores). The derivation decodes the five
    // stores after the anchor generically (any base register), requires the
    // +1/+3/+4 adjacency, then demands each seed immediate (1.0 / 1.0 / 100 /
    // 1.0) at the predicted displacement in the 0x80 bytes before the anchor
    // — exactly once each — so a layout that merely LOOKS similar refuses.

    /// Course-flag seed anchor: `MOV RAX,[rip]; MOV RCX,[RAX]; CMP qword
    /// [RCX+0x70],0; SETNE AL`. Unique on all four supported builds.
    const GPA_CTOR_COURSE_SEED: &'static str =
        "48 8B 05 ?? ?? ?? ?? 48 8B 08 48 83 79 70 00 0F 95 C0";
    /// `MOVZX EAX, byte [RBP+0x97]` — the ctor's death-gate argument load.
    const GPA_CTOR_GATE_ARG_LOAD: [u8; 7] = [0x0F, 0xB6, 0x85, 0x97, 0x00, 0x00, 0x00];

    fn derive_gameplay_actor_layout(&mut self) {
        const LABEL: &str = "gameplay_actor_layout";
        let hits = scan_pattern_all(self.base, self.size, Self::GPA_CTOR_COURSE_SEED);
        if hits.len() != 1 {
            log_warn!(
                "  [-] {} -- expected 1 ctor course-seed anchor, found {}",
                LABEL,
                hits.len()
            );
            return;
        }
        let anchor = hits[0].address;
        let mod_hi = self.base as usize + self.size;
        if (anchor as usize) < self.base as usize + 0x80 || anchor as usize + 0x60 > mod_hi {
            log_warn!("  [-] {} -- anchor too close to a module edge", LABEL);
            return;
        }
        unsafe {
            // The anchor is 18 bytes (7 + 3 + 5 + 3).
            let mut p = anchor.add(18);
            // 1. MOV byte [A+course], AL
            let Some((len, course)) = decode_mem_store_disp32(p, 0x88, false) else {
                log_warn!("  [-] {} -- course-flag store not decodable", LABEL);
                return;
            };
            p = p.add(len);
            // 2. MOV word [A+course+1], 0
            let Some((len, word)) = decode_mem_store_disp32(p, 0xC7, true) else {
                log_warn!("  [-] {} -- word-clear store not decodable", LABEL);
                return;
            };
            p = p.add(len);
            // 3. MOVZX EAX, byte [RBP+0x97]
            for (i, b) in Self::GPA_CTOR_GATE_ARG_LOAD.iter().enumerate() {
                if *p.add(i) != *b {
                    log_warn!("  [-] {} -- gate-argument load not found", LABEL);
                    return;
                }
            }
            p = p.add(Self::GPA_CTOR_GATE_ARG_LOAD.len());
            // 4. MOV byte [A+course+3], AL
            let Some((len, gate)) = decode_mem_store_disp32(p, 0x88, false) else {
                log_warn!("  [-] {} -- death-gate store not decodable", LABEL);
                return;
            };
            p = p.add(len);
            // 5. MOV byte [A+course+4], 0
            let Some((_, result)) = decode_mem_store_disp32(p, 0xC6, false) else {
                log_warn!("  [-] {} -- death-result store not decodable", LABEL);
                return;
            };
            if word != course + 1 || gate != course + 3 || result != course + 4 {
                log_warn!(
                    "  [-] {} -- store adjacency broken (course 0x{:X} word 0x{:X} gate 0x{:X} result 0x{:X})",
                    LABEL,
                    course,
                    word,
                    gate,
                    result
                );
                return;
            }
            if !(0x200..=0x400).contains(&course) {
                log_warn!("  [-] {} -- course flag 0x{:X} out of range", LABEL, course);
                return;
            }
            let speed = course - 0x24;
            let gauge = course - 0x14;

            // Seed validation in the preceding 0x80 bytes: `disp32 imm32`
            // byte pairs, each exactly once.
            let window = std::slice::from_raw_parts(anchor.sub(0x80), 0x80);
            let count = |disp: usize, imm: u32| -> usize {
                let mut needle = [0u8; 8];
                needle[..4].copy_from_slice(&(disp as u32).to_le_bytes());
                needle[4..].copy_from_slice(&imm.to_le_bytes());
                window.windows(8).filter(|w| *w == needle).count()
            };
            const ONE: u32 = 0x3F80_0000;
            let seeds = [
                ("speed current = 1.0", count(speed, ONE)),
                ("speed target = 1.0", count(speed + 4, ONE)),
                ("speed int = 100", count(speed + 0xC, 100)),
                ("gauge min = 1.0", count(gauge, ONE)),
            ];
            for (what, n) in seeds {
                if n != 1 {
                    log_warn!(
                        "  [-] {} -- ctor seed `{}` found {} time(s) before the anchor (want 1); refusing",
                        LABEL,
                        what,
                        n
                    );
                    return;
                }
            }
            self.publish_value("gpa_speed_cluster", speed);
            self.publish_value("gpa_gauge_cluster", gauge);
            self.publish_value("gpa_death_gate", gate);
            log_info!(
                "  [+] {} (ctor @ +0x{:X}): speed +0x{:X} gauge +0x{:X} course +0x{:X} gate +0x{:X} result +0x{:X}",
                LABEL,
                anchor.offset_from(self.base) as usize,
                speed,
                gauge,
                course,
                gate,
                result
            );
        }
    }

    /// The build's `GamePlayActor` speed / gauge-tracking / death-flag
    /// offsets, or `None` when the ctor-seed derivation refused. Consumers
    /// MUST go inert on `None` — the region shifted by 8 bytes between
    /// 20260224 and 20260324, so no default is safe.
    pub fn gameplay_actor_layout(&self) -> Option<GamePlayActorLayout> {
        let speed = self.published_value("gpa_speed_cluster")?;
        let gauge = self.published_value("gpa_gauge_cluster")?;
        let gate = self.published_value("gpa_death_gate")?;
        Some(GamePlayActorLayout {
            speed_current: speed,
            speed_target: speed + 4,
            speed_int: speed + 0xC,
            gauge_min: gauge,
            gauge_max: gauge + 4,
            gauge_last: gauge + 8,
            gauge_loss: gauge + 0xC,
            gauge_gain: gauge + 0x10,
            death_gate: gate,
            death_result: gate + 1,
        })
    }

    // ── ShutterActor build-dependent layout ─────────────────────────────
    //
    // The stage-jacket shutter's kind fields and kind enumeration moved
    // between 20260224 and 20260324: active kind `+0x2E0` → `+0x310`,
    // pending kind `+0x2E4` → `+0x314`, 6 → 9 layer slots, and the STAGE
    // panel is kind 1 → kind 3. quick_restart_or_fail's bannerless dismiss
    // fast path reads all of those. Anchor = the per-kind layer lookup in
    // `ShutterActor::onUpdate`, followed within 0x40 bytes by the stage-kind
    // compare:
    //
    //   48 63 8E d32          MOVSXD RCX, [RSI+active_kind]
    //   48 03 C9              ADD    RCX, RCX
    //   48 8D 94 CE 88 00 00 00  LEA RDX, [RSI+RCX*8+0x88]   ; layer table, 0x10 stride
    //   ...
    //   83 BE d32 imm8        CMP    dword [RSI+active_kind], STAGE_KIND
    //
    // Unique on all four supported builds; the compare must reuse the SAME
    // displacement or the derivation refuses.
    const SHUTTER_KIND_LAYER_LOOKUP: &'static str =
        "48 63 8E ?? ?? 00 00 48 03 C9 48 8D 94 CE 88 00 00 00";

    fn derive_shutter_actor_layout(&mut self) {
        const LABEL: &str = "shutter_actor_layout";
        let hits = scan_pattern_all(self.base, self.size, Self::SHUTTER_KIND_LAYER_LOOKUP);
        if hits.len() != 1 {
            log_warn!(
                "  [-] {} -- expected 1 kind/layer lookup anchor, found {}",
                LABEL,
                hits.len()
            );
            return;
        }
        let anchor = hits[0].address;
        if anchor as usize + 0x60 > self.base as usize + self.size {
            log_warn!("  [-] {} -- anchor too close to the module end", LABEL);
            return;
        }
        unsafe {
            let active = (anchor.add(3) as *const u32).read_unaligned() as usize;
            if !(0x100..=0x800).contains(&active) {
                log_warn!(
                    "  [-] {} -- active-kind offset 0x{:X} out of range",
                    LABEL,
                    active
                );
                return;
            }
            // The stage-kind compare: `83 BE <active> imm8` within 0x40 bytes.
            let mut stage_kind: Option<i32> = None;
            let mut cmp_hits = 0usize;
            for i in 18..0x40 {
                let q = anchor.add(i);
                if *q == 0x83
                    && *q.add(1) == 0xBE
                    && (q.add(2) as *const u32).read_unaligned() as usize == active
                {
                    stage_kind = Some(*q.add(6) as i8 as i32);
                    cmp_hits += 1;
                }
            }
            let Some(stage_kind) = stage_kind else {
                log_warn!(
                    "  [-] {} -- stage-kind compare not found after the anchor",
                    LABEL
                );
                return;
            };
            if cmp_hits != 1 || !(0..=8).contains(&stage_kind) {
                log_warn!(
                    "  [-] {} -- stage-kind compare ambiguous ({} hits, kind {})",
                    LABEL,
                    cmp_hits,
                    stage_kind
                );
                return;
            }
            self.publish_value("shutter_active_kind", active);
            self.publish_value("shutter_stage_kind", stage_kind as usize);
            log_info!(
                "  [+] {} (onUpdate lookup @ +0x{:X}): active kind +0x{:X}, pending +0x{:X}, stage kind {}",
                LABEL,
                anchor.offset_from(self.base) as usize,
                active,
                active + 4,
                stage_kind
            );
        }
    }

    /// The build's `ShutterActor` kind-field offsets + stage-kind id, or
    /// `None` when the derivation refused (consumers must not fall back to
    /// the 20260324+ constants — the fields moved).
    pub fn shutter_actor_layout(&self) -> Option<ShutterActorLayout> {
        let active = self.published_value("shutter_active_kind")?;
        let stage = self.published_value("shutter_stage_kind")?;
        Some(ShutterActorLayout {
            active_kind: active,
            pending_kind: active + 4,
            stage_kind: stage as i32,
        })
    }

    /// Derive `file_manager_singleton` from xrefs to `file_manager_load`.
    ///
    /// The engine's file-loading call sites follow a consistent pattern:
    ///
    /// ```text
    /// MOV Rxx, [RIP+disp32]     ; load FileManager singleton pointer
    /// ...                        ; (0–16 bytes of setup)
    /// MOV RCX, Rxx              ; this = singleton
    /// CALL file_manager_load
    /// ```
    ///
    /// We find the first CALL xref to file_manager_load, scan backwards
    /// for the RIP-relative MOV that loads the singleton, and decode the
    /// displacement.
    fn derive_file_manager_singleton(&mut self) {
        let fm_load = match self.get_address("file_manager_load") {
            Some(a) => a,
            None => return,
        };

        let call_sites = self.xrefs_to(fm_load);
        if call_sites.is_empty() {
            log_warn!("  [-] file_manager_singleton -- no xrefs to file_manager_load");
            return;
        }

        // Scan backwards from each call site for MOV Rxx, [RIP+disp32].
        // Encoding: 48 8B {0D|1D|3D|...} disp32  (REX.W MOV reg, [RIP+disp32])
        // The ModRM byte's low 3 bits = 101 (RIP-relative), bits 3-5 = dest reg.
        unsafe {
            for &site in &call_sites {
                for back in 5..48usize {
                    let p = site.sub(back);
                    if p < self.base {
                        break;
                    }
                    // REX.W prefix (48 or 4C) + MOV opcode (8B) + ModRM with mod=00, rm=101
                    let rex = *p;
                    if (rex != 0x48 && rex != 0x4C) || *p.add(1) != 0x8B {
                        continue;
                    }
                    let modrm = *p.add(2);
                    if (modrm & 0xC7) != 0x05 {
                        // Not [RIP+disp32] addressing mode
                        continue;
                    }
                    let singleton_addr = decode_rip_relative(p.add(3));
                    // Sanity: must point into the module's .data/.rdata section
                    let off = singleton_addr.offset_from(self.base) as usize;
                    if off >= self.size {
                        continue;
                    }
                    self.resolved
                        .insert("file_manager_singleton".into(), singleton_addr);
                    log_info!(
                        "  [+] file_manager_singleton (derived from file_manager_load xref) @ +0x{:X}",
                        off,
                    );
                    return;
                }
            }
        }
        log_warn!("  [-] file_manager_singleton -- no RIP-relative MOV found near file_manager_load call sites");
    }

    /// Derive render-pipeline globals from the `render_notes` function body.
    ///
    /// The function contains inlined CommandList writes and calls to helper
    /// functions. We locate three derived addresses:
    ///
    /// - `screen_renderer_state`: the global that holds the CommandList
    ///   pointer array, found via `MOV Rxx, [RIP+disp32]` after the
    ///   blend-mode write.
    /// - `default_shader`: the shader used for the silver shock-arrow
    ///   glyph pass, found inside the shock-arrow render helper (the
    ///   CALL target just before the blend-mode write).
    /// - `set_render_state`: the function that flushes blend-mode changes
    ///   to the CommandList, found as the CALL immediately after the
    ///   blend-mode write.
    fn derive_render_globals(&mut self) {
        let render_notes = match self.get_address("render_notes") {
            Some(a) => a,
            None => return,
        };

        // Scan for `MOV dword [RSI+0x2C], 0x2` — the additive blend mode
        // write that precedes the lightning overlay. Encoding: C7 46 2C 02 00 00 00.
        // This is distinctive: literal 2 written to a fixed struct offset.
        const BLEND_WRITE: [u8; 7] = [0xC7, 0x46, 0x2C, 0x02, 0x00, 0x00, 0x00];
        const SCAN_LEN: usize = 0x300;

        let body = unsafe { std::slice::from_raw_parts(render_notes, SCAN_LEN) };
        let blend_off = match body.windows(7).position(|w| w == BLEND_WRITE) {
            Some(off) => off,
            None => {
                log_warn!(
                    "  [-] render globals -- blend-mode write pattern not found in render_notes"
                );
                return;
            }
        };

        unsafe {
            // set_render_state: the E8 CALL within ~16 bytes AFTER the blend write.
            // Pattern: MOV RCX, RSI (48 8B CE) then E8 <disp32>.
            let search_start = blend_off + 7;
            let mut found_srs = false;
            let mut srs_call_end: usize = search_start + 20; // fallback
            #[allow(clippy::needless_range_loop)]
            for i in search_start..std::cmp::min(search_start + 20, SCAN_LEN - 5) {
                if body[i] == 0xE8 {
                    let call_addr = render_notes.add(i);
                    let target = decode_call_rel32(call_addr);
                    self.resolved.insert("set_render_state".into(), target);
                    let off = target.offset_from(self.base) as usize;
                    log_info!(
                        "  [+] set_render_state (derived from render_notes blend write) @ +0x{:X}",
                        off
                    );
                    found_srs = true;
                    srs_call_end = i + 5;
                    break;
                }
            }
            if !found_srs {
                log_warn!("  [-] set_render_state -- no CALL found after blend-mode write");
            }

            // screen_renderer_state: the first RIP-relative MOV to R10 or R9
            // (4C 8B 15 or 4C 8B 0D) within ~32 bytes after set_render_state.
            for i in srs_call_end..std::cmp::min(srs_call_end + 40, SCAN_LEN - 7) {
                if body[i] == 0x4C
                    && body[i + 1] == 0x8B
                    && (body[i + 2] == 0x15 || body[i + 2] == 0x0D)
                {
                    let global = decode_rip_relative(render_notes.add(i + 3));
                    let off = global.offset_from(self.base) as usize;
                    if off < self.size {
                        self.resolved.insert("screen_renderer_state".into(), global);
                        log_info!(
                            "  [+] screen_renderer_state (derived from render_notes) @ +0x{:X}",
                            off,
                        );
                    }
                    break;
                }
            }

            // default_shader: inside the shock-arrow render helper, which is
            // the E8 CALL just BEFORE the blend-mode write. Scan backwards.
            let mut shock_helper: Option<*const u8> = None;
            for i in (0..blend_off).rev() {
                if body[i] == 0xE8 {
                    shock_helper = Some(decode_call_rel32(render_notes.add(i)));
                    break;
                }
            }
            if let Some(helper) = shock_helper {
                // Scan the helper's first ~128 bytes for MOV RAX, [RIP+disp32]
                // (48 8B 05 <disp32>) that loads the default shader global.
                let helper_body = std::slice::from_raw_parts(helper, 128);
                for i in 0..helper_body.len() - 7 {
                    if helper_body[i] == 0x48
                        && helper_body[i + 1] == 0x8B
                        && helper_body[i + 2] == 0x05
                    {
                        let shader_global = decode_rip_relative(helper.add(i + 3));
                        let off = shader_global.offset_from(self.base) as usize;
                        if off < self.size {
                            self.resolved.insert("default_shader".into(), shader_global);
                            log_info!(
                                "  [+] default_shader (derived from shock-arrow helper) @ +0x{:X}",
                                off,
                            );
                        }
                        break;
                    }
                }
            } else {
                log_warn!(
                    "  [-] default_shader -- shock-arrow helper CALL not found before blend write"
                );
            }
        }
    }

    /// Derive `layer_table` (the render layer-table global) from the
    /// `layer_dispatcher` signature: the dispatcher's first RIP-relative
    /// load (`48 8B 15 <disp32>` at match+10, operand at +13) reads the
    /// table pointer. The overlay-draw animated-background emitter detours
    /// the dispatcher and replicates its per-entry walk conditions to pick
    /// the widget layer's list. RE: docs/overlay_draw_research.md.
    fn derive_layer_table(&mut self) {
        let dispatcher = match self.get_address("layer_dispatcher") {
            Some(a) => a,
            None => return,
        };
        unsafe {
            let body = std::slice::from_raw_parts(dispatcher, 16);
            // Structural check: the load must be exactly where the pattern
            // fixed it (48 8B 15 at +10).
            if body[10] != 0x48 || body[11] != 0x8B || body[12] != 0x15 {
                log_warn!("  [-] layer_table -- dispatcher prologue shape unexpected");
                return;
            }
            let global = decode_rip_relative(dispatcher.add(13));
            let off = global.offset_from(self.base) as usize;
            if off < self.size {
                self.resolved.insert("layer_table".into(), global);
                log_info!(
                    "  [+] layer_table (derived from layer_dispatcher) @ +0x{:X}",
                    off
                );
            } else {
                log_warn!("  [-] layer_table -- derived global outside module");
            }
        }
    }

    /// Derive `player_work_table` from the short accessor function anchored
    /// by `player_work_table_anchor`.
    /// The anchor's first instruction is `MOV RAX, [RIP+disp32]` loading
    /// the table pointer for slot 0, with the RIP-relative operand at
    /// offset +3 into the anchor. Decoding that operand yields the
    /// address of the table global itself (whose entries are 8-byte
    /// wrapper pointers, indexed by playSide).
    fn derive_player_work_table(&mut self) {
        let anchor = match self.get_address("player_work_table_anchor") {
            Some(a) => a,
            None => {
                log_warn!("  [-] player_work_table -- anchor not resolved");
                return;
            }
        };

        unsafe {
            // Sanity-check the three-byte MOV opcode prefix at the anchor.
            if *anchor != 0x48 || *anchor.add(1) != 0x8B || *anchor.add(2) != 0x05 {
                log_warn!(
                    "  [-] player_work_table -- unexpected opcode at anchor ({:02X} {:02X} {:02X})",
                    *anchor,
                    *anchor.add(1),
                    *anchor.add(2)
                );
                return;
            }
            let table = decode_rip_relative(anchor.add(3));
            let off = table.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!(
                    "  [-] player_work_table -- derived address {:p} lies outside module",
                    table
                );
                return;
            }
            self.resolved.insert("player_work_table".into(), table);
            log_info!("  [+] player_work_table (derived) @ +0x{:X}", off);
        }
    }

    /// Resolve `max_stage_global` — the operator's
    /// `/gameOptions/max_stage/current` cache (`DAT_18047E784` on 20260721),
    /// read once per session start inside `createNextSequence` case 7:
    ///
    ///   LEA RDX,[global]                       ; 48 8D 15 d32 — the out-pointer
    ///   LEA RCX,["/gameOptions/max_stage/current"]  ; 48 8D 0D d32
    ///   CALL [avs property read]
    ///
    /// Anchored on the unique string bytes, then the (single) RIP-relative
    /// `LEA RCX` xref, then the `LEA RDX` immediately before it. Fails closed
    /// on any ambiguity. Consumed by stage_records' session-state decode
    /// (the quick-fail fast path's session-continues predicate).
    fn derive_max_stage_global(&mut self) {
        const STRING_PATTERN: &str =
            "2F 67 61 6D 65 4F 70 74 69 6F 6E 73 2F 6D 61 78 5F 73 74 61 67 65 2F 63 75 72 72 65 6E 74 00";

        let hits = scan_pattern_all(self.base, self.size, STRING_PATTERN);
        if hits.len() != 1 {
            log_warn!(
                "  [-] max_stage_global -- expected 1 string match, found {}",
                hits.len()
            );
            return;
        }
        let string_addr = hits[0].address;

        unsafe {
            // All RIP-relative LEAs targeting the string, filtered to RCX
            // (ModRM 0x0D) — the property-key argument load.
            let leas: Vec<*const u8> =
                crate::core::scanner::scan_lea_xrefs_to(self.base, self.size, string_addr)
                    .into_iter()
                    .filter(|lea| *lea.add(2) == 0x0D)
                    .collect();
            if leas.len() != 1 {
                log_warn!(
                    "  [-] max_stage_global -- expected 1 LEA RCX xref to the key string, found {}",
                    leas.len()
                );
                return;
            }
            let lea_rcx = leas[0];

            // The out-pointer LEA RDX (48 8D 15 d32) sits immediately before.
            let lea_rdx = lea_rcx.sub(7);
            if *lea_rdx != 0x48 || *lea_rdx.add(1) != 0x8D || *lea_rdx.add(2) != 0x15 {
                log_warn!(
                    "  [-] max_stage_global -- expected LEA RDX before the key LEA ({:02X} {:02X} {:02X})",
                    *lea_rdx,
                    *lea_rdx.add(1),
                    *lea_rdx.add(2)
                );
                return;
            }
            let global = decode_rip_relative(lea_rdx.add(3));
            let off = global.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!(
                    "  [-] max_stage_global -- derived address {:p} lies outside module",
                    global
                );
                return;
            }
            self.resolved.insert("max_stage_global".into(), global);
            log_info!("  [+] max_stage_global (derived) @ +0x{:X}", off);
        }
    }

    /// Derive `shutter_actor_global` — the ShutterActor singleton pointer
    /// global (`DAT_1806f2d40` on 20260721) — from the `shutter_close_request`
    /// wrapper's `MOV RBX,[rip+d32]` at match+9. The opcode bytes are literal
    /// in the pattern, so only the RIP decode + a bounds check remain here.
    /// Consumed by the quick-restart/fail bannerless fast path.
    fn derive_shutter_actor_global(&mut self) {
        let wrapper = match self.get_address("shutter_close_request") {
            Some(w) => w,
            None => {
                log_warn!("  [-] shutter_actor_global -- shutter_close_request not resolved");
                return;
            }
        };
        unsafe {
            // match+9: 48 8B 1D d32 (MOV RBX,[rip+d32]); d32 at match+12.
            let global = decode_rip_relative(wrapper.add(12));
            let off = global.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!(
                    "  [-] shutter_actor_global -- derived address {:p} lies outside module",
                    global
                );
                return;
            }
            self.resolved.insert("shutter_actor_global".into(), global);
            log_info!("  [+] shutter_actor_global (derived) @ +0x{:X}", off);
        }
    }

    /// Derive `selectmusic_model` from `selectmusic_model_anchor`.
    ///
    /// The anchor is the MusicCard tick's `MOV R11,[rip+d32]` (d32 at
    /// match+3) whose following instructions read the highlighted-song
    /// shared_ptr at `[R11+0x1B8]` / `[R11+0x1B0]` — those offsets are
    /// pinned as immediate bytes in the pattern, so a successful match
    /// certifies both the global and the +0x1B0 object-slot layout.
    /// Consumer: music_wheel_song_length (selection-changed polling).
    fn derive_selectmusic_model(&mut self) {
        let anchor = match self.get_address("selectmusic_model_anchor") {
            Some(a) => a,
            None => {
                log_warn!("  [-] selectmusic_model -- anchor not resolved");
                return;
            }
        };
        unsafe {
            let global = decode_rip_relative(anchor.add(3));
            let off = global.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!(
                    "  [-] selectmusic_model -- derived address {:p} lies outside module",
                    global
                );
                return;
            }
            // The highlighted-song shared_ptr slot moved between builds
            // (0x190 pre-20260324, 0x1B0 after): decode it from the two
            // pinned-shape loads and require the ctrl/obj adjacency.
            let ctrl_disp = (anchor.add(26) as *const u32).read_unaligned() as usize;
            let obj_disp = (anchor.add(33) as *const u32).read_unaligned() as usize;
            if ctrl_disp != obj_disp + 8 || !(0x40..=0x1000).contains(&obj_disp) {
                log_warn!(
                    "  [-] selectmusic_model -- highlight slot disps +0x{:X}/+0x{:X} out of shape; refusing to derive",
                    obj_disp,
                    ctrl_disp
                );
                return;
            }
            self.resolved.insert("selectmusic_model".into(), global);
            self.resolved
                .insert("selectmusic_highlight_slot".into(), self.base.add(obj_disp));
            log_info!(
                "  [+] selectmusic_model (derived) @ +0x{:X} (highlight slot model+0x{:X})",
                off,
                obj_disp
            );
        }
    }

    /// Offset of the highlighted-song `shared_ptr` (obj at +0, ctrl at +8)
    /// inside the select-music model object; `None` when
    /// `selectmusic_model` did not derive. 0x1B0 on 20260324+, 0x190 on
    /// 20250805 / 20260224.
    pub fn selectmusic_highlight_slot(&self) -> Option<usize> {
        self.get_address("selectmusic_highlight_slot")
            .map(|p| (p as usize).wrapping_sub(self.base as usize))
    }

    /// Derive `frame_tick_global` from `dps_timing_anchor_site`.
    ///
    /// The site is `MOV RAX,[rip+d32]` (d32 at match+3) followed by the
    /// `[RAX+0x1268]` frame-tick read that DPS state 6 broadcasts as the
    /// msg-0x1044 timing anchor. The pattern matches once on 20260616 /
    /// 20260721 and twice on 20250805 (a second state machine shares the
    /// shape); every match must decode to the SAME global or the
    /// derivation refuses — the in-place reset must never anchor the music
    /// clock to the wrong time source.
    fn derive_frame_tick_global(&mut self) {
        let matches = self.get_all_matches("dps_timing_anchor_site");
        if matches.is_empty() {
            log_warn!("  [-] frame_tick_global -- dps_timing_anchor_site not resolved");
            return;
        }
        unsafe {
            let mut global: Option<*const u8> = None;
            for m in &matches {
                let g = decode_rip_relative(m.add(3));
                match global {
                    None => global = Some(g),
                    Some(prev) if prev != g => {
                        log_warn!(
                            "  [-] frame_tick_global -- anchor sites disagree ({:p} vs {:p}); refusing",
                            prev,
                            g
                        );
                        return;
                    }
                    _ => {}
                }
            }
            let global = global.unwrap();
            let off = global.offset_from(self.base) as usize;
            if off >= self.size {
                log_warn!(
                    "  [-] frame_tick_global -- derived address {:p} lies outside module",
                    global
                );
                return;
            }
            self.resolved.insert("frame_tick_global".into(), global);
            log_info!(
                "  [+] frame_tick_global (derived from {} anchor site(s)) @ +0x{:X}",
                matches.len(),
                off
            );
        }
    }

    /// Verify `input_tick_function` (the input manager's per-frame tick
    /// function) against `frame_tick_global` and publish `input_tick_store`
    /// = the address of its tail `MOV [RBP+0x1268],RAX`.
    ///
    /// Two checks, both required (a hit on all four builds proves nothing
    /// about what the consumer reads): the `MOV RBP,[rip+d32]` at match+0x13
    /// must decode to the derived `frame_tick_global` (the same global every
    /// other clock consumer dereferences), and the function tail — found by
    /// scanning ≤ 0x400 bytes for `FF 15 ?? ?? ?? ?? 48 89 85 68 12 00 00
    /// 48 83 C4 28 41 5F` — must exist exactly once. The audio clock detours
    /// the function ENTRY (post-original: read `*(global)+0x1268` and QPC),
    /// so the store site is published for the boot log / sweep only.
    fn derive_input_tick_function(&mut self) {
        // (global-load disp offset, tail pattern) per shape.
        const V2: (usize, &str) = (
            0x16,
            "FF 15 ?? ?? ?? ?? 48 89 85 68 12 00 00 48 83 C4 28 41 5F",
        );
        const V1: (usize, &str) = (
            0x12,
            "FF 15 ?? ?? ?? ?? 49 89 84 24 68 12 00 00 48 83 C4 20 41 5E",
        );
        let primary = self.get_all_matches("input_tick_function");
        let v1 = self.get_all_matches("input_tick_function_v1");
        self.resolved.remove("input_tick_function");
        self.resolved.remove("input_tick_function_v1");
        let (entry, shape, label) = match (primary.as_slice(), v1.as_slice()) {
            ([entry], []) => (*entry, V2, "20260224+"),
            ([], [entry]) => (*entry, V1, "v1 (20250805)"),
            ([], []) => {
                log_warn!("  [-] input_tick_function -- no shape matched");
                return;
            }
            _ => {
                log_warn!(
                    "  [-] input_tick_function -- expected exactly one match, found {} (v1: {}); refusing",
                    primary.len(),
                    v1.len()
                );
                return;
            }
        };
        let Some(global) = self.get_address("frame_tick_global") else {
            log_warn!("  [-] input_tick_function -- frame_tick_global unresolved; refusing");
            return;
        };
        unsafe {
            let loaded = decode_rip_relative(entry.add(shape.0));
            if loaded != global {
                log_warn!(
                    "  [-] input_tick_function -- input-state global {:p} != frame_tick_global {:p}; refusing",
                    loaded,
                    global
                );
                return;
            }
            let end = (self.base as usize + self.size).min(entry as usize + 0x400);
            let len = end.saturating_sub(entry as usize);
            let tails = scan_pattern_all(entry, len, shape.1);
            let [tail] = tails.as_slice() else {
                log_warn!(
                    "  [-] input_tick_function -- tail store `MOV [reg+0x1268],RAX` found {} times; refusing",
                    tails.len()
                );
                return;
            };
            let store = entry.add(tail.offset + 6);
            self.resolved.insert("input_tick_function".into(), entry);
            self.resolved.insert("input_tick_store".into(), store);
            log_info!(
                "  [+] input_tick_function -- {} shape @ +0x{:X}",
                label,
                entry.offset_from(self.base)
            );
            log_info!(
                "  [+] input_tick_store (derived, input_tick_function tail) @ +0x{:X}",
                store.offset_from(self.base)
            );
        }
    }

    /// Custom Resolution anchors (mods/custom_resolution), computed from the
    /// LINEAR hits only so the mod can use them inside `early_apply` (before
    /// `resolve_derived`); [`Self::derive_custom_resolution`] publishes the
    /// same values into the store for the boot log / sweep. Anchors:
    /// `fps_target_imm32` (onBoot), `display_backbuffer_dims`. Fields:
    /// - `aa_config_imm`: onBoot's `MOV dword [RSP+d],3` AA-config store at
    ///   fps+0x69 (imm at +0x6D) — forced to 0 for the 4:3 plan only.
    /// - `graphics_init`: the `CALL rel32` right after that store (the
    ///   display-struct consumer; the mod's post-init fixup detour).
    /// - `render_surfaces_global`: onBoot's first `MOV RCX,[RIP+d]` after the
    ///   graphics_init CALL loads the 0x170-byte render-surface object
    ///   (DAT_1806f1ef0 on 20260616; PRESENT rt struct at +0x80).
    /// - `screen_w_global` / `screen_h_global`: RIP targets of the HD-branch
    ///   stores in `display_backbuffer_dims` (match+11 / +21).
    /// Every field is independent (`None` = shape not found). (The former
    /// `surface_create` / `present_depth_release` / `present_depth_addref`
    /// anchors served the render < output depth swap, removed 2026-09-09
    /// with the render knob; the RE stays in `docs/custom_resolution.md`.)
    pub fn custom_resolution_anchors(&self) -> CustomResolutionAnchors {
        let mut a = CustomResolutionAnchors::default();
        let in_module = |p: *const u8| -> bool {
            (p as usize) >= (self.base as usize) && (p as usize) < (self.base as usize + self.size)
        };

        // ── onBoot: AA imm, graphics_init, render_surfaces_global ──
        if let Some(fps) = self.get_address("fps_target_imm32") {
            const AA_OFF: usize = 0x69;
            const SCAN_START: usize = 0x71;
            const SCAN_LEN: usize = 0x20;
            let body = unsafe { std::slice::from_raw_parts(fps, 0x100) };
            // Stock imm is 3; custom_resolution may already have written 0
            // by the time `resolve_derived` re-derives this (the patch layer
            // checks the stock value itself before writing).
            if body[AA_OFF] == 0xC7
                && body[AA_OFF + 1] == 0x44
                && body[AA_OFF + 2] == 0x24
                && (body[AA_OFF + 4..AA_OFF + 8] == [0x03, 0x00, 0x00, 0x00]
                    || body[AA_OFF + 4..AA_OFF + 8] == [0x00, 0x00, 0x00, 0x00])
            {
                a.aa_config_imm = Some(unsafe { fps.add(AA_OFF + 4) });
            }
            if let Some(i) = (SCAN_START..SCAN_START + SCAN_LEN).find(|&i| body[i] == 0xE8) {
                unsafe {
                    let target = decode_call_rel32(fps.add(i));
                    if in_module(target) {
                        a.graphics_init = Some(target);
                    }
                    for j in (i + 5)..(i + 5 + 0x60).min(body.len() - 7) {
                        if body[j] == 0x48 && body[j + 1] == 0x8B && body[j + 2] == 0x0D {
                            let g = decode_rip_relative(fps.add(j + 3));
                            if in_module(g) {
                                a.render_surfaces_global = Some(g);
                            }
                            break;
                        }
                    }
                }
            }
        }

        // ── screen globals ──
        if let Some(m) = self.get_address("display_backbuffer_dims") {
            // `C7 05 <disp32> <imm32>`: RIP is the END of the instruction, i.e.
            // 4 bytes (the imm32) past what `decode_rip_relative` assumes.
            let w = unsafe { decode_rip_relative(m.add(11)).add(4) };
            let h = unsafe { decode_rip_relative(m.add(21)).add(4) };
            if in_module(w) {
                a.screen_w_global = Some(w);
            }
            if in_module(h) {
                a.screen_h_global = Some(h);
            }
        }
        a
    }

    /// Publish [`Self::custom_resolution_anchors`] into the store (boot log +
    /// sweep visibility). One `[+]`/`[-]` line per anchor.
    fn derive_custom_resolution(&mut self) {
        let a = self.custom_resolution_anchors();
        for (name, v, from) in [
            ("aa_config_imm", a.aa_config_imm, "fps_target_imm32"),
            ("graphics_init", a.graphics_init, "fps_target_imm32"),
            ("render_surfaces_global", a.render_surfaces_global, "onBoot"),
            (
                "screen_w_global",
                a.screen_w_global,
                "display_backbuffer_dims",
            ),
            (
                "screen_h_global",
                a.screen_h_global,
                "display_backbuffer_dims",
            ),
        ] {
            match v {
                Some(p) => {
                    self.resolved.insert(name.into(), p);
                    log_info!(
                        "  [+] {} (derived from {}) @ +0x{:X}",
                        name,
                        from,
                        (p as usize).wrapping_sub(self.base as usize)
                    );
                }
                None => log_warn!("  [-] {} -- shape not found (from {})", name, from),
            }
        }
    }

    /// Find the gauge-actor family + ScoreActor + ControlMessageActor +
    /// NoteResultActor vtables via RTTI. The in-place song reset restores
    /// each side's gauge child by matching its vtable against this set
    /// (value at gauge+0x90, latches +0xB8/+0xD9) and resets the
    /// ScoreActor's displayed-score sentinel (+0x6C = -1 → full digit
    /// repaint) — see
    /// .agents/planning/20260812-inplace-restart/research/run_state_re.md
    /// §5. The ControlMessageActor vtable identifies each side's
    /// end-cascade child for the seek clamp (chart_end_raw at +0x98,
    /// StackStep — training research §4.3). The NoteResultActor vtable
    /// identifies each side's judge-display child, whose
    /// `dance_score_compare` clip (+0xB0) is the pacemaker readout: the
    /// reset rewinds it out of the msg-0x103A "out" outro (a one-way
    /// latch for the actor's lifetime — the msg-0x1036 update refuses
    /// once the clip's frame reaches the "out" label), and the pacemaker
    /// ms-error swap vtable-guards its visibility write against it. All
    /// are resolved fail-open per class; `song_reset` gates itself on the
    /// sets it needs.
    fn find_gauge_vtables(&mut self) {
        for (rtti, name) in [
            (
                ".?AVNormalGaugeActor@dance@sequence@@",
                "normal_gauge_vtable",
            ),
            (".?AVGradeGaugeActor@dance@sequence@@", "grade_gauge_vtable"),
            (".?AVLifeGaugeActor@dance@sequence@@", "life_gauge_vtable"),
            (".?AVFlareGaugeActor@dance@sequence@@", "flare_gauge_vtable"),
            (
                ".?AVImmortalGaugeActor@dance@sequence@@",
                "immortal_gauge_vtable",
            ),
            (".?AVScoreActor@dance@sequence@@", "score_actor_vtable"),
            (
                ".?AVControlMessageActor@dance@sequence@@",
                "control_message_actor_vtable",
            ),
            (
                ".?AVNoteResultActor@dance@sequence@@",
                "note_result_actor_vtable",
            ),
            (".?AVGhostActor@dance@sequence@@", "ghost_actor_vtable"),
        ] {
            if let Some(vtable) = self.find_vtable_by_rtti(rtti, name) {
                self.resolved.insert(name.into(), vtable);
                let offset = unsafe { vtable.offset_from(self.base) as usize };
                log_info!("  [+] {} (RTTI) @ +0x{:X}", name, offset);
            }
        }
    }

    /// Derive the judge-record rebuild trio (seek-to-T, training design
    /// §4.4) from `judge_rebuild_anchor` — the msg-0x1044 rewind worker's
    /// anchor stores. The FIRST call is pinned by its records-vector
    /// argument setup (`LEA RCX,[R12+0xB0]` immediately before the E8 —
    /// refuses on layout drift); the next two E8s are reserve and rebuild
    /// (the flash-renderer virtual call in between is `FF 50 10`, never
    /// E8; region verified byte-identical on 20260616/20260721). Fail-open:
    /// any check failing leaves the trio unresolved — nonzero-T seeks
    /// refuse, the shipped t=0 reset is unaffected.
    fn derive_judge_rebuild_trio(&mut self) {
        let anchor = match self.get_address("judge_rebuild_anchor") {
            Some(a) => a,
            None => {
                log_warn!("  [-] judge_rebuild_trio -- judge_rebuild_anchor not resolved");
                return;
            }
        };
        // `LEA RCX,[R12+0xB0]` — the records-vector argument load.
        const VECTOR_LEA: [u8; 8] = [0x49, 0x8D, 0x8C, 0x24, 0xB0, 0x00, 0x00, 0x00];
        // The trio calls sit at match+0x37/+0x5F/+0x93 on 20260616/20260721
        // (cabinet-verified 2026-08-13: a 0x60 window truncated the scan
        // after the second call); the NEXT unrelated call is at +0xE0, and
        // the scan stops at three targets anyway.
        const SCAN_LIMIT: usize = 0xC0;
        unsafe {
            // Call 1 (clear): the LEA+E8 pair.
            let mut clear_site: Option<*const u8> = None;
            for index in 0..SCAN_LIMIT {
                let at = anchor.add(index);
                if std::slice::from_raw_parts(at, VECTOR_LEA.len()) == VECTOR_LEA
                    && *at.add(VECTOR_LEA.len()) == 0xE8
                {
                    clear_site = Some(at.add(VECTOR_LEA.len()));
                    break;
                }
            }
            let Some(clear_site) = clear_site else {
                log_warn!(
                    "  [-] judge_rebuild_trio -- records-vector LEA+CALL pair not found (layout drift?)"
                );
                return;
            };
            // Calls 2 (reserve) and 3 (rebuild): the next two E8 sites,
            // skipping each call's own rel32 operand.
            let mut targets = vec![decode_call_rel32(clear_site)];
            let mut cursor = clear_site.add(5);
            let end = anchor.add(SCAN_LIMIT);
            while (cursor as usize) < end as usize && targets.len() < 3 {
                if *cursor == 0xE8 {
                    targets.push(decode_call_rel32(cursor));
                    cursor = cursor.add(5);
                } else {
                    cursor = cursor.add(1);
                }
            }
            if targets.len() < 3 {
                log_warn!("  [-] judge_rebuild_trio -- fewer than three calls after the anchor");
                return;
            }
            for (index, target) in targets.iter().enumerate() {
                let off = target.offset_from(self.base);
                if off < 0 || off as usize >= self.size {
                    log_warn!(
                        "  [-] judge_rebuild_trio -- call {} target {:p} lies outside module",
                        index,
                        target
                    );
                    return;
                }
            }
            if targets[0] == targets[1] || targets[1] == targets[2] || targets[0] == targets[2] {
                log_warn!("  [-] judge_rebuild_trio -- call targets are not distinct");
                return;
            }
            for (name, target) in [
                ("judge_rebuild_clear", targets[0]),
                ("judge_rebuild_reserve", targets[1]),
                ("judge_rebuild_rebuild", targets[2]),
            ] {
                self.resolved.insert(name.into(), target);
                log_info!(
                    "  [+] {} (derived) @ +0x{:X}",
                    name,
                    target.offset_from(self.base) as usize
                );
            }
        }
    }

    /// Resolve `row_builder_fn` — the 21-row OptionForm builder.
    /// Matched directly via `row_builder_fn_prologue` (unique 5-register
    /// save + ~0x1B00 __chkstk frame).
    fn derive_row_builder_fn(&mut self) {
        let entry = match self.get_address("row_builder_fn_prologue") {
            Some(e) => e,
            None => {
                log_warn!("  [-] row_builder_fn -- prologue not resolved");
                return;
            }
        };
        self.resolved.insert("row_builder_fn".into(), entry);
        let offset = unsafe { entry.offset_from(self.base) as usize };
        log_info!(
            "  [+] row_builder_fn (direct prologue match) @ +0x{:X}",
            offset
        );
    }

    /// Find `OptionTab::vftable` via RTTI. Same mechanism as `sprite_vtable`,
    /// `check_step_data_vtable`, `auto_foot_panel_vtable`.
    fn find_option_tab_vtable(&mut self) {
        let vtable = match self
            .find_vtable_by_rtti(".?AVOptionTab@selectmusic@sequence@@", "option_tab_vtable")
        {
            Some(v) => v,
            None => return,
        };
        self.resolved.insert("option_tab_vtable".into(), vtable);
        let offset = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] option_tab_vtable (RTTI) @ +0x{:X}", offset);
    }

    /// Derive a per-KIND `OptionElement<T>::ctor` + its primary vtable from
    /// the RTTI string naming the specialization.
    ///
    /// The ctor is found by RTTI-walking to one of the specialization's four
    /// vtables, scanning `.text` for `LEA reg, [RIP+disp32]` instructions
    /// whose disp32 resolves to that vtable, then walking each hit back to
    /// the MSVC function prologue (`48 89 4C 24 08` = `MOV [RSP+0x8], RCX`).
    /// Two LEAs reference the vtable — one in the ctor, one in the
    /// destructor — disambiguated by counting `E8` (CALL rel32) bytes
    /// between the prologue and the LEA: the ctor has ≥1 (the parent-class
    /// ctor call); the destructor has 0.
    ///
    /// The primary vtable is then derived from the ctor's canonical 7-byte
    /// LEA at ctor+0x49:
    ///
    /// ```text
    /// ctor+0x49: 48 8D 05 <disp32>   LEA RAX, [RIP + disp]   ; primary vtable
    /// ctor+0x50: 48 89 06            MOV [RSI], RAX          ; install at row+0x00
    /// ```
    ///
    /// The instruction layout is structurally invariant across toolchain
    /// drift for every `OptionElement<T>` specialization the game ships
    /// (verified cross-version on both the ArrowColor and int
    /// specializations); only the disp32 value moves between builds as
    /// `.rdata` shifts.
    fn derive_option_element_ctor(
        &mut self,
        rtti_name: &str,
        ctor_sig_name: &str,
        vtable_sig_name: &str,
    ) {
        let vtable_ref = match self.find_vtable_by_rtti(rtti_name, ctor_sig_name) {
            Some(v) => v,
            None => return,
        };

        let mut lea_hits: Vec<*const u8> = Vec::new();
        unsafe {
            let text = std::slice::from_raw_parts(self.base, self.size);
            for i in 0..text.len().saturating_sub(7) {
                let rex = text[i];
                if rex != 0x48 && rex != 0x4C {
                    continue;
                }
                if text[i + 1] != 0x8D {
                    continue;
                }
                let modrm = text[i + 2];
                if (modrm & 0xC7) != 0x05 {
                    continue;
                }
                let disp = i32::from_le_bytes([text[i + 3], text[i + 4], text[i + 5], text[i + 6]]);
                let instr_addr = self.base.add(i);
                let tgt = instr_addr.add(7).offset(disp as isize);
                if tgt == vtable_ref {
                    lea_hits.push(instr_addr);
                }
            }
        }

        if lea_hits.is_empty() {
            log_warn!("  [-] {ctor_sig_name} -- no LEA to vtable found in .text");
            return;
        }

        let mut ctor_entry: Option<*const u8> = None;
        unsafe {
            for lea in &lea_hits {
                let max_back = 0x400usize;
                let mut prologue: Option<*const u8> = None;
                for back in 5..max_back {
                    let p = lea.sub(back);
                    if p < self.base {
                        break;
                    }
                    if *p == 0x48
                        && *p.add(1) == 0x89
                        && *p.add(2) == 0x4C
                        && *p.add(3) == 0x24
                        && *p.add(4) == 0x08
                    {
                        prologue = Some(p);
                        break;
                    }
                }

                let prologue = match prologue {
                    Some(p) => p,
                    None => continue,
                };

                // Byte-level E8 count in [prologue, lea). Sufficient here
                // because the prologue/setup region is structurally
                // well-formed and E8 bytes don't occur as operands in the
                // specific instruction shapes that populate this region.
                let span = (*lea as usize) - (prologue as usize);
                let region = std::slice::from_raw_parts(prologue, span);
                let e8_count = region.iter().filter(|&&b| b == 0xE8).count();

                if e8_count >= 1 {
                    ctor_entry = Some(prologue);
                    break;
                }
            }
        }

        let ctor = match ctor_entry {
            Some(a) => a,
            None => {
                log_warn!(
                    "  [-] {ctor_sig_name} -- no CALL-bearing prologue found (ctor heuristic failed)"
                );
                return;
            }
        };

        self.resolved.insert(ctor_sig_name.into(), ctor);
        let offset = unsafe { ctor.offset_from(self.base) as usize };
        log_info!(
            "  [+] {ctor_sig_name} (RTTI + ctor/dtor disambig) @ +0x{:X}",
            offset
        );

        // Now derive the primary vtable from the canonical LEA at ctor+0x49.
        unsafe {
            let lea = ctor.add(0x49);
            if *lea != 0x48 || *lea.add(1) != 0x8D || *lea.add(2) != 0x05 {
                log_warn!(
                    "  [-] {vtable_sig_name} -- expected LEA RAX,[RIP+disp32] at ctor+0x49, got {:02X} {:02X} {:02X}",
                    *lea,
                    *lea.add(1),
                    *lea.add(2)
                );
                return;
            }
            let vtable = decode_rip_relative(lea.add(3));
            let voffset = vtable.offset_from(self.base) as usize;
            if voffset >= self.size {
                log_warn!(
                    "  [-] {vtable_sig_name} -- derived address {:p} lies outside module",
                    vtable
                );
                return;
            }
            self.resolved.insert(vtable_sig_name.into(), vtable);
            log_info!(
                "  [+] {vtable_sig_name} (derived from ctor+0x49 LEA) @ +0x{:X}",
                voffset
            );
        }
    }

    /// Derive `string_assign` (MSVC `std::basic_string::assign(const char*,
    /// size_t)`) from xrefs to `metadata_insert`.
    ///
    /// Pair-locality derivation: the row builder's per-tag caller sequence is
    ///
    /// ```text
    /// CALL string_assign    ; RCX=&stack_str, RDX=literal, R8=len
    /// NOP                   ; (optional)
    /// LEA  RDX, [RBP+buf]
    /// MOV  RCX, <row_ptr>
    /// CALL metadata_insert  ; 0x10 bytes after the string_assign CALL
    /// ```
    ///
    /// Every xref to `metadata_insert` has a `CALL string_assign` at
    /// exactly 0x10 bytes prior; decoding that CALL's disp32 yields the
    /// string_assign entry point. Pair-locality is preferred over a direct
    /// AOB scan on string_assign because three overloads of MSVC's
    /// `basic_string::assign` share identical prologue bytes.
    fn derive_string_assign_via_pair(&mut self) {
        let metadata_insert = match self.get_address("metadata_insert") {
            Some(a) => a,
            None => {
                log_warn!("  [-] string_assign -- metadata_insert not resolved");
                return;
            }
        };

        let call_sites = self.xrefs_to(metadata_insert);
        if call_sites.is_empty() {
            log_warn!("  [-] string_assign -- no xrefs to metadata_insert found");
            return;
        }

        // Expected layout at each xref:
        //   site - 0x10  E8 <disp32>         CALL string_assign  (5 bytes)
        //   site - 0x0B  90                  NOP                 (1 byte)
        //   site - 0x0A  48 8D 55 ??          LEA RDX,[RBP+??]   (4 bytes)
        //   site - 0x06  ...                 setup of RCX for metadata_insert
        //   site         E8 <disp32>         CALL metadata_insert
        //
        // Try each xref until one matches the CALL shape exactly; take the
        // first successful decode as the definitive target.
        unsafe {
            for &site in &call_sites {
                let call_site = site.sub(0x10);
                if call_site < self.base {
                    continue;
                }
                if *call_site != 0xE8 {
                    continue;
                }
                let target = decode_call_rel32(call_site);
                let tgt_offset = target.offset_from(self.base) as usize;
                if tgt_offset >= self.size {
                    continue;
                }
                self.resolved.insert("string_assign".into(), target);
                log_info!(
                    "  [+] string_assign (derived from metadata_insert xref pair-locality) @ +0x{:X}",
                    tgt_offset
                );
                return;
            }
        }

        log_warn!("  [-] string_assign -- no metadata_insert xref had a CALL at -0x10 offset");
    }

    /// Derive three MSVC `_Impl_no_alloc0` vtable slots the
    /// `event_register` mechanism expects from mod-authored lambdas.
    ///
    /// These addresses are not AOB-scannable individually (the bodies are
    /// compiler-emitted one-liners that match thousands of sites in the
    /// binary), so the derivation rides on an already-verified RIP chain
    /// whose every link is structurally invariant:
    ///
    /// ```text
    /// option_element_arrowcolor_primary_vtable (LEA-derived from ctor+0x49)
    ///   └── [4]  → FUN_180173c10 (native advanceValue)
    ///        └── +0x2B = E8 <disp32> CALL → FUN_18017dc40 (register left/right)
    ///             └── +0x0D = 48 8D 05 <disp32> LEA → lambda232_vtable
    ///                  ├── [3]  → lambda_destruct_slot3
    ///                  ├── [4]  → lambda_release_slot4
    ///                  └── [5]  → lambda_get_captured_slot5
    /// ```
    ///
    /// Slot 0 (copy constructor) is intentionally NOT derived here. The
    /// native slot-0 body hardcodes the lambda232 vtable as the
    /// destination's initial vtable — fine for native lambdas, fatal for
    /// mod-authored lambdas, because the heap-copied registration would
    /// end up invoking lambda232's native value-list walker instead of
    /// our direction-specific trampoline. Mod code provides its own copy
    /// trampoline that preserves whatever vtable the source lambda holds.
    fn derive_event_lambda_vtable_slots(&mut self) {
        let primary_vtable = match self.get_address("option_element_arrowcolor_primary_vtable") {
            Some(a) => a,
            None => {
                log_warn!("  [-] event_lambda_vtable_slots -- primary vtable not resolved");
                return;
            }
        };

        unsafe {
            // Slot 4 of the primary vtable = FUN_180173c10 (native advanceValue).
            let advance_value = *(primary_vtable.add(4 * 8) as *const *const u8);
            let ofs = advance_value.offset_from(self.base) as usize;
            if ofs >= self.size {
                log_warn!(
                    "  [-] event_lambda_vtable_slots -- primary vtable slot 4 {:p} outside module",
                    advance_value
                );
                return;
            }

            // +0x2B inside FUN_180173c10: E8 <disp32> CALL FUN_18017dc40.
            let call_site = advance_value.add(0x2B);
            if *call_site != 0xE8 {
                log_warn!(
                    "  [-] event_lambda_vtable_slots -- expected E8 CALL at advanceValue+0x2B, got {:02X}",
                    *call_site
                );
                return;
            }
            let register_lambdas = decode_call_rel32(call_site);

            // +0x0D inside FUN_18017dc40: 48 8D 05 <disp32> LEA RAX, [lambda232_vtable].
            let lea = register_lambdas.add(0x0D);
            if *lea != 0x48 || *lea.add(1) != 0x8D || *lea.add(2) != 0x05 {
                log_warn!(
                    "  [-] event_lambda_vtable_slots -- expected 48 8D 05 LEA at register_lambdas+0x0D, got {:02X} {:02X} {:02X}",
                    *lea, *lea.add(1), *lea.add(2)
                );
                return;
            }
            let lambda_vtable = decode_rip_relative(lea.add(3));
            let lv_ofs = lambda_vtable.offset_from(self.base) as usize;
            if lv_ofs >= self.size {
                log_warn!(
                    "  [-] event_lambda_vtable_slots -- lambda vtable {:p} outside module",
                    lambda_vtable
                );
                return;
            }

            let slots = [
                ("lambda_destruct_slot3", 3usize),
                ("lambda_release_slot4", 4usize),
                ("lambda_get_captured_slot5", 5usize),
            ];
            for &(name, idx) in &slots {
                let addr = *(lambda_vtable.add(idx * 8) as *const *const u8);
                let off = addr.offset_from(self.base) as usize;
                if off >= self.size {
                    log_warn!("  [-] {} -- slot {} {:p} outside module", name, idx, addr);
                    continue;
                }
                self.resolved.insert(name.into(), addr);
                log_info!(
                    "  [+] {} (derived via lambda232_vtable[{}]) @ +0x{:X}",
                    name,
                    idx,
                    off
                );
            }
        }
    }

    /// Resolve `textlayer_bind` — binds a TextLayer to a parent MC path.
    ///
    /// Preferred: direct prologue match (`textlayer_bind_direct`, 20260526+).
    /// Legacy fallback: anchor at fn+0x33 (`textlayer_bind_anchor`).
    /// Publish the pre-20260324 (`_v1`) prologue matches of the S-Marvelous
    /// results-side detour targets under their primary names when the
    /// primary AOB missed. The function bodies and every object-layout
    /// offset the detours consume were verified decompile-identical on
    /// 20250805 and 20260224 (see the `_v1` signature comments), so the
    /// consumers stay build-agnostic. Exactly one of the pair must match.
    fn derive_smarvelous_results_fallbacks(&mut self) {
        for (primary, v1) in [
            ("playdata_row_write", "playdata_row_write_v1"),
            ("result_window_build", "result_window_build_v1"),
            ("total_result_populate", "total_result_populate_v1"),
        ] {
            match (self.get_address(primary), self.get_address(v1)) {
                (Some(_), Some(_)) => {
                    // Both shapes present would mean one of them drifted
                    // onto an unrelated function — refuse rather than guess.
                    self.resolved.remove(primary);
                    log_warn!(
                        "  [-] {} -- both the 20260324+ and the v1 prologue matched; refusing to publish",
                        primary
                    );
                }
                (None, Some(addr)) => {
                    let off = (addr as usize).wrapping_sub(self.base as usize);
                    self.resolved.insert(primary.into(), addr);
                    log_info!("  [+] {} (via v1 prologue) @ +0x{:X}", primary, off);
                }
                _ => {}
            }
        }
    }

    fn derive_textlayer_bind(&mut self) {
        if let Some(entry) = self.get_address("textlayer_bind_direct") {
            self.resolved.insert("textlayer_bind".into(), entry);
            let offset = unsafe { entry.offset_from(self.base) as usize };
            log_info!(
                "  [+] textlayer_bind (direct prologue match) @ +0x{:X}",
                offset
            );
            return;
        }

        let anchor = match self.get_address("textlayer_bind_anchor") {
            Some(a) => a,
            None => {
                log_warn!("  [-] textlayer_bind -- anchor not resolved");
                return;
            }
        };

        unsafe {
            let fn_entry = anchor.sub(0x33);
            if fn_entry < self.base {
                log_warn!("  [-] textlayer_bind -- derived fn entry lies before module base");
                return;
            }
            self.resolved.insert("textlayer_bind".into(), fn_entry);
            let offset = fn_entry.offset_from(self.base) as usize;
            log_info!(
                "  [+] textlayer_bind (derived from anchor - 0x33) @ +0x{:X}",
                offset
            );
        }
    }

    /// Derive `customize_offset` — the byte offset of `ddr::player::Customize`
    /// within `PlayerWork`. Detected via RTTI walk: find the Customize vtable,
    /// scan for the LEA that loads its address, then read the displacement
    /// from the preceding `LEA RCX, [RDI + disp32]` which addresses the
    /// Customize sub-object within PlayerWork.
    ///
    /// The result is stored as a pointer whose numeric value IS the offset
    /// (e.g., `0x1790 as *const u8`). Consumers cast it to `usize`.
    fn derive_customize_offset(&mut self) {
        let vtable =
            match self.find_vtable_by_rtti(".?AVCustomize@player@ddr@@", "customize_vtable") {
                Some(v) => v,
                None => return,
            };

        let vt_off = unsafe { vtable.offset_from(self.base) as usize };
        log_info!("  [+] customize_vtable (RTTI) @ +0x{:X}", vt_off);

        unsafe {
            let text = std::slice::from_raw_parts(self.base, self.size);

            // Scan for LEA reg, [RIP+disp32] instructions that resolve to the vtable.
            // Encoding: 48/4C 8D (ModRM & 0xC7 == 0x05) <disp32>
            for i in 0..text.len().saturating_sub(7) {
                let rex = text[i];
                if rex != 0x48 && rex != 0x4C {
                    continue;
                }
                if text[i + 1] != 0x8D {
                    continue;
                }
                let modrm = text[i + 2];
                if (modrm & 0xC7) != 0x05 {
                    continue;
                }
                let disp = i32::from_le_bytes([text[i + 3], text[i + 4], text[i + 5], text[i + 6]]);
                let instr_end = self.base.add(i + 7);
                let tgt = instr_end.offset(disp as isize);
                if tgt != vtable {
                    continue;
                }

                // Found LEA that loads the Customize vtable. Two patterns
                // are known for how the compiler stores it into PlayerWork:
                //
                // Pattern A (older builds): LEA RCX,[reg+disp32] immediately
                //   BEFORE the vtable LEA, then MOV [RCX],RAX after.
                //   Encoding: 48 8D 8F/8B/8E/89 <disp32> (7 bytes before)
                //
                // Pattern B (20260526+): MOV [reg+disp32],RAX immediately
                //   AFTER the vtable LEA — compiler folded the LEA+MOV into
                //   a single store.
                //   Encoding: 48 89 87/83/86/85 <disp32> (7 bytes after)

                // Try Pattern A: LEA RCX,[reg+disp32] before vtable LEA
                if i >= 7 {
                    let prev = i - 7;
                    if text[prev] == 0x48 && text[prev + 1] == 0x8D {
                        let prev_modrm = text[prev + 2];
                        // mod=10 (disp32), reg=001 (RCX), rm=any base reg
                        if (prev_modrm & 0xC0) == 0x80 && (prev_modrm & 0x38) == 0x08 {
                            let offset = u32::from_le_bytes([
                                text[prev + 3],
                                text[prev + 4],
                                text[prev + 5],
                                text[prev + 6],
                            ]) as usize;
                            if offset >= 0x1000 && offset <= 0x4000 {
                                self.resolved
                                    .insert("customize_offset".into(), offset as *const u8);
                                log_info!(
                                    "  [+] customize_offset (derived from ctor LEA) = 0x{:X}",
                                    offset
                                );
                                return;
                            }
                        }
                    }
                }

                // Try Pattern B: MOV [reg+disp32],RAX after vtable LEA
                let next = i + 7; // first byte after the LEA instruction
                if next + 7 <= text.len() {
                    if text[next] == 0x48 && text[next + 1] == 0x89 {
                        let next_modrm = text[next + 2];
                        // mod=10 (disp32), reg=000 (RAX), rm=any base reg
                        if (next_modrm & 0xC0) == 0x80 && (next_modrm & 0x38) == 0x00 {
                            let offset = u32::from_le_bytes([
                                text[next + 3],
                                text[next + 4],
                                text[next + 5],
                                text[next + 6],
                            ]) as usize;
                            if offset >= 0x1000 && offset <= 0x4000 {
                                self.resolved
                                    .insert("customize_offset".into(), offset as *const u8);
                                log_info!(
                                    "  [+] customize_offset (derived from vtable store) = 0x{:X}",
                                    offset
                                );
                                return;
                            }
                        }
                    }
                }
            }
        }

        log_warn!("  [-] customize_offset -- could not derive from vtable LEA pattern");
    }

    // ── Generic helpers ─────────────────────────────────────────────

    /// Find a function by scanning for a debug string reference, then walking
    /// backwards to the function prologue (48 8B C4 = mov rax, rsp).
    fn find_function_by_debug_string(&self, needle: &str, label: &str) -> Option<*const u8> {
        // Build AOB pattern for the string bytes + null terminator
        let name_pattern: String = needle
            .bytes()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ")
            + " 00";

        let str_matches = scan_pattern_all(self.base, self.size, &name_pattern);
        if str_matches.is_empty() {
            log_warn!("  [-] {} -- debug string not found", label);
            return None;
        }
        let str_addr = str_matches[0].address;

        // LEA ModRM bytes for RIP-relative: mod=00, rm=101, reg=any
        let lea_modrm: &[u8] = &[0x05, 0x0D, 0x15, 0x1D, 0x25, 0x2D, 0x35, 0x3D];

        let bytes = unsafe { std::slice::from_raw_parts(self.base, self.size) };

        for i in 0..self.size.saturating_sub(7) {
            let b0 = bytes[i];
            if b0 != 0x48 && b0 != 0x4C {
                continue;
            }
            if bytes[i + 1] != 0x8D {
                continue;
            }
            if !lea_modrm.contains(&bytes[i + 2]) {
                continue;
            }

            let disp = i32::from_le_bytes([bytes[i + 3], bytes[i + 4], bytes[i + 5], bytes[i + 6]]);
            let instr_addr = unsafe { self.base.add(i) };
            let resolved = unsafe { instr_addr.add(7).offset(disp as isize) };
            if resolved != str_addr {
                continue;
            }

            // Found LEA referencing our string. Walk backwards to find prologue.
            for back in 0..0x2000usize {
                if i < back {
                    break;
                }
                let ci = i - back;
                if bytes[ci] == 0x48
                    && ci + 2 < self.size
                    && bytes[ci + 1] == 0x8B
                    && bytes[ci + 2] == 0xC4
                {
                    // Verify next byte is a PUSH (0x50-0x57 or REX 0x41 + 0x50-0x57)
                    if ci + 3 < self.size {
                        let next = bytes[ci + 3];
                        if (0x50..=0x57).contains(&next) {
                            return Some(unsafe { self.base.add(ci) });
                        }
                        if next == 0x41
                            && ci + 4 < self.size
                            && (0x50..=0x57).contains(&bytes[ci + 4])
                        {
                            return Some(unsafe { self.base.add(ci) });
                        }
                    }
                }
            }

            log_warn!(
                "  [-] {} -- found string ref but could not locate function prologue",
                label
            );
            return None;
        }

        log_warn!("  [-] {} -- no LEA referencing debug string found", label);
        None
    }

    /// Generic RTTI vtable finder for MSVC C++ classes.
    pub fn find_vtable_by_rtti(&self, rtti_name: &str, label: &str) -> Option<*const u8> {
        let name_pattern: String = rtti_name
            .bytes()
            .map(|b| format!("{:02X}", b))
            .collect::<Vec<_>>()
            .join(" ")
            + " 00";

        let name_matches = scan_pattern_all(self.base, self.size, &name_pattern);
        if name_matches.is_empty() {
            log_warn!("  [-] {} -- RTTI string \"{}\" not found", label, rtti_name);
            return None;
        }
        let name_addr = name_matches[0].address;

        unsafe {
            // TypeDescriptor = name_addr - 0x10
            let type_desc = name_addr.sub(0x10);
            let td_rva = type_desc.offset_from(self.base) as u32;
            let td_rva_pattern = format!(
                "{:02X} {:02X} {:02X} {:02X}",
                td_rva & 0xFF,
                (td_rva >> 8) & 0xFF,
                (td_rva >> 16) & 0xFF,
                (td_rva >> 24) & 0xFF
            );

            let rva_matches = scan_pattern_all(self.base, self.size, &td_rva_pattern);
            let mut col_addr: Option<*const u8> = None;

            for m in &rva_matches {
                // COL candidate = match - 0x0C
                let candidate = m.address.sub(0x0C);
                if candidate < self.base {
                    continue;
                }
                // x64 RTTI signature must be 1
                if (candidate as *const u32).read_unaligned() != 1 {
                    continue;
                }
                // pSelf must point back to COL
                let p_self = (candidate.add(0x14) as *const u32).read_unaligned();
                if self.base.add(p_self as usize) == candidate {
                    col_addr = Some(candidate);
                    break;
                }
            }

            let col = match col_addr {
                Some(c) => c,
                None => {
                    log_warn!("  [-] {} -- CompleteObjectLocator not found", label);
                    return None;
                }
            };

            // Find pointer to COL (vtable[-1])
            let col_bytes = (col as u64).to_le_bytes();
            let col_ptr_pattern = col_bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join(" ");

            let ptr_matches = scan_pattern_all(self.base, self.size, &col_ptr_pattern);
            if ptr_matches.is_empty() {
                log_warn!("  [-] {} -- vtable meta pointer not found", label);
                return None;
            }

            // vtable = meta_ptr + 8
            Some(ptr_matches[0].address.add(8))
        }
    }

    /// Find a RIP-relative MOV near a SHL reg, 0x6 instruction.
    fn find_rip_load_near_shl(&self, func_addr: *const u8, max_bytes: usize) -> Option<*const u8> {
        let rip_modrm: &[u8] = &[0x05, 0x0D, 0x15, 0x1D, 0x2D, 0x35, 0x3D];

        unsafe {
            for i in 0..max_bytes.saturating_sub(7) {
                let b0 = *func_addr.add(i);
                if (b0 & 0xFE) != 0x48 {
                    continue;
                }
                if *func_addr.add(i + 1) != 0xC1 {
                    continue;
                }
                let b2 = *func_addr.add(i + 2);
                if (b2 & 0xF8) != 0xE0 {
                    continue;
                }
                if *func_addr.add(i + 3) != 0x06 {
                    continue;
                }

                // Found SHL reg, 6. Scan forward for RIP-relative MOV.
                let search_end = std::cmp::min(i + 16, max_bytes.saturating_sub(6));
                for j in (i + 4)..search_end {
                    let m0 = *func_addr.add(j);
                    if (m0 & 0xFE) != 0x48 {
                        continue;
                    }
                    if *func_addr.add(j + 1) != 0x8B {
                        continue;
                    }
                    let m2 = *func_addr.add(j + 2);
                    if !rip_modrm.contains(&m2) {
                        continue;
                    }

                    return Some(decode_rip_relative(func_addr.add(j + 3)));
                }
            }
        }
        None
    }

    /// Find FF 15 [rip+disp] indirect call targets within `max_bytes` of `addr`,
    /// filtered to addresses within the given module.
    fn find_ff15_targets(
        &self,
        addr: *const u8,
        max_bytes: usize,
        mod_base: *const u8,
        mod_size: usize,
    ) -> Vec<*const u8> {
        let mut targets = Vec::new();
        let mod_end = mod_base as usize + mod_size;
        unsafe {
            for i in 0..max_bytes.saturating_sub(6) {
                if *addr.add(i) == 0xFF && *addr.add(i + 1) == 0x15 {
                    let target = decode_rip_relative(addr.add(i + 2));
                    let t = target as usize;
                    if t >= mod_base as usize && t + 8 <= mod_end {
                        targets.push(target);
                    }
                }
            }
        }
        targets
    }

    /// Find all CALL rel32 (E8) targets within `max_bytes` of `addr`.
    fn find_all_calls(&self, addr: *const u8, max_bytes: usize) -> Vec<*const u8> {
        let mut targets = Vec::new();
        unsafe {
            for i in 0..max_bytes {
                if *addr.add(i) == 0xE8 {
                    targets.push(decode_call_rel32(addr.add(i)));
                }
            }
        }
        targets
    }

    /// Find all indirect call targets (FF 15 [disp32] or MOV+CALL reg) within `max_bytes`.
    fn find_all_indirect_call_targets(&self, addr: *const u8, max_bytes: usize) -> Vec<*const u8> {
        let mut targets = Vec::new();
        unsafe {
            for i in 0..max_bytes.saturating_sub(6) {
                let b0 = *addr.add(i);
                let b1 = *addr.add(i + 1);

                // FF 15 [disp32] — CALL [rip+disp]
                if b0 == 0xFF && b1 == 0x15 {
                    targets.push(decode_rip_relative(addr.add(i + 2)));
                    continue;
                }

                // 48 8B 05/0D/15/35/3D [disp32] ... FF D0-D7 — MOV reg,[rip+disp]; CALL reg
                if b0 == 0x48 && b1 == 0x8B {
                    let b2 = *addr.add(i + 2);
                    if matches!(b2, 0x05 | 0x0D | 0x15 | 0x35 | 0x3D) {
                        let data_addr = decode_rip_relative(addr.add(i + 3));
                        // Look for CALL reg (FF D0-D7) within next 32 bytes
                        for j in (i + 7)..std::cmp::min(i + 39, max_bytes.saturating_sub(1)) {
                            if *addr.add(j) == 0xFF
                                && (*addr.add(j + 1) >= 0xD0 && *addr.add(j + 1) <= 0xD7)
                            {
                                targets.push(data_addr);
                                break;
                            }
                        }
                    }
                }
            }
        }
        targets
    }
}

/// Resolve a named export from the already-loaded `libafp-win64.dll`, or
/// `None` if the module or export is missing. Used by the CMovieClip
/// color-twin resolver to compare each twin body's IAT target against the
/// canonical `afp_layer_set_color` / `afp_layer_set_acolor` addresses.
/// libafp is a static import of gamemdx, so it is guaranteed loaded (and its
/// IAT slots patched) by the time `resolve_derived` runs.
///
/// Host-side (non-Windows) builds — the offline signature harness
/// (`scripts/validate_signatures.sh`) — have no loaded libafp. The harness
/// emulates the loader instead: it patches gamemdx's IAT slots for the libafp
/// imports it cares about with synthetic pointers and registers them here, so
/// the twin disambiguation runs the SAME comparison it runs on the cabinet.
/// Unregistered names return `None` (fail-open like a missing DLL).
#[cfg(not(windows))]
pub static HOST_LIBAFP_EXPORTS: std::sync::Mutex<Option<HashMap<String, usize>>> =
    std::sync::Mutex::new(None);

#[cfg(not(windows))]
fn resolve_libafp_export(name: &str) -> Option<*const u8> {
    let table = HOST_LIBAFP_EXPORTS.lock().ok()?;
    table.as_ref()?.get(name).map(|&p| p as *const u8)
}

#[cfg(windows)]
fn resolve_libafp_export(name: &str) -> Option<*const u8> {
    use std::ffi::CString;
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    let module = CString::new("libafp-win64.dll").ok()?;
    let export = CString::new(name).ok()?;
    unsafe {
        let handle = match GetModuleHandleA(PCSTR(module.as_ptr() as *const u8)) {
            Ok(h) if !h.is_invalid() => h,
            _ => return None,
        };
        GetProcAddress(handle, PCSTR(export.as_ptr() as *const u8)).map(|f| f as *const u8)
    }
}
