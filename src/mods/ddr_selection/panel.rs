//! The legacy stage panel: A3's pre-song stage-choice composite, its era
//! cut-in and its stage call, hosted in World's ShutterActor kind 3 (design
//! D28, §4.5; RE `.agents/planning/2026-09-22-ddr-selection/research/`
//! `stage-panel.md`). Two variants ([`panel_logic::Variant`]): the eras'
//! legacy fill on `common_choice_v2`'s root, and — for the themes — A3's own
//! skin-0 fill on the theme generation's root (`common_choice_v0` / `_v2` /
//! `_v1`): the band texture on the root's own `choice_stage_usr`, the root's
//! own background and jacket frame, the stage call at once, no cut-in, and
//! A3's per-player score sets (`score_set.rs` / `score_set_logic.rs`) — the
//! theme session's only packages are the sets' glyphs / digits
//! (`common_texture_v0`) and area names (`common_area_lang_<lang>_vN`).
//!
//! World's kind 3 is the jacket / stage / difficulty screen between song
//! select and the lanes (`shutter_play` of `common_shutter_v3`). Hosting A3's
//! panel in the same kind keeps every World gate that waits on it valid (the
//! stage loader's gate A, the DancePlaySequence steps 1 / 5, quick restart's
//! bannerless `0x100c`):
//!
//! * **Just-in-time hosting.** World requests the stage panel from the song
//!   select confirm (scene 25 — cabinet run #1: the 25 → 26 arm was always
//!   too late) and, when the shutter is still idle, from
//!   `SelectMusicTerminateSequence` (scene 26). The detour sees the request
//!   PRE-original, in the update whose state 0 reads the kind table: during
//!   song select it resolves the song's skin right there from the wheel's
//!   highlighted song (`PlayerWork+0x54` is still the PREVIOUS song at that
//!   point — `super::stage_panel_request_skin`), requests the era
//!   packages and creates the session; later requests use the session the
//!   25 → 26 arm created.
//! * **Row patch, for one update only**: the default kind table's stage row
//!   gets `pkg = "common_choice_v2"` (a theme: its own root package),
//!   `root = "shutter_choice_hd_root"`,
//!   `SE in = ""` just before World's kind-art loader reads it and is restored
//!   right after (the loader copies the row) — World's own named-package path
//!   (the one its galaxy-brave shutter uses) then loads and creates A3's root
//!   in the stage slot, and its state machine drives the legacy labels
//!   natively (`in`, `loop` = covered, `out`, `end`).
//! * **One detour** on `ShutterActor::onUpdate` (RTTI slot 6) drives the pure
//!   [`panel_logic::Panel`] machine from what each update did (post-original):
//!   the adoption (the pending stage clip is A3's root — only then do World's
//!   stage voice, READY dismissal and dwell stand down; on the old layout,
//!   pre-original of every stage-kind state-2 update, World's
//!   un-null-checked `jacket_usr` SetVisible is NOPed whenever the pending
//!   clip has no `jacket_usr`),
//!   the legacy fill (A3 `FUN_180030d10`: the era `choice_stage` /
//!   `choice_background` / `choice_jacket` loaded into the root's
//!   placeholders, the stage band texture), the cut-in (`choice_cutin` of
//!   `common_choice_cutin000N`, SE `sele_*`, START-skippable after frame
//!   0x3B; the root is held at frame 0 until the cut-in's `close`), the jacket
//!   (skins 1–2 none, 3 the SuperNOVA 2 banner, 4–5 the song jacket), the
//!   stage call at the stage clip's `voice` label, `frame_out` when World
//!   parks the panel (state 6), and the READY dismissal (A3's ReadyGo
//!   `0x100D` = World `0x100c`).
//! * World's own stage voice is silenced by a `code_se` site while A3's root
//!   is live; the DancePlaySequence READY? dwell is skipped then too (A3's
//!   panel holds itself). A session alone never does either — World's panel
//!   may still be the one on screen (cabinet run #1).
//! * The era cut-in is optional (GLOBAL SETTINGS → DDR SELECTION → Era
//!   Cut-In, `settings::era_cutin`, latched per panel): OFF requests no
//!   cut-in packages and the root plays `in` at World's swap (A3's own
//!   "cut-in not loaded" path).
//! * The era packages are requested with the session; the fill waits for
//!   them (at the latest until World's swap). World's own kind-3 fill runs on
//!   A3's root first and is harmless on every build (its child lookups are
//!   `afp_layer_mc_refer` misses, its SpriteLayers hide anchor-less) except
//!   the old layout's un-null-checked `jacket_usr` SetVisible (NOPed above).
//!
//! Old builds (20250805 / 20260224): stage kind 1, 0x30-stride rows (the first
//! three pointers laid out like the 0x40 rows), the same named-package branch
//! and state machine — everything above applies unchanged.
//!
//! Layer-before-package: the four era packages (a theme: the two score-set
//! packages) are ours (tickets); the root
//! layer that references them is World's and dies at the drain's state 8. The
//! tickets are released only once that layer id is invalid and a grace period
//! passed (the release queue is polled from the same detour, every frame).
//!
//! Fail-open: without every requirement nothing is hosted (World's panel and
//! the Step 4 dismissal); a root that turns out not to be A3's is left alone.

use std::ffi::{CStr, CString};
use std::ptr::addr_of;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::GenericDetour;

use crate::core::hooks;
use crate::core::memory;
use crate::core::signatures::{DdrSelPanelSites, SignatureStore};
use crate::services::bm2d_api::{self, AfpLayer};
use crate::services::bm2d_package::{self, LoadTicket};
use crate::services::{asset_loader, input_manager, shutter, song_reset, stage_records};
use crate::types::buttons::{button, Player};
use crate::{log_info, log_warn};

use super::panel_logic::{self as logic, Action, CutinView, Frame, Jacket, Panel, StageCtx};
use super::score_set_logic;

type UpdateFn = unsafe extern "C" fn(*mut u8);

/// `afp_mc_op` ops (libafp RE; World `CMovieClip` wrappers).
const MC_OP_GOTO_LABEL: i32 = 0xF03;
const MC_OP_GOTO_STOP: i32 = 0xF04;
const MC_OP_GOTO_LABEL_PLAY: i32 = 0xF09;
/// `afp_mc_set_param` visibility / dirty.
const MC_PARAM_VISIBLE: i32 = 0x1007;
const MC_PARAM_DIRTY: i32 = 0x101E;
/// `afp_mc_traversal` directions: first child / next sibling / next instance.
const TRAVERSE_CHILD: i32 = 1;
const TRAVERSE_SIBLING: i32 = 3;
const TRAVERSE_INSTANCE: i32 = 6;
/// The standard post-create display attribute; bit 0 = visible.
const ATTR_DISPLAY_SETUP: u32 = 0x200;
const ATTR_VISIBLE: u32 = 0x1;
const CUTIN_GROUP: u16 = 5;
/// Frames between World releasing the root and our package release.
const RELEASE_GRACE_FRAMES: u32 = 60;
/// Value seeded into the READY?-dwell timer (the quick-restart value).
const DWELL_SEED: f32 = 1000.0;

static ROOT_CLIP_C: &CStr = c"shutter_choice_hd_root";
static BM2D_DIR: &CStr = c"bm2d";
static BANNER_DIR: &CStr = c"banner";

static mut HOOK: Option<GenericDetour<UpdateFn>> = None;
static SITES: OnceLock<SitesSync> = OnceLock::new();
/// The three stock row pointers (pkg, root, SE in) and the row's own `""`.
static STOCK_ROW: OnceLock<[usize; 4]> = OnceLock::new();
static CAPABLE: AtomicBool = AtomicBool::new(false);
static REQUIREMENTS_OK: AtomicBool = AtomicBool::new(false);
/// The mod is enabled (song-select requests may be hosted).
static ENABLED: AtomicBool = AtomicBool::new(false);
/// The current scene is song select (scene callback).
static IN_SONG_SELECT: AtomicBool = AtomicBool::new(false);
/// A session exists for this window (the next stage-panel request of this
/// window loads A3's root).
static HOSTED: AtomicBool = AtomicBool::new(false);
/// World's stage slot holds A3's root, adopted by us: World's stage voice,
/// READY dismissal and READY? dwell stand down (cabinet run #1: gating those
/// on the arm alone doubled World's READY? when nothing was hosted).
static ROOT_LIVE: AtomicBool = AtomicBool::new(false);
static ROW_PATCHED: AtomicBool = AtomicBool::new(false);
/// The root-package pointer the row patch wrote (restore expects it).
static ROW_PKG: AtomicUsize = AtomicUsize::new(0);
/// Old layout: the `jacket_usr` SetVisible CALL is NOPed.
static JACKET_CALL_NOPED: AtomicBool = AtomicBool::new(false);
/// Its stock bytes (read at init).
static JACKET_CALL_STOCK: OnceLock<[u8; 5]> = OnceLock::new();
const NOP5: [u8; 5] = [0x0F, 0x1F, 0x44, 0x00, 0x00];
/// Tickets (or a banner) waiting for the root layer to die.
static RELEASE_PENDING: AtomicBool = AtomicBool::new(false);
static DWELL_TIMER_OFF: AtomicUsize = AtomicUsize::new(0);
static WARNED: AtomicU32 = AtomicU32::new(0);
const W_UNAVAILABLE: u32 = 1;
const W_PACKAGE: u32 = 2;
const W_FOREIGN: u32 = 4;
const W_DISMISS: u32 = 8;
const W_ROW: u32 = 16;
const W_MOVIE: u32 = 32;
const W_JACKET_NOP: u32 = 64;
const W_MISMATCH: u32 = 128;

struct SitesSync(DdrSelPanelSites);
unsafe impl Send for SitesSync {}
unsafe impl Sync for SitesSync {}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
static RELEASES: Mutex<Vec<PendingRelease>> = Mutex::new(Vec::new());

struct Tickets {
    choice: Option<LoadTicket>,
    shutter: Option<LoadTicket>,
    cutin: Option<LoadTicket>,
    cutin_bg: Option<LoadTicket>,
    /// Theme score sets: `common_texture_v0` (name glyphs, score digits).
    texture: Option<LoadTicket>,
    /// Theme score sets: `common_area_lang_<lang>_vN` (area names).
    area: Option<LoadTicket>,
}

impl Tickets {
    const fn none() -> Self {
        Tickets {
            choice: None,
            shutter: None,
            cutin: None,
            cutin_bg: None,
            texture: None,
            area: None,
        }
    }

    fn into_vec(self) -> Vec<LoadTicket> {
        [
            self.choice,
            self.shutter,
            self.cutin,
            self.cutin_bg,
            self.texture,
            self.area,
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// Every requested ticket is ready (one that could not be requested has
    /// nothing to wait for: its fields stay hidden).
    fn ready(&self) -> bool {
        [
            &self.choice,
            &self.shutter,
            &self.cutin,
            &self.cutin_bg,
            &self.texture,
            &self.area,
        ]
        .iter()
        .all(|t| t.as_ref().is_none_or(bm2d_package::is_ready))
    }
}

struct Cutin {
    layer: AfpLayer,
    mc: u32,
    out: Option<u32>,
    close: Option<u32>,
    se: Option<&'static str>,
}

struct Banner {
    stem: String,
    handle: Option<asset_loader::AssetHandle>,
    applied: bool,
}

struct Session {
    skin: u8,
    /// Era (A3's legacy fill) or theme (A3's own skin-0 fill).
    variant: logic::Variant,
    /// The package the row patch names for the root (static).
    root_pkg: &'static CStr,
    /// The era cut-in plays before the panel (GLOBAL SETTINGS, latched here).
    cutin_enabled: bool,
    /// Where the session was created (logs): the song-select request or the
    /// play edge.
    origin: &'static str,
    tickets: Tickets,
    machine: Panel,
    /// The adoption refused the root (World's own panel is on screen).
    abandoned: bool,
    /// Stage-panel requests World made this window (diagnostics).
    requests: u32,
    /// World's kind-art loader read our row (A3's root is on its way).
    row_consumed: bool,
    /// Last shutter (state, pending, active) seen while hosted (diagnostics).
    last_seen: Option<(i32, i32, i32)>,
    /// Our legacy root (World's stage clip, verified A3's at the adoption).
    root: Option<shutter::KindClip>,
    /// Its layer id (kept after `retire` took `root`), so the release queue
    /// can wait for it exactly once.
    root_layer_for_release: u32,
    root_data_release: Option<u32>,
    stage_mc: Option<u32>,
    voice_label: u32,
    voice: Option<String>,
    jacket: Jacket,
    banner: Option<Banner>,
    cutin: Option<Cutin>,
    start_held: bool,
}

unsafe impl Send for Session {}

struct PendingRelease {
    tickets: Vec<LoadTicket>,
    banner: Option<asset_loader::AssetHandle>,
    root_layer: u32,
    frames: u32,
}

unsafe impl Send for PendingRelease {}

fn warn_once(bit: u32) -> bool {
    WARNED.fetch_or(bit, Ordering::Relaxed) & bit == 0
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// Resolve the sites (mod init). Nothing is installed yet.
pub fn init(signatures: &SignatureStore) {
    if let Some(off) = signatures.ddr_sel_dps_ready_timer_off() {
        DWELL_TIMER_OFF.store(off, Ordering::Release);
    }
    let Some(sites) = signatures.ddr_sel_panel_sites() else {
        log_warn!("DDR SELECTION: stage-panel sites unresolved -- World's stage panel stays");
        return;
    };
    if !sites.host_ok {
        log_info!(
            "DDR SELECTION: this build's ShutterActor cannot host A3's stage panel -- World's panel stays"
        );
        return;
    }
    let row = sites.stage_row as *const usize;
    let stock = unsafe { [*row, *row.add(1), *row.add(2), *row.add(3)] };
    let _ = STOCK_ROW.set(stock);
    if let Some(call) = sites.jacket_vis_call {
        let mut bytes = [0u8; 5];
        unsafe { std::ptr::copy_nonoverlapping(call, bytes.as_mut_ptr(), 5) };
        if bytes[0] != 0xE8 {
            log_warn!(
                "DDR SELECTION: the jacket_usr SetVisible site is not a CALL -- World's stage panel stays"
            );
            return;
        }
        let _ = JACKET_CALL_STOCK.set(bytes);
    }
    let _ = SITES.set(SitesSync(sites));
    // The legacy end banners ride the same detour.
    super::banner::init(sites.banner_rows);

    let mut missing = Vec::new();
    if !bm2d_api::is_available() || !bm2d_api::afp_layers_available() {
        missing.push("AFP layer API");
    }
    if !bm2d_api::mc_load_movie_available() {
        missing.push("afp_mc_load_movie");
    }
    if !bm2d_package::is_available() {
        missing.push("BM2D package registry");
    }
    if !shutter::is_available() {
        missing.push("ShutterActor");
    }
    if !song_reset::intro_cascade_available() {
        missing.push("ControlMessageActor cascade");
    }
    if missing.is_empty() {
        REQUIREMENTS_OK.store(true, Ordering::Release);
    } else {
        log_warn!(
            "DDR SELECTION: legacy stage panel unavailable ({}) -- World's stage panel stays",
            missing.join(", ")
        );
    }
}

/// Install the ShutterActor update detour (mod enable; once).
pub fn start() {
    if CAPABLE.load(Ordering::Acquire) || !REQUIREMENTS_OK.load(Ordering::Acquire) {
        return;
    }
    let Some(sites) = SITES.get() else {
        return;
    };
    let target: UpdateFn = unsafe { std::mem::transmute(sites.0.shutter_update) };
    match unsafe { hooks::install_enabled(std::ptr::addr_of_mut!(HOOK), target, update_hook) } {
        Ok(()) => {
            CAPABLE.store(true, Ordering::Release);
            log_info!("DDR SELECTION: ShutterActor update detour installed (legacy stage panel)");
        }
        Err(e) => log_warn!(
            "DDR SELECTION: ShutterActor update detour failed: {e} -- World's stage panel stays"
        ),
    }
}

/// The legacy panel can be hosted on this boot.
pub fn capable() -> bool {
    CAPABLE.load(Ordering::Acquire)
}

/// The mod toggle (song-select requests are only hosted while enabled).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Release);
}

/// Scene callback (game thread, before `createNextSequence`).
pub fn on_scene_change(next: i32) {
    IN_SONG_SELECT.store(
        next == crate::types::scenes::scene::SONG_SELECT,
        Ordering::Release,
    );
}

/// A session exists for this window.
pub fn hosted() -> bool {
    HOSTED.load(Ordering::Acquire)
}

/// The skin of this window's session, if any.
pub fn hosted_skin() -> Option<u8> {
    if !hosted() {
        return None;
    }
    lock(&SESSION).as_ref().map(|s| s.skin)
}

/// A3's root is in World's stage slot (lock-free; also read from libafp's
/// display pass by the sound route).
pub fn root_live() -> bool {
    ROOT_LIVE.load(Ordering::Acquire)
}

/// The legacy panel owns the READY dismissal (A3's root is live) — the
/// World-art dismissal in `intro`, World's stage voice and World's READY?
/// dwell stand down. Never true merely because a session exists.
pub fn handles_dismissal() -> bool {
    root_live()
}

/// Adopt / drop A3's root (game thread): World's stage voice follows via
/// `code_se`. (The old layout's jacket SetVisible NOP is independent of the
/// session: [`guard_jacket_call`].)
fn set_root_live(live: bool) {
    if ROOT_LIVE.swap(live, Ordering::AcqRel) == live {
        return;
    }
    super::sound::code_se::sync();
}

/// Old layout only: World's state 2 runs `SetVisible(find(stage clip,
/// "jacket_usr"), 1)` with no null check, and A3's root has no `jacket_usr`.
/// Pre-original of every update that is about to run state 2 for the stage
/// kind, NOP the CALL exactly when the pending stage clip lacks `jacket_usr`
/// (restore it otherwise). Evaluated from the clip itself, not from our
/// session, so a root that outlives a disarm / disable is still covered.
unsafe fn guard_jacket_call(actor: *mut u8, pre: &shutter::Snapshot) {
    if JACKET_CALL_STOCK.get().is_none() {
        return;
    }
    let Some(stage) = shutter::stage_kind() else {
        return;
    };
    if pre.state != logic::ST_SWAP || pre.pending_kind != stage {
        return;
    }
    let Some(clip) = shutter::kind_clip(actor, stage) else {
        return; // World would crash on its own (null clip object); not ours
    };
    let missing = bm2d_api::layer_find_child(clip.layer, "jacket_usr").is_none();
    if !set_jacket_call_nop(missing) && missing && warn_once(W_JACKET_NOP) {
        log_warn!(
            "DDR SELECTION: the pending stage clip has no jacket_usr and World's SetVisible could not be patched out"
        );
    }
}

/// `true` = NOP the old-layout jacket SetVisible CALL. Returns whether the
/// site is in the wanted state.
fn set_jacket_call_nop(on: bool) -> bool {
    let (Some(call), Some(stock)) = (
        sites().and_then(|s| s.jacket_vis_call),
        JACKET_CALL_STOCK.get(),
    ) else {
        return true; // new layout: null-checked, nothing to do
    };
    if JACKET_CALL_NOPED.load(Ordering::Acquire) == on {
        return true;
    }
    let (from, to) = if on { (stock, &NOP5) } else { (&NOP5, stock) };
    match unsafe { memory::apply_checked_patch(call as *mut u8, from, to) } {
        Ok(()) => {
            JACKET_CALL_NOPED.store(on, Ordering::Release);
            true
        }
        Err(e) => {
            if warn_once(W_JACKET_NOP) {
                log_warn!(
                    "DDR SELECTION: jacket_usr SetVisible patch ({}) failed: {:?}",
                    if on { "apply" } else { "restore" },
                    e
                );
            }
            false
        }
    }
}

/// Arm the panel for a legacy song (game thread): the song-select stage-panel
/// request (`update_hook`, pre-original) or the 25 → 26 scene callback,
/// whichever comes first. Idempotent for the same skin. The row is patched
/// only when World next requests the stage panel.
pub fn arm(skin: u8, origin: &'static str) {
    if let Some(have) = hosted_skin() {
        if have != skin && warn_once(W_MISMATCH) {
            log_warn!(
                "DDR SELECTION: the stage panel is hosted for skin {} but the song now resolves skin {} ({}) -- the panel keeps skin {}",
                have,
                skin,
                origin,
                have
            );
        }
        return;
    }
    disarm_session();
    if !capable() {
        return; // init / start already said why
    }
    if !super::intro::capable() {
        if warn_once(W_UNAVAILABLE) {
            log_warn!("DDR SELECTION: legacy stage panel needs the legacy READY -- World's stage panel stays");
        }
        return;
    }
    let (Some(variant), Some(root_pkg)) = (
        logic::variant(skin),
        logic::root_package_cstr(skin).and_then(|n| CStr::from_bytes_with_nul(n.as_bytes()).ok()),
    ) else {
        return;
    };
    let root_name = root_pkg.to_str().unwrap_or("?");
    if !super::package_helper::probe_arc(BM2D_DIR, root_pkg) {
        if warn_once(W_PACKAGE) {
            log_warn!(
                "DDR SELECTION: {} not found -- World's stage panel stays",
                root_name
            );
        }
        return;
    }
    let request = |name: &str| -> Option<LoadTicket> {
        let t = bm2d_package::request_load("bm2d", name);
        if t.is_none() && warn_once(W_PACKAGE) {
            log_warn!("DDR SELECTION: could not request {name} (stage panel)");
        }
        t
    };
    let (tickets, cutin_enabled, detail) = match variant {
        logic::Variant::Era => {
            let Some(names) = logic::packages(skin) else {
                return;
            };
            // A legacy end banner (`banner.rs`) makes World load
            // `common_shutter000N` by name and release it without a
            // refcount: while World's copy is still registered, a ticket of
            // ours would only borrow it and World's release would destroy it
            // under the panel's layers.
            if let Ok(c) = CString::new(names.shutter.as_str()) {
                if bm2d_package::lookup_unowned(&c).is_some()
                    && !bm2d_package::held_by_tickets(&names.shutter)
                {
                    if warn_once(W_PACKAGE) {
                        log_warn!(
                            "DDR SELECTION: {} is still World's (a legacy end banner) -- World's stage panel for this song",
                            names.shutter
                        );
                    }
                    return;
                }
            }
            // The cut-in's packages only when it plays (GLOBAL SETTINGS;
            // latched for this panel — an edit applies from the next song).
            let cutin_enabled = super::settings::era_cutin();
            let tickets = Tickets {
                choice: request(&names.choice),
                shutter: request(&names.shutter),
                cutin: cutin_enabled.then(|| request(&names.cutin)).flatten(),
                cutin_bg: cutin_enabled.then(|| request(names.cutin_bg)).flatten(),
                ..Tickets::none()
            };
            if tickets.choice.is_none() || tickets.shutter.is_none() {
                queue_release(tickets.into_vec(), None, 0);
                return;
            }
            let detail = format!(
                "{} + {}, {}, {}",
                root_name,
                names.choice,
                names.shutter,
                if cutin_enabled {
                    names.cutin.as_str()
                } else {
                    "era cut-in off"
                }
            );
            (tickets, cutin_enabled, detail)
        }
        // A3's own UI: everything is in the theme's root package; no cut-in.
        // The score sets' glyphs / digits and area names are the only
        // packages (A3 held them resident; World loads neither).
        logic::Variant::Theme => {
            let area_name = super::score_set::area_package(skin).filter(|n| {
                CString::new(n.as_str())
                    .is_ok_and(|c| super::package_helper::probe_arc(BM2D_DIR, &c))
            });
            let tickets = Tickets {
                texture: request(score_set_logic::TEXTURE_PACKAGE),
                area: area_name.as_deref().and_then(|n| request(n)),
                ..Tickets::none()
            };
            let detail = format!(
                "{} -- A3's own skin-0 panel, no cut-in; score sets {} + {}",
                root_name,
                score_set_logic::TEXTURE_PACKAGE,
                area_name.as_deref().unwrap_or("no area package")
            );
            (tickets, false, detail)
        }
    };
    *lock(&SESSION) = Some(Session {
        skin,
        variant,
        root_pkg,
        cutin_enabled,
        origin,
        tickets,
        machine: Panel::new(),
        abandoned: false,
        requests: 0,
        row_consumed: false,
        last_seen: None,
        root: None,
        root_layer_for_release: 0,
        root_data_release: None,
        stage_mc: None,
        voice_label: 0,
        voice: None,
        jacket: Jacket::Hidden,
        banner: None,
        cutin: None,
        start_held: true,
    });
    HOSTED.store(true, Ordering::Release);
    log_info!(
        "DDR SELECTION: legacy stage panel armed at {} (skin {}: {})",
        origin,
        skin,
        detail
    );
}

/// The play edge resolved this song differently from the song-select
/// request: drop the session unless World already read our row and A3's root
/// is on its way or live (only the session can fill / close it). An
/// abandoned session (World's own root appeared) is always dropped.
pub fn drop_if_unadopted() -> bool {
    let droppable = lock(&SESSION)
        .as_ref()
        .is_some_and(|s| !s.row_consumed || s.abandoned);
    if droppable {
        disarm_session_with(false);
    }
    droppable
}

/// Disarm (leaving the song window, mod disable): restore the row, hand the
/// packages to the release queue.
pub fn disarm() {
    disarm_session();
}

fn disarm_session() {
    disarm_session_with(true);
}

fn disarm_session_with(diagnose: bool) {
    HOSTED.store(false, Ordering::Release);
    if ROW_PATCHED.load(Ordering::Acquire) {
        patch_row(None);
    }
    set_root_live(false);
    let session = lock(&SESSION).take();
    if let Some(mut s) = session {
        if diagnose && s.machine.phase() == logic::Phase::Waiting {
            log_warn!(
                "DDR SELECTION: legacy stage panel (skin {}, armed at {}) never hosted -- {} stage-panel request(s) seen this window, row {}, last shutter (state, pending, active) {:?}",
                s.skin,
                s.origin,
                s.requests,
                if s.row_consumed {
                    "read by World (A3's root never appeared)"
                } else {
                    "never read"
                },
                s.last_seen
            );
        }
        retire(&mut s);
    }
}

/// Tear a session's own layers down and queue its packages.
fn retire(s: &mut Session) {
    if let Some(c) = s.cutin.take() {
        if let Some(se) = c.se {
            super::sound::stop_era_cue(se);
        }
        let _ = bm2d_api::destroy_layer(c.layer);
    }
    let banner = s.banner.as_mut().and_then(|b| b.handle.take());
    let tickets = std::mem::replace(&mut s.tickets, Tickets::none());
    let root_layer = std::mem::take(&mut s.root_layer_for_release);
    queue_release(tickets.into_vec(), banner, root_layer);
}

fn queue_release(
    tickets: Vec<LoadTicket>,
    banner: Option<asset_loader::AssetHandle>,
    root_layer: u32,
) {
    if tickets.is_empty() && banner.is_none() && root_layer == 0 {
        return;
    }
    lock(&RELEASES).push(PendingRelease {
        tickets,
        banner,
        root_layer,
        frames: 0,
    });
    RELEASE_PENDING.store(true, Ordering::Release);
}

/// The stage row: `Some(root package)` = A3's root from that package (a
/// static name), `None` = stock. Checked patch.
fn patch_row(legacy: Option<&'static CStr>) -> bool {
    let (Some(sites), Some(stock)) = (SITES.get(), STOCK_ROW.get()) else {
        return false;
    };
    let ours = |pkg: usize| [pkg, ROOT_CLIP_C.as_ptr() as usize, stock[3]];
    let stock3 = [stock[0], stock[1], stock[2]];
    let bytes = |v: [usize; 3]| -> Vec<u8> { v.iter().flat_map(|p| p.to_le_bytes()).collect() };
    let (from, to, pkg) = match legacy {
        Some(pkg) => {
            let pkg = pkg.as_ptr() as usize;
            (bytes(stock3), bytes(ours(pkg)), pkg)
        }
        None => (
            bytes(ours(ROW_PKG.load(Ordering::Acquire))),
            bytes(stock3),
            0,
        ),
    };
    match unsafe { memory::apply_checked_patch(sites.0.stage_row as *mut u8, &from, &to) } {
        Ok(()) => {
            ROW_PKG.store(pkg, Ordering::Release);
            ROW_PATCHED.store(legacy.is_some(), Ordering::Release);
            true
        }
        Err(e) => {
            if warn_once(W_ROW) {
                log_warn!(
                    "DDR SELECTION: stage row patch ({}) failed: {:?}",
                    if legacy.is_some() { "apply" } else { "restore" },
                    e
                );
            }
            false
        }
    }
}

unsafe extern "C" fn update_hook(this: *mut u8) {
    let Some(hook) = (*addr_of!(HOOK)).as_ref() else {
        return;
    };
    let watch = ENABLED.load(Ordering::Acquire)
        || HOSTED.load(Ordering::Acquire)
        || RELEASE_PENDING.load(Ordering::Acquire)
        || JACKET_CALL_STOCK.get().is_some()
        || super::banner::active();
    let pre = if watch {
        shutter::snapshot_of(this).ok()
    } else {
        None
    };
    let row_patched = match pre {
        Some(p) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            guard_jacket_call(this, &p);
            before_update(&p)
        }))
        .unwrap_or(false),
        None => false,
    };
    // The end banners (`banner.rs`): the same one-update row patch for the
    // CLEARED / FAILED rows, plus the overlay's safety net.
    let banner_row = match pre {
        Some(p) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::banner::before_update(this, &p)
        }))
        .unwrap_or(None),
        None => None,
    };
    hook.call(this);
    if row_patched {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            after_row_read(this)
        }));
    }
    if let Some(kind) = banner_row {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::banner::after_row_read(this, kind)
        }));
    }
    if HOSTED.load(Ordering::Acquire) || RELEASE_PENDING.load(Ordering::Acquire) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            after_update(this, pre)
        }));
    }
    if super::banner::active() {
        let post = shutter::snapshot_of(this).ok();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
            super::banner::after_update(this, pre, post)
        }));
    }
}

/// Pre-original. When this update's state 0 is about to load the stage
/// panel's art (it reads the kind table once, here): host the song if the
/// request comes from song select, and patch the row for this one read.
/// `true` = the row was patched (restore it right after the original).
unsafe fn before_update(pre: &shutter::Snapshot) -> bool {
    let Some(stage) = shutter::stage_kind() else {
        return false;
    };
    if pre.state != logic::ST_IDLE || pre.pending_kind != stage {
        return false;
    }
    if !HOSTED.load(Ordering::Acquire)
        && ENABLED.load(Ordering::Acquire)
        && IN_SONG_SELECT.load(Ordering::Acquire)
    {
        if let Some(skin) = super::stage_panel_request_skin() {
            arm(skin, "the song-select stage-panel request");
        }
    }
    if !HOSTED.load(Ordering::Acquire) {
        return false;
    }
    let mut guard = lock(&SESSION);
    let Some(s) = guard.as_mut() else {
        return false;
    };
    s.requests += 1;
    if s.row_consumed || s.machine.phase() != logic::Phase::Waiting {
        log_info!(
            "DDR SELECTION: stage-panel request #{} this window -- World's own panel (A3's was already used)",
            s.requests
        );
        return false;
    }
    if !patch_row(Some(s.root_pkg)) {
        return false;
    }
    s.row_consumed = true;
    true
}

/// Post-original of the update that read our row: restore it (the loader
/// copied it) and say what World did with it.
unsafe fn after_row_read(actor: *mut u8) {
    let pkg = lock(&SESSION)
        .as_ref()
        .map(|s| s.root_pkg.to_string_lossy().into_owned())
        .unwrap_or_default();
    patch_row(None);
    let post = shutter::snapshot_of(actor).ok();
    log_info!(
        "DDR SELECTION: World's kind-art loader read the stage row ({} / {}) -- shutter now state {:?}, pending kind {:?}; row restored",
        pkg,
        logic::ROOT_CLIP,
        post.map(|p| p.state),
        post.map(|p| p.pending_kind)
    );
}

unsafe fn after_update(this: *mut u8, pre: Option<shutter::Snapshot>) {
    let post = shutter::snapshot_of(this).ok();
    if RELEASE_PENDING.load(Ordering::Acquire) {
        poll_releases(this);
    }
    if !HOSTED.load(Ordering::Acquire) {
        return;
    }
    if handles_dismissal() {
        seed_dwell();
    }
    let (Some(pre), Some(post), Some(stage)) = (pre, post, shutter::stage_kind()) else {
        return;
    };
    let mut guard = lock(&SESSION);
    let Some(s) = guard.as_mut() else {
        return;
    };
    s.last_seen = Some((post.state, post.pending_kind, post.active_kind));
    if s.machine.phase() == logic::Phase::Gone {
        // World released the root: nothing left to observe (its MovieClip ids
        // are dead — reading them spams `afp_mc_get_param … is invalid`).
        return;
    }
    if s.machine.phase() == logic::Phase::Waiting && !s.row_consumed {
        // A stage panel World requested before our row patch is World's own:
        // never adopt (or abandon over) it — wait for our request.
        return;
    }
    let frame = observe(s, &pre, &post, stage);
    for action in s.machine.advance(&frame) {
        if s.abandoned {
            break;
        }
        perform(s, this, &post, stage, action);
    }
    if s.banner.as_ref().is_some_and(|b| !b.applied) {
        apply_banner(s);
    }
}

/// What the machine needs from this update.
fn observe(
    s: &mut Session,
    pre: &shutter::Snapshot,
    post: &shutter::Snapshot,
    stage: i32,
) -> Frame {
    let cutin = s.cutin.as_ref().and_then(|c| {
        bm2d_api::mc_current_frame(c.mc).map(|frame| CutinView {
            frame,
            out: c.out,
            close: c.close,
        })
    });
    // World destroys the root layer (and every MovieClip under it) in its
    // state-8 release — possibly inside this very update. Read nothing from a
    // dead layer: libafp logs every access to an invalid MovieClip id.
    let root = s.root.filter(|r| bm2d_api::layer_id_is_valid(r.layer));
    let root_frame = root.and_then(|r| bm2d_api::mc_current_frame(r.mc));
    let voice_due = root
        .and(s.stage_mc)
        .and_then(bm2d_api::mc_current_frame)
        .is_some_and(|f| f >= s.voice_label);
    let held = [Player::P1, Player::P2]
        .iter()
        .any(|p| input_manager::get_button_state(*p) & button::START != 0);
    let skip_pressed = held && !s.start_held;
    s.start_held = held;
    Frame {
        pre_state: pre.state,
        post_state: post.state,
        pending_is_stage: post.pending_kind == stage,
        active_is_stage: post.active_kind == stage,
        cutin,
        root_frame,
        root_data_release: s.root_data_release,
        voice_due,
        ready_fired: song_reset::intro_cascade_step().unwrap_or(0) >= super::intro_logic::CMA_READY,
        skip_pressed,
        // A ticket that could not be requested has nothing to wait for (the
        // era arm refuses without the choice / shutter ones).
        art_ready: s.tickets.ready(),
    }
}

unsafe fn perform(
    s: &mut Session,
    actor: *mut u8,
    post: &shutter::Snapshot,
    stage: i32,
    action: Action,
) {
    match action {
        Action::Adopt => adopt(s, actor, stage),
        Action::Fill => fill(s, actor),
        Action::SetJacket => set_jacket(s, actor),
        Action::HoldRoot => {
            if let Some(r) = s.root {
                bm2d_api::layer_play_raw(r.layer, 0.0);
                bm2d_api::layer_set_attribute_raw(r.layer, ATTR_VISIBLE, 0);
            }
        }
        Action::ReleaseRoot => {
            if let Some(r) = s.root {
                bm2d_api::mc_op_str(r.mc, MC_OP_GOTO_LABEL_PLAY, c"in");
                bm2d_api::layer_play_raw(r.layer, 1.0);
                bm2d_api::layer_set_attribute_raw(r.layer, ATTR_VISIBLE, ATTR_VISIBLE);
            }
        }
        Action::SkipCutin => {
            if let Some(c) = &s.cutin {
                bm2d_api::mc_op_str(c.mc, MC_OP_GOTO_LABEL_PLAY, c"out");
                if let Some(se) = c.se {
                    super::sound::stop_era_cue(se);
                }
                log_info!("DDR SELECTION: era cut-in skipped");
            }
        }
        Action::DestroyCutin => {
            if let Some(c) = s.cutin.take() {
                let id = c.layer.id();
                if !bm2d_api::destroy_layer(c.layer) {
                    log_warn!("DDR SELECTION: destroying cut-in layer 0x{:08X} failed", id);
                }
            }
        }
        Action::PlayVoice => {
            if let Some(v) = &s.voice {
                let ok = super::sound::play_era_cue(v);
                log_info!(
                    "DDR SELECTION: stage call {}{}",
                    v,
                    if ok { "" } else { " (not played)" }
                );
            }
        }
        Action::FrameOut => {
            if let Some(r) = s.root {
                deep_goto(r.mc, c"frame_out");
                log_info!("DDR SELECTION: legacy stage panel frame_out (song ready)");
            }
        }
        Action::Dismiss => match shutter::send_dismiss(post) {
            Ok(()) => log_info!(
                "DDR SELECTION: legacy stage panel closed at READY (state {} -> 7)",
                post.state
            ),
            Err(e) => {
                if warn_once(W_DISMISS) {
                    log_warn!(
                        "DDR SELECTION: legacy stage panel dismiss refused ({:?})",
                        e
                    );
                }
            }
        },
        Action::UnblockDrain => {
            let u = shutter::unblock_drain(actor);
            log_warn!(
                "DDR SELECTION: legacy stage panel drain stalled -- unblocked ({:?})",
                u
            );
        }
        Action::Finished => {
            log_info!("DDR SELECTION: legacy stage panel released by World");
            set_root_live(false);
            // The root and its MovieClips died with World's release.
            s.root = None;
            s.stage_mc = None;
            retire(s);
        }
    }
}

/// World's pending stage clip exists (state 1 → 2): keep it only if it is
/// A3's root, and take it over.
unsafe fn adopt(s: &mut Session, actor: *mut u8, stage: i32) {
    let Some(clip) = shutter::kind_clip(actor, stage) else {
        return abandon(s, "no pending stage clip");
    };
    // Only A3's root has this placeholder.
    if bm2d_api::layer_find_child(clip.layer, "choice_stage_usr2").is_none() {
        return abandon(s, "the stage root is not shutter_choice_hd_root");
    }
    s.root = Some(clip);
    s.root_layer_for_release = clip.layer;
    bm2d_api::layer_set_priority_raw(clip.layer, logic::clayer_priority(logic::ROOT_PRIORITY));
    s.root_data_release = bm2d_api::mc_frame_by_label(clip.mc, c"data_release").filter(|&f| f > 0);
    // The era fill loads the era's stage clip into `choice_stage_usr2`; A3's
    // skin-0 fill keeps the root's own `choice_stage_usr` and hides `…2`.
    let stage_placeholder = match s.variant {
        logic::Variant::Era => "choice_stage_usr",
        logic::Variant::Theme => "choice_stage_usr2",
    };
    for path in [
        stage_placeholder,
        "caution_usr",
        "fullcombo_challenge_usr",
        "p1_score_set_mc",
        "p2_score_set_mc",
    ] {
        set_visible(clip.layer, path, false);
    }
    log_info!(
        "DDR SELECTION: A3's stage root adopted (skin {}, layer 0x{:08X}, armed at {}{})",
        s.skin,
        clip.layer,
        s.origin,
        if JACKET_CALL_STOCK.get().is_some() {
            "; old layout: World's jacket_usr SetVisible NOPed at the swap"
        } else {
            ""
        }
    );
    set_root_live(true);
}

/// A3's legacy fill (`FUN_180030d10`, legacy branch) on the adopted root,
/// plus the cut-in start (A3 state 1).
unsafe fn fill(s: &mut Session, actor: *mut u8) {
    let Some(clip) = s.root else {
        return;
    };
    if s.variant == logic::Variant::Theme {
        return fill_theme(s, clip);
    }
    let ctx = stage_ctx();
    let choice_pkg = s.tickets.choice.as_ref().and_then(package_id);
    let shutter_pkg = s.tickets.shutter.as_ref().and_then(package_id);
    let mut loaded = Vec::new();
    if let Some(pkg) = choice_pkg {
        if load_movie(clip.layer, "choice_stage_usr2", pkg, "choice_stage") {
            loaded.push("choice_stage");
        }
        set_texture(
            clip.layer,
            "choice_stage_usr2/scene_choice_stage_usr",
            &logic::stage_texture(s.skin, &ctx),
        );
    }
    if let Some(pkg) = shutter_pkg {
        if load_movie(
            clip.layer,
            "choice_background_usr",
            pkg,
            "choice_background",
        ) {
            loaded.push("choice_background");
        }
    }
    let basename = sites().and_then(|st| read_msvc_string(actor.add(st.basename_off)));
    let has_banner = s.skin == 3
        && basename.as_deref().is_some_and(|b| {
            CString::new(logic::banner_stem(b))
                .is_ok_and(|c| super::package_helper::probe_arc(BANNER_DIR, &c))
        });
    s.jacket = logic::jacket(s.skin, has_banner);
    match s.jacket {
        Jacket::Hidden => {
            set_visible(clip.layer, "choice_jacket_usr", false);
            if let Some(mc) = bm2d_api::layer_find_child(clip.layer, "choice_jacket_usr") {
                deep_pause(mc, 0);
            }
        }
        Jacket::Song | Jacket::Banner => {
            if let Some(pkg) = shutter_pkg {
                if load_movie(clip.layer, "choice_jacket_usr", pkg, "choice_jacket") {
                    loaded.push("choice_jacket");
                }
            }
        }
    }
    if s.jacket == Jacket::Banner {
        let stem = basename
            .as_deref()
            .map(logic::banner_stem)
            .unwrap_or_default();
        match asset_loader::load(&format!("data/arc/banner/{stem}.arc"), &stem) {
            Some(handle) => {
                s.banner = Some(Banner {
                    stem,
                    handle: Some(handle),
                    applied: false,
                });
            }
            // Could not load it: the song jacket instead (set at the swap).
            None => s.jacket = Jacket::Song,
        }
    }
    // Re-resolve after the movie load replaced the placeholder's content.
    s.stage_mc = bm2d_api::layer_find_child(clip.layer, "choice_stage_usr2");
    s.voice_label = s
        .stage_mc
        .and_then(|mc| bm2d_api::mc_frame_by_label(mc, c"voice"))
        .unwrap_or(0);
    s.voice = logic::stage_voice(s.skin, &ctx);
    if choice_pkg.is_none() || shutter_pkg.is_none() {
        if warn_once(W_MOVIE) {
            log_warn!("DDR SELECTION: an era panel package was not ready at the fill -- its placeholders stay");
        }
    }
    let cutin = if !s.cutin_enabled {
        ", era cut-in off (setting)"
    } else if start_cutin(s) {
        ", era cut-in"
    } else {
        ", no cut-in"
    };
    log_info!(
        "DDR SELECTION: legacy stage panel filled (skin {}, stage {} -> {}, jacket {:?}, voice {}, loaded [{}]{})",
        s.skin,
        ctx.stage,
        logic::stage_texture(s.skin, &ctx),
        s.jacket,
        s.voice.as_deref().unwrap_or("none"),
        loaded.join(", "),
        cutin
    );
}

/// A3's skin-0 fill (`FUN_180030d10`, skin-0 branch) on the adopted theme
/// root: the band texture on the root's own `choice_stage_usr`, the root's
/// own background and jacket frame (the song jacket goes in at the swap),
/// the stage call at once (A3 played it without waiting for a label).
fn fill_theme(s: &mut Session, clip: shutter::KindClip) {
    let ctx = stage_ctx();
    let band = logic::theme_stage_texture(&ctx);
    set_texture(clip.layer, "choice_stage_usr/scene_choice_stage_usr", &band);
    s.jacket = Jacket::Song;
    s.stage_mc = bm2d_api::layer_find_child(clip.layer, "choice_stage_usr").or(Some(clip.mc));
    s.voice_label = 0;
    s.voice = logic::stage_voice(s.skin, &ctx);
    log_info!(
        "DDR SELECTION: A3 stage panel filled (skin {}, {}, stage {} -> {}{}, voice {})",
        s.skin,
        s.root_pkg.to_string_lossy(),
        ctx.stage,
        band,
        if logic::special_stage(&ctx) {
            " (special stage)"
        } else {
            ""
        },
        s.voice.as_deref().unwrap_or("none")
    );
    fill_score_sets(s, clip.layer, ctx.stage);
}

/// A3 `FUN_180032240` for both sides (`score_set.rs` reads World's records,
/// `score_set_logic.rs` decides): a package not ready by now hides the
/// fields it carries.
fn fill_score_sets(s: &Session, layer: u32, stage: i32) {
    let ready = |t: &Option<LoadTicket>| t.as_ref().is_some_and(bm2d_package::is_ready);
    let ctx =
        super::score_set::FillCtx::new(stage, ready(&s.tickets.texture), ready(&s.tickets.area));
    let mut summary = Vec::with_capacity(2);
    for side in 0..2 {
        let inputs = super::score_set::side_inputs(side, &ctx);
        for w in score_set_logic::fill_side(&inputs) {
            if let Some(t) = &w.texture {
                set_texture(layer, &w.path, t);
            }
            set_visible(layer, &w.path, w.visible);
        }
        summary.push(super::score_set::describe(&inputs));
    }
    log_info!(
        "DDR SELECTION: theme score sets filled ({}{}{})",
        summary.join("; "),
        if ctx.glyphs_ready {
            ""
        } else {
            "; glyphs not ready"
        },
        if ctx.area_ready { "" } else { "; no area" }
    );
}

fn abandon(s: &mut Session, why: &str) {
    s.machine.abandon();
    s.abandoned = true;
    // World's own panel is on screen after all: its stage voice, READY
    // dismissal and dwell stay World's.
    set_root_live(false);
    if warn_once(W_FOREIGN) {
        log_warn!("DDR SELECTION: {why} -- legacy stage panel skipped for this song");
    }
}

/// A3 state 1: the cut-in over the covered screen, with its SE.
fn start_cutin(s: &mut Session) -> bool {
    let Some(pkg) = s.tickets.cutin.as_ref().and_then(package_id) else {
        return false;
    };
    if !s
        .tickets
        .cutin_bg
        .as_ref()
        .is_some_and(bm2d_package::is_ready)
    {
        return false;
    }
    let Some(layer) = bm2d_api::create_layer_from_package(pkg, "choice_cutin") else {
        return false;
    };
    bm2d_api::layer_set_attribute(&layer, ATTR_DISPLAY_SETUP, ATTR_DISPLAY_SETUP);
    bm2d_api::layer_set_group(&layer, CUTIN_GROUP);
    bm2d_api::layer_set_priority(&layer, logic::clayer_priority(logic::CUTIN_PRIORITY));
    let Some(mc) = bm2d_api::layer_find_child(layer.id(), "/") else {
        let _ = bm2d_api::destroy_layer(layer);
        return false;
    };
    bm2d_api::mc_op_str(mc, MC_OP_GOTO_LABEL_PLAY, c"in");
    bm2d_api::layer_play(&layer, 1.0);
    bm2d_api::layer_set_visible(&layer, true);
    let out = bm2d_api::mc_frame_by_label(mc, c"out").filter(|&f| f > 0);
    let close = bm2d_api::mc_frame_by_label(mc, c"close").filter(|&f| f > 0);
    let se = logic::cutin_se(s.skin);
    if let Some(cue) = se {
        if !super::sound::play_era_cue(cue) {
            log_info!("DDR SELECTION: cut-in SE {cue} not played");
        }
    }
    s.cutin = Some(Cutin {
        layer,
        mc,
        out,
        close,
        se,
    });
    true
}

/// A3 state 4: the jacket frame's texture (skins 4–5, 3 without a banner).
unsafe fn set_jacket(s: &mut Session, actor: *mut u8) {
    if s.jacket != Jacket::Song {
        return;
    }
    let Some(r) = s.root else {
        return;
    };
    if let Some(name) = sites().and_then(|st| read_msvc_string(actor.add(st.jacket_off))) {
        set_texture(
            r.layer,
            "choice_jacket_usr/jacket_root_usr/jacket_usr",
            &name,
        );
    }
}

/// Skin 3: the SuperNOVA 2 banner once the FileManager registered it.
fn apply_banner(s: &mut Session) {
    let Some(r) = s.root else {
        return;
    };
    let Some(b) = s.banner.as_mut() else {
        return;
    };
    if b.handle.is_none() || asset_loader::resolve(&b.stem).is_none() {
        return;
    }
    set_texture(
        r.layer,
        "choice_jacket_usr/jacket_root_usr/jacket_usr",
        &b.stem,
    );
    b.applied = true;
    log_info!(
        "DDR SELECTION: SuperNOVA 2 banner {} on the stage panel",
        b.stem
    );
}

fn package_id(t: &LoadTicket) -> Option<u32> {
    bm2d_package::lookup(t).map(|h| h.afpu_package_id())
}

fn sites() -> Option<&'static DdrSelPanelSites> {
    SITES.get().map(|s| &s.0)
}

fn stage_ctx() -> StageCtx {
    StageCtx {
        stage: stage_records::stage_counter().unwrap_or(0),
        max_stage: stage_records::max_stage_setting().unwrap_or(2),
        override_stage: stage_records::final_stage_override().unwrap_or(-1),
        course: false,
        event_mode: stage_records::event_mode().unwrap_or(0),
    }
}

/// Every instance of `path` under `layer`.
fn instances(layer: u32, path: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let mut next = bm2d_api::layer_find_child(layer, path);
    while let Some(mc) = next {
        if out.len() >= 16 || out.contains(&mc) {
            break;
        }
        out.push(mc);
        next = bm2d_api::mc_traversal(mc, TRAVERSE_INSTANCE);
    }
    out
}

fn children(mc: u32) -> Vec<u32> {
    let mut out = Vec::new();
    let mut next = bm2d_api::mc_traversal(mc, TRAVERSE_CHILD);
    while let Some(c) = next {
        if out.len() >= 256 || out.contains(&c) {
            break;
        }
        out.push(c);
        next = bm2d_api::mc_traversal(c, TRAVERSE_SIBLING);
    }
    out
}

fn set_visible(layer: u32, path: &str, visible: bool) {
    for mc in instances(layer, path) {
        bm2d_api::mc_set_param(mc, MC_PARAM_VISIBLE, visible as i32);
        bm2d_api::mc_set_param(mc, MC_PARAM_DIRTY, 1);
    }
}

fn set_texture(layer: u32, path: &str, texture: &str) {
    for mc in instances(layer, path) {
        bm2d_api::mc_load_bitmap(mc, texture);
    }
}

fn load_movie(layer: u32, path: &str, package: u32, template: &str) -> bool {
    let mut ok = false;
    for mc in instances(layer, path) {
        ok |= bm2d_api::mc_load_movie(mc, package, template);
    }
    ok
}

/// `BM2D::CMovieClip::SetFrameLabel`: the label on the MC and each direct
/// child (World `FUN_1801ae160`, A3 `FUN_180100310`).
fn deep_goto(mc: u32, label: &CStr) {
    bm2d_api::mc_op_str(mc, MC_OP_GOTO_LABEL, label);
    for c in children(mc) {
        bm2d_api::mc_op_str(c, MC_OP_GOTO_LABEL, label);
    }
}

/// `BM2D::CMovieClip::Pause(true)`: stop the MC and every descendant at its
/// current frame (World `FUN_180258fd0`, A3 `FUN_1801b7e40`).
fn deep_pause(mc: u32, depth: u32) {
    if depth > 8 {
        return;
    }
    if let Some(f) = bm2d_api::mc_current_frame(mc) {
        bm2d_api::mc_op(mc, MC_OP_GOTO_STOP, f as i32);
    }
    for c in children(mc) {
        deep_pause(c, depth + 1);
    }
}

/// A3 had no READY? dwell (its panel held itself): seed World's timer during
/// the DancePlaySequence's pre-song steps while the legacy panel is hosted.
fn seed_dwell() {
    let off = DWELL_TIMER_OFF.load(Ordering::Acquire);
    if off == 0 {
        return;
    }
    let (Some(step), Some(dps)) = (song_reset::dps_step(), song_reset::live_dps()) else {
        return;
    };
    if !super::intro_logic::seeds_dwell(step) {
        return;
    }
    let field = unsafe { dps.add(off) };
    if memory::is_readable(field, 4) {
        unsafe { memory::write_f32(field, DWELL_SEED) };
    }
}

/// Release queued packages once World's root layer is gone (+ a grace).
fn poll_releases(actor: *mut u8) {
    let stage = shutter::stage_kind().unwrap_or(-1);
    let stage_layer = shutter::kind_clip(actor, stage).map(|c| c.layer);
    let mut queue = lock(&RELEASES);
    let mut i = 0;
    while i < queue.len() {
        let e = &mut queue[i];
        let alive = e.root_layer != 0
            && (stage_layer == Some(e.root_layer) || bm2d_api::layer_id_is_valid(e.root_layer));
        if alive {
            e.frames = 0;
            i += 1;
            continue;
        }
        e.frames += 1;
        if e.frames < RELEASE_GRACE_FRAMES {
            i += 1;
            continue;
        }
        let e = queue.swap_remove(i);
        let names: Vec<String> = e.tickets.iter().map(|t| t.name().to_string()).collect();
        for t in e.tickets {
            bm2d_package::release(t);
        }
        if let Some(h) = e.banner {
            asset_loader::release(h);
        }
        if !names.is_empty() {
            log_info!(
                "DDR SELECTION: stage-panel packages released [{}]",
                names.join(", ")
            );
        }
    }
    if queue.is_empty() {
        RELEASE_PENDING.store(false, Ordering::Release);
    }
}

/// An MSVC `std::string` at `addr` (SSO 16), or `None`.
unsafe fn read_msvc_string(addr: *const u8) -> Option<String> {
    if !memory::is_readable(addr, 0x20) {
        return None;
    }
    let len = memory::read_u64(addr.add(0x10)) as usize;
    let cap = memory::read_u64(addr.add(0x18)) as usize;
    if len == 0 || len > cap || cap > 0x100 {
        return None;
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
