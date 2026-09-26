//! A3's in-gameplay option icons on the legacy skins 2–5 and the themes
//! (A3's own skin-0 UI, where the icons came from; SuperNOVA had
//! them too; 1st-5th never shows any — World's own `GameWork+0xA8 == 1`
//! gate skips the actor, like A3).
//!
//! World's `sequence::dance::OptionIconActor` builds one AFP clip
//! (`dance_option_root` of `dance_option`, World's `daop_icon_*` art). A3's
//! drew a row of `BM2D::CSprite`s from the texture-only package
//! `dance_option_icon0000_v0` (World ships it byte-identical): the package
//! helper registers that package under World's `dance_option` record for a
//! legacy song (policy `fixed_arc`), the Step 7 marker post-pass writes A3's
//! `option` position (`option_icon_<n>p[_reverse]_usr`), and two detours on
//! World's actor (nobody else hooks it) run A3's behaviour for actors whose
//! `dance_option` record is legacy:
//!
//! * init (slot 4) — World's init skipped (it would NULL-deref on the
//!   texture-only package); the eleven sprites of [`logic::SLOTS`] created
//!   from the game's own CSprite pool exactly like A3 (texture, priority 8,
//!   group side + 2, centred, scaled to the marker width, hidden at the
//!   default value);
//! * update (slot 6) — World's skipped (it reads its clip); the speed and
//!   floating-flare icons follow the option like World's own update does.
//!   A3 re-textured the speed icon on its speed-change message; World's
//!   in-song speed change (`ControlSpeedActor`, msg `0x1042`) changes only
//!   that actor's own Option copy and the side's `GamePlayActor` speed
//!   cluster, so the speed is read from the GamePlayActor's int ×100 target
//!   (`gameplay_actor_layout().speed_int`) once it exists — the value the
//!   lanes scroll at, real speed included.
//!
//! Sprites are destroyed when gameplay is left (the scene callback runs
//! before the LayoutActor releases the package), when the side gets a new
//! actor, at disarm and disable. Game thread only; hook bodies under
//! `catch_unwind`. RE: `.agents/planning/2026-09-22-ddr-selection/research/
//! option-icons.md`.

use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::memory;
use crate::core::signatures::{DdrSelOptionIconSites, SignatureStore};
use crate::{log_info, log_warn};

use super::option_icons_logic::{self as logic, Opts, Slot, SLOTS};

type ActorFn = unsafe extern "C" fn(*mut u8);
type LookupFn = unsafe extern "C" fn(*mut u8, *const u8) -> *const u8;
type ResolverFn = unsafe extern "C" fn(*mut u8, u8) -> *const u8;
type CreateFn = unsafe extern "C" fn(*mut u8, *const u8, u32);

static mut INIT_HOOK: Option<GenericDetour<ActorFn>> = None;
static mut UPDATE_HOOK: Option<GenericDetour<ActorFn>> = None;

struct Sites(DdrSelOptionIconSites);
unsafe impl Send for Sites {}
unsafe impl Sync for Sites {}

static SITES: OnceLock<Sites> = OnceLock::new();
/// `GamePlayActor` offset of the int ×100 target speed (0 = unresolved).
static GPA_SPEED_INT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static CAPABLE: AtomicBool = AtomicBool::new(false);
static LOGGED: AtomicBool = AtomicBool::new(false);

// CSprite vtable slots (identical on every supported build).
const VT_DESTROY: usize = 0x18;
const VT_SET_VISIBLE: usize = 0x20;
const VT_SET_POSITION: usize = 0x38;
const VT_SET_ANCHOR: usize = 0x48;
const VT_SET_SCALE: usize = 0xC8;
const VT_SET_GROUP: usize = 0xE8;
const VT_GET_SIZE: usize = 0x120;
const VT_IS_VALID: usize = 0x138;
const ANCHOR_CENTRE: i32 = 3;
const RECORD_SKIN: usize = 0x28;

#[derive(Default)]
struct Side {
    /// The actor we took over (kept after the sprites are released, so its
    /// later updates never reach World's update).
    actor: usize,
    sprites: [usize; 11],
    values: [&'static str; 11],
    marker: (i32, i32, i32),
    /// The side's `GamePlayActor` (found lazily; same lifetime as the icon
    /// actor — both belong to the one DancePlaySequence).
    gpa: usize,
}

static SIDES: Mutex<[Option<Side>; 2]> = Mutex::new([None, None]);

/// Resolve (mod init).
pub fn init(signatures: &SignatureStore) -> bool {
    if let Some(l) = signatures.gameplay_actor_layout() {
        GPA_SPEED_INT.store(l.speed_int, Ordering::Release);
    } else {
        log_warn!("DDR SELECTION: GamePlayActor speed field unresolved -- the A3 speed icon will not follow in-song speed changes");
    }
    match signatures.ddr_sel_option_icon_sites() {
        Some(s) => {
            let _ = SITES.set(Sites(s));
            true
        }
        None => {
            log_warn!(
                "DDR SELECTION: option-icon sites unresolved -- the option icons stay World's"
            );
            false
        }
    }
}

/// Install the detours (enable). `false` ⇒ the icons stay World's.
pub fn start() -> bool {
    if CAPABLE.load(Ordering::Acquire) {
        return true;
    }
    let Some(s) = SITES.get() else {
        return false;
    };
    unsafe {
        let init: ActorFn = std::mem::transmute(s.0.init);
        let update: ActorFn = std::mem::transmute(s.0.update);
        if let Err(e) = hooks::install_enabled(std::ptr::addr_of_mut!(INIT_HOOK), init, init_hook) {
            log_warn!(
                "DDR SELECTION: option-icon init detour failed: {} -- World's icons",
                e
            );
            return false;
        }
        if let Err(e) =
            hooks::install_enabled(std::ptr::addr_of_mut!(UPDATE_HOOK), update, update_hook)
        {
            // The init detour must not take actors over without the update's.
            if let Some(h) = (*std::ptr::addr_of_mut!(INIT_HOOK)).take() {
                let _ = h.disable();
            }
            log_warn!(
                "DDR SELECTION: option-icon update detour failed: {} -- World's icons",
                e
            );
            return false;
        }
    }
    CAPABLE.store(true, Ordering::Release);
    log_info!("DDR SELECTION: A3 option icons ready");
    true
}

/// The `OptionIcons` adapter.
pub fn capable() -> bool {
    CAPABLE.load(Ordering::Acquire)
}

fn sides() -> std::sync::MutexGuard<'static, [Option<Side>; 2]> {
    SIDES.lock().unwrap_or_else(|e| e.into_inner())
}

unsafe fn vfn<T: Copy>(obj: *mut u8, slot: usize) -> T {
    let vt = memory::read_ptr(obj);
    std::mem::transmute_copy::<*const u8, T>(&memory::read_ptr(vt.add(slot)))
}

unsafe fn destroy(sprite: usize) {
    if sprite == 0 {
        return;
    }
    let f: unsafe extern "C" fn(*mut u8) = vfn(sprite as *mut u8, VT_DESTROY);
    f(sprite as *mut u8);
}

fn release_side(slot: &mut Option<Side>, keep_actor: bool) -> usize {
    let Some(s) = slot.as_mut() else {
        return 0;
    };
    let mut n = 0;
    for sp in s.sprites.iter_mut() {
        if *sp != 0 {
            unsafe { destroy(*sp) };
            *sp = 0;
            n += 1;
        }
    }
    if !keep_actor {
        *slot = None;
    }
    n
}

/// Destroy every sprite (leaving gameplay, disarm, disable).
pub fn release_all(reason: &str) {
    let mut g = sides();
    let n: usize = g.iter_mut().map(|s| release_side(s, true)).sum();
    if n > 0 {
        log_info!(
            "DDR SELECTION: A3 option icons released ({} sprites, {})",
            n,
            reason
        );
    }
}

/// Leaving GAMEPLAY (scene callback, before the LayoutActor goes).
pub fn on_scene_change(next: i32) {
    if next != crate::types::scenes::scene::GAMEPLAY {
        release_all("left gameplay");
    }
}

/// `(side, holder)` of an actor, or `None`.
unsafe fn side_of(st: &DdrSelOptionIconSites, actor: *mut u8) -> Option<(u8, *mut u8)> {
    if !memory::is_readable(actor, st.holder_off + 8) {
        return None;
    }
    let holder = memory::read_ptr(actor.add(st.holder_off)) as *mut u8;
    if holder.is_null() || !memory::is_readable(holder, 4) {
        return None;
    }
    let side = memory::read_i32(holder);
    (0..=1).contains(&side).then_some((side as u8, holder))
}

unsafe fn legacy_skin(st: &DdrSelOptionIconSites, holder: *mut u8) -> u8 {
    let record_fn: LookupFn = std::mem::transmute(st.record_fn);
    let rec = record_fn(holder, c"dance_option".as_ptr().cast());
    if rec.is_null() || !memory::is_readable(rec, RECORD_SKIN + 4) {
        return 0;
    }
    let skin = memory::read_i32(rec.add(RECORD_SKIN));
    if (2..=super::policy::SKIN_MAX as i32).contains(&skin) {
        skin as u8
    } else {
        0
    }
}

unsafe fn read_opts(st: &DdrSelOptionIconSites, side: u8) -> Option<Opts> {
    let entry = st.option_table.add(side as usize * 8);
    if !memory::is_readable(entry, 8) {
        return None;
    }
    let holder = memory::read_ptr(entry) as *mut u8;
    if holder.is_null() {
        return None;
    }
    let resolver: ResolverFn = std::mem::transmute(st.option_resolver);
    let opt = resolver(holder, 0);
    let f = &st.fields;
    if opt.is_null() || !memory::is_readable(opt, 0x80) || memory::read_ptr(opt) != st.option_vtable
    {
        return None;
    }
    let rd = |o: usize| memory::read_i32(opt.add(o));
    let speed_x100 = match rd(f.speed_type) {
        1 => rd(f.hispeed),
        _ => rd(f.speed_derived),
    };
    Some(Opts {
        speed_x100,
        gauge: rd(f.gauge),
        flare: rd(f.flare),
        scroll: rd(f.scroll),
        visibility: rd(f.visibility),
        lane_cover: rd(f.lane_cover),
        stepzone: rd(f.stepzone),
        boost: rd(f.boost),
        turn: rd(f.turn),
        color: rd(f.color),
        cut: rd(f.cut),
        freeze: rd(f.freeze),
        jump: rd(f.jump),
    })
}

/// One sprite from the game's pool, set up like A3's (`0` on failure).
unsafe fn make_sprite(
    st: &DdrSelOptionIconSites,
    side: u8,
    slot: Slot,
    index: usize,
    value: &str,
    shown: bool,
    marker: (i32, i32, i32),
) -> usize {
    let mut free = std::ptr::null_mut::<u8>();
    for i in 0..st.sprite_count {
        let p = st.sprite_pool.add(i * st.sprite_stride) as *mut u8;
        if memory::read_ptr(p) != st.sprite_vtable {
            return 0; // pool not constructed as expected
        }
        let valid: unsafe extern "C" fn(*mut u8) -> u8 = vfn(p, VT_IS_VALID);
        if valid(p) == 0 {
            free = p;
            break;
        }
    }
    if free.is_null() {
        return 0;
    }
    let tex = logic::texture(side, slot, value);
    let create: CreateFn = std::mem::transmute(st.sprite_create);
    create(free, tex.as_ptr(), logic::PRIORITY);
    let valid: unsafe extern "C" fn(*mut u8) -> u8 = vfn(free, VT_IS_VALID);
    if valid(free) == 0 {
        return 0;
    }
    let group: unsafe extern "C" fn(*mut u8, u32) = vfn(free, VT_SET_GROUP);
    group(free, side as u32 + 2);
    let anchor: unsafe extern "C" fn(*mut u8, i32, i32) = vfn(free, VT_SET_ANCHOR);
    anchor(free, ANCHOR_CENTRE, ANCHOR_CENTRE);
    let (x, y) = logic::position(marker, index);
    let pos: unsafe extern "C" fn(*mut u8, i32, i32) = vfn(free, VT_SET_POSITION);
    pos(free, x, y);
    let vis: unsafe extern "C" fn(*mut u8, u8) = vfn(free, VT_SET_VISIBLE);
    vis(free, u8::from(shown));
    let (mut w, mut h) = (0i32, 0i32);
    let size: unsafe extern "C" fn(*mut u8, *mut i32, *mut i32) = vfn(free, VT_GET_SIZE);
    size(free, &mut w, &mut h);
    if let Some(s) = logic::scale(marker.2, w) {
        let sc: unsafe extern "C" fn(*mut u8, f32) = vfn(free, VT_SET_SCALE);
        sc(free, s);
    }
    free as usize
}

/// A3's init for a legacy actor. `false` ⇒ run World's init.
unsafe fn legacy_init(st: &DdrSelOptionIconSites, actor: *mut u8) -> bool {
    let Some((side, holder)) = side_of(st, actor) else {
        return false;
    };
    let skin = legacy_skin(st, holder);
    let mut g = sides();
    let slot = &mut g[side as usize];
    // A new actor for this side: the old one's sprites go, and a stock
    // actor must not be mistaken for ours.
    release_side(slot, false);
    if skin == 0 {
        return false;
    }
    // From here on World's init must not run (texture-only package).
    let mut state = Side {
        actor: actor as usize,
        ..Side::default()
    };
    let marker_fn: LookupFn = std::mem::transmute(st.marker_fn);
    let m = marker_fn(holder, c"option".as_ptr().cast());
    let marker = if !m.is_null() && memory::is_readable(m, 12) {
        (
            memory::read_i32(m),
            memory::read_i32(m.add(4)),
            memory::read_i32(m.add(8)),
        )
    } else {
        (0, 0, 0)
    };
    state.marker = marker;
    let opts = read_opts(st, side);
    let (Some(opts), true) = (opts, marker.2 > 0) else {
        log_warn!(
            "DDR SELECTION: A3 option icons skipped ({}P, skin {}: option {}, marker {:?})",
            side + 1,
            skin,
            if opts.is_some() { "ok" } else { "unreadable" },
            marker
        );
        *slot = Some(state);
        return true;
    };
    let mut made = 0;
    for (i, s) in SLOTS.iter().enumerate() {
        let Some((value, shown)) = logic::icon(*s, &opts) else {
            continue;
        };
        state.values[i] = value;
        state.sprites[i] = make_sprite(st, side, *s, i, value, shown, marker);
        if state.sprites[i] != 0 {
            made += 1;
        }
    }
    if !LOGGED.swap(true, Ordering::Relaxed) || made == 0 {
        log_info!(
            "DDR SELECTION: A3 option icons ({}P, skin {}): {} sprites at {:?}, speed {}, gauge {:?}",
            side + 1,
            skin,
            made,
            marker,
            logic::speed_name(opts.speed_x100),
            logic::gauge_name(opts.gauge, opts.flare)
        );
    }
    *slot = Some(state);
    true
}

/// A3's per-frame follow-up for our actor. `false` ⇒ not ours.
unsafe fn legacy_update(st: &DdrSelOptionIconSites, actor: *mut u8) -> bool {
    let Some((side, _)) = side_of(st, actor) else {
        return false;
    };
    let mut g = sides();
    let Some(state) = g[side as usize].as_mut() else {
        return false;
    };
    if state.actor != actor as usize {
        return false;
    }
    if state.sprites.iter().all(|s| *s == 0) {
        return true;
    }
    let Some(mut opts) = read_opts(st, side) else {
        return true;
    };
    if let Some(v) = live_speed(state, side) {
        opts.speed_x100 = v;
    }
    for idx in [0usize, 10] {
        let slot = SLOTS[idx];
        let want = logic::icon(slot, &opts);
        let have = state.values[idx];
        match want {
            Some((v, shown)) if v != have => {
                destroy(state.sprites[idx]);
                state.sprites[idx] = make_sprite(st, side, slot, idx, v, shown, state.marker);
                state.values[idx] = v;
                log_info!(
                    "DDR SELECTION: A3 option icon ({}P) {} -> {}",
                    side + 1,
                    logic::kind(slot),
                    v
                );
            }
            None if state.sprites[idx] != 0 => {
                destroy(state.sprites[idx]);
                state.sprites[idx] = 0;
                state.values[idx] = "";
            }
            _ => {}
        }
    }
    true
}

/// The side's live scroll speed ×100 from its `GamePlayActor` (the in-song
/// speed change lands there, not in the player's Option). `None` before the
/// actor exists or when the field is unresolved.
unsafe fn live_speed(state: &mut Side, side: u8) -> Option<i32> {
    use crate::services::song_reset;
    let off = GPA_SPEED_INT.load(Ordering::Acquire);
    if off == 0 {
        return None;
    }
    if state.gpa == 0 {
        let dps = song_reset::live_dps()?;
        state.gpa = song_reset::gameplay_actors(dps)
            .into_iter()
            .find(|a| {
                memory::is_readable(*a, song_reset::GPA_SIDE_OFFSET + 4)
                    && memory::read_i32(a.add(song_reset::GPA_SIDE_OFFSET)) == side as i32
            })
            .map(|a| a as usize)
            .unwrap_or(0);
    }
    let gpa = state.gpa as *const u8;
    if gpa.is_null() || !memory::is_readable(gpa, off + 4) {
        state.gpa = 0;
        return None;
    }
    let v = memory::read_i32(gpa.add(off));
    (v > 0).then_some(v)
}

unsafe extern "C" fn init_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(INIT_HOOK)).as_ref() else {
        return;
    };
    let handled = SITES.get().is_some_and(|s| {
        std::panic::catch_unwind(|| unsafe { legacy_init(&s.0, actor) }).unwrap_or(false)
    });
    if !handled {
        hook.call(actor);
    }
}

unsafe extern "C" fn update_hook(actor: *mut u8) {
    let Some(hook) = (*addr_of!(UPDATE_HOOK)).as_ref() else {
        return;
    };
    let handled = SITES.get().is_some_and(|s| {
        std::panic::catch_unwind(|| unsafe { legacy_update(&s.0, actor) }).unwrap_or(false)
    });
    if !handled {
        hook.call(actor);
    }
}
