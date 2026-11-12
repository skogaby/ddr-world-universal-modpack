//! The Winx Club ENCHANTIX TRANSFORMATION — the pure half (std only,
//! harness-mounted as `transform_logic`). RE: main.dol `FUN_800a6d3c`
//! (the sequencer at game+0x2dc), `docs/wii_ddr_winx_club_research.md` §3
//! / §7.3; the engine-facing half is `director.rs` (`transform_tick`) +
//! `lifecycle.rs` (the trigger poll).
//!
//! Content (`port_character_hottest2.py GAME=winx`): an outfit's body folder
//! carries `transform.txt` ([`parse_marker`]: its Enchantix twin's key + the
//! twin's lift), `motion/transform/{transform,pose}.anm` (the girl's APL
//! clips 0 / 1, 1:1 at 60 Hz) and `camera/transform.camanm` (GAME_DEF_CAM
//! `/#3/#0`); the twin (a `… Fairy` dancer) carries its own transform clips
//! and the `winx_fx/` effect banks.
//!
//! The sequence, on the REAL clock from the trigger `r0` ([`look`]):
//!
//! | phase | real time | what shows |
//! |---|---|---|
//! | [`Phase::Fade`] | `r0 .. r0+0.5` | scene colour 1 → 0, the outfit dancing |
//! | [`Phase::Transform`] | `+0.5 .. +0.5+T` (T = the clip, 2 s) | both twins play `transform.anm`; colour 0 → 1 over 0.5 s; the event camera; the effect; the WIPE: a dancer-relative point (0, −10 → 30, 0) zan units over 2 s, projected — outfit above the line, twin below; the other dancers hidden |
//! | [`Phase::Pose`] | until the next choreography PIECE | both play `pose.anm`; the effect fades out over 0.5 s; the outfit is gone (the line passed its head) |
//! | [`Phase::After`] | to the song's end | the twin dances the outfit's playlist at the schedule position; camera released |
//!
//! "The next choreography piece" (`FUN_800a7678` from `FUN_800307f0`): a
//! Wii MOT piece is ONE bar; the ported clips are runs of pieces at one bar
//! = [`BAR_S`] of dance time, so the switch lands on the next bar line of
//! the clip playing underneath ([`next_piece_boundary`]).
//!
//! The trigger ([`should_trigger`], maintainer 2026-10-20): the side still
//! holds a FULL COMBO of any kind (Marvelous / Perfect / Great / Good — no
//! Boo, Miss or N.G. judged yet, [`is_full_combo`]) when the song reaches
//! [`TRIGGER_AT_PERMILLE`] (80 %) of its length — independent of the chart's difficulty
//! (a Beginner chart may never reach a high combo). Once per side per
//! play, human sides only (never the Multiplayer Bot's phantom side — the
//! caller's), never in previews. A rewind before `r0` re-arms. (The Wii:
//! a full Dance Gauge before 80 % of the song.)

/// The song fraction (‰) at which a held full combo transforms (the Wii's
/// `FUN_8003149c` limit, `pos ≤ 0.8 · length`, turned into the moment).
pub const TRIGGER_AT_PERMILLE: i64 = 800;

/// The song has reached [`TRIGGER_AT_PERMILLE`] (exact integer test).
pub fn past_trigger_point(pos_ms: i64, end_ms: i32) -> bool {
    end_ms > 0 && pos_ms >= 0 && pos_ms * 1000 >= end_ms as i64 * TRIGGER_AT_PERMILLE
}
/// `JudgeDiag::grades` slots (`song_reset`: +0x1A0 + grade·4): 0 Marvelous,
/// 1 Perfect, 2 Great, 3 Good, 4 Boo, 5 Miss, 6 O.K., 7 N.G.
pub const GRADES_KEEPING_COMBO: [usize; 4] = [0, 1, 2, 3];
/// The grades that end a full combo (Boo, Miss, N.G. — a missed freeze or a
/// stepped-on shock arrow).
pub const GRADES_BREAKING_COMBO: [usize; 3] = [4, 5, 7];
/// Phase 1: scene colour 1 → 0 (SDA2 r2-0x6bc4).
pub const FADE_S: f32 = 0.5;
/// Phase 2's colour 0 → 1.
pub const FADE_IN_S: f32 = 0.5;
/// The transformation clip's length when it did not parse (APL 0, 120 f).
pub const DEFAULT_TRANSFORM_S: f32 = 2.0;
/// The wipe's duration (`seq + 8`, set in `FUN_8002eb34`).
pub const WIPE_S: f32 = 2.0;
/// The wipe point's y, zan units, start → end (`DAT_801e1cb8` / `cc4`).
pub const WIPE_FROM_UNITS: f32 = -10.0;
pub const WIPE_TO_UNITS: f32 = 30.0;
/// The effect rides the line this much higher (`r2-0x6bb0`).
pub const EFFECT_LIFT_UNITS: f32 = 0.2;
/// The effect's fade-out at phase 3 (`FUN_800fb13c`).
pub const EFFECT_FADE_S: f32 = 0.5;
/// One ported choreography piece = one bar = 120 frames of dance time.
pub const BAR_S: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Fade,
    Transform,
    Pose,
    After,
}

/// Which clip a twin plays this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Play {
    /// Its dance playlist at the schedule position.
    Dance,
    /// `transform.anm` at this local time (s).
    Transform(f32),
    /// `pose.anm` at this local time (s; held on its last frame).
    Pose(f32),
}

/// One frame of the sequence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub phase: Phase,
    /// Scene colour (stage + dancers' tint rgb), 0..1.
    pub colour: f32,
    pub outfit: Option<Play>,
    pub twin: Option<Play>,
    /// The other dancers show.
    pub others: bool,
    /// The wipe's progress 0..1 (`Some` only while it runs).
    pub wipe: Option<f32>,
    /// The effect runs (with this fade, 1 = full).
    pub effect: Option<f32>,
    /// The event camera's local time (s) while it films.
    pub camera: Option<f32>,
}

/// A full combo so far: at least one step judged, none of
/// [`GRADES_BREAKING_COMBO`].
pub fn is_full_combo(grades: &[i32; 8]) -> bool {
    GRADES_BREAKING_COMBO.iter().all(|&g| grades[g] == 0)
        && GRADES_KEEPING_COMBO
            .iter()
            .map(|&g| grades[g].max(0) as i64)
            .sum::<i64>()
            > 0
}

/// The trigger rule. `grades` = the side's live judge counts (`None`
/// outside a run), `pos_ms` / `end_ms` the music count and the chart's
/// song-over: a held full combo once the song passed
/// [`TRIGGER_AT_PERMILLE`] (and has not ended).
pub fn should_trigger(grades: Option<[i32; 8]>, pos_ms: i64, end_ms: Option<i32>) -> bool {
    let (Some(g), Some(end)) = (grades, end_ms) else {
        return false;
    };
    past_trigger_point(pos_ms, end) && pos_ms < end as i64 && is_full_combo(&g)
}

/// The sequence at real time `real_s` for a trigger at `r0`, a
/// transformation clip of `transform_s`, and the switch (`None` until the
/// pose phase has computed it; real seconds). `None` before `r0` (a rewind
/// past the trigger: the caller re-arms).
pub fn look(r0: f32, transform_s: f32, switch: Option<f32>, real_s: f32) -> Option<Look> {
    let e = real_s - r0;
    if !e.is_finite() || e < 0.0 {
        return None;
    }
    let tr = if transform_s > 0.0 {
        transform_s
    } else {
        DEFAULT_TRANSFORM_S
    };
    if e < FADE_S {
        return Some(Look {
            phase: Phase::Fade,
            colour: 1.0 - e / FADE_S,
            outfit: Some(Play::Dance),
            twin: None,
            others: true,
            wipe: None,
            effect: None,
            camera: None,
        });
    }
    let e2 = e - FADE_S;
    if e2 < tr {
        return Some(Look {
            phase: Phase::Transform,
            colour: (e2 / FADE_IN_S).min(1.0),
            outfit: Some(Play::Transform(e2)),
            twin: Some(Play::Transform(e2)),
            others: false,
            wipe: Some((e2 / WIPE_S).min(1.0)),
            effect: Some(1.0),
            camera: Some(e2),
        });
    }
    let e3 = e2 - tr;
    if switch.map_or(true, |s| real_s < s) {
        return Some(Look {
            phase: Phase::Pose,
            colour: 1.0,
            outfit: None,
            twin: Some(Play::Pose(e3)),
            others: false,
            wipe: None,
            effect: (e3 < EFFECT_FADE_S).then(|| 1.0 - e3 / EFFECT_FADE_S),
            camera: Some(e2),
        });
    }
    Some(Look {
        phase: Phase::After,
        colour: 1.0,
        outfit: None,
        twin: Some(Play::Dance),
        others: true,
        wipe: None,
        effect: None,
        camera: None,
    })
}

/// The dance time of the next choreography piece boundary at or after dance
/// time `t`, for a clip that started at dance time `clip_start` and ends at
/// `clip_end`: the next bar line of the clip ([`BAR_S`]), or its end.
pub fn next_piece_boundary(t: f32, clip_start: f32, clip_end: f32) -> f32 {
    let local = (t - clip_start).max(0.0);
    let bar = (local / BAR_S).ceil() * BAR_S;
    let at = clip_start + bar.max(local);
    if clip_end > clip_start {
        at.min(clip_end)
    } else {
        at
    }
}

/// The wipe point's height above the dancer (zan units) at progress `p`.
pub fn wipe_units(p: f32) -> f32 {
    WIPE_FROM_UNITS + (WIPE_TO_UNITS - WIPE_FROM_UNITS) * p.clamp(0.0, 1.0)
}

/// The shader word for a wipe line at NDC y `line` (`mdl_common.hlsli::
/// wipe_clip`): `side` +1 keeps the part ABOVE (the outfit), −1 below.
pub fn wipe_word(line_ndc: f32, above: bool) -> f32 {
    let l = if line_ndc.is_finite() {
        line_ndc.clamp(-3.0, 3.0)
    } else {
        3.0
    };
    if above {
        l + 4.0
    } else {
        -(l + 4.0)
    }
}

/// The crossfade fallback (wipe containers not served): the twin's alpha
/// at progress `p` (the outfit's is `1 − that`).
pub fn crossfade_alpha(p: f32) -> f32 {
    p.clamp(0.0, 1.0)
}

/// `transform.txt`: the first non-comment token is the twin's key; an
/// optional `lift <metres>` line. `None` without a key.
pub fn parse_marker(text: &str) -> Option<(String, f32)> {
    let mut key = None;
    let mut lift = 0.0f32;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut f = line.split_whitespace();
        match (f.next(), f.next()) {
            (Some("lift"), Some(v)) => {
                if let Ok(x) = v.parse::<f32>() {
                    if x.is_finite() && x.abs() < 10.0 {
                        lift = x;
                    }
                }
            }
            (Some(k), None) if key.is_none() => key = Some(k.to_string()),
            _ => {}
        }
    }
    key.filter(|k| !k.is_empty()).map(|k| (k, lift))
}

/// Bones whose motion comes from the solver or the attach rig, never from
/// another body's clip (the head / wing skeletons — their rest locals
/// differ between the outfit and the twin).
pub fn is_private_bone(name: &str) -> bool {
    name.starts_with("DB_")
        || name.starts_with("rot_DB")
        || name == "bone_head"
        || name == "wing_root"
}

/// For playing the OUTFIT's clips on the TWIN (after the switch): per
/// outfit bone index, the twin's bone of the same name (`None` for a name
/// the twin lacks or a [`is_private_bone`]). `outfit` / `twin` are `(name,
/// index)` tables (`.b2it`; role aliases resolve to the same index and are
/// harmless).
pub fn retarget_map(
    outfit: &[(String, usize)],
    twin: &[(String, usize)],
    outfit_bones: usize,
) -> Vec<Option<u8>> {
    let mut map = vec![None; outfit_bones];
    for (name, oi) in outfit {
        if is_private_bone(name) {
            continue;
        }
        let Some(slot) = map.get_mut(*oi) else {
            continue;
        };
        if slot.is_some() {
            continue;
        }
        if let Some((_, ti)) = twin.iter().find(|(n, _)| n == name) {
            if *ti <= u8::MAX as usize {
                *slot = Some(*ti as u8);
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_rule() {
        // a Good full combo counts as much as a Marvelous one
        let gfc = [10, 20, 5, 3, 0, 0, 4, 0];
        let mfc = [12, 0, 0, 0, 0, 0, 0, 0];
        assert!(is_full_combo(&gfc) && is_full_combo(&mfc));
        assert!(!is_full_combo(&[10, 0, 0, 0, 0, 1, 0, 0]), "a miss");
        assert!(!is_full_combo(&[10, 0, 0, 0, 1, 0, 0, 0]), "a boo");
        assert!(!is_full_combo(&[10, 0, 0, 0, 0, 0, 2, 1]), "an N.G.");
        assert!(
            !is_full_combo(&[0, 0, 0, 0, 0, 0, 3, 0]),
            "nothing stepped yet"
        );
        // at 80 % of the song, not before, not after its end
        assert!(should_trigger(Some(gfc), 80_000, Some(100_000)));
        assert!(should_trigger(Some(mfc), 95_000, Some(100_000)));
        assert!(!should_trigger(Some(gfc), 79_999, Some(100_000)));
        assert!(!should_trigger(Some(gfc), 100_000, Some(100_000)));
        assert!(!should_trigger(
            Some([1, 0, 0, 0, 0, 1, 0, 0]),
            90_000,
            Some(100_000)
        ));
        assert!(!should_trigger(None, 90_000, Some(100_000)));
        assert!(!should_trigger(Some(gfc), 90_000, None));
        assert!(!should_trigger(Some(gfc), 10, Some(0)));
    }

    #[test]
    fn sequence_phases_and_values() {
        assert!(look(10.0, 2.0, None, 9.99).is_none(), "before the trigger");
        let a = look(10.0, 2.0, None, 10.25).unwrap();
        assert_eq!(a.phase, Phase::Fade);
        assert!((a.colour - 0.5).abs() < 1e-6 && a.others && a.twin.is_none());
        let b = look(10.0, 2.0, None, 10.75).unwrap();
        assert_eq!(b.phase, Phase::Transform);
        assert!((b.colour - 0.5).abs() < 1e-6);
        assert_eq!(b.outfit, Some(Play::Transform(0.25)));
        assert_eq!(b.twin, Some(Play::Transform(0.25)));
        assert!(!b.others && b.effect == Some(1.0) && b.camera == Some(0.25));
        assert!((b.wipe.unwrap() - 0.125).abs() < 1e-6);
        let c = look(10.0, 2.0, None, 12.75).unwrap();
        assert_eq!(c.phase, Phase::Pose);
        assert!(
            c.outfit.is_none() && matches!(c.twin, Some(Play::Pose(x)) if (x - 0.25).abs() < 1e-5)
        );
        assert!((c.effect.unwrap() - 0.5).abs() < 1e-5, "the effect fades");
        assert!(look(10.0, 2.0, None, 13.5).unwrap().effect.is_none());
        // pose holds until the switch, then the twin dances
        assert_eq!(
            look(10.0, 2.0, Some(14.0), 13.9).unwrap().phase,
            Phase::Pose
        );
        let d = look(10.0, 2.0, Some(14.0), 14.0).unwrap();
        assert_eq!(d.phase, Phase::After);
        assert_eq!(d.twin, Some(Play::Dance));
        assert!(d.outfit.is_none() && d.others && d.camera.is_none());
        // a clip that failed to parse: the Wii's 2 s
        assert_eq!(look(0.0, 0.0, None, 2.4).unwrap().phase, Phase::Transform);
    }

    #[test]
    fn piece_boundaries_and_wipe() {
        assert!((next_piece_boundary(5.1, 4.0, 20.0) - 6.0).abs() < 1e-6);
        assert!(
            (next_piece_boundary(6.0, 4.0, 20.0) - 6.0).abs() < 1e-6,
            "on a bar line"
        );
        assert!(
            (next_piece_boundary(19.5, 4.0, 19.8) - 19.8).abs() < 1e-6,
            "the clip ends first"
        );
        assert_eq!(wipe_units(0.0), -10.0);
        assert_eq!(wipe_units(1.0), 30.0);
        assert_eq!(wipe_units(2.0), 30.0);
        assert_eq!(wipe_word(0.25, true), 4.25);
        assert_eq!(wipe_word(0.25, false), -4.25);
        assert_eq!(wipe_word(-9.0, true), 1.0, "clamped, never 0");
        assert_eq!(wipe_word(f32::NAN, false), -7.0);
        assert_eq!(crossfade_alpha(1.5), 1.0);
    }

    #[test]
    fn marker_grammar() {
        let m = parse_marker("# a comment\nwinxbloom04\nlift 0.090314\n").unwrap();
        assert_eq!(m.0, "winxbloom04");
        assert!((m.1 - 0.090314).abs() < 1e-6);
        assert_eq!(
            parse_marker("winxmusa04\n").unwrap(),
            ("winxmusa04".to_string(), 0.0)
        );
        assert!(parse_marker("# nothing\n\n").is_none());
        assert_eq!(parse_marker("k\nlift nope\n").unwrap().1, 0.0);
    }

    #[test]
    fn retarget_by_name_skips_private_bones() {
        let o: Vec<(String, usize)> = [
            ("Hips", 0),
            ("Spine", 1),
            ("DB_hair01", 2),
            ("Head", 3),
            ("Spine2", 1),
        ]
        .iter()
        .map(|(n, i)| (n.to_string(), *i))
        .collect();
        let t: Vec<(String, usize)> = [
            ("Hips", 0),
            ("AccSpine1", 1),
            ("Spine", 2),
            ("Head", 4),
            ("DB_hair01", 5),
        ]
        .iter()
        .map(|(n, i)| (n.to_string(), *i))
        .collect();
        let m = retarget_map(&o, &t, 4);
        assert_eq!(m, vec![Some(0), Some(2), None, Some(4)]);
        assert!(
            is_private_bone("rot_DBbone")
                && is_private_bone("wing_root")
                && !is_private_bone("Head")
        );
    }
}
