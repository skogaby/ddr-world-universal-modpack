//! One 3D scene's asset + node lifecycle — the gameplay `Window`'s
//! cabinet-proven machinery (load → parse thread → residency-gated build →
//! per-frame publish → disable → item list drops every item for 2 frames →
//! queue destroys → dtors → node blocks → arcs; 5 s caps ⇒ leak + WARN),
//! extracted 2026-09-21 (design §4.6 "preview/window.rs") so the options
//! previews run the identical lifecycle on their own scenes. The gameplay
//! wrapper (`lifecycle::Window`) adds the music-count clock, the tempo map,
//! the 2D hide, the movie-size override and camera slot 0; a preview adds
//! its pass set and camera. Neither re-implements anything here.
//!
//! Every log line is prefixed by the owner's `tag` (`BackgroundDancers` for
//! gameplay — the strings a field log is read against are byte-identical to
//! the pre-extraction ones); `scope` fills the "no dancers this song" tail.
//!
//! Engine calls happen on the game thread only (`input_manager::on_frame` /
//! `run_on_render_thread`); the parse thread never touches the engine.
//! Every path is panic-free.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::services::scene3d::node_layout::SceneNode;
use crate::services::scene3d::{arc_set, node, render_item, scene_graph, texture};
use crate::{log_info, log_warn};

use super::director;
use super::session::{parse_pick, ParseOptions, Parsed, Pick, Session};

/// Give up waiting for residency (one WARN) after this long; whatever was
/// built keeps running (FR-13).
pub(super) const RESIDENCY_TIMEOUT_MS: u64 = 20_000;
/// Teardown: how long to wait for the engine per phase before leaking.
pub(super) const TEARDOWN_TIMEOUT_MS: u64 = 5_000;
/// Consecutive frames every item must be absent from the engine's list
/// before the destroys are queued (covers the intra-frame job ordering).
pub(super) const UNLISTED_FRAMES_REQUIRED: u32 = 2;
/// Frames after the first attach for the "not collected yet" checkpoint.
pub(super) const NOT_COLLECTED_DIAG_FRAMES: u32 = 180;
pub(super) const NOT_COLLECTED_WARN_FRAMES: u32 = 900;
/// Per-frame texture retry gives up (one WARN) after this many frames.
pub(super) const TEXTURE_RETRY_FRAMES: u32 = 20 * 60;

/// Where the window's assets are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetPhase {
    /// Arcs requested, parse thread running / models not all resident.
    Requested,
    /// Every instance built or skipped.
    Built,
    /// Residency timeout: no more building this song.
    Abandoned,
}

/// The spike's teardown phases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenePhase {
    /// Nodes live (or none built yet).
    Live,
    /// Nodes disabled; waiting for the engine's item list to drop them all.
    Detaching,
    /// Destroys queued; waiting for every dtor.
    Destroying,
    /// Torn down; only the arcs may remain.
    Done,
}

/// The pick's arcs handed to the engine's FileManager (game thread) — held
/// apart from the window so a caller can still refuse the window (and free
/// them) after loading.
pub struct LoadedArcs {
    set: arc_set::ArcSet,
    /// Arcs the FileManager accepted.
    accepted: usize,
    /// Arcs requested.
    requested: usize,
}

impl LoadedArcs {
    /// The window never opened: release immediately.
    pub fn free(self) {
        arc_set::free(self.set);
    }
}

/// Game thread: hand the pick's arcs to the FileManager.
pub fn load_arcs(pick: &Pick, opts: &ParseOptions) -> LoadedArcs {
    let arcs = pick.arcs_for(opts);
    let arc_refs: Vec<&str> = arcs.iter().map(String::as_str).collect();
    let set = arc_set::load(&arc_refs);
    LoadedArcs {
        accepted: set.len(),
        requested: arcs.len(),
        set,
    }
}

/// One scene's assets, session and teardown state.
pub struct SceneWindow {
    tag: String,
    scope: &'static str,
    pick: Pick,
    arcs: Option<arc_set::ArcSet>,
    parse_rx: Arc<Mutex<Option<Parsed>>>,
    session: Option<Session>,
    requested_at: Instant,
    assets: AssetPhase,
    scene: ScenePhase,
    // one-shot logs
    warnings_logged: bool,
    built_logged: bool,
    frames_since_attach: u32,
    first_frame_logged: bool,
    collected_logged: bool,
    // teardown
    teardown_started: Option<Instant>,
    unlisted_frames: u32,
    queue_retries: u32,
}

impl SceneWindow {
    /// Game thread: start the parse thread over `loaded` and open the
    /// window. `tag` prefixes every log line; `scope` = "this song" /
    /// "this preview".
    pub fn start(
        tag: impl Into<String>,
        scope: &'static str,
        pick: Pick,
        loaded: LoadedArcs,
        opts: ParseOptions,
    ) -> SceneWindow {
        let tag = tag.into();
        let parse_rx: Arc<Mutex<Option<Parsed>>> = Arc::new(Mutex::new(None));
        if loaded.accepted > 0 {
            let rx = Arc::clone(&parse_rx);
            let pick_for_thread = pick.clone();
            let spawned = std::thread::Builder::new()
                .name("bg-dancers-parse".into())
                .spawn(move || {
                    let parsed = parse_pick(&pick_for_thread, &opts);
                    if let Ok(mut slot) = rx.lock() {
                        *slot = Some(parsed);
                    }
                });
            if spawned.is_err() {
                log_warn!(
                    "{}: parse thread could not be spawned -- no dancers {}",
                    tag,
                    scope
                );
            }
            log_info!(
                "{}: FileManager::Load accepted {} of {} arcs -- parsing + polling residency",
                tag,
                loaded.accepted,
                loaded.requested
            );
        } else {
            log_warn!(
                "{}: no arc loaded (see the scene3d WARNs above) -- no dancers {}",
                tag,
                scope
            );
        }
        SceneWindow {
            tag,
            scope,
            pick,
            arcs: Some(loaded.set),
            parse_rx,
            session: None,
            requested_at: Instant::now(),
            assets: if loaded.accepted > 0 {
                AssetPhase::Requested
            } else {
                AssetPhase::Abandoned
            },
            scene: ScenePhase::Live,
            warnings_logged: false,
            built_logged: false,
            frames_since_attach: 0,
            first_frame_logged: false,
            collected_logged: false,
            teardown_started: None,
            unlisted_frames: 0,
            queue_retries: 0,
        }
    }

    pub fn pick(&self) -> &Pick {
        &self.pick
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    pub fn session_mut(&mut self) -> Option<&mut Session> {
        self.session.as_mut()
    }

    pub fn assets(&self) -> AssetPhase {
        self.assets
    }

    pub fn scene(&self) -> ScenePhase {
        self.scene
    }

    pub fn requested_at(&self) -> Instant {
        self.requested_at
    }

    pub fn since_request_ms(&self) -> u64 {
        self.requested_at.elapsed().as_millis() as u64
    }

    pub fn frames_since_attach(&self) -> u32 {
        self.frames_since_attach
    }

    /// At least one instance is built (its node is live until torn down).
    pub fn has_built(&self) -> bool {
        self.session
            .as_ref()
            .map_or(false, |s| s.built().next().is_some())
    }

    /// Game thread, every frame while `Live`: Requested → Built/Abandoned.
    /// `make_session` turns the landed parse into the session (the caller
    /// decides style / hulls / slot base / pass mask / schedule). Returns
    /// whether anything is built, advancing the attach frame counter when so.
    pub fn drive_assets(
        &mut self,
        make_session: impl FnOnce(&Pick, Parsed, Instant) -> Session,
    ) -> bool {
        let since_request_ms = self.since_request_ms();

        // Parse result → session (once).
        if self.session.is_none() && self.assets == AssetPhase::Requested {
            let parsed = self.parse_rx.lock().ok().and_then(|mut g| g.take());
            if let Some(parsed) = parsed {
                if !self.warnings_logged {
                    self.warnings_logged = true;
                    for warn in &parsed.warnings {
                        log_warn!("{}: parse: {}", self.tag, warn);
                    }
                }
                log_info!(
                    "{}: parsed in {} ms -- {} stage part(s), {} dancer(s) with {:?} clip(s), {:?} part(s), shadow={}",
                    self.tag,
                    parsed.elapsed_ms,
                    parsed.stage_parts.len(),
                    parsed.dancers.len(),
                    parsed.dancers.iter().map(|d| d.clips.len()).collect::<Vec<_>>(),
                    parsed.dancers.iter().map(|d| d.parts.len()).collect::<Vec<_>>(),
                    parsed.shadow.is_some()
                );
                if parsed.stage_parts.is_empty() && parsed.dancers.is_empty() {
                    log_warn!("{}: nothing parsed -- no dancers {}", self.tag, self.scope);
                    self.assets = AssetPhase::Abandoned;
                } else {
                    self.session = Some(make_session(&self.pick, parsed, self.requested_at));
                }
            }
        }

        // Build what is resident.
        if self.assets == AssetPhase::Requested {
            if let Some(sess) = self.session.as_mut() {
                let progress = sess.build_pending(since_request_ms);
                if progress.built_now > 0 && self.frames_since_attach == 0 {
                    self.frames_since_attach = 1;
                }
                if sess.all_settled() {
                    self.assets = AssetPhase::Built;
                    sess.built_at = Some(Instant::now());
                    if !self.built_logged {
                        self.built_logged = true;
                        let (st, dn, pt, sh, hu) = sess.built_counts();
                        log_info!(
                            "{}: built {} ms after request -- {} instance(s) attached hidden ({} stage, {} dancer, {} part, {} shadow, {} hull), {} skipped",
                            self.tag,
                            since_request_ms,
                            sess.built().count(),
                            st,
                            dn,
                            pt,
                            sh,
                            hu,
                            sess.instances.len() - sess.built().count()
                        );
                    }
                }
            }
            if self.assets == AssetPhase::Requested && since_request_ms > RESIDENCY_TIMEOUT_MS {
                self.assets = AssetPhase::Abandoned;
                let pending: Vec<String> = self
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
                    "{}: residency timeout after {} ms -- still missing {:?}{}; whatever was built keeps running",
                    self.tag,
                    since_request_ms,
                    pending,
                    if self.session.is_none() {
                        " (parse thread never delivered)"
                    } else {
                        ""
                    }
                );
            }
        }

        let has_built = self.has_built();
        if has_built && self.frames_since_attach > 0 {
            self.frames_since_attach = self.frames_since_attach.saturating_add(1);
        }
        has_built
    }

    /// A node the ENGINE destroyed outside our teardown (dtor ran without a
    /// queued destroy): stop touching it, leak its block, say so once.
    pub fn park_engine_destroyed(&mut self) {
        let Some(sess) = self.session.as_mut() else {
            return;
        };
        for inst in sess.built_mut() {
            if inst.queued || inst.freed {
                continue;
            }
            // SAFETY: the node block is ours until `free_node_block`.
            if unsafe { node::is_destroyed(inst.node as *mut SceneNode) } {
                inst.queued = true;
                inst.freed = true; // block deliberately leaked (still linked?)
                log_warn!(
                    "{}: {} node 0x{:X} was destroyed by the ENGINE outside our teardown (its item is freed) -- instance parked, node block leaked",
                    self.tag,
                    inst.model_name,
                    inst.node
                );
            }
        }
    }

    /// Publish every built instance's pose for scene time `t` (hidden when
    /// `!visible`) and drop the node-level "force hidden" of every instance
    /// that has now been published at least once.
    pub fn publish(&mut self, t: f32, visible: bool) {
        let Some(sess) = self.session.as_mut() else {
            return;
        };
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

    /// Per-frame texture re-resolve for instances built before their DDS
    /// registered.
    pub fn retry_textures(&mut self) {
        let frames = self.frames_since_attach;
        let Some(sess) = self.session.as_mut() else {
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
                    "{}: {} material textures re-resolved {} ms after attach (total={} load={} re={} default={})",
                    self.tag,
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
                    "{}: {} -- {} material texture(s) STILL unregistered after {} frames -- stays untextured {}",
                    self.tag,
                    inst.model_name,
                    inst.textures_pending,
                    frames,
                    self.scope
                );
                inst.textures_pending = 0;
            }
        }
    }

    /// The "did the engine take our nodes" lines (Step 3 shapes).
    pub fn attached_diagnostics(&mut self) {
        let Some(stats) = scene_graph::graph_stats() else {
            return;
        };
        let wanted = self
            .session
            .as_ref()
            .map(|s| s.built().count())
            .unwrap_or(0);
        if !self.first_frame_logged && self.frames_since_attach >= 2 {
            self.first_frame_logged = true;
            log_info!(
                "{}: first frame after attach -- graph enabled={} visible-nodes={} items={} records={} (nodes attached so far: {})",
                self.tag,
                stats.enabled,
                stats.visible,
                stats.items,
                stats.records,
                wanted
            );
        }
        if !self.collected_logged && stats.items > 0 {
            self.collected_logged = true;
            log_info!(
                "{}: items collected by SceneGraph::update {} ms after request -- graph enabled={} visible-nodes={} items={} records={} (nodes attached: {})",
                self.tag,
                self.requested_at.elapsed().as_millis(),
                stats.enabled,
                stats.visible,
                stats.items,
                stats.records,
                wanted
            );
        }
        if !self.collected_logged
            && (self.frames_since_attach == NOT_COLLECTED_DIAG_FRAMES
                || self.frames_since_attach == NOT_COLLECTED_WARN_FRAMES)
        {
            let anomaly = stats.enabled || self.frames_since_attach == NOT_COLLECTED_WARN_FRAMES;
            let msg = format!(
                "{}: items not collected {} frames after attach -- graph enabled={} visible-nodes={} items={} (enabled=false ⇒ DPS has not reached step 5; enabled=true & visible=0 ⇒ pass-4 gate/visit; visible>0 & items=0 ⇒ item push)",
                self.tag, self.frames_since_attach, stats.enabled, stats.visible, stats.items
            );
            if anomaly {
                log_warn!("{}", msg);
            } else {
                log_info!("{}", msg);
            }
        }
    }

    /// Game thread: the window closed — start the teardown (or free the
    /// arcs right away when nothing was attached). `exit_label` names the
    /// exit in the log (`song-window exit`). `true` = a teardown is now in
    /// flight (the caller keeps driving frames); `false` = nothing to do.
    pub fn begin_teardown(&mut self, exit_label: &str) -> bool {
        if self.scene != ScenePhase::Live {
            return false;
        }
        let built: Vec<usize> = self
            .session
            .as_ref()
            .map(|s| s.built().map(|i| i.node).collect())
            .unwrap_or_default();
        if built.is_empty() {
            self.scene = ScenePhase::Done;
            if let Some(set) = self.arcs.take() {
                let n = set.len();
                arc_set::free(set);
                log_info!(
                    "{}: {} arc handle(s) freed at {} (no nodes)",
                    self.tag,
                    n,
                    exit_label
                );
            }
            return false;
        }
        if let Some(sess) = self.session.as_mut() {
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
        self.scene = ScenePhase::Detaching;
        self.teardown_started = Some(Instant::now());
        self.unlisted_frames = 0;
        log_info!(
            "{}: {} -- {} node(s) disabled, waiting for the engine's item list to drop them",
            self.tag,
            exit_label,
            self.session
                .as_ref()
                .map(|s| s.built().count())
                .unwrap_or(0)
        );
        true
    }

    /// Advance the teardown by one frame. `true` = the window is finished
    /// (caller frees/leaks the arcs via [`finish`](Self::finish)).
    pub fn drive_teardown(&mut self, label: &str) -> bool {
        match self.scene {
            ScenePhase::Live => false,
            ScenePhase::Done => true,
            ScenePhase::Detaching => {
                let elapsed = self
                    .teardown_started
                    .map(|t| t.elapsed().as_millis() as u64)
                    .unwrap_or(0);
                let Some(sess) = self.session.as_mut() else {
                    self.scene = ScenePhase::Done;
                    return true;
                };
                let any_listed = sess
                    .built()
                    .filter(|i| !i.queued)
                    .any(|i| scene_graph::item_listed(i.item as *const u8) != Some(false));
                if any_listed {
                    self.unlisted_frames = 0;
                } else {
                    self.unlisted_frames += 1;
                }
                if self.unlisted_frames >= UNLISTED_FRAMES_REQUIRED {
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
                        self.scene = ScenePhase::Destroying;
                        log_info!(
                            "{}: {} -- {} destroy(s) queued {} ms after window exit (items unlisted for {} frames)",
                            self.tag,
                            label,
                            sess.built().count(),
                            elapsed,
                            self.unlisted_frames
                        );
                        return false;
                    }
                    self.queue_retries += 1;
                    if self.queue_retries == 60 {
                        log_warn!(
                            "{}: queue_destroy refused 60 frames in a row for {} node(s) -- still retrying",
                            self.tag,
                            pending
                        );
                    }
                    if elapsed > TEARDOWN_TIMEOUT_MS {
                        log_warn!(
                            "{}: {} -- {} node(s) could not be queued for destroy within {} ms -- leaking them (disabled), freeing the arcs",
                            self.tag,
                            label,
                            pending,
                            elapsed
                        );
                        self.scene = ScenePhase::Done;
                        return true;
                    }
                    return false;
                }
                if elapsed > TEARDOWN_TIMEOUT_MS {
                    log_warn!(
                        "{}: {} -- an item is still referenced by the engine's item list {} ms after window exit -- leaking nodes+items+arcs (graph disabled with a stale list?)",
                        self.tag,
                        label,
                        elapsed
                    );
                    // Leak the arcs too: an item may still be read.
                    if let Some(a) = self.arcs.take() {
                        std::mem::forget(a);
                    }
                    self.scene = ScenePhase::Done;
                    return true;
                }
                false
            }
            ScenePhase::Destroying => {
                let elapsed = self
                    .teardown_started
                    .map(|t| t.elapsed().as_millis() as u64)
                    .unwrap_or(0);
                let Some(sess) = self.session.as_mut() else {
                    self.scene = ScenePhase::Done;
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
                        "{}: {} -- all {} node(s) destroyed by the engine flush {} ms after window exit -- node blocks freed",
                        self.tag,
                        label,
                        sess.built().count(),
                        elapsed
                    );
                    self.scene = ScenePhase::Done;
                    return true;
                }
                if elapsed > TEARDOWN_TIMEOUT_MS {
                    log_warn!(
                        "{}: {} -- {} dtor(s) not observed {} ms after window exit -- leaking those nodes+items, freeing the arcs (items are unlisted)",
                        self.tag,
                        label,
                        remaining,
                        elapsed
                    );
                    self.scene = ScenePhase::Done;
                    return true;
                }
                false
            }
        }
    }

    /// Free a finished window's arcs (unless the teardown leaked them) and
    /// log the texture balance.
    pub fn finish(self, label: &str) {
        match self.arcs {
            Some(set) => {
                let n = set.len();
                arc_set::free(set);
                log_info!(
                    "{}: {}{} arc handle(s) freed after the scene teardown; scene3d textures: {}",
                    self.tag,
                    label,
                    n,
                    texture::balance()
                );
            }
            None => {}
        }
        // The parse thread's result (if it still lands) is dropped with `self`.
    }

    /// A finished window whose arcs are released without the texture-balance
    /// line (a `Done` predecessor found when a new window opens).
    pub fn finish_silent(self) {
        if let Some(a) = self.arcs {
            arc_set::free(a);
        }
    }

    /// Give the arcs up for good (an orphan dropped for a newer one — its
    /// nodes may still be linked, so the arcs must outlive the process).
    pub fn forget_arcs(self) {
        if let Some(a) = self.arcs {
            std::mem::forget(a);
        }
    }

    /// Owner disable: the frame callback that would drive a teardown to
    /// completion is gone, so live nodes are DISABLED + hidden and LEAKED
    /// with their items and arcs — never freed, never a use-after-free.
    /// Returns whether anything was leaked (the caller logs the summary).
    pub fn neutralise(self) -> bool {
        if self.scene == ScenePhase::Live {
            if let Some(sess) = self.session.as_ref() {
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
        let had_nodes = self.has_built();
        if self.scene != ScenePhase::Done && had_nodes {
            // Nodes stay linked (disabled) in the engine tree with their
            // items; the arcs must outlive them.
            if let Some(a) = self.arcs {
                std::mem::forget(a);
            }
            true
        } else {
            if let Some(a) = self.arcs {
                arc_set::free(a);
            }
            false
        }
    }
}
