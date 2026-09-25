//! Legacy combo — A3's `ComboActor` re-hosted in World's.
//!
//! World's `ComboActor` builds three label-driven layers
//! (`dance_combo_root1..3`) and repaints them from code; A3's drew ONE clip
//! (`dance_combo` of `dance_combo000N`) and laid it out from code: the digits
//! grow with the count, the clip is re-centred on the `combo` marker for the
//! digits shown, the centre moves the FAST/SLOW indicator, skin 1 lays its
//! digits out at half the cell width, skins 4–5 swap to the worst grade's
//! sheet, and the clip shows from combo 4 and replays on every step. This
//! module re-hosts that inside World's object for an actor whose
//! `dance_combo` record is legacy (World's object and vtable stay — `song_reset`
//! and the finalize's record write-back keep working):
//!
//! * **init** (`services::combo_hooks` PRE / POST): for the one call, three
//!   checked patches make World's own init create exactly A3's clip — the
//!   root loop runs once (count `2` → `0`), starts at root1 (`+0x80` →
//!   `+0x70`, so the layer priority is A3's `1` / `10`) and names export
//!   `dance_combo` (the `"dance_combo_root%d"` format LEA → a near-allocated
//!   NUL-padded `"dance_combo"`). World's create, record / package lookup,
//!   layer, marker and position code then run unchanged; roots 2/3 stay
//!   null. Post-original the patches are restored and A3's init finishes
//!   (alpha back to 1, hidden by attribute, cell size, layout).
//! * **msg** (OVERRIDE): the combo message `0x1033` keeps World's counters
//!   (`+0x68` combo, `+0x6C` worst grade — the finalize saves the worst
//!   grade) and runs A3's show / hide / replay / texture writes instead of
//!   World's three-root case (which would NULL-deref root2); the pre-start
//!   broadcast `0x1043` re-lays the clip out (A3's `0x1046`) before World's
//!   own handler; everything else is World's.
//! * **update** (OVERRIDE): A3's per-frame `number_usr` scale (growth ×
//!   `combo_usr`'s animated scale) and its game-over stop.
//! * **refresh** (OVERRIDE): World's three-root repaint never runs for a
//!   legacy actor (so neither does S-Marvelous' repaint on top of it).
//!
//! A3 sent the combo centre to its siblings (`0x1038`); World's
//! NoteResultActor still handles it as `0x1035` (FAST/SLOW x) but World's
//! combo never sends it — sent here to the same side's NoteResultActor only.
//!
//! Skin 5: World ships `dance_combo0005_v0.arc` blanked (the member
//! decompresses to zeros); [`package_usable`] refuses a damaged package
//! (stock combo + one WARN naming the A3 import script) — the package helper
//! consults it before registering.
//!
//! Game thread only. RE: `.agents/planning/2026-09-22-ddr-selection/research/legacy-combo.md`.

use std::ffi::c_char;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::core::memory;
use crate::core::signatures::{DdrSelComboSites, SignatureStore};
use crate::services::{bm2d_api, combo_hooks};
use crate::{log_info, log_warn};

use super::combo_math as cm;

const PARAM_VISIBLE: i32 = 0x1007;
const PARAM_DIRTY: i32 = 0x101E;
/// `afp_mc_get_param` scale (x, y).
const PARAM_GET_SCALE: i32 = 0x100D;
const PARAM_WIDTH: i32 = 0x1015;
const PARAM_HEIGHT: i32 = 0x1016;
const OP_GOTO_AND_PLAY_FRAME: i32 = 0xF08;
const OP_GOTO_LABEL: i32 = 0xF09;
const TRAVERSE_SIBLINGS: i32 = 6;
/// Layer attribute bit 1 = visible (`afp_layer_set_attribute(id, 1, v)`).
const ATTR_VISIBLE: u32 = 1;

/// World's combo message `{side, combo, max, grade, …}` (A3 `0x1036`).
const MSG_COMBO: i32 = 0x1033;
/// NoteResultActor: the combo centre `{x}` (A3 `0x1038`).
const MSG_COMBO_CENTER: i32 = 0x1035;
/// The DPS pre-start broadcast (A3 `0x1046` — A3's combo re-laid out there).
const MSG_PRE_START: i32 = 0x1043;

/// agcs actor tree: parent, first child, next sibling.
const PARENT_OFF: usize = 0x08;
const FIRST_CHILD_OFF: usize = 0x18;
const NEXT_SIBLING_OFF: usize = 0x10;
/// `CMovieClip`: the layer id.
const CLIP_LAYER_OFF: usize = 0x08;
/// LayoutActor record value: the skin.
const RECORD_SKIN_OFF: usize = 0x28;

type RecordFn = unsafe extern "C" fn(*mut u8, *const c_char) -> *const u8;
type MarkerFn = unsafe extern "C" fn(*mut u8, *const c_char) -> *const i32;
type SetPositionFn = unsafe extern "C" fn(*mut u8, i32, i32);
type SetColorFn = unsafe extern "C" fn(*mut u8, f32, f32, f32, f32);
type BroadcastFn = unsafe extern "C" fn(*mut u8, i32, *mut u8, i32);

struct State {
    s: DdrSelComboSites,
    /// Near buffer holding the NUL-padded export name.
    fmt_buf: usize,
    stock_fmt: [u8; 4],
    broadcast: usize,
    nra_vtable: usize,
}
unsafe impl Send for State {}
unsafe impl Sync for State {}

static STATE: OnceLock<State> = OnceLock::new();
static CAPABLE: AtomicBool = AtomicBool::new(false);
static BROKEN: AtomicBool = AtomicBool::new(false);
static PATCH_LOCK: Mutex<()> = Mutex::new(());
static LOGGED_CREATE: AtomicBool = AtomicBool::new(false);
static LOGGED_SHOW: AtomicBool = AtomicBool::new(false);
static WARNED_CLIP: AtomicBool = AtomicBool::new(false);
/// Per skin 1..=5: 0 unchecked, 1 usable, 2 damaged.
static PACKAGE_STATE: [AtomicU8; 6] = [const { AtomicU8::new(0) }; 6];

/// One legacy actor.
#[derive(Clone, Copy)]
struct Legacy {
    actor: usize,
    skin: u8,
    side: i32,
    /// Patches live for this actor's init (restore post-original).
    patched: bool,
    /// The clip was created and has A3's children.
    ok: bool,
    clip: usize,
    layer: u32,
    root_mc: i32,
    cell_w: i32,
    marker: (i32, i32),
    /// A3's `+0x88` — the growth the per-frame `number_usr` scale uses.
    growth: f32,
}

static TABLE: Mutex<Vec<Legacy>> = Mutex::new(Vec::new());

fn table() -> std::sync::MutexGuard<'static, Vec<Legacy>> {
    TABLE.lock().unwrap_or_else(|e| e.into_inner())
}

fn lookup(actor: *mut u8) -> Option<Legacy> {
    table().iter().find(|e| e.actor == actor as usize).copied()
}

fn store(e: Legacy) {
    let mut t = table();
    if let Some(x) = t.iter_mut().find(|x| x.actor == e.actor) {
        *x = e;
    } else {
        t.push(e);
    }
}

fn rel32(from_next: usize, to: usize) -> Option<[u8; 4]> {
    i32::try_from(to as i64 - from_next as i64)
        .ok()
        .map(i32::to_le_bytes)
}

/// Resolve the sites, read the stock bytes, build the near buffer (mod init).
pub fn init(signatures: &SignatureStore) -> bool {
    let Some(s) = signatures.ddr_sel_combo_sites() else {
        log_warn!("DDR SELECTION: legacy-combo sites unresolved -- the combo stays World's");
        return false;
    };
    let head = s.loop_head as usize;
    let stock_count = unsafe { *((head + 2) as *const u8) };
    let stock_disp = unsafe { memory::read_u32((head + 13) as *const u8) } as usize;
    if stock_count != 2 || stock_disp != s.root3_off {
        log_warn!(
            "DDR SELECTION: combo init loop head is not stock (count {}, first root 0x{:X}) -- the combo stays World's",
            stock_count,
            stock_disp
        );
        return false;
    }
    let buf = unsafe { memory::alloc_near(s.root_fmt_lea, 0x1000) } as usize;
    if buf == 0 {
        log_warn!(
            "DDR SELECTION: no near buffer for the combo export name -- the combo stays World's"
        );
        return false;
    }
    let fmt = cm::root_fmt_bytes();
    unsafe { std::ptr::copy_nonoverlapping(fmt.as_ptr(), buf as *mut u8, fmt.len()) };
    let lea = s.root_fmt_lea as usize;
    if rel32(lea + 7, buf).is_none() {
        log_warn!("DDR SELECTION: combo export name out of rel32 reach -- the combo stays World's");
        return false;
    }
    let mut stock_fmt = [0u8; 4];
    unsafe { std::ptr::copy_nonoverlapping((lea + 3) as *const u8, stock_fmt.as_mut_ptr(), 4) };
    let broadcast = signatures
        .get_address("update_broadcast")
        .map(|p| p as usize)
        .unwrap_or(0);
    let nra_vtable = signatures
        .get_address("note_result_actor_vtable")
        .map(|p| p as usize)
        .unwrap_or(0);
    if broadcast == 0 || nra_vtable == 0 {
        log_warn!(
            "DDR SELECTION: update_broadcast / NoteResultActor vtable unresolved -- the legacy combo will not move FAST/SLOW"
        );
    }
    let _ = STATE.set(State {
        s,
        fmt_buf: buf,
        stock_fmt,
        broadcast,
        nra_vtable,
    });
    true
}

/// Subscribe to the shared combo hooks and install them (mod enable).
pub fn start() -> bool {
    if CAPABLE.load(Ordering::Acquire) {
        return true;
    }
    if STATE.get().is_none() {
        return false;
    }
    if !bm2d_api::is_available()
        || !bm2d_api::afp_layers_available()
        || !bm2d_api::layer_info_available()
    {
        log_warn!(
            "DDR SELECTION: MovieClip / layer-info API unavailable -- the combo stays World's"
        );
        return false;
    }
    combo_hooks::subscribe_refresh_override(refresh_override);
    combo_hooks::subscribe_init_pre(init_pre);
    combo_hooks::subscribe_init_post(init_post);
    combo_hooks::subscribe_msg_override(msg_override);
    combo_hooks::subscribe_update_override(update_override);
    combo_hooks::subscribe_finalize_post(finalize_post);
    // The refresh must be covered too: World's init calls it (combo > 0) and
    // it would NULL-deref the absent root2 of a legacy actor.
    if !combo_hooks::acquire_refresh() || !combo_hooks::acquire_actor() {
        log_warn!("DDR SELECTION: combo hooks unavailable -- the combo stays World's");
        return false;
    }
    CAPABLE.store(true, Ordering::Release);
    log_info!("DDR SELECTION: legacy combo ready (A3 ComboActor re-hosted in World's)");
    true
}

/// Whether `dance_combo` can turn legacy on this boot (the `Combo` adapter).
pub fn capable() -> bool {
    CAPABLE.load(Ordering::Acquire) && !BROKEN.load(Ordering::Acquire)
}

/// New song window: one INFO per song again.
pub fn reset_logs() {
    LOGGED_CREATE.store(false, Ordering::Relaxed);
    LOGGED_SHOW.store(false, Ordering::Relaxed);
}

// ── Patches ─────────────────────────────────────────────────────────

/// `(address, stock bytes, legacy bytes)` of the three init patches.
fn patch_sites(st: &State) -> Option<[(usize, Vec<u8>, Vec<u8>); 3]> {
    let head = st.s.loop_head as usize;
    let lea = st.s.root_fmt_lea as usize;
    Some([
        (head + 2, vec![2], vec![0]),
        (
            head + 13,
            (st.s.root3_off as u32).to_le_bytes().to_vec(),
            (st.s.root1_off as u32).to_le_bytes().to_vec(),
        ),
        (
            lea + 3,
            st.stock_fmt.to_vec(),
            rel32(lea + 7, st.fmt_buf)?.to_vec(),
        ),
    ])
}

/// Switch World's init between its three roots and A3's one clip.
fn switch(to_legacy: bool) -> bool {
    let Some(st) = STATE.get() else {
        return false;
    };
    let Some(sites) = patch_sites(st) else {
        return false;
    };
    let _g = PATCH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut done: Vec<(usize, &[u8], &[u8])> = Vec::new();
    for (at, stock, legacy) in sites.iter() {
        let (old, new) = if to_legacy {
            (stock.as_slice(), legacy.as_slice())
        } else {
            (legacy.as_slice(), stock.as_slice())
        };
        if let Err(e) = unsafe { memory::apply_checked_patch(*at as *mut u8, old, new) } {
            log_warn!(
                "DDR SELECTION: combo init patch failed ({:?}) -- the combo stays World's",
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

// ── Package check (skin 5) ──────────────────────────────────────────

/// Whether `dance_combo000N` resolves to a real package. World's copy of
/// `dance_combo0005_v0.arc` is blanked (its member decompresses to zeros);
/// the A3 import puts a good copy in `data_mods/ddr_selection_a3/`. The first
/// arc the game's probe would open (LayeredFS mod file first, then `data/`)
/// is read and its member's IFS magic checked once per skin per boot. No arc
/// at all ⇒ `true` (the game's own probe then refuses).
pub fn package_usable(skin: u8) -> bool {
    let i = skin as usize;
    if !(1..=5).contains(&i) {
        return false;
    }
    match PACKAGE_STATE[i].load(Ordering::Acquire) {
        1 => return true,
        2 => return false,
        _ => {}
    }
    let (ok, path) = check_package(skin);
    PACKAGE_STATE[i].store(if ok { 1 } else { 2 }, Ordering::Release);
    if !ok {
        log_warn!(
            "DDR SELECTION: {} is damaged (World ships a blanked dance_combo0005) -- skin {} combo stays World's; run the A3 import (ddr_selection_import/import_a3_assets.bat or .sh with your A3 install)",
            path.as_deref().unwrap_or("dance_combo arc"),
            skin
        );
    }
    ok
}

fn check_package(skin: u8) -> (bool, Option<String>) {
    use crate::services::avs_layeredfs::mod_paths;
    let base = format!("dance_combo{:04}", skin);
    for name in cm::arc_candidates(&base) {
        let rel = format!("arc/bm2d/{}", name);
        let path = mod_paths::find_first_modfile(&rel).or_else(|| {
            let stock = format!("data/{}", rel);
            std::path::Path::new(&stock).is_file().then_some(stock)
        });
        let Some(path) = path else {
            continue;
        };
        let ok = std::fs::read(&path).ok().is_some_and(|data| {
            crate::core::arc::parse(&data)
                .and_then(|entries| {
                    let e = entries.first()?;
                    crate::core::arc::extract(&data, e)
                })
                .is_some_and(|member| cm::ifs_member_ok(&member))
        });
        return (ok, Some(path));
    }
    (true, None)
}

// ── Engine helpers ──────────────────────────────────────────────────

unsafe fn vcall_set_position(clip: usize, vslot: usize, x: i32, y: i32) {
    let vt = memory::read_ptr(clip as *const u8);
    let f = memory::read_ptr(vt.add(vslot));
    if memory::is_readable(f, 16) {
        let set: SetPositionFn = std::mem::transmute(f);
        set(clip as *mut u8, x, y);
    }
}

unsafe fn vcall_set_color(clip: usize, vslot: usize, a: f32) {
    let vt = memory::read_ptr(clip as *const u8);
    let f = memory::read_ptr(vt.add(vslot));
    if memory::is_readable(f, 16) {
        let set: SetColorFn = std::mem::transmute(f);
        set(clip as *mut u8, a, 1.0, 1.0, 1.0);
    }
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

fn set_shown(layer: u32, shown: bool) {
    bm2d_api::layer_play_raw(layer, if shown { 1.0 } else { 0.0 });
    bm2d_api::layer_set_attribute_raw(layer, ATTR_VISIBLE, shown as u32);
}

fn counters(actor: *mut u8, st: &State) -> (i32, i32) {
    unsafe {
        (
            memory::read_i32(actor.add(st.s.actor.combo_off)),
            memory::read_i32(actor.add(st.s.actor.worst_off)),
        )
    }
}

/// A3 `FUN_180047570`: while visible, `number_usr` scale = growth ×
/// `combo_usr`'s current (animated) scale.
fn apply_number_scale(e: &Legacy) {
    let Some(info) = bm2d_api::layer_get_info_raw(e.layer) else {
        return;
    };
    if !info.visible {
        return;
    }
    let Some(word) = bm2d_api::layer_find_child(e.layer, "combo_usr") else {
        return;
    };
    let Some((sx, sy)) = bm2d_api::mc_get_vec2(word, PARAM_GET_SCALE) else {
        return;
    };
    let g = e.growth;
    for_siblings(e.layer, "number_usr", |id| {
        bm2d_api::mc_set_scale(id, g * sx, g * sy);
    });
}

/// A3 `FUN_180047460`: position for `digits` at `growth`, then tell the
/// FAST/SLOW indicator the centre.
fn layout(e: &Legacy, digits: i32, growth: f32, st: &State) {
    let x = cm::layout_x(e.marker.0, e.cell_w, digits, growth);
    unsafe { vcall_set_position(e.clip, st.s.clip_set_position_vslot, x, e.marker.1) };
    let w = bm2d_api::layer_get_info_raw(e.layer)
        .map(|i| i.width)
        .unwrap_or(0);
    let mut center = cm::center_x(x, w);
    send_center(e.actor as *mut u8, &mut center, st);
}

/// The combo centre to this side's NoteResultActor (a sibling under the
/// same GamePlayActor), through the engine's own delivery.
fn send_center(actor: *mut u8, center: &mut i32, st: &State) {
    if st.broadcast == 0 || st.nra_vtable == 0 {
        return;
    }
    unsafe {
        let parent = memory::read_ptr(actor.add(PARENT_OFF)) as *mut u8;
        if !memory::is_readable(parent, FIRST_CHILD_OFF + 8) {
            return;
        }
        let broadcast: BroadcastFn = std::mem::transmute(st.broadcast);
        let mut child = memory::read_ptr(parent.add(FIRST_CHILD_OFF)) as *mut u8;
        let mut guard = 0;
        while !child.is_null() && guard < 256 {
            if !memory::is_readable(child, NEXT_SIBLING_OFF + 8) {
                return;
            }
            let next = memory::read_ptr(child.add(NEXT_SIBLING_OFF)) as *mut u8;
            if memory::read_ptr(child) as usize == st.nra_vtable {
                broadcast(child, MSG_COMBO_CENTER, center as *mut i32 as *mut u8, 0);
            }
            child = next;
            guard += 1;
        }
    }
}

/// A3 `FUN_1800470e0`: word + digit textures, growth, layout.
fn texture_write(actor: *mut u8, e: &mut Legacy, st: &State) {
    let (combo, worst) = counters(actor, st);
    let prefix = cm::sheet_prefix(e.skin, worst);
    let word = cm::word_texture(&prefix);
    for_siblings(e.layer, "combo_usr", |id| {
        bm2d_api::mc_load_bitmap(id, &word);
    });
    for_siblings(e.layer, "combo_usr", |id| {
        bm2d_api::mc_set_param(id, PARAM_VISIBLE, 1);
        bm2d_api::mc_set_param(id, PARAM_DIRTY, 1);
    });
    let n = cm::shown(combo);
    let g = cm::growth(n, e.skin);
    e.growth = g;
    apply_number_scale(e);
    layout(e, cm::digit_count(n), g, st);
    for p in cm::places(&prefix, n) {
        for_siblings(e.layer, p.path, |id| {
            bm2d_api::mc_load_bitmap(id, &p.texture);
        });
        for_siblings(e.layer, p.path, |id| {
            bm2d_api::mc_set_param(id, PARAM_VISIBLE, p.visible as i32);
            bm2d_api::mc_set_param(id, PARAM_DIRTY, 1);
        });
    }
    if !LOGGED_SHOW.swap(true, Ordering::Relaxed) {
        log_info!(
            "DDR SELECTION: legacy combo shown (skin {}, {}P, combo {}, sheet {}, growth {:.3})",
            e.skin,
            e.side + 1,
            combo,
            prefix,
            g
        );
    }
}

// ── combo_hooks subscribers ─────────────────────────────────────────

/// Init PRE: a legacy record ⇒ patch World's init to A3's one clip. `true`
/// (skip World's init) only when the patches could not be applied — a
/// legacy package has no `dance_combo_root%d` exports and World's init
/// would NULL-deref; the actor then keeps no clip (no combo this song).
fn init_pre(actor: *mut u8) -> bool {
    let Some(st) = STATE.get() else {
        return false;
    };
    // A new actor at a recycled address is never a stale legacy one.
    table().retain(|e| e.actor != actor as usize);
    if !CAPABLE.load(Ordering::Acquire) || super::armed_skin() == 0 || actor.is_null() {
        return false;
    }
    let (skin, side) = unsafe {
        if !memory::is_readable(actor, st.s.root3_off + 8) {
            return false;
        }
        let holder = memory::read_ptr(actor.add(st.s.side_off)) as *mut u8;
        if !memory::is_readable(holder, 4) {
            return false;
        }
        let record_fn: RecordFn = std::mem::transmute(st.s.record_fn);
        let rec = record_fn(holder, c"dance_combo".as_ptr());
        if !memory::is_readable(rec, RECORD_SKIN_OFF + 4) {
            return false;
        }
        (
            memory::read_i32(rec.add(RECORD_SKIN_OFF)),
            memory::read_i32(holder),
        )
    };
    if !(1..=5).contains(&skin) {
        return false;
    }
    let patched = !BROKEN.load(Ordering::Acquire) && switch(true);
    store(Legacy {
        actor: actor as usize,
        skin: skin as u8,
        side,
        patched,
        ok: false,
        clip: 0,
        layer: 0,
        root_mc: 0,
        cell_w: 0,
        marker: (0, 0),
        growth: cm::ONE,
    });
    if !patched {
        log_warn!(
            "DDR SELECTION: legacy combo package but the init patches failed -- no combo on {}P this song",
            side + 1
        );
    }
    !patched
}

/// Init POST: restore World's init and finish A3's.
fn init_post(actor: *mut u8) {
    let Some(st) = STATE.get() else {
        return;
    };
    let Some(mut e) = lookup(actor) else {
        return;
    };
    if e.patched {
        e.patched = false;
        if !switch(false) {
            log_warn!("DDR SELECTION: could not restore World's combo init");
        }
    }
    unsafe {
        let clip = memory::read_ptr(actor.add(st.s.root1_off)) as usize;
        let extra = (st.s.root1_off + 8..=st.s.root3_off)
            .step_by(8)
            .any(|o| !memory::read_ptr(actor.add(o)).is_null());
        let layer = if memory::is_readable(clip as *const u8, st.s.clip_root_mc_off + 4) {
            memory::read_u32((clip + CLIP_LAYER_OFF) as *const u8)
        } else {
            0
        };
        let children = layer != 0
            && bm2d_api::layer_id_is_valid(layer)
            && bm2d_api::layer_find_child(layer, "combo_usr").is_some()
            && bm2d_api::layer_find_child(layer, cm::PLACES[0]).is_some();
        if extra || !children {
            if layer != 0 && bm2d_api::layer_id_is_valid(layer) {
                set_shown(layer, false);
            }
            store(e);
            if !WARNED_CLIP.swap(true, Ordering::Relaxed) {
                log_warn!(
                    "DDR SELECTION: legacy combo clip missing or not A3's (clip {:#x}, layer {:#x}, extra roots {}) -- no combo on {}P",
                    clip,
                    layer,
                    extra,
                    e.side + 1
                );
            }
            return;
        }
        e.clip = clip;
        e.layer = layer;
        e.root_mc = memory::read_i32((clip + st.s.clip_root_mc_off) as *const u8);
        // World hid the root with alpha 0; A3 hides by attribute.
        vcall_set_color(clip, st.s.clip_set_color_vslot, 1.0);
        set_shown(layer, false);
        let holder = memory::read_ptr(actor.add(st.s.side_off)) as *mut u8;
        let marker_fn: MarkerFn = std::mem::transmute(st.s.marker_fn);
        let m = marker_fn(holder, c"combo".as_ptr());
        if memory::is_readable(m as *const u8, 8) {
            e.marker = (*m, *m.add(1));
        }
    }
    let (raw_w, h) = bm2d_api::layer_find_child(e.layer, cm::PLACES[0])
        .map(|mc| {
            (
                bm2d_api::mc_get_param(mc, PARAM_WIDTH).unwrap_or(0),
                bm2d_api::mc_get_param(mc, PARAM_HEIGHT).unwrap_or(0),
            )
        })
        .unwrap_or((0, 0));
    e.cell_w = cm::cell_width(raw_w, e.skin);
    e.ok = true;
    let (combo, _) = counters(actor, st);
    layout(&e, cm::digit_count(combo), cm::growth(combo, e.skin), st);
    if combo > 0 {
        set_shown(e.layer, true);
        if e.root_mc > 0 {
            bm2d_api::mc_op_str(e.root_mc as u32, OP_GOTO_LABEL, c"loop");
        }
        texture_write(actor, &mut e, st);
    }
    store(e);
    if !LOGGED_CREATE.swap(true, Ordering::Relaxed) {
        log_info!(
            "DDR SELECTION: legacy combo created (skin {}, {}P, cell {} x {} (0001 width {}), marker ({}, {}), layer {:#x})",
            e.skin,
            e.side + 1,
            e.cell_w,
            h,
            raw_w,
            e.marker.0,
            e.marker.1,
            e.layer
        );
    }
}

fn msg_override(actor: *mut u8, msg: i32, payload: *mut u8) -> Option<u64> {
    let st = STATE.get()?;
    let mut e = lookup(actor)?;
    match msg {
        MSG_COMBO => {
            unsafe {
                if !memory::is_readable(payload, 16) {
                    return Some(0);
                }
                let p = payload as *const i32;
                if *p != e.side {
                    return Some(0);
                }
                let combo = *p.add(1);
                let (_, worst) = counters(actor, st);
                memory::write_i32(actor.add(st.s.actor.combo_off), combo);
                memory::write_i32(
                    actor.add(st.s.actor.worst_off),
                    cm::update_worst(worst, combo, *p.add(3)),
                );
                if !e.ok {
                    return Some(0);
                }
                if !cm::visible(combo) {
                    layout(&e, cm::digit_count(combo), cm::growth(combo, e.skin), st);
                    set_shown(e.layer, false);
                    return Some(0);
                }
                set_shown(e.layer, true);
                if e.root_mc > 0 {
                    bm2d_api::mc_op(e.root_mc as u32, OP_GOTO_AND_PLAY_FRAME, 0);
                }
            }
            texture_write(actor, &mut e, st);
            store(e);
            Some(0)
        }
        MSG_PRE_START => {
            if e.ok {
                let (combo, _) = counters(actor, st);
                layout(&e, cm::digit_count(combo), cm::growth(combo, e.skin), st);
            }
            None
        }
        _ => None,
    }
}

/// A3 `FUN_180046e40`.
fn update_override(actor: *mut u8) -> bool {
    let Some(st) = STATE.get() else {
        return false;
    };
    let Some(e) = lookup(actor) else {
        return false;
    };
    if !e.ok {
        return true;
    }
    apply_number_scale(&e);
    let game_over = unsafe { *actor.add(st.s.actor.gameover_off) } != 0;
    if game_over && e.root_mc > 0 {
        let playing = bm2d_api::layer_get_info_raw(e.layer).is_some_and(|i| i.rate != 0.0);
        if playing {
            let root = e.root_mc as u32;
            let loop_frame = bm2d_api::mc_frame_by_label(root, c"loop").unwrap_or(0);
            let current = bm2d_api::mc_current_frame(root).unwrap_or(0);
            if loop_frame <= current {
                set_shown(e.layer, false);
            }
        }
    }
    true
}

fn refresh_override(actor: *mut u8) -> bool {
    let Some(st) = STATE.get() else {
        return false;
    };
    let Some(mut e) = lookup(actor) else {
        return false;
    };
    if e.ok {
        texture_write(actor, &mut e, st);
        store(e);
    }
    true
}

fn finalize_post(actor: *mut u8) {
    table().retain(|e| e.actor != actor as usize);
}

/// Legacy actors alive (diagnostics).
pub fn live() -> usize {
    table().len()
}
