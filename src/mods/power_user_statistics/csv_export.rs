//! CSV Export — writes per-player step data to CSV on song end.

use std::io::Write;
use std::path::Path;

use crate::services::custom_options;
use crate::{log_info, log_warn};

use super::data_feed;

const EXPORT_DIR: &str = "./step_data_exports";

/// Flush per-step data to CSV for any player whose option was ON.
/// Called on scene 28 → non-28 transitions.
pub fn flush() {
    let bufs = data_feed::buffers();
    for side in 0..2u8 {
        let Ok(mut b) = bufs[side as usize].lock() else {
            continue;
        };

        let steps = match b.per_step.take() {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };

        let identity = match b.song_identity.take() {
            Some(id) => id,
            None => continue,
        };

        let option_on = custom_options::get_value(side, "step_data_export").unwrap_or(0) != 0;
        if !option_on {
            continue;
        }

        drop(b);

        if let Err(e) = write_csv(&identity, side, &steps) {
            log_warn!("csv_export: failed for P{}: {}", side + 1, e);
        }
    }
}

/// Snapshot song identity from the actor on first judgment of a song.
/// Must be called from the judge_submit detour (game thread, actor valid).
pub unsafe fn snapshot_song_identity(actor: *mut u8, player_side: usize) {
    if actor.is_null() {
        return;
    }
    let bufs = data_feed::buffers();
    let Ok(mut b) = bufs[player_side].try_lock() else {
        return;
    };
    if b.song_identity.is_some() {
        return;
    }

    // The actor's parent (at +0x08) is the DancePlaySequence.
    let dps = *(actor.add(0x08) as *const *const u8);
    if dps.is_null() {
        return;
    }

    // Basename std::string at DPS+0xA0 (standard MSVC layout).
    let string_base = dps.add(0xA0);
    let basename = read_msvc_string(string_base);

    // Difficulty index at DPS+0x50 (u8: 0=beg, 1=bas, 2=dif, 3=exp, 4=cha).
    let difficulty = *(dps.add(0x50) as *const u8) as i32;

    if !basename.is_empty() {
        b.song_identity = Some(data_feed::SongIdentity {
            songcode: basename,
            difficulty,
            // One lock-free seqlock read; the first judgment fires strictly
            // after any loader-thread rate commit, so a committed rate is
            // visible here and an armed-but-failed attempt reads identity.
            rate: crate::services::song_rate::clock_patch::snapshot(),
        });
    }
}

/// Read an MSVC std::string from its base address.
/// Layout: [+0x00] 16-byte inline buffer / heap ptr, [+0x10] size, [+0x18] capacity.
/// If capacity >= 16, the first 8 bytes are a pointer to heap-allocated data.
unsafe fn read_msvc_string(string_base: *const u8) -> String {
    let size = *(string_base.add(0x10) as *const usize);
    if size == 0 || size > 256 {
        return String::new();
    }
    let cap = *(string_base.add(0x18) as *const usize);
    let data_ptr = if cap >= 16 {
        *(string_base as *const *const u8)
    } else {
        string_base
    };
    if data_ptr.is_null() {
        return String::new();
    }
    let slice = std::slice::from_raw_parts(data_ptr, size.min(64));
    String::from_utf8_lossy(slice).into_owned()
}

fn difficulty_name(idx: i32) -> &'static str {
    match idx {
        0 => "beginner",
        1 => "basic",
        2 => "difficult",
        3 => "expert",
        4 => "challenge",
        _ => "unknown",
    }
}

fn write_csv(
    identity: &data_feed::SongIdentity,
    side: u8,
    steps: &[data_feed::StepRecord],
) -> std::io::Result<()> {
    std::fs::create_dir_all(EXPORT_DIR)?;

    let now = chrono_lite_timestamp();
    let filename = format!(
        "{}_{}_{}_{}_P{}.csv",
        now,
        identity.songcode,
        difficulty_name(identity.difficulty),
        identity.difficulty,
        side + 1,
    );
    let path = Path::new(EXPORT_DIR).join(&filename);

    let mut file = std::fs::File::create(&path)?;
    // Design req 34: ms-error cells stay content-domain (chart
    // milliseconds) with their labels byte-identical to the pre-rate
    // export; the two appended rate columns are what make that domain
    // interpretable. Every row carries the per-song latched requested
    // percent and the committed exact ratio (identity songs uniformly emit
    // 100 and 1/1 — see `RateSnapshot::csv_rate_cells`).
    let (requested, effective) = identity.rate.csv_rate_cells();
    file.write_all(
        b"Expected,Actual,Delta (Ms Error),Song Rate Requested (%),Song Rate Effective\r\n",
    )?;
    for step in steps {
        let _ = write!(
            file,
            "{},{},{},{},{}\r\n",
            step.expected_ms, step.actual_ms, step.delta_ms, requested, effective
        );
    }

    log_info!(
        "csv_export: wrote {} steps to {} for P{}",
        steps.len(),
        filename,
        side + 1
    );
    Ok(())
}

fn chrono_lite_timestamp() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    let days = secs / 86400;
    let (y, doy) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        y, doy.0, doy.1, h, m, s
    )
}

fn days_to_ymd(mut days: u64) -> (u64, (u64, u64)) {
    let mut y = 1970;
    loop {
        let ylen = if is_leap(y) { 366 } else { 365 };
        if days < ylen {
            break;
        }
        days -= ylen;
        y += 1;
    }
    let mdays: [u64; 12] = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut m = 1u64;
    for &ml in &mdays {
        if days < ml {
            break;
        }
        days -= ml;
        m += 1;
    }
    (y, (m, days + 1))
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
