//! Assist-tick alignment (design §10): re-lay the committed tick track so its
//! byte 0 is the tick voice's TRUE DAC onset, then swap the bytes ahead of
//! the decoder's read pointer.
//!
//! The stock-shaped defect: the tick track is one cue started by the game
//! thread; its voice `Start` is a posted command, so the track's sample 0
//! lands at the first frame of whichever 10 ms pass drains it — a uniform
//! ±5 ms play-to-play spread against the song (plus the commit's ±1.45 ms
//! block quantization). With the audio clock both onsets are exact:
//!
//! ```text
//! E_tick0 = (F0_tick − F0_song)/Hz·1000 + C + content_offset     (song clock reading at the tick's sample 0)
//! skip*   = (E_tick0 − S + J) − wall(m0) − C                    (the exact shift the commit SHOULD have used)
//! pos'(t) = wall(t + J − m0) − S − skip*                        (each clap re-laid relative to the tick's sample 0)
//! ```
//!
//! Derivation of `skip*`: the committed track is authored so that note `t`
//! sits at position `p(t) = wall(t + J − m0) − S` and is served from
//! `skip = wall(mc_c − m0)` (`mc_c` = the count at commit). In the stock model
//! the tick's sample 0 is heard `S` after the Play call, which makes the clap
//! for `t` land together with the song's content `t`. Under the DAC clock the
//! tick's sample 0 is heard at song content `D = E_tick0 − C`; requiring the
//! same coincidence gives `skip* = p(t) − (wall(t) − D) = (E_tick0 − S + J)
//! − wall(m0) − C`. (With `S == C` — a perfectly calibrated SOUND_OFFSET — and
//! no pass jitter this is exactly the stock `skip`; design §10's `mc_tick0`
//! line omitted the `−C` term.) The correction `skip* − served` is this tick
//! start's deviation from the mean pass phase (±5 ms) plus the commit's block
//! quantisation — the two defects §10.1 names.
//!
//! `pos'` is mixed at SAMPLE positions (fractional ms survive), encoded, and
//! copied over the mod-owned bank from `consumed_bytes(node) + SAFETY` to the
//! end — the engine reads the client-owned in-memory bank lazily, so the swap
//! takes effect when the decoder reaches it. The ≤ ~350 ms already served
//! keep the commit's alignment (≤ 10 ms off); the cue starts during READY,
//! so no real tick is affected on a fresh song.
//!
//! Fail-open ladder: song clock not ACTIVE ⇒ nothing (the ticks stay aligned
//! to the stock clock, as shipped); aux onset never arrives ⇒ one WARN, keep
//! the committed track; any synthesis/swap failure ⇒ keep the committed
//! track. With the timing mod disabled no listener is registered and
//! `assist_tick` is byte-for-byte the shipped behaviour.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::mods::assist_tick::{self, TrackCommit, TrackEvent};
use crate::services::audio_clock::{self, engine, game, onset};
use crate::services::{game_audio, input_manager, se_bank_synth};
use crate::{log_info, log_warn};

/// Bytes ahead of the decoder's read pointer the swap never touches
/// (≈ 340 ms of mono MS-ADPCM at 44.1 kHz).
const SAFETY_BYTES: usize = 8 * 1024;
/// How long to wait for the tick voice's onset after a commit.
const ONSET_TIMEOUT: Duration = Duration::from_millis(2_000);
/// Corrections beyond this are implausible (identity mixup) — refuse.
const MAX_CORRECTION_MS: f64 = 60.0;

enum Stage {
    Idle,
    /// Committed; waiting for the aux voice's first produce.
    ///
    /// `generation_floor` = `engine::aux_voice_generation()` read right after
    /// the commit. It is only a LOWER bound: the tick voice's identity lands
    /// in the engine's in-memory submission, which `SoundBank::Play` may run
    /// asynchronously a few ms after it returns (the notify pump) — the first
    /// live run read 0 at commit and skipped a song whose voice was
    /// identified moments later. So nothing is decided at commit; the onset
    /// is recognised by `generation ≥ floor` AND `F0_tick ≥ F0_song` (the
    /// tick always starts after the song it belongs to, so a previous
    /// segment's stale onset — smaller F0 — can never match).
    AwaitOnset {
        commit: TrackCommit,
        since: Instant,
        generation_floor: u64,
    },
    /// Background re-synthesis in flight for `job`.
    Synthesizing {
        commit: TrackCommit,
        job: u32,
        correction_ms: f64,
        floor_sample: Option<i64>,
    },
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static STAGE: Mutex<Stage> = Mutex::new(Stage::Idle);
static FRAME_CALLBACK: AtomicUsize = AtomicUsize::new(0);
static JOB: AtomicU32 = AtomicU32::new(0);
/// Synthesis results: (job, encoded track).
static MAILBOX: Mutex<Option<(u32, Arc<Vec<u8>>)>> = Mutex::new(None);
static ONSET_TIMEOUT_WARNED: AtomicBool = AtomicBool::new(false);
static ALIGNMENTS: AtomicU32 = AtomicU32::new(0);

/// Enable the alignment (called by the mod's enable). `align` = the config
/// switch `gameplay_timing_fixes.assist_tick_alignment`.
pub fn enable(align: bool) {
    if !align {
        log_info!("GameplayTimingFixes: assist-tick alignment disabled by config");
        return;
    }
    if ENABLED.swap(true, Ordering::AcqRel) {
        return;
    }
    assist_tick::set_track_listener(Arc::new(on_track_event));
    if input_manager::is_available() {
        let id = input_manager::on_frame(Arc::new(poll));
        FRAME_CALLBACK.store(id, Ordering::Release);
    } else {
        log_warn!("GameplayTimingFixes: input manager unavailable -- assist-tick alignment has no frame driver");
    }
    log_info!("GameplayTimingFixes: assist-tick alignment armed (post-onset exact re-lay + in-place swap)");
}

pub fn disable() {
    if !ENABLED.swap(false, Ordering::AcqRel) {
        return;
    }
    assist_tick::clear_track_listener();
    let id = FRAME_CALLBACK.swap(0, Ordering::AcqRel);
    if id != 0 && input_manager::is_available() {
        input_manager::remove_frame_callback(id);
    }
    reset("disabled");
}

fn reset(_why: &str) {
    JOB.fetch_add(1, Ordering::AcqRel);
    if let Ok(mut stage) = STAGE.lock() {
        *stage = Stage::Idle;
    }
    if let Ok(mut mailbox) = MAILBOX.lock() {
        *mailbox = None;
    }
}

fn on_track_event(event: TrackEvent) {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    match event {
        TrackEvent::Stopped => reset("track stopped"),
        TrackEvent::Committed(commit) => {
            reset("new commit");
            if !audio_clock::is_available() {
                return;
            }
            // Never decide here (see `Stage::AwaitOnset`): the voice may not
            // be identified yet. Wait for its onset either way.
            let generation_floor = engine::aux_voice_generation();
            if let Ok(mut stage) = STAGE.lock() {
                *stage = Stage::AwaitOnset {
                    commit,
                    since: Instant::now(),
                    generation_floor,
                };
            }
        }
    }
}

/// `content_to_wall` as f64 for the committed rate (identity ⇒ x).
fn wall_f64(content_ms: f64, rate: &crate::services::song_rate::clock_patch::RateSnapshot) -> f64 {
    if !rate.is_non_identity_commit() {
        return content_ms;
    }
    let source = rate.effective_rate.source_frames as f64;
    let output = rate.effective_rate.output_frames as f64;
    if source <= 0.0 || output <= 0.0 {
        return content_ms;
    }
    content_ms * output / source
}

/// The exact shift (ms) the commit should have served from, and the
/// correction relative to what it did serve (see the module docs).
fn plan(commit: &TrackCommit, e_tick0_ms: f64, c_ms: f64) -> (f64, f64) {
    let skip_star = onset::tick_skip_star_ms(
        e_tick0_ms,
        commit.sound_offset,
        commit.judgment_timing_signed,
        wall_f64(f64::from(commit.m0), &commit.rate),
        c_ms,
    );
    let served_ms = commit.skip_bytes as f64 / se_bank_synth::adpcm::BLOCK_ALIGN as f64
        * se_bank_synth::adpcm::SAMPLES_PER_BLOCK as f64
        * 1000.0
        / f64::from(se_bank_synth::TICK_RATE_HZ);
    (skip_star, skip_star - served_ms)
}

/// Clap positions (samples from the tick's sample 0) for the re-laid track.
fn positions(commit: &TrackCommit, skip_star_ms: f64) -> Vec<i64> {
    let hz = f64::from(se_bank_synth::TICK_RATE_HZ);
    commit
        .times
        .iter()
        .map(|&t| {
            let content =
                f64::from(t) + f64::from(commit.judgment_timing_signed) - f64::from(commit.m0);
            let pos_ms =
                wall_f64(content, &commit.rate) - f64::from(commit.sound_offset) - skip_star_ms;
            (pos_ms * hz / 1000.0).round() as i64
        })
        .collect()
}

fn poll() {
    if !ENABLED.load(Ordering::Acquire) {
        return;
    }
    let _ = std::panic::catch_unwind(poll_inner);
}

fn poll_inner() {
    let Ok(mut stage) = STAGE.try_lock() else {
        return;
    };
    match &*stage {
        Stage::Idle => {}
        Stage::AwaitOnset {
            commit,
            since,
            generation_floor,
        } => {
            if since.elapsed() > ONSET_TIMEOUT {
                if !ONSET_TIMEOUT_WARNED.swap(true, Ordering::AcqRel) {
                    // Name every gate so a field log pinpoints the miss
                    // (identity never landed / no first produce / clock not
                    // active / onset not comparable) without a repro.
                    let aux_state = match engine::aux_onset() {
                        Some(o) => format!("gen {} F0 {} epoch {}", o.generation, o.f0, o.epoch),
                        None => "none".to_string(),
                    };
                    let song_state = match engine::song_onset() {
                        Some(o) => format!("gen {} F0 {} epoch {}", o.generation, o.f0, o.epoch),
                        None => "none".to_string(),
                    };
                    log_warn!(
                        "GameplayTimingFixes: tick alignment timed out {} ms after the commit -- keeping the committed track (song clock active={}, aux voice gen {} (floor at commit {}), aux onset {}, song onset {}) (warned once)",
                        ONSET_TIMEOUT.as_millis(),
                        game::session_active(),
                        engine::aux_voice_generation(),
                        generation_floor,
                        aux_state,
                        song_state
                    );
                }
                *stage = Stage::Idle;
                return;
            }
            // The song clock must be ACTIVE: only then is the game running on
            // the DAC-derived count the ticks are being aligned to — and the
            // song onset is the reference the tick onset is matched against.
            if !game::session_active() {
                return;
            }
            let (Some(song), Some((line, epoch))) = (engine::song_onset(), engine::line()) else {
                return;
            };
            // Recognise THIS commit's tick voice (see `Stage::AwaitOnset`):
            // generation at/after the commit-time floor, same frame epoch as
            // the song, and started after the song — a stale onset from the
            // previous segment fails the F0 test.
            let Some(aux) = engine::aux_onset().filter(|o| {
                o.generation >= *generation_floor && o.epoch == song.epoch && o.f0 >= song.f0
            }) else {
                return;
            };
            if epoch != aux.epoch || !line.ready || song.hz == 0 {
                return;
            }
            let Some(c_ms) = onset::latency_constant_ms(
                &line,
                song.hz,
                f64::from(audio_clock::config().latency_bias_ms),
            ) else {
                return;
            };
            let e_tick0 = (aux.f0 - song.f0) as f64 * 1000.0 / f64::from(song.hz)
                + c_ms
                + f64::from(game::current_origin_ms());
            let (skip_star, correction) = plan(commit, e_tick0, c_ms);
            if !correction.is_finite() || correction.abs() > MAX_CORRECTION_MS {
                log_warn!(
                    "GameplayTimingFixes: tick alignment refused -- implausible correction {:+.2} ms (F0_tick {} F0_song {} skip* {:.2} ms served {} ms)",
                    correction,
                    aux.f0,
                    song.f0,
                    skip_star,
                    commit.skip_ms
                );
                *stage = Stage::Idle;
                return;
            }
            let commit = commit.clone();
            let job = JOB.fetch_add(1, Ordering::AcqRel) + 1;
            let floor_sample = commit.reset_floor_ms.map(|floor| {
                let content = f64::from(floor) + f64::from(commit.judgment_timing_signed)
                    - f64::from(commit.m0);
                let pos_ms =
                    wall_f64(content, &commit.rate) - f64::from(commit.sound_offset) - skip_star;
                (pos_ms * f64::from(se_bank_synth::TICK_RATE_HZ) / 1000.0).round() as i64
            });
            let positions = positions(&commit, skip_star);
            let clap = commit.clap.clone();
            let volume = commit.volume_percent;
            log_info!(
                "GameplayTimingFixes: tick onset gen {} F0_tick={} F0_song={} (+{} frames = {} passes) E_tick0={:.2} ms skip*={:.2} ms served={} ms -> correction {:+.2} ms; re-laying {} claps",
                aux.generation,
                aux.f0,
                song.f0,
                aux.f0 - song.f0,
                (aux.f0 - song.f0) as f64 / line.pass_frames.max(1.0),
                e_tick0,
                skip_star,
                commit.skip_ms,
                correction,
                positions.len()
            );
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(move || {
                    let synth = if volume == 100 {
                        se_bank_synth::synthesize_track_at_samples(&clap, &positions)
                    } else {
                        let scaled = se_bank_synth::scale_pcm(&clap, volume);
                        se_bank_synth::synthesize_track_at_samples(&scaled, &positions)
                    };
                    synth.encoded
                });
                if let Ok(encoded) = result {
                    if let Ok(mut mailbox) = MAILBOX.lock() {
                        *mailbox = Some((job, Arc::new(encoded)));
                    }
                }
            });
            *stage = Stage::Synthesizing {
                commit,
                job,
                correction_ms: correction,
                floor_sample,
            };
        }
        Stage::Synthesizing {
            commit,
            job,
            correction_ms,
            floor_sample,
        } => {
            let ready = MAILBOX.try_lock().ok().and_then(|mut m| match &*m {
                Some((j, _)) if *j == *job => m.take().map(|(_, e)| e),
                _ => None,
            });
            let Some(encoded) = ready else {
                return;
            };
            if JOB.load(Ordering::Acquire) != *job {
                *stage = Stage::Idle;
                return;
            }
            let Some(node) = engine::aux_onset_node() else {
                *stage = Stage::Idle;
                return;
            };
            let Some(consumed) = engine::consumed_bytes(node) else {
                *stage = Stage::Idle;
                return;
            };
            let from = se_bank_synth::ceil_block_bytes(
                usize::try_from(consumed)
                    .unwrap_or(usize::MAX)
                    .saturating_add(SAFETY_BYTES),
            );
            let mute_head = floor_sample.map_or(0, se_bank_synth::block_offset_for_sample);
            if encoded.len() != commit.handle.sample_segment_len() {
                *stage = Stage::Idle;
                return;
            }
            let ok = game_audio::patch_tick_wave_tail(&commit.handle, &encoded, from, mute_head);
            if ok {
                ALIGNMENTS.fetch_add(1, Ordering::Relaxed);
                log_info!(
                    "GameplayTimingFixes: tick track aligned -- correction {:+.2} ms applied from byte {} ({:.0} ms into the track; {} bytes consumed) mute_head {} bytes",
                    correction_ms,
                    from,
                    from as f64 / se_bank_synth::adpcm::BLOCK_ALIGN as f64
                        * se_bank_synth::adpcm::SAMPLES_PER_BLOCK as f64
                        * 1000.0
                        / f64::from(se_bank_synth::TICK_RATE_HZ),
                    consumed,
                    mute_head
                );
            } else {
                log_warn!("GameplayTimingFixes: tick track swap refused by game_audio -- committed track kept");
            }
            *stage = Stage::Idle;
        }
    }
}

/// Alignments performed this session (status line).
#[must_use]
pub fn alignments() -> u32 {
    ALIGNMENTS.load(Ordering::Relaxed)
}
