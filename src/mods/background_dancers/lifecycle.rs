//! The per-song lifecycle (design §3.2 / §4.3.1): tables at enable, the
//! pick at the first entry into the song window {26, 27, 28}, the parse
//! thread + residency-gated build, the visibility/clock gates, the per-frame
//! director call, and the teardown at window exit — the Step 3/4 spike's
//! cabinet-proven teardown state machine (disable + hide → item list drops
//! every item for 2 frames → queue destroys → dtors → node blocks → arcs;
//! 5 s caps ⇒ leak + WARN) generalised over a [`Session`]. Since 2026-09-21
//! the load / build / teardown machinery itself lives in
//! [`scene_window::SceneWindow`] (shared with the options previews); this
//! file is the GAMEPLAY wrapper — pick, clock, tempo map, 2D hide, movie
//! size / Background Movies mode (`movie_mode.rs`), camera slot 0.
//!
//! Visibility + clock (FR-8/FR-9), evaluated every frame from live state
//! rather than a phase ladder so quick restarts (fresh DPS), in-place
//! `song_reset`s and training scrubs all fall out of three rules:
//! - graph disabled (DPS before step 5) ⇒ everything hidden, `t = 0`;
//! - graph enabled but not anchored ⇒ the scene shows its `t = 0` pose
//!   (A3: everything appears on the start edge) — or, once a run WAS
//!   anchored, holds its last `t` (A3: dancers keep going through the tail);
//! - anchored ⇒ `t = (count − t0) / 1000`, `t0` latched ONCE on the anchor
//!   edge; a count jump back (training rewind / loop / in-place restart)
//!   moves the timeline back — never re-latches (an in-place restart lands
//!   at the song-start count ⇒ `t ≈ 0`).
//!
//! Engine calls happen on the game thread only: the scene callback
//! schedules load/teardown through `run_on_render_thread`, the per-frame
//! work already runs on the game thread; the parse thread never touches the
//! engine. Every path is panic-free.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::anm::rlist;
use crate::core::arc as arcfile;
use crate::services::movie_policy::{self, MovieSuppressor};
use crate::services::scene3d::{arc_set, frame_board, scene_graph};
use crate::services::{song_reset, stage_records, widget_renderer};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use super::background_hide;
use super::clock::{Clock, ClockEvent};
use super::director;
use super::movie_backdrop;
use super::movie_camera;
use super::movie_mode::{
    self, Backdrop, Capabilities, MovieMode, SceneMask, ScreenFilter, SongMovie,
};
use super::movie_size;
use super::scene_window::{self, ScenePhase, SceneWindow};
use super::screen_route;
use super::selection::{
    apply_pin, dancer_candidates, parse_pin, random_stage_pool, seed_from, stage_candidates,
    DancerCandidate, PickSource, Pin, Rng, StageCandidate, StagePool,
};
use super::session::{assemble_pick, make_pick, CameraSet, ParseOptions, Pick, Session};
use super::tempo::{TempoMap, TempoOptions};
use super::tempo_source;

/// The music count the scene is posed at before the run anchors (a few
/// frames between the DPS step-5 edge and the `0x1044` anchor): close to
/// the first anchored count (≈ −276 ms — the anchor is future-dated by the
/// lead) so the stage loops do not visibly jump when the count takes over.
const PRE_SONG_MC_MS: i32 = -300;

const STARTUP_ARC: &str = "data/arc/startup.arc";
const MAP_RLIST: &str = "data/map/map_resources.rlist";
const CAMERA_RLIST: &str = "data/camera/stage_camera_resources.rlist";
const CHARA_RLIST: &str = "data/chara/chara_resources.rlist";

// ---------------------------------------------------------------------------
// Tables (enable time)
// ---------------------------------------------------------------------------

struct Tables {
    stages: Vec<StageCandidate>,
    camera_rows: Vec<(String, Vec<String>)>,
    dancers: Vec<DancerCandidate>,
    pin: Option<Pin>,
    /// `(key, label)` of every CUSTOM entry (`data_mods/custom_models`) — the catalog
    /// appends these after the stock block with their folder-derived names.
    custom_labels: Vec<(String, String)>,
    /// Stage keys whose arc carries video screens (an `offscreen1.dds`
    /// member — `movie_mode::arc_members_have_screen`), for Background
    /// Movies = STAGE SCREENS. Built once at enable, stock + custom +
    /// LayeredFS overrides alike.
    screen_stages: HashSet<String>,
}

static TABLES: Mutex<Option<Tables>> = Mutex::new(None);
static TABLES_READY: AtomicBool = AtomicBool::new(false);

/// Load the four `startup.arc` rlists, build the candidate tables (arc
/// existence through the LayeredFS-aware resolver), append the custom
/// dancers/stages discovered under `data_mods/custom_models` when
/// `background_dancers.custom_content` is on, read the developer pin.
/// `false` + WARN when nothing can be shown.
pub fn init_tables() -> bool {
    let Some(data) = arc_set::read_bytes(STARTUP_ARC) else {
        log_warn!(
            "BackgroundDancers: {} unreadable -- mod inactive",
            STARTUP_ARC
        );
        return false;
    };
    let Some(entries) = arcfile::parse(&data) else {
        log_warn!(
            "BackgroundDancers: {} is not an arc -- mod inactive",
            STARTUP_ARC
        );
        return false;
    };
    let read_rlist = |member: &str| -> Option<Vec<(String, Vec<String>)>> {
        let e = entries.iter().find(|e| e.path == member)?;
        let bytes = arcfile::extract(&data, e)?;
        match rlist::parse(&bytes) {
            Ok(rows) => Some(rows),
            Err(err) => {
                log_warn!("BackgroundDancers: {}: {}", member, err);
                None
            }
        }
    };
    let (Some(map_rows), Some(camera_rows), Some(chara_rows)) = (
        read_rlist(MAP_RLIST),
        read_rlist(CAMERA_RLIST),
        read_rlist(CHARA_RLIST),
    ) else {
        log_warn!("BackgroundDancers: an rlist is missing from startup.arc -- mod inactive");
        return false;
    };
    let exists = |arc: &str| arc_set::resolve_path(&format!("data/arc/{arc}")).is_some();
    let stages = stage_candidates(&map_rows, exists);
    let dancers = dancer_candidates(&chara_rows, exists);
    if stages.is_empty() || dancers.is_empty() {
        log_warn!(
            "BackgroundDancers: no {} candidate (mapset_*/pl_* arcs missing from the install) -- mod inactive",
            if stages.is_empty() { "stage" } else { "dancer" }
        );
        return false;
    }
    let bd = crate::mods::config::get()
        .and_then(|c| c.background_dancers.clone())
        .unwrap_or_default();
    // Custom dancers / stages from `data_mods/custom_models` (design
    // 2026-09-22): appended AFTER the stock rows so stock row indices and
    // option values never move; the stage camera rows stay row-parallel.
    let (stages, camera_rows, dancers, custom_labels) = if bd.custom_content {
        let default_camera_row = stages
            .first()
            .and_then(|s| camera_rows.get(s.row))
            .map(|(_, f)| f.clone())
            .unwrap_or_default();
        let stock = super::custom_content::StockContext {
            stage_keys: stages.iter().map(|s| s.key.clone()).collect(),
            dancer_keys: dancers.iter().map(|d| d.key.clone()).collect(),
            next_stage_row: map_rows.len(),
            next_dancer_row: chara_rows.len(),
            default_camera_row,
        };
        let plan = super::custom_scan::discover_and_mount(&stock);
        let mut stages = stages;
        let mut dancers = dancers;
        let mut camera_rows = camera_rows;
        for (row, names) in plan.camera_rows {
            if camera_rows.len() <= row {
                camera_rows.resize(row + 1, (String::new(), Vec::new()));
            }
            let key = plan
                .stages
                .iter()
                .find(|s| s.row == row)
                .map(|s| s.key.clone())
                .unwrap_or_default();
            camera_rows[row] = (key, names);
        }
        stages.extend(plan.stages);
        dancers.extend(plan.dancers);
        (stages, camera_rows, dancers, plan.labels)
    } else {
        log_info!(
            "BackgroundDancers: custom content OFF -- data_mods/custom_models is not scanned"
        );
        (stages, camera_rows, dancers, Vec::new())
    };
    let distinct = super::selection::distinct_stage_keys(&stages).len();
    let pin = read_pin();
    let dev_mode = crate::mods::config::get()
        .and_then(|c| c.layeredfs.as_ref())
        .map(|l| l.developer_mode)
        .unwrap_or(false);
    let static_poses = dev_mode && std::env::var_os("DDR_DANCERS_STATIC").is_some();
    STATIC_POSES.store(static_poses, Ordering::Release);
    log_info!(
        "BackgroundDancers: scene clock -- bpm_sync={} (dance at chart BPM/120, beat-phase pinned) stop_slow={} (1/12 speed below 10 BPM)",
        bd.bpm_sync,
        bd.stop_slow
    );
    if static_poses {
        log_warn!(
            "BackgroundDancers: DDR_DANCERS_STATIC -- poses published once per song (bisect mode)"
        );
    }
    log_info!(
        "BackgroundDancers: tables ready -- {} stage rows ({} distinct stages), {} dancers, {} camera rows, {} custom{}",
        stages.len(),
        distinct,
        dancers.len(),
        camera_rows.len(),
        custom_labels.len(),
        match &pin {
            Some(p) => format!(" (DDR_DANCERS_PIN honoured: {:?})", p),
            None => String::new(),
        }
    );
    let screen_stages = scan_screen_stages(&stages);
    if let Ok(mut t) = TABLES.lock() {
        *t = Some(Tables {
            stages,
            camera_rows,
            dancers,
            pin,
            custom_labels,
            screen_stages,
        });
    }
    TABLES_READY.store(true, Ordering::Release);
    true
}

pub fn tables_ready() -> bool {
    TABLES_READY.load(Ordering::Acquire)
}

/// Which distinct stages have video screens (design R5): the stage arc — as
/// the engine will load it (mounts / LayeredFS / stock, `arc_set`) — lists
/// an `offscreen1.dds` member. One header read per distinct key at enable
/// (a 64 KiB prefix). An unreadable arc counts as "no screens" and is named
/// in the INFO.
fn scan_screen_stages(stages: &[StageCandidate]) -> HashSet<String> {
    let keys = super::selection::distinct_stage_keys(stages);
    let mut with = HashSet::new();
    let mut unreadable: Vec<&str> = Vec::new();
    for key in &keys {
        let members = arc_set::resolve_path(&format!("data/arc/mapset_{key}.arc"))
            .and_then(|path| super::custom_scan::read_arc_members(&path));
        match members {
            Some(m) => {
                if movie_mode::arc_members_have_screen(&m) {
                    with.insert((*key).to_string());
                }
            }
            None => unreadable.push(*key),
        }
    }
    let mut listed: Vec<&str> = with.iter().map(String::as_str).collect();
    listed.sort_unstable();
    log_info!(
        "BackgroundDancers: stages with screens: {} ({} of {}){}",
        if listed.is_empty() {
            "none".to_string()
        } else {
            listed.join(", ")
        },
        listed.len(),
        keys.len(),
        if unreadable.is_empty() {
            String::new()
        } else {
            format!(
                " -- arc header unreadable (counted as no screens): {}",
                unreadable.join(", ")
            )
        }
    );
    with
}

/// Whether the stage `key` has video screens (see [`scan_screen_stages`]).
fn stage_has_screens(key: &str) -> bool {
    TABLES
        .lock()
        .ok()
        .and_then(|t| t.as_ref().map(|t| t.screen_stages.contains(key)))
        .unwrap_or(false)
}

/// `(key, label)` of the custom (`data_mods/custom_models`) entries in the tables — empty
/// with the toggle off or nothing installed.
pub(super) fn custom_labels_snapshot() -> Vec<(String, String)> {
    TABLES
        .lock()
        .ok()
        .and_then(|t| t.as_ref().map(|t| t.custom_labels.clone()))
        .unwrap_or_default()
}

/// A copy of the candidate tables `(stages, camera_rows, dancers)` for the
/// option-row catalog and the preview scene builder; `None` before
/// [`init_tables`] succeeded.
pub(super) fn tables_snapshot() -> Option<(
    Vec<StageCandidate>,
    Vec<(String, Vec<String>)>,
    Vec<DancerCandidate>,
)> {
    let tables = TABLES.lock().ok()?;
    let t = tables.as_ref()?;
    Some((t.stages.clone(), t.camera_rows.clone(), t.dancers.clone()))
}

/// `layeredfs.developer_mode` (the dev-only log gate).
fn dev_mode() -> bool {
    crate::mods::config::get()
        .and_then(|c| c.layeredfs.as_ref())
        .map(|l| l.developer_mode)
        .unwrap_or(false)
}

/// `DDR_DANCERS_PIN`, developer_mode-gated (design FR-15).
fn read_pin() -> Option<Pin> {
    let raw = std::env::var("DDR_DANCERS_PIN").ok()?;
    let pin = parse_pin(&raw)?;
    let dev_mode = crate::mods::config::get()
        .and_then(|c| c.layeredfs.as_ref())
        .map(|l| l.developer_mode)
        .unwrap_or(false);
    if !dev_mode {
        log_warn!(
            "BackgroundDancers: DDR_DANCERS_PIN set but layeredfs.developer_mode is off -- pin ignored"
        );
        return None;
    }
    Some(pin)
}

// ---------------------------------------------------------------------------
// Window state
// ---------------------------------------------------------------------------

/// The gameplay window: the shared [`SceneWindow`] (assets, session,
/// teardown) plus what only a song needs — the music-count clock, the tempo
/// map, the one-shot logs, the camera / hide / static-pose latches.
struct Window {
    generation: u64,
    scene: SceneWindow,
    clock: Clock,
    /// The live song's tempo map (dance time from the music count) and the
    /// DPS instance / basename it belongs to; `None` ⇒ real time.
    tempo: Option<TempoMap>,
    tempo_dps: usize,
    tempo_basename: String,
    tempo_rx: Option<tempo_source::TempoSlot>,
    tempo_failed_logged: bool,
    // one-shot logs
    visible_logged: bool,
    playing_logged: u32,
    rewind_logged: bool,
    /// The camera source of the last frame (`None` before the first).
    camera_source: Option<CamSource>,
    hide_armed: bool,
    static_published: bool,
    /// Background Movies, latched at window entry (what the movie-size
    /// writes and the suppressor were set up for).
    movie_mode: MovieMode,
    /// The live song's movie state (re-probed every frame in FULLSCREEN and
    /// MOVIE ONLY — `movie_mode::probes_backdrop`; always `None` otherwise).
    backdrop: Backdrop,
}

struct State {
    generation: u64,
    live: Option<Window>,
    /// A previous window's scene still tearing down when a new one opened
    /// (only ever driven to its end).
    orphan: Option<SceneWindow>,
}

static STATE: Mutex<State> = Mutex::new(State {
    generation: 0,
    live: None,
    orphan: None,
});

/// A song window is open.
static IN_WINDOW: AtomicBool = AtomicBool::new(false);
/// The movie-size values the open window overrode (restored at exit).
static MOVIE_SIZE_SAVED: Mutex<[Option<movie_size::Saved>; 2]> = Mutex::new([None, None]);
/// The Background Movies mode the open window latched (`MovieMode::row_value`).
static WINDOW_MOVIE_MODE: AtomicU8 = AtomicU8::new(1);
/// The window set the `BackgroundDancers` BuildGraph suppressor (OFF mode).
static MOVIE_SUPPRESSED: AtomicBool = AtomicBool::new(false);
/// Mode-unavailable WARNs already emitted this boot (bit = `row_value`).
static DEGRADE_WARNED: AtomicU8 = AtomicU8::new(0);
/// The per-frame driver has work (O(1) when clear).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// `stage_records` unavailable WARN — once per boot.
static RECORDS_WARNED: AtomicBool = AtomicBool::new(false);
/// Dev bisect knob `DDR_DANCERS_STATIC` (developer_mode): publish every
/// instance ONCE (its t = 0 pose) and never again — isolates the per-frame
/// item writes from everything else.
static STATIC_POSES: AtomicBool = AtomicBool::new(false);
/// The two A3 ConfigBank switches (`background_dancers.{bpm_sync,
/// stop_slow}`) — live values owned by `style.rs` (mod-menu rows, next song).
fn tempo_options() -> TempoOptions {
    super::style::tempo_options()
}

fn in_song_window(s: i32) -> bool {
    matches!(
        s,
        scene::SONG_TO_STAGE_INTERSTITIAL | scene::STAGE_INDICATOR | scene::GAMEPLAY
    )
}

/// Scene callback (game thread, fires before the game's `createNextSequence`).
pub fn on_scene_change(prev: i32, next: i32) {
    if in_song_window(next) && !in_song_window(prev) {
        if IN_WINDOW.swap(true, Ordering::AcqRel) {
            return;
        }
        window_entry(next);
    } else if in_song_window(prev)
        && !in_song_window(next)
        && IN_WINDOW.swap(false, Ordering::AcqRel)
    {
        // Movie size + suppressor back first — synchronously, on the game
        // thread, before anything downstream (SONG_SELECT's row re-seed, the
        // logout save, a results-screen movie) can observe the override.
        restore_movie_mode();
        let window_gen = STATE.lock().map(|st| st.generation).unwrap_or(0);
        widget_renderer::run_on_render_thread(move || begin_teardown(window_gen));
    }
}

/// What this boot can honour (each mode's dependencies).
fn capabilities() -> Capabilities {
    Capabilities {
        movie_size: movie_size::is_available(),
        probe: movie_backdrop::is_available(),
        route: screen_route::is_available(),
    }
}

/// The mode this boot can honour (`movie_mode::degrade`): FULLSCREEN needs
/// the movie-size write (an ON player would otherwise keep a thumbnail over
/// a stage-less scene) and the movie probe; STAGE SCREENS also the screen
/// route. A miss degrades to THUMBNAIL (one WARN per boot per mode).
fn effective_movie_mode(requested: MovieMode) -> MovieMode {
    let caps = capabilities();
    let mode = movie_mode::degrade(requested, caps);
    if mode != requested {
        let bit = 1u8 << (requested.row_value() as u32 & 7);
        if DEGRADE_WARNED.fetch_or(bit, Ordering::AcqRel) & bit == 0 {
            let ok = |b: bool| if b { "ok" } else { "MISSING" };
            log_warn!(
                "BackgroundDancers: Background Movies = {} unavailable this boot (movie-size override {}, movie probe {}, screen route {}) -- {} instead",
                requested.label(),
                ok(caps.movie_size),
                ok(caps.probe),
                ok(caps.route),
                mode.label()
            );
        }
    }
    mode
}

/// The Background Movies mode the open window latched.
fn window_movie_mode() -> MovieMode {
    MovieMode::from_row_value(WINDOW_MOVIE_MODE.load(Ordering::Acquire) as i32)
}

/// Window entry (game thread, before `createNextSequence`): latch the
/// Background Movies mode and set the song up for it — every entered side's
/// movie size per `movie_mode::size_override` (OFF → 3, THUMBNAIL → 2,
/// FULLSCREEN / routed STAGE SCREENS → 1), plus, for OFF, the shared
/// BuildGraph suppressor (the backstop for builds where 3 still builds a
/// graph). STAGE SCREENS runs only when the picked stage `has_screens`
/// (else THUMBNAIL — `movie_mode::window_mode`), and the screen route is
/// armed BEFORE any movie-size write: a refused arm makes the song
/// THUMBNAIL first, so a song can never end up with a fullscreen-size movie
/// hidden under a full stage. Returns the latched (window) mode.
fn apply_movie_mode(has_screens: bool) -> MovieMode {
    let requested = super::style::movie_mode();
    let entered = [
        stage_records::side_entered(0).unwrap_or(false),
        stage_records::side_entered(1).unwrap_or(false),
    ];
    let mut mode = movie_mode::window_mode(effective_movie_mode(requested), has_screens);
    let mut routed = false;
    let mut route_note = "";
    if movie_mode::routes_to_screens(mode) {
        if !movie_size::any_shows_movie(entered) {
            // Nobody shows a movie: nothing to route (and 20250805 would
            // still build a MovieActor for VIDEO SIZE OFF).
            mode = MovieMode::Thumbnail;
            route_note = " (no entered side shows a movie)";
        } else if screen_route::arm() {
            routed = true;
        } else {
            mode = MovieMode::Thumbnail;
            route_note = " (layer-select byte refused)";
        }
    }
    let screens_note = if requested == MovieMode::StageScreens {
        format!(
            " -- stage screens: {}, routed: {}{}",
            if has_screens { "yes" } else { "no" },
            if routed { "yes" } else { "no" },
            route_note
        )
    } else {
        String::new()
    };
    WINDOW_MOVIE_MODE.store(mode.row_value() as u8, Ordering::Release);
    if mode == MovieMode::Off && movie_policy::is_available() {
        movie_policy::set_suppressed(MovieSuppressor::BackgroundDancers, true);
        MOVIE_SUPPRESSED.store(true, Ordering::Release);
    }
    if !movie_size::is_available() {
        log_info!(
            "BackgroundDancers: background movies {} for the song{} (movie-size override unavailable -- VIDEO SIZE as configured{})",
            mode.key(),
            screens_note,
            if MOVIE_SUPPRESSED.load(Ordering::Acquire) {
                ", movie graphs suppressed"
            } else {
                ""
            }
        );
        return mode;
    }
    let saved = movie_size::apply(entered, mode);
    let fmt = |s: &Option<movie_size::Saved>| match s {
        Some(s) => format!("{} -> {}", s.original, s.written),
        None => "kept".to_string(),
    };
    log_info!(
        "BackgroundDancers: background movies {} for the song{} -- movie size P1 {} P2 {}{} (restored at window exit)",
        mode.key(),
        screens_note,
        fmt(&saved[0]),
        fmt(&saved[1]),
        if MOVIE_SUPPRESSED.load(Ordering::Acquire) {
            ", movie graphs suppressed"
        } else {
            ""
        }
    );
    if let Ok(mut g) = MOVIE_SIZE_SAVED.lock() {
        *g = saved;
    }
    mode
}

/// Background Movies = FULLSCREEN: the movie camera set for `dancers`
/// dancers (`movie_camera.rs`), split + shuffled like a stage row. Listed
/// per window (a dozen directory entries) so an operator's edits apply from
/// the next song. Empty (+ one INFO) when the folder holds no main clip —
/// the stage cameras then film the movie backdrop too.
fn movie_camera_lists(rng: &mut Rng, dancers: usize) -> (Vec<String>, Vec<String>) {
    let stems = movie_camera::list_stems(movie_camera::MOVIE_CAMERA_DIR);
    let chosen = movie_camera::filter_for(&stems, dancers);
    let (main, non) = super::selection::camera_lists(rng, &chosen);
    if main.is_empty() {
        log_info!(
            "BackgroundDancers: no movie camera clip for {} dancer(s) in {} ({} file(s)) -- the stage cameras film the movie backdrop",
            dancers,
            movie_camera::MOVIE_CAMERA_DIR,
            stems.len()
        );
        return (Vec::new(), Vec::new());
    }
    (main, non)
}

/// Window exit / disable: put the screen route's code byte back, clear the
/// suppressor and put the remembered movie sizes back.
fn restore_movie_mode() {
    screen_route::disarm();
    if MOVIE_SUPPRESSED.swap(false, Ordering::AcqRel) {
        movie_policy::set_suppressed(MovieSuppressor::BackgroundDancers, false);
        log_info!("BackgroundDancers: movie graph suppression lifted at song-window exit");
    }
    let saved = MOVIE_SIZE_SAVED
        .lock()
        .map(|mut g| std::mem::replace(&mut *g, [None, None]))
        .unwrap_or([None, None]);
    if saved.iter().any(Option::is_some) {
        let n = movie_size::restore(saved);
        log_info!(
            "BackgroundDancers: movie size restored ({} side(s)) at song-window exit",
            n
        );
    }
}

/// Entered sides in side order (FR-3: the bot's phantom side counts) — the
/// dancer index *i* is the *i*-th entered side. `None` when the record
/// layout is unavailable.
fn entered_side_list() -> Option<Vec<u8>> {
    let a = stage_records::side_entered(0);
    let b = stage_records::side_entered(1);
    if a.is_none() && b.is_none() {
        return None;
    }
    let mut sides = Vec::with_capacity(2);
    if a.unwrap_or(false) {
        sides.push(0);
    }
    if b.unwrap_or(false) {
        sides.push(1);
    }
    Some(sides)
}

fn seed_now(scene_id: i32, generation: u64) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x5EED);
    seed_from(nanos ^ generation.rotate_left(17), scene_id as u32)
}

/// First entry into the window: eligibility, pick, load + parse.
fn window_entry(scene_id: i32) {
    if !tables_ready() {
        return;
    }
    let sides = match entered_side_list() {
        Some(s) => s,
        None => {
            if !RECORDS_WARNED.swap(true, Ordering::AcqRel) {
                log_warn!(
                    "BackgroundDancers: stage_records unavailable -- cannot count entered sides; no dancers"
                );
            }
            return;
        }
    };
    let n = sides.len();
    if n == 0 {
        return;
    }
    let generation = {
        let Ok(mut st) = STATE.lock() else { return };
        st.generation += 1;
        st.generation
    };
    let seed = seed_now(scene_id, generation);
    let mut rng = Rng::new(seed);
    // The RANDOM stage pool's screen rule: STAGE SCREENS + a movie that
    // plays ⇒ only stages with screens (the movie on them, never a
    // THUMBNAIL); otherwise only stages without screens (they would be
    // black). Read before the tables lock (it calls the game's music-DB
    // lookup).
    let entered = [
        stage_records::side_entered(0).unwrap_or(false),
        stage_records::side_entered(1).unwrap_or(false),
    ];
    let pool_mode = effective_movie_mode(super::style::movie_mode());
    let song_movie = super::song_movie::committed_song_movie(entered);
    let screen_filter = movie_mode::random_pool_filter(pool_mode, song_movie);
    // Part arcs are probed through the same LayeredFS-aware resolver as the
    // candidates (a missing part is silent — A3 behaviour).
    let arc_exists = |arc: &str| arc_set::resolve_path(&format!("data/arc/{arc}")).is_some();
    let pick = {
        let Ok(tables) = TABLES.lock() else { return };
        let Some(t) = tables.as_ref() else { return };
        let pool = random_stage_pool(&t.stages, |k| {
            screen_filter.keeps(t.screen_stages.contains(k))
        });
        log_random_pool(&pool, screen_filter, pool_mode, song_movie, t.stages.len());
        let random_stages = pool.rows(&t.stages);
        // 1. Developer pin (developer_mode) wins.
        let pinned = t.pin.as_ref().and_then(|p| {
            let (stage, dancers) = apply_pin(p, &t.stages, &t.dancers, n)?;
            let stage_src = if stage.is_some() {
                PickSource::Pin
            } else {
                PickSource::Random
            };
            let stage = match stage {
                Some(s) => s,
                None => super::selection::pick_stage(&mut rng, random_stages)?.clone(),
            };
            let dancer_src = if dancers.is_empty() {
                PickSource::Random
            } else {
                PickSource::Pin
            };
            let dancers = if dancers.is_empty() {
                super::selection::pick_dancers(&mut rng, &t.dancers, n)
            } else {
                dancers
            };
            let count = dancers.len();
            Some(
                assemble_pick(&mut rng, stage, &t.camera_rows, dancers, true, &arc_exists)
                    .with_sources(stage_src, vec![dancer_src; count]),
            )
        });
        match pinned {
            Some(p) => p,
            None => {
                if t.pin.is_some() {
                    log_warn!(
                        "BackgroundDancers: DDR_DANCERS_PIN names an unknown stage/dancer -- random pick"
                    );
                }
                // 2. The BACKGROUND DANCER / BACKGROUND STAGE rows.
                match option_pick(&mut rng, t, random_stages, &sides, &arc_exists) {
                    Some(p) => p,
                    // 3. Plain random.
                    None => match make_pick(
                        &mut rng,
                        random_stages,
                        &t.camera_rows,
                        &t.dancers,
                        n,
                        &arc_exists,
                    ) {
                        Some(p) => p,
                        None => {
                            log_warn!(
                                "BackgroundDancers: no pick possible -- no dancers this song"
                            );
                            return;
                        }
                    },
                }
            }
        }
    };
    let mut pick = pick;
    pick.seed = seed;
    let has_screens = pick
        .stage
        .as_ref()
        .map_or(false, |s| stage_has_screens(&s.key));
    if apply_movie_mode(has_screens) == MovieMode::Fullscreen {
        let (main, non) = movie_camera_lists(&mut rng, pick.dancers.len());
        pick = pick.with_movie_cameras(main, non);
    }
    log_info!("BackgroundDancers: {}", pick.summary());
    widget_renderer::run_on_render_thread(move || request_load(generation, pick));
}

/// One INFO per song: which stages a RANDOM stage draw may land on (only
/// consulted when the stage is not chosen explicitly).
fn log_random_pool(
    pool: &StagePool,
    filter: ScreenFilter,
    mode: MovieMode,
    song: SongMovie,
    rows: usize,
) {
    let song = match song {
        SongMovie::Plays => "movie plays",
        SongMovie::None => "no movie",
        SongMovie::Unknown => "movie unknown",
    };
    match pool {
        StagePool::All => log_info!(
            "BackgroundDancers: random stage pool -- all {} rows ({}; Background Movies {}, {})",
            rows,
            filter.label(),
            mode.key(),
            song
        ),
        StagePool::Filtered { rows: kept, excluded } => log_info!(
            "BackgroundDancers: random stage pool -- {} of {} rows, {} stage(s) excluded ({}; Background Movies {}, {})",
            kept.len(),
            rows,
            excluded,
            filter.label(),
            mode.key(),
            song
        ),
        StagePool::NoneLeft => log_warn!(
            "BackgroundDancers: random stage pool -- no stage matches ({}); drawing from all {} rows (Background Movies {}, {})",
            filter.label(),
            rows,
            mode.key(),
            song
        ),
    }
}

/// The option rows' pick (design §4.3 / FR-6): the first entered side's
/// BACKGROUND STAGE (mirrored in versus, so both sides agree) and each
/// entered side's BACKGROUND DANCER for that side's dancer index. `None`
/// when every element is RANDOM (the plain random path then runs). A key the
/// catalog no longer covers (data drift between the cached value and the
/// install) ⇒ one WARN naming it, that element falls back to RANDOM.
/// `random_stages` is the RANDOM stage pool (the screen rule applied); an
/// explicitly chosen stage is looked up in the whole table.
fn option_pick(
    rng: &mut Rng,
    t: &Tables,
    random_stages: &[StageCandidate],
    sides: &[u8],
    arc_exists: &dyn Fn(&str) -> bool,
) -> Option<Pick> {
    let mut stage_key = sides.first().and_then(|&s| super::options::stage_choice(s));
    let mut dancer_keys: Vec<Option<String>> = sides
        .iter()
        .map(|&s| super::options::dancer_choice(s))
        .collect();
    if stage_key.is_none() && dancer_keys.iter().all(Option::is_none) {
        return None;
    }
    // Drop unknown keys up front (resolve_choice refuses the whole request
    // on any unknown key, so validate element by element).
    if let Some(k) = stage_key.as_deref() {
        if !t.stages.iter().any(|s| s.key == k) {
            log_warn!(
                "BackgroundDancers: BACKGROUND STAGE names unknown stage {:?} -- RANDOM for this song",
                k
            );
            stage_key = None;
        }
    }
    for (i, key) in dancer_keys.iter_mut().enumerate() {
        if let Some(k) = key.as_deref() {
            if !t.dancers.iter().any(|d| d.key == k) {
                log_warn!(
                    "BackgroundDancers: BACKGROUND DANCER (P{}) names unknown dancer {:?} -- RANDOM for this song",
                    sides.get(i).map(|s| s + 1).unwrap_or(0),
                    k
                );
                *key = None;
            }
        }
    }
    if stage_key.is_none() && dancer_keys.iter().all(Option::is_none) {
        return None;
    }
    let stage_src = if stage_key.is_some() {
        PickSource::Option
    } else {
        PickSource::Random
    };
    let dancer_src: Vec<PickSource> = dancer_keys
        .iter()
        .map(|k| {
            if k.is_some() {
                PickSource::Option
            } else {
                PickSource::Random
            }
        })
        .collect();
    let refs: Vec<Option<&str>> = dancer_keys.iter().map(Option::as_deref).collect();
    // resolve_choice draws a RANDOM stage from `stages` and looks a chosen
    // key up in it: the pool for RANDOM, the whole table for a choice.
    let stages = if stage_key.is_some() {
        &t.stages[..]
    } else {
        random_stages
    };
    let (stage, dancers) =
        super::selection::resolve_choice(rng, stages, &t.dancers, stage_key.as_deref(), &refs)?;
    Some(
        assemble_pick(rng, stage, &t.camera_rows, dancers, false, arc_exists)
            .with_sources(stage_src, dancer_src),
    )
}

/// Game thread: hand the arcs to the FileManager and start the parse thread.
fn request_load(generation: u64, pick: Pick) {
    let loaded = scene_window::load_arcs(&pick, &ParseOptions::GAMEPLAY);
    let Ok(mut st) = STATE.lock() else {
        loaded.free();
        return;
    };
    if st.generation != generation || !IN_WINDOW.load(Ordering::Acquire) {
        // The window closed before we ran: release immediately.
        loaded.free();
        return;
    }
    // A previous window still tearing down? Park it as the orphan so its
    // dtor poll continues and its arcs are freed at the right time.
    if let Some(prev) = st.live.take() {
        if prev.scene.scene() != ScenePhase::Done {
            log_warn!(
                "BackgroundDancers: new window while the previous scene is still {:?} -- parking it",
                prev.scene.scene()
            );
            if let Some(old) = st.orphan.take() {
                log_warn!("BackgroundDancers: dropping an older orphan (arcs leaked)");
                old.forget_arcs();
            }
            st.orphan = Some(prev.scene);
        } else {
            prev.scene.finish_silent();
        }
    }
    st.live = Some(Window {
        generation,
        scene: SceneWindow::start(
            "BackgroundDancers",
            "this song",
            pick,
            loaded,
            ParseOptions::GAMEPLAY,
        ),
        clock: Clock::new(),
        tempo: None,
        tempo_dps: 0,
        tempo_basename: String::new(),
        tempo_rx: None,
        tempo_failed_logged: false,
        visible_logged: false,
        playing_logged: 0,
        rewind_logged: false,
        camera_source: None,
        hide_armed: false,
        static_published: false,
        movie_mode: window_movie_mode(),
        backdrop: Backdrop::None,
    });
    ACTIVE.store(true, Ordering::Release);
}

/// Game thread: the window closed — start the teardown (or free the arcs
/// right away when nothing was attached).
fn begin_teardown(window_gen: u64) {
    let Ok(mut st) = STATE.lock() else { return };
    if st.generation != window_gen {
        return;
    }
    background_hide::disarm();
    let Some(w) = st.live.as_mut() else { return };
    if w.scene.begin_teardown("song-window exit") {
        ACTIVE.store(true, Ordering::Release);
    }
}

/// Per-frame driver (game thread). O(1) when idle.
pub fn on_frame() {
    if !ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let Ok(mut st) = STATE.lock() else { return };
    let mut live_done = false;
    let mut orphan_done = false;
    if let Some(w) = st.live.as_mut() {
        if w.scene.scene() == ScenePhase::Live {
            drive_live(w);
        }
        if w.scene.drive_teardown("scene") {
            live_done = true;
        }
    }
    if let Some(o) = st.orphan.as_mut() {
        if o.drive_teardown("orphan scene") {
            orphan_done = true;
        }
    }
    if live_done {
        if let Some(w) = st.live.take() {
            w.scene.finish("");
        }
    }
    if orphan_done {
        if let Some(o) = st.orphan.take() {
            o.finish("orphan window's ");
        }
    }
    if st.live.is_none() && st.orphan.is_none() {
        ACTIVE.store(false, Ordering::Release);
    }
}

/// Requested → Built/Abandoned + the visibility/clock/director step.
fn drive_live(w: &mut Window) {
    let since_request_ms = w.scene.since_request_ms();

    // Parse result → session (once), build what is resident, residency
    // timeout — the shared machinery; the session's style / outline plan is
    // decided here, when the parse lands: the operator's request gated on
    // what the synthesis actually serves this boot (a bit-31 twin against a
    // stock 4×(0,0,0) container would just draw the body twice). The hull
    // plan (INK / LAYERED colours + width bands) is frozen per song.
    let has_built = w.scene.drive_assets(|pick, parsed, requested_at| {
        let eff = super::style::effective();
        let hulls = super::style::hull_plan(&eff);
        Session::new(
            pick.clone(),
            parsed,
            requested_at,
            tempo_options(),
            eff.style,
            hulls,
            0,
            None,
        )
    });
    if !has_built {
        return;
    }

    // A node the ENGINE destroyed outside our teardown: park it (one WARN).
    w.scene.park_engine_destroyed();

    tempo_tick(w);

    // Visibility + clock (pure rules in `clock.rs`). The graph enable bit
    // alone is NOT the song-start edge — it is still set from the previous
    // scene while the loader runs (scenes 26/27) and only the live
    // DancePlaySequence's onInitialize clears it. Nor is "GAMEPLAY + a live
    // child": the scene callback fires BEFORE `createNextSequence`, so the
    // active child is still the stage-indicator sequence for the first
    // frames of scene 28 (deploy #2: `visible` at 33–43 ms on every window
    // whose scenes 26/27 took under a frame). FR-9 = the bit ∧ a
    // vtable-verified DancePlaySequence at/after the step that sets it.
    let graph_enabled = scene_graph::graph_stats().map_or(false, |g| g.enabled)
        && matches!(song_reset::dps_step(), Some(s) if s >= song_reset::DPS_STEP_GRAPH_ENABLE);
    let anchored = song_reset::first_anchored_frame();
    let count = if anchored {
        song_reset::current_raw_music_count()
    } else {
        None
    };
    let (mc, visible, event) = w.clock.step(graph_enabled, anchored, count);
    // Dance time from the music count (design + RE doc §3.5): the tempo map
    // when it resolved, real time from music 0 otherwise. Pre-song frames
    // pose the scene just before the expected first count.
    let mc_ms = mc.unwrap_or(PRE_SONG_MC_MS);
    let t = match w.tempo.as_ref() {
        Some(map) => map.tau(mc_ms as f64),
        None => mc_ms as f32 / 1000.0,
    };
    match event {
        ClockEvent::Latched { mc } => {
            // A3 resets the shadow low-pass at every song start; the camera
            // event loop re-simulates from 0.
            if let Some(sess) = w.scene.session_mut() {
                sess.reset_shadow();
                sess.reset_camera();
            }
            if w.playing_logged < 3 {
                w.playing_logged += 1;
                log_info!(
                    "BackgroundDancers: playing -- first count {} ms (dance time {:.2} s{})",
                    mc,
                    t,
                    match w.tempo.as_ref() {
                        Some(map) => format!(
                            ", tempo map: {:.1} BPM here, tau=0 at {} ms",
                            map.bpm_at(mc as f64),
                            map.anchor_ms()
                        ),
                        None => String::from(", real time"),
                    }
                );
            }
        }
        ClockEvent::Rewound { from, to } => {
            // The timeline follows the count (training rewind / loop ⇒ back
            // to that position; in-place restart ⇒ the song-start count ⇒
            // the start). Dance/camera/stage loops are pure functions of the
            // dance time; the camera director re-simulates on t < prev.
            if !w.rewind_logged {
                w.rewind_logged = true;
                log_info!(
                    "BackgroundDancers: music count jumped back ({} -> {} ms) -- timeline follows (dance time {:.2} s)",
                    from,
                    to,
                    t
                );
            }
        }
        ClockEvent::None => {}
    }

    // Background Movies = FULLSCREEN / MOVIE ONLY: is this song's movie
    // drawn? Probed every frame (the probe reads the LIVE DancePlaySequence
    // only — before its step 2 there is no SceneManageActor yet and the
    // answer is None), so the camera director switches to the movie set
    // while the scene is still hidden and the first visible frame is
    // already filmed by it. By DPS step 5 (the visibility edge) the
    // SceneManageActor has settled the movie: it answers the step-3
    // readiness poll only after its MovieActor left the opening step.
    if movie_mode::probes_backdrop(w.movie_mode) {
        let backdrop = movie_backdrop::probe();
        if backdrop != w.backdrop {
            w.backdrop = backdrop;
            let what = match (w.movie_mode, backdrop) {
                (MovieMode::MovieOnly, Backdrop::Active) => {
                    "movie is drawn: the whole 3D scene hidden (stage, dancers, shadows), 2D background left to the game"
                }
                (MovieMode::MovieOnly, Backdrop::Pending) => {
                    "movie still opening: the whole 3D scene hidden until it settles"
                }
                (MovieMode::MovieOnly, Backdrop::None) => {
                    "no movie drawn this song: stage + dancers shown"
                }
                (_, Backdrop::Active) => {
                    "movie is the backdrop: stage + floor shadows hidden, 2D background left to the game"
                }
                (_, Backdrop::Pending) => {
                    "movie still opening: stage + floor shadows hidden until it settles"
                }
                (_, Backdrop::None) => "no movie drawn this song: stage shown",
            };
            log_info!(
                "BackgroundDancers: background movies {} -- {} ({})",
                w.movie_mode.key(),
                what,
                movie_backdrop::describe()
            );
        }
    }
    let mask = movie_mode::scene_mask(w.movie_mode, w.backdrop);

    // The 2D hide arms on the first visible frame whose scene shows the
    // stage (bg_root exists by then — it is created by the DPS whose step we
    // just verified) and stays armed while it does: `background_hide::
    // on_frame` re-resolves the live layer every frame, so a `finish`-path
    // quick restart's fresh bg_root is covered too. Behind a fullscreen
    // movie the game disables the whole BackgroundFrame itself, so the hide
    // stands down (a course stage with a movie after one without, or back).
    if visible {
        let want = movie_mode::wants_bg_hide(mask);
        if want && !w.hide_armed {
            w.hide_armed = true;
            background_hide::arm();
        } else if !want && w.hide_armed {
            w.hide_armed = false;
            background_hide::disarm();
        }
    }
    if visible && !w.visible_logged {
        w.visible_logged = true;
        log_info!(
            "BackgroundDancers: visible -- graph enabled {} ms after request (dance time {:.2} s, dps step {:?}, tempo map {})",
            since_request_ms,
            t,
            song_reset::dps_step(),
            if w.tempo.is_some() { "ready" } else { "absent -- real time" }
        );
    }
    let publish = if STATIC_POSES.load(Ordering::Relaxed) {
        // Bisect mode: exactly one publish once visible.
        if visible && !w.static_published {
            w.static_published = true;
            true
        } else {
            !visible && !w.static_published
        }
    } else {
        true
    };
    if publish {
        w.scene.publish(t, visible, mask);
    }
    camera_tick(w, t, mask);

    w.scene.retry_textures();
    w.scene.attached_diagnostics();
}

/// Which camera films a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CamSource {
    /// A `.camanm` set through the A3 stage-mode director.
    Set(CameraSet),
    /// The fixed fallback framing (no usable set for this scene).
    Fixed,
}

/// The camera for this frame: the MOVIE set while the scene shows the
/// dancers alone over a fullscreen movie (and the set parsed), else the
/// stage row's set, else the fixed fallback.
fn camera_source(sess: &Session, mask: SceneMask) -> CamSource {
    if mask == SceneMask::DANCERS_ONLY && sess.has_movie_camera() {
        CamSource::Set(CameraSet::Movie)
    } else if sess.has_camera() {
        CamSource::Set(CameraSet::Stage)
    } else {
        CamSource::Fixed
    }
}

/// The fixed fallback camera: the A3 add-on's cabinet-verified framing
/// (`docs/3d_model_format_research.md` §6 — eye (0, 1.6, 5.0) m looking at
/// (0, 0.9, 0), in-game hFOV 76.8°), which put the whole boom00 stage + a
/// dancer at 38 % of the frame in view.
fn fixed_camera() -> scene_graph::CamSample {
    scene_graph::CamSample::perspective(
        [0.0, 1.6, 5.0],
        [0.0, 0.9, 0.0],
        [0.0, 1.0, 0.0],
        (76.8f32 * 0.5).to_radians().tan(),
        16.0 / 9.0,
        0.1,
        500.0,
    )
}

/// Camera director: camera slot 0 every frame from the chosen set (A3
/// stage mode), or the fixed camera once whenever the source becomes Fixed.
/// Written while the scene is still hidden too, so the first visible frame
/// already renders through the right camera (the tick copies the slot into
/// the passes a frame after the dirty bytes are raised) — harmless: stock
/// World draws nothing through the MODEL passes. A switch between sets
/// (a course stage with / without a movie, a movie that failed to open)
/// starts the other set's event loop where the song clock is — both loops
/// are pure functions of dance time.
fn camera_tick(w: &mut Window, t: f32, mask: SceneMask) {
    let (camera_main, camera_non, movie_main, movie_non) = {
        let p = w.scene.pick();
        (
            p.camera_main.clone(),
            p.camera_non.clone(),
            p.movie_camera_main.clone(),
            p.movie_camera_non.clone(),
        )
    };
    let Some(sess) = w.scene.session_mut() else {
        return;
    };
    let source = camera_source(sess, mask);
    let changed = w.camera_source != Some(source);
    match source {
        CamSource::Fixed => {
            if changed {
                if scene_graph::write_camera0(&fixed_camera()) {
                    log_info!(
                        "BackgroundDancers: camera slot 0 written (fixed fallback camera: eye [0,1.6,5] target [0,0.9,0] hFOV 76.8 -- no camera set for this scene)"
                    );
                } else {
                    log_warn!(
                        "BackgroundDancers: camera slot 0 write refused -- the engine's own camera stays (models may be out of frame)"
                    );
                }
            }
        }
        CamSource::Set(set) => match director::camera_frame(sess, t, set) {
            Some(cam) => {
                let ok = scene_graph::write_camera0(&cam);
                if changed {
                    if ok {
                        let (main, non) = match set {
                            CameraSet::Stage => (&camera_main, &camera_non),
                            CameraSet::Movie => (&movie_main, &movie_non),
                        };
                        log_info!(
                            "BackgroundDancers: camera director -- {} set main:{:?} non:{:?} (slot 0 written every frame)",
                            set.tag(),
                            main,
                            non
                        );
                        if dev_mode() {
                            log_info!(
                                "BackgroundDancers: {} camera timeline -- {}",
                                set.tag(),
                                director::camera_timeline(sess, 180.0, set)
                            );
                        }
                    } else {
                        log_warn!(
                            "BackgroundDancers: camera slot 0 write refused -- the engine's own camera stays (models may be out of frame)"
                        );
                    }
                }
            }
            None => {
                if changed {
                    log_warn!(
                        "BackgroundDancers: {} camera director produced no sample -- camera slot 0 not written (schedule/clips inconsistent)",
                        set.tag()
                    );
                }
            }
        },
    }
    w.camera_source = Some(source);
}

/// Keep the window's tempo map matched to the live DancePlaySequence:
/// a verified DPS with a new pointer re-reads the basename; a new basename
/// (course stage) or a first sighting spawns the SSQ resolver; the result
/// is polled here. Real time until the map lands (it lands within
/// milliseconds, seconds before the scene becomes visible).
fn tempo_tick(w: &mut Window) {
    if let Some((dps, basename)) = song_reset::live_dps_basename() {
        if dps != w.tempo_dps {
            w.tempo_dps = dps;
            if basename != w.tempo_basename || (w.tempo.is_none() && w.tempo_rx.is_none()) {
                w.tempo_basename = basename.clone();
                w.tempo = None;
                w.tempo_failed_logged = false;
                w.tempo_rx = Some(tempo_source::spawn(basename, tempo_options()));
            }
        }
    }
    let Some(rx) = w.tempo_rx.as_ref() else {
        return;
    };
    let result = rx.lock().ok().and_then(|mut g| g.take());
    if let Some(result) = result {
        w.tempo_rx = None;
        match result {
            Ok(map) => {
                let o = map.options();
                log_info!(
                    "BackgroundDancers: tempo map for '{}' -- {} node(s), {:.1} BPM at music 0, dance time 0 at {} ms (bpm_sync={} stop_slow={})",
                    w.tempo_basename,
                    map.node_count(),
                    map.bpm_at(0.0),
                    map.anchor_ms(),
                    o.bpm_sync,
                    o.stop_slow
                );
                w.tempo = Some(map);
            }
            Err(e) => {
                if !w.tempo_failed_logged {
                    w.tempo_failed_logged = true;
                    log_warn!(
                        "BackgroundDancers: no tempo map for '{}' ({}) -- dancing in real time this song",
                        w.tempo_basename,
                        e
                    );
                }
            }
        }
    }
}

/// Mod disable: neutralise any live scene. The frame callback that drives a
/// teardown to completion is gone once the mod is disabled, so attached
/// nodes are DISABLED + hidden and then leaked with their items and arcs
/// (one WARN) — never freed, never a use-after-free.
pub fn teardown_on_disable() {
    IN_WINDOW.store(false, Ordering::Release);
    ACTIVE.store(false, Ordering::Release);
    restore_movie_mode();
    let window_gen = STATE.lock().map(|st| st.generation).unwrap_or(0);
    widget_renderer::run_on_render_thread(move || neutralise_on_disable(window_gen));
}

fn neutralise_on_disable(window_gen: u64) {
    let Ok(mut st) = STATE.lock() else { return };
    if st.generation != window_gen {
        return;
    }
    background_hide::disarm();
    let mut leaked = false;
    for scene in st
        .live
        .take()
        .map(|w| w.scene)
        .into_iter()
        .chain(st.orphan.take())
    {
        leaked |= scene.neutralise();
    }
    frame_board::clear_all();
    if leaked {
        log_warn!(
            "BackgroundDancers: mod disabled with a live scene -- nodes disabled and LEAKED with their items and arcs (re-enable + a new song starts fresh)"
        );
    } else {
        log_info!("BackgroundDancers: arcs freed at mod disable");
    }
}
