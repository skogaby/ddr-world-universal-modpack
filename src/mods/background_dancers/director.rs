//! The FrameState producer (design §4.3.4): once per frame on the game
//! thread, turn song time into every instance's pose and publish it on the
//! frame board for the node's `visit(2)`.
//!
//! Step 8 scope: stage parts (`_play_loop` wrapped, or the bind pose for
//! parts without a loop; a `_play_loop.sanm` beside it is sampled on the same
//! clock into the part's material parameters — UV scroll / colour / glow —
//! and rides the same board slot), dancer bodies (playlist clip at the schedule's
//! segment, A3 placement), each dancer's rigid accessory PARTS
//! (`part_world(mirror, bones[attach], body)`) and its `pl_shadow00` quad
//! (the A3 shadow rule over the ground bones, low-passed per frame). The
//! camanm camera (Step 9) plugs into the same loop. BIG HEAD (`style.rs`,
//! live) scales each dancer's `Head` subtree ×3 about the Head joint between
//! the evaluation and the body publish, so every derived instance inherits it
//! (`docs/big_head_mode_feasibility.md`).
//!
//! FLIGHT stages (`Session::flight_stage`): a `pre_*` part (the launch
//! platform, its sky, the intro space and the tunnel mouth) shows until
//! `Session::flight_switch` (the take-off's end), a `fly_*` part (the tunnel
//! and its space) from it on, on its own clock `t − switch`; a one-shot
//! `.anm` on either side holds its last frame. The `pre_*` parts follow
//! MUSIC FIT's intro script on the way (`director_math::intro_look`: the
//! platform dims, the mouth fades in and opens, the sky fades out) through
//! the instance tint; the floor shadows go with the platform
//! (`director_math::phase_clock`).
//!
//! FLIGHT CLOCKS: the caller passes SCHEDULE time `t`
//! (`Session::schedule_time`: the take-off on the real clock, the flight on
//! dance time) and the real clock `real_s`; the tunnel (`fly_*`) runs on the
//! real clock from the switch, the intro's stage effect (the sky burst,
//! `flight_fx::StageFx`, instances `InstanceKind::StageFx`) on the real
//! clock from the take-off's start, and the take-off is filmed by the
//! stage's `_intro` shots in order ([`camera_frame`]).
//!
//! FLIGHT EFFECTS (`flight_fx.rs`): every flying dancer runs player
//! (index mod 4)'s `boss_ddr3.TEB` effects — the leap burst from take-off
//! frame 544 at its Hips (position only) and feet, the orb / trail / hand
//! stars from the switch at its Hips and hands — on the real clock (the
//! game's 60 Hz effect tick), billboarded at this frame's camera
//! (`Session::fx_camera`), the world scrolling from the switch; the pools'
//! bones / record colours / flip-book offsets go out on the effect
//! instances' slots.
//!
//! WINX CLUB (`mod.rs`): each body's dynamic bones are solved after its clip
//! (`dynbones_logic::solve`), its kira loop sampled on the real clock;
//! [`winx_prepass`] resolves every armed transformation's look first (the
//! scene colour, which dancers show, the pose's switch on the next piece),
//! the twin is evaluated into `Session::twin_bones` (`produce_twin`), both
//! bodies publish the wipe word as the slot's AUX word, and
//! [`transform_camera`] replaces the stage camera while the event shot runs.
//!
//! No per-frame allocation: the session's scratch buffers are sized once;
//! the ground-point buffer is a fixed array; the effect pools are sized at
//! session build.

use crate::core::anm::camera::sample_camera;
use crate::core::anm::pose::evaluate_into;
use crate::core::anm::sanm;
use crate::services::scene3d::{frame_board, scene_graph};

use crate::core::anm::Mat4;
use crate::log_info;
use crate::services::scene3d::camera_math;

use super::director_math::{
    clip_frame, intro_look, mat_mul, part_phase, part_world, phase_clock, scale_subtree_about_root,
    shadow_step, shadow_target, shadow_world, transform_point, PartPhase, BLACK, IDENTITY, WHITE,
};
use super::dynbones_logic;
use super::flight_fx::{self, Affine, Camera};
use super::instance_plan::CHARA_BANK_CHENGE;
use super::movie_mode::SceneMask;
use super::schedule::ClipSel;
use super::session::{CameraSet, Clip, InstanceKind, InstanceStatus, Session};
use super::transform_logic::{self, Play};

/// Produce + publish every built instance's frame for song time `t`
/// (seconds since the song-start edge; DANCE time under the tempo map) and
/// `real_s` (the music count in real seconds — what the flight effects tick
/// on). `visible = false` publishes the
/// same poses hidden (the pre-edge / abandoned states). `mask` hides whole
/// instance kinds on top of that (Background Movies = FULLSCREEN: the stage
/// parts — and with them their hull twins, which read the part's slot — and
/// the floor shadows, while a fullscreen movie is the backdrop; MOVIE ONLY:
/// additionally the dancer bodies and their parts, whose twins read the
/// body's / part's slot too); the poses keep advancing underneath so a kind
/// that comes back (a course stage without a movie after one with) is
/// already current.
pub fn produce(sess: &mut Session, t: f32, real_s: f32, visible: bool, mask: SceneMask) {
    let hidden = !visible;
    let stage_hidden = hidden || !mask.stage;
    // FLIGHT stages: `pre_` / `fly_` parts switch at the take-off's end
    // (`Session::flight_switch`); there is no floor to shadow after it.
    let flight_stage = sess.flight_stage;
    let flight_switch = sess.flight_switch;
    let flying = flight_stage && flight_switch.is_some_and(|s| t >= s);
    let shadow_hidden = hidden || !mask.shadows || flying;
    let dancer_hidden = hidden || !mask.dancers;
    let n_dancers = sess.parsed.dancers.len();
    // BIG HEAD is live: read every frame (gameplay and previews share this
    // path), so a mod-menu toggle shows on the next frame.
    let head_scale = super::style::head_scale();
    // Winx Club: this frame's transformation looks (the scene colour fades
    // with any of them) and the dynamic bones' real dt.
    let scene_colour = winx_prepass(sess, t, real_s);
    let others_hidden = sess.winx.iter().any(|r| r.look.is_some_and(|l| !l.others));
    let dyn_dt = match sess.winx_last_real {
        Some(prev) if real_s >= prev && real_s - prev < 0.25 => real_s - prev,
        _ => 0.0,
    };
    let dyn_jump = sess
        .winx_last_real
        .is_some_and(|prev| real_s < prev || real_s - prev >= 0.25);
    sess.winx_last_real = Some(real_s);
    if dyn_jump {
        sess.reset_winx(false);
    }

    // Stage parts.
    for idx in 0..sess.instances.len() {
        let inst = &sess.instances[idx];
        if inst.status != InstanceStatus::Built || inst.queued {
            continue;
        }
        let InstanceKind::StagePart(i) = inst.kind else {
            continue;
        };
        let slot = inst.slot;
        let bone_count = inst.bone_count;
        let world = sess.initial_world(inst);
        let Some(p) = sess.parsed.stage_parts.get(i) else {
            continue;
        };
        // Flight phase: shown / its clock (a `fly_` part runs from the
        // switch; a one-shot clip — no loop bit: the tunnel mouth's opening —
        // holds its last frame instead of re-opening every loop). A `pre_`
        // part also follows MUSIC FIT's intro script (`intro_look`: the
        // platform dims, the sky fades out, the mouth fades in and opens).
        let (phase, (mut shown, mut pt)) = if flight_stage {
            let ph = part_phase(&p.part);
            (ph, phase_clock(ph, flight_switch, t))
        } else {
            (PartPhase::Always, (true, t))
        };
        // The tunnel runs on the REAL clock from the switch, like the Wii's
        // 60 Hz stage objects: its UV scrolls stay a steady flight at any
        // chart tempo (on dance time a fast song strobed the grid lines
        // into a back-and-forth shuffle, cabinet 2026-10-05). `real_s` is
        // the lifecycle's real clock: the raw music count (+ the take-off
        // length after a READY hold), never smoothed.
        // A flight stage's phase-less parts (FuruFuru Party's ring tunnels,
        // STG102 / 103: no platform, no pre_ / fly_ split) likewise run on
        // the real clock — on the Wii every stage object does.
        if phase == PartPhase::Fly {
            if let Some(sw) = flight_switch {
                pt = (real_s - sw).max(0.0);
            }
        } else if phase == PartPhase::Always && flight_stage && flight_switch.is_some() {
            pt = real_s;
        }
        let mut tint = WHITE;
        if phase == PartPhase::Pre {
            let look = intro_look(&p.part, pt);
            tint = look.tint;
            shown &= look.shown;
            pt = look.clock;
        }
        if scene_colour < 1.0 {
            tint = [
                tint[0] * scene_colour,
                tint[1] * scene_colour,
                tint[2] * scene_colour,
                tint[3],
            ];
        }
        let Session {
            scratch,
            bones,
            mat_params,
            ..
        } = sess;
        match &p.loop_clip {
            Some(clip) => {
                let loops = phase == PartPhase::Always || clip.anm.loops;
                let frame = clip_frame(pt, clip.anm.duration_s(), clip.anm.fps, loops);
                evaluate_into(
                    &clip.anm,
                    &clip.bytes,
                    frame,
                    &p.skeleton,
                    &p.seed,
                    scratch,
                    bones,
                );
            }
            None => {
                // Bind pose: the seed evaluated with no tracks.
                let n = bone_count.min(bones.len());
                for (k, m) in bones.iter_mut().take(n).enumerate() {
                    *m = p.skeleton.bind_world.get(k).copied().unwrap_or(IDENTITY);
                }
            }
        }
        // Material animation: its own clip length (normally the loop's), the
        // same dance clock, always wrapped.
        mat_params.clear();
        if let Some(mc) = &p.material_clip {
            let frame = clip_frame(pt, mc.sanm.duration_s(), mc.sanm.fps, true);
            sanm::sample_writes(&mc.sanm, &mc.bytes, frame, &mc.binding, |w| {
                if mat_params.len() < frame_board::MAX_MAT_PARAMS {
                    mat_params.push(frame_board::MatParam {
                        material: w.material,
                        index: w.index,
                        value: w.value,
                    });
                }
            });
        }
        frame_board::publish_with_materials(
            slot,
            &world,
            tint,
            stage_hidden || !shown,
            &bones[..bone_count.min(bones.len())],
            mat_params,
        );
    }

    // The flight intro's stage effect (the sky burst), on the intro clock
    // (real time since the take-off began).
    publish_stage_fx(sess, real_s, stage_hidden);

    // Dancers: evaluate the body ONCE, then derive its parts + shadow.
    for i in 0..n_dancers {
        let Some(body_idx) = sess
            .instances
            .iter()
            .position(|inst| inst.kind == InstanceKind::Dancer(i))
        else {
            continue;
        };
        let body_built = {
            let inst = &sess.instances[body_idx];
            inst.status == InstanceStatus::Built && !inst.queued
        };
        let any_child_built = sess.children.get(i).map_or(false, |c| {
            c.iter().any(|&k| {
                sess.instances
                    .get(k)
                    .map_or(false, |x| x.status == InstanceStatus::Built && !x.queued)
            })
        });
        if !body_built && !any_child_built {
            continue;
        }
        let Some(sched) = sess.schedule.as_ref() else {
            continue;
        };
        if i >= sched.dancer_count() {
            continue;
        }
        let pos = sched.at(i, t);
        let body_world = sess.dancer_body_world(i);
        let body_slot = sess.instances[body_idx].slot;
        let body_bone_count = sess.instances[body_idx].bone_count;
        // Winx Club: what the outfit / its twin play this frame.
        let look = sess.winx.get(i).and_then(|r| r.look);
        let outfit_play = look.map_or(Some(Play::Dance), |l| l.outfit);
        let twin_play = look.and_then(|l| l.twin);
        let me_hidden_by_other = others_hidden && look.map_or(true, |l| l.others);
        let colour = look.map_or(1.0, |l| l.colour).min(scene_colour);
        let segment_cut = sess
            .winx
            .get(i)
            .is_some_and(|r| r.last_segment.is_some_and(|s| s != pos.segment));
        if let Some(r) = sess.winx.get_mut(i) {
            r.last_segment = Some(pos.segment);
        }
        {
            let Some(d) = sess.parsed.dancers.get(i) else {
                continue;
            };
            let Session {
                scratch,
                bones,
                winx,
                ..
            } = sess;
            let clip = match outfit_play.unwrap_or(Play::Dance) {
                Play::Dance => d.clips.get(pos.clip).map(|c| (c, pos.local_t)),
                Play::Transform(lt) => d.winx.transform.as_ref().map(|c| (c, lt)),
                Play::Pose(lt) => d.winx.pose.as_ref().map(|c| (c, lt)),
            };
            let Some((clip, local_t)) =
                clip.or_else(|| d.clips.get(pos.clip).map(|c| (c, pos.local_t)))
            else {
                continue;
            };
            let loops = clip.anm.loops && matches!(outfit_play, Some(Play::Dance) | None);
            let frame = clip_frame(local_t, clip.anm.duration_s(), clip.anm.fps, loops);
            evaluate_into(
                &clip.anm,
                &clip.bytes,
                frame,
                &d.skeleton,
                &d.seed,
                scratch,
                bones,
            );
            // Winx Club dynamic bones: after the clip, before Big Head (the
            // head subtree scales the solved hair with it).
            if let (Some(rig), Some(r)) = (d.winx.dyn_rig.as_ref(), winx.get_mut(i)) {
                let play = play_tag(outfit_play);
                let teleport = segment_cut || r.last_play_body.is_some_and(|p| p != play);
                r.last_play_body = Some(play);
                if let Some(st) = r.dyn_body.as_mut() {
                    dynbones_logic::solve(rig, st, bones, dyn_dt, teleport);
                }
            }
            // Big Head: scale the Head subtree about the Head joint in the
            // freshly evaluated bones, BEFORE the body publish — the body,
            // its head parts (`part_world` reads `bones[attach]`) and every
            // hull twin (they read those slots) all inherit it; the shadow
            // reads translations only and the Head joint's does not move.
            // `evaluate_into` rewrites every bone each call, so this never
            // accumulates across frames or dancers.
            if head_scale != 1.0 {
                scale_subtree_about_root(bones, &d.head_subtree, head_scale);
            }
        }
        let wipe = look.and_then(|l| l.wipe);
        let twin_present = sess.parsed.dancers.get(i).is_some_and(|d| d.twin.is_some());
        if body_built {
            let n = body_bone_count.min(sess.bones.len());
            let shown = outfit_play.is_some() && !me_hidden_by_other;
            let (alpha, aux) = wipe_terms(sess, &body_world, wipe, true);
            let tint = [colour, colour, colour, alpha];
            let mats = body_material_params(sess, i, false, real_s);
            frame_board::publish_aux(
                body_slot,
                &body_world,
                tint,
                dancer_hidden || !shown,
                &sess.bones[..n],
                &sess.mat_params[..mats],
                &[],
                aux,
            );
        }
        tick_flight_fx(sess, i, t, real_s, &body_world);
        // The body's own wing sparkles (a standalone Fairy dancer).
        let body_shown = outfit_play.is_some() && !me_hidden_by_other && !dancer_hidden;
        tick_wing_fx(sess, i, false, body_shown, real_s, &body_world);

        // Winx Club: the Enchantix twin.
        let twin_active = twin_present && twin_play.is_some();
        if twin_present {
            produce_twin(
                sess,
                i,
                &pos,
                twin_play,
                colour,
                wipe,
                dancer_hidden || me_hidden_by_other,
                dyn_dt,
                segment_cut,
                head_scale,
                real_s,
                &body_world,
            );
        }

        // Children read the freshly evaluated `sess.bones` (the shadow: the
        // twin's once it has taken over).
        let Some(children) = sess.children.get(i).cloned() else {
            continue;
        };
        for k in children {
            let Some(inst) = sess.instances.get(k) else {
                continue;
            };
            if inst.status != InstanceStatus::Built || inst.queued {
                continue;
            }
            let slot = inst.slot;
            match inst.kind {
                InstanceKind::Part { dancer, part } if dancer == i => {
                    let Some(p) = sess.parsed.dancers.get(i).and_then(|d| d.parts.get(part)) else {
                        continue;
                    };
                    let bone = sess.bones.get(p.attach).copied().unwrap_or(IDENTITY);
                    let world = part_world(p.mirror, &bone, &body_world);
                    let tint = [colour, colour, colour, 1.0];
                    frame_board::publish(
                        slot,
                        &world,
                        tint,
                        dancer_hidden || outfit_play.is_none() || me_hidden_by_other,
                        &[IDENTITY],
                    );
                }
                InstanceKind::Shadow(dancer) if dancer == i => {
                    let Some(d0) = sess.parsed.dancers.get(i) else {
                        continue;
                    };
                    let use_twin = twin_active && outfit_play.is_none();
                    let (d, src) = match (use_twin, d0.twin.as_ref()) {
                        (true, Some(tw)) => (&tw.body, &sess.twin_bones),
                        _ => (d0, &sess.bones),
                    };
                    let mut ground = [[0.0f32; 3]; 5];
                    let mut n = 0usize;
                    for &g in d.ground.iter().take(ground.len()) {
                        if let Some(m) = src.get(g) {
                            ground[n] = [m[12], m[13], m[14]];
                            n += 1;
                        }
                    }
                    let (bind_y, anim_y) = match d.hips {
                        Some(h) => (
                            d.skeleton.bind_world.get(h).map_or(0.0, |m| m[13]),
                            src.get(h).map_or(0.0, |m| m[13]),
                        ),
                        None => (0.0, 0.0),
                    };
                    let Some((centre, target)) =
                        shadow_target(&ground[..n], bind_y, anim_y, d.shadow_scale)
                    else {
                        continue;
                    };
                    let shadow_scale = d0.shadow_scale;
                    let prev = sess.shadow_size.get(i).copied().unwrap_or(shadow_scale);
                    let size = shadow_step(prev, target);
                    if let Some(s) = sess.shadow_size.get_mut(i) {
                        *s = size;
                    }
                    let world = shadow_world(size, transform_point(&body_world, centre));
                    frame_board::publish(
                        slot,
                        &world,
                        BLACK,
                        shadow_hidden || (me_hidden_by_other && outfit_play.is_some()),
                        &[IDENTITY],
                    );
                }
                InstanceKind::Fx { dancer, pool } if dancer == i => {
                    let Session { fx, mat_params, .. } = sess;
                    let Some(frame) = fx
                        .get(i)
                        .and_then(Option::as_ref)
                        .and_then(|f| f.frames.get(pool))
                    else {
                        continue;
                    };
                    mat_params.clear();
                    for w in frame.mats.iter().take(frame_board::MAX_MAT_PARAMS) {
                        mat_params.push(frame_board::MatParam {
                            material: w.material,
                            index: w.index,
                            value: w.value,
                        });
                    }
                    frame_board::publish_full(
                        slot,
                        &IDENTITY,
                        WHITE,
                        dancer_hidden || frame.drawn == 0,
                        &frame.bones,
                        mat_params,
                        &frame.colours,
                    );
                }
                InstanceKind::CharaFx {
                    dancer,
                    twin,
                    bank,
                    pool,
                } if dancer == i => {
                    let Session {
                        winx, mat_params, ..
                    } = sess;
                    let Some(r) = winx.get(i) else {
                        continue;
                    };
                    let fxr = match (bank, twin) {
                        (CHARA_BANK_CHENGE, _) => r.chenge.as_ref(),
                        (_, true) => r.wing_twin.as_ref(),
                        (_, false) => r.wing_body.as_ref(),
                    };
                    let Some(frame) = fxr.and_then(|f| f.frames.get(pool)) else {
                        continue;
                    };
                    mat_params.clear();
                    for w in frame.mats.iter().take(frame_board::MAX_MAT_PARAMS) {
                        mat_params.push(frame_board::MatParam {
                            material: w.material,
                            index: w.index,
                            value: w.value,
                        });
                    }
                    frame_board::publish_full(
                        slot,
                        &IDENTITY,
                        WHITE,
                        dancer_hidden || frame.drawn == 0,
                        &frame.bones,
                        mat_params,
                        &frame.colours,
                    );
                }
                _ => {}
            }
        }
    }
}

/// A small tag of what a body plays (the solver's teleport rule: a change
/// of clip family is a cut).
fn play_tag(p: Option<Play>) -> u8 {
    match p {
        None | Some(Play::Dance) => 0,
        Some(Play::Transform(_)) => 1,
        Some(Play::Pose(_)) => 2,
    }
}

/// Winx Club, before anything is published: every armed transformation's
/// look at this frame (`transform_logic::look`, stored in
/// `WinxRun::look`), the pose's switch resolved on the next choreography
/// piece of the clip playing underneath, the INFO lines. Returns the scene
/// colour (the darkest fade of any look).
fn winx_prepass(sess: &mut Session, t: f32, real_s: f32) -> f32 {
    let mut colour = 1.0f32;
    for i in 0..sess.winx.len() {
        let Some(run) = sess.winx.get(i).and_then(|r| r.transform) else {
            if let Some(r) = sess.winx.get_mut(i) {
                r.look = None;
            }
            continue;
        };
        let transform_s = sess
            .parsed
            .dancers
            .get(i)
            .and_then(|d| d.winx.transform.as_ref())
            .map_or(transform_logic::DEFAULT_TRANSFORM_S, |c| c.anm.duration_s());
        let mut run = run;
        let mut look = transform_logic::look(run.r0, transform_s, run.switch_real, real_s);
        if look.is_none() {
            // A rewind past the trigger: re-armed (the combo starts over too).
            log_info!(
                "BackgroundDancers: Enchantix transformation of dancer {} undone (the clock went back before its trigger at {:.2} s)",
                i + 1,
                run.r0
            );
            if let Some(r) = sess.winx.get_mut(i) {
                r.transform = None;
                r.look = None;
            }
            continue;
        }
        if look.is_some_and(|l| l.phase == transform_logic::Phase::Pose) {
            if run.switch_dance.is_none() {
                if let Some(pos) = sess
                    .schedule
                    .as_ref()
                    .filter(|s| i < s.dancer_count())
                    .map(|s| s.at(i, t))
                {
                    run.switch_dance = Some(transform_logic::next_piece_boundary(
                        t,
                        pos.segment_start,
                        pos.segment_end,
                    ));
                }
            }
            if run.switch_dance.is_some_and(|sw| t >= sw) {
                run.switch_real = Some(real_s);
                look = transform_logic::look(run.r0, transform_s, run.switch_real, real_s);
                log_info!(
                    "BackgroundDancers: Enchantix transformation of dancer {} -- switch at {:.2} s (dance {:.2} s): {} dances on",
                    i + 1,
                    real_s,
                    t,
                    sess.parsed
                        .dancers
                        .get(i)
                        .and_then(|d| d.twin.as_ref())
                        .map_or("?", |tw| tw.body.key.as_str())
                );
            }
        }
        if let Some(l) = look {
            colour = colour.min(l.colour);
        }
        if let Some(r) = sess.winx.get_mut(i) {
            r.transform = Some(run);
            r.look = look;
        }
    }
    colour
}

/// The wipe's `(tint alpha, AUX word)` for a body this frame: the shader
/// wipe (`transform_logic::wipe_word` of the line projected through this
/// frame's camera) when the containers are served, else a crossfade on the
/// tint alpha; `(1, 0)` outside the wipe.
fn wipe_terms(sess: &Session, body_world: &Mat4, wipe: Option<f32>, outfit: bool) -> (f32, f32) {
    let Some(p) = wipe else {
        return (1.0, 0.0);
    };
    if !sess.wipe_ok {
        let a = transform_logic::crossfade_alpha(p);
        return (if outfit { 1.0 - a } else { a }, 0.0);
    }
    let y = transform_logic::wipe_units(p) * WINX_METRES_PER_UNIT;
    let world = transform_point(body_world, [0.0, y, 0.0]);
    let ndc = match sess.fx_camera.as_ref() {
        Some(c) => {
            let (view, proj) = camera_math::view_proj(&c.frustum());
            let v = camera_math::transform_point(&view, world);
            let clip = camera_math::transform_point(&proj, [v[0], v[1], v[2]]);
            if clip[3].abs() > 1e-6 {
                clip[1] / clip[3]
            } else {
                0.0
            }
        }
        None => 0.0,
    };
    (1.0, transform_logic::wipe_word(ndc, outfit))
}

/// Metres per zan unit (`zan_dump.GAME_SCALE`: the Hips at 0.97 m = 8.593
/// units) — the Winx wipe line and effects.
pub const WINX_METRES_PER_UNIT: f32 = 0.970 / 8.593;

/// Sample a body's material loop (the kira overlays' UV scroll) on the real
/// clock into `sess.mat_params`; returns the count.
fn body_material_params(sess: &mut Session, i: usize, twin: bool, real_s: f32) -> usize {
    let Session {
        parsed, mat_params, ..
    } = sess;
    mat_params.clear();
    let Some(d) = parsed.dancers.get(i) else {
        return 0;
    };
    let body = if twin {
        match d.twin.as_ref() {
            Some(t) => &t.body,
            None => return 0,
        }
    } else {
        d
    };
    let Some(mc) = body.winx.material_clip.as_ref() else {
        return 0;
    };
    let frame = clip_frame(real_s.max(0.0), mc.sanm.duration_s(), mc.sanm.fps, true);
    sanm::sample_writes(&mc.sanm, &mc.bytes, frame, &mc.binding, |w| {
        if mat_params.len() < frame_board::MAX_MAT_PARAMS {
            mat_params.push(frame_board::MatParam {
                material: w.material,
                index: w.index,
                value: w.value,
            });
        }
    });
    mat_params.len()
}

/// Evaluate + publish dancer `i`'s Enchantix twin: hidden until the
/// transformation; its own `transform` / `pose` clips during it; after the
/// switch the OUTFIT's playlist clip at the schedule position (retargeted,
/// lifted). Its dynamic bones, its effects (the transformation effect riding
/// the wipe line, the wing sparkles) tick here too.
#[allow(clippy::too_many_arguments)]
fn produce_twin(
    sess: &mut Session,
    i: usize,
    pos: &super::schedule::DancePos,
    play: Option<Play>,
    colour: f32,
    wipe: Option<f32>,
    hidden: bool,
    dyn_dt: f32,
    segment_cut: bool,
    head_scale: f32,
    real_s: f32,
    body_world: &Mat4,
) {
    let Some(twin_idx) = sess
        .instances
        .iter()
        .position(|inst| inst.kind == InstanceKind::Twin(i))
    else {
        return;
    };
    let (slot, bone_count, built) = {
        let inst = &sess.instances[twin_idx];
        (
            inst.slot,
            inst.bone_count,
            inst.status == InstanceStatus::Built && !inst.queued,
        )
    };
    {
        let Session {
            parsed,
            scratch,
            twin_bones,
            winx,
            ..
        } = sess;
        let Some(tw) = parsed.dancers.get(i).and_then(|d| d.twin.as_deref()) else {
            return;
        };
        let b = &tw.body;
        let clip = match play {
            Some(Play::Dance) => tw.clips.get(pos.clip).map(|c| (c, pos.local_t, true)),
            Some(Play::Transform(lt)) => b.winx.transform.as_ref().map(|c| (c, lt, false)),
            // hidden until the transformation: posed at its first frame
            None => b.winx.transform.as_ref().map(|c| (c, 0.0, false)),
            Some(Play::Pose(lt)) => b.winx.pose.as_ref().map(|c| (c, lt, false)),
        };
        if let Some((clip, local_t, dance)) = clip {
            let loops = dance && clip.anm.loops;
            let frame = clip_frame(local_t, clip.anm.duration_s(), clip.anm.fps, loops);
            evaluate_into(
                &clip.anm,
                &clip.bytes,
                frame,
                &b.skeleton,
                &b.seed,
                scratch,
                twin_bones,
            );
            if dance && tw.lift != 0.0 {
                let n = b.skeleton.bone_count().min(twin_bones.len());
                for m in twin_bones.iter_mut().take(n) {
                    m[13] += tw.lift;
                }
            }
        }
        if let (Some(rig), Some(r)) = (b.winx.dyn_rig.as_ref(), winx.get_mut(i)) {
            let tag = play_tag(play);
            let teleport = segment_cut || r.last_play_twin.is_some_and(|p| p != tag);
            r.last_play_twin = Some(tag);
            if let Some(st) = r.dyn_twin.as_mut() {
                if play.is_some() {
                    dynbones_logic::solve(rig, st, twin_bones, dyn_dt, teleport);
                }
            }
        }
        if head_scale != 1.0 {
            scale_subtree_about_root(twin_bones, &b.head_subtree, head_scale);
        }
    }
    let shown = play.is_some() && !hidden;
    if built {
        let n = bone_count.min(sess.twin_bones.len());
        let (alpha, aux) = wipe_terms(sess, body_world, wipe, false);
        let tint = [colour, colour, colour, alpha];
        let mats = body_material_params(sess, i, true, real_s);
        frame_board::publish_aux(
            slot,
            body_world,
            tint,
            !shown,
            &sess.twin_bones[..n],
            &sess.mat_params[..mats],
            &[],
            aux,
        );
    }
    // The transformation effect: at the wipe line (+0.2 units), fading out
    // over the pose.
    let effect = sess.winx.get(i).and_then(|r| r.look).and_then(|l| l.effect);
    let line_p = wipe.unwrap_or(1.0);
    {
        let Session {
            parsed,
            winx,
            fx_camera,
            ..
        } = sess;
        if let (Some(r), Some(fx)) = (
            winx.get_mut(i),
            parsed
                .dancers
                .get(i)
                .and_then(|d| d.twin.as_ref())
                .and_then(|t| t.body.winx.chenge.as_ref()),
        ) {
            if let Some(j) = r.chenge.as_mut() {
                let k = fx.layout.metres_per_unit;
                let cam = fx_cam(fx_camera.as_ref(), k);
                let at = Affine {
                    r: flight_fx::M3_IDENTITY,
                    t: [
                        body_world[12] / k,
                        body_world[13] / k
                            + transform_logic::wipe_units(line_p)
                            + transform_logic::EFFECT_LIFT_UNITS,
                        body_world[14] / k,
                    ],
                };
                j.tick(
                    &fx.teb,
                    &fx.layout,
                    effect.is_some() && !hidden,
                    real_s,
                    &[at],
                    &cam,
                    effect.unwrap_or(0.0),
                );
            }
        }
    }
    tick_wing_fx(sess, i, true, shown, real_s, body_world);
}

fn fx_cam(c: Option<&scene_graph::CamSample>, k: f32) -> Camera {
    match c {
        Some(c) => Camera::look_at_world(c.eye, c.target, c.up, k),
        None => Camera::look_at_world([0.0, 1.6, 5.0], [0.0, 0.9, 0.0], [0.0, 1.0, 0.0], k),
    }
}

/// The wing sparkles of dancer `i`'s body (`twin` = its twin's): at both
/// `DB_*wing_02` joints (where the wings' `FX_wing` markers sit),
/// continuous while the body shows (research §7.4: the Wii never plays
/// this bank — the binding is the port's).
fn tick_wing_fx(
    sess: &mut Session,
    i: usize,
    twin: bool,
    on: bool,
    real_s: f32,
    body_world: &Mat4,
) {
    let Session {
        parsed,
        winx,
        bones,
        twin_bones,
        fx_camera,
        ..
    } = sess;
    let Some(d) = parsed.dancers.get(i) else {
        return;
    };
    let (body, src) = if twin {
        match d.twin.as_ref() {
            Some(t) => (&t.body, &*twin_bones),
            None => return,
        }
    } else {
        (d, &*bones)
    };
    let (Some(tips), Some(fx), Some(r)) = (
        body.winx.wing_tips,
        body.winx.wing.as_ref(),
        winx.get_mut(i),
    ) else {
        return;
    };
    let j = if twin {
        r.wing_twin.as_mut()
    } else {
        r.wing_body.as_mut()
    };
    let Some(j) = j else {
        return;
    };
    let k = fx.layout.metres_per_unit;
    let mut at = [Affine::IDENTITY; 2];
    for (dst, &(b, _len)) in at.iter_mut().zip(tips.iter()) {
        // at the joint itself: the wings' own `FX_wing` markers sit at each
        // DB_*wing_02 origin (wing sets 02–06)
        let m = mat_mul(&src.get(b).copied().unwrap_or(IDENTITY), body_world);
        *dst = Affine::from_world_row(&m, k);
    }
    let cam = fx_cam(fx_camera.as_ref(), k);
    j.tick(&fx.teb, &fx.layout, on, real_s, &at, &cam, 1.0);
}

/// The transformation's event camera (GAME_DEF_CAM `/#3/#0`, shipped with
/// the outfit) while a transformation films (`Look::camera`), moved with the
/// dancer's placement; `None` otherwise — the stage set films.
pub fn transform_camera(sess: &Session, real_s: f32) -> Option<scene_graph::CamSample> {
    let _ = real_s;
    for (i, r) in sess.winx.iter().enumerate() {
        let Some(lt) = r.look.and_then(|l| l.camera) else {
            continue;
        };
        let clip = sess.parsed.dancers.get(i)?.winx.camera.as_ref()?;
        let frame = clip_frame(lt.max(0.0), clip.anm.duration_s(), clip.anm.fps, false);
        let c = sample_camera(&clip.anm, &clip.bytes, frame, 1.0, 1.0);
        let w = sess.dancer_body_world(i);
        let off = [w[12], 0.0, w[14]];
        let add = |p: [f32; 3]| [p[0] + off[0], p[1] + off[1], p[2] + off[2]];
        return Some(scene_graph::CamSample {
            eye: add(c.eye),
            target: add(c.target),
            up: c.up,
            l: c.l,
            r: c.r,
            b: c.b,
            t: c.t,
            near: c.near,
            far: c.far,
        });
    }
    None
}

/// Tick the flight intro's stage effect and publish its pool instances.
fn publish_stage_fx(sess: &mut Session, intro_s: f32, hidden: bool) {
    let Session {
        stage_fx,
        parsed,
        fx_camera,
        instances,
        mat_params,
        ..
    } = sess;
    let (Some(sfx), Some(assets)) = (stage_fx.as_mut(), parsed.flight_fx.as_ref()) else {
        return;
    };
    let Some(teb) = assets.stage_teb.as_ref() else {
        return;
    };
    let k = assets.layout.metres_per_unit;
    let cam = match fx_camera.as_ref() {
        Some(c) => Camera::look_at_world(c.eye, c.target, c.up, k),
        None => Camera::look_at_world([0.0, 1.6, 5.0], [0.0, 0.9, 0.0], [0.0, 1.0, 0.0], k),
    };
    sfx.tick(teb, &assets.layout, intro_s, &cam);
    for inst in instances.iter() {
        let InstanceKind::StageFx(pool) = inst.kind else {
            continue;
        };
        if inst.status != InstanceStatus::Built || inst.queued {
            continue;
        }
        let Some(frame) = sfx.frames.get(pool) else {
            continue;
        };
        mat_params.clear();
        for w in frame.mats.iter().take(frame_board::MAX_MAT_PARAMS) {
            mat_params.push(frame_board::MatParam {
                material: w.material,
                index: w.index,
                value: w.value,
            });
        }
        frame_board::publish_full(
            inst.slot,
            &IDENTITY,
            WHITE,
            hidden || frame.drawn == 0,
            &frame.bones,
            mat_params,
            &frame.colours,
        );
    }
}

/// Dancer `i`'s flight effects for this frame (no-op without them): its
/// attach joints from the freshly evaluated `sess.bones` (model space) under
/// `body_world`, the camera of this frame, the leap / switch times.
fn tick_flight_fx(sess: &mut Session, i: usize, t: f32, real_s: f32, body_world: &[f32; 16]) {
    let Session {
        fx,
        parsed,
        bones,
        fx_leap,
        fx_camera,
        flight_switch,
        ..
    } = sess;
    let (Some(Some(dfx)), Some(assets), Some(switch)) =
        (fx.get_mut(i), parsed.flight_fx.as_ref(), *flight_switch)
    else {
        return;
    };
    let Some(joints_idx) = parsed.dancers.get(i).and_then(|d| d.fx_joints) else {
        return;
    };
    let k = assets.layout.metres_per_unit;
    let mut joints = [Affine::IDENTITY; 5];
    for (dst, &b) in joints.iter_mut().zip(joints_idx.iter()) {
        let m = bones.get(b).copied().unwrap_or(IDENTITY);
        *dst = Affine::from_world_row(&mat_mul(&m, body_world), k);
    }
    let cam = match fx_camera.as_ref() {
        Some(c) => Camera::look_at_world(c.eye, c.target, c.up, k),
        None => Camera::look_at_world([0.0, 1.6, 5.0], [0.0, 0.9, 0.0], [0.0, 1.0, 0.0], k),
    };
    let leap = fx_leap.get(i).copied().unwrap_or(switch).min(switch);
    dfx.tick(
        &assets.teb,
        &assets.layout,
        t,
        real_s,
        leap,
        switch,
        &joints,
        &cam,
    );
}

/// The camera for song time `t` (design §4.3.3/§4.3.4): advance the A3
/// stage-mode event loop of `set` from its last frame (or re-simulate from 0
/// after a song (re)start / any backwards jump / the first frame this set
/// films), then sample the selected `.camanm` at its local time. `None`
/// without that set (the caller keeps / writes the fixed fallback camera).
pub fn camera_frame(sess: &mut Session, t: f32, set: CameraSet) -> Option<scene_graph::CamSample> {
    // FLIGHT: the take-off is filmed by its own shots, in order (main.dol's
    // intro camera, STG201_CAM00_01..03 = 3 + 4 + 3 s = the 600-frame
    // intro), then the stage set takes over at the switch.
    if let Some(c) = intro_camera(sess, t) {
        return Some(c);
    }
    let dance = sess.schedule.as_ref()?;
    let (sched, state, cams) = match set {
        CameraSet::Stage => (
            sess.camera.as_ref()?,
            &mut sess.camera_state,
            sess.parsed.cameras.as_ref()?,
        ),
        CameraSet::Movie => (
            sess.movie_camera.as_ref()?,
            &mut sess.movie_camera_state,
            sess.parsed.movie_cameras.as_ref()?,
        ),
    };
    let st = match *state {
        Some((prev, prev_t)) if t >= prev_t => sched.advance(&prev, prev_t, t, dance),
        _ => sched.at(t, dance),
    };
    *state = Some((st, t));
    let clip: &Clip = match st.clip {
        ClipSel::Main(i) => cams.main.get(i % cams.main.len().max(1))?,
        ClipSel::Non(i) => {
            if cams.non.is_empty() {
                cams.main.get(st.main_index % cams.main.len().max(1))?
            } else {
                cams.non.get(i % cams.non.len())?
            }
        }
    };
    let frame = clip_frame(
        t - st.clip_start,
        clip.anm.duration_s(),
        clip.anm.fps,
        clip.anm.loops,
    );
    let c = sample_camera(&clip.anm, &clip.bytes, frame, 1.0, 1.0);
    Some(scene_graph::CamSample {
        eye: c.eye,
        target: c.target,
        up: c.up,
        l: c.l,
        r: c.r,
        b: c.b,
        t: c.t,
        near: c.near,
        far: c.far,
    })
}

/// The take-off shot at schedule time `t` (`None` outside a flight's
/// take-off, without intro shots, or once they ran out before the switch —
/// FuruFuru Party's one 3 s shot: the stage's own cycle takes over, as the
/// Wii's camera controller leaves its intro mode): shot k from the sum of
/// the earlier shots' lengths.
fn intro_camera(sess: &Session, t: f32) -> Option<scene_graph::CamSample> {
    let sw = sess.flight_switch.filter(|_| sess.pick.flight)?;
    if t >= sw {
        return None;
    }
    let intro = &sess.parsed.cameras.as_ref()?.intro;
    let mut start = 0.0f32;
    for clip in intro.iter() {
        let d = clip.anm.duration_s();
        if t < start + d {
            let frame = clip_frame((t - start).max(0.0), d, clip.anm.fps, false);
            let c = sample_camera(&clip.anm, &clip.bytes, frame, 1.0, 1.0);
            return Some(scene_graph::CamSample {
                eye: c.eye,
                target: c.target,
                up: c.up,
                l: c.l,
                r: c.r,
                b: c.b,
                t: c.t,
                near: c.near,
                far: c.far,
            });
        }
        start += d;
    }
    None
}

/// The camera timeline of `set` over the first `until_s` seconds — one entry
/// per dance cut: `k@cut: <shot just before> -> <shot at the cut>` (the
/// dev-mode song-start log).
pub fn camera_timeline(sess: &Session, until_s: f32, set: CameraSet) -> String {
    let sched = match set {
        CameraSet::Stage => sess.camera.as_ref(),
        CameraSet::Movie => sess.movie_camera.as_ref(),
    };
    let (Some(dance), Some(sched)) = (sess.schedule.as_ref(), sched) else {
        return String::from("(no camera schedule)");
    };
    let name = |t: f32| -> String {
        let st = sched.at(t, dance);
        sched.clip_of(&st).name.clone()
    };
    let mut out = format!("t0: {}", name(0.0));
    for (k, c) in dance.cuts_until(until_s) {
        out.push_str(&format!(
            " | {k}@{c:.1}s: {} -> {}",
            name(c - 0.01),
            name(c)
        ));
    }
    out
}

/// Publish every built instance hidden with its current bones untouched
/// (teardown start / abandon): the item keeps rendering nothing until the
/// node is disabled.
pub fn hide_all(sess: &mut Session) {
    for idx in 0..sess.instances.len() {
        if sess.instances[idx].status != InstanceStatus::Built {
            continue;
        }
        // Hull twins share their body's slot — the body's publish covers them.
        if !sess.instances[idx].kind.owns_slot() {
            continue;
        }
        let slot = sess.instances[idx].slot;
        let world = sess.initial_world(&sess.instances[idx]);
        let tint = if matches!(sess.instances[idx].kind, InstanceKind::Shadow(_)) {
            BLACK
        } else {
            WHITE
        };
        frame_board::publish(slot, &world, tint, true, &[]);
    }
}

#[cfg(test)]
mod tests {
    use super::super::director_math;
    use crate::core::anm::{camera, mat_mul, sample};
    use crate::services::scene3d::scene_graph;

    /// The two `CamSample` types must stay field-identical (the director
    /// copies field-for-field).
    #[test]
    fn cam_sample_shapes_agree() {
        let slots = camera::CamSlots {
            q: [0.0, 0.0, 0.0, 1.0],
            pos_cm: [100.0, 160.0, 500.0],
            fov_v_deg: 41.53,
            near: 0.1,
            far: 32768.0,
            aspect_file: 4.0 / 3.0,
            present: [true; 6],
        };
        let c = camera::camera_from_slots(&slots, 1.0, 1.0);
        let g = scene_graph::CamSample {
            eye: c.eye,
            target: c.target,
            up: c.up,
            l: c.l,
            r: c.r,
            b: c.b,
            t: c.t,
            near: c.near,
            far: c.far,
        };
        assert_eq!(g.eye, [1.0, 1.6, 5.0]);
        assert!((g.target[2] - (5.0 - 10.0)).abs() < 1e-5);
        assert_eq!(g.up, [0.0, 1.0, 0.0]);
        assert!((g.r + g.l).abs() < 1e-7 && (g.t + g.b).abs() < 1e-7);
        assert!((g.t * 16.0 / 9.0 - g.r).abs() < 1e-6);
        assert_eq!((g.near, g.far), (0.1, 32768.0));
    }

    /// The director's mountable `clip_time` copy must stay identical to the
    /// codec's.
    #[test]
    fn clip_time_copies_agree() {
        for (t, dur, loops) in [
            (0.5f32, 2.0f32, true),
            (2.5, 2.0, true),
            (-0.5, 2.0, true),
            (4.0, 2.0, true),
            (2.5, 2.0, false),
            (-1.0, 2.0, false),
            (2.0, 2.0, false),
            (1.0, 0.0, true),
        ] {
            assert_eq!(
                director_math::clip_time(t, dur, loops),
                sample::clip_time(t, dur, loops)
            );
        }
    }

    /// The director's mountable `mat_mul` copy must stay identical to the
    /// codec's (same row-vector "apply a then b" convention).
    #[test]
    fn mat_mul_copies_agree() {
        let a: [f32; 16] = [
            0.36, -0.48, 0.8, 0.0, //
            0.8, 0.6, 0.0, 0.0, //
            -0.48, 0.64, 0.6, 0.0, //
            1.5, -2.0, 0.25, 1.0,
        ];
        let b: [f32; 16] = [
            0.9, 0.0, 0.0, 0.0, //
            0.0, 0.9, 0.0, 0.0, //
            0.0, 0.0, 0.9, 0.0, //
            -0.8, 0.0, 0.0, 1.0,
        ];
        let ours = director_math::mat_mul(&a, &b);
        let theirs = mat_mul(&a, &b);
        assert!(ours
            .iter()
            .zip(theirs.iter())
            .all(|(x, y)| (x - y).abs() < 1e-6));
    }
}
