//! Session-window facts of the impersonation (design §4.5) — pure.
//!
//! The scene-callback decisions of `impersonation.rs` are decided here so the
//! host harness can pin them: WHEN to flip (the song-select → stage edge),
//! WHEN to restore (the first scene outside the play window), and WHEN to
//! re-seed the bot (GAMEPLAY entry, incl. a quick-restart re-entry), plus
//! the name-plate formatter. Scene ids are the game's 0-indexed sequence ids
//! written as literals (no `crate::` imports); `impersonation.rs` pins them
//! to `types::scenes::scene` with a `const` assertion.

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

/// `"BOT LV<n>"` as the game's fixed name buffer: NUL-terminated, zero
/// padded. Every level in `1..=10` fits the 8-character plate (`BOT LV10`
/// is exactly 8). Out-of-range levels clamp.
pub fn format_bot_name(level: u8) -> [u8; NAME_LEN] {
    let level = level.clamp(1, 10);
    let mut out = [0u8; NAME_LEN];
    let prefix = b"BOT LV";
    out[..prefix.len()].copy_from_slice(prefix);
    let mut i = prefix.len();
    if level >= 10 {
        out[i] = b'0' + level / 10;
        i += 1;
    }
    out[i] = b'0' + level % 10;
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
            let n = format_bot_name(level);
            let s = name_str(&n);
            assert_eq!(s, format!("BOT LV{level}"));
            assert!(s.len() <= 8, "level {level}: {s:?}");
            let nul = s.len();
            assert!(n[nul..].iter().all(|&b| b == 0), "zero padded after NUL");
        }
        assert_eq!(&format_bot_name(1), b"BOT LV1\0\0");
        assert_eq!(&format_bot_name(10), b"BOT LV10\0");
    }

    #[test]
    fn bot_name_clamps() {
        assert_eq!(name_str(&format_bot_name(0)), "BOT LV1");
        assert_eq!(name_str(&format_bot_name(11)), "BOT LV10");
        assert_eq!(name_str(&format_bot_name(255)), "BOT LV10");
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
