//! What songs with a background movie do while the dancers are on (the
//! GLOBAL SETTINGS row "Background Movies", config
//! `background_dancers.movie_mode`) — pure, std-only, host-tested
//! (`scripts/validate_background_dancers.sh`).
//!
//! Three modes, each applied to EVERY entered player whose VIDEO SIZE shows a
//! movie (a player with VIDEO SIZE = OFF never gets one):
//!
//! - **OFF** — no movie. The movie-size field (`Customize + 0x30`) reads 3
//!   (the game's own VIDEO SIZE OFF) for the song, and the shared BuildGraph
//!   hook suppresses the graph as a backstop (20250805's SceneManageActor
//!   has no early return for 3 and would still build one). The 3D stage
//!   shows as on a song without a movie.
//! - **THUMBNAIL** — the movie in its small "ON" window over the 3D stage
//!   (FULLSCREEN is written as ON for the song). The original behaviour.
//! - **FULLSCREEN** — the DDR 5th Mix look: the movie fills the screen
//!   behind the dancers and the stage is not drawn (ON is written as
//!   FULLSCREEN). No detour or compositor is involved: the game already
//!   draws a FULLSCREEN movie into the 3D render target BEFORE the model
//!   passes (layer-table entry 9 → the OFFSCREEN0 ScreenCommandList, which
//!   the render graph attaches to RENDER-3D at prio 0x65, ahead of the MODEL
//!   passes at 0x66..0x68, with Z test and Z write off), so the dancers land
//!   on top of it by construction — what hid the movie before was the stage
//!   geometry painted over it. Per song, the stage parts and the floor
//!   shadows are published hidden while the movie is live
//!   ([`classify`] → [`scene_mask`]), and the 2D background hide stays
//!   disarmed (the game disables the whole BackgroundFrame for a fullscreen
//!   movie on its own). Research: `docs/background_dancers_research.md` §7.
//!
//! Every value written is restored at song-window exit (in memory only; the
//! player's saved VIDEO SIZE is never touched).

/// `Customize + 0x30` values (the VIDEO SIZE option).
pub const SIZE_UNSET: u32 = 0;
pub const SIZE_FULLSCREEN: u32 = 1;
pub const SIZE_ON: u32 = 2;
pub const SIZE_OFF: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovieMode {
    Off,
    Thumbnail,
    Fullscreen,
}

impl MovieMode {
    pub const DEFAULT: MovieMode = MovieMode::Thumbnail;
    pub const ALL: [MovieMode; 3] = [MovieMode::Off, MovieMode::Thumbnail, MovieMode::Fullscreen];

    /// Config spelling.
    pub fn key(self) -> &'static str {
        match self {
            MovieMode::Off => "off",
            MovieMode::Thumbnail => "thumbnail",
            MovieMode::Fullscreen => "fullscreen",
        }
    }

    /// Mod-menu value label.
    pub fn label(self) -> &'static str {
        match self {
            MovieMode::Off => "OFF",
            MovieMode::Thumbnail => "THUMBNAIL",
            MovieMode::Fullscreen => "FULLSCREEN (NO STAGE)",
        }
    }

    pub fn parse(s: &str) -> Option<MovieMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "hide" | "hidden" | "disabled" => Some(MovieMode::Off),
            "thumbnail" | "thumb" | "on" | "small" => Some(MovieMode::Thumbnail),
            "fullscreen" | "full" | "5th" | "5thmix" | "fifth" => Some(MovieMode::Fullscreen),
            _ => None,
        }
    }

    /// Overlay-row value — OFF / THUMBNAIL / FULLSCREEN.
    pub fn row_value(self) -> i32 {
        match self {
            MovieMode::Off => 0,
            MovieMode::Thumbnail => 1,
            MovieMode::Fullscreen => 2,
        }
    }

    /// Unknown row values fall back to the default.
    pub fn from_row_value(v: i32) -> MovieMode {
        match v {
            0 => MovieMode::Off,
            2 => MovieMode::Fullscreen,
            _ => MovieMode::Thumbnail,
        }
    }
}

/// Whether `current` shows a movie at all (0 is read by the game as
/// FULLSCREEN — the getter returns 1 for an unset field).
pub fn shows_movie(current: u32) -> bool {
    matches!(current, SIZE_UNSET | SIZE_FULLSCREEN | SIZE_ON)
}

/// What the window entry writes into a side's movie-size field for `mode`,
/// or `None` to leave it alone. VIDEO SIZE OFF (3) is never touched, and
/// neither is a value the game does not define.
pub fn size_override(mode: MovieMode, current: u32) -> Option<u32> {
    if !shows_movie(current) {
        return None;
    }
    let target = match mode {
        MovieMode::Off => SIZE_OFF,
        MovieMode::Thumbnail => SIZE_ON,
        MovieMode::Fullscreen => SIZE_FULLSCREEN,
    };
    // An unset field already reads as FULLSCREEN: nothing to write.
    let effective = if current == SIZE_UNSET {
        SIZE_FULLSCREEN
    } else {
        current
    };
    (effective != target).then_some(target)
}

/// The fullscreen-movie state of the live song.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// No movie this song (none authored, VIDEO SIZE OFF, the open failed,
    /// or the graph was suppressed / faked so nothing will be drawn).
    None,
    /// A MovieActor exists and is still opening its movie.
    Pending,
    /// The movie opened for real and is being drawn under the 3D pass.
    Active,
}

/// `sequence::dance::MovieActor`'s StackStep values (its onUpdate /
/// onReceiveMessage, identical on 20250805 and 20260825).
pub const MOVIE_STEP_OPENING: i32 = 0;
pub const MOVIE_STEP_READY: i32 = 1;
pub const MOVIE_STEP_WAITING: i32 = 2;
pub const MOVIE_STEP_PLAYING: i32 = 3;
pub const MOVIE_STEP_NO_MOVIE: i32 = 4;

/// Classify the live song. `movie_step`: the MovieActor's step, `None` when
/// the live DancePlaySequence has no MovieActor (no movie / VIDEO SIZE OFF /
/// the tree is not readable). `suppressed`: the shared BuildGraph hook is
/// suppressing graphs right now (song rate, non-native suppress mode).
/// `real_opened`: the most recent BuildGraph was a REAL successful build
/// (not suppressed, not a faked fallback) — only consulted once the actor
/// reports the movie open, when that build is this movie's.
pub fn classify(movie_step: Option<i32>, suppressed: bool, real_opened: bool) -> Backdrop {
    match movie_step {
        None => Backdrop::None,
        Some(MOVIE_STEP_OPENING) => {
            if suppressed {
                Backdrop::None
            } else {
                Backdrop::Pending
            }
        }
        Some(MOVIE_STEP_READY | MOVIE_STEP_WAITING | MOVIE_STEP_PLAYING) => {
            if suppressed || !real_opened {
                Backdrop::None
            } else {
                Backdrop::Active
            }
        }
        Some(_) => Backdrop::None,
    }
}

/// Which instance kinds a frame publishes visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneMask {
    /// Stage parts (their hull twins follow — they share the part's slot).
    pub stage: bool,
    /// The dancers' floor shadows.
    pub shadows: bool,
}

impl SceneMask {
    pub const ALL: SceneMask = SceneMask {
        stage: true,
        shadows: true,
    };
    pub const DANCERS_ONLY: SceneMask = SceneMask {
        stage: false,
        shadows: false,
    };
}

/// The scene shape for `mode` and the live song's backdrop: FULLSCREEN with
/// a movie that is (or is about to be) drawn shows the dancers alone.
pub fn scene_mask(mode: MovieMode, backdrop: Backdrop) -> SceneMask {
    if mode == MovieMode::Fullscreen && backdrop != Backdrop::None {
        SceneMask::DANCERS_ONLY
    } else {
        SceneMask::ALL
    }
}

/// Whether the 2D gameplay background must be made transparent: exactly
/// when the stage is what the player sees behind the lane. With a
/// fullscreen movie backdrop the game disables the BackgroundFrame itself.
pub fn wants_bg_hide(mask: SceneMask) -> bool {
    mask.stage
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_round_trips() {
        for m in MovieMode::ALL {
            assert_eq!(MovieMode::parse(m.key()), Some(m));
            assert_eq!(MovieMode::from_row_value(m.row_value()), m);
            assert!(m.label().len() <= 24);
        }
        assert_eq!(
            MovieMode::parse(" FullScreen "),
            Some(MovieMode::Fullscreen)
        );
        assert_eq!(MovieMode::parse("bogus"), None);
        assert_eq!(MovieMode::from_row_value(99), MovieMode::DEFAULT);
        assert_eq!(MovieMode::DEFAULT, MovieMode::Thumbnail);
    }

    #[test]
    fn size_override_table() {
        use MovieMode::*;
        // OFF: every movie-showing value → 3.
        assert_eq!(size_override(Off, SIZE_UNSET), Some(SIZE_OFF));
        assert_eq!(size_override(Off, SIZE_FULLSCREEN), Some(SIZE_OFF));
        assert_eq!(size_override(Off, SIZE_ON), Some(SIZE_OFF));
        assert_eq!(size_override(Off, SIZE_OFF), None);
        // THUMBNAIL: the original rule — fullscreen (0/1) → 2, ON kept.
        assert_eq!(size_override(Thumbnail, SIZE_UNSET), Some(SIZE_ON));
        assert_eq!(size_override(Thumbnail, SIZE_FULLSCREEN), Some(SIZE_ON));
        assert_eq!(size_override(Thumbnail, SIZE_ON), None);
        assert_eq!(size_override(Thumbnail, SIZE_OFF), None);
        // FULLSCREEN: ON → 1; 0 already reads as fullscreen.
        assert_eq!(size_override(Fullscreen, SIZE_UNSET), None);
        assert_eq!(size_override(Fullscreen, SIZE_FULLSCREEN), None);
        assert_eq!(size_override(Fullscreen, SIZE_ON), Some(SIZE_FULLSCREEN));
        assert_eq!(size_override(Fullscreen, SIZE_OFF), None);
        // Values the game does not define are never touched.
        for m in MovieMode::ALL {
            assert_eq!(size_override(m, 7), None);
            assert_eq!(size_override(m, u32::MAX), None);
        }
    }

    #[test]
    fn backdrop_classification() {
        // No MovieActor ⇒ no movie.
        assert_eq!(classify(None, false, true), Backdrop::None);
        // Opening: pending unless graphs are suppressed (nothing will draw).
        assert_eq!(
            classify(Some(MOVIE_STEP_OPENING), false, false),
            Backdrop::Pending
        );
        assert_eq!(
            classify(Some(MOVIE_STEP_OPENING), true, false),
            Backdrop::None
        );
        // Opened for real ⇒ active in every post-open step.
        for s in [MOVIE_STEP_READY, MOVIE_STEP_WAITING, MOVIE_STEP_PLAYING] {
            assert_eq!(classify(Some(s), false, true), Backdrop::Active);
            // Faked / failed build or live suppression ⇒ nothing is drawn.
            assert_eq!(classify(Some(s), false, false), Backdrop::None);
            assert_eq!(classify(Some(s), true, true), Backdrop::None);
        }
        // The actor's own "no movie" step and anything unknown.
        assert_eq!(
            classify(Some(MOVIE_STEP_NO_MOVIE), false, true),
            Backdrop::None
        );
        assert_eq!(classify(Some(9), false, true), Backdrop::None);
        assert_eq!(classify(Some(-1), false, true), Backdrop::None);
    }

    #[test]
    fn scene_mask_and_bg_hide() {
        use MovieMode::*;
        for b in [Backdrop::None, Backdrop::Pending, Backdrop::Active] {
            // OFF / THUMBNAIL always show the whole scene over a hidden 2D bg.
            for m in [Off, Thumbnail] {
                assert_eq!(scene_mask(m, b), SceneMask::ALL);
                assert!(wants_bg_hide(scene_mask(m, b)));
            }
        }
        // FULLSCREEN: dancers alone over a live (or opening) movie…
        assert_eq!(
            scene_mask(Fullscreen, Backdrop::Active),
            SceneMask::DANCERS_ONLY
        );
        assert_eq!(
            scene_mask(Fullscreen, Backdrop::Pending),
            SceneMask::DANCERS_ONLY
        );
        assert!(!wants_bg_hide(scene_mask(Fullscreen, Backdrop::Active)));
        // …and the ordinary stage scene on a song without one.
        assert_eq!(scene_mask(Fullscreen, Backdrop::None), SceneMask::ALL);
        assert!(wants_bg_hide(scene_mask(Fullscreen, Backdrop::None)));
    }
}
