//! Session-window facts of the impersonation (design §4.5) — pure.
//!
//! The scene-callback decisions of `impersonation.rs` are decided here so the
//! host harness can pin them: WHEN to flip (the song-select → stage edge),
//! WHEN to restore (the first scene outside the play window), and WHEN to
//! re-seed the bot (GAMEPLAY entry, incl. a quick-restart re-entry), plus
//! the name plate (formatter, the Target Score target-name sanitizer) and
//! the secondary `TARGET BOT` label's glyphs and placement math. Scene ids
//! are the game's 0-indexed sequence ids written as literals (no `crate::`
//! imports); `impersonation.rs` pins them to `types::scenes::scene` with a
//! `const` assertion.

use super::eligibility::BotMode;

/// 0-indexed SONG_SELECT (`types::scenes::scene::SONG_SELECT`).
pub const SONG_SELECT: i32 = 25;
/// 0-indexed GAMEPLAY (`scene::GAMEPLAY`).
pub const GAMEPLAY: i32 = 28;
/// The scenes a song-select commit may hand off to (the interstitial, the
/// stage indicator, or GAMEPLAY directly). A flip is valid on any of them:
/// the scene callback fires BEFORE the original `createNextSequence`, so the
/// entered byte is set before the 27/28 loaders read it.
pub const FLIP_TARGETS: [i32; 3] = [26, 27, 28];
/// The play window: interstitial, stage indicator, gameplay, the post-song
/// loader, and the stage results. The first scene outside it restores.
pub const PLAY_WINDOW: [i32; 5] = [26, 27, 28, 29, 30];

/// Name-plate buffer: 8 characters + NUL (`PlayerWork+0xC..+0x15`).
pub const NAME_LEN: usize = 9;

/// What the state machine should do on a `(prev, next)` scene edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// Idle, leaving song select for a stage scene: evaluate and apply.
    Flip,
    /// Active, (re-)entering GAMEPLAY: re-roll the bot's seed.
    Reseed,
    /// Active, leaving the play window: undo everything.
    Restore,
    /// Nothing to do.
    None,
}

/// Whether `scene` is inside the play window.
pub fn in_play_window(scene: i32) -> bool {
    PLAY_WINDOW.contains(&scene)
}

/// Classify a scene edge given whether an impersonation is active.
pub fn classify(prev: i32, next: i32, active: bool) -> Edge {
    if !active {
        if prev == SONG_SELECT && FLIP_TARGETS.contains(&next) {
            return Edge::Flip;
        }
        return Edge::None;
    }
    if !in_play_window(next) {
        return Edge::Restore;
    }
    if next == GAMEPLAY {
        return Edge::Reseed;
    }
    Edge::None
}

/// The name plate of a Target Score replay whose target could not be named
/// (no target ghost, TARGET off, lookup unavailable).
pub const TARGET_NAME: &[u8] = b"TARGET";

/// The secondary label drawn above a NAMED Target Score replay's plate
/// (gameplay and results) — the plate itself carries the target's own
/// name. The name fonts have no parentheses (see [`glyph_suffix`]).
pub const TARGET_LABEL: &str = "TARGET BOT";

/// The texture-name suffix of one character in the game's name glyph sets
/// (`cote_edge_*` in gameplay, `cote_shadow_*` on the results screen, both
/// in `common_texture_v3`) — the game's own mapping (letters lower-cased;
/// `&` `$` `!` `-` `.` `?` spelled out; anything else `blank`), restricted
/// to the glyphs those sets actually ship (A–Z, 0–9, `& $ ! - . ?`, space).
/// `None` = a character the sets cannot draw.
pub fn glyph_suffix(c: u8) -> Option<&'static str> {
    const LETTERS: [&str; 26] = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r",
        "s", "t", "u", "v", "w", "x", "y", "z",
    ];
    const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    match c {
        b'A'..=b'Z' => LETTERS.get((c - b'A') as usize).copied(),
        b'a'..=b'z' => LETTERS.get((c - b'a') as usize).copied(),
        b'0'..=b'9' => DIGITS.get((c - b'0') as usize).copied(),
        b'&' => Some("ampersand"),
        b'$' => Some("dollar"),
        b'!' => Some("exclamation"),
        b'-' => Some("hyphen"),
        b'.' => Some("period"),
        b'?' => Some("question"),
        b' ' => Some("blank"),
        _ => None,
    }
}

/// The full glyph texture names of `text` in the set `prefix` (e.g.
/// `"cote_edge_"`), or `None` when a character has no glyph.
pub fn glyph_names(prefix: &str, text: &str) -> Option<Vec<String>> {
    text.bytes()
        .map(|c| glyph_suffix(c).map(|s| format!("{prefix}{s}")))
        .collect()
}

/// The target's name as the plate buffer: the bytes up to the first NUL
/// (at most 8), printable ASCII only, at least one non-space character.
/// `None` = unusable (the plate falls back to [`TARGET_NAME`]). Characters
/// outside the glyph sets are kept — the game draws them as blanks, the
/// same way it shows that name everywhere else.
pub fn sanitize_target_name(raw: &[u8]) -> Option<[u8; NAME_LEN]> {
    let len = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let name = raw.get(..len.min(NAME_LEN - 1))?;
    if !name.iter().all(|b| (0x20..=0x7E).contains(b)) || name.iter().all(|&b| b == b' ') {
        return None;
    }
    let mut out = [0u8; NAME_LEN];
    out[..name.len()].copy_from_slice(name);
    Some(out)
}

/// Placement of the secondary label relative to the plate's anchor box:
/// `(fixed scale, y offset)` for a label whose glyph height is `ratio` × the
/// plate's scaled anchor height `name_h`, sitting `gap` px ABOVE the box
/// (top-aligned layout). `glyph_h` = the label glyphs' texture height.
/// `None` when the inputs are not finite and positive.
pub fn label_geometry(name_h: f64, glyph_h: f64, ratio: f64, gap: f64) -> Option<(f64, f64)> {
    let ok = |v: f64| v.is_finite() && v > 0.0;
    if !ok(name_h) || !ok(glyph_h) || !ok(ratio) || !gap.is_finite() {
        return None;
    }
    let scale = ratio * name_h / glyph_h;
    Some((scale, -(glyph_h * scale + gap)))
}

/// [`TARGET_LABEL`] in the gameplay plate's glyph set (`cote_edge_*`; every
/// name ≤ 15 bytes, so each rides the names vector in SSO form).
pub const LABEL_GLYPHS_GAMEPLAY: [&str; 10] = [
    "cote_edge_t",
    "cote_edge_a",
    "cote_edge_r",
    "cote_edge_g",
    "cote_edge_e",
    "cote_edge_t",
    "cote_edge_blank",
    "cote_edge_b",
    "cote_edge_o",
    "cote_edge_t",
];

/// [`TARGET_LABEL`] in the results plate's glyph set (`cote_shadow_*`; the
/// 17-byte blank needs the heap form).
pub const LABEL_GLYPHS_RESULTS: [&str; 10] = [
    "cote_shadow_t",
    "cote_shadow_a",
    "cote_shadow_r",
    "cote_shadow_g",
    "cote_shadow_e",
    "cote_shadow_t",
    "cote_shadow_blank",
    "cote_shadow_b",
    "cote_shadow_o",
    "cote_shadow_t",
];

/// The bot's name as the game's fixed name buffer: NUL-terminated, zero
/// padded. `"BOT LV<n>"` for a level (every level in `1..=10` fits the
/// 8-character plate — `BOT LV10` is exactly 8; out-of-range levels clamp),
/// [`TARGET_NAME`] for a Target Score replay whose target is unnamed (a
/// named one gets [`sanitize_target_name`]'s buffer instead).
pub fn format_bot_name(mode: BotMode) -> [u8; NAME_LEN] {
    let mut out = [0u8; NAME_LEN];
    match mode {
        BotMode::Target => {
            out[..TARGET_NAME.len()].copy_from_slice(TARGET_NAME);
        }
        BotMode::Level(level) => {
            let level = level.clamp(1, 10);
            let prefix = b"BOT LV";
            out[..prefix.len()].copy_from_slice(prefix);
            let mut i = prefix.len();
            if level >= 10 {
                out[i] = b'0' + level / 10;
                i += 1;
            }
            out[i] = b'0' + level % 10;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_str(n: &[u8; NAME_LEN]) -> &str {
        let end = n.iter().position(|&b| b == 0).expect("NUL present");
        std::str::from_utf8(&n[..end]).expect("ascii")
    }

    #[test]
    fn bot_names_fit_eight_chars_plus_nul() {
        for level in 1..=10u8 {
            let n = format_bot_name(BotMode::Level(level));
            let s = name_str(&n);
            assert_eq!(s, format!("BOT LV{level}"));
            assert!(s.len() <= 8, "level {level}: {s:?}");
            let nul = s.len();
            assert!(n[nul..].iter().all(|&b| b == 0), "zero padded after NUL");
        }
        assert_eq!(&format_bot_name(BotMode::Level(1)), b"BOT LV1\0\0");
        assert_eq!(&format_bot_name(BotMode::Level(10)), b"BOT LV10\0");
        let t = format_bot_name(BotMode::Target);
        assert_eq!(name_str(&t), "TARGET");
        assert!(TARGET_NAME.len() <= 8);
        assert_eq!(&t, b"TARGET\0\0\0");
    }

    #[test]
    fn bot_name_clamps() {
        assert_eq!(name_str(&format_bot_name(BotMode::Level(0))), "BOT LV1");
        assert_eq!(name_str(&format_bot_name(BotMode::Level(11))), "BOT LV10");
        assert_eq!(name_str(&format_bot_name(BotMode::Level(255))), "BOT LV10");
    }

    #[test]
    fn target_name_sanitizes_to_the_plate_buffer() {
        assert_eq!(&sanitize_target_name(b"AFRO\0").unwrap(), b"AFRO\0\0\0\0\0");
        // A full 8-char name + the rival buffer's NUL.
        assert_eq!(&sanitize_target_name(b"ALEXANDR\0").unwrap(), b"ALEXANDR\0");
        // No NUL within the slice: the first 8 bytes.
        assert_eq!(&sanitize_target_name(b"ABCDEFGHIJ").unwrap(), b"ABCDEFGH\0");
        // Stops at the first NUL even inside a longer holder slot.
        assert_eq!(
            &sanitize_target_name(b"DJ.X\0ZZZZZZZ").unwrap(),
            b"DJ.X\0\0\0\0\0"
        );
        // Symbols and spaces survive; the font blanks what it cannot draw.
        assert_eq!(
            name_str(&sanitize_target_name(b"A B#*\0").unwrap()),
            "A B#*"
        );
        for bad in [
            &b""[..],
            b"\0AFRO",
            b"    \0",
            b"AF\x01RO\0",
            b"\xE3\x81\x82\0",
        ] {
            assert_eq!(sanitize_target_name(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn glyph_suffixes_follow_the_game_mapping() {
        assert_eq!(glyph_suffix(b'A'), Some("a"));
        assert_eq!(glyph_suffix(b'z'), Some("z"));
        assert_eq!(glyph_suffix(b'7'), Some("7"));
        assert_eq!(glyph_suffix(b' '), Some("blank"));
        assert_eq!(glyph_suffix(b'-'), Some("hyphen"));
        assert_eq!(glyph_suffix(b'&'), Some("ampersand"));
        assert_eq!(glyph_suffix(b'?'), Some("question"));
        // The mapping names these but common_texture ships no art for them,
        // and parentheses map to blank.
        for c in [b'(', b')', b'[', b'%', b'+', b'/', b'~', b','] {
            assert_eq!(glyph_suffix(c), None, "{}", c as char);
        }
    }

    #[test]
    fn label_glyph_tables_spell_the_label() {
        let edge: Vec<String> = LABEL_GLYPHS_GAMEPLAY
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(glyph_names("cote_edge_", TARGET_LABEL), Some(edge));
        let shadow: Vec<String> = LABEL_GLYPHS_RESULTS.iter().map(|s| s.to_string()).collect();
        assert_eq!(glyph_names("cote_shadow_", TARGET_LABEL), Some(shadow));
        assert!(
            LABEL_GLYPHS_GAMEPLAY.iter().all(|n| n.len() <= 15),
            "gameplay glyph names must stay SSO"
        );
        assert_eq!(glyph_names("cote_edge_", "(TARGET BOT)"), None);
    }

    #[test]
    fn label_geometry_sits_above_at_the_ratio() {
        let (scale, dy) = label_geometry(40.0, 72.0, 0.5, 2.0).unwrap();
        assert!(
            (scale * 72.0 - 20.0).abs() < 1e-9,
            "label glyphs are half the plate"
        );
        assert!(
            (dy + 22.0).abs() < 1e-9,
            "label bottom sits 2 px above the box"
        );
        for (h, g, r, gap) in [
            (0.0, 72.0, 0.5, 2.0),
            (40.0, 0.0, 0.5, 2.0),
            (40.0, 72.0, -0.5, 2.0),
            (f64::NAN, 72.0, 0.5, 2.0),
            (40.0, 72.0, 0.5, f64::INFINITY),
        ] {
            assert_eq!(label_geometry(h, g, r, gap), None);
        }
    }

    #[test]
    fn flip_only_from_song_select_into_stage_scenes_while_idle() {
        for next in [26, 27, 28] {
            assert_eq!(classify(25, next, false), Edge::Flip, "25 -> {next}");
        }
        for (prev, next) in [(25, 24), (24, 25), (26, 27), (30, 31), (25, 29), (25, 30)] {
            assert_eq!(classify(prev, next, false), Edge::None, "{prev} -> {next}");
        }
        // Already active: the same edge is in-window, nothing to do.
        assert_eq!(classify(25, 26, true), Edge::None);
    }

    #[test]
    fn restore_on_any_exit_from_the_window_while_active() {
        for (prev, next) in [(30, 31), (28, 24), (29, 24), (30, 32), (28, 34), (27, 25)] {
            assert_eq!(
                classify(prev, next, true),
                Edge::Restore,
                "{prev} -> {next}"
            );
            assert_eq!(
                classify(prev, next, false),
                Edge::None,
                "idle {prev} -> {next}"
            );
        }
    }

    #[test]
    fn reseed_on_gameplay_reentry_while_active() {
        assert_eq!(classify(27, 28, true), Edge::Reseed);
        // Quick restart: 28 -> 27 (stage loader) -> 28.
        assert_eq!(classify(28, 27, true), Edge::None);
        assert_eq!(classify(27, 28, true), Edge::Reseed);
        assert_eq!(classify(28, 28, true), Edge::Reseed);
        // In-window non-gameplay hops are silent.
        assert_eq!(classify(28, 29, true), Edge::None);
        assert_eq!(classify(29, 30, true), Edge::None);
    }

    #[test]
    fn play_window_bounds() {
        assert!(!in_play_window(25));
        for s in 26..=30 {
            assert!(in_play_window(s), "{s}");
        }
        assert!(!in_play_window(31));
        assert!(!in_play_window(24));
    }
}
