//! Widget allocation, layout constants, and repaint for the tabbed mod-menu
//! shell. The modal chrome (rounded gradient panel, selection bar, tab
//! indicator, scrollbar, header backing bars) renders as ImageWidgets bound
//! to the runtime-synthesized textures from `chrome_loader`; chrome widgets
//! are created BEFORE the text widgets because z = render-list creation
//! order. Every chrome element is individually optional — an unresolved or
//! failed texture just hides it (design §6 ladder: solid strip → text-only).

use crate::log_warn;
use crate::services::widget_renderer;
use crate::widgets::image_widget::ImageWidgetConfig;
use crate::widgets::text_widget::{TextAlignment, TextWidget};

use super::model::{Navigator, Row, RowKind, SelectorState, TabId};
use super::{chrome, chrome_loader, theme, ModMenuState};

/// Visible row slots per page.
pub(super) const VISIBLE_ROWS: usize = 12;

/// Visible CONTENT rows for a tab: the PLAYER tab surrenders its first row
/// band to the pinned side selector (approved layout decision 2026-08-25).
pub(super) fn visible_rows(tab: TabId) -> usize {
    if tab == TabId::PlayerSettings {
        VISIBLE_ROWS - 1
    } else {
        VISIBLE_ROWS
    }
}

/// Top of a tab's content rows: shifted one row band down on the PLAYER tab
/// (the selector occupies the original first band).
fn list_start_y(tab: TabId) -> f32 {
    if tab == TabId::PlayerSettings {
        LIST_START_Y + ROW_H
    } else {
        LIST_START_Y
    }
}

/// The Navigator for a tab: the PLAYER tab carries the pinned selector slot
/// and the reduced page; everything else is the plain navigator.
pub(super) fn navigator_for<'a>(tab: TabId, rows: &'a [Row]) -> Navigator<'a> {
    Navigator::new_with_pinned(rows, visible_rows(tab), tab == TabId::PlayerSettings)
}

// Modal footprint (the panel texture fills it; text is laid out inside it).
const MODAL_X: f32 = 60.0;
const MODAL_Y: f32 = 60.0;
const MODAL_W: f32 = 1160.0;
const MODAL_H: f32 = 600.0;

/// The modal rect in integer pixel space — the animated background's
/// scissor/constant rect (one source of truth with the panel above).
pub(super) fn modal_rect() -> (u16, u16, u16, u16) {
    (
        MODAL_X as u16,
        MODAL_Y as u16,
        MODAL_W as u16,
        MODAL_H as u16,
    )
}

const CONTENT_X: f32 = MODAL_X + 40.0; // 100
const RIGHT_X: f32 = MODAL_X + MODAL_W - 40.0; // 1180 (right-aligned column)

const TITLE_Y: f32 = MODAL_Y + 14.0;
const TITLE_SCALE: f32 = 0.75;

const TAB_Y: f32 = MODAL_Y + 56.0;
const TAB_X0: f32 = CONTENT_X;
const TAB_SPACING: f32 = 260.0;
/// Active tab renders larger than inactive ones (a "grow" affordance on top
/// of the color change; maintainer feedback 2026-08-24 — no brackets).
const TAB_SCALE_ACTIVE: f32 = 0.62;
const TAB_SCALE_INACTIVE: f32 = 0.52;

const LIST_START_Y: f32 = MODAL_Y + 96.0; // 156
const ROW_H: f32 = 34.0;
const ROW_SCALE: f32 = 0.55;
const CURSOR_X: f32 = CONTENT_X - 24.0;

// PLAYER SETTINGS pinned selector (renders in the original first row band;
// content shifts one band down on that tab).
const SELECTOR_Y: f32 = LIST_START_Y;
const SELECTOR_SCALE: f32 = ROW_SCALE;

// Session/framework banner (PLAYER tab): centered text over a backing strip
// at the middle of the content area.
const BANNER_Y: f32 = LIST_START_Y + 5.0 * ROW_H;
const BANNER_SCALE: f32 = 0.62;
const BANNER_X: f32 = MODAL_X + MODAL_W / 2.0; // center-aligned text anchor
const BANNER_BACK_X: f32 = MODAL_X + 120.0;
const BANNER_BACK_W: f32 = MODAL_W - 240.0;
const BANNER_BACK_H: f32 = 44.0;
const BANNER_BACK_Y_OFF: f32 = -10.0;

const FOOTER_DESC_Y: f32 = MODAL_Y + 522.0; // 582
const FOOTER_HINTS_Y: f32 = MODAL_Y + 554.0; // 614
/// Selected-row description matches the option rows' size (maintainer
/// feedback: it read smaller than the rows at 0.45).
const FOOTER_DESC_SCALE: f32 = 0.55;
const FOOTER_HINTS_SCALE: f32 = 0.45;

const KEY_HINTS: &str =
    "UP/DOWN: Scroll the options     LEFT/RIGHT: Toggle/adjust     1/3: Switch between tabs     0-0-0: Close the menu";

/// Credit tag rendered to the right of the title (maintainer request). The
/// title ends ≈ x=386 on the 1280 canvas (measured from a screenshot); text
/// width isn't queryable, so the offset is tuned by eye.
const CREDIT_TEXT: &str = "(by skogaby)";
const CREDIT_X: f32 = CONTENT_X + 300.0;
const CREDIT_Y: f32 = TITLE_Y + 10.0;
const CREDIT_SCALE: f32 = 0.45;

// Colors come from the active theme's palette (`theme::Palette` — one
// field per use site here). Resolved once per allocate/refresh pass.
fn pal() -> &'static theme::Palette {
    &theme::theme(chrome_loader::active_theme_index()).palette
}

/// Set a text widget's color from a palette RGB triple (alpha 1.0).
fn set_rgb(w: &TextWidget, c: [f32; 3]) {
    w.set_color(c[0], c[1], c[2], 1.0);
}

// ── Chrome geometry (ImageWidgets over the synthesized textures) ─────
// First-deploy guesses tuned by maintainer visual review, like the text
// layout above.

/// Selection bar: spans the content area behind the selected row.
const SELBAR_X: f32 = MODAL_X + 16.0; // 76
const SELBAR_W: f32 = MODAL_W - 32.0; // 1128
const SELBAR_H: f32 = 30.0;
const SELBAR_Y_OFF: f32 = -4.0;

/// Header backing bars: one per visible slot, shown on Header rows.
const HDRBAR_X: f32 = MODAL_X + 24.0; // 84
const HDRBAR_W: f32 = MODAL_W - 48.0; // 1112
const HDRBAR_H: f32 = 28.0;
const HDRBAR_Y_OFF: f32 = -3.0;

/// Active-tab indicator: an underline strip beneath the tab label (text
/// width isn't queryable, so a fixed width underline reads cleaner than a
/// backing box).
const TAB_IND_W: f32 = 220.0;
const TAB_IND_H: f32 = 4.0;
const TAB_IND_X_OFF: f32 = -4.0;
const TAB_IND_Y: f32 = TAB_Y + 30.0;
/// Tab labels render center-aligned over their underline's midpoint
/// (maintainer feedback 2026-08-24 — left-justified labels read misaligned
/// against the fixed-width underline; centering also makes the active-tab
/// grow affordance symmetric).
const TAB_TEXT_CENTER_OFF: f32 = TAB_IND_X_OFF + TAB_IND_W / 2.0;

/// Scrollbar: right edge of the content band, proportional thumb.
const SCROLL_X: f32 = MODAL_X + MODAL_W - 20.0; // 1200
const SCROLL_W: f32 = 6.0;
const SCROLL_TRACK_Y: f32 = LIST_START_Y - 4.0; // 152
const SCROLL_TRACK_H: f32 = VISIBLE_ROWS as f32 * ROW_H + 8.0; // 416
const SCROLL_THUMB_MIN_H: f32 = 24.0;

/// Pack an ABGR tint (alpha in the high byte — `ImageWidget::set_color`).
const fn abgr(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
}

// Chrome tints for the white strip texture (multiplied at render). RGB comes
// from the active theme's palette; the alpha bytes are fixed design values.
const ALPHA_SELBAR: u8 = 0x38; // accent @ ~22 %
const ALPHA_HDRBAR: u8 = 0x30; // header bar @ ~19 %
const ALPHA_TAB_IND: u8 = 0xE6; // accent underline @ ~90 %
/// Banner backing: dark, mostly opaque, so the banner reads over greyed rows.
const ALPHA_BANNER_BACK: u8 = 0xC8;
const TINT_SCROLL_TRACK: u32 = abgr(0x26, 255, 255, 255);
const TINT_SCROLL_THUMB: u32 = abgr(0x80, 255, 255, 255);
/// Panel widget tint when the real panel texture is bound (opacity is baked
/// into the texture, so the tint stays neutral).
const TINT_PANEL_NEUTRAL: u32 = abgr(0xFF, 255, 255, 255);
/// Panel tint alpha while the ANIMATED BACKGROUND is live: the gradient
/// becomes a translucent wash over the animation (even at MENU OPACITY
/// 100% the animation stays visible — maintainer feedback 2026-08-25;
/// gameplay translucency is the quad's own alpha, which carries the
/// configured opacity).
const PANEL_ALPHA_OVER_ANIMATION: u8 = 0x59; // ~35 %

/// Pack a palette RGB tint with a fixed alpha.
const fn tint(alpha: u8, rgb: [u8; 3]) -> u32 {
    abgr(alpha, rgb[0], rgb[1], rgb[2])
}

pub(super) struct Slot {
    label: TextWidget,
    value: TextWidget,
}

/// Text-widget budget of `allocate_widgets` (title + credit + tabs +
/// indicator + cursor + 12×2 slots + 2 footers + side selector + banner) —
/// the chrome headroom check protects this count.
const TEXT_WIDGET_COUNT: usize = 8 + TabId::ALL.len() + VISIBLE_ROWS * 2;
/// Chrome-widget budget (panel + header bars + tab indicator + selection
/// bar + scrollbar track/thumb + banner backing).
const CHROME_WIDGET_COUNT: usize = 6 + VISIBLE_ROWS;

/// One-shot latch for the headroom-skip WARN.
static CHROME_SKIP_WARNED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Create the chrome ImageWidgets. Called FIRST in `allocate_widgets` so the
/// chrome sits beneath the text (z = render-list creation order). Chrome is
/// decorative: when the free pool can't fit chrome + text, skip ALL of it so
/// exhaustion degrades looks, not function (design §6). An unavailable walk
/// (`None`) proceeds — per-widget creation failure is already non-fatal.
fn allocate_chrome_widgets(state: &mut ModMenuState) {
    if let Some(free) = widget_renderer::free_node_count() {
        if free < CHROME_WIDGET_COUNT + TEXT_WIDGET_COUNT {
            if !CHROME_SKIP_WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                log_warn!(
                    "ModMenu: widget pool low ({} free < {} needed) — skipping chrome (text-only menu)",
                    free,
                    CHROME_WIDGET_COUNT + TEXT_WIDGET_COUNT
                );
            }
            return;
        }
    }

    // All created hidden (create_image_widget starts widgets hidden) and
    // unbound (`texture_name: None` — chrome_loader supplies texture ids;
    // refresh_all binds/positions/shows).
    let image = |x: f32, y: f32, w: f32, h: f32| {
        widget_renderer::create_image_widget(&ImageWidgetConfig {
            x,
            y,
            width: w,
            height: h,
            texture_name: None,
            ..Default::default()
        })
    };

    // Panel first: the bottom of the modal's z sandwich.
    state.panel_widget = image(MODAL_X, MODAL_Y, MODAL_W, MODAL_H);
    // Header backing bars, one per visible slot (positioned per refresh).
    for i in 0..VISIBLE_ROWS {
        let y = LIST_START_Y + i as f32 * ROW_H + HDRBAR_Y_OFF;
        if let Some(w) = image(HDRBAR_X, y, HDRBAR_W, HDRBAR_H) {
            state.header_bar_widgets.push(w);
        }
    }
    state.tab_indicator_widget = image(TAB_X0 + TAB_IND_X_OFF, TAB_IND_Y, TAB_IND_W, TAB_IND_H);
    state.selection_bar_widget = image(SELBAR_X, LIST_START_Y + SELBAR_Y_OFF, SELBAR_W, SELBAR_H);
    state.scroll_track_widget = image(SCROLL_X, SCROLL_TRACK_Y, SCROLL_W, SCROLL_TRACK_H);
    state.scroll_thumb_widget = image(SCROLL_X, SCROLL_TRACK_Y, SCROLL_W, SCROLL_THUMB_MIN_H);
    state.banner_backing_widget = image(
        BANNER_BACK_X,
        BANNER_Y + BANNER_BACK_Y_OFF,
        BANNER_BACK_W,
        BANNER_BACK_H,
    );
}

pub(super) fn allocate_widgets(state: &mut ModMenuState) {
    // The emission anchor MUST be the very first widget registered: the
    // animated-background quad is emitted at ITS wrapper_render, and
    // render-list z = registration order — anchor first ⇒ quad beneath the
    // panel/chrome/text but above all earlier walk content (loading art).
    if let Some((w, wrapper)) = widget_renderer::create_text_widget_with_wrapper() {
        w.set_text(" ");
        w.set_position(-100.0, -100.0);
        w.set_scale(0.1, 0.1);
        w.hide(); // never rasterizes; only the wrapper_render dispatch matters
        let dirty = w.dirty_flag_addr() as usize;
        crate::services::overlay_draw::set_emit_anchor(wrapper, dirty);
        w.mark_dirty(); // prime the walk's first dispatch
        state.bg_anchor_widget = Some(w);
    } else {
        log_warn!("ModMenu: background anchor widget unavailable — animated backgrounds off");
    }

    // Chrome before text: z = creation order, and the panel/bars must draw
    // beneath every label.
    allocate_chrome_widgets(state);

    let p = pal();

    if let Some(w) = widget_renderer::create_text_widget() {
        w.set_text("DDR World Universal Modpack");
        w.set_position(CONTENT_X, TITLE_Y);
        w.set_scale(TITLE_SCALE, TITLE_SCALE);
        set_rgb(&w, p.title);
        state.title_widget = Some(w);
    }
    if let Some(w) = widget_renderer::create_text_widget() {
        w.set_text(CREDIT_TEXT);
        w.set_position(CREDIT_X, CREDIT_Y);
        w.set_scale(CREDIT_SCALE, CREDIT_SCALE);
        set_rgb(&w, p.hints);
        state.title_credit_widget = Some(w);
    }

    for (i, _tab) in TabId::ALL.iter().enumerate() {
        if let Some(w) = widget_renderer::create_text_widget() {
            w.set_text(".");
            w.set_position(TAB_X0 + i as f32 * TAB_SPACING + TAB_TEXT_CENTER_OFF, TAB_Y);
            w.set_alignment(TextAlignment::Center);
            w.set_scale(TAB_SCALE_INACTIVE, TAB_SCALE_INACTIVE);
            set_rgb(&w, p.tab_inactive);
            state.tab_widgets.push(w);
        }
    }

    if let Some(w) = widget_renderer::create_text_widget() {
        w.set_text(".");
        w.set_position(RIGHT_X, TAB_Y);
        w.set_scale(0.5, 0.5);
        w.set_alignment(TextAlignment::Right);
        set_rgb(&w, p.hints);
        state.indicator_widget = Some(w);
    }

    if let Some(w) = widget_renderer::create_text_widget() {
        w.set_text(">");
        w.set_position(CURSOR_X, LIST_START_Y);
        w.set_scale(ROW_SCALE, ROW_SCALE);
        set_rgb(&w, p.tab_active);
        state.cursor_widget = Some(w);
    }

    for i in 0..VISIBLE_ROWS {
        let y = LIST_START_Y + i as f32 * ROW_H;
        let label = widget_renderer::create_text_widget();
        let value = widget_renderer::create_text_widget();
        if let (Some(lw), Some(vw)) = (label, value) {
            lw.set_text(".");
            lw.set_position(CONTENT_X, y);
            lw.set_scale(ROW_SCALE, ROW_SCALE);
            set_rgb(&lw, p.label);

            vw.set_text(".");
            vw.set_position(RIGHT_X, y);
            vw.set_scale(ROW_SCALE, ROW_SCALE);
            vw.set_alignment(TextAlignment::Right);
            set_rgb(&vw, p.value);

            state.slots.push(Slot {
                label: lw,
                value: vw,
            });
        }
    }

    if let Some(w) = widget_renderer::create_text_widget() {
        w.set_text(".");
        w.set_position(CONTENT_X, FOOTER_DESC_Y);
        w.set_scale(FOOTER_DESC_SCALE, FOOTER_DESC_SCALE);
        set_rgb(&w, p.footer);
        state.footer_desc_widget = Some(w);
    }
    if let Some(w) = widget_renderer::create_text_widget() {
        w.set_text(KEY_HINTS);
        w.set_position(CONTENT_X, FOOTER_HINTS_Y);
        w.set_scale(FOOTER_HINTS_SCALE, FOOTER_HINTS_SCALE);
        set_rgb(&w, p.hints);
        state.footer_hints_widget = Some(w);
    }

    // PLAYER SETTINGS pinned side selector + session/framework banner
    // (hidden on other tabs; refresh positions/paints them).
    if let Some(w) = widget_renderer::create_text_widget() {
        w.set_text(".");
        w.set_position(CONTENT_X, SELECTOR_Y);
        w.set_scale(SELECTOR_SCALE, SELECTOR_SCALE);
        set_rgb(&w, p.label);
        state.side_selector_widget = Some(w);
    }
    if let Some(w) = widget_renderer::create_text_widget() {
        w.set_text(".");
        w.set_position(BANNER_X, BANNER_Y);
        w.set_scale(BANNER_SCALE, BANNER_SCALE);
        w.set_alignment(TextAlignment::Center);
        set_rgb(&w, p.off_value);
        state.banner_widget = Some(w);
    }

    hide_all_widgets(state);
    state.widgets_allocated = true;
}

pub(super) fn destroy_widgets(state: &mut ModMenuState) {
    // Clear the emission anchor BEFORE freeing the anchor widget — the
    // wrapper hook compares against it on the render thread.
    crate::services::overlay_draw::clear_emit_anchor();
    if let Some(ref mut w) = state.bg_anchor_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.panel_widget {
        w.destroy();
    }
    for w in &mut state.header_bar_widgets {
        w.destroy();
    }
    state.header_bar_widgets.clear();
    if let Some(ref mut w) = state.tab_indicator_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.selection_bar_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.scroll_track_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.scroll_thumb_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.banner_backing_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.title_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.title_credit_widget {
        w.destroy();
    }
    for w in &mut state.tab_widgets {
        w.destroy();
    }
    state.tab_widgets.clear();
    if let Some(ref mut w) = state.indicator_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.cursor_widget {
        w.destroy();
    }
    for slot in &mut state.slots {
        slot.label.destroy();
        slot.value.destroy();
    }
    state.slots.clear();
    if let Some(ref mut w) = state.footer_desc_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.footer_hints_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.side_selector_widget {
        w.destroy();
    }
    if let Some(ref mut w) = state.banner_widget {
        w.destroy();
    }
    state.widgets_allocated = false;
}

/// Repaint everything from the active tab's row list + navigation state.
pub(super) fn refresh_all(state: &ModMenuState) {
    if !state.is_open {
        return;
    }

    let chrome_status = chrome_loader::status();
    let p = pal();

    // Panel: real texture when resolved; the tinted strip stretched over the
    // modal rect when the panel FAILED but the strip resolved (design §6
    // solid rung); hidden otherwise (text-only until resolve).
    if let Some(ref w) = state.panel_widget {
        if let Some(tex) = chrome_status.panel_tex {
            w.set_texture_id(tex);
            // Over a live animation the gradient dims to a wash so the
            // shader stays visible at every opacity.
            let tint = if crate::services::overlay_draw::is_background_active() {
                abgr(PANEL_ALPHA_OVER_ANIMATION, 255, 255, 255)
            } else {
                TINT_PANEL_NEUTRAL
            };
            w.set_color(tint);
            w.show();
        } else if chrome_status.panel_failed {
            if let Some(tex) = chrome_status.strip_tex {
                let base = p.panel_top;
                let alpha = chrome::opacity_alpha(chrome_status.opacity);
                w.set_texture_id(tex);
                w.set_color(abgr(alpha, base[0], base[1], base[2]));
                w.show();
            } else {
                w.hide();
            }
        } else {
            w.hide();
        }
    }

    // Every other chrome piece rides the strip texture.
    let strip_tex = chrome_status.strip_tex;

    // Title + static footer hints. Colors re-applied every repaint so a
    // THEME change repaints the creation-time-colored widgets too.
    if let Some(ref w) = state.title_widget {
        set_rgb(w, p.title);
        w.show();
    }
    if let Some(ref w) = state.title_credit_widget {
        set_rgb(w, p.hints);
        w.show();
    }
    if let Some(ref w) = state.footer_hints_widget {
        set_rgb(w, p.hints);
        w.show();
    }

    // Tab bar: active tab accent-colored + slightly larger (grow effect);
    // inactive tabs dim + smaller. No brackets (maintainer feedback).
    let active = state.tab_nav.active();
    for (tab, w) in TabId::ALL.iter().zip(state.tab_widgets.iter()) {
        w.set_text(tab.label());
        if *tab == active {
            w.set_scale(TAB_SCALE_ACTIVE, TAB_SCALE_ACTIVE);
            set_rgb(w, p.tab_active);
        } else {
            w.set_scale(TAB_SCALE_INACTIVE, TAB_SCALE_INACTIVE);
            set_rgb(w, p.tab_inactive);
        }
        w.show();
    }

    // Active-tab underline indicator.
    if let Some(ref w) = state.tab_indicator_widget {
        match strip_tex {
            Some(tex) => {
                let idx = active.index() as f32;
                w.set_texture_id(tex);
                w.set_color(tint(ALPHA_TAB_IND, p.accent));
                w.set_position(TAB_X0 + idx * TAB_SPACING + TAB_IND_X_OFF, TAB_IND_Y);
                w.show();
            }
            None => w.hide(),
        }
    }

    let tab_idx = active.index();
    let empty: Vec<super::model::Row> = Vec::new();
    let tab_rows = state.tab_rows.get(tab_idx).unwrap_or(&empty);
    let nav = state.tab_nav.state();
    let navigator = navigator_for(active, tab_rows);
    // The PLAYER tab's content shifts one band down (pinned selector) and
    // shows one fewer row; slot/bar/scroll geometry follows per refresh.
    let start_y = list_start_y(active);
    let page = visible_rows(active);
    let is_player_tab = active == TabId::PlayerSettings;

    // PLAYER SETTINGS pinned side selector + session/framework banner.
    let selector = super::model::selector_state(state.player_editable);
    if let Some(ref w) = state.side_selector_widget {
        if is_player_tab {
            let n = state.player_side + 1;
            let text = match selector {
                SelectorState::Free => format!("CONFIGURING:  < PLAYER {n} >"),
                SelectorState::Locked | SelectorState::AllGated => {
                    format!("CONFIGURING:  PLAYER {n}")
                }
            };
            w.set_text(&text);
            let focused = navigator.pinned_focused(&nav);
            let c = match (focused, selector) {
                (true, _) => p.tab_active,
                (false, SelectorState::Free) => p.label,
                (false, _) => p.greyed,
            };
            set_rgb(w, c);
            w.show();
        } else {
            w.hide();
        }
    }
    let banner_text = if is_player_tab && state.framework_unavailable {
        Some("OPTIONS FRAMEWORK UNAVAILABLE")
    } else if is_player_tab && selector == SelectorState::AllGated {
        Some("NO ACTIVE SESSION")
    } else {
        None
    };
    if let Some(ref w) = state.banner_widget {
        match banner_text {
            Some(text) => {
                w.set_text(text);
                set_rgb(w, p.off_value);
                w.show();
            }
            None => w.hide(),
        }
    }
    if let Some(ref w) = state.banner_backing_widget {
        match (banner_text, strip_tex) {
            (Some(_), Some(tex)) => {
                w.set_texture_id(tex);
                w.set_color(tint(ALPHA_BANNER_BACK, p.banner_back));
                w.show();
            }
            _ => w.hide(),
        }
    }

    // Rows in the visible window.
    let window = navigator.page_window(&nav);
    for (slot_i, slot) in state.slots.iter().enumerate() {
        let row_idx = window.start + slot_i;
        if row_idx >= window.end {
            slot.label.hide();
            slot.value.hide();
            if let Some(bar) = state.header_bar_widgets.get(slot_i) {
                bar.hide();
            }
            continue;
        }
        let row = &tab_rows[row_idx];
        let slot_y = start_y + slot_i as f32 * ROW_H;
        slot.label.set_position(CONTENT_X, slot_y);
        slot.value.set_position(RIGHT_X, slot_y);
        // Headers render uppercase over an accent backing bar, reading as
        // section dividers rather than options (maintainer feedback).
        let is_header = matches!(row.kind, RowKind::Header);
        if let Some(bar) = state.header_bar_widgets.get(slot_i) {
            match (is_header, strip_tex) {
                (true, Some(tex)) => {
                    bar.set_texture_id(tex);
                    bar.set_color(tint(ALPHA_HDRBAR, p.header_bar));
                    bar.set_position(HDRBAR_X, slot_y + HDRBAR_Y_OFF);
                    bar.show();
                }
                _ => bar.hide(),
            }
        }
        if is_header {
            slot.label.set_text(&row.label.to_uppercase());
        } else {
            slot.label.set_text(&row.label);
        }
        match (&row.kind, row.greyed) {
            (RowKind::Header, _) => set_rgb(&slot.label, p.header),
            (_, true) => set_rgb(&slot.label, p.greyed),
            (_, false) => set_rgb(&slot.label, p.label),
        }
        slot.label.show();

        match &row.kind {
            RowKind::Header => slot.value.hide(),
            RowKind::Boolean { value } => {
                slot.value.set_text(if *value { "ON" } else { "OFF" });
                let c = if row.greyed {
                    p.greyed
                } else if *value {
                    p.on_value
                } else {
                    p.off_value
                };
                set_rgb(&slot.value, c);
                slot.value.show();
            }
            RowKind::Scalar {
                value, formatted, ..
            } => {
                // Mirrored rows carry the framework's formatted text (display
                // parity with the in-game menu); otherwise explicit sign for
                // positive values makes polarity unambiguous.
                let text = match formatted {
                    Some(f) => f.clone(),
                    None if *value > 0 => format!("+{value}"),
                    None => format!("{value}"),
                };
                slot.value.set_text(&text);
                let c = if row.greyed { p.greyed } else { p.value };
                set_rgb(&slot.value, c);
                slot.value.show();
            }
            RowKind::Enum { index, labels, .. } => {
                let text = labels.get(*index).map(|s| s.as_str()).unwrap_or("?");
                slot.value.set_text(text);
                let c = if row.greyed { p.greyed } else { p.value };
                set_rgb(&slot.value, c);
                slot.value.show();
            }
        }
    }

    // Cursor + selection bar + footer description follow the selection —
    // or the pinned selector when it holds focus (PLAYER tab).
    let selected = navigator.selected(&nav);
    let selected_slot = selected.and_then(|idx| idx.checked_sub(nav.scroll));
    let pinned_focus = navigator.pinned_focused(&nav);
    // Highlight band Y: the focused slot's row band, or the selector band.
    let highlight_y = if pinned_focus {
        Some(SELECTOR_Y)
    } else {
        selected_slot.map(|slot_i| start_y + slot_i as f32 * ROW_H)
    };
    if let Some(ref cw) = state.cursor_widget {
        match highlight_y {
            Some(y) => {
                cw.set_position(CURSOR_X, y);
                set_rgb(cw, p.tab_active);
                cw.show();
            }
            None => cw.hide(),
        }
    }
    if let Some(ref bar) = state.selection_bar_widget {
        match (highlight_y, strip_tex) {
            (Some(y), Some(tex)) => {
                bar.set_texture_id(tex);
                bar.set_color(tint(ALPHA_SELBAR, p.accent));
                bar.set_position(SELBAR_X, y + SELBAR_Y_OFF);
                bar.show();
            }
            _ => bar.hide(),
        }
    }
    if let Some(ref w) = state.footer_desc_widget {
        let desc = if pinned_focus {
            "Switch which player's settings you are configuring"
        } else {
            selected
                .and_then(|i| tab_rows.get(i))
                .map(|r| r.description.as_str())
                .unwrap_or("")
        };
        w.set_text(desc);
        set_rgb(w, p.footer);
        w.show();
    }

    // Position indicator + proportional scrollbar, only when the list
    // overflows one page.
    let overflows = navigator.overflows();
    if let Some(ref w) = state.indicator_widget {
        if overflows {
            let (pos, total) = navigator.scroll_indicator(&nav);
            w.set_text(&format!("{pos}/{total}"));
            set_rgb(w, p.hints);
            w.show();
        } else {
            w.hide();
        }
    }
    let scroll_visible = overflows && strip_tex.is_some();
    if let (Some(track), Some(thumb)) = (&state.scroll_track_widget, &state.scroll_thumb_widget) {
        if scroll_visible {
            let tex = strip_tex.unwrap_or_default();
            let len = tab_rows.len() as f32;
            let page_f = page as f32;
            let track_y = start_y - 4.0;
            let track_h = page_f * ROW_H + 8.0;
            let thumb_h = (track_h * page_f / len.max(1.0)).max(SCROLL_THUMB_MIN_H);
            let denom = (len - page_f).max(1.0);
            let frac = (nav.scroll as f32 / denom).clamp(0.0, 1.0);
            let thumb_y = track_y + (track_h - thumb_h) * frac;
            track.set_texture_id(tex);
            track.set_color(TINT_SCROLL_TRACK);
            track.set_position(SCROLL_X, track_y);
            track.set_size(SCROLL_W, track_h);
            track.show();
            thumb.set_texture_id(tex);
            thumb.set_color(TINT_SCROLL_THUMB);
            thumb.set_position(SCROLL_X, thumb_y);
            thumb.set_size(SCROLL_W, thumb_h);
            thumb.show();
        } else {
            track.hide();
            thumb.hide();
        }
    }
}

pub(super) fn hide_all_widgets(state: &ModMenuState) {
    if let Some(ref w) = state.panel_widget {
        w.hide();
    }
    for w in &state.header_bar_widgets {
        w.hide();
    }
    if let Some(ref w) = state.tab_indicator_widget {
        w.hide();
    }
    if let Some(ref w) = state.selection_bar_widget {
        w.hide();
    }
    if let Some(ref w) = state.scroll_track_widget {
        w.hide();
    }
    if let Some(ref w) = state.scroll_thumb_widget {
        w.hide();
    }
    if let Some(ref w) = state.title_widget {
        w.hide();
    }
    if let Some(ref w) = state.title_credit_widget {
        w.hide();
    }
    for w in &state.tab_widgets {
        w.hide();
    }
    if let Some(ref w) = state.indicator_widget {
        w.hide();
    }
    if let Some(ref w) = state.cursor_widget {
        w.hide();
    }
    for slot in &state.slots {
        slot.label.hide();
        slot.value.hide();
    }
    if let Some(ref w) = state.footer_desc_widget {
        w.hide();
    }
    if let Some(ref w) = state.footer_hints_widget {
        w.hide();
    }
    if let Some(ref w) = state.side_selector_widget {
        w.hide();
    }
    if let Some(ref w) = state.banner_widget {
        w.hide();
    }
    if let Some(ref w) = state.banner_backing_widget {
        w.hide();
    }
}
