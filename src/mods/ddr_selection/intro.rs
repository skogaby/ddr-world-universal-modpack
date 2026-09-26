//! The legacy song intro: A3's READY! / HERE WE GO!! clips from
//! `dance_message000N` (a theme: its own `dance_message_vN`), and the
//! suppression of World's own intro while they play (its "READY?" panel, its
//! code-played `vo_ingame_ready`, its 5.0 s READY? dwell).
//!
//! World deleted A3's `ReadyGoActor` (its `dance_message` package has no
//! World consumer), so this module re-hosts it without a fabricated actor:
//!
//! * **Package.** `dance_message` turns legacy in the package helper only
//!   when this module is [`capable`] (policy adapter `ReadyGo`): the gameplay
//!   `LayoutActor` then loads `dance_message000N` and owns it (releases it at
//!   its finalize). The clips are raw AFP layers bound to that package
//!   (`bm2d_package::lookup_unowned`), created once the DancePlaySequence is
//!   past step 0 (the `LayoutActor` ready), group 5 / priority 5 like A3's
//!   CMovieClips, parked invisible at frame 0.
//! * **Triggers.** Every frame (`input_manager::on_frame`, game thread) the
//!   highest ControlMessageActor step (`song_reset::intro_cascade_step`: 1
//!   READY, 2 HERE, 3 OUT) drives the pure [`intro_logic::ReadyGo`] machine.
//!   Skin 1's HERE voice (`ACT3_1` / final stage `ACT4_2`) plays from the era
//!   bank; the other skins' voices are inside the clips.
//! * **World's panel.** World's stage panel (ShutterActor kind 3, the
//!   jacket / stage / difficulty screen between song select and the lanes)
//!   reveals with `stage_out` when the song starts, then shows its own
//!   "READY?" and "HERE WE GO!". It is dismissed with the game's bannerless
//!   `0x100c` (`services::shutter`, the quick-restart mechanism) as soon as
//!   the reveal has taken the panel elements away (`jacket_usr` gone, frame
//!   344) — before World's `ready` sprite (frame 350); backstops: that
//!   sprite already placed, `ready_loop` reached, or READY fired (A3's
//!   `0x100D`). See [`intro_logic::PanelView`].
//! * **Legacy panel.** When A3's stage root is live in World's stage slot
//!   (`panel::handles_dismissal`, set at its adoption — not merely when a
//!   session exists), `panel.rs` closes it at READY and skips World's 5.0 s
//!   READY? dwell (A3 had none — its panel holds itself); the World-art
//!   dismissal here stands down. With World's panel the dwell stays stock
//!   (skipping it cut World's panel to a flash, cabinet 2026-09-23).
//!
//! Layer-before-package rule: the layers are destroyed on the scene change
//! that leaves GAMEPLAY (the scene callback fires before
//! `createNextSequence` / `installSequence` tear the DancePlaySequence and
//! its `LayoutActor` down), when the package lookup stops returning the same
//! pointer, when the DancePlaySequence changes, and once both clips ended.
//!
//! Fail-open: without every requirement `dance_message` stays stock (World's
//! intro, no legacy clips). A clip that fails to create leaves the song on
//! World's intro (the panel is only dismissed once the legacy clips exist).

use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

use crate::core::signatures::SignatureStore;
use crate::services::bm2d_api::{self, AfpLayer};
use crate::services::{bm2d_package, input_manager, shutter, song_reset, stage_records};
use crate::{log_info, log_warn};

use super::intro_logic::{self, Action, ReadyGo, Stage};
use super::policy;

/// `afp_mc_op` goto-frame-label + play.
const MC_OP_GOTO_LABEL_PLAY: i32 = 0xF09;
/// A3's CMovieClip `SetView(5)` / `SetPriority(5)` for both clips (the
/// `CLayer` priority 5 is the raw display priority 95 — above A3's stage
/// panel at 94, below World's own shutter kinds at 97).
const LAYER_GROUP: u16 = 5;
const LAYER_CLAYER_PRIORITY: u16 = 5;
/// The standard post-create display attribute the game applies.
const ATTR_DISPLAY_SETUP: u32 = 0x200;
/// World panel elements used to see the reveal end (`jacket_usr`, removed at
/// the end of `stage_out`) and World's own READY? sprite.
const PANEL_ELEMENT: &str = "jacket_usr";
const WORLD_READY_SPRITE: &str = "ready";

static CAPABLE: AtomicBool = AtomicBool::new(false);
static FRAME_CB: Mutex<Option<usize>> = Mutex::new(None);
/// Scene tracking (written by the mod's scene callback).
static IN_GAMEPLAY: AtomicBool = AtomicBool::new(false);
/// The legacy clips exist for the current song (read by `code_se`).
static LIVE: AtomicBool = AtomicBool::new(false);
/// One-shot WARN latches (per session).
static WARNED: AtomicU32 = AtomicU32::new(0);
const W_CREATE: u32 = 1;
const W_DISMISS: u32 = 2;
const W_PACKAGE_LOST: u32 = 4;

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

struct Clip {
    layer: AfpLayer,
    mc: u32,
    end_frame: Option<u32>,
}

struct Session {
    dps: usize,
    package: *const u8,
    package_name: CString,
    skin: u8,
    ready: Option<Clip>,
    here: Option<Clip>,
    machine: ReadyGo,
    /// The stage panel's `ready_loop` frame, per clip id (read once).
    ready_loop: Option<(u32, Option<u32>)>,
    /// The panel layer on which `jacket_usr` was seen (element signal valid).
    elements_seen_on: Option<u32>,
    panel_done: bool,
}

unsafe impl Send for Session {}

fn warn_once(bit: u32) -> bool {
    WARNED.fetch_or(bit, Ordering::Relaxed) & bit == 0
}

/// Resolve everything the intro needs (mod init). `false` ⇒ `dance_message`
/// stays stock for the session.
pub fn init(signatures: &SignatureStore) -> bool {
    let mut missing = Vec::new();
    if !bm2d_api::afp_layers_available() {
        missing.push("AFP layer API");
    }
    if !bm2d_package::is_available() {
        missing.push("BM2D package registry");
    }
    if !song_reset::intro_cascade_available() {
        missing.push("ControlMessageActor cascade");
    }
    if !shutter::is_available() {
        missing.push("ShutterActor");
    }
    if signatures.ddr_sel_dps_ready_timer_off().is_none() {
        log_warn!(
            "DDR SELECTION: READY? dwell site unresolved -- legacy songs keep World's 5 s dwell"
        );
    }
    if missing.is_empty() {
        CAPABLE.store(true, Ordering::Release);
        true
    } else {
        log_warn!(
            "DDR SELECTION: legacy READY / HERE WE GO unavailable ({}) -- World's intro stays",
            missing.join(", ")
        );
        false
    }
}

/// The `ReadyGo` adapter resolved (policy: `dance_message` may turn legacy).
pub fn capable() -> bool {
    CAPABLE.load(Ordering::Acquire)
}

/// The legacy READY / HERE clips exist for the current song.
pub fn legacy_intro_live() -> bool {
    LIVE.load(Ordering::Acquire)
}

/// Register the per-frame driver (mod enable).
pub fn start() {
    if !capable() {
        return;
    }
    let Ok(mut cb) = FRAME_CB.lock() else {
        return;
    };
    if cb.is_none() {
        *cb = Some(input_manager::on_frame(std::sync::Arc::new(on_frame)));
    }
}

/// Unregister and tear down (mod disable).
pub fn stop() {
    if let Ok(mut cb) = FRAME_CB.lock() {
        if let Some(id) = cb.take() {
            input_manager::remove_frame_callback(id);
        }
    }
    teardown("mod disabled");
}

/// Scene callback (game thread, before `createNextSequence`): leaving
/// GAMEPLAY destroys the clips while the owning `LayoutActor` still lives.
pub fn on_scene_change(next: i32) {
    let gameplay = next == crate::types::scenes::scene::GAMEPLAY;
    IN_GAMEPLAY.store(gameplay, Ordering::Release);
    if !gameplay {
        teardown("left gameplay");
    }
}

/// Destroy the clips (if any) and forget the session.
pub fn teardown(reason: &str) {
    let session = match SESSION.lock() {
        Ok(mut g) => g.take(),
        Err(p) => p.into_inner().take(),
    };
    if let Some(mut s) = session {
        let had = s.ready.is_some() || s.here.is_some();
        destroy_clips(&mut s);
        if had {
            log_info!(
                "DDR SELECTION: legacy READY / HERE clips destroyed ({})",
                reason
            );
        }
    }
    set_live(false);
}

fn set_live(live: bool) {
    if LIVE.swap(live, Ordering::AcqRel) != live {
        super::sound::code_se::sync();
    }
}

fn destroy_clips(s: &mut Session) {
    for clip in [s.ready.take(), s.here.take()].into_iter().flatten() {
        let id = clip.layer.id();
        if !bm2d_api::destroy_layer(clip.layer) {
            log_warn!("DDR SELECTION: destroying intro layer 0x{:08X} failed", id);
        }
    }
}

fn on_frame() {
    let armed = super::armed_skin();
    if armed == 0 || !IN_GAMEPLAY.load(Ordering::Acquire) || !super::legacy_package("dance_message")
    {
        if LIVE.load(Ordering::Acquire) {
            teardown("disarmed");
        }
        return;
    }
    let Some(step) = song_reset::dps_step() else {
        return;
    };
    let Some(dps) = song_reset::live_dps() else {
        return;
    };

    let mut guard = match SESSION.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };

    // A different DancePlaySequence (should always come with a scene
    // change, which already tore down) — never keep layers across it.
    if guard.as_ref().is_some_and(|s| s.dps != dps as usize) {
        if let Some(mut s) = guard.take() {
            destroy_clips(&mut s);
        }
        drop(guard);
        set_live(false);
        return;
    }

    if guard.is_none() {
        if step < 1 {
            return; // the LayoutActor is not ready yet
        }
        *guard = create_session(dps as usize, armed);
        let live = guard.as_ref().is_some_and(|s| s.ready.is_some());
        drop(guard);
        set_live(live);
        return;
    }
    let Some(s) = guard.as_mut() else {
        return;
    };

    // The owner released the package (or it was replaced): drop the clips
    // before the deferred destroy can find them.
    if s.ready.is_some() || s.here.is_some() {
        let still = bm2d_package::lookup_unowned(&s.package_name).map(|p| p.ptr);
        if still != Some(s.package) {
            if warn_once(W_PACKAGE_LOST) {
                log_warn!(
                    "DDR SELECTION: {} vanished from the package registry -- intro clips dropped",
                    s.package_name.to_string_lossy()
                );
            }
            destroy_clips(s);
            s.machine = ReadyGo::new(true, intro_logic::CMA_READY);
            drop(guard);
            set_live(false);
            return;
        }
    }

    if s.ready.is_none() && s.machine.stage() != Stage::Done {
        return; // clips failed to create: World's intro stays
    }
    if s.ready.is_none() {
        return; // finished: the clips are gone
    }

    let cascade = song_reset::intro_cascade_step().unwrap_or(0);
    let voice = intro_logic::here_voice(s.skin, final_stage());
    for action in s.machine.advance(cascade, voice) {
        perform(s, action);
    }

    // A3's stage panel closes itself at READY (`panel.rs`); World's is
    // dismissed here.
    if !s.panel_done && !super::panel::handles_dismissal() {
        dismiss_world_panel(s, cascade >= intro_logic::CMA_READY);
    }

    if s.machine.stage() == Stage::Out {
        let ready_ended = clip_ended(s.ready.as_mut());
        let here_ended = clip_ended(s.here.as_mut());
        if s.machine.clips_ended(ready_ended, here_ended) {
            destroy_clips(s);
            log_info!("DDR SELECTION: legacy READY / HERE WE GO finished");
            // LIVE stays set: the song is still on the legacy intro path
            // (World's voice stays silenced until the song window ends).
        }
    }
}

fn create_session(dps: usize, skin: u8) -> Option<Session> {
    let policy::Decision::Legacy {
        arc_base, naming, ..
    } = policy::decide("dance_message", skin, super::adapters())
    else {
        return None;
    };
    let name = policy::package_name(arc_base, skin, naming);
    let package_name = CString::new(name.trim_end_matches('\0')).ok()?;
    let pkg = bm2d_package::lookup_unowned(&package_name)?;
    let first = song_reset::intro_cascade_step().unwrap_or(0);
    let ready = create_clip(pkg.afpu_package_id, "00_ready");
    let here = ready
        .as_ref()
        .and_then(|_| create_clip(pkg.afpu_package_id, "00_here"));
    if ready.is_none() && warn_once(W_CREATE) {
        log_warn!(
            "DDR SELECTION: could not create the {} READY clip -- World's intro stays",
            package_name.to_string_lossy()
        );
    }
    let machine = ReadyGo::new(here.is_some(), first);
    if ready.is_some() {
        log_info!(
            "DDR SELECTION: legacy READY / HERE clips ready ({}, HERE {}){}",
            package_name.to_string_lossy(),
            if here.is_some() { "yes" } else { "no" },
            if machine.stage() == Stage::Done {
                " -- the intro already passed READY, skipped"
            } else {
                ""
            }
        );
    }
    Some(Session {
        dps,
        package: pkg.ptr,
        package_name,
        skin,
        ready,
        here,
        machine,
        ready_loop: None,
        elements_seen_on: None,
        panel_done: false,
    })
}

fn create_clip(package_id: u32, template: &str) -> Option<Clip> {
    let layer = bm2d_api::create_layer_from_package(package_id, template)?;
    bm2d_api::layer_set_attribute(&layer, ATTR_DISPLAY_SETUP, ATTR_DISPLAY_SETUP);
    bm2d_api::layer_set_group(&layer, LAYER_GROUP);
    bm2d_api::layer_set_priority(
        &layer,
        super::panel_logic::clayer_priority(LAYER_CLAYER_PRIORITY),
    );
    bm2d_api::layer_play(&layer, 0.0);
    bm2d_api::layer_set_visible(&layer, false);
    let Some(mc) = bm2d_api::layer_find_child(layer.id(), "/") else {
        let _ = bm2d_api::destroy_layer(layer);
        return None;
    };
    let end_frame = bm2d_api::mc_frame_by_label(mc, c"end").filter(|&f| f > 0);
    Some(Clip {
        layer,
        mc,
        end_frame,
    })
}

fn perform(s: &mut Session, action: Action) {
    match action {
        Action::PlayReady => {
            if let Some(c) = &s.ready {
                bm2d_api::layer_set_visible(&c.layer, true);
                bm2d_api::layer_play(&c.layer, 1.0);
            }
            log_info!("DDR SELECTION: READY (legacy clip)");
        }
        Action::ReadyOut => {
            if let Some(c) = &s.ready {
                bm2d_api::mc_op_str(c.mc, MC_OP_GOTO_LABEL_PLAY, c"out");
            }
        }
        Action::PlayHere { voice } => {
            if let Some(c) = &s.here {
                bm2d_api::layer_set_visible(&c.layer, true);
                bm2d_api::layer_play(&c.layer, 1.0);
            }
            let voiced = voice.map(super::sound::play_era_cue);
            log_info!(
                "DDR SELECTION: HERE WE GO (legacy clip{})",
                match (voice, voiced) {
                    (Some(v), Some(true)) => format!(", voice {v}"),
                    (Some(v), _) => format!(", voice {v} unavailable"),
                    _ => String::new(),
                }
            );
        }
        Action::HereOut => {
            if let Some(c) = &s.here {
                bm2d_api::mc_op_str(c.mc, MC_OP_GOTO_LABEL_PLAY, c"out");
            }
        }
    }
}

/// A clip is done once it reached its `end` label (no label ⇒ done: the
/// out label already played). A missing clip counts as ended.
fn clip_ended(clip: Option<&mut Clip>) -> bool {
    let Some(c) = clip else {
        return true;
    };
    let Some(end) = c.end_frame else {
        return true;
    };
    bm2d_api::mc_current_frame(c.mc).is_none_or(|f| f >= end)
}

/// A3's final-stage test for the HERE voice (fail-closed ⇒ not final).
fn final_stage() -> bool {
    match (
        stage_records::stage_counter(),
        stage_records::max_stage_setting(),
        stage_records::final_stage_override(),
    ) {
        (Some(stage), Some(max), Some(ovr)) => intro_logic::is_final_stage(stage, max, ovr),
        _ => false,
    }
}

fn dismiss_world_panel(s: &mut Session, ready_fired: bool) {
    let snap = match shutter::snapshot() {
        Ok(Some(snap)) => snap,
        Ok(None) => {
            s.panel_done = true;
            return;
        }
        Err(why) => {
            if warn_once(W_DISMISS) {
                log_warn!("DDR SELECTION: shutter read failed ({why}) -- World's panel stays");
            }
            s.panel_done = true;
            return;
        }
    };
    if snap.fully_idle() {
        // Already gone (a stage panel never shown, or dismissed by someone
        // else — quick restart's fast path).
        if ready_fired {
            s.panel_done = true;
        }
        return;
    }
    let mc = shutter::active_clip_mc(&snap);
    let ready_loop = match (mc, s.ready_loop) {
        (Some(m), Some((cached, f))) if cached == m => f,
        (Some(m), _) => {
            let f = bm2d_api::mc_frame_by_label(m, c"ready_loop");
            s.ready_loop = Some((m, f));
            f
        }
        (None, _) => None,
    };
    let frame = mc.and_then(bm2d_api::mc_current_frame);
    let layer = shutter::active_clip_layer(&snap);
    let elements_present =
        layer.is_some_and(|l| bm2d_api::layer_find_child(l, PANEL_ELEMENT).is_some());
    if elements_present {
        s.elements_seen_on = layer;
    }
    let elements_seen = layer.is_some() && s.elements_seen_on == layer;
    let world_ready_shown =
        layer.is_some_and(|l| bm2d_api::layer_find_child(l, WORLD_READY_SPRITE).is_some());
    let view = intro_logic::PanelView {
        shutter_state: snap.state,
        stage_panel_alone: snap.stage_panel_alone(),
        elements_seen,
        elements_gone: !elements_present,
        world_ready_shown,
        clip_frame: frame,
        ready_loop_frame: ready_loop,
        ready_fired,
    };
    if !intro_logic::should_dismiss_world_panel(&view) {
        return;
    }
    s.panel_done = true;
    match shutter::send_dismiss(&snap) {
        Ok(()) => {
            let unblock = shutter::unblock_drain(snap.actor);
            log_info!(
                "DDR SELECTION: World's stage panel dismissed at frame {:?} (ready_loop {:?}, state {}, panel elements {}, World READY? {}, READY {}) -- drain {:?}",
                frame,
                ready_loop,
                snap.state,
                if !elements_seen {
                    "never seen"
                } else if elements_present {
                    "present"
                } else {
                    "gone"
                },
                if world_ready_shown { "shown" } else { "not shown" },
                if ready_fired { "fired" } else { "pending" },
                unblock
            );
        }
        Err(e) => {
            if warn_once(W_DISMISS) {
                log_warn!(
                    "DDR SELECTION: World's stage panel dismiss refused ({:?}) -- its READY? stays over the legacy clip",
                    e
                );
            }
        }
    }
}
