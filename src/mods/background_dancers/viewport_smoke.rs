//! `DDR_DANCERS_VIEWPORT_SMOKE` — the developer-mode cabinet smoke for the
//! viewport-pass compositor (design §7 item 3; plan Step 3): at each
//! SONG_SELECT entry attach a [`viewport_pass::PassSet`] with an opaque
//! violet colour+depth clear over the P1 preview-box rect for
//! [`ATTACHED_MS`], disable it for [`DISABLED_MS`], then detach — and log
//! every transition with the rect in render-target pixels. No scene, no
//! items: a solid rectangle exactly over the box proves the rect + Clear
//! semantics (and that RENDER_2D priorities ≥ 0x68 draw above the UI); a
//! bleed, a missing rectangle or a crash invalidates design §4.5 and stops
//! the feature. Kept after the step as a bisect tool.
//!
//! Gate: `layeredfs.developer_mode` ∧ env `DDR_DANCERS_VIEWPORT_SMOKE=1`,
//! read once at mod enable. All attach/detach work happens in the mod's
//! `on_frame` callback (game thread, mid-frame — the compositor's contract);
//! the scene callback only records requests.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::mods::webui_options::discovery::MarkerColor;
use crate::mods::webui_options::preview_gen;
use crate::services::scene3d::camera_math::{view_proj, Frustum};
use crate::services::scene3d::viewport_pass::{self, ClearSpec, PassSet, RtRect};
use crate::services::scene3d::viewport_pass_layout::{FILTER_BIT, PRIO_BASE};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use super::preview::layout::{CHROME_ORIGIN, FALLBACK_MARKER};

const ATTACHED_MS: u128 = 3000;
const DISABLED_MS: u128 = 1000;
/// Opaque violet — a D3DCOLOR `0xAARRGGBB`: A FF, R A0, G 20, B FF. (The
/// first cabinet smoke shipped `0xFF20A0FF` = R 20 / G A0 / B FF, an azure
/// rectangle — which proved the record is consumed as ARGB exactly as
/// encoded, 2026-09-21.)
const SMOKE_COLOR: u32 = 0xFFA0_20FF;

static ENABLED: AtomicBool = AtomicBool::new(false);
/// 0 idle, 1 = arm requested (SONG_SELECT entered), 2 = teardown requested.
static REQUEST: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Attached,
    Disabled,
}

struct Smoke {
    set: PassSet,
    phase: Phase,
    since: Instant,
}

static LIVE: Mutex<Option<Smoke>> = Mutex::new(None);

/// Read the gate once (mod enable).
pub fn init_from_env() {
    let dev_mode = crate::mods::config::get()
        .and_then(|c| c.layeredfs.as_ref())
        .map(|l| l.developer_mode)
        .unwrap_or(false);
    let on = dev_mode && std::env::var_os("DDR_DANCERS_VIEWPORT_SMOKE").is_some();
    ENABLED.store(on, Ordering::Release);
    if on {
        log_warn!(
            "viewport_smoke: DDR_DANCERS_VIEWPORT_SMOKE -- a violet rectangle will cover the P1 preview box for {} ms at every song-select entry (bisect mode)",
            ATTACHED_MS
        );
    }
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Scene callback (game thread): record the arm / teardown request.
pub fn on_scene_change(prev: i32, next: i32) {
    if !is_enabled() {
        return;
    }
    if next == scene::SONG_SELECT && prev != scene::SONG_SELECT {
        REQUEST.store(1, Ordering::Release);
    } else if prev == scene::SONG_SELECT && next != scene::SONG_SELECT {
        REQUEST.store(2, Ordering::Release);
    }
}

/// The P1 box in render-target pixels, or `None` while the display has no
/// readable target yet.
fn p1_box() -> Option<(RtRect, (u32, u32))> {
    let (mx, my, mw, mh) =
        match preview_gen::marker_rect_for("background_dancer", MarkerColor::Green) {
            Some(m) => (m.x as f32, m.y as f32, m.w as f32, m.h as f32),
            None => FALLBACK_MARKER,
        };
    let (ox, oy) = CHROME_ORIGIN[0];
    let dims = viewport_pass::render_target_dims()?;
    Some((RtRect::from_canvas(ox + mx, oy + my, mw, mh, dims), dims))
}

/// Per-frame driver (game thread, from the mod's frame callback).
pub fn on_frame() {
    if !is_enabled() {
        return;
    }
    let Ok(mut live) = LIVE.lock() else { return };
    match REQUEST.swap(0, Ordering::AcqRel) {
        1 => {
            if let Some(old) = live.take() {
                old.set.detach();
            }
            if !viewport_pass::is_available() {
                log_warn!("viewport_smoke: compositor unavailable -- nothing to smoke");
                return;
            }
            let Some((rect, dims)) = p1_box() else {
                // The target is not readable this frame: re-arm and retry.
                REQUEST.store(1, Ordering::Release);
                return;
            };
            let Some(mut set) = viewport_pass::create(
                FILTER_BIT[0],
                rect,
                ClearSpec {
                    depth: true,
                    color: Some(SMOKE_COLOR),
                },
                PRIO_BASE[0],
            ) else {
                log_warn!(
                    "viewport_smoke: PassSet::create failed -- see the viewport_pass WARN above"
                );
                return;
            };
            let (view, proj) = view_proj(&Frustum::perspective(
                [0.0, 1.6, 5.0],
                [0.0, 0.9, 0.0],
                [0.0, 1.0, 0.0],
                0.79 * rect.aspect(),
                rect.aspect(),
                0.1,
                100.0,
            ));
            set.set_camera(&view, &proj);
            log_info!(
                "viewport_smoke: attached P1 clear@{:#x} opaque@{:#x} trans@{:#x} rect=({},{},{},{}) rt={}x{} color={:#010x}",
                PRIO_BASE[0],
                PRIO_BASE[0] + 1,
                PRIO_BASE[0] + 2,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                dims.0,
                dims.1,
                SMOKE_COLOR
            );
            *live = Some(Smoke {
                set,
                phase: Phase::Attached,
                since: Instant::now(),
            });
        }
        2 => {
            if let Some(old) = live.take() {
                old.set.detach();
                log_info!("viewport_smoke: detached at song-select exit");
            }
        }
        _ => {}
    }
    let Some(s) = live.as_mut() else { return };
    let elapsed = s.since.elapsed().as_millis();
    match s.phase {
        Phase::Attached if elapsed >= ATTACHED_MS => {
            s.set.set_enabled(false);
            s.phase = Phase::Disabled;
            s.since = Instant::now();
            log_info!(
                "viewport_smoke: disabled (DISABLED bit) for {} ms",
                DISABLED_MS
            );
        }
        Phase::Disabled if elapsed >= DISABLED_MS => {
            if let Some(done) = live.take() {
                done.set.detach();
                log_info!("viewport_smoke: detached -- sequence complete (attach 3 s / disable 1 s / detach)");
            }
        }
        _ => {}
    }
}

/// Mod disable: drop anything live (detaches).
pub fn shutdown() {
    if let Ok(mut live) = LIVE.lock() {
        if let Some(old) = live.take() {
            old.set.detach();
        }
    }
    REQUEST.store(0, Ordering::Release);
}
