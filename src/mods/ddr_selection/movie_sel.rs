//! `_sel` background movies: a song played with a legacy era gets A3's DDR
//! SELECTION movie (`data/mdb_apx/movie/<name>_sel.wmv`) through World's own
//! dormant path (RE `.agents/planning/2026-09-22-ddr-selection/research/`
//! `end-banners-sel-movies.md` §4; pure rules in [`sel_movie_logic`]).
//!
//! ONE detour, on `SceneManageActor::onInitialize` (RTTI slot 4 — the
//! function that creates the song's MovieActor; its own onInitialize, which
//! picks the file, runs on a later tree tick):
//!
//! * **Pre-original**: when a legacy skin is armed and the `_sel` file exists
//!   (LayeredFS mod folders first, then the stock `data/`), and World's gate
//!   would NOT create a MovieActor (11 of the 18 `_sel` songs have no World
//!   movie), the music-info entry's decisive movie byte is set to
//!   [`sel_movie_logic::FORCED_MOVIE_KIND`] for this one call. A data write
//!   on the one entry rather than a code patch of the gate: it cannot affect
//!   any other song, needs no per-build instruction shape beyond the offsets
//!   decoded from the gate itself, and the MovieActor's own init (which
//!   re-reads the byte to pick a layout value) sees the stock byte again and
//!   keeps World's default for `_w` / `_sel` movies. VIDEO SIZE is never
//!   touched: OFF still means no MovieActor, no movie.
//! * **Post-original**: the byte is restored; if World created a MovieActor
//!   (`SceneManageActor+0xD8`, RTTI-vtable-verified), its `_sel`-first flag
//!   byte is set — World's init then tries `<name>_sel` before `_w` / bare /
//!   `_vj` / `_m`.
//! * A per-frame check (≤ 10 s) logs the path the MovieActor actually opened.
//!
//! Game thread only (the detour runs inside the actor tree's init). Fail-open:
//! no derivation / no file / no actor ⇒ World's own movie rules, one WARN per
//! cause.

use std::ffi::{c_char, CString};
use std::path::Path;
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::memory;
use crate::core::signatures::{DdrSelMovieSites, SignatureStore};
use crate::services::avs_layeredfs::mod_paths;
use crate::services::{input_manager, song_reset};
use crate::{log_info, log_warn};

use super::sel_movie_logic::{self as logic, Plan};

type InitFn = unsafe extern "C" fn(*mut u8);
type LookupFn = unsafe extern "C" fn(*const c_char) -> *mut u8;
type NameFn = unsafe extern "C" fn(*mut u8) -> *const c_char;

/// Frames the post-hook check waits for the MovieActor to pick its file.
const VERIFY_FRAMES: u32 = 600;
/// `agcs::Actor` embedded StackStep (values `+0x58 + idx*8`, index `+0x82`).
const ACTOR_STEP_BASE: usize = 0x58;
const ACTOR_STEP_INDEX: usize = 0x82;
/// MovieActor step "no movie found".
const MOVIE_STEP_NONE: i32 = 4;
const MAX_CHILDREN: usize = 256;

static mut HOOK: Option<GenericDetour<InitFn>> = None;
static SITES: OnceLock<SitesSync> = OnceLock::new();
static CAPABLE: AtomicBool = AtomicBool::new(false);
static WARNED: AtomicU32 = AtomicU32::new(0);
const W_ACTOR: u32 = 1;
const W_ENTRY: u32 = 2;
static FRAME_CB: Mutex<Option<usize>> = Mutex::new(None);
/// The MovieActor whose file is being verified (0 = none) + frames left.
static VERIFY_ACTOR: AtomicUsize = AtomicUsize::new(0);
static VERIFY_LEFT: AtomicU32 = AtomicU32::new(0);

struct SitesSync {
    sites: DdrSelMovieSites,
    sma_vtable: *const u8,
    module_lo: usize,
    module_hi: usize,
}
unsafe impl Send for SitesSync {}
unsafe impl Sync for SitesSync {}

fn warn_once(bit: u32) -> bool {
    WARNED.fetch_or(bit, Ordering::Relaxed) & bit == 0
}

/// Resolve the sites (mod init). Nothing is installed yet.
pub fn init(signatures: &SignatureStore, module_lo: usize, module_size: usize) {
    let (Some(sites), Some(sma_vt)) = (
        signatures.ddr_sel_movie_sites(),
        signatures.get_address("scene_manage_actor_vtable"),
    ) else {
        log_warn!("DDR SELECTION: _sel movie sites unresolved -- World's movie rules stay");
        return;
    };
    let _ = SITES.set(SitesSync {
        sites,
        sma_vtable: sma_vt,
        module_lo,
        module_hi: module_lo + module_size,
    });
}

/// Install the SceneManageActor init detour (mod enable; once).
pub fn start() {
    if !CAPABLE.load(Ordering::Acquire) {
        let Some(s) = SITES.get() else {
            return;
        };
        let target: InitFn = unsafe { std::mem::transmute(s.sites.sma_init) };
        match unsafe { hooks::install_enabled(std::ptr::addr_of_mut!(HOOK), target, init_hook) } {
            Ok(()) => {
                CAPABLE.store(true, Ordering::Release);
                log_info!("DDR SELECTION: SceneManageActor init detour installed (_sel movies)");
            }
            Err(e) => {
                log_warn!(
                    "DDR SELECTION: SceneManageActor init detour failed: {e} -- no _sel movies"
                );
                return;
            }
        }
    }
    let mut cb = FRAME_CB.lock().unwrap_or_else(|p| p.into_inner());
    if cb.is_none() {
        *cb = Some(input_manager::on_frame(std::sync::Arc::new(verify_frame)));
    }
}

/// Mod disable: the detour stays (a passthrough while disarmed).
pub fn stop() {
    let mut cb = FRAME_CB.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(id) = cb.take() {
        input_manager::remove_frame_callback(id);
    }
    VERIFY_ACTOR.store(0, Ordering::Release);
}

/// The `_sel` movies can play on this boot.
pub fn capable() -> bool {
    CAPABLE.load(Ordering::Acquire)
}

/// What the pre-original half decided (for the post-original half).
struct Decision {
    plan: Plan,
    rel: String,
    /// `(entry byte address, stock value)` of a forced gate byte.
    forced: Option<(*mut u8, u8)>,
}

unsafe extern "C" fn init_hook(sma: *mut u8) {
    let Some(hook) = (*addr_of!(HOOK)).as_ref() else {
        return;
    };
    let decision = if super::armed_skin() != 0 {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { before(sma) }))
            .ok()
            .flatten()
    } else {
        None
    };
    hook.call(sma);
    if let Some(d) = decision {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { after(sma, d) }));
    }
}

/// An MSVC `std::string` (SSO 16) at `addr`, empty allowed.
unsafe fn read_string(addr: *const u8) -> Option<String> {
    if !memory::is_readable(addr, 0x20) {
        return None;
    }
    let len = memory::read_u64(addr.add(0x10)) as usize;
    let cap = memory::read_u64(addr.add(0x18)) as usize;
    if len > cap || cap > 0x400 {
        return None;
    }
    if len == 0 {
        return Some(String::new());
    }
    let buf = if cap >= 0x10 {
        memory::read_ptr(addr)
    } else {
        addr
    };
    if buf.is_null() || !memory::is_readable(buf, len) {
        return None;
    }
    let bytes = std::slice::from_raw_parts(buf, len);
    bytes
        .iter()
        .all(|b| b.is_ascii_graphic())
        .then(|| String::from_utf8_lossy(bytes).into_owned())
}

/// A NUL-terminated game string, bounded.
unsafe fn read_cstr(p: *const c_char) -> Option<String> {
    if p.is_null() || !memory::is_readable(p as *const u8, 1) {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..0x40 {
        let c = *(p as *const u8).add(i);
        if c == 0 {
            return Some(String::from_utf8_lossy(&out).into_owned());
        }
        if !memory::is_readable((p as *const u8).add(i + 1), 1) {
            return None;
        }
        out.push(c);
    }
    None
}

/// The `_sel` file exists: a LayeredFS mod folder first, then stock `data/`.
fn sel_exists(rel: &str) -> bool {
    mod_paths::find_first_modfile(rel).is_some() || Path::new(&format!("data/{rel}")).is_file()
}

unsafe fn before(sma: *mut u8) -> Option<Decision> {
    let s = SITES.get()?;
    let st = &s.sites;
    let basename = read_string(sma.add(st.sma_basename_off))?;
    let suffix = read_string(sma.add(st.sma_suffix_off)).unwrap_or_default();
    let c_base = CString::new(basename.clone()).ok()?;
    let lookup: LookupFn = std::mem::transmute(st.music_lookup);
    let entry = lookup(c_base.as_ptr());
    let readable = !entry.is_null()
        && memory::is_readable(entry, st.movie_name_off.max(st.movie_kind_off) + 0x20);
    if !entry.is_null() && !readable {
        if warn_once(W_ENTRY) {
            log_warn!("DDR SELECTION: music-info entry of {basename} unreadable -- no _sel movie");
        }
        return None;
    }
    // World's name: the entry's override, else its basename (vslot 1).
    let name = if readable {
        let over = read_string(entry.add(st.movie_name_off)).unwrap_or_default();
        if over.is_empty() {
            entry_basename(s, entry).unwrap_or_else(|| basename.clone())
        } else {
            over
        }
    } else {
        basename.clone()
    };
    let (b1, b2) = if readable {
        (
            memory::read_u8(entry.add(st.movie_kind_off)),
            memory::read_u8(entry.add(st.movie_kind2_off)),
        )
    } else {
        (0, 0)
    };
    let world = logic::world_has_movie(readable, b1, b2);
    let rel = logic::sel_movie_rel(&name, &suffix);
    let plan = logic::plan(true, sel_exists(&rel), world);
    match plan {
        Plan::Stock => {
            log_info!("DDR SELECTION: no _sel movie for {basename} ({rel} not found) -- World's movie rules");
            return None;
        }
        Plan::Flag => log_info!(
            "DDR SELECTION: _sel movie {rel} found ({basename} has a World movie too) -- flagging the MovieActor"
        ),
        Plan::ForceAndFlag => {}
    }
    let forced = if plan == Plan::ForceAndFlag {
        let byte = entry.add(st.movie_kind_off);
        memory::write_u8(byte, logic::FORCED_MOVIE_KIND);
        log_info!(
            "DDR SELECTION: _sel movie {rel} found ({basename} has no World movie: movie byte {} -> {} for this init, restored after)",
            b1,
            logic::FORCED_MOVIE_KIND
        );
        Some((byte, b1))
    } else {
        None
    };
    Some(Decision { plan, rel, forced })
}

/// The entry's vtable slot 1 (its basename `const char*`), module-checked.
unsafe fn entry_basename(s: &SitesSync, entry: *mut u8) -> Option<String> {
    let vt = memory::read_ptr(entry);
    if !memory::is_readable(vt.add(8), 8) {
        return None;
    }
    let f = memory::read_ptr(vt.add(8)) as usize;
    if !(s.module_lo..s.module_hi).contains(&f) {
        return None;
    }
    let get: NameFn = std::mem::transmute(f);
    read_cstr(get(entry)).filter(|n| !n.is_empty())
}

unsafe fn after(sma: *mut u8, d: Decision) {
    let Some(s) = SITES.get() else {
        return;
    };
    let st = &s.sites;
    if let Some((byte, stock)) = d.forced {
        memory::write_u8(byte, stock);
    }
    let video_size = if memory::is_readable(sma.add(st.sma_video_size_off), 4) {
        memory::read_i32(sma.add(st.sma_video_size_off))
    } else {
        -1
    };
    let movie = memory::read_ptr(sma.add(st.sma_movie_off)) as *mut u8;
    if movie.is_null() {
        log_info!(
            "DDR SELECTION: no MovieActor for this song (VIDEO SIZE {}{}) -- no movie",
            video_size,
            if matches!(video_size, 1 | 2) {
                ", allocation failed"
            } else {
                " = OFF"
            }
        );
        return;
    }
    if !memory::is_readable(movie, st.sel_flag_off + 1)
        || memory::read_ptr(movie) != st.movie_actor_vtable
    {
        if warn_once(W_ACTOR) {
            log_warn!(
                "DDR SELECTION: SceneManageActor+0x{:X} is not a MovieActor -- no _sel movie",
                st.sma_movie_off
            );
        }
        return;
    }
    memory::write_u8(movie.add(st.sel_flag_off), 1);
    log_info!(
        "DDR SELECTION: MovieActor 0x{:X} will try {} first ({}, VIDEO SIZE {})",
        movie as usize,
        d.rel,
        match d.plan {
            Plan::ForceAndFlag => "actor created for a song without a World movie",
            _ => "World's own actor",
        },
        video_size
    );
    VERIFY_ACTOR.store(movie as usize, Ordering::Release);
    VERIFY_LEFT.store(VERIFY_FRAMES, Ordering::Release);
}

/// Per frame (game thread): once the flagged MovieActor ran its init, log
/// the file it picked. The actor is re-found from the live DancePlaySequence
/// every frame (it dies with the song).
fn verify_frame() {
    let actor = VERIFY_ACTOR.load(Ordering::Acquire);
    if actor == 0 {
        return;
    }
    let left = VERIFY_LEFT.load(Ordering::Acquire);
    if left == 0 {
        VERIFY_ACTOR.store(0, Ordering::Release);
        log_info!(
            "DDR SELECTION: _sel movie not verified (the MovieActor did not pick a file in time)"
        );
        return;
    }
    VERIFY_LEFT.store(left - 1, Ordering::Release);
    let Some(s) = SITES.get() else {
        return;
    };
    let Some(live) = (unsafe { live_movie_actor(s) }) else {
        return; // not reachable this frame (or the song ended — times out)
    };
    if live as usize != actor {
        VERIFY_ACTOR.store(0, Ordering::Release);
        return; // a different song's actor: that one was checked on its own
    }
    unsafe {
        let path = read_string(live.add(s.sites.movie_path_off)).unwrap_or_default();
        let step = actor_step(live);
        if !path.is_empty() {
            VERIFY_ACTOR.store(0, Ordering::Release);
            if logic::is_sel_path(&path) {
                log_info!("DDR SELECTION: MovieActor opened {path} (_sel movie)");
            } else {
                log_warn!("DDR SELECTION: MovieActor opened {path} -- not the _sel movie");
            }
        } else if step == Some(MOVIE_STEP_NONE) {
            VERIFY_ACTOR.store(0, Ordering::Release);
            log_warn!("DDR SELECTION: the flagged MovieActor found no movie file (step 4)");
        }
    }
}

/// The live DancePlaySequence's SceneManageActor's MovieActor (vtables
/// verified), or `None`.
unsafe fn live_movie_actor(s: &SitesSync) -> Option<*mut u8> {
    song_reset::dps_step()?;
    let dps = song_reset::live_dps()? as *const u8;
    let mut child = memory::read_ptr(dps.add(song_reset::FIRST_CHILD_OFFSET));
    let mut n = 0usize;
    while !child.is_null() && n < MAX_CHILDREN {
        n += 1;
        if memory::read_ptr(child) == s.sma_vtable {
            let movie = memory::read_ptr(child.add(s.sites.sma_movie_off)) as *mut u8;
            if movie.is_null()
                || !memory::is_readable(movie, s.sites.movie_path_off + 0x20)
                || memory::read_ptr(movie) != s.sites.movie_actor_vtable
            {
                return None;
            }
            return Some(movie);
        }
        child = memory::read_ptr(child.add(song_reset::NEXT_SIBLING_OFFSET));
    }
    None
}

unsafe fn actor_step(actor: *const u8) -> Option<i32> {
    if !memory::is_readable(actor, ACTOR_STEP_BASE + 5 * 8) {
        return None;
    }
    let idx = *(actor.add(ACTOR_STEP_INDEX) as *const u16) as usize;
    (idx < 5).then(|| memory::read_i32(actor.add(ACTOR_STEP_BASE + idx * 8)))
}
