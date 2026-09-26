//! DDR SELECTION themes — the dancer name in the theme difficulty frame,
//! A3's way (design R9, §4.11, §6.2; pure rules in `score_name_logic.rs`).
//!
//! A3's ScoreActor init (`FUN_180055390`) created an `agcs::BmpString` in
//! font 6 (`2d_font_player`) for the side's name when `FUN_18014f310(6)`
//! found that font loaded, pushed it onto the screen graph's slot-8 render
//! list (`*DAT_1802eee58 + 0xC8`) with node sort key `wrapper+0xC =
//! 0x7FFFFFFB`, styled it (yellow `0xFFFFEB08`; scale x 0.8·(1.16 | 1.6 with a
//! profile, `PlayerWork+1`), y 0.8·0.8, SD base 0.576), bound it to the
//! difficulty frame's `name_usr` placeholder (`FUN_180100480(…, 1, 3, 1.0)`)
//! and hid the placeholder (`score.rs` does that part).
//!
//! **RE (World 20260825, all five builds byte-checked)** — design Appendix C
//! row 5:
//!
//! * **The list.** World builds the identical screen graph (`FUN_18002aab0`,
//!   `DAT_1806f2d20`: 11 slots, slots 7..=10 `agcs::ScreenRoot` lists of 0x100
//!   nodes, then six `BM2DGroupWithPan` groups: group 0 key `0x7FFFFFFF`
//!   slot 0; groups 1 / 2 / 3 keys `0x7FFFFFFA` / `0x7FFFFFFC` / `0x7FFFFFFE`
//!   slot 8; group 4 slot 2; group 5 key `0xFFFFFFFF` slot 7). Slot 7
//!   (`+0xB0`) is the list every DLL overlay uses; slot 8 (`+0xC8`) is still
//!   World's gameplay 2D list (GamePlayActor and the 2D-object helper
//!   `FUN_1801ce8e0` push onto it — the `ddr_sel_gameplay_list_push`
//!   signature). A ScreenRoot draws its live children sorted by key
//!   (`FUN_1802180a0`), and World's ScoreActor puts the difficulty frame in
//!   group 1 and the score frames in groups 2 / 3 exactly as A3's did, so
//!   A3's key lands the name above the difficulty frame, below the score
//!   frames — and, being in slot 8, below the stage panel / end shutter
//!   (group 5, slot 7). The READY-to-shutter fallback of §6.2 is not needed.
//!   Nodes are only reclaimed when their `+0x28` byte is set (A3's finalize
//!   did that); ours never are, so the two widgets keep their nodes and are
//!   reused.
//! * **Font.** World loads all seven fonts at boot (`FUN_18002a430` /
//!   `FUN_18002a540`, table `system, ark_system, ui, songtitle_m,
//!   songtitle_s, rival, player`); `font_by_id` (`FUN_18020b0d0`, the
//!   `ddr_sel_font_by_id` signature) is A3's `FUN_18014f310` and gates the
//!   creation.
//! * **`PlayerWork+1`** is the e-pass profile byte, World `+5` (Step 5's RE:
//!   the header gained a 4-byte side index at `+0`; the entry profile window
//!   tests `+5` on 20250805 and 20260825).
//!
//! **Lifecycle.** Two widgets (one per side), created once, lazily, on the
//! render thread, in the slot-8 list with A3's key, and reused — render-list
//! nodes are never returned. A theme ScoreActor init POST (`score.rs`) binds
//! a side: text (World's rule — `PLAYER1` / `PLAYER2` for a guest), style,
//! then every frame while the difficulty layer lives the A3 binding is
//! recomputed from the placeholder (it follows anything that moves the
//! frame) and the widget follows the layer's visibility. A dead layer, the
//! GAMEPLAY exit, disarm and disable hide it. Fail-open: no sites, font 6
//! missing, a foreign screen graph or no node ⇒ no name, one WARN.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::core::memory;
use crate::core::signatures::{DdrSelNameSites, SignatureStore};
use crate::services::widget_renderer::{self, RenderList, WidgetStyle};
use crate::services::{bm2d_api, cabinet, stage_records};
use crate::types::scenes::scene;
use crate::widgets::text_widget::{TextAlignment, TextWidget};
use crate::{log_info, log_warn};

use super::score_name_logic as logic;

/// `Font* font_by_id(int id)`.
type FontByIdFn = unsafe extern "C" fn(i32) -> *const u8;

// ── Placeholder reads (`afp_mc_get_param`) ──────────────────────────────
const PARAM_POSITION: i32 = 0x1008;
const PARAM_WIDTH: i32 = 0x1015;
const PARAM_HEIGHT: i32 = 0x1016;
const PARAM_SCALE: i32 = 0x100D;

// ── PlayerWork header (identical on every supported build) ──────────────
const PW_SIDE: usize = 0x0;
const PW_PROFILE: usize = 0x5;
const PW_NAME: usize = 0xC;
const PW_NAME_LEN: usize = 9;
const PW_HEADER_LEN: usize = 0x18;

struct Sites(DdrSelNameSites);
// Raw pointers into the game module — valid for the process lifetime.
unsafe impl Send for Sites {}
unsafe impl Sync for Sites {}

static SITES: OnceLock<Sites> = OnceLock::new();
static WARNED: AtomicU32 = AtomicU32::new(0);
const W_FONT: u32 = 1;
const W_GRAPH: u32 = 2;
const W_CREATE: u32 = 4;
const W_PLAYER: u32 = 8;
const W_PANIC: u32 = 16;

fn warn_once(bit: u32) -> bool {
    WARNED.fetch_or(bit, Ordering::Relaxed) & bit == 0
}

/// One side's binding.
struct Bound {
    layer: u32,
    placeholder: u32,
    text: String,
    style: logic::Style,
    /// Text and style written to the widget.
    applied: bool,
}

struct State {
    widgets: [Option<TextWidget>; 2],
    /// Creation was refused for good (font / graph / pool): no name.
    create_failed: bool,
    sides: [Option<Bound>; 2],
    /// The per-frame render-thread job is queued.
    scheduled: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    widgets: [None, None],
    create_failed: false,
    sides: [None, None],
    scheduled: false,
});

fn lock() -> MutexGuard<'static, State> {
    match STATE.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// Capture the sites (mod init). A miss is one WARN: no theme name.
pub fn init(signatures: &SignatureStore) {
    match signatures.ddr_sel_name_sites() {
        Some(s) => {
            log_info!(
                "DDR SELECTION: theme dancer name ready (gameplay list +0x{:X}, font {})",
                s.gameplay_list_off,
                logic::FONT_PLAYER
            );
            let _ = SITES.set(Sites(s));
        }
        None => log_warn!(
            "DDR SELECTION: theme dancer-name sites unresolved -- no name in the theme difficulty frame"
        ),
    }
}

/// A theme ScoreActor init POST (game thread): bind `side`'s name to the
/// difficulty frame's `name_usr` placeholder.
pub fn bind(side: usize, layer: u32, placeholder: u32) {
    if SITES.get().is_none() || side > 1 {
        return;
    }
    let Some(pw) =
        stage_records::player_work(side).filter(|&p| memory::is_readable(p, PW_HEADER_LEN))
    else {
        if warn_once(W_PLAYER) {
            log_warn!("DDR SELECTION: PlayerWork[{side}] unreadable -- no dancer name");
        }
        return;
    };
    let (text, profile) = unsafe {
        let mut raw = [0u8; PW_NAME_LEN];
        std::ptr::copy_nonoverlapping(pw.add(PW_NAME), raw.as_mut_ptr(), PW_NAME_LEN);
        (
            logic::name_text(&raw, memory::read_u32(pw.add(PW_SIDE))),
            memory::read_u8(pw.add(PW_PROFILE)) != 0,
        )
    };
    let sd = matches!(cabinet::machine_type(), Some(0 | 1));
    let style = logic::style(profile, sd);
    log_info!(
        "DDR SELECTION: theme dancer name bound ({}P, {} chars, {}{})",
        side + 1,
        text.len(),
        if profile { "profile" } else { "guest" },
        if sd { ", SD scale" } else { "" }
    );
    let schedule = {
        let mut st = lock();
        st.sides[side] = Some(Bound {
            layer,
            placeholder,
            text,
            style,
            applied: false,
        });
        !std::mem::replace(&mut st.scheduled, true)
    };
    if schedule {
        widget_renderer::run_on_render_thread(frame);
    }
}

/// Hide both names (GAMEPLAY exit, disarm, disable).
pub fn hide_all() {
    let schedule = {
        let mut st = lock();
        if st.sides.iter().all(Option::is_none) {
            return;
        }
        st.sides = [None, None];
        !std::mem::replace(&mut st.scheduled, true)
    };
    if schedule {
        widget_renderer::run_on_render_thread(frame);
    }
}

/// Scene callback: leaving GAMEPLAY hides the names.
pub fn on_scene_change(prev: i32, next: i32) {
    if prev == scene::GAMEPLAY && next != scene::GAMEPLAY {
        hide_all();
    }
}

/// The render-thread job: create, re-bind, show / hide; re-queued while a
/// side is bound. The lock is never held across the schedule.
fn frame() {
    let again = std::panic::catch_unwind(|| {
        let mut st = lock();
        ensure_widgets(&mut st);
        for side in 0..2 {
            update_side(&mut st, side);
        }
        let any = st.sides.iter().any(Option::is_some);
        if !any {
            st.scheduled = false;
        }
        any
    });
    match again {
        Ok(true) => widget_renderer::run_on_render_thread(frame),
        Ok(false) => {}
        Err(_) => {
            let mut st = lock();
            st.scheduled = false;
            st.sides = [None, None];
            if warn_once(W_PANIC) {
                log_warn!("DDR SELECTION: theme dancer-name job panicked -- names off");
            }
        }
    }
}

/// Create the two widgets once (a side must be bound).
fn ensure_widgets(st: &mut State) {
    if st.widgets[0].is_some() || st.create_failed || st.sides.iter().all(Option::is_none) {
        return;
    }
    let Some(sites) = SITES.get().map(|s| &s.0) else {
        return;
    };
    if !widget_renderer::is_available() {
        return; // the font pointer is not captured yet: retry next frame
    }
    let font = unsafe {
        let f: FontByIdFn = std::mem::transmute(sites.font_by_id);
        f(logic::FONT_PLAYER)
    };
    if font.is_null() {
        st.create_failed = true;
        if warn_once(W_FONT) {
            log_warn!("DDR SELECTION: font 6 (2d_font_player) is not loaded -- no dancer name");
        }
        return;
    }
    if widget_renderer::scene_manager_global() != sites.screen_graph_global {
        st.create_failed = true;
        if warn_once(W_GRAPH) {
            log_warn!(
                "DDR SELECTION: the widget list is not in World's screen graph -- no dancer name"
            );
        }
        return;
    }
    let list = RenderList::Screen {
        offset: sites.gameplay_list_off,
        sort_key: logic::SORT_KEY,
    };
    for slot in st.widgets.iter_mut() {
        match widget_renderer::create_text_widget_with_font(
            logic::FONT_PLAYER,
            WidgetStyle::Native,
            list,
        ) {
            Some((w, _)) => {
                w.hide();
                *slot = Some(w);
            }
            None => {
                st.create_failed = true;
                if warn_once(W_CREATE) {
                    log_warn!(
                        "DDR SELECTION: could not create the dancer-name widget -- no dancer name"
                    );
                }
                return;
            }
        }
    }
    log_info!(
        "DDR SELECTION: theme dancer-name widgets created (font {}, gameplay list +0x{:X}, key 0x{:08X})",
        logic::FONT_PLAYER,
        sites.gameplay_list_off,
        logic::SORT_KEY
    );
}

fn update_side(st: &mut State, side: usize) {
    let State { widgets, sides, .. } = st;
    let (Some(w), Some(slot)) = (
        widgets.get(side).and_then(Option::as_ref),
        sides.get_mut(side),
    ) else {
        return;
    };
    let Some(b) = slot.as_mut() else {
        w.hide();
        return;
    };
    if !bm2d_api::layer_id_is_valid(b.layer) {
        w.hide();
        *slot = None;
        return;
    }
    let Some(rect) = placeholder_rect(b.placeholder) else {
        w.hide();
        return;
    };
    if !b.applied {
        w.set_text(&b.text);
        let (r, g, bl, a) = b.style.color;
        w.set_color(r, g, bl, a);
        w.set_scale(b.style.scale.0, b.style.scale.1);
        b.applied = true;
    }
    let bind = logic::binding(&rect);
    w.set_position(bind.position.0, bind.position.1);
    w.set_box(bind.box_lr.0, bind.box_lr.1, true);
    w.set_alignment(TextAlignment::Center);
    w.set_vertical_alignment(bind.valign);
    let visible = bm2d_api::layer_get_info_raw(b.layer).is_none_or(|i| i.visible);
    if visible {
        w.show();
    } else {
        w.hide();
    }
}

/// The placeholder's position, size and scale (A3's reads).
fn placeholder_rect(mc: u32) -> Option<logic::Rect> {
    let (x, y) = bm2d_api::mc_get_vec2(mc, PARAM_POSITION)?;
    let (w, _) = bm2d_api::mc_get_vec2(mc, PARAM_WIDTH)?;
    let (h, _) = bm2d_api::mc_get_vec2(mc, PARAM_HEIGHT)?;
    let (sx, sy) = bm2d_api::mc_get_vec2(mc, PARAM_SCALE).unwrap_or((1.0, 1.0));
    Some(logic::Rect { x, y, w, h, sx, sy })
}
