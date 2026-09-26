//! Legacy score — A3's `ScoreActor` display on World's.
//!
//! World's `ScoreActor` is A3's with its export names, texture names and
//! difficulty display changed and a player-name clip added; this module puts
//! A3's back for an actor whose `dance_score` record is legacy
//! (`dance_score000N` or a theme's `dance_score0000_vN`, record skin N):
//!
//! * **Exports** (init PRE / POST detour): for the one init call, checked
//!   patches of World's three clip creates (`MOV R9D,7; LEA R8,[name]`) make
//!   World's own init create A3's clips — `dance_score` → `frame_score`,
//!   `dance_difficulty` → `frame_difficulty_<n>p[_reverse]` (a per-call near
//!   buffer; skin 2 at A3's priority 3), `dance_name` → a small stand-in
//!   export (the legacy UIs had no player name; hidden after the init — World
//!   never releases that clip, exactly like its own name clip). POST restores
//!   the bytes and runs A3's EX indicator (`ex_tex` visible in EX mode); on a
//!   theme it hides the frame's `name_usr` and hands it to `score_name.rs`
//!   (A3's dancer name).
//! * **Digits** (full-replacement detour on World's digit refresh, slot 7):
//!   World's smoothing and place walk with A3's textures
//!   (`dance_score000N_score_num_*`, `…_0_gray` leading zeros, commas) — a
//!   negative displayed value (`song_reset`'s repaint sentinel) repaints all.
//! * **Difficulty** (msg `0x104F` override, A3 `0x1052`): the
//!   `difficulty_level_usr` label / level texture and skin 2's
//!   `difficulty_level_base_usr` label.
//!
//! Textures / labels the legacy art lacks (skin 1 has no leading-zero or level
//! art) are requested and miss, as in A3. Game thread only. RE:
//! `.agents/planning/2026-09-22-ddr-selection/research/legacy-score.md`.

use std::ffi::{c_char, CString};
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::memory;
use crate::core::signatures::{DdrSelScoreSites, SignatureStore};
use crate::services::{bm2d_api, hud_layout_hooks};
use crate::{log_info, log_warn};

use super::policy;
use super::score_math as sm;

const PARAM_VISIBLE: i32 = 0x1007;
const PARAM_DIRTY: i32 = 0x101E;
const OP_GOTO_LABEL: i32 = 0xF09;
const TRAVERSE_SIBLINGS: i32 = 6;
const ATTR_VISIBLE: u32 = 1;
const CLIP_LAYER_OFF: usize = 0x08;
const RECORD_SKIN_OFF: usize = 0x28;
/// The dancer-name placeholder in a theme's difficulty frame.
const NAME_PLACEHOLDER: &str = "name_usr";
/// World's difficulty message (A3 `0x1052`).
const MSG_DIFFICULTY: i32 = 0x104F;
/// Near-buffer layout: fixed names, then the per-call difficulty export.
const BUF_SCORE: usize = 0x00;
const BUF_NAME: usize = 0x20;
const BUF_DIFFICULTY: usize = 0x40;
const BUF_DIFFICULTY_LEN: usize = 0x40;

type ActorFn = unsafe extern "C" fn(*mut u8);
type MsgFn = unsafe extern "C" fn(*mut u8, i32, *mut u8) -> u64;
type RecordFn = unsafe extern "C" fn(*mut u8, *const c_char) -> *const u8;

static mut INIT_HOOK: Option<GenericDetour<ActorFn>> = None;
static mut DIGITS_HOOK: Option<GenericDetour<ActorFn>> = None;
static mut MSG_HOOK: Option<GenericDetour<MsgFn>> = None;

struct State {
    s: DdrSelScoreSites,
    buf: usize,
    /// Stock disp32s of the three name LEAs (score, difficulty, name).
    stock_disp: [[u8; 4]; 3],
    /// `reverse byte = *(side_holder + reverse_delta)`.
    reverse_delta: Option<usize>,
}
unsafe impl Send for State {}
unsafe impl Sync for State {}

static STATE: OnceLock<State> = OnceLock::new();
static CAPABLE: AtomicBool = AtomicBool::new(false);
static BROKEN: AtomicBool = AtomicBool::new(false);
static PATCH_LOCK: Mutex<()> = Mutex::new(());
static LOGGED_CREATE: AtomicBool = AtomicBool::new(false);
static LOGGED_DIFFICULTY: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
struct Legacy {
    actor: usize,
    skin: u8,
    side: i32,
    /// Patches live for this init (restore post-original).
    patched: Option<bool>,
}

static TABLE: Mutex<Vec<Legacy>> = Mutex::new(Vec::new());

fn table() -> std::sync::MutexGuard<'static, Vec<Legacy>> {
    TABLE.lock().unwrap_or_else(|e| e.into_inner())
}

fn lookup(actor: *mut u8) -> Option<Legacy> {
    table().iter().find(|e| e.actor == actor as usize).copied()
}

fn rel32(from_next: usize, to: usize) -> Option<[u8; 4]> {
    i32::try_from(to as i64 - from_next as i64)
        .ok()
        .map(i32::to_le_bytes)
}

fn write_cstr(at: usize, s: &str, cap: usize) -> bool {
    if s.len() + 1 > cap {
        return false;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), at as *mut u8, s.len());
        *((at + s.len()) as *mut u8) = 0;
    }
    true
}

/// Resolve the sites and build the near buffer (mod init).
pub fn init(signatures: &SignatureStore) -> bool {
    let Some(s) = signatures.ddr_sel_score_sites() else {
        log_warn!("DDR SELECTION: legacy-score sites unresolved -- the score stays World's");
        return false;
    };
    let creates = [s.create_score, s.create_difficulty, s.create_name];
    if creates.iter().any(|c| unsafe { *c.add(2) } != sm::PRIORITY) {
        log_warn!(
            "DDR SELECTION: ScoreActor create priorities are not stock -- the score stays World's"
        );
        return false;
    }
    let buf = unsafe { memory::alloc_near(s.create_score, 0x1000) } as usize;
    if buf == 0 {
        log_warn!(
            "DDR SELECTION: no near buffer for the score export names -- the score stays World's"
        );
        return false;
    }
    if !write_cstr(buf + BUF_SCORE, sm::SCORE_EXPORT, BUF_NAME - BUF_SCORE)
        || !write_cstr(buf + BUF_NAME, sm::NAME_STAND_IN, BUF_DIFFICULTY - BUF_NAME)
    {
        return false;
    }
    let mut stock_disp = [[0u8; 4]; 3];
    for (i, c) in creates.iter().enumerate() {
        let at = *c as usize;
        if rel32(at + 13, buf).is_none() {
            log_warn!(
                "DDR SELECTION: score export names out of rel32 reach -- the score stays World's"
            );
            return false;
        }
        unsafe {
            std::ptr::copy_nonoverlapping((at + 9) as *const u8, stock_disp[i].as_mut_ptr(), 4)
        };
    }
    let reverse_delta = match (hud_layout_hooks::side_sites(), super::records_side_off()) {
        (Some(side), Some(rec)) if side.reverse_off >= rec && side.reverse_off - rec < 0x48 => {
            Some(side.reverse_off - rec)
        }
        _ => {
            log_warn!("DDR SELECTION: layout reverse flag unresolved -- legacy difficulty frames use the normal-scroll export");
            None
        }
    };
    let _ = STATE.set(State {
        s,
        buf,
        stock_disp,
        reverse_delta,
    });
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
    if !bm2d_api::is_available() || !bm2d_api::afp_layers_available() {
        log_warn!("DDR SELECTION: MovieClip / layer API unavailable -- the score stays World's");
        return false;
    }
    unsafe {
        let actor_hooks: [(
            *mut Option<GenericDetour<ActorFn>>,
            *const u8,
            ActorFn,
            &str,
        ); 2] = [
            (
                std::ptr::addr_of_mut!(INIT_HOOK),
                st.s.init,
                init_hook,
                "ScoreActor init",
            ),
            (
                std::ptr::addr_of_mut!(DIGITS_HOOK),
                st.s.digits,
                digits_hook,
                "ScoreActor digits",
            ),
        ];
        for (storage, target, cb, what) in actor_hooks {
            if (*storage).is_some() {
                continue;
            }
            let t: ActorFn = std::mem::transmute(target);
            if let Err(e) = hooks::install_enabled(storage, t, cb) {
                log_warn!(
                    "DDR SELECTION: {} detour failed ({:?}) -- the score stays World's",
                    what,
                    e
                );
                return false;
            }
        }
        if (*addr_of!(MSG_HOOK)).is_none() {
            let t: MsgFn = std::mem::transmute(st.s.msg);
            if let Err(e) = hooks::install_enabled(std::ptr::addr_of_mut!(MSG_HOOK), t, msg_hook) {
                log_warn!(
                    "DDR SELECTION: ScoreActor msg detour failed ({:?}) -- the score stays World's",
                    e
                );
                return false;
            }
        }
    }
    CAPABLE.store(true, Ordering::Release);
    log_info!("DDR SELECTION: legacy score ready (A3 exports, digits, difficulty)");
    true
}

/// Whether `dance_score` can turn legacy on this boot (the `Score` adapter).
pub fn capable() -> bool {
    CAPABLE.load(Ordering::Acquire) && !BROKEN.load(Ordering::Acquire)
}

/// New song window: one INFO per song again.
pub fn reset_logs() {
    LOGGED_CREATE.store(false, Ordering::Relaxed);
    LOGGED_DIFFICULTY.store(false, Ordering::Relaxed);
}

// ── Patches ─────────────────────────────────────────────────────────

/// `(address, stock, legacy)` for the init patches of `skin`.
fn patch_sites(st: &State, skin: u8) -> Option<Vec<(usize, Vec<u8>, Vec<u8>)>> {
    let mut v = Vec::new();
    let names = [
        (st.s.create_score as usize, BUF_SCORE),
        (st.s.create_difficulty as usize, BUF_DIFFICULTY),
        (st.s.create_name as usize, BUF_NAME),
    ];
    for (i, (at, off)) in names.iter().enumerate() {
        v.push((
            at + 9,
            st.stock_disp[i].to_vec(),
            rel32(at + 13, st.buf + off)?.to_vec(),
        ));
    }
    let prio = sm::difficulty_priority(skin);
    if prio != sm::PRIORITY {
        v.push((
            st.s.create_difficulty as usize + 2,
            vec![sm::PRIORITY],
            vec![prio],
        ));
    }
    Some(v)
}

fn apply(sites: &[(usize, Vec<u8>, Vec<u8>)], to_legacy: bool) -> bool {
    let _g = PATCH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut done: Vec<(usize, &[u8], &[u8])> = Vec::new();
    for (at, stock, legacy) in sites {
        let (old, new) = if to_legacy {
            (stock.as_slice(), legacy.as_slice())
        } else {
            (legacy.as_slice(), stock.as_slice())
        };
        if let Err(e) = unsafe { memory::apply_checked_patch(*at as *mut u8, old, new) } {
            log_warn!(
                "DDR SELECTION: score init patch failed ({:?}) -- the score stays World's",
                e
            );
            for (a, o, n) in done {
                let _ = unsafe { memory::apply_checked_patch(a as *mut u8, n, o) };
            }
            BROKEN.store(true, Ordering::Release);
            return false;
        }
        done.push((*at, old, new));
    }
    true
}

// ── Detours ─────────────────────────────────────────────────────────

/// Init PRE: a legacy record ⇒ patch World's init to A3's exports. Returns
/// `false` when World's init must NOT run (a legacy package whose patches
/// failed: World's init would NULL-deref on its missing exports).
unsafe fn init_pre(actor: *mut u8, st: &State) -> bool {
    table().retain(|e| e.actor != actor as usize);
    if !CAPABLE.load(Ordering::Acquire) || super::armed_skin() == 0 || actor.is_null() {
        return true;
    }
    if !memory::is_readable(actor, st.s.ex_off + 1) {
        return true;
    }
    let holder = memory::read_ptr(actor.add(st.s.side_off)) as *mut u8;
    if !memory::is_readable(holder, 0x48) {
        return true;
    }
    let record_fn: RecordFn = std::mem::transmute(st.s.record_fn);
    let rec = record_fn(holder, c"dance_score".as_ptr());
    if !memory::is_readable(rec, RECORD_SKIN_OFF + 4) {
        return true;
    }
    let skin = memory::read_i32(rec.add(RECORD_SKIN_OFF));
    if !(1..=policy::SKIN_MAX as i32).contains(&skin) {
        return true;
    }
    let skin = skin as u8;
    let side = memory::read_i32(holder);
    let reverse = st
        .reverse_delta
        .is_some_and(|d| memory::read_u8(holder.add(d)) != 0);
    let export = sm::difficulty_export(side, reverse);
    let ok = !BROKEN.load(Ordering::Acquire)
        && write_cstr(st.buf + BUF_DIFFICULTY, &export, BUF_DIFFICULTY_LEN)
        && patch_sites(st, skin).is_some_and(|p| apply(&p, true));
    table().push(Legacy {
        actor: actor as usize,
        skin,
        side,
        patched: Some(ok),
    });
    if !ok {
        log_warn!(
            "DDR SELECTION: legacy score package but the init patches failed -- no score on {}P this song",
            side + 1
        );
        return false;
    }
    if !LOGGED_CREATE.swap(true, Ordering::Relaxed) {
        log_info!(
            "DDR SELECTION: legacy score created (skin {}, {}P, exports {} / {}, difficulty priority {})",
            skin,
            side + 1,
            sm::SCORE_EXPORT,
            export,
            sm::difficulty_priority(skin)
        );
    }
    true
}

unsafe fn clip_layer(actor: *mut u8, off: usize) -> Option<u32> {
    let clip = memory::read_ptr(actor.add(off));
    if !memory::is_readable(clip, CLIP_LAYER_OFF + 4) {
        return None;
    }
    let layer = memory::read_u32(clip.add(CLIP_LAYER_OFF));
    (layer != 0 && bm2d_api::layer_id_is_valid(layer)).then_some(layer)
}

/// Every clip of a child's sibling chain.
fn for_siblings(layer: u32, path: &str, mut f: impl FnMut(u32)) {
    let mut mc = bm2d_api::layer_find_child(layer, path);
    let mut guard = 0;
    while let Some(id) = mc {
        f(id);
        mc = bm2d_api::mc_traversal(id, TRAVERSE_SIBLINGS);
        guard += 1;
        if guard > 64 {
            break;
        }
    }
}

/// Init POST: World's bytes back, then A3's EX indicator, no World name clip,
/// and the difficulty frame's `name_usr` placeholder hidden (only the themes'
/// frames have one — A3 hid it once it had bound the dancer name there) and
/// handed to `score_name` (a theme: A3's dancer name).
unsafe fn init_post(actor: *mut u8, st: &State) {
    let Some(e) = lookup(actor) else {
        return;
    };
    if e.patched == Some(true) {
        if let Some(p) = patch_sites(st, e.skin) {
            if !apply(&p, false) {
                log_warn!("DDR SELECTION: could not restore World's score init");
            }
        }
    }
    if let Some(name) = clip_layer(actor, st.s.name_clip_off) {
        bm2d_api::layer_play_raw(name, 0.0);
        bm2d_api::layer_set_attribute_raw(name, ATTR_VISIBLE, 0);
    }
    if let Some(difficulty) = clip_layer(actor, st.s.difficulty_clip_off) {
        let mut first = None;
        for_siblings(difficulty, NAME_PLACEHOLDER, |id| {
            first.get_or_insert(id);
            bm2d_api::mc_set_param(id, PARAM_VISIBLE, 0);
            bm2d_api::mc_set_param(id, PARAM_DIRTY, 1);
        });
        // A3 drew the dancer name there instead (`score_name.rs`).
        if let (Some(placeholder), true, Ok(side)) = (
            first,
            policy::is_theme(e.skin) && e.patched == Some(true),
            usize::try_from(e.side),
        ) {
            super::score_name::bind(side, difficulty, placeholder);
        }
    }
    if let Some(score) = clip_layer(actor, st.s.score_clip_off) {
        let ex = *actor.add(st.s.ex_off) != 0;
        for_siblings(score, "ex_tex", |id| {
            bm2d_api::mc_set_param(id, PARAM_VISIBLE, ex as i32);
            bm2d_api::mc_set_param(id, PARAM_DIRTY, 1);
        });
    }
}

unsafe extern "C" fn init_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(INIT_HOOK)).as_ref() else {
        return;
    };
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        STATE.get().is_none_or(|st| init_pre(actor, st))
    }))
    .unwrap_or(true);
    if run {
        hook.call(actor);
    }
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        if let Some(st) = STATE.get() {
            init_post(actor, st);
        }
    }));
}

/// A3's digit refresh for one legacy actor. `false` ⇒ run World's.
unsafe fn legacy_digits(actor: *mut u8, st: &State) -> bool {
    let Some(e) = lookup(actor) else {
        return false;
    };
    if e.patched != Some(true) {
        // No legacy clips (World's init was skipped): nothing to draw, and
        // World's refresh must not run on them either.
        return true;
    }
    let Some(layer) = clip_layer(actor, st.s.score_clip_off) else {
        return memory::read_ptr(actor.add(st.s.score_clip_off)).is_null();
    };
    let old = memory::read_i32(actor.add(st.s.displayed_off));
    let target = memory::read_i32(actor.add(st.s.target_off));
    let new = sm::smooth(old, target);
    memory::write_i32(actor.add(st.s.displayed_off), new);
    let ex = *actor.add(st.s.ex_off) != 0;
    for w in sm::digit_writes(e.skin, old, new, ex) {
        for_siblings(layer, &w.path, |id| {
            bm2d_api::mc_load_bitmap(id, &w.texture);
        });
        for_siblings(layer, &w.path, |id| {
            bm2d_api::mc_set_param(id, PARAM_VISIBLE, w.visible as i32);
            bm2d_api::mc_set_param(id, PARAM_DIRTY, 1);
        });
    }
    true
}

unsafe extern "C" fn digits_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(DIGITS_HOOK)).as_ref() else {
        return;
    };
    let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        STATE.get().is_some_and(|st| legacy_digits(actor, st))
    }))
    .unwrap_or(false);
    if !handled {
        hook.call(actor);
    }
}

/// A3's difficulty message for one legacy actor.
unsafe fn legacy_difficulty(actor: *mut u8, st: &State, e: Legacy) {
    let Some(layer) = clip_layer(actor, st.s.difficulty_clip_off) else {
        return;
    };
    let difficulty = memory::read_i32(actor.add(st.s.difficulty_off));
    let level = memory::read_i32(actor.add(st.s.level_off));
    let w = sm::difficulty_writes(e.skin, e.side, difficulty, level);
    let mut done = Vec::new();
    if let Some(mc) = bm2d_api::layer_find_child(layer, "difficulty_level_usr") {
        if let Ok(label) = CString::new(w.level_label.as_str()) {
            bm2d_api::mc_op_str(mc, OP_GOTO_LABEL, &label);
            done.push(w.level_label.clone());
        }
        if let Some(tex) = &w.level_texture {
            for_siblings(layer, "difficulty_level_usr/level_tex", |id| {
                bm2d_api::mc_load_bitmap(id, tex);
            });
            done.push(tex.clone());
        }
    }
    if let (Some(base), Some(mc)) = (
        &w.base_label,
        bm2d_api::layer_find_child(layer, "difficulty_level_base_usr"),
    ) {
        if let Ok(label) = CString::new(base.as_str()) {
            bm2d_api::mc_op_str(mc, OP_GOTO_LABEL, &label);
            done.push(base.clone());
        }
    }
    if !LOGGED_DIFFICULTY.swap(true, Ordering::Relaxed) {
        log_info!(
            "DDR SELECTION: legacy difficulty (skin {}, {}P, difficulty {} level {}) -> {:?}",
            e.skin,
            e.side + 1,
            difficulty,
            level,
            done
        );
    }
}

unsafe extern "C" fn msg_hook(actor: *mut u8, msg: i32, payload: *mut u8) -> u64 {
    let Some(hook) = (*addr_of!(MSG_HOOK)).as_ref() else {
        return 0;
    };
    if msg == MSG_DIFFICULTY {
        let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            let (Some(st), Some(e)) = (STATE.get(), lookup(actor)) else {
                return false;
            };
            if e.patched == Some(true) {
                legacy_difficulty(actor, st, e);
            }
            true
        }))
        .unwrap_or(false);
        if handled {
            return 0;
        }
    }
    hook.call(actor, msg, payload)
}
