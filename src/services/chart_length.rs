//! Chart-length service — song lengths derived from SSQ chart data,
//! computed on a background worker and cached by song code.
//!
//! Consumers:
//! * `music_wheel_song_length` — drives requests the frame the wheel
//!   selection changes (it owns the selection poll) and displays the
//!   answer as `LENGTH M:SS`;
//! * `training_mode` — seeds its SONG START/END rows (and everything
//!   scaled from the seeded end: row ranges, section previews) from
//!   [`latest`], the most recent completed parse, instead of waiting for
//!   the wheel-settle audio publication.
//!
//! ## Semantics
//!
//! Length = the LAST event time across every step chunk (taps, freeze
//! releases — offsets are stored ascending, and freeze ends are entries
//! of their own per `docs/ssq_format.md` §5.4), converted through the
//! tempo chunk with `core::ssq::timing::TempoConverter` — the same
//! bit-exact beat→seconds math the gameplay engine uses — and rounded UP
//! to a whole second. Chart length ≤ audio length (no outro silence);
//! it is the honest "how long do I play this song" number, matching the
//! original hex-edit mod's chart-derived display.
//!
//! ## Mechanics
//!
//! Requests are latest-wins: the single worker drains its queue and
//! serves only the newest (wheel scrolling floods requests; only the
//! resting selection matters). Results land in a code-keyed cache
//! (revisits answer instantly with no I/O) plus a [`latest`] cell carrying
//! the most recent COMPLETED parse with its `song_code_digest` — the
//! digest-keyed consumers (training seeding) match against it without
//! knowing the code. SSQ files are KBs; a cold parse typically completes
//! within a frame or two of the request.
//!
//! LayeredFS-aware: custom songs ship SSQs in mod folders, so the
//! mod-paths lookup runs before the stock `data/mdb_apx/ssq/` path.
//!
//! Failure model: missing/unparseable SSQ caches as `Failed` — consumers
//! fall back to the audio-length publication (`song_rate::selected_song`).
//! The worker body is panic-contained; a panicking blob caches as
//! `Failed` with one WARN.

use std::collections::HashMap;
use std::sync::{mpsc, Mutex, OnceLock};

use crate::core::ssq::{ssq_chunk, timing::TempoConverter};
use crate::log_warn;
use crate::services::song_rate::binding::song_code_digest;

/// Cache entry / lookup outcome for one song code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// No request has been dispatched for this code yet.
    Unknown,
    /// A request is in flight (or queued behind a newer one).
    Pending,
    /// Parsed: chart length in seconds (rounded up).
    Ready(u32),
    /// SSQ missing/unparseable — use a fallback length source.
    Failed,
}

/// The most recent COMPLETED parse (either outcome), digest-stamped.
#[derive(Clone)]
pub struct Latest {
    pub code_digest: u64,
    /// `Some(secs)` on success; `None` = failed parse.
    pub secs: Option<u32>,
}

struct Inner {
    cache: HashMap<String, State>,
    latest: Option<Latest>,
}

static INNER: Mutex<Option<Inner>> = Mutex::new(None);
static TX: OnceLock<mpsc::Sender<String>> = OnceLock::new();

fn with_inner<R>(f: impl FnOnce(&mut Inner) -> R) -> R {
    let mut guard = INNER.lock().unwrap();
    let inner = guard.get_or_insert_with(|| Inner {
        cache: HashMap::new(),
        latest: None,
    });
    f(inner)
}

/// Look up a code's state (cache only — never dispatches).
pub fn get(code: &str) -> State {
    with_inner(|inner| inner.cache.get(code).copied().unwrap_or(State::Unknown))
}

/// The most recent completed parse, if any.
pub fn latest() -> Option<Latest> {
    with_inner(|inner| inner.latest.clone())
}

/// Ensure a parse for `code` is cached or in flight. Cheap to call
/// repeatedly (cache hit = one map lookup). Callers poll [`get`] (or
/// [`latest`]) afterwards.
pub fn request(code: &str) {
    let dispatch = with_inner(|inner| match inner.cache.get(code) {
        Some(State::Pending) | Some(State::Ready(_)) | Some(State::Failed) => false,
        _ => {
            inner.cache.insert(code.to_string(), State::Pending);
            true
        }
    });
    if dispatch {
        let _ = sender().send(code.to_string());
    }
}

fn sender() -> &'static mpsc::Sender<String> {
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::Builder::new()
            .name("chart-length".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    // Latest-wins: drain the queue, park the superseded
                    // codes back to Unknown so a revisit re-dispatches.
                    let mut code = first;
                    let mut superseded: Vec<String> = Vec::new();
                    while let Ok(next) = rx.try_recv() {
                        superseded.push(std::mem::replace(&mut code, next));
                    }
                    if !superseded.is_empty() {
                        with_inner(|inner| {
                            for old in &superseded {
                                if inner.cache.get(old) == Some(&State::Pending) {
                                    inner.cache.insert(old.clone(), State::Unknown);
                                }
                            }
                        });
                    }

                    let secs =
                        std::panic::catch_unwind(|| parse_for_code(&code)).unwrap_or_else(|_| {
                            log_warn!(
                                "chart_length: SSQ parse panicked for '{}' — treating as failed",
                                code
                            );
                            None
                        });
                    let state = match secs {
                        Some(s) => State::Ready(s),
                        None => State::Failed,
                    };
                    with_inner(|inner| {
                        inner.cache.insert(code.clone(), state);
                        inner.latest = Some(Latest {
                            code_digest: song_code_digest(&code),
                            secs,
                        });
                    });
                }
            })
            .expect("chart-length worker spawn");
        tx
    })
}

/// Resolve + read + parse one song's SSQ; `None` on any failure.
fn parse_for_code(code: &str) -> Option<u32> {
    use crate::services::avs_layeredfs::mod_paths;
    let rel = format!("mdb_apx/ssq/{}.ssq", code);
    let path = mod_paths::find_first_modfile(&rel).unwrap_or_else(|| format!("data/{}", rel));
    let blob = std::fs::read(&path).ok()?;
    chart_length_secs(&blob)
}

/// Chart length in seconds from an SSQ blob: the latest last-event time
/// across every step chunk (type 3), converted through the tempo chunk.
/// Rounded up. Pure — host-testable.
pub fn chart_length_secs(blob: &[u8]) -> Option<u32> {
    let tempo = TempoConverter::from_ssq(blob)?;
    let tps = tempo.tps();
    if tps <= 0 {
        return None;
    }

    // Walk every chunk; for each step chart take its LAST time offset
    // (offsets are stored ascending — docs/ssq_format.md §5.2).
    let mut max_tick: Option<i32> = None;
    let mut offset = 0usize;
    while offset + ssq_chunk::CHUNK_HEADER_SIZE <= blob.len() {
        let length = u32::from_le_bytes([
            blob[offset],
            blob[offset + 1],
            blob[offset + 2],
            blob[offset + 3],
        ]) as usize;
        if length == 0 || length < ssq_chunk::CHUNK_HEADER_SIZE || offset + length > blob.len() {
            break;
        }
        let kind = u16::from_le_bytes([blob[offset + 4], blob[offset + 5]]);
        let param2 = u16::from_le_bytes([blob[offset + 6], blob[offset + 7]]);
        if param2 == 0xFFFF {
            break;
        }
        if kind == 3 {
            let n = u16::from_le_bytes([blob[offset + 8], blob[offset + 9]]) as usize;
            let body = &blob[offset + ssq_chunk::CHUNK_HEADER_SIZE..offset + length];
            if n > 0 && body.len() >= n * 4 {
                let last = i32::from_le_bytes([
                    body[(n - 1) * 4],
                    body[(n - 1) * 4 + 1],
                    body[(n - 1) * 4 + 2],
                    body[(n - 1) * 4 + 3],
                ]);
                max_tick = Some(max_tick.map_or(last, |m: i32| m.max(last)));
            }
        }
        offset += length;
    }

    let mc = tempo.beat_to_music_count(max_tick?);
    if mc <= 0 {
        return None;
    }
    // seconds-ticks → seconds, rounded up.
    Some(((mc as u32) + (tps as u32) - 1) / (tps as u32))
}
