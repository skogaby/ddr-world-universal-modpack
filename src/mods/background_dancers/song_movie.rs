//! Whether the song about to play has a background movie, read at the song
//! window entry (game thread, before `createNextSequence` — there is no
//! DancePlaySequence / MovieActor yet), for the RANDOM stage pool's screen
//! rule (`movie_mode::random_pool_filter`).
//!
//! The song is the entered sides' COMMITTED mcode (`PlayerWork + 0x54`,
//! written by the song-select commit; versus sides share it). Its music-DB
//! entry (`find_music_by_mcode` — the same entry object World's
//! `SceneManageActor::onInitialize` movie gate looks up by basename) carries
//! the two movie bytes that gate reads (`ddr_sel_music_movie_kind_off` /
//! `_kind2_off`, published by `derive_ddr_sel_movie`); the gate rule itself
//! is `ddr_selection::sel_movie_logic::world_has_movie`. On top of the DB:
//! no entered side's VIDEO SIZE shows a movie, or the shared BuildGraph hook
//! suppresses graphs (song rate without SYNC BACKGROUND VIDEO — its scene-26
//! arm runs before this mod's callback — or the non-native suppress mode) ⇒
//! no movie.
//!
//! Courses (the per-stage song is not the committed mcode), a missing
//! derivation or an unreadable entry ⇒ `Unknown` (one INFO per cause), which
//! the pool rule treats like no movie (stages without screens only).
//! Not modelled: DDR SELECTION's `_sel` movies for songs World ships
//! without one — they are decided later, and treating those songs as
//! movie-less only keeps screen stages out of their random pool.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::OnceLock;

use crate::core::memory;
use crate::core::signatures::SignatureStore;
use crate::log_info;
use crate::mods::ddr_selection::sel_movie_logic;
use crate::services::{movie_policy, stage_records};

use super::movie_mode::SongMovie;
use super::movie_size;

/// `PlayerWork` offset of the committed mcode (the song-select commit's
/// write; the wheel highlight lives elsewhere).
const PW_COMMITTED_MCODE: usize = 0x54;

struct Lookup {
    find_music_by_mcode: unsafe extern "C" fn(i32) -> *mut u8,
    kind_off: usize,
    kind2_off: usize,
}

static LOOKUP: OnceLock<Lookup> = OnceLock::new();
/// `Unknown` causes already logged this boot (bit per cause).
static LOGGED: AtomicU32 = AtomicU32::new(0);
const L_LOOKUP: u32 = 1;
const L_COURSE: u32 = 2;
const L_MCODE: u32 = 4;
const L_ENTRY: u32 = 8;

fn once(bit: u32) -> bool {
    LOGGED.fetch_or(bit, Ordering::Relaxed) & bit == 0
}

/// Resolve the lookup (mod init). Missing ⇒ every song is `Unknown`.
pub fn init(signatures: &SignatureStore) -> bool {
    // The movie-byte offsets ride `derive_ddr_sel_movie` (all-or-nothing).
    let sites = signatures.ddr_sel_movie_sites();
    let (Some(find), Some(kind_off), Some(kind2_off)) = (
        signatures.get_address("find_music_by_mcode"),
        sites.as_ref().map(|s| s.movie_kind_off),
        sites.as_ref().map(|s| s.movie_kind2_off),
    ) else {
        log_info!(
            "BackgroundDancers: song movie lookup unresolved (find_music_by_mcode / music movie bytes) -- RANDOM stage draws from stages without screens only"
        );
        return false;
    };
    let _ = LOOKUP.set(Lookup {
        // SAFETY: derived function address (`int mcode -> entry*`).
        find_music_by_mcode: unsafe { std::mem::transmute::<*const u8, _>(find) },
        kind_off,
        kind2_off,
    });
    true
}

fn in_course() -> Option<bool> {
    let off = stage_records::course_field_offset();
    if off == 0 {
        return None;
    }
    let gw = stage_records::game_work().filter(|gw| memory::is_readable(*gw, off + 8))?;
    // SAFETY: probed readable.
    Some(unsafe { memory::read_u64(gw.add(off)) } != 0)
}

fn committed_mcode(side: usize) -> Option<i32> {
    let pw = stage_records::player_work(side)?;
    if !memory::is_readable(pw, PW_COMMITTED_MCODE + 4) {
        return None;
    }
    // SAFETY: probed readable.
    Some(unsafe { memory::read_i32(pw.add(PW_COMMITTED_MCODE)) })
}

/// Whether the music DB gives the committed song a movie.
fn db_has_movie(entered: [bool; 2]) -> Option<bool> {
    let Some(l) = LOOKUP.get() else {
        if once(L_LOOKUP) {
            log_info!("BackgroundDancers: song movie lookup unavailable -- song movie unknown");
        }
        return None;
    };
    if in_course() != Some(false) {
        if once(L_COURSE) {
            log_info!(
                "BackgroundDancers: course (or course state unreadable) -- song movie unknown; RANDOM stage draws from stages without screens only"
            );
        }
        return None;
    }
    let Some(mcode) = (0..2)
        .filter(|&s| entered[s])
        .filter_map(committed_mcode)
        .find(|&m| m > 0)
    else {
        if once(L_MCODE) {
            log_info!("BackgroundDancers: no committed mcode readable -- song movie unknown");
        }
        return None;
    };
    // SAFETY: game-thread call of the game's own music-DB lookup.
    let entry = unsafe { (l.find_music_by_mcode)(mcode) };
    if entry.is_null() || !memory::is_readable(entry, l.kind_off.max(l.kind2_off) + 1) {
        if once(L_ENTRY) {
            log_info!(
                "BackgroundDancers: music-DB entry of mcode {} unreadable -- song movie unknown",
                mcode
            );
        }
        return None;
    }
    // SAFETY: probed readable.
    let (b1, b2) = unsafe {
        (
            memory::read_u8(entry.add(l.kind_off)),
            memory::read_u8(entry.add(l.kind2_off)),
        )
    };
    Some(sel_movie_logic::world_has_movie(true, b1, b2))
}

/// The committed song's movie state (window entry, game thread).
pub fn committed_song_movie(entered: [bool; 2]) -> SongMovie {
    if movie_policy::should_suppress() {
        return SongMovie::None;
    }
    if movie_size::is_available() && !movie_size::any_shows_movie(entered) {
        return SongMovie::None;
    }
    match db_has_movie(entered) {
        Some(true) => SongMovie::Plays,
        Some(false) => SongMovie::None,
        None => SongMovie::Unknown,
    }
}
