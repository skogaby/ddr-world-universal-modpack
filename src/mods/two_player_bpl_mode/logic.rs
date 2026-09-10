//! Pure decision / arithmetic layer of the 2-Player BPL Mode re-host.
//!
//! Dependency-free on purpose: `scripts/validate_two_player_bpl.sh` mounts this
//! file into a throwaway host crate and runs the `#[cfg(test)]` suite there
//! (plain `cargo test` cannot compile the DLL crate on non-x86 hosts). Nothing
//! here touches game memory; `mod.rs` feeds it plain values and acts on the
//! results.

/// Everything the eligibility gate looks at. `None` = the backing service
/// could not provide the value (fail-open ⇒ `Gate::Unavailable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateInputs {
    /// `GameWork+0`: 1 = local versus (two players), 0 = solo / doubles.
    pub versus: Option<i32>,
    /// `PlayerWork[side]+0x4 != 0` per side.
    pub entered: [Option<bool>; 2],
    /// `GameWork+0xD0`: 1 / 2 = event chains (real BPL battle already has the
    /// frame), anything else = ordinary play.
    pub event_mode: Option<i32>,
    /// `*(GameWork + course_field_offset)` — non-zero in course / Dan play.
    pub course_word: Option<u64>,
    /// The scene manager's current scene is GAMEPLAY.
    pub scene_is_gameplay: bool,
}

/// Outcome of the eligibility gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// A local 2-player versus song in GAMEPLAY — place the frame.
    Eligible,
    /// Ordinary refusal (solo, doubles, course, event chain, wrong scene).
    Ineligible(&'static str),
    /// A gate input could not be read — treat as ineligible and WARN once.
    Unavailable(&'static str),
}

/// The session gate (design R-GATE). Order matters only for the reason
/// string: unavailable inputs are reported before ordinary refusals so a
/// broken service is never mistaken for "just a solo game".
pub fn eligibility(i: &GateInputs) -> Gate {
    let Some(versus) = i.versus else {
        return Gate::Unavailable("game_work");
    };
    let (Some(p1), Some(p2)) = (i.entered[0], i.entered[1]) else {
        return Gate::Unavailable("side_entered");
    };
    let Some(event_mode) = i.event_mode else {
        return Gate::Unavailable("event_mode");
    };
    let Some(course_word) = i.course_word else {
        return Gate::Unavailable("course_field");
    };
    if !i.scene_is_gameplay {
        return Gate::Ineligible("scene != GAMEPLAY");
    }
    if versus != 1 {
        return Gate::Ineligible("not versus (solo / doubles)");
    }
    if !(p1 && p2) {
        return Gate::Ineligible("both sides not entered");
    }
    if event_mode == 1 || event_mode == 2 {
        return Gate::Ineligible("event chain (real BPL / event mode)");
    }
    if course_word != 0 {
        return Gate::Ineligible("course / Dan session");
    }
    Gate::Eligible
}

/// Longest dancer name the boards carry (the network record copies 8 chars +
/// NUL; the BATTLE_INFO field is 16 bytes).
pub const NAME_MAX: usize = 8;
/// Size of the `BATTLE_INFO` name field.
pub const NAME_FIELD: usize = 16;

/// Reproduces the game's `PlayerWork` getName (`FUN_1801e88a0` on 20260825):
/// an ENTERED player whose inline name (`PlayerWork+0x0C`, `char[]`) is empty
/// is shown as `PLAYER1` / `PLAYER2`; otherwise the inline bytes up to the
/// first NUL, clamped to [`NAME_MAX`]. Output is the full 16-byte field,
/// NUL-padded, always terminated.
pub fn player_name(entered: bool, raw: &[u8], side: usize) -> [u8; NAME_FIELD] {
    let mut out = [0u8; NAME_FIELD];
    let first = raw.first().copied().unwrap_or(0);
    if entered && first == 0 {
        let fallback: &[u8] = match side {
            0 => b"PLAYER1",
            1 => b"PLAYER2",
            _ => b"PLAYER",
        };
        out[..fallback.len()].copy_from_slice(fallback);
        return out;
    }
    let mut n = 0;
    while n < NAME_MAX && n < raw.len() && raw[n] != 0 {
        out[n] = raw[n];
        n += 1;
    }
    out
}

/// Stock onUpdate smoothing: eases UP by halves toward the target and snaps
/// DOWN immediately (`min((display + target + 1) / 2, target)`, C truncating
/// division). Computed in i64 so a near-`i32::MAX` pair cannot wrap (stock
/// does it in i32 with CDQ — identical for every reachable score).
pub fn smooth(display: i32, target: i32) -> i32 {
    let eased = (display as i64 + target as i64 + 1) / 2;
    eased
        .min(target as i64)
        .clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Stock gauge fraction `display / max` (stored as f32 at BATTLE_INFO+0x34);
/// 0.0 when `max == 0` (stock skips the whole smoothing block then — the
/// caller must skip the store too, but a defined value is safer than NaN).
pub fn gauge_fraction(display: i32, max: i32) -> f32 {
    if max == 0 {
        0.0
    } else {
        display as f32 / max as f32
    }
}

/// Number of virtual slots in `MatchingBattleFrameActor::vftable`.
pub const VTABLE_SLOTS: usize = 9;
/// Slot indices the mod overrides.
pub const SLOT_ON_INITIALIZE: usize = 4;
pub const SLOT_ON_UPDATE: usize = 6;

/// Physical image of the vtable clone: `[COL, slot0 .. slot8]` — index 0 is
/// the RTTI complete-object-locator pointer MSVC keeps at `vtable[-1]`, so the
/// installed vtable pointer is `image.as_ptr() + 1`. Slots 4 and 6 are
/// replaced, everything else is the donor's.
pub fn clone_vtable_image(
    donor: &[usize; VTABLE_SLOTS],
    col: usize,
    on_initialize: usize,
    on_update: usize,
) -> [usize; VTABLE_SLOTS + 1] {
    let mut img = [0usize; VTABLE_SLOTS + 1];
    img[0] = col;
    img[1..].copy_from_slice(donor);
    img[1 + SLOT_ON_INITIALIZE] = on_initialize;
    img[1 + SLOT_ON_UPDATE] = on_update;
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok() -> GateInputs {
        GateInputs {
            versus: Some(1),
            entered: [Some(true), Some(true)],
            event_mode: Some(0),
            course_word: Some(0),
            scene_is_gameplay: true,
        }
    }

    #[test]
    fn eligible_versus() {
        assert_eq!(eligibility(&ok()), Gate::Eligible);
    }

    #[test]
    fn solo_and_doubles_ineligible() {
        let mut i = ok();
        i.versus = Some(0);
        assert!(matches!(eligibility(&i), Gate::Ineligible(_)));
        i.versus = Some(2);
        assert!(matches!(eligibility(&i), Gate::Ineligible(_)));
    }

    #[test]
    fn one_side_only_ineligible() {
        let mut i = ok();
        i.entered = [Some(true), Some(false)];
        assert!(matches!(eligibility(&i), Gate::Ineligible(_)));
        i.entered = [Some(false), Some(true)];
        assert!(matches!(eligibility(&i), Gate::Ineligible(_)));
    }

    #[test]
    fn event_chain_ineligible() {
        for em in [1, 2] {
            let mut i = ok();
            i.event_mode = Some(em);
            assert!(matches!(eligibility(&i), Gate::Ineligible(_)), "event {em}");
        }
        let mut i = ok();
        i.event_mode = Some(3);
        assert_eq!(eligibility(&i), Gate::Eligible);
    }

    #[test]
    fn course_ineligible() {
        let mut i = ok();
        i.course_word = Some(0x1234);
        assert!(matches!(eligibility(&i), Gate::Ineligible(_)));
    }

    #[test]
    fn wrong_scene_ineligible() {
        let mut i = ok();
        i.scene_is_gameplay = false;
        assert!(matches!(eligibility(&i), Gate::Ineligible(_)));
    }

    #[test]
    fn unavailable_inputs_win_over_refusals() {
        let mut i = ok();
        i.versus = None;
        i.scene_is_gameplay = false; // would be Ineligible if versus were known
        assert_eq!(eligibility(&i), Gate::Unavailable("game_work"));
        let mut i = ok();
        i.entered[1] = None;
        assert_eq!(eligibility(&i), Gate::Unavailable("side_entered"));
        let mut i = ok();
        i.event_mode = None;
        assert_eq!(eligibility(&i), Gate::Unavailable("event_mode"));
        let mut i = ok();
        i.course_word = None;
        assert_eq!(eligibility(&i), Gate::Unavailable("course_field"));
    }

    #[test]
    fn name_fallback_for_entered_blank() {
        assert_eq!(&player_name(true, &[0u8; 12], 0)[..8], b"PLAYER1\0");
        assert_eq!(&player_name(true, &[0u8; 12], 1)[..8], b"PLAYER2\0");
        assert_eq!(&player_name(true, &[0u8; 12], 5)[..7], b"PLAYER\0");
    }

    #[test]
    fn name_not_entered_blank_stays_blank() {
        assert_eq!(player_name(false, &[0u8; 12], 0), [0u8; NAME_FIELD]);
    }

    #[test]
    fn name_copies_up_to_nul_and_clamps() {
        let mut raw = [0u8; 12];
        raw[..3].copy_from_slice(b"ABC");
        let n = player_name(true, &raw, 0);
        assert_eq!(&n[..4], b"ABC\0");
        assert!(n[4..].iter().all(|&b| b == 0));

        // 12 non-NUL bytes: clamp to NAME_MAX and still terminate.
        let raw = [b'X'; 12];
        let n = player_name(true, &raw, 1);
        assert_eq!(&n[..NAME_MAX], b"XXXXXXXX");
        assert_eq!(n[NAME_MAX], 0);
    }

    #[test]
    fn name_handles_empty_slice() {
        assert_eq!(&player_name(true, &[], 0)[..8], b"PLAYER1\0");
        assert_eq!(player_name(false, &[], 0), [0u8; NAME_FIELD]);
    }

    #[test]
    fn smooth_eases_up_by_halves() {
        let mut d = 0;
        let seq: Vec<i32> = (0..6)
            .map(|_| {
                d = smooth(d, 1000);
                d
            })
            .collect();
        assert_eq!(seq, vec![500, 750, 875, 938, 969, 985]);
    }

    #[test]
    fn smooth_snaps_down_and_is_fixed_at_target() {
        assert_eq!(smooth(900_000, 100), 100);
        assert_eq!(smooth(1000, 1000), 1000);
        assert_eq!(smooth(0, 0), 0);
    }

    #[test]
    fn smooth_reaches_target_exactly() {
        let mut d = 0;
        for _ in 0..40 {
            d = smooth(d, 999_999);
        }
        assert_eq!(d, 999_999);
    }

    #[test]
    fn smooth_no_overflow_near_max() {
        assert_eq!(smooth(i32::MAX - 1, i32::MAX), i32::MAX);
        assert_eq!(smooth(i32::MAX, i32::MAX), i32::MAX);
    }

    #[test]
    fn gauge_fraction_values() {
        assert_eq!(gauge_fraction(5, 0), 0.0);
        assert_eq!(gauge_fraction(500_000, 1_000_000), 0.5);
        assert_eq!(gauge_fraction(0, 7), 0.0);
    }

    #[test]
    fn clone_image_layout() {
        let donor: [usize; VTABLE_SLOTS] = [10, 11, 12, 13, 14, 15, 16, 17, 18];
        let img = clone_vtable_image(&donor, 0xC01, 0xAAAA, 0xBBBB);
        assert_eq!(img[0], 0xC01);
        for (i, &d) in donor.iter().enumerate() {
            match i {
                SLOT_ON_INITIALIZE => assert_eq!(img[1 + i], 0xAAAA),
                SLOT_ON_UPDATE => assert_eq!(img[1 + i], 0xBBBB),
                _ => assert_eq!(img[1 + i], d),
            }
        }
    }
}
