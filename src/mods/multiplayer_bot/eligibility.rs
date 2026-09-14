//! Bot eligibility gate (design §4.4) — pure.
//!
//! Evaluated once per song at the song-select → stage transition. Every game
//! input arrives as `Option<T>` because the backing services
//! (`stage_records::side_entered` / `event_mode` / `game_work`) return `None`
//! when a derivation is missing; an unavailable input MUST read as
//! `Refusal::Unavailable` (stock 1P song + WARN), never as an ordinary "not a
//! solo game" refusal. Dependency-free so the host harness can mount it.

/// Everything the gate looks at. Sides are 0 = P1, 1 = P2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Inputs {
    /// `PlayerWork[side]+0x4 != 0` per side.
    pub entered: [Option<bool>; 2],
    /// `GameWork+0x4`: 0 = SINGLE, 1 = DOUBLE.
    pub style: Option<i32>,
    /// `*(GameWork + course_field_offset)` — non-zero in course / Dan play.
    pub course_word: Option<u64>,
    /// `GameWork+0xD0`: 1 / 2 = event chains.
    pub event_mode: Option<i32>,
    /// `GameWork+0x0`: 1 = already a local versus session.
    pub versus: Option<i32>,
    /// The `bot_opponent` option per side (only the ENTERED side's matters —
    /// per-side option values outlive the player).
    pub option_on: [bool; 2],
    /// The `bot_opponent_level` option per side (raw; clamped on use).
    pub level: [i32; 2],
}

/// Why the bot did not engage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A gate input could not be read — treat as ineligible and WARN once.
    Unavailable(&'static str),
    NotExactlyOneEntered,
    NotSingle,
    Course,
    EventMode,
    AlreadyVersus,
    OptionOff,
}

/// The engaged configuration: `bot` is always the side `human` is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub human: usize,
    pub bot: usize,
    pub level: u8,
}

/// Lowest / highest bot level the option and the skill model accept.
pub const MIN_LEVEL: u8 = 1;
pub const MAX_LEVEL: u8 = 10;

/// Clamp a raw option value into `MIN_LEVEL..=MAX_LEVEL`.
pub fn clamp_level(level: i32) -> u8 {
    level.clamp(MIN_LEVEL as i32, MAX_LEVEL as i32) as u8
}

/// The gate. Unavailable inputs are reported before ordinary refusals so a
/// broken service is never mistaken for "just a 2P / doubles / course game".
pub fn evaluate(i: &Inputs) -> Result<Plan, Refusal> {
    let (Some(p1), Some(p2)) = (i.entered[0], i.entered[1]) else {
        return Err(Refusal::Unavailable("side_entered"));
    };
    let Some(style) = i.style else {
        return Err(Refusal::Unavailable("style"));
    };
    let Some(course_word) = i.course_word else {
        return Err(Refusal::Unavailable("course_field"));
    };
    let Some(event_mode) = i.event_mode else {
        return Err(Refusal::Unavailable("event_mode"));
    };
    let Some(versus) = i.versus else {
        return Err(Refusal::Unavailable("versus_word"));
    };

    let human = match (p1, p2) {
        (true, false) => 0,
        (false, true) => 1,
        _ => return Err(Refusal::NotExactlyOneEntered),
    };
    if style != 0 {
        return Err(Refusal::NotSingle);
    }
    if course_word != 0 {
        return Err(Refusal::Course);
    }
    if event_mode != 0 {
        return Err(Refusal::EventMode);
    }
    if versus != 0 {
        return Err(Refusal::AlreadyVersus);
    }
    if !i.option_on[human] {
        return Err(Refusal::OptionOff);
    }
    Ok(Plan {
        human,
        bot: 1 - human,
        level: clamp_level(i.level[human]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(human: usize) -> Inputs {
        let mut entered = [Some(false), Some(false)];
        entered[human] = Some(true);
        let mut option_on = [false, false];
        option_on[human] = true;
        Inputs {
            entered,
            style: Some(0),
            course_word: Some(0),
            event_mode: Some(0),
            versus: Some(0),
            option_on,
            level: [7, 7],
        }
    }

    #[test]
    fn either_entered_side_is_the_human() {
        for human in 0..2 {
            let plan = evaluate(&ok(human)).expect("eligible");
            assert_eq!(plan.human, human);
            assert_eq!(plan.bot, 1 - human);
            assert_eq!(plan.level, 7);
        }
    }

    #[test]
    fn refusals_named() {
        let mut i = ok(0);
        i.entered = [Some(true), Some(true)];
        assert_eq!(evaluate(&i), Err(Refusal::NotExactlyOneEntered));
        i.entered = [Some(false), Some(false)];
        assert_eq!(evaluate(&i), Err(Refusal::NotExactlyOneEntered));

        let mut i = ok(0);
        i.style = Some(1);
        assert_eq!(evaluate(&i), Err(Refusal::NotSingle));

        let mut i = ok(0);
        i.course_word = Some(0x1234);
        assert_eq!(evaluate(&i), Err(Refusal::Course));

        for em in [1, 2] {
            let mut i = ok(0);
            i.event_mode = Some(em);
            assert_eq!(evaluate(&i), Err(Refusal::EventMode), "event {em}");
        }

        let mut i = ok(0);
        i.versus = Some(1);
        assert_eq!(evaluate(&i), Err(Refusal::AlreadyVersus));

        let mut i = ok(1);
        i.option_on = [true, false]; // ON only on the NON-entered side
        assert_eq!(evaluate(&i), Err(Refusal::OptionOff));
    }

    #[test]
    fn unavailable_inputs_win_over_refusals() {
        let mut i = ok(0);
        i.entered[1] = None;
        i.style = Some(1); // would be NotSingle if entered were known
        assert_eq!(evaluate(&i), Err(Refusal::Unavailable("side_entered")));
        let mut i = ok(0);
        i.style = None;
        assert_eq!(evaluate(&i), Err(Refusal::Unavailable("style")));
        let mut i = ok(0);
        i.course_word = None;
        assert_eq!(evaluate(&i), Err(Refusal::Unavailable("course_field")));
        let mut i = ok(0);
        i.event_mode = None;
        assert_eq!(evaluate(&i), Err(Refusal::Unavailable("event_mode")));
        let mut i = ok(0);
        i.versus = None;
        assert_eq!(evaluate(&i), Err(Refusal::Unavailable("versus_word")));
    }

    #[test]
    fn level_clamps() {
        assert_eq!(clamp_level(0), 1);
        assert_eq!(clamp_level(-5), 1);
        assert_eq!(clamp_level(11), 10);
        assert_eq!(clamp_level(5), 5);
        let mut i = ok(0);
        i.level = [99, 0];
        assert_eq!(evaluate(&i).expect("eligible").level, 10);
        let mut i = ok(1);
        i.level = [99, 0];
        assert_eq!(evaluate(&i).expect("eligible").level, 1);
    }

    #[test]
    fn other_sides_level_and_option_are_ignored() {
        let mut i = ok(0);
        i.option_on = [true, true];
        i.level = [3, 10];
        assert_eq!(evaluate(&i).expect("eligible").level, 3);
    }
}
