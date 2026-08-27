//! Beat → musicCount converter driven by an SSQ tempo chunk.
//!
//! `docs/ssq_format.md` §3 describes the tempo chunk layout:
//!
//! * `param2` = ticks per second (TPS) — per-file (150 or 1000 in DDR World).
//! * `param3` = entry count N.
//! * Body: N × i32 `time_offset[]` (measure-tick positions) followed by
//!   N × i32 `tempo_data[]` (seconds-ticks, `elapsed_seconds × TPS`).
//!
//! At chunk-load time the engine pre-computes a per-entry normalized
//! millisecond value (`docs/ssq_format.md §3.4`). We don't need that for
//! mine injection — we only need to convert a mine's `beat_count` to a
//! `music_count` that matches what the engine assigns regular notes at
//! the same beat.
//!
//! `musicCount` is elapsed time expressed in seconds-ticks relative to the
//! first tempo entry's timestamp. Between consecutive tempo entries the
//! relationship is linear, so the conversion is piecewise linear across
//! the `(time_offset[], tempo_data[])` pairs. Integer math throughout to
//! match the game's computation bit-for-bit.

use crate::core::ssq::ssq_chunk;

pub struct TempoConverter {
    /// Monotonically non-decreasing measure-tick positions. Pairs with `tempo_data`.
    time_offsets: Vec<i32>,
    /// Elapsed seconds-ticks at each time offset (`seconds × tps`).
    tempo_data: Vec<i32>,
    tps: i32,
}

impl TempoConverter {
    /// Parse the tempo chunk (SSQ chunk type 1, param2 = TPS).
    /// Returns `None` if no tempo chunk exists or the chunk is malformed.
    pub fn from_ssq(blob: &[u8]) -> Option<Self> {
        // We don't know TPS up front, so iterate chunks manually looking for
        // any kind=1 chunk. ssq_chunk::find_chunk requires a specific
        // (kind, param2) — instead we walk and match on kind alone.
        let chunk = find_tempo_chunk(blob)?;
        let n = chunk.param3 as usize;
        let tps = chunk.param2 as i32;
        if n == 0 || tps <= 0 {
            return None;
        }
        let need_bytes = n * 4 * 2;
        if chunk.body.len() < need_bytes {
            return None;
        }

        let mut time_offsets = Vec::with_capacity(n);
        let mut tempo_data = Vec::with_capacity(n);
        for i in 0..n {
            time_offsets.push(read_i32_le(chunk.body, i * 4));
        }
        for i in 0..n {
            tempo_data.push(read_i32_le(chunk.body, (n + i) * 4));
        }

        Some(Self {
            time_offsets,
            tempo_data,
            tps,
        })
    }

    /// Convert a measure-tick position to a music_count value using linear
    /// interpolation across the tempo chunk's entries.
    ///
    /// The result is in the same units as `tempo_data[]` (seconds-ticks at
    /// `tps`), matching what the engine stores for `musicCount` on each
    /// regular note at the same `beat_count` — the field name confirmed
    /// by the game's own debug format string
    /// `"shock ng : pressedDir=%d, musicCount=%d, note.musicCount=%d, diff=%d"`
    /// embedded as a read-only string in gamemdx.dll.
    ///
    /// For `beat_count` before the first entry, extrapolates using the first
    /// pair's slope. After the last entry, extrapolates using the last pair's
    /// slope. Degenerate intervals (delta_measure == 0 — a stop) are treated
    /// as zero-width: the music_count jumps by `tempo_data[i] - tempo_data[i-1]`
    /// as soon as `beat_count` reaches the stop position.
    pub fn beat_to_music_count(&self, beat_count: i32) -> i32 {
        let n = self.time_offsets.len();
        debug_assert!(
            n >= 2,
            "tempo chunk must have at least 2 entries for interpolation"
        );
        if n == 0 {
            return 0;
        }
        if n == 1 {
            return self.tempo_data[0];
        }

        // Before first entry: linear extrapolation using (0, 1) pair.
        if beat_count <= self.time_offsets[0] {
            return interpolate(
                self.time_offsets[0],
                self.tempo_data[0],
                self.time_offsets[1],
                self.tempo_data[1],
                beat_count,
            );
        }

        // Find the bracketing pair (i-1, i) such that
        // time_offsets[i-1] <= beat_count <= time_offsets[i].
        for i in 1..n {
            if beat_count <= self.time_offsets[i] {
                return interpolate(
                    self.time_offsets[i - 1],
                    self.tempo_data[i - 1],
                    self.time_offsets[i],
                    self.tempo_data[i],
                    beat_count,
                );
            }
        }

        // After last entry: extrapolate using last pair.
        interpolate(
            self.time_offsets[n - 2],
            self.tempo_data[n - 2],
            self.time_offsets[n - 1],
            self.tempo_data[n - 1],
            beat_count,
        )
    }

    pub fn tps(&self) -> i32 {
        self.tps
    }
    pub fn entry_count(&self) -> usize {
        self.time_offsets.len()
    }
}

/// Integer linear interpolation. Given two points `(x1, y1)` and `(x2, y2)`,
/// returns `y` for input `x`. Extrapolates cleanly outside the bracket.
///
/// Degenerate case `x1 == x2` (a stop in the tempo chunk — same tick, different
/// time): returns `y2` once `x >= x1`, `y1` otherwise.
///
/// Uses `i64` intermediate to avoid overflow: `music_count` can reach into
/// the tens of millions (a 4-minute chart at TPS=1000 = 240_000 seconds-ticks;
/// times a large delta_measure = comfortably within i64).
fn interpolate(x1: i32, y1: i32, x2: i32, y2: i32, x: i32) -> i32 {
    if x1 == x2 {
        return if x >= x1 { y2 } else { y1 };
    }
    let dx = (x2 as i64) - (x1 as i64);
    let dy = (y2 as i64) - (y1 as i64);
    let offset = (x as i64) - (x1 as i64);
    let result = (y1 as i64) + (dy * offset) / dx;
    result as i32
}

/// Walk the SSQ blob for the first chunk of kind=1 (tempo). Doesn't take a
/// specific param2 since TPS is per-file and we want whichever the file uses.
fn find_tempo_chunk(blob: &[u8]) -> Option<ssq_chunk::SsqChunk<'_>> {
    let mut offset = 0usize;
    while offset + ssq_chunk::CHUNK_HEADER_SIZE <= blob.len() {
        let length = u32::from_le_bytes([
            blob[offset],
            blob[offset + 1],
            blob[offset + 2],
            blob[offset + 3],
        ]) as usize;
        if length == 0 {
            return None;
        }
        if length < ssq_chunk::CHUNK_HEADER_SIZE || offset + length > blob.len() {
            return None;
        }
        let kind = u16::from_le_bytes([blob[offset + 4], blob[offset + 5]]);
        let param2 = u16::from_le_bytes([blob[offset + 6], blob[offset + 7]]);
        if param2 == 0xFFFF {
            return None;
        }
        if kind == 1 {
            return Some(ssq_chunk::SsqChunk {
                kind,
                param2,
                param3: u16::from_le_bytes([blob[offset + 8], blob[offset + 9]]),
                param4: u16::from_le_bytes([blob[offset + 10], blob[offset + 11]]),
                body: &blob[offset + ssq_chunk::CHUNK_HEADER_SIZE..offset + length],
            });
        }
        offset += length;
    }
    None
}

#[inline]
fn read_i32_le(buf: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}
