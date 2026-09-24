//! Pure rules of the `_sel` background movies (Step 6): A3's DDR SELECTION
//! movies (`data/mdb_apx/movie/<name>_sel.wmv`, 1280×720 VC-1, 18 songs of
//! 1st MIX … EXTREME) for every song played with a legacy era.
//! Dependency-free (host-tested by `scripts/validate_ddr_selection.sh`); the
//! engine side is `movie_sel.rs`.
//!
//! RE: `.agents/planning/2026-09-22-ddr-selection/research/`
//! `end-banners-sel-movies.md` §4.
//!
//! * World's `MovieActor::onInitialize` tries `<name><suffix>_sel` FIRST when
//!   its flag byte (`+0x149`) is set — a dormant flag nothing writes (A3 set
//!   it for songs picked inside the DDR SELECTION folder). `<name>` is the
//!   music-info movie override, else the entry's basename; `<suffix>` the
//!   play sequence's movie suffix (empty for every `_sel` song).
//! * The MovieActor exists only when `SceneManageActor::onInitialize` creates
//!   it: a missing music-info entry, or movie bytes that say "has a movie"
//!   ([`world_has_movie`]), AND VIDEO SIZE FULLSCREEN / ON. 11 of the 18
//!   `_sel` songs have no World movie, so for them the gate is made to pass
//!   for that one call (a scoped write of the entry's movie byte). VIDEO SIZE
//!   OFF is never overridden (no MovieActor, no movie) — unlike A3, which
//!   ignored it.

/// The movie byte written for the one gate call on a movie-less `_sel` song
/// (1st MIX's value; anything but 0 / 5 passes). Restored right after; the
/// MovieActor's own init re-reads the stock byte (and then keeps its layout
/// value 1 — the value World sets itself for `_w` / `_sel` movies).
pub const FORCED_MOVIE_KIND: u8 = 4;
/// The movie-byte value World treats as "no movie" besides 0.
pub const NO_MOVIE: u8 = 5;

/// World's `SceneManageActor::onInitialize` gate (all five builds): create a
/// MovieActor iff the entry is missing, or `b1 ∉ {0, 5}`, or `b1 == 5` and
/// `b2 ∉ {0, 5}` (`b1` = `+0x141`, `b2` = `+0x140` on every build).
pub fn world_has_movie(entry_present: bool, b1: u8, b2: u8) -> bool {
    if !entry_present {
        return true;
    }
    let decisive = if b1 == NO_MOVIE { b2 } else { b1 };
    decisive != 0 && decisive != NO_MOVIE
}

/// The game path relative to `data/` World's `_sel` probe opens
/// (`FUN_18007c890` → `FUN_18007c7b0`: `data/mdb_apx/movie/` + name + suffix
/// + `_sel` + `.wmv`).
pub fn sel_movie_rel(name: &str, suffix: &str) -> String {
    format!("mdb_apx/movie/{name}{suffix}_sel.wmv")
}

/// The movie name World composes the path from: the entry's override
/// string when non-empty, else its basename.
pub fn movie_name<'a>(override_name: &'a str, basename: &'a str) -> &'a str {
    if override_name.is_empty() {
        basename
    } else {
        override_name
    }
}

/// What the SceneManageActor detour does for one song.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Stock (no legacy era, or no `_sel` file).
    Stock,
    /// World creates the MovieActor itself; set its `_sel` flag afterwards.
    Flag,
    /// World would not create one: pass its gate for this call, then flag.
    ForceAndFlag,
}

pub fn plan(legacy_armed: bool, sel_exists: bool, world_has_movie: bool) -> Plan {
    if !legacy_armed || !sel_exists {
        Plan::Stock
    } else if world_has_movie {
        Plan::Flag
    } else {
        Plan::ForceAndFlag
    }
}

/// The MovieActor's found path is a `_sel` movie.
pub fn is_sel_path(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with("_sel.wmv")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_matches_world() {
        // Missing entry: World creates the actor (and finds no movie itself).
        assert!(world_has_movie(false, 0, 0));
        // `<movie>` absent (0) → none; 1 / 3 / 4 → movie.
        assert!(!world_has_movie(true, 0, 0));
        assert!(!world_has_movie(true, 0, 4), "b2 only counts when b1 is 5");
        for v in [1u8, 3, 4] {
            assert!(world_has_movie(true, v, 0), "{v}");
        }
        assert!(!world_has_movie(true, 5, 5));
        assert!(!world_has_movie(true, 5, 0));
        assert!(world_has_movie(true, 5, 2));
        assert!(world_has_movie(true, FORCED_MOVIE_KIND, 0));
        assert!(world_has_movie(true, FORCED_MOVIE_KIND, 5));
    }

    #[test]
    fn paths() {
        assert_eq!(sel_movie_rel("trip", ""), "mdb_apx/movie/trip_sel.wmv");
        assert_eq!(
            sel_movie_rel("goru", "_ac"),
            "mdb_apx/movie/goru_ac_sel.wmv"
        );
        assert_eq!(movie_name("", "bril"), "bril");
        assert_eq!(movie_name("maxx", "xmax"), "maxx");
        assert!(is_sel_path("data/mdb_apx/movie/trip_sel.wmv"));
        assert!(is_sel_path("DATA/MDB_APX/MOVIE/TRIP_SEL.WMV"));
        assert!(!is_sel_path("data/mdb_apx/movie/bril_w.wmv"));
    }

    /// World's musicdb (stock data): the 18 `_sel` songs, 7 with a World
    /// `<movie>` (1 / 3 / 4), 11 without.
    #[test]
    fn the_eighteen_songs() {
        let songs: [(&str, u8); 18] = [
            ("afro", 0),
            ("afte", 0),
            ("bagg", 1),
            ("bfor", 0),
            ("bom2", 0),
            ("bril", 4),
            ("burn", 4),
            ("cand", 0),
            ("drte", 0),
            ("ichi", 4),
            ("kaku", 0),
            ("maxx", 3),
            ("para", 4),
            ("para2", 0),
            ("radu", 1),
            ("roll", 0),
            ("stil", 0),
            ("trip", 0),
        ];
        let mut flag = 0;
        let mut force = 0;
        for (name, movie) in songs {
            match plan(true, true, world_has_movie(true, movie, 0)) {
                Plan::Flag => flag += 1,
                Plan::ForceAndFlag => force += 1,
                Plan::Stock => panic!("{name}"),
            }
        }
        assert_eq!((flag, force), (7, 11));
    }

    #[test]
    fn stock_unless_armed_and_present() {
        assert_eq!(plan(false, true, false), Plan::Stock);
        assert_eq!(plan(true, false, false), Plan::Stock);
        assert_eq!(plan(true, false, true), Plan::Stock);
        assert_eq!(plan(true, true, true), Plan::Flag);
        assert_eq!(plan(true, true, false), Plan::ForceAndFlag);
    }
}
