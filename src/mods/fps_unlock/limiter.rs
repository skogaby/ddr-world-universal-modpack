//! The sub-60 frame limiter (installed ONLY when the boot target is below the
//! stock 60 — see `pacing::mode_for`; targets at or above 60 never reach this
//! file).
//!
//! Seam: a post-original detour on the app tick's `frame_begin` — the
//! once-per-frame function that waits for the GPU executor to go idle and
//! submits the previous frame's command stream (single caller, the per-frame
//! application tick; `docs/fps_frame_limiter.md`). Waiting AFTER the original
//! means:
//! - the previous frame is already on its way to `Present` while we wait
//!   (the GPU is never held back by the limiter);
//! - input, the frame delta-time and the whole update run right after the
//!   wait, so the limiter adds no input-to-present latency beyond the lower
//!   frame rate itself.
//!
//! The wait is a deadline-grid pacer (`pacing::Pacer`) on `Instant` (QPC):
//! coarse `thread::sleep` up to [`SPIN_MARGIN`] before the deadline, then
//! `yield_now` until it. `timeBeginPeriod(1)` is requested at install so the
//! coarse sleep has 1 ms granularity even if nothing else in the process
//! asked for it.
//!
//! The game clamps each frame's delta-time to `2 / 59.94` s (≈33.4 ms). A
//! 50 ms (20 fps) frame would be clipped every frame — every dt-driven
//! animation would run at 2/3 speed — and a 33.3 ms (30 fps) frame sits right
//! at the edge. The detour therefore raises that global (`frame_dt_clamp`,
//! written once by onBoot) to `pacing::limiter_dt_clamp` on every frame it
//! paces, and `set_active(false)` restores the captured stock value.
//!
//! Hot-path discipline: the callback does one atomic load, one f32 compare,
//! an uncontended mutex and the deliberate wait. No allocation, no logging
//! after the first frame, panics contained.

use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use retour::GenericDetour;

use super::pacing::{self, Pacer};
use crate::core::hooks;
use crate::core::memory;
use crate::core::signatures::FrameLimiterAnchors;
use crate::{log_info, log_warn};

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(period_ms: u32) -> u32;
}

type FrameBeginFn = unsafe extern "C" fn();

static mut DETOUR: Option<GenericDetour<FrameBeginFn>> = None;
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Pacing on/off. Cleared by `set_active(false)` (menu toggle OFF); the
/// detour then forwards the original untouched.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static TARGET_FPS: AtomicU32 = AtomicU32::new(0);
static CLAMP_ADDR: AtomicUsize = AtomicUsize::new(0);
/// f32 bits of the clamp the limiter writes.
static CLAMP_WANT: AtomicU32 = AtomicU32::new(0);
/// f32 bits of the game's own clamp, captured on the first paced frame
/// (onBoot has written it by then); `NO_STOCK` until captured.
static CLAMP_STOCK: AtomicU32 = AtomicU32::new(NO_STOCK);
const NO_STOCK: u32 = u32::MAX;
static PACER: Mutex<Option<Pacer>> = Mutex::new(None);
static EPOCH: OnceLock<Instant> = OnceLock::new();
static FIRST_FRAME_LOGGED: AtomicBool = AtomicBool::new(false);

/// Sleep coarsely until this close to the deadline, then yield-spin.
const SPIN_MARGIN: Duration = Duration::from_millis(2);

/// Install the limiter for `fps` (must be a `pacing::Mode::Limit` target).
/// Needs both anchors: pacing without the dt-clamp raise would slow every
/// dt-driven animation at 20 fps, so a missing clamp installs nothing.
pub fn install(anchors: &FrameLimiterAnchors, fps: u32) -> Result<(), String> {
    if INSTALLED.load(Ordering::Acquire) {
        return Ok(());
    }
    let target = anchors
        .frame_begin
        .ok_or("frame_begin unresolved (app_tick_frame_begin_site shape)")?;
    let clamp = anchors
        .frame_dt_clamp
        .ok_or("frame_dt_clamp unresolved (frame_dt shape)")?;
    if !memory::is_readable(clamp, 4) {
        return Err(format!("frame_dt_clamp {clamp:p} unreadable"));
    }
    let fps = fps.max(1);
    let want = pacing::limiter_dt_clamp(fps, 0.0);

    TARGET_FPS.store(fps, Ordering::Release);
    CLAMP_ADDR.store(clamp as usize, Ordering::Release);
    CLAMP_WANT.store(want.to_bits(), Ordering::Release);
    if let Ok(mut p) = PACER.lock() {
        *p = Some(Pacer::new(pacing::frame_period_ns(fps)));
    }
    let _ = EPOCH.get_or_init(Instant::now);
    ACTIVE.store(true, Ordering::Release);

    unsafe {
        let target: FrameBeginFn = std::mem::transmute(target);
        if let Err(e) = hooks::install_enabled(
            std::ptr::addr_of_mut!(DETOUR),
            target,
            frame_begin_detour as FrameBeginFn,
        ) {
            ACTIVE.store(false, Ordering::Release);
            return Err(format!("frame_begin detour install failed: {e}"));
        }
        let rc = timeBeginPeriod(1);
        if rc != 0 {
            log_warn!("FpsUnlock: timeBeginPeriod(1) returned {rc} -- limiter waits may overshoot");
        }
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!(
        "FpsUnlock: frame limiter installed -- {fps}fps ({:.3} ms/frame), dt clamp -> {:.1} ms",
        pacing::frame_period_ns(fps) as f64 / 1e6,
        want * 1000.0
    );
    Ok(())
}

pub fn installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// Resume / suspend pacing (menu toggle). Suspending restores the game's
/// own dt clamp; resuming starts a fresh pacing grid. No-op when the limiter
/// was never installed.
pub fn set_active(active: bool) {
    if !installed() {
        return;
    }
    if active {
        if let Ok(mut p) = PACER.lock() {
            if let Some(p) = p.as_mut() {
                p.reset();
            }
        }
        ACTIVE.store(true, Ordering::Release);
        log_info!(
            "FpsUnlock: frame limiter resumed ({}fps)",
            TARGET_FPS.load(Ordering::Acquire)
        );
    } else {
        ACTIVE.store(false, Ordering::Release);
        let stock = CLAMP_STOCK.load(Ordering::Acquire);
        let clamp = CLAMP_ADDR.load(Ordering::Acquire) as *mut u8;
        if stock != NO_STOCK && !clamp.is_null() {
            // Aligned 4-byte store to a game global the main thread only
            // reads once per frame.
            unsafe { memory::write_f32(clamp, f32::from_bits(stock)) };
        }
        log_info!("FpsUnlock: frame limiter suspended (stock pacing and dt clamp restored)");
    }
}

unsafe extern "C" fn frame_begin_detour() {
    if let Some(hook) = (*addr_of!(DETOUR)).as_ref() {
        hook.call();
    }
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let _ = std::panic::catch_unwind(|| {
        raise_dt_clamp();
        pace();
    });
}

/// Keep the game's per-frame dt clamp at least at the limiter's value. The
/// first call captures the stock value (for `set_active(false)`).
fn raise_dt_clamp() {
    let clamp = CLAMP_ADDR.load(Ordering::Acquire) as *mut u8;
    if clamp.is_null() {
        return;
    }
    let want_bits = CLAMP_WANT.load(Ordering::Acquire);
    let cur = unsafe { memory::read_f32(clamp) };
    if CLAMP_STOCK.load(Ordering::Acquire) == NO_STOCK && cur.to_bits() != want_bits {
        CLAMP_STOCK.store(cur.to_bits(), Ordering::Release);
    }
    let want = pacing::limiter_dt_clamp(TARGET_FPS.load(Ordering::Acquire), cur);
    if want.to_bits() != cur.to_bits() {
        unsafe { memory::write_f32(clamp, want) };
    }
    if !FIRST_FRAME_LOGGED.swap(true, Ordering::AcqRel) {
        log_info!(
            "FpsUnlock: first paced frame -- game dt clamp {:.2} ms -> {:.2} ms",
            cur * 1000.0,
            want * 1000.0
        );
    }
}

fn pace() {
    let Some(epoch) = EPOCH.get() else {
        return;
    };
    let now_ns = Instant::now().duration_since(*epoch).as_nanos() as u64;
    // Lock scope ends before the wait.
    let wait = match PACER.lock() {
        Ok(mut p) => p.as_mut().map_or(0, |p| p.wait_ns(now_ns)),
        Err(_) => 0,
    };
    if wait > 0 {
        wait_precise(Duration::from_nanos(wait));
    }
}

fn wait_precise(d: Duration) {
    let deadline = Instant::now() + d;
    loop {
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        let left = deadline - now;
        if left > SPIN_MARGIN {
            std::thread::sleep(left - SPIN_MARGIN);
        } else {
            std::thread::yield_now();
        }
    }
}
