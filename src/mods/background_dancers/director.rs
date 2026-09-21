//! The FrameState producer (design §4.3.4): once per frame on the game
//! thread, turn song time into every instance's pose and publish it on the
//! frame board for the node's `visit(2)`.
//!
//! Step 8 scope: stage parts (`_play_loop` wrapped, or the bind pose for
//! parts without a loop), dancer bodies (playlist clip at the schedule's
//! segment, A3 placement), each dancer's rigid accessory PARTS
//! (`part_world(mirror, bones[attach], body)`) and its `pl_shadow00` quad
//! (the A3 shadow rule over the ground bones, low-passed per frame). The
//! camanm camera (Step 9) plugs into the same loop.
//!
//! No per-frame allocation: the session's scratch buffers are sized once;
//! the ground-point buffer is a fixed array.

use crate::core::anm::camera::sample_camera;
use crate::core::anm::pose::evaluate_into;
use crate::services::scene3d::{frame_board, scene_graph};

use super::director_math::{
    clip_frame, part_world, shadow_step, shadow_target, shadow_world, transform_point, BLACK,
    IDENTITY, WHITE,
};
use super::schedule::ClipSel;
use super::session::{Clip, InstanceKind, InstanceStatus, Session};

/// Produce + publish every built instance's frame for song time `t`
/// (seconds since the song-start edge). `visible = false` publishes the
/// same poses hidden (the pre-edge / abandoned states).
pub fn produce(sess: &mut Session, t: f32, visible: bool) {
    let hidden = !visible;
    let n_dancers = sess.parsed.dancers.len();

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
        let Session { scratch, bones, .. } = sess;
        match &p.loop_clip {
            Some(clip) => {
                let frame = clip_frame(t, clip.anm.duration_s(), clip.anm.fps, true);
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
        frame_board::publish(
            slot,
            &world,
            WHITE,
            hidden,
            &bones[..bone_count.min(bones.len())],
        );
    }

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
        {
            let Some(d) = sess.parsed.dancers.get(i) else {
                continue;
            };
            let Some(clip) = d.clips.get(pos.clip) else {
                continue;
            };
            let frame = clip_frame(
                pos.local_t,
                clip.anm.duration_s(),
                clip.anm.fps,
                clip.anm.loops,
            );
            let Session { scratch, bones, .. } = sess;
            evaluate_into(
                &clip.anm,
                &clip.bytes,
                frame,
                &d.skeleton,
                &d.seed,
                scratch,
                bones,
            );
        }
        if body_built {
            let n = body_bone_count.min(sess.bones.len());
            frame_board::publish(body_slot, &body_world, WHITE, hidden, &sess.bones[..n]);
        }

        // Children read the freshly evaluated `sess.bones`.
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
                    frame_board::publish(slot, &world, WHITE, hidden, &[IDENTITY]);
                }
                InstanceKind::Shadow(dancer) if dancer == i => {
                    let Some(d) = sess.parsed.dancers.get(i) else {
                        continue;
                    };
                    let mut ground = [[0.0f32; 3]; 5];
                    let mut n = 0usize;
                    for &g in d.ground.iter().take(ground.len()) {
                        if let Some(m) = sess.bones.get(g) {
                            ground[n] = [m[12], m[13], m[14]];
                            n += 1;
                        }
                    }
                    let (bind_y, anim_y) = match d.hips {
                        Some(h) => (
                            d.skeleton.bind_world.get(h).map_or(0.0, |m| m[13]),
                            sess.bones.get(h).map_or(0.0, |m| m[13]),
                        ),
                        None => (0.0, 0.0),
                    };
                    let Some((centre, target)) =
                        shadow_target(&ground[..n], bind_y, anim_y, d.shadow_scale)
                    else {
                        continue;
                    };
                    let prev = sess.shadow_size.get(i).copied().unwrap_or(d.shadow_scale);
                    let size = shadow_step(prev, target);
                    if let Some(s) = sess.shadow_size.get_mut(i) {
                        *s = size;
                    }
                    let world = shadow_world(size, transform_point(&body_world, centre));
                    frame_board::publish(slot, &world, BLACK, hidden, &[IDENTITY]);
                }
                _ => {}
            }
        }
    }
}

/// The camera for song time `t` (design §4.3.3/§4.3.4): advance the A3
/// stage-mode event loop from the last frame (or re-simulate from 0 after a
/// song (re)start / any backwards jump), then sample the selected `.camanm`
/// at its local time. `None` without a camera set (the caller keeps the
/// fixed fallback camera).
pub fn camera_frame(sess: &mut Session, t: f32) -> Option<scene_graph::CamSample> {
    let dance = sess.schedule.as_ref()?;
    let sched = sess.camera.as_ref()?;
    let st = match sess.camera_state {
        Some((prev, prev_t)) if t >= prev_t => sched.advance(&prev, prev_t, t, dance),
        _ => sched.at(t, dance),
    };
    sess.camera_state = Some((st, t));
    let cams = sess.parsed.cameras.as_ref()?;
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

/// The camera timeline over the first `until_s` seconds — one entry per
/// dance cut: `k@cut: <shot just before> -> <shot at the cut>` (the dev-mode
/// song-start log).
pub fn camera_timeline(sess: &Session, until_s: f32) -> String {
    let (Some(dance), Some(sched)) = (sess.schedule.as_ref(), sess.camera.as_ref()) else {
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
