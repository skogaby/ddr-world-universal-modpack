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
//! size, camera slot 0.
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
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::anm::rlist;
use crate::core::arc as arcfile;
use crate::services::scene3d::{arc_set, frame_board, scene_graph};
use crate::services::{song_reset, stage_records, widget_renderer};
use crate::types::scenes::scene;
use crate::{log_info, log_warn};

use super::background_hide;
use super::clock::{Clock, ClockEvent};
use super::director;
use super::movie_size;
use super::scene_window::{self, ScenePhase, SceneWindow};
use super::selection::{
    apply_pin, dancer_candidates, parse_pin, seed_from, stage_candidates, DancerCandidate,
    PickSource, Pin, Rng, StageCandidate,
};
use super::session::{assemble_pick, make_pick, ParseOptions, Pick, Session};
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
    camera_written: bool,
    hide_armed: bool,
    static_published: bool,
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
    // Part arcs are probed through the same LayeredFS-aware resolver as the
    // candidates (a missing part is silent — A3 behaviour).
    let arc_exists = |arc: &str| arc_set::resolve_path(&format!("data/arc/{arc}")).is_some();
    let pick = {
        let Ok(tables) = TABLES.lock() else { return };
        let Some(t) = tables.as_ref() else { return };
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
                None => super::selection::pick_stage(&mut rng, &t.stages)?.clone(),
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
                match option_pick(&mut rng, t, &sides, &arc_exists) {
                    Some(p) => p,
                    // 3. Plain random.
                    None => match make_pick(
                        &mut rng,
                        &t.stages,
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
    log_info!("BackgroundDancers: {}", pick.summary());
    apply_movie_size();
    widget_renderer::run_on_render_thread(move || request_load(generation, pick));
}

/// The option rows' pick (design §4.3 / FR-6): the first entered side's
/// BACKGROUND STAGE (mirrored in versus, so both sides agree) and each
/// entered side's BACKGROUND DANCER for that side's dancer index. `None`
/// when every element is RANDOM (the plain random path then runs). A key the
/// catalog no longer covers (data drift between the cached value and the
/// install) ⇒ one WARN naming it, that element falls back to RANDOM.
fn option_pick(
    rng: &mut Rng,
    t: &Tables,
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
    let (stage, dancers) =
        super::selection::resolve_choice(rng, &t.stages, &t.dancers, stage_key.as_deref(), &refs)?;
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
        camera_written: false,
        hide_armed: false,
        static_published: false,
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

    // Fallback fixed camera when the stage row has no usable camera set: the
    // A3 add-on's cabinet-verified framing (`docs/3d_model_format_research.md`
    // §6 — eye (0, 1.6, 5.0) m looking at (0, 0.9, 0), in-game hFOV 76.8°)
    // which put the whole boom00 stage + a dancer at 38 % of the frame in
    // view. With a camera set the director writes slot 0 every visible frame
    // (below) and this block never runs.
    let has_camera = w.scene.session().map_or(false, |s| s.has_camera());
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
        w.scene.publish(t, visible);
    }
    // Camera director: slot 0 every frame (A3 stage mode). Written while
    // the scene is still hidden too, so the first visible frame already
    // renders through the right camera (the tick copies the slot into
    // the passes a frame after the dirty bytes are raised) — harmless:
    // stock World draws nothing through the MODEL passes.
    // (The camera lists are only needed for the one-shot INFO.)
    let camera_lists = if w.camera_written {
        None
    } else {
        let p = w.scene.pick();
        Some((p.camera_main.clone(), p.camera_non.clone()))
    };
    if let Some(sess) = w.scene.session_mut() {
        if sess.has_camera() {
            match director::camera_frame(sess, t) {
                Some(cam) => {
                    let ok = scene_graph::write_camera0(&cam);
                    if !w.camera_written {
                        w.camera_written = true;
                        if ok {
                            let (camera_main, camera_non) = camera_lists.unwrap_or_default();
                            log_info!(
                                "BackgroundDancers: camera director -- main:{:?} non:{:?} (slot 0 written every frame)",
                                camera_main,
                                camera_non
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

    w.scene.retry_textures();
    w.scene.attached_diagnostics();
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
