//! Legacy life gauge — A3's gauge behaviour on World's gauge actors.
//!
//! World's gauge actors (the percent family `Normal` / `Grade` / `Flare` /
//! `Immortal`, and `LifeGaugeActor`) are A3's with four things removed; this
//! module puts them back for a song whose `dance_gauge` record is legacy
//! (`dance_gauge000N`, or a theme's `dance_gauge0000_vN`; record skin N at
//! the actor's `+skin` field):
//!
//! * **Export name.** World creates `dance_gauge`, A3's packages export
//!   `00_dance_gauge` — a checked code patch of each init's clip-create LEA
//!   (→ a near-allocated `"00_dance_gauge"`), live exactly while the current
//!   `LayoutActor`'s `dance_gauge` records are legacy (applied by the package
//!   helper before it registers the package; restored on a stock request,
//!   at disarm and disable). A patch failure keeps the package stock (World's
//!   init NULL-derefs on a package without its export).
//! * **2P mirror.** A3 drew the 2P percent gauge at `SetScale(-1, 1)`;
//!   World at `(1, 1)` — post-original detour on the percent-family init.
//! * **Fill.** A3's segmented fill (skins 1, 5 and the themes, non-FLARE) and
//!   its 2P-mirrored continuous fill (skins 2–4, FLARE) replace World's
//!   continuous one — full-replacement detour on World's fill, pure math in
//!   [`super::gauge_math`]. The fill reads only the displayed value, so
//!   `song_reset`'s gauge restore stays valid.
//! * **Skin-3 intro on LIFE4 / RISKY.** A3's LifeGaugeActor played the root
//!   label `1p_in` / `2p_in` on skin 3 (World kept it only in the percent
//!   family) — post-original detour on the LifeGaugeActor init.
//!
//! FLARE / GRADE labels the legacy art lacks are requested and miss, exactly
//! as in A3. Game thread only (actor init / update). RE:
//! `.agents/planning/2026-09-22-ddr-selection/research/legacy-gauge.md`.

use std::ffi::c_void;
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::memory;
use crate::core::signatures::{DdrSelGaugeSites, SignatureStore};
use crate::services::bm2d_api;
use crate::{log_info, log_warn};

use super::gauge_math::{self, FillClip};
use super::policy;

/// A3's export in `dance_gauge000N`.
const EXPORT: &[u8] = b"00_dance_gauge\0";
const PARAM_VISIBLE: i32 = 0x1007;
const PARAM_POSITION: i32 = 0x1008;
const PARAM_WIDTH: i32 = 0x1015;
const PARAM_HEIGHT: i32 = 0x1016;
const PARAM_DIRTY: i32 = 0x101E;
const PARAM_SCISSOR: i32 = 0x1023;
const OP_GOTO_PLAY_LABEL: i32 = 0xF03;
const TRAVERSE_SIBLINGS: i32 = 6;

type ActorFn = unsafe extern "C" fn(*mut u8);
type SetScaleFn = unsafe extern "C" fn(*mut u8, f32, f32);
type LabelFn = unsafe extern "C" fn(*mut u8, i32) -> i32;

static mut GAUGE_INIT_HOOK: Option<GenericDetour<ActorFn>> = None;
static mut LIFE_INIT_HOOK: Option<GenericDetour<ActorFn>> = None;
static mut FILL_HOOK: Option<GenericDetour<ActorFn>> = None;

struct State {
    s: DdrSelGaugeSites,
    buffer: usize,
    stock: [[u8; 4]; 2],
}
unsafe impl Send for State {}
unsafe impl Sync for State {}

static STATE: OnceLock<State> = OnceLock::new();
static CAPABLE: AtomicBool = AtomicBool::new(false);
static APPLIED: AtomicBool = AtomicBool::new(false);
static BROKEN: AtomicBool = AtomicBool::new(false);
static LOCK: Mutex<()> = Mutex::new(());
/// One INFO per legacy song (the first actor to init / fill).
static LOGGED_INIT: AtomicBool = AtomicBool::new(false);
static LOGGED_FILL: AtomicBool = AtomicBool::new(false);

fn rel32(from_next: usize, to: usize) -> Option<i32> {
    i32::try_from(to as i64 - from_next as i64).ok()
}

fn leas(st: &State) -> [usize; 2] {
    [
        st.s.gauge_export_lea as usize,
        st.s.life_export_lea as usize,
    ]
}

/// Resolve the sites and build the near buffer (mod init).
pub fn init(signatures: &SignatureStore) -> bool {
    let Some(s) = signatures.ddr_sel_gauge_sites() else {
        log_warn!("DDR SELECTION: legacy-gauge sites unresolved -- the life gauge stays World's");
        return false;
    };
    let buffer = unsafe { memory::alloc_near(s.gauge_export_lea, 0x1000) } as usize;
    if buffer == 0 {
        log_warn!("DDR SELECTION: no near buffer for the gauge export name -- the life gauge stays World's");
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(EXPORT.as_ptr(), buffer as *mut u8, EXPORT.len());
    }
    let mut stock = [[0u8; 4]; 2];
    let lea = [s.gauge_export_lea as usize, s.life_export_lea as usize];
    for (i, l) in lea.iter().enumerate() {
        if rel32(l + 7, buffer).is_none() {
            log_warn!("DDR SELECTION: gauge export name out of rel32 reach -- the life gauge stays World's");
            return false;
        }
        unsafe { std::ptr::copy_nonoverlapping((l + 3) as *const u8, stock[i].as_mut_ptr(), 4) };
    }
    let _ = STATE.set(State { s, buffer, stock });
    true
}

/// Install the three detours (mod enable; they stay for the session).
pub fn start() -> bool {
    if CAPABLE.load(Ordering::Acquire) {
        return true;
    }
    let Some(st) = STATE.get() else {
        return false;
    };
    if !bm2d_api::is_available() {
        log_warn!("DDR SELECTION: MovieClip API unavailable -- the life gauge stays World's");
        return false;
    }
    unsafe {
        let installs: [(
            *mut Option<GenericDetour<ActorFn>>,
            *const u8,
            ActorFn,
            &str,
        ); 3] = [
            (
                std::ptr::addr_of_mut!(GAUGE_INIT_HOOK),
                st.s.gauge_init,
                gauge_init_hook,
                "gauge init",
            ),
            (
                std::ptr::addr_of_mut!(LIFE_INIT_HOOK),
                st.s.life_init,
                life_init_hook,
                "LifeGauge init",
            ),
            (
                std::ptr::addr_of_mut!(FILL_HOOK),
                st.s.gauge_fill,
                fill_hook,
                "gauge fill",
            ),
        ];
        for (storage, target, cb, what) in installs {
            if (*storage).is_some() {
                continue;
            }
            let t: ActorFn = std::mem::transmute(target);
            if let Err(e) = hooks::install_enabled(storage, t, cb) {
                log_warn!(
                    "DDR SELECTION: {} detour failed ({:?}) -- the life gauge stays World's",
                    what,
                    e
                );
                return false;
            }
        }
    }
    CAPABLE.store(true, Ordering::Release);
    log_info!("DDR SELECTION: legacy life gauge ready (export alias, 2P mirror, A3 fills)");
    true
}

/// Whether `dance_gauge` can turn legacy on this boot (the `Gauge` adapter).
pub fn capable() -> bool {
    CAPABLE.load(Ordering::Acquire) && !BROKEN.load(Ordering::Acquire)
}

fn switch(to_legacy: bool) -> bool {
    let Some(st) = STATE.get() else {
        return !to_legacy;
    };
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if APPLIED.load(Ordering::Acquire) == to_legacy {
        return true;
    }
    let bytes = |i: usize, l: usize, legacy: bool| -> Option<[u8; 4]> {
        if legacy {
            Some(rel32(l + 7, st.buffer)?.to_le_bytes())
        } else {
            Some(st.stock[i])
        }
    };
    let mut done: Vec<(usize, [u8; 4], [u8; 4])> = Vec::new();
    for (i, l) in leas(st).into_iter().enumerate() {
        let (Some(old), Some(new)) = (bytes(i, l, !to_legacy), bytes(i, l, to_legacy)) else {
            return false;
        };
        if let Err(e) = unsafe { memory::apply_checked_patch((l + 3) as *mut u8, &old, &new) } {
            log_warn!(
                "DDR SELECTION: gauge export patch failed ({:?}) -- the life gauge stays World's",
                e
            );
            for (at, o, n) in done {
                let _ = unsafe { memory::apply_checked_patch((at + 3) as *mut u8, &n, &o) };
            }
            BROKEN.store(true, Ordering::Release);
            return false;
        }
        done.push((l, old, new));
    }
    APPLIED.store(to_legacy, Ordering::Release);
    if to_legacy {
        LOGGED_INIT.store(false, Ordering::Relaxed);
        LOGGED_FILL.store(false, Ordering::Relaxed);
        log_info!("DDR SELECTION: gauge actors -> A3 export 00_dance_gauge");
    }
    true
}

/// Patch in A3's export (package helper, before it registers
/// `dance_gauge000N`). `false` ⇒ keep `dance_gauge` stock.
pub fn apply() -> bool {
    capable() && switch(true)
}

/// World's export back (a stock `dance_gauge`, disarm, disable).
pub fn restore() {
    if APPLIED.load(Ordering::Acquire) && !switch(false) {
        log_warn!("DDR SELECTION: could not restore World's gauge export name");
    }
}

/// The actor's legacy skin (1..=[`policy::SKIN_MAX`]) and side, or `None`
/// for a World gauge.
unsafe fn legacy_actor(actor: *mut u8, skin_off: usize, st: &State) -> Option<(u8, u8)> {
    if actor.is_null() || super::armed_skin() == 0 {
        return None;
    }
    if !memory::is_readable(actor, skin_off.max(st.s.side_off) + 8) {
        return None;
    }
    let skin = memory::read_i32(actor.add(skin_off));
    if !(1..=policy::SKIN_MAX as i32).contains(&skin) {
        return None;
    }
    let parent = memory::read_ptr(actor.add(st.s.side_off));
    if !memory::is_readable(parent, 4) {
        return None;
    }
    let side = memory::read_i32(parent);
    Some((skin as u8, (side != 0) as u8))
}

/// The actor's `CMovieClip` and its layer id.
unsafe fn clip_of(actor: *mut u8, clip_off: usize) -> Option<(*mut u8, u32)> {
    let clip = memory::read_ptr(actor.add(clip_off)) as *mut u8;
    if !memory::is_readable(clip, 0x120) {
        return None;
    }
    Some((clip, memory::read_u32(clip.add(8))))
}

unsafe extern "C" fn gauge_init_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(GAUGE_INIT_HOOK)).as_ref() else {
        return;
    };
    hook.call(actor);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let Some(st) = STATE.get() else {
            return;
        };
        let Some((skin, side)) = legacy_actor(actor, st.s.gauge_skin_off, st) else {
            return;
        };
        let mirrored = side == 1
            && clip_of(actor, st.s.gauge_clip_off).is_some_and(|(clip, _)| {
                let vt = memory::read_ptr(clip);
                let f = memory::read_ptr(vt.add(st.s.clip_set_scale_vslot));
                if !memory::is_readable(f, 16) {
                    return false;
                }
                let set_scale: SetScaleFn = std::mem::transmute(f);
                set_scale(clip, -1.0, 1.0);
                true
            });
        if !LOGGED_INIT.swap(true, Ordering::Relaxed) {
            log_info!(
                "DDR SELECTION: legacy gauge created (skin {}, {}P{})",
                skin,
                side + 1,
                if mirrored { ", mirrored" } else { "" }
            );
        }
    }));
}

unsafe extern "C" fn life_init_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(LIFE_INIT_HOOK)).as_ref() else {
        return;
    };
    hook.call(actor);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let Some(st) = STATE.get() else {
            return;
        };
        let Some((skin, side)) = legacy_actor(actor, st.s.life_skin_off, st) else {
            return;
        };
        if skin != 3 {
            return;
        }
        let Some((clip, _)) = clip_of(actor, st.s.life_clip_off) else {
            return;
        };
        let root = memory::read_i32(clip.add(st.s.clip_root_mc_off));
        if root < 1 {
            return;
        }
        let label: &std::ffi::CStr = if side == 0 { c"1p_in" } else { c"2p_in" };
        let ok = bm2d_api::mc_op_str(root as u32, OP_GOTO_PLAY_LABEL, label);
        log_info!(
            "DDR SELECTION: legacy LIFE gauge intro {} ({})",
            label.to_str().unwrap_or("?"),
            if ok { "played" } else { "refused" }
        );
    }));
}

/// World's `CMovieClip::SetVisible`: every clip of the sibling chain.
fn set_visible(first: u32, visible: bool) {
    let mut mc = Some(first);
    let mut guard = 0;
    while let Some(id) = mc {
        bm2d_api::mc_set_param(id, PARAM_VISIBLE, visible as i32);
        bm2d_api::mc_set_param(id, PARAM_DIRTY, 1);
        mc = bm2d_api::mc_traversal(id, TRAVERSE_SIBLINGS);
        guard += 1;
        if guard > 64 {
            break;
        }
    }
}

/// A3's fill for one legacy percent-family actor. `false` ⇒ run World's.
unsafe fn legacy_fill(actor: *mut u8, st: &State) -> bool {
    let Some((skin, side)) = legacy_actor(actor, st.s.gauge_skin_off, st) else {
        return false;
    };
    let Some((_, layer)) = clip_of(actor, st.s.gauge_clip_off) else {
        return false;
    };
    let (Some(fill), Some(fill2)) = (
        bm2d_api::layer_find_child(layer, "fill _usr"),
        bm2d_api::layer_find_child(layer, "fill _2_usr"),
    ) else {
        return false;
    };
    // The state's label (World's pure `vt+label(state)` getter).
    let state = memory::read_i32(actor.add(st.s.gauge_state_off));
    let vt = memory::read_ptr(actor);
    let f = memory::read_ptr(vt.add(st.s.gauge_label_vslot));
    if !memory::is_readable(f, 16) {
        return false;
    }
    let label_of: LabelFn = std::mem::transmute(f);
    let label = label_of(actor, state);
    let mode = gauge_math::fill_mode(skin, label);
    let value = memory::read_f32(actor.add(st.s.gauge_value_off));

    if gauge_math::clamp01(value) >= 1.0 {
        set_visible(fill, false);
        set_visible(fill2, false);
        return true;
    }
    set_visible(fill, true);
    let Some((x, y)) = bm2d_api::mc_get_vec2(fill, PARAM_POSITION) else {
        return true;
    };
    let clip = FillClip {
        x,
        y,
        w: bm2d_api::mc_get_param(fill, PARAM_WIDTH).unwrap_or(0),
        h: bm2d_api::mc_get_param(fill, PARAM_HEIGHT).unwrap_or(0),
    };
    let Some(out) = gauge_math::fill(mode, value, side, clip) else {
        return true;
    };
    match out.partial {
        Some(rect) => {
            set_visible(fill2, true);
            bm2d_api::mc_set_param_ptr(fill2, PARAM_SCISSOR, rect.as_ptr() as *const c_void);
        }
        None => set_visible(fill2, false),
    }
    bm2d_api::mc_set_param_ptr(fill, PARAM_SCISSOR, out.main.as_ptr() as *const c_void);
    if !LOGGED_FILL.swap(true, Ordering::Relaxed) {
        log_info!(
            "DDR SELECTION: legacy gauge fill (skin {}, {}P, {:?}, label {})",
            skin,
            side + 1,
            mode,
            label
        );
    }
    true
}

unsafe extern "C" fn fill_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(FILL_HOOK)).as_ref() else {
        return;
    };
    let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        STATE.get().is_some_and(|st| legacy_fill(actor, st))
    }))
    .unwrap_or(false);
    if !handled {
        hook.call(actor);
    }
}
