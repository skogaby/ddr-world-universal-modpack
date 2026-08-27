//! NoteTypesExpansion Mod — Framework for introducing new note types (mines,
//! lifts, rolls, ...) into DDR World's note pipeline.
//!
//! This module is a broker: it owns cross-cutting concerns for all sub-types
//! (config surface, SSQ chunk parsing glue, hook registration, per-chart
//! state lifecycle) and each note-type implementation plugs in through the
//! NoteTypeRegistry. Sub-type modules (mines, future lifts, etc.) live as
//! siblings of this file.

pub mod hooks;
pub mod mine_render;
pub mod mines;
pub mod note_type;
pub mod notes_vec;
pub mod registry;
pub use crate::core::ssq::ssq_chunk;
pub mod texture_loader;
pub use crate::core::ssq::timing;

use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::{judge_hook, scene_manager};
use crate::types::scenes::ATTRACT_SCENE_MAX;
use crate::{log_info, log_warn};

use self::hooks::{dispatch_reset, registry};
use self::mines::MineNoteType;
use self::texture_loader::MineTextureLoader;

/// Scene IDs at or above this threshold are gameplay and later.
/// Transitioning below this after being above it indicates chart end
/// (back to attract loop, result screen, etc.), which is our cue to
/// clear per-chart sidecar state.
const GAMEPLAY_SCENE_MIN: i32 = ATTRACT_SCENE_MAX + 1;

pub struct NoteTypesExpansionMod {
    analyze_addr: *const u8,
    malloc_addr: *const u8,
    free_addr: *const u8,
    heap_handle_addr: *const u8,
    judge_submit_addr: *const u8,
    file_manager_load_addr: *const u8,
    file_manager_singleton_addr: *const u8,
    get_texture_hash_value_addr: *const u8,
    get_texture_data_addr: *const u8,
    render_sprite_final_addr: *const u8,
    set_direction_addr: *const u8,
    set_render_state_addr: *const u8,
    get_offset_y_addr: *const u8,
    screen_renderer_state_addr: *const u8,
    default_shader_addr: *const u8,
    player_work_table_addr: *const u8,
    hooks_installed: bool,
    scene_cb_id: Option<usize>,
    pre_judge_handle: Option<judge_hook::CallbackHandle>,
    post_judge_handle: Option<judge_hook::CallbackHandle>,
    texture_loader: Option<MineTextureLoader>,
}

unsafe impl Send for NoteTypesExpansionMod {}

impl NoteTypesExpansionMod {
    pub fn new() -> Self {
        Self {
            analyze_addr: std::ptr::null(),
            malloc_addr: std::ptr::null(),
            free_addr: std::ptr::null(),
            heap_handle_addr: std::ptr::null(),
            judge_submit_addr: std::ptr::null(),
            file_manager_load_addr: std::ptr::null(),
            file_manager_singleton_addr: std::ptr::null(),
            get_texture_hash_value_addr: std::ptr::null(),
            get_texture_data_addr: std::ptr::null(),
            render_sprite_final_addr: std::ptr::null(),
            set_direction_addr: std::ptr::null(),
            set_render_state_addr: std::ptr::null(),
            get_offset_y_addr: std::ptr::null(),
            screen_renderer_state_addr: std::ptr::null(),
            default_shader_addr: std::ptr::null(),
            player_work_table_addr: std::ptr::null(),
            hooks_installed: false,
            scene_cb_id: None,
            pre_judge_handle: None,
            post_judge_handle: None,
            texture_loader: None,
        }
    }
}

impl Mod for NoteTypesExpansionMod {
    fn id(&self) -> &str {
        "note-types-expansion"
    }
    fn name(&self) -> &str {
        "Note Types Expansion"
    }
    fn description(&self) -> &str {
        "Framework for new note types. v1 target: ITG-style mines."
    }
    fn required_signatures(&self) -> &[&str] {
        &[
            "step_reader_analyze",
            "agcs_heap_malloc",
            "agcs_heap_free",
            "app_heap_handle",
            "judge_submit",
            "file_manager_load",
            "file_manager_singleton",
            "resource_manager_get_texture_hash_value",
            "resource_manager_get_texture_data",
            // NB: render_notes itself is owned by services::render_notes_hook
            // (the shared dispatcher this mod's mine pass subscribes to).
            "render_sprite_final",
            "set_direction",
            "set_render_state",
            "get_offset_y",
            "screen_renderer_state",
            "default_shader",
            "player_work_table",
        ]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        // Stash the signature addresses. ModRegistry::register has already
        // verified (via required_signatures()) that these all resolved, so
        // require_address is safe here.
        self.analyze_addr = ctx.signatures.require_address("step_reader_analyze");
        self.malloc_addr = ctx.signatures.require_address("agcs_heap_malloc");
        self.free_addr = ctx.signatures.require_address("agcs_heap_free");
        self.heap_handle_addr = ctx.signatures.require_address("app_heap_handle");
        self.judge_submit_addr = ctx.signatures.require_address("judge_submit");
        self.file_manager_load_addr = ctx.signatures.require_address("file_manager_load");
        self.file_manager_singleton_addr = ctx.signatures.require_address("file_manager_singleton");
        self.get_texture_hash_value_addr = ctx
            .signatures
            .require_address("resource_manager_get_texture_hash_value");
        self.get_texture_data_addr = ctx
            .signatures
            .require_address("resource_manager_get_texture_data");
        self.render_sprite_final_addr = ctx.signatures.require_address("render_sprite_final");
        self.set_direction_addr = ctx.signatures.require_address("set_direction");
        self.set_render_state_addr = ctx.signatures.require_address("set_render_state");
        self.get_offset_y_addr = ctx.signatures.require_address("get_offset_y");
        self.screen_renderer_state_addr = ctx.signatures.require_address("screen_renderer_state");
        self.default_shader_addr = ctx.signatures.require_address("default_shader");
        self.player_work_table_addr = ctx.signatures.require_address("player_work_table");

        // Install detours now, not on enable(). Detours are one-shot
        // (backed by OnceLock); the registry gates behavior, so detour
        // presence is cheap when the mod is disabled.
        let analyze_ok = hooks::install(
            self.malloc_addr,
            self.free_addr,
            self.heap_handle_addr,
            self.judge_submit_addr,
        );
        let mine_render_ok = mine_render::install(
            self.render_sprite_final_addr,
            self.set_direction_addr,
            self.set_render_state_addr,
            self.get_offset_y_addr,
            self.screen_renderer_state_addr,
            self.default_shader_addr,
            self.player_work_table_addr,
        );
        self.hooks_installed = analyze_ok && mine_render_ok;
        if !self.hooks_installed {
            log_warn!(
                "NoteTypesExpansion: hook install failed (analyze={}, mine_render={}) -- mod will register but do nothing",
                analyze_ok, mine_render_ok,
            );
        }
        log_info!("NoteTypesExpansion: initialized");
        true
    }

    fn enable(&mut self) {
        if !self.hooks_installed {
            log_warn!("NoteTypesExpansion: enable called but hooks are not installed");
            return;
        }

        // Load mine textures via the engine's file pipeline. The PNGs are
        // registered asynchronously; the render pass checks availability
        // per-frame via the texture data lookup and gracefully skips if
        // not ready.
        if self.texture_loader.is_none() {
            let loader = unsafe {
                MineTextureLoader::new(
                    self.file_manager_load_addr,
                    self.file_manager_singleton_addr,
                    self.get_texture_hash_value_addr,
                    self.get_texture_data_addr,
                )
            };
            self.texture_loader = Some(loader);
        }
        if let Some(ref mut loader) = self.texture_loader {
            loader.request_load_all();
        }

        // Give the mine render pass a reference to the texture loader
        // so it can look up TextureData per-frame.
        if let Some(ref loader) = self.texture_loader {
            unsafe {
                mine_render::set_texture_loader(loader as *const MineTextureLoader);
            }
        }

        // Register MineNoteType into the shared registry. Duplicate registers
        // are rejected with a warning by the registry, so enable/disable/
        // re-enable cycles are safe.
        let reg = registry();
        if let Ok(mut g) = reg.lock() {
            g.register(Box::new(MineNoteType::new()));
            log_info!(
                "NoteTypesExpansion: registry now holds {} NoteType(s) after MineNoteType register",
                g.len(),
            );
        } else {
            log_warn!("NoteTypesExpansion: registry mutex poisoned on enable() -- MineNoteType not registered");
        }

        // Subscribe to judge_hook so mine Results get pre-marked as
        // judged before vanilla judge runs (Priority::Early pre —
        // ahead of autoplay's Priority::Late pre), and mine-hit
        // detection runs after the vanilla judge restores state
        // (Priority::Late post — after autoplay's Priority::Early post
        // restores the user foot panel). The pre-mark also causes
        // AutoFootPanel::update's judged-state filter to naturally
        // skip mine panels, so autoplay avoids mines without any
        // separate hook.
        self.pre_judge_handle =
            judge_hook::register_pre(judge_hook::Priority::Early, note_types_pre_judge);
        self.post_judge_handle =
            judge_hook::register_post(judge_hook::Priority::Late, note_types_post_judge);
        if self.pre_judge_handle.is_none() || self.post_judge_handle.is_none() {
            log_warn!(
                "NoteTypesExpansion: judge_hook unavailable (pre={}, post={}) -- mines will not judge",
                self.pre_judge_handle.is_some(),
                self.post_judge_handle.is_some(),
            );
        }

        // Watch for gameplay scene exits to reset per-chart state.
        if scene_manager::is_available() {
            let cb_id = scene_manager::on_scene_change(Box::new(|prev, next| {
                if prev >= GAMEPLAY_SCENE_MIN && next < GAMEPLAY_SCENE_MIN {
                    dispatch_reset();
                    mine_render::reset_cache();
                }
            }));
            self.scene_cb_id = Some(cb_id);
        }

        log_info!("NoteTypesExpansion: enabled -- mine chunks will be parsed on chart load");
    }

    fn disable(&mut self) {
        // Clear the registry so the Analyze hook short-circuits. The hook
        // itself stays installed; reinstall on enable would fail the OnceLock.
        let reg = registry();
        if let Ok(mut g) = reg.lock() {
            *g = crate::mods::note_types_expansion::registry::NoteTypeRegistry::new();
        }
        if let Some(h) = self.pre_judge_handle.take() {
            judge_hook::unregister(h);
        }
        if let Some(h) = self.post_judge_handle.take() {
            judge_hook::unregister(h);
        }
        if let Some(id) = self.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        // Reset the arrow-shape cache so a subsequent enable re-resolves
        // it against the current Option state rather than holding onto
        // whatever was valid at the last chart.
        mine_render::reset_cache();
        log_info!("NoteTypesExpansion: disabled");
    }
}

// ── judge_hook callbacks ────────────────────────────────────────────
// Plain `fn` bodies (not `extern "C"`) because judge_hook's dispatcher
// is the FFI boundary; subscribers are regular Rust functions. See
// services/judge_hook.rs for the full ABI discussion.

fn note_types_pre_judge(actor: *mut u8, _music_count: i32) {
    // Prime the mine-render arrow-shape cache once per chart. The call
    // is cheap after the first successful resolve (single atomic load
    // + early return), so running it every pre-judge tick is fine.
    unsafe {
        mine_render::prime_arrow_shape(actor);
    }

    // Mark every active Result entry whose underlying Note uses a
    // registered NoteType kind with result::SENTINEL_SKIP so the
    // vanilla judge loop and AutoFootPanel::update skip it.
    let reg = registry();
    let guard = match reg.lock() {
        Ok(g) => g,
        Err(_) => return, // poisoned — fall through; vanilla judge still runs
    };
    unsafe {
        guard.mark_handled_results_skipped(actor);
    }
}

fn note_types_post_judge(actor: *mut u8, music_count: i32) {
    // Read foot-panel pointer from the actor. By the time the Late
    // post-callback runs, autoplay's Early post-callback has restored
    // the user's foot panel, so this reads either the real UserFootPanel
    // or — if autoplay is disabled — whatever IFootPanel the game
    // installed.
    let fp_offset = match judge_hook::foot_panel_offset() {
        Some(o) => o,
        None => return,
    };
    let foot_panel = unsafe { *(actor.add(fp_offset) as *const *mut u8) };
    if foot_panel.is_null() {
        return;
    }

    let reg = registry();
    let mut guard = match reg.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    guard.on_judge_tick(actor, music_count, foot_panel);
}
