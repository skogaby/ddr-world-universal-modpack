//! What songs with a background movie do while the dancers are on (the
//! GLOBAL SETTINGS row "Background Movies", config
//! `background_dancers.movie_mode`) — pure, std-only, host-tested
//! (`scripts/validate_background_dancers.sh`).
//!
//! Each mode applies to EVERY entered player whose VIDEO SIZE shows a
//! movie (a player with VIDEO SIZE = OFF never gets one):
//!
//! - **OFF** — no movie. The movie-size field (`Customize + 0x30`) reads 3
//!   (the game's own VIDEO SIZE OFF) for the song, and the shared BuildGraph
//!   hook suppresses the graph as a backstop (20250805's SceneManageActor
//!   has no early return for 3 and would still build one). The 3D stage
//!   shows as on a song without a movie.
//! - **THUMBNAIL** — the movie in its small "ON" window over the 3D stage
//!   (FULLSCREEN is written as ON for the song). The original behaviour,
//!   and what STAGE SCREENS does on a stage without screens.
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
//!
//! - **STAGE SCREENS** (2026-09-23, research §8; THE DEFAULT since the
//!   cabinet pass the same day) — DDR A3's look: when the
//!   song's 3D stage has video screens (a material textured `offscreen1`,
//!   detected at enable from the stage arc's `offscreen1.dds` placeholder —
//!   [`arc_members_have_screen`]), the movie plays ON those screens: VIDEO
//!   SIZE is written as FULLSCREEN (so the MovieActor's thumbnail flag stays
//!   clear), the MovieActor's layer choice is routed from entry 9 to entry
//!   10 (the OFFSCREEN1 render target the screens sample — one checked code
//!   byte, [`imm_action`]) and its fit rectangle is set to A3's contain fit
//!   in the 1280 × 1280 square ([`SCREEN_RT_EXTENT`], [`fit_writable`]). A
//!   stage without screens plays the song exactly as THUMBNAIL
//!   ([`window_mode`]).
//! - **MOVIE ONLY (NO DANCERS)** (2026-09-23) — DDR A3's default for an
//!   ordinary movie song: VIDEO SIZE is left as the player set it, the 3D
//!   scene loads as usual, and while the live song's movie is actually drawn
//!   (the same probe as FULLSCREEN — [`probes_backdrop`]) EVERY 3D element
//!   is published hidden ([`SceneMask::NOTHING`]) and the 2D gameplay
//!   background is left to the game: the song looks like stock World. A
//!   player with VIDEO SIZE OFF, a song without a movie, or a suppressed /
//!   faked movie keeps the dancers.

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
    StageScreens,
    MovieOnly,
}

impl MovieMode {
    /// STAGE SCREENS (cabinet-proven 2026-09-23): the movie on the stage's
    /// screens where it has some, the THUMBNAIL everywhere else.
    pub const DEFAULT: MovieMode = MovieMode::StageScreens;
    /// Display order of the mod-menu row (the row is index-based, so the
    /// stored row values need not be monotonic).
    pub const ALL: [MovieMode; 5] = [
        MovieMode::Off,
        MovieMode::Thumbnail,
        MovieMode::StageScreens,
        MovieMode::Fullscreen,
        MovieMode::MovieOnly,
    ];

    /// Config spelling.
    pub fn key(self) -> &'static str {
        match self {
            MovieMode::Off => "off",
            MovieMode::Thumbnail => "thumbnail",
            MovieMode::Fullscreen => "fullscreen",
            MovieMode::StageScreens => "stage_screens",
            MovieMode::MovieOnly => "movie_only",
        }
    }

    /// Mod-menu value label.
    pub fn label(self) -> &'static str {
        match self {
            MovieMode::Off => "OFF",
            MovieMode::Thumbnail => "THUMBNAIL",
            MovieMode::Fullscreen => "FULLSCREEN (NO STAGE)",
            MovieMode::StageScreens => "STAGE SCREENS",
            MovieMode::MovieOnly => "MOVIE ONLY (NO DANCERS)",
        }
    }

    pub fn parse(s: &str) -> Option<MovieMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" | "none" | "hide" | "hidden" | "disabled" => Some(MovieMode::Off),
            "thumbnail" | "thumb" | "on" | "small" => Some(MovieMode::Thumbnail),
            "fullscreen" | "full" | "5th" | "5thmix" | "fifth" => Some(MovieMode::Fullscreen),
            "stage_screens" | "stage-screens" | "stagescreens" | "screens" | "monitor"
            | "monitors" => Some(MovieMode::StageScreens),
            "movie_only" | "movie-only" | "movieonly" | "no_dancers" | "no-dancers" | "a3" => {
                Some(MovieMode::MovieOnly)
            }
            _ => None,
        }
    }

    /// Overlay-row value — OFF 0 / THUMBNAIL 1 / FULLSCREEN 2 / STAGE
    /// SCREENS 3 / MOVIE ONLY 4 (the original three kept their values).
    pub fn row_value(self) -> i32 {
        match self {
            MovieMode::Off => 0,
            MovieMode::Thumbnail => 1,
            MovieMode::Fullscreen => 2,
            MovieMode::StageScreens => 3,
            MovieMode::MovieOnly => 4,
        }
    }

    /// Unknown row values fall back to the default.
    pub fn from_row_value(v: i32) -> MovieMode {
        match v {
            0 => MovieMode::Off,
            2 => MovieMode::Fullscreen,
            1 => MovieMode::Thumbnail,
            3 => MovieMode::StageScreens,
            4 => MovieMode::MovieOnly,
            _ => MovieMode::DEFAULT,
        }
    }

    /// Every config spelling, for the unknown-value WARN.
    pub fn keys_list() -> String {
        MovieMode::ALL
            .iter()
            .map(|m| m.key())
            .collect::<Vec<_>>()
            .join("/")
    }
}

/// What this boot can honour (each mode's dependencies).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// The movie-size override (`movie_size::is_available`).
    pub movie_size: bool,
    /// The live-movie probe (`movie_backdrop::is_available`).
    pub probe: bool,
    /// The STAGE SCREENS route (`screen_route::is_available` — both
    /// signatures of `derive_movie_screen_route`).
    pub route: bool,
}

/// The mode a boot can honour for `requested`: FULLSCREEN needs the
/// movie-size write and the probe; STAGE SCREENS additionally the route;
/// MOVIE ONLY only the probe (it never writes VIDEO SIZE). Any miss ⇒
/// THUMBNAIL (the original behaviour).
pub fn degrade(requested: MovieMode, caps: Capabilities) -> MovieMode {
    let ok = match requested {
        MovieMode::Off | MovieMode::Thumbnail => true,
        MovieMode::Fullscreen => caps.movie_size && caps.probe,
        MovieMode::StageScreens => caps.movie_size && caps.probe && caps.route,
        MovieMode::MovieOnly => caps.probe,
    };
    if ok {
        requested
    } else {
        MovieMode::Thumbnail
    }
}

/// The mode one song window runs: STAGE SCREENS on a stage without screens
/// (or with no stage at all) is exactly THUMBNAIL.
pub fn window_mode(mode: MovieMode, stage_has_screens: bool) -> MovieMode {
    if mode == MovieMode::StageScreens && !stage_has_screens {
        MovieMode::Thumbnail
    } else {
        mode
    }
}

/// Whether a WINDOW mode routes the movie onto the stage's screens.
pub fn routes_to_screens(mode: MovieMode) -> bool {
    mode == MovieMode::StageScreens
}

/// Whether the song about to play will have a background movie, as far as
/// the window entry can tell (before the DancePlaySequence exists).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SongMovie {
    /// The music DB says the song has a movie, an entered side's VIDEO SIZE
    /// shows one and no BuildGraph suppressor is set.
    Plays,
    /// No movie will be drawn: none authored, every entered side has VIDEO
    /// SIZE OFF, or the graph is suppressed (song rate, non-native suppress
    /// mode).
    None,
    /// The song could not be determined (course, lookup unavailable, …).
    Unknown,
}

/// Which stages a RANDOM stage draw may land on, by whether they have video
/// screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenFilter {
    /// Only stages WITH screens (the movie plays on them).
    WithScreens,
    /// Only stages WITHOUT screens (screens would stay black).
    WithoutScreens,
}

impl ScreenFilter {
    /// Whether a stage with / without screens passes the filter.
    pub fn keeps(self, stage_has_screens: bool) -> bool {
        match self {
            ScreenFilter::WithScreens => stage_has_screens,
            ScreenFilter::WithoutScreens => !stage_has_screens,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ScreenFilter::WithScreens => "stages with screens only",
            ScreenFilter::WithoutScreens => "stages without screens only",
        }
    }
}

/// The RANDOM stage pool's screen rule. Under STAGE SCREENS a song whose
/// movie plays draws only from the screen stages (the movie on the screens
/// — never demoted to a THUMBNAIL by a screen-less stage); in every other
/// case — including a song whose movie state cannot be determined — a
/// screen stage's screens could stay black, so RANDOM draws only from the
/// stages without screens. (An explicitly chosen BACKGROUND STAGE is never
/// filtered.) `mode` is the mode the boot can honour (after [`degrade`]).
pub fn random_pool_filter(mode: MovieMode, song: SongMovie) -> ScreenFilter {
    if routes_to_screens(mode) && song == SongMovie::Plays {
        ScreenFilter::WithScreens
    } else {
        ScreenFilter::WithoutScreens
    }
}

/// Whether a WINDOW mode needs the per-frame live-movie probe (its scene
/// shape depends on whether the movie is drawn).
pub fn probes_backdrop(mode: MovieMode) -> bool {
    matches!(mode, MovieMode::Fullscreen | MovieMode::MovieOnly)
}

/// The texture name a screen material samples: the engine's OFFSCREEN1
/// render target is registered at boot under this (already folded) name.
pub const SCREEN_TEXTURE_STEM: &str = "offscreen1";

/// The placeholder DDS every screen stage ships beside its screen part.
const SCREEN_PLACEHOLDER_FILE: &str = "offscreen1.dds";

/// Whether a stage arc's member list says "this stage has screens": any
/// member whose file name is `offscreen1.dds` (ASCII case ignored). True for
/// exactly the ten stock screen stages and for every add-on export that
/// names its screen image `offscreen1`.
pub fn arc_members_have_screen(members: &[String]) -> bool {
    members.iter().any(|m| {
        let name = m.rsplit(['/', '\\']).next().unwrap_or("");
        name.eq_ignore_ascii_case(SCREEN_PLACEHOLDER_FILE)
    })
}

/// Side of the OFFSCREEN1 square (the layer-table entry 10 canvas) — A3's
/// monitor fit is origin (0, 0), size (1280, 1280).
pub const SCREEN_RT_EXTENT: f64 = 1280.0;

/// Whether the MovieActor's fit rectangle may still be written: the fit is
/// applied only while the actor is at step 2 (its 0x1045 case), then frozen
/// at the 2 → 3 transition — so steps 0 / 1 / 2 only.
pub fn fit_writable(step: i32) -> bool {
    matches!(
        step,
        MOVIE_STEP_OPENING | MOVIE_STEP_READY | MOVIE_STEP_WAITING
    )
}

/// The two values of the MovieActor layer-choice imm8
/// (`signatures::derive_movie_screen_route`).
pub struct RouteImm;

impl RouteImm {
    /// Layer-table entry 10 — OFFSCREEN1 (the stage screens).
    pub const ROUTED: u8 = 0x0A;
    /// Layer-table entry 9 — the stock fullscreen movie (OFFSCREEN0).
    pub const STOCK: u8 = 0x09;
}

/// What to do with the imm8 given what it reads now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImmAction {
    /// Write this value.
    Write(u8),
    /// It already holds the wanted value.
    Already,
    /// It holds something unknown — never write over it.
    Refuse,
}

/// Checked-write rule: write only over the OTHER known value.
pub fn imm_action(current: u8, want_routed: bool) -> ImmAction {
    let (want, other) = if want_routed {
        (RouteImm::ROUTED, RouteImm::STOCK)
    } else {
        (RouteImm::STOCK, RouteImm::ROUTED)
    };
    if current == want {
        ImmAction::Already
    } else if current == other {
        ImmAction::Write(want)
    } else {
        ImmAction::Refuse
    }
}

/// Whether `current` shows a movie at all (0 is read by the game as
/// FULLSCREEN — the getter returns 1 for an unset field).
pub fn shows_movie(current: u32) -> bool {
    matches!(current, SIZE_UNSET | SIZE_FULLSCREEN | SIZE_ON)
}

/// What the window entry writes into a side's movie-size field for `mode`,
/// or `None` to leave it alone. VIDEO SIZE OFF (3) is never touched, and
/// neither is a value the game does not define; MOVIE ONLY never writes
/// (the player's VIDEO SIZE decides the movie). Called with the WINDOW mode
/// ([`window_mode`]): an unrouted STAGE SCREENS song arrives as THUMBNAIL;
/// a routed one needs FULLSCREEN (the MovieActor picks entry 0 — the 2D
/// thumbnail — whenever its thumbnail flag is set, whatever the route says).
pub fn size_override(mode: MovieMode, current: u32) -> Option<u32> {
    if !shows_movie(current) {
        return None;
    }
    let target = match mode {
        MovieMode::Off => SIZE_OFF,
        MovieMode::Thumbnail => SIZE_ON,
        MovieMode::Fullscreen | MovieMode::StageScreens => SIZE_FULLSCREEN,
        MovieMode::MovieOnly => return None,
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

/// Which instance kinds a frame publishes visible (outline twins follow the
/// slot they read — the part's / the body's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneMask {
    /// Stage parts.
    pub stage: bool,
    /// The dancers' floor shadows.
    pub shadows: bool,
    /// Dancer bodies and their accessory parts.
    pub dancers: bool,
}

impl SceneMask {
    pub const ALL: SceneMask = SceneMask {
        stage: true,
        shadows: true,
        dancers: true,
    };
    pub const DANCERS_ONLY: SceneMask = SceneMask {
        stage: false,
        shadows: false,
        dancers: true,
    };
    pub const NOTHING: SceneMask = SceneMask {
        stage: false,
        shadows: false,
        dancers: false,
    };
}

/// The scene shape for `mode` and the live song's backdrop: FULLSCREEN with
/// a movie that is (or is about to be) drawn shows the dancers alone; MOVIE
/// ONLY shows nothing at all.
pub fn scene_mask(mode: MovieMode, backdrop: Backdrop) -> SceneMask {
    match mode {
        MovieMode::Fullscreen if backdrop != Backdrop::None => SceneMask::DANCERS_ONLY,
        MovieMode::MovieOnly if backdrop != Backdrop::None => SceneMask::NOTHING,
        _ => SceneMask::ALL,
    }
}

/// Whether the 2D gameplay background must be made transparent: exactly
/// when the stage is what the player sees behind the lane. With a
/// fullscreen movie backdrop the game disables the BackgroundFrame itself;
/// with MOVIE ONLY's empty scene the game's own 2D background shows.
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
        assert_eq!(MovieMode::DEFAULT, MovieMode::StageScreens);
        assert_eq!(MovieMode::from_row_value(-1), MovieMode::StageScreens);
    }

    #[test]
    fn row_values_and_display_order() {
        use MovieMode::*;
        // The original three keep their stored values; new values append.
        assert_eq!(Off.row_value(), 0);
        assert_eq!(Thumbnail.row_value(), 1);
        assert_eq!(Fullscreen.row_value(), 2);
        assert_eq!(StageScreens.row_value(), 3);
        // Display order: STAGE SCREENS between THUMBNAIL and FULLSCREEN.
        assert_eq!(
            MovieMode::ALL,
            [Off, Thumbnail, StageScreens, Fullscreen, MovieOnly]
        );
        // Row values and keys are unique.
        let mut rows: Vec<i32> = MovieMode::ALL.iter().map(|m| m.row_value()).collect();
        rows.sort_unstable();
        rows.dedup();
        assert_eq!(rows.len(), MovieMode::ALL.len());
        assert_eq!(StageScreens.key(), "stage_screens");
        assert_eq!(StageScreens.label(), "STAGE SCREENS");
        for s in [
            "stage_screens",
            "Stage-Screens",
            "screens",
            "MONITOR",
            "monitors",
        ] {
            assert_eq!(MovieMode::parse(s), Some(StageScreens), "{s}");
        }
        assert_eq!(MovieOnly.row_value(), 4);
        assert_eq!(MovieOnly.key(), "movie_only");
        assert_eq!(MovieOnly.label(), "MOVIE ONLY (NO DANCERS)");
        for s in ["movie_only", "Movie-Only", "no_dancers", "A3"] {
            assert_eq!(MovieMode::parse(s), Some(MovieOnly), "{s}");
        }
        assert_eq!(
            MovieMode::keys_list(),
            "off/thumbnail/stage_screens/fullscreen/movie_only"
        );
    }

    #[test]
    fn stage_screens_size_override() {
        use MovieMode::*;
        // A routed song needs the thumbnail flag clear: ON → FULLSCREEN.
        assert_eq!(size_override(StageScreens, SIZE_UNSET), None);
        assert_eq!(size_override(StageScreens, SIZE_FULLSCREEN), None);
        assert_eq!(size_override(StageScreens, SIZE_ON), Some(SIZE_FULLSCREEN));
        // VIDEO SIZE OFF stays off (black screens).
        assert_eq!(size_override(StageScreens, SIZE_OFF), None);
    }

    #[test]
    fn degrade_truth_table() {
        use MovieMode::*;
        let caps = |movie_size, probe, route| Capabilities {
            movie_size,
            probe,
            route,
        };
        for ms in [false, true] {
            for pr in [false, true] {
                for rt in [false, true] {
                    let c = caps(ms, pr, rt);
                    // OFF / THUMBNAIL never degrade.
                    assert_eq!(degrade(Off, c), Off);
                    assert_eq!(degrade(Thumbnail, c), Thumbnail);
                    assert_eq!(
                        degrade(Fullscreen, c),
                        if ms && pr { Fullscreen } else { Thumbnail }
                    );
                    assert_eq!(
                        degrade(StageScreens, c),
                        if ms && pr && rt {
                            StageScreens
                        } else {
                            Thumbnail
                        }
                    );
                    assert_eq!(
                        degrade(MovieOnly, c),
                        if pr { MovieOnly } else { Thumbnail }
                    );
                }
            }
        }
    }

    #[test]
    fn random_pool_screen_rule() {
        use MovieMode::*;
        use ScreenFilter::*;
        // STAGE SCREENS + a movie: only screen stages (the movie on them).
        assert_eq!(
            random_pool_filter(StageScreens, SongMovie::Plays),
            WithScreens
        );
        // STAGE SCREENS + no movie: the screens would be black.
        assert_eq!(
            random_pool_filter(StageScreens, SongMovie::None),
            WithoutScreens
        );
        // Undeterminable song: the safe side (screens might stay black).
        assert_eq!(
            random_pool_filter(StageScreens, SongMovie::Unknown),
            WithoutScreens
        );
        // Any other mode never feeds the screens.
        for m in [Off, Thumbnail, Fullscreen, MovieOnly] {
            for s in [SongMovie::Plays, SongMovie::None, SongMovie::Unknown] {
                assert_eq!(random_pool_filter(m, s), WithoutScreens, "{m:?} {s:?}");
            }
        }
        assert!(WithScreens.keeps(true) && !WithScreens.keeps(false));
        assert!(!WithoutScreens.keeps(true) && WithoutScreens.keeps(false));
    }

    #[test]
    fn window_mode_and_routing() {
        use MovieMode::*;
        assert_eq!(window_mode(StageScreens, true), StageScreens);
        // No screens (or no stage — the caller passes false) ⇒ THUMBNAIL.
        assert_eq!(window_mode(StageScreens, false), Thumbnail);
        for m in [Off, Thumbnail, Fullscreen, MovieOnly] {
            assert_eq!(window_mode(m, true), m);
            assert_eq!(window_mode(m, false), m);
            assert!(!routes_to_screens(m));
        }
        assert!(routes_to_screens(StageScreens));
        // The live probe runs for the two modes whose scene depends on it.
        for m in MovieMode::ALL {
            assert_eq!(probes_backdrop(m), matches!(m, Fullscreen | MovieOnly));
        }
    }

    #[test]
    fn screen_member_detection() {
        let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // A stock screen stage: one placeholder beside the screen part.
        assert!(arc_members_have_screen(&v(&[
            "data/map/gm_monitor00_bg/gm_monitor00_bg.model",
            "data/map/gm_monitor00_monitor/gm_monitor00_monitor.model",
            "data/map/gm_monitor00_monitor/offscreen1.dds",
        ])));
        // A stage without screens.
        assert!(!arc_members_have_screen(&v(&[
            "data/map/gm_boom00_bg/gm_boom00_bg.model",
            "data/map/gm_boom00_bg/bg.dds",
        ])));
        // Case is ignored; a bare member name counts.
        assert!(arc_members_have_screen(&v(&["data/map/x/OffScreen1.DDS"])));
        assert!(arc_members_have_screen(&v(&["offscreen1.dds"])));
        assert!(arc_members_have_screen(&v(&[
            "data\\map\\x\\offscreen1.dds"
        ])));
        // Only the FILE name counts — a directory of that name does not,
        // nor a longer name.
        assert!(!arc_members_have_screen(&v(&[
            "data/map/offscreen1.dds/foo.dds"
        ])));
        assert!(!arc_members_have_screen(&v(&[
            "data/map/x/offscreen1.dds/"
        ])));
        assert!(!arc_members_have_screen(&v(&[
            "data/map/x/offscreen10.dds"
        ])));
        assert!(!arc_members_have_screen(&v(&["data/map/x/offscreen1.png"])));
        assert!(!arc_members_have_screen(&[]));
    }

    #[test]
    fn fit_window() {
        assert!(fit_writable(MOVIE_STEP_OPENING));
        assert!(fit_writable(MOVIE_STEP_READY));
        assert!(fit_writable(MOVIE_STEP_WAITING));
        assert!(!fit_writable(MOVIE_STEP_PLAYING));
        assert!(!fit_writable(MOVIE_STEP_NO_MOVIE));
        assert!(!fit_writable(-1));
        assert!(!fit_writable(5));
        assert_eq!(SCREEN_RT_EXTENT, 1280.0);
        assert_eq!(SCREEN_TEXTURE_STEM, "offscreen1");
    }

    #[test]
    fn imm_checked_write() {
        // Arm: only from the stock value.
        assert_eq!(
            imm_action(RouteImm::STOCK, true),
            ImmAction::Write(RouteImm::ROUTED)
        );
        assert_eq!(imm_action(RouteImm::ROUTED, true), ImmAction::Already);
        // Disarm: only from the routed value.
        assert_eq!(
            imm_action(RouteImm::ROUTED, false),
            ImmAction::Write(RouteImm::STOCK)
        );
        assert_eq!(imm_action(RouteImm::STOCK, false), ImmAction::Already);
        // Anything else is someone else's patch — never written over.
        for b in [0x00u8, 0x08, 0x0B, 0x90, 0xFF] {
            assert_eq!(imm_action(b, true), ImmAction::Refuse);
            assert_eq!(imm_action(b, false), ImmAction::Refuse);
        }
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
            // OFF / THUMBNAIL / STAGE SCREENS always show the whole scene
            // over a hidden 2D bg.
            for m in [Off, Thumbnail, StageScreens] {
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
        assert!(SceneMask::DANCERS_ONLY.dancers);
        // MOVIE ONLY: nothing over a live (or opening) movie, the game's own
        // 2D background; the whole scene without one.
        for b in [Backdrop::Pending, Backdrop::Active] {
            assert_eq!(scene_mask(MovieOnly, b), SceneMask::NOTHING);
            assert!(!wants_bg_hide(scene_mask(MovieOnly, b)));
        }
        assert_eq!(scene_mask(MovieOnly, Backdrop::None), SceneMask::ALL);
        assert!(wants_bg_hide(scene_mask(MovieOnly, Backdrop::None)));
        assert!(!SceneMask::NOTHING.dancers);
        assert!(!SceneMask::NOTHING.stage);
        assert!(!SceneMask::NOTHING.shadows);
    }

    #[test]
    fn movie_only_never_writes_video_size() {
        for v in [SIZE_UNSET, SIZE_FULLSCREEN, SIZE_ON, SIZE_OFF, 7] {
            assert_eq!(size_override(MovieMode::MovieOnly, v), None);
        }
    }
}
