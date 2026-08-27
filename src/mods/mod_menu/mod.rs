//! Mod Menu — In-game overlay for viewing and toggling mods.
//!
//! Triple-0 gesture activation (press 0 three times on either pinpad).
//! Tabbed shell (overlay-menu rewrite design §4.1): MODS (registry toggles)
//! and GLOBAL SETTINGS (cabinet-wide contributed rows grouped per owning
//! mod); PLAYER SETTINGS and THEME arrive in later steps.
//!
//! Module layout:
//! - `mod.rs` — lifecycle (`Mod` impl), the triple-0 open gesture, open/close,
//!   and the shared `ModMenuState`.
//! - `model.rs` — the PURE row/tab/navigation model (host-tested via
//!   `scripts/validate_mod_menu.sh`).
//! - `tabs.rs` — snapshot assembly + tab row-list rebuilds (impure glue
//!   between the registry/contributed registrations and the model).
//! - `rows.rs` — the public row-registration API (`ScalarRowSpec`/
//!   `EnumRowSpec`), the contributed-row store, and the edit paths.
//! - `input.rs` — exclusive input handling while open and the hold-to-repeat
//!   thread.
//! - `render.rs` — widget allocation, layout constants, and refresh.

pub(crate) mod chrome;
mod chrome_loader;
mod input;
pub(crate) mod model;
mod render;
mod rows;
mod tabs;
pub(crate) mod theme;

#[allow(unused_imports)]
pub use rows::{
    register_enum_row, register_scalar_row, remove_rows_for, EntriesCallback, EnumRowSpec, MenuRow,
    RowChangeCallback, RowKind, ScalarRowSpec, ToggleCallback,
};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::log_info;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::{custom_options, input_manager, scene_manager, widget_renderer};
use crate::types::buttons::*;
use crate::widgets::image_widget::ImageWidget;
use crate::widgets::text_widget::TextWidget;

use render::Slot;

const GESTURE_WINDOW_MS: u128 = 1250;
const GESTURE_COUNT: usize = 3;

/// Shared state that the ModMenu and the global registry interact with.
pub struct ModMenuState {
    is_open: bool,
    /// Per-tab cursor/scroll memory + the active tab (reset on every open).
    tab_nav: model::TabNav,
    /// Built display lists, parallel to `model::TabId::ALL`. Rebuilt on open,
    /// tab switch, and after any edit (a toggle may register/remove rows).
    tab_rows: Vec<Vec<model::Row>>,
    /// Rows contributed by mods via `register_scalar_row`/`register_enum_row`
    /// (the registration store; persists across opens).
    contributed_rows: Vec<MenuRow>,
    /// PLAYER SETTINGS: the configured side (0/1). Seeded from the side whose
    /// pinpad completed the open gesture; normalized against the editable
    /// sides at every tab rebuild (`model::resolve_selected_side`).
    player_side: u8,
    /// Editable-sides snapshot from the last tab rebuild (render + selector
    /// state read this; the marshaled edit path re-checks LIVE).
    player_editable: [bool; 2],
    /// PLAYER SETTINGS renders the "OPTIONS FRAMEWORK UNAVAILABLE" banner
    /// when custom_options never initialized (set at tab rebuild).
    framework_unavailable: bool,
    /// custom_options value-changed observer token (enable → disable).
    observer_token: Option<usize>,
    /// scene_manager change-callback id (enable → disable).
    scene_cb_id: Option<usize>,
    // Widgets (allocated once on first open, reused thereafter). Chrome
    // ImageWidgets are created BEFORE the text widgets — z = render-list
    // creation order, and the chrome must draw beneath the text.
    panel_widget: Option<ImageWidget>,
    /// The overlay-draw EMISSION ANCHOR: created FIRST (before the panel)
    /// so the animated-background quad — emitted at this wrapper's render —
    /// draws beneath the panel and every menu widget, but above everything
    /// the widget layer drew earlier in the walk (incl. full-screen loading
    /// art). Permanently hidden (its glyph never rasterizes; only the
    /// wrapper_render dispatch matters).
    bg_anchor_widget: Option<TextWidget>,
    header_bar_widgets: Vec<ImageWidget>,
    tab_indicator_widget: Option<ImageWidget>,
    selection_bar_widget: Option<ImageWidget>,
    scroll_track_widget: Option<ImageWidget>,
    scroll_thumb_widget: Option<ImageWidget>,
    banner_backing_widget: Option<ImageWidget>,
    title_widget: Option<TextWidget>,
    title_credit_widget: Option<TextWidget>,
    tab_widgets: Vec<TextWidget>,
    indicator_widget: Option<TextWidget>,
    cursor_widget: Option<TextWidget>,
    slots: Vec<Slot>,
    footer_desc_widget: Option<TextWidget>,
    footer_hints_widget: Option<TextWidget>,
    side_selector_widget: Option<TextWidget>,
    banner_widget: Option<TextWidget>,
    zero_press_times: Vec<Instant>,
    input_cb_id: Option<usize>,
    widgets_allocated: bool,
    /// Callback to toggle a mod in the registry. Set externally after registration.
    pub toggle_callback: Option<ToggleCallback>,
    /// Callback to get current mod entries from the registry.
    pub entries_callback: Option<EntriesCallback>,
}

unsafe impl Send for ModMenuState {}

pub static MOD_MENU_STATE: once_cell::sync::Lazy<Mutex<ModMenuState>> =
    once_cell::sync::Lazy::new(|| {
        Mutex::new(ModMenuState {
            is_open: false,
            tab_nav: model::TabNav::new(),
            tab_rows: Vec::new(),
            contributed_rows: Vec::new(),
            player_side: 0,
            player_editable: [false, false],
            framework_unavailable: false,
            observer_token: None,
            scene_cb_id: None,
            panel_widget: None,
            bg_anchor_widget: None,
            header_bar_widgets: Vec::new(),
            tab_indicator_widget: None,
            selection_bar_widget: None,
            scroll_track_widget: None,
            scroll_thumb_widget: None,
            banner_backing_widget: None,
            title_widget: None,
            title_credit_widget: None,
            tab_widgets: Vec::new(),
            indicator_widget: None,
            cursor_widget: None,
            slots: Vec::new(),
            footer_desc_widget: None,
            footer_hints_widget: None,
            side_selector_widget: None,
            banner_widget: None,
            zero_press_times: Vec::new(),
            input_cb_id: None,
            widgets_allocated: false,
            toggle_callback: None,
            entries_callback: None,
        })
    });

pub struct ModMenuMod;

impl ModMenuMod {
    pub fn new() -> Self {
        Self
    }
}

impl Mod for ModMenuMod {
    fn id(&self) -> &str {
        "mod-menu"
    }
    fn name(&self) -> &str {
        "Mod Menu"
    }
    fn description(&self) -> &str {
        "In-game mod configuration menu"
    }
    fn required_signatures(&self) -> &[&str] {
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        true
    }

    fn enable(&mut self) {
        // Kick the chrome pipeline (synthesis → cache → texture load) so the
        // modal textures are resident before the first open (~0.7 s resolve).
        chrome_loader::kick();

        // Listen for the triple-0 open gesture
        if input_manager::is_available() {
            let id = input_manager::on_input_event(Arc::new(|event: &InputEvent| {
                let Ok(mut state) = MOD_MENU_STATE.lock() else {
                    return;
                };
                if state.is_open {
                    return;
                }
                if event.event_type == InputEventType::Pressed
                    && event.button == button::NUM_0
                    && on_zero_pressed(&mut state)
                {
                    // The side whose press COMPLETED the gesture becomes the
                    // PLAYER SETTINGS default side (design §4.8).
                    state.player_side = event.player as u8;
                    drop(state);
                    open();
                }
            }));
            if let Ok(mut state) = MOD_MENU_STATE.lock() {
                state.input_cb_id = Some(id);
            }
        }

        // PLAYER SETTINGS live-mirror plumbing: repaint (coalesced) whenever
        // any custom_options value changes or the scene changes while the
        // menu is open. Both callbacks are cheap no-ops while closed.
        if custom_options::is_available() {
            let token = custom_options::subscribe_value_changed(Arc::new(
                |_id: &str, _side: u8, _value: i32| {
                    schedule_coalesced_refresh();
                },
            ));
            if let Ok(mut state) = MOD_MENU_STATE.lock() {
                state.observer_token = Some(token);
            }
        }
        if scene_manager::is_available() {
            let cb_id = scene_manager::on_scene_change(Box::new(|_prev, _next| {
                // Session gating depends on the scene band — re-evaluate via
                // a full rebuild (the gating adapter runs inside it).
                schedule_coalesced_refresh();
            }));
            if let Ok(mut state) = MOD_MENU_STATE.lock() {
                state.scene_cb_id = Some(cb_id);
            }
        }

        log_info!("ModMenu: enabled");
    }

    fn disable(&mut self) {
        close();
        let Ok(mut state) = MOD_MENU_STATE.lock() else {
            return;
        };
        if let Some(id) = state.input_cb_id.take() {
            input_manager::remove_callback(id);
        }
        if let Some(token) = state.observer_token.take() {
            custom_options::unsubscribe_value_changed(token);
        }
        if let Some(id) = state.scene_cb_id.take() {
            scene_manager::remove_callback(id);
        }
        render::destroy_widgets(&mut state);
        log_info!("ModMenu: disabled");
    }
}

/// One-shot pending latch for observer/scene-driven repaints: bursts (a
/// card-in reset fires one observer event per option) collapse into a single
/// queued rebuild+refresh. The queued closure re-arms the latch FIRST so an
/// event landing during the rebuild schedules one more pass (never lost).
static REFRESH_PENDING: AtomicBool = AtomicBool::new(false);

/// Lock-free mirror of `ModMenuState.is_open` for the background feed
/// (the emitter's hot path can't take the state mutex).
static MENU_OPEN: AtomicBool = AtomicBool::new(false);

/// Whether the ACTIVE theme currently has a live shader path — feeds the
/// ANIMATED BACKGROUND row's greyed state (MINIMAL is Static by design;
/// otherwise requires the synthesis export).
pub(super) fn background_available() -> bool {
    if !crate::services::overlay_draw::emitter_ready() {
        return false;
    }
    match theme::theme(chrome_loader::active_theme_index()).background {
        theme::Background::Static => false,
        theme::Background::Shader { .. } => {
            crate::services::overlay_draw::theme_program_indices().is_some()
        }
    }
}

/// Recompute the animated-background activation from (menu open ∧ ANIMATED
/// BACKGROUND ∧ active theme shader-backed ∧ synthesis export) and feed the
/// emitter. Atomics only — callable from any thread; invoked on open,
/// close, and THEME / ANIMATED BACKGROUND edits.
pub(super) fn update_background_feed() {
    let params = if !MENU_OPEN.load(Ordering::Acquire)
        || !chrome_loader::animate_background()
        || !crate::services::overlay_draw::emitter_ready()
    {
        None
    } else {
        match theme::theme(chrome_loader::active_theme_index()).background {
            theme::Background::Static => None,
            theme::Background::Shader { program } => {
                crate::services::overlay_draw::theme_program_indices().map(|idx| {
                    crate::services::overlay_draw::BackgroundParams {
                        program: idx[program.slot()],
                        rect: render::modal_rect(),
                        // MENU OPACITY drives the animation's translucency
                        // exactly like the static panel's baked alpha.
                        alpha: chrome::opacity_alpha(chrome_loader::effective_opacity()),
                        params: [0.0, 0.0],
                    }
                })
            }
        }
    };
    log_info!(
        "ModMenu: background feed -> {:?} (open={}, animate={}, theme={})",
        params.as_ref().map(|p| p.program),
        MENU_OPEN.load(Ordering::Relaxed),
        chrome_loader::animate_background(),
        theme::theme(chrome_loader::active_theme_index()).id
    );
    crate::services::overlay_draw::set_background(params);

    // Prime the anchor's dirty chain on (re)activation: the walk's dispatch
    // of the anchor is what carries emission, and the post-render re-arm
    // only runs while active — a fresh activation needs one manual kick.
    // Marshaled to the render thread (widget mutation rule).
    if params.is_some() {
        widget_renderer::run_on_render_thread(|| {
            if let Ok(state) = MOD_MENU_STATE.lock() {
                if let Some(ref w) = state.bg_anchor_widget {
                    w.mark_dirty();
                }
            }
        });
    }
}

pub(crate) fn schedule_coalesced_refresh() {
    if REFRESH_PENDING.swap(true, Ordering::AcqRel) {
        return; // one already queued
    }
    widget_renderer::run_on_render_thread(|| {
        REFRESH_PENDING.store(false, Ordering::Release);
        let Ok(mut state) = MOD_MENU_STATE.lock() else {
            return;
        };
        if !state.is_open {
            return;
        }
        tabs::rebuild_tabs(&mut state);
        render::refresh_all(&state);
    });
}

fn on_zero_pressed(state: &mut ModMenuState) -> bool {
    let now = Instant::now();
    state.zero_press_times.push(now);
    state
        .zero_press_times
        .retain(|t| now.duration_since(*t).as_millis() <= GESTURE_WINDOW_MS);
    if state.zero_press_times.len() >= GESTURE_COUNT {
        state.zero_press_times.clear();
        return true;
    }
    false
}

fn open() {
    let Ok(mut state) = MOD_MENU_STATE.lock() else {
        return;
    };
    if state.is_open {
        return;
    }

    // Fresh navigation (MODS tab, top) + fresh row lists from current
    // registry/contributed state.
    state.tab_nav.reset();
    tabs::rebuild_tabs(&mut state);

    state.is_open = true;
    MENU_OPEN.store(true, Ordering::Release);
    update_background_feed();

    // Grab input exclusivity (modpack side) and suppress numpad inputs from
    // reaching the game (game side).
    if input_manager::is_available() {
        input_manager::set_exclusive_consumer(Arc::new(|event: &InputEvent| -> bool {
            input::handle_exclusive_input(event)
        }));
        input_manager::set_input_suppressed(true);
    }

    // Start the hold-to-repeat thread for scalar rows (stops on close).
    drop(state);
    input::start_repeat_thread();

    // Defer all widget operations to the render thread (widget mutations are
    // not safe from non-game threads).
    widget_renderer::run_on_render_thread(|| {
        let Ok(mut state) = MOD_MENU_STATE.lock() else {
            return;
        };
        if !state.widgets_allocated {
            render::allocate_widgets(&mut state);
        }
        render::refresh_all(&state);
    });

    log_info!("ModMenu: opened");
}

fn close() {
    let Ok(mut state) = MOD_MENU_STATE.lock() else {
        return;
    };
    if !state.is_open {
        return;
    }
    state.is_open = false;
    MENU_OPEN.store(false, Ordering::Release);
    // Stop the animated background immediately (the deferred widget hide
    // would otherwise leave a frame or two of emission).
    update_background_feed();

    // Stop the hold-to-repeat thread.
    input::stop_repeat_thread();

    if input_manager::is_available() {
        input_manager::clear_exclusive_consumer();
        input_manager::set_input_suppressed(false);
    }

    // Defer widget hide to the render thread
    widget_renderer::run_on_render_thread(|| {
        let Ok(state) = MOD_MENU_STATE.lock() else {
            return;
        };
        render::hide_all_widgets(&state);
    });

    log_info!("ModMenu: closed");
}
