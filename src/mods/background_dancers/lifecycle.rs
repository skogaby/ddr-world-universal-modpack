//! The per-song lifecycle (design §3.2 / §4.3.1): tables at enable, the
//! pick at the first entry into the song window {26, 27, 28}, the parse
//! thread + residency-gated build, the visibility/clock gates, the per-frame
//! director call, and the teardown at window exit — the Step 3/4 spike's
//! cabinet-proven teardown state machine (disable + hide → item list drops
//! every item for 2 frames → queue destroys → dtors → node blocks → arcs;
//! 5 s caps ⇒ leak + WARN) generalised over a [`Session`].
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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::core::anm::rlist;
use crate::core::arc as arcfile;
use crate::services::scene3d::node_layout::SceneNode;
use crate::services::scene3d::{arc_set, frame_board, node, render_item, scene_graph, texture};
use crate::services::{song_reset, stage_records, widget_renderer};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use super::background_hide;
use super::clock::{Clock, ClockEvent};
use super::director;
use super::movie_size;
use super::selection::{
    apply_pin, dancer_candidates, parse_pin, seed_from, stage_candidates, DancerCandidate, Pin,
    Rng, StageCandidate,
};
use super::session::{assemble_pick, make_pick, parse_pick, Parsed, Pick, Session};
use super::tempo::{TempoMap, TempoOptions};
use super::tempo_source;

/// Give up waiting for residency (one WARN) after this long; whatever was
/// built keeps running (FR-13).
const RESIDENCY_TIMEOUT_MS: u64 = 20_000;
/// Teardown: how long to wait for the engine per phase before leaking.
const TEARDOWN_TIMEOUT_MS: u64 = 5_000;
/// Consecutive frames every item must be absent from the engine's list
/// before the destroys are queued (covers the intra-frame job ordering).
const UNLISTED_FRAMES_REQUIRED: u32 = 2;
/// Frames after the first attach for the "not collected yet" checkpoint.
const NOT_COLLECTED_DIAG_FRAMES: u32 = 180;
const NOT_COLLECTED_WARN_FRAMES: u32 = 900;
/// Per-frame texture retry gives up (one WARN) after this many frames.
const TEXTURE_RETRY_FRAMES: u32 = 20 * 60;
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
}

static TABLES: Mutex<Option<Tables>> = Mutex::new(None);
static TABLES_READY: AtomicBool = AtomicBool::new(false);

/// Load the four `startup.arc` rlists, build the candidate tables (arc
/// existence through the LayeredFS-aware resolver), read the developer pin.
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
    let distinct = super::selection::distinct_stage_keys(&stages).len();
    if stages.is_empty() || dancers.is_empty() {
        log_warn!(
            "BackgroundDancers: no {} candidate (mapset_*/pl_* arcs missing from the install) -- mod inactive",
            if stages.is_empty() { "stage" } else { "dancer" }
        );
        return false;
    }
    let pin = read_pin();
    let dev_mode = crate::mods::config::get()
        .and_then(|c| c.layeredfs.as_ref())
        .map(|l| l.developer_mode)
        .unwrap_or(false);
    let static_poses = dev_mode && std::env::var_os("DDR_DANCERS_STATIC").is_some();
    STATIC_POSES.store(static_poses, Ordering::Release);
    let bd = crate::mods::config::get()
        .and_then(|c| c.background_dancers.clone())
        .unwrap_or_default();
    TEMPO_OPTS.store(
        (bd.bpm_sync as u8) | ((bd.stop_slow as u8) << 1),
        Ordering::Release,
    );
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
        "BackgroundDancers: tables ready -- {} stage rows ({} distinct stages), {} dancers, {} camera rows{}",
        stages.len(),
        distinct,
        dancers.len(),
        camera_rows.len(),
        match &pin {
            Some(p) => format!(" (DDR_DANCERS_PIN honoured: {:?})", p),
            None => String::new(),
        }
    );
    if let Ok(mut t) = TABLES.lock() {
        *t = Some(Tables {
            stages,
            camera_rows,
            dancers,
            pin,
        });
    }
    TABLES_READY.store(true, Ordering::Release);
    true
}

pub fn tables_ready() -> bool {
    TABLES_READY.load(Ordering::Acquire)
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

/// Where the window's assets are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssetPhase {
    /// Arcs requested, parse thread running / models not all resident.
    Requested,
    /// Every instance built or skipped.
    Built,
    /// Residency timeout: no more building this song.
    Abandoned,
}

/// The spike's teardown phases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScenePhase {
    /// Nodes live (or none built yet).
    Live,
    /// Nodes disabled; waiting for the engine's item list to drop them all.
    Detaching,
    /// Destroys queued; waiting for every dtor.
    Destroying,
    /// Torn down; only the arcs may remain.
    Done,
}

struct Window {
    generation: u64,
    pick: Pick,
    arcs: Option<arc_set::ArcSet>,
    parse_rx: Arc<Mutex<Option<Parsed>>>,
    session: Option<Session>,
    requested_at: Instant,
    assets: AssetPhase,
    scene: ScenePhase,
    clock: Clock,
    /// The live song's tempo map (dance time from the music count) and the
    /// DPS instance / basename it belongs to; `None` ⇒ real time.
    tempo: Option<TempoMap>,
    tempo_dps: usize,
    tempo_basename: String,
    tempo_rx: Option<tempo_source::TempoSlot>,
    tempo_failed_logged: bool,
    // one-shot logs
    warnings_logged: bool,
    built_logged: bool,
    visible_logged: bool,
    playing_logged: u32,
    rewind_logged: bool,
    camera_written: bool,
    hide_armed: bool,
    static_published: bool,
    frames_since_attach: u32,
    first_frame_logged: bool,
    collected_logged: bool,
    // teardown
    teardown_started: Option<Instant>,
    unlisted_frames: u32,
    queue_retries: u32,
}

struct State {
    generation: u64,
    live: Option<Window>,
    /// A previous window still tearing down when a new one opened (only
    /// ever driven to its end).
    orphan: Option<Window>,
}

static STATE: Mutex<State> = Mutex::new(State {
    generation: 0,
    live: None,
    orphan: None,
});

/// A song window is open.
static IN_WINDOW: AtomicBool = AtomicBool::new(false);
/// The movie-size values the open window overrode (restored at exit).
static MOVIE_SIZE_SAVED: Mutex<[Option<u32>; 2]> = Mutex::new([None, None]);
/// The per-frame driver has work (O(1) when clear).
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// `stage_records` unavailable WARN — once per boot.
static RECORDS_WARNED: AtomicBool = AtomicBool::new(false);
/// Dev bisect knob `DDR_DANCERS_STATIC` (developer_mode): publish every
/// instance ONCE (its t = 0 pose) and never again — isolates the per-frame
/// item writes from everything else.
static STATIC_POSES: AtomicBool = AtomicBool::new(false);
/// The two A3 ConfigBank switches (`background_dancers.{bpm_sync,
/// stop_slow}`), read at enable. Bits: 1 = bpm_sync, 2 = stop_slow.
static TEMPO_OPTS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(3);

fn tempo_options() -> TempoOptions {
    let b = TEMPO_OPTS.load(Ordering::Relaxed);
    TempoOptions {
        bpm_sync: b & 1 != 0,
        stop_slow: b & 2 != 0,
    }
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
        // Movie size back first — synchronously, on the game thread, before
        // anything downstream (SONG_SELECT's row re-seed, the logout save)
        // can observe the override.
        restore_movie_size();
        let window_gen = STATE.lock().map(|st| st.generation).unwrap_or(0);
        widget_renderer::run_on_render_thread(move || begin_teardown(window_gen));
    }
}

/// Window entry (game thread, before `createNextSequence`): switch every
/// entered side's fullscreen movie to the sized thumbnail for this song.
fn apply_movie_size() {
    if !movie_size::is_available() {
        return;
    }
    let entered = [
        stage_records::side_entered(0).unwrap_or(false),
        stage_records::side_entered(1).unwrap_or(false),
    ];
    let saved = movie_size::apply(entered);
    if saved.iter().any(Option::is_some) {
        log_info!(
            "BackgroundDancers: movie size overridden for the song -- P1 {:?} P2 {:?} -> 2 (sized thumbnail; restored at window exit)",
            saved[0],
            saved[1]
        );
    }
    if let Ok(mut g) = MOVIE_SIZE_SAVED.lock() {
        *g = saved;
    }
}

/// Window exit / disable: put the remembered movie sizes back.
fn restore_movie_size() {
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

/// Entered sides (FR-3: the bot's phantom side counts). `None` when the
/// record layout is unavailable.
fn entered_sides() -> Option<usize> {
    let a = stage_records::side_entered(0);
    let b = stage_records::side_entered(1);
    if a.is_none() && b.is_none() {
        return None;
    }
    Some(a.unwrap_or(false) as usize + b.unwrap_or(false) as usize)
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
    let n = match entered_sides() {
        Some(n) => n,
        None => {
            if !RECORDS_WARNED.swap(true, Ordering::AcqRel) {
                log_warn!(
                    "BackgroundDancers: stage_records unavailable -- cannot count entered sides; no dancers"
                );
            }
            return;
        }
    };
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
    // Part arcs are probed through the same LayeredFS-aware resolver as the
    // candidates (a missing part is silent — A3 behaviour).
    let arc_exists = |arc: &str| arc_set::resolve_path(&format!("data/arc/{arc}")).is_some();
    let pick = {
        let Ok(tables) = TABLES.lock() else { return };
        let Some(t) = tables.as_ref() else { return };
        let pinned = t.pin.as_ref().and_then(|p| {
            let (stage, dancers) = apply_pin(p, &t.stages, &t.dancers, n)?;
            let stage = match stage {
                Some(s) => s,
                None => super::selection::pick_stage(&mut rng, &t.stages)?.clone(),
            };
            let dancers = if dancers.is_empty() {
                super::selection::pick_dancers(&mut rng, &t.dancers, n)
            } else {
                dancers
            };
            Some(assemble_pick(
                &mut rng,
                stage,
                &t.camera_rows,
                dancers,
                true,
                &arc_exists,
            ))
        });
        match pinned {
            Some(p) => p,
            None => {
                if t.pin.is_some() {
                    log_warn!(
                        "BackgroundDancers: DDR_DANCERS_PIN names an unknown stage/dancer -- random pick"
                    );
                }
                match make_pick(
                    &mut rng,
                    &t.stages,
                    &t.camera_rows,
                    &t.dancers,
                    n,
                    &arc_exists,
                ) {
                    Some(p) => p,
                    None => {
                        log_warn!("BackgroundDancers: no pick possible -- no dancers this song");
                        return;
                    }
                }
            }
        }
    };
    let mut pick = pick;
    pick.seed = seed;
    log_info!("BackgroundDancers: {}", pick.summary());
    apply_movie_size();
    widget_renderer::run_on_render_thread(move || request_load(generation, pick));
}

/// Game thread: hand the arcs to the FileManager and start the parse thread.
fn request_load(generation: u64, pick: Pick) {
    let arcs = pick.arcs();
    let arc_refs: Vec<&str> = arcs.iter().map(String::as_str).collect();
    let set = arc_set::load(&arc_refs);
    let loaded = set.len();
    let Ok(mut st) = STATE.lock() else {
        arc_set::free(set);
        return;
    };
    if st.generation != generation || !IN_WINDOW.load(Ordering::Acquire) {
        // The window closed before we ran: release immediately.
        arc_set::free(set);
        return;
    }
    // A previous window still tearing down? Park it as the orphan so its
    // dtor poll continues and its arcs are freed at the right time.
    if let Some(prev) = st.live.take() {
        if prev.scene != ScenePhase::Done {
            log_warn!(
                "BackgroundDancers: new window while the previous scene is still {:?} -- parking it",
                prev.scene
            );
            if let Some(old) = st.orphan.take() {
                log_warn!("BackgroundDancers: dropping an older orphan (arcs leaked)");
                if let Some(a) = old.arcs {
                    std::mem::forget(a);
                }
            }
            st.orphan = Some(prev);
        } else if let Some(a) = prev.arcs {
            arc_set::free(a);
        }
    }
    let parse_rx: Arc<Mutex<Option<Parsed>>> = Arc::new(Mutex::new(None));
    if loaded > 0 {
        let rx = Arc::clone(&parse_rx);
        let pick_for_thread = pick.clone();
        let spawned = std::thread::Builder::new()
            .name("bg-dancers-parse".into())
            .spawn(move || {
                let parsed = parse_pick(&pick_for_thread);
                if let Ok(mut slot) = rx.lock() {
                    *slot = Some(parsed);
                }
            });
        if spawned.is_err() {
            log_warn!(
                "BackgroundDancers: parse thread could not be spawned -- no dancers this song"
            );
        }
        log_info!(
            "BackgroundDancers: FileManager::Load accepted {} of {} arcs -- parsing + polling residency",
            loaded,
            arcs.len()
        );
    } else {
        log_warn!(
            "BackgroundDancers: no arc loaded (see the scene3d WARNs above) -- no dancers this song"
        );
    }
    st.live = Some(Window {
        generation,
        pick,
        arcs: Some(set),
        parse_rx,
        session: None,
        requested_at: Instant::now(),
        assets: if loaded > 0 {
            AssetPhase::Requested
        } else {
            AssetPhase::Abandoned
        },
        scene: ScenePhase::Live,
        clock: Clock::new(),
        tempo: None,
        tempo_dps: 0,
        tempo_basename: String::new(),
        tempo_rx: None,
        tempo_failed_logged: false,
        warnings_logged: false,
        built_logged: false,
        visible_logged: false,
        playing_logged: 0,
        rewind_logged: false,
        camera_written: false,
        hide_armed: false,
        static_published: false,
        frames_since_attach: 0,
        first_frame_logged: false,
        collected_logged: false,
        teardown_started: None,
        unlisted_frames: 0,
        queue_retries: 0,
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
    if w.scene != ScenePhase::Live {
        return;
    }
    let built: Vec<usize> = w
        .session
        .as_ref()
        .map(|s| s.built().map(|i| i.node).collect())
        .unwrap_or_default();
    if built.is_empty() {
        w.scene = ScenePhase::Done;
        if let Some(set) = w.arcs.take() {
            let n = set.len();
            arc_set::free(set);
            log_info!(
                "BackgroundDancers: {} arc handle(s) freed at song-window exit (no nodes)",
                n
            );
        }
        return;
    }
    if let Some(sess) = w.session.as_mut() {
        director::hide_all(sess);
    }
    for n in built {
        let n = n as *mut SceneNode;
        // SAFETY: attached, dtor not run (scene Live).
        unsafe {
            node::set_enabled(n, false);
            node::set_hidden(n, true);
        }
    }
    w.scene = ScenePhase::Detaching;
    w.teardown_started = Some(Instant::now());
    w.unlisted_frames = 0;
    log_info!(
        "BackgroundDancers: song-window exit -- {} node(s) disabled, waiting for the engine's item list to drop them",
        w.session.as_ref().map(|s| s.built().count()).unwrap_or(0)
    );
    ACTIVE.store(true, Ordering::Release);
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
        if w.scene == ScenePhase::Live {
            drive_live(w);
        }
        if drive_teardown(w, "scene") {
            live_done = true;
        }
    }
    if let Some(o) = st.orphan.as_mut() {
        if drive_teardown(o, "orphan scene") {
            orphan_done = true;
        }
    }
    if live_done {
        if let Some(w) = st.live.take() {
            finish_window(w, "");
        }
    }
    if orphan_done {
        if let Some(o) = st.orphan.take() {
            finish_window(o, "orphan window's ");
        }
    }
    if st.live.is_none() && st.orphan.is_none() {
        ACTIVE.store(false, Ordering::Release);
    }
}

/// Requested → Built/Abandoned + the visibility/clock/director step.
fn drive_live(w: &mut Window) {
    let since_request_ms = w.requested_at.elapsed().as_millis() as u64;

    // Parse result → session (once).
    if w.session.is_none() && w.assets == AssetPhase::Requested {
        let parsed = w.parse_rx.lock().ok().and_then(|mut g| g.take());
        if let Some(parsed) = parsed {
            if !w.warnings_logged {
                w.warnings_logged = true;
                for warn in &parsed.warnings {
                    log_warn!("BackgroundDancers: parse: {}", warn);
                }
            }
            log_info!(
                "BackgroundDancers: parsed in {} ms -- {} stage part(s), {} dancer(s) with {:?} clip(s), {:?} part(s), shadow={}",
                parsed.elapsed_ms,
                parsed.stage_parts.len(),
                parsed.dancers.len(),
                parsed.dancers.iter().map(|d| d.clips.len()).collect::<Vec<_>>(),
                parsed.dancers.iter().map(|d| d.parts.len()).collect::<Vec<_>>(),
                parsed.shadow.is_some()
            );
            if parsed.stage_parts.is_empty() && parsed.dancers.is_empty() {
                log_warn!("BackgroundDancers: nothing parsed -- no dancers this song");
                w.assets = AssetPhase::Abandoned;
            } else {
                w.session = Some(Session::new(
                    w.pick.clone(),
                    parsed,
                    w.requested_at,
                    tempo_options(),
                ));
            }
        }
    }

    // Build what is resident.
    if w.assets == AssetPhase::Requested {
        if let Some(sess) = w.session.as_mut() {
            let progress = sess.build_pending(since_request_ms);
            if progress.built_now > 0 && w.frames_since_attach == 0 {
                w.frames_since_attach = 1;
            }
            if sess.all_settled() {
                w.assets = AssetPhase::Built;
                sess.built_at = Some(Instant::now());
                if !w.built_logged {
                    w.built_logged = true;
                    let (st, dn, pt, sh) = sess.built_counts();
                    log_info!(
                        "BackgroundDancers: built {} ms after request -- {} instance(s) attached hidden ({} stage, {} dancer, {} part, {} shadow), {} skipped",
                        since_request_ms,
                        sess.built().count(),
                        st,
                        dn,
                        pt,
                        sh,
                        sess.instances.len() - sess.built().count()
                    );
                }
            }
        }
        if w.assets == AssetPhase::Requested && since_request_ms > RESIDENCY_TIMEOUT_MS {
            w.assets = AssetPhase::Abandoned;
            let pending: Vec<String> = w
                .session
                .as_ref()
                .map(|s| {
                    s.instances
                        .iter()
                        .filter(|i| i.status == super::session::InstanceStatus::Pending)
                        .map(|i| i.model_name.clone())
                        .collect()
                })
                .unwrap_or_default();
            log_warn!(
                "BackgroundDancers: residency timeout after {} ms -- still missing {:?}{}; whatever was built keeps running",
                since_request_ms,
                pending,
                if w.session.is_none() {
                    " (parse thread never delivered)"
                } else {
                    ""
                }
            );
        }
    }

    let has_built = w
        .session
        .as_ref()
        .map_or(false, |s| s.built().next().is_some());
    if !has_built {
        return;
    }
    if w.frames_since_attach > 0 {
        w.frames_since_attach = w.frames_since_attach.saturating_add(1);
    }

    // Fallback fixed camera when the stage row has no usable camera set: the
    // A3 add-on's cabinet-verified framing (`docs/3d_model_format_research.md`
    // §6 — eye (0, 1.6, 5.0) m looking at (0, 0.9, 0), in-game hFOV 76.8°)
    // which put the whole boom00 stage + a dancer at 38 % of the frame in
    // view. With a camera set the director writes slot 0 every visible frame
    // (below) and this block never runs.
    let has_camera = w.session.as_ref().map_or(false, |s| s.has_camera());
    if !w.camera_written && !has_camera {
        w.camera_written = true;
        let cam = scene_graph::CamSample::perspective(
            [0.0, 1.6, 5.0],
            [0.0, 0.9, 0.0],
            [0.0, 1.0, 0.0],
            (76.8f32 * 0.5).to_radians().tan(),
            16.0 / 9.0,
            0.1,
            500.0,
        );
        if scene_graph::write_camera0(&cam) {
            log_info!(
                "BackgroundDancers: camera slot 0 written (fixed fallback camera: eye [0,1.6,5] target [0,0.9,0] hFOV 76.8 -- no camera set for this stage row)"
            );
        } else {
            log_warn!(
                "BackgroundDancers: camera slot 0 write refused -- the engine's own camera stays (models may be out of frame)"
            );
        }
    }

    // A node the ENGINE destroyed outside our teardown (dtor ran without a
    // queued destroy): stop touching it, leak its block, say so once.
    if let Some(sess) = w.session.as_mut() {
        for inst in sess.built_mut() {
            if inst.queued || inst.freed {
                continue;
            }
            // SAFETY: the node block is ours until `free_node_block`.
            if unsafe { node::is_destroyed(inst.node as *mut SceneNode) } {
                inst.queued = true;
                inst.freed = true; // block deliberately leaked (still linked?)
                log_warn!(
                    "BackgroundDancers: {} node 0x{:X} was destroyed by the ENGINE outside our teardown (its item is freed) -- instance parked, node block leaked",
                    inst.model_name,
                    inst.node
                );
            }
        }
    }

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
            if let Some(sess) = w.session.as_mut() {
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

    // The 2D hide arms on the first visible frame (bg_root exists by then —
    // it is created by the DPS whose step we just verified) and stays armed
    // for the window: `background_hide::on_frame` re-resolves the live layer
    // every frame, so a `finish`-path quick restart's fresh bg_root is
    // covered too.
    if visible && !w.hide_armed {
        w.hide_armed = true;
        background_hide::arm();
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
    if let Some(sess) = w.session.as_mut() {
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
            director::produce(sess, t, visible);
            // Every built instance has now been published at least once, so
            // its board slot — hidden bit included — is authoritative: drop
            // the node-level "force hidden" it was attached with. Deploy #2:
            // the flag was cleared only on the ONE frame `visible` first
            // became true, so every node built after that frame (the dancer
            // is always the last) stayed hidden for the whole song.
            for inst in sess.built_mut().filter(|i| !i.queued && !i.node_shown) {
                inst.node_shown = true;
                // SAFETY: attached, dtor not run (scene Live).
                unsafe { node::set_hidden(inst.node as *mut SceneNode, false) };
            }
        }
        // Camera director: slot 0 every frame (A3 stage mode). Written while
        // the scene is still hidden too, so the first visible frame already
        // renders through the right camera (the tick copies the slot into
        // the passes a frame after the dirty bytes are raised) — harmless:
        // stock World draws nothing through the MODEL passes.
        if sess.has_camera() {
            match director::camera_frame(sess, t) {
                Some(cam) => {
                    let ok = scene_graph::write_camera0(&cam);
                    if !w.camera_written {
                        w.camera_written = true;
                        if ok {
                            log_info!(
                                "BackgroundDancers: camera director -- main:{:?} non:{:?} (slot 0 written every frame)",
                                w.pick.camera_main,
                                w.pick.camera_non
                            );
                            if dev_mode() {
                                log_info!(
                                    "BackgroundDancers: camera timeline -- {}",
                                    director::camera_timeline(sess, 180.0)
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
                    if !w.camera_written {
                        w.camera_written = true;
                        log_warn!(
                            "BackgroundDancers: camera director produced no sample -- fixed camera NOT written either (schedule/clips inconsistent)"
                        );
                    }
                }
            }
        }
    }

    retry_textures(w);
    attached_diagnostics(w);
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

/// Per-frame texture re-resolve for instances built before their DDS
/// registered.
fn retry_textures(w: &mut Window) {
    let frames = w.frames_since_attach;
    let Some(sess) = w.session.as_mut() else {
        return;
    };
    for inst in sess.built_mut() {
        if inst.textures_pending == 0 {
            continue;
        }
        // SAFETY: attached, dtor not run (scene Live).
        let ts = unsafe {
            render_item::retry_texture_resolve(inst.item as *mut u8, inst.material_count)
        };
        if ts.still_default < inst.textures_pending {
            log_info!(
                "BackgroundDancers: {} material textures re-resolved {} ms after attach (total={} load={} re={} default={})",
                inst.model_name,
                inst.attached_at.map(|a| a.elapsed().as_millis()).unwrap_or(0),
                ts.total,
                ts.resolved_at_load,
                ts.re_resolved,
                ts.still_default
            );
        }
        inst.textures_pending = ts.still_default;
        if inst.textures_pending > 0 && frames >= TEXTURE_RETRY_FRAMES {
            log_warn!(
                "BackgroundDancers: {} -- {} material texture(s) STILL unregistered after {} frames -- stays untextured this song",
                inst.model_name,
                inst.textures_pending,
                frames
            );
            inst.textures_pending = 0;
        }
    }
}

/// The "did the engine take our nodes" lines (Step 3 shapes).
fn attached_diagnostics(w: &mut Window) {
    let Some(stats) = scene_graph::graph_stats() else {
        return;
    };
    let wanted = w.session.as_ref().map(|s| s.built().count()).unwrap_or(0);
    if !w.first_frame_logged && w.frames_since_attach >= 2 {
        w.first_frame_logged = true;
        log_info!(
            "BackgroundDancers: first frame after attach -- graph enabled={} visible-nodes={} items={} records={} (nodes attached so far: {})",
            stats.enabled,
            stats.visible,
            stats.items,
            stats.records,
            wanted
        );
    }
    if !w.collected_logged && stats.items > 0 {
        w.collected_logged = true;
        log_info!(
            "BackgroundDancers: items collected by SceneGraph::update {} ms after request -- graph enabled={} visible-nodes={} items={} records={} (nodes attached: {})",
            w.requested_at.elapsed().as_millis(),
            stats.enabled,
            stats.visible,
            stats.items,
            stats.records,
            wanted
        );
    }
    if !w.collected_logged
        && (w.frames_since_attach == NOT_COLLECTED_DIAG_FRAMES
            || w.frames_since_attach == NOT_COLLECTED_WARN_FRAMES)
    {
        let anomaly = stats.enabled || w.frames_since_attach == NOT_COLLECTED_WARN_FRAMES;
        let msg = format!(
            "BackgroundDancers: items not collected {} frames after attach -- graph enabled={} visible-nodes={} items={} (enabled=false ⇒ DPS has not reached step 5; enabled=true & visible=0 ⇒ pass-4 gate/visit; visible>0 & items=0 ⇒ item push)",
            w.frames_since_attach, stats.enabled, stats.visible, stats.items
        );
        if anomaly {
            log_warn!("{}", msg);
        } else {
            log_info!("{}", msg);
        }
    }
}

/// Advance a window's teardown by one frame. `true` = the window is finished
/// (caller frees/leaks the arcs via `finish_window`).
fn drive_teardown(w: &mut Window, label: &str) -> bool {
    match w.scene {
        ScenePhase::Live => false,
        ScenePhase::Done => true,
        ScenePhase::Detaching => {
            let elapsed = w
                .teardown_started
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
            let Some(sess) = w.session.as_mut() else {
                w.scene = ScenePhase::Done;
                return true;
            };
            let any_listed = sess
                .built()
                .filter(|i| !i.queued)
                .any(|i| scene_graph::item_listed(i.item as *const u8) != Some(false));
            if any_listed {
                w.unlisted_frames = 0;
            } else {
                w.unlisted_frames += 1;
            }
            if w.unlisted_frames >= UNLISTED_FRAMES_REQUIRED {
                let mut pending = 0;
                for inst in sess.built_mut() {
                    if inst.queued {
                        continue;
                    }
                    // (The frame-board slot is NOT cleared here: a new
                    // window may already own it, and a stale slot is harmless
                    // — its node is disabled, a new node stays hidden through
                    // its own node flag until the director publishes.)
                    if scene_graph::queue_destroy(inst.node as *mut SceneNode) {
                        inst.queued = true;
                    } else {
                        pending += 1;
                    }
                }
                if pending == 0 {
                    w.scene = ScenePhase::Destroying;
                    log_info!(
                        "BackgroundDancers: {} -- {} destroy(s) queued {} ms after window exit (items unlisted for {} frames)",
                        label,
                        sess.built().count(),
                        elapsed,
                        w.unlisted_frames
                    );
                    return false;
                }
                w.queue_retries += 1;
                if w.queue_retries == 60 {
                    log_warn!(
                        "BackgroundDancers: queue_destroy refused 60 frames in a row for {} node(s) -- still retrying",
                        pending
                    );
                }
                if elapsed > TEARDOWN_TIMEOUT_MS {
                    log_warn!(
                        "BackgroundDancers: {} -- {} node(s) could not be queued for destroy within {} ms -- leaking them (disabled), freeing the arcs",
                        label,
                        pending,
                        elapsed
                    );
                    w.scene = ScenePhase::Done;
                    return true;
                }
                return false;
            }
            if elapsed > TEARDOWN_TIMEOUT_MS {
                log_warn!(
                    "BackgroundDancers: {} -- an item is still referenced by the engine's item list {} ms after window exit -- leaking nodes+items+arcs (graph disabled with a stale list?)",
                    label,
                    elapsed
                );
                // Leak the arcs too: an item may still be read.
                if let Some(a) = w.arcs.take() {
                    std::mem::forget(a);
                }
                w.scene = ScenePhase::Done;
                return true;
            }
            false
        }
        ScenePhase::Destroying => {
            let elapsed = w
                .teardown_started
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
            let Some(sess) = w.session.as_mut() else {
                w.scene = ScenePhase::Done;
                return true;
            };
            let mut remaining = 0;
            for inst in sess.built_mut() {
                if inst.freed {
                    continue;
                }
                let n = inst.node as *mut SceneNode;
                // SAFETY: the node block is ours until `free_node_block`.
                if unsafe { node::is_destroyed(n) } {
                    unsafe { node::free_node_block(n) };
                    inst.freed = true;
                } else {
                    remaining += 1;
                }
            }
            if remaining == 0 {
                log_info!(
                    "BackgroundDancers: {} -- all {} node(s) destroyed by the engine flush {} ms after window exit -- node blocks freed",
                    label,
                    sess.built().count(),
                    elapsed
                );
                w.scene = ScenePhase::Done;
                return true;
            }
            if elapsed > TEARDOWN_TIMEOUT_MS {
                log_warn!(
                    "BackgroundDancers: {} -- {} dtor(s) not observed {} ms after window exit -- leaking those nodes+items, freeing the arcs (items are unlisted)",
                    label,
                    remaining,
                    elapsed
                );
                w.scene = ScenePhase::Done;
                return true;
            }
            false
        }
    }
}

/// Free a finished window's arcs (unless the teardown leaked them) and log
/// the texture balance.
fn finish_window(w: Window, label: &str) {
    match w.arcs {
        Some(set) => {
            let n = set.len();
            arc_set::free(set);
            log_info!(
                "BackgroundDancers: {}{} arc handle(s) freed after the scene teardown; scene3d textures: {}",
                label,
                n,
                texture::balance()
            );
        }
        None => {}
    }
    // The parse thread's result (if it still lands) is dropped with `w`.
}

/// Mod disable: neutralise any live scene. The frame callback that drives a
/// teardown to completion is gone once the mod is disabled, so attached
/// nodes are DISABLED + hidden and then leaked with their items and arcs
/// (one WARN) — never freed, never a use-after-free.
pub fn teardown_on_disable() {
    IN_WINDOW.store(false, Ordering::Release);
    ACTIVE.store(false, Ordering::Release);
    restore_movie_size();
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
    for w in st.live.take().into_iter().chain(st.orphan.take()) {
        if w.scene == ScenePhase::Live {
            if let Some(sess) = w.session.as_ref() {
                for inst in sess.built() {
                    let n = inst.node as *mut SceneNode;
                    // SAFETY: attached, dtor not run.
                    unsafe {
                        node::set_enabled(n, false);
                        node::set_hidden(n, true);
                    }
                }
            }
        }
        let had_nodes = w
            .session
            .as_ref()
            .map_or(false, |s| s.built().next().is_some());
        if w.scene != ScenePhase::Done && had_nodes {
            // Nodes stay linked (disabled) in the engine tree with their
            // items; the arcs must outlive them.
            leaked = true;
            if let Some(a) = w.arcs {
                std::mem::forget(a);
            }
        } else if let Some(a) = w.arcs {
            arc_set::free(a);
        }
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
