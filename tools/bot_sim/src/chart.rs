//! SSQ → simulation notes. Pure parsing over the game's chart format
//! (`docs/ssq_format.md`) using the DLL's own chunk walker and tempo converter.
//!
//! Only SINGLE charts are produced (the bot engages in SINGLE only — design
//! R2). Note representation mirrors what the game's parser hands the judge:
//!
//! * a normal step byte → one kind-0 note, `state[p] = 1` (TRG) per set bit;
//! * a freeze head → the same note with `state[p] = 4` (REP) on the held panels
//!   and `length[p]` = duration in ticks (other panels struck by the same step
//!   get `length = 1`, as the game's `emit_freeze` promotes them);
//! * a freeze end → one kind-2 "tail" note at the end tick carrying the head's
//!   states (never tap-judged; the freeze judge resolves it);
//! * `0xFF` / `0x0F` → a shock note, all four pad panels `state = 1`.
//!
//! Music counts are milliseconds: the game normalises the tempo chunk to ms
//! (`round(seconds_ticks × 1000 / TPS)`) — reproduced here to the rounding.

use crate::core::ssq::ssq_chunk::CHUNK_HEADER_SIZE;
use crate::core::ssq::timing::TempoConverter;

/// Single-style chart difficulty; the discriminant is the game's difficulty
/// index (0..=4), the `slot()` is the SSQ chunk's high byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Difficulty {
    Beginner = 0,
    Basic = 1,
    Difficult = 2,
    Expert = 3,
    Challenge = 4,
}

impl Difficulty {
    pub const ALL: [Difficulty; 5] = [
        Difficulty::Beginner,
        Difficulty::Basic,
        Difficulty::Difficult,
        Difficulty::Expert,
        Difficulty::Challenge,
    ];

    /// SSQ chunk `param2` high byte for this difficulty.
    pub fn slot(self) -> u8 {
        match self {
            Difficulty::Beginner => 0x04,
            Difficulty::Basic => 0x01,
            Difficulty::Difficult => 0x02,
            Difficulty::Expert => 0x03,
            Difficulty::Challenge => 0x06,
        }
    }

    pub fn from_slot(slot: u8) -> Option<Difficulty> {
        Difficulty::ALL.into_iter().find(|d| d.slot() == slot)
    }

    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Beginner => "BEGINNER",
            Difficulty::Basic => "BASIC",
            Difficulty::Difficult => "DIFFICULT",
            Difficulty::Expert => "EXPERT",
            Difficulty::Challenge => "CHALLENGE",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Difficulty::Beginner => "b",
            Difficulty::Basic => "B",
            Difficulty::Difficult => "D",
            Difficulty::Expert => "E",
            Difficulty::Challenge => "C",
        }
    }
}

/// SSQ style byte for SINGLE charts.
pub const STYLE_SINGLE: u8 = 0x14;
/// Note kinds (the `GameNote+0x00` byte).
pub const KIND_TAP: i8 = 0;
pub const KIND_FREEZE_TAIL: i8 = 2;
/// Panel states.
pub const STATE_TRG: i32 = 1;
pub const STATE_REP: i32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimNote {
    pub kind: i8,
    /// Measure-tick position (4096 per measure).
    pub beat: i32,
    /// Music count in milliseconds.
    pub mc: i32,
    pub state: [i32; 8],
    /// Freeze length per panel in ticks (head only).
    pub length: [i32; 8],
    /// For a tail: index of its head note.
    pub head: Option<usize>,
}

impl SimNote {
    pub fn is_shock(&self) -> bool {
        self.state[..4].iter().all(|&s| s == STATE_TRG)
            || self.state[4..].iter().all(|&s| s == STATE_TRG)
    }
    pub fn arrow_panels(&self) -> impl Iterator<Item = usize> + '_ {
        (0..8).filter(move |&p| self.state[p] == STATE_TRG || self.state[p] == STATE_REP)
    }
    pub fn is_freeze_head(&self) -> bool {
        self.kind == KIND_TAP && self.length.iter().any(|&l| l > 0)
    }
}

/// Piecewise-linear tick ⇄ ms map built from the tempo chunk.
#[derive(Debug, Clone)]
pub struct TempoMap {
    /// `(tick, ms)` in file order (non-decreasing ticks; stops repeat a tick).
    pts: Vec<(i64, i64)>,
}

impl TempoMap {
    pub fn from_converter(t: &TempoConverter) -> Option<TempoMap> {
        let tps = t.tps() as i64;
        if tps <= 0 {
            return None;
        }
        let pts: Vec<(i64, i64)> = t
            .entries()
            .map(|(tick, st)| (tick as i64, div_round(st as i64 * 1000, tps)))
            .collect();
        if pts.len() < 2 {
            return None;
        }
        Some(TempoMap { pts })
    }

    /// Milliseconds at a tick position (the game's per-note `musicCount`).
    pub fn ms_at_tick(&self, tick: i32) -> i32 {
        let x = tick as i64;
        let p = &self.pts;
        if x <= p[0].0 {
            return interp(p[0], p[1], x) as i32;
        }
        for i in 1..p.len() {
            if x <= p[i].0 {
                return interp(p[i - 1], p[i], x) as i32;
            }
        }
        interp(p[p.len() - 2], p[p.len() - 1], x) as i32
    }

    /// Tick position at a music count (the actor's `cur_beat`, +0x168).
    pub fn tick_at_ms(&self, ms: i32) -> i32 {
        let y = ms as i64;
        let p = &self.pts;
        let sw = |(a, b): (i64, i64)| (b, a);
        if y <= p[0].1 {
            return interp(sw(p[0]), sw(p[1]), y) as i32;
        }
        for i in 1..p.len() {
            if y <= p[i].1 {
                return interp(sw(p[i - 1]), sw(p[i]), y) as i32;
            }
        }
        interp(sw(p[p.len() - 2]), sw(p[p.len() - 1]), y) as i32
    }
}

fn div_round(a: i64, b: i64) -> i64 {
    (a + b / 2).div_euclid(b)
}

/// Linear interpolation in i64; a zero-width bracket (a stop) returns the
/// later value once `x` reaches it.
fn interp((x1, y1): (i64, i64), (x2, y2): (i64, i64), x: i64) -> i64 {
    if x1 == x2 {
        return if x >= x1 { y2 } else { y1 };
    }
    y1 + (y2 - y1) * (x - x1) / (x2 - x1)
}

#[derive(Debug, Clone)]
pub struct Chart {
    /// File stem (e.g. `aaaa`, `sabm_5`).
    pub name: String,
    pub difficulty: Difficulty,
    pub tempo: TempoMap,
    /// Notes in chart order (taps/shocks and freeze tails interleaved by
    /// beat; tails after taps at the same beat).
    pub notes: Vec<SimNote>,
    /// Counts the game keeps: taps (incl. freeze heads and jumps as ONE),
    /// freezes (tails), shocks.
    pub taps: u32,
    pub freezes: u32,
    pub shocks: u32,
}

impl Chart {
    /// A stable 32-bit identity for seeding (charts have no mcode here).
    pub fn mcode_hash(&self) -> i32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in self.name.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h as i32
    }

    pub fn duration_ms(&self) -> i32 {
        self.notes.last().map(|n| n.mc).unwrap_or(0) - self.notes.first().map(|n| n.mc).unwrap_or(0)
    }
}

/// Parse every SINGLE step chunk of an SSQ blob.
pub fn parse_single_charts(name: &str, blob: &[u8]) -> Result<Vec<Chart>, String> {
    let conv = TempoConverter::from_ssq(blob).ok_or("no tempo chunk")?;
    let tempo = TempoMap::from_converter(&conv).ok_or("tempo chunk too short")?;

    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset + CHUNK_HEADER_SIZE <= blob.len() {
        let length = u32::from_le_bytes([
            blob[offset],
            blob[offset + 1],
            blob[offset + 2],
            blob[offset + 3],
        ]) as usize;
        if length == 0 {
            break;
        }
        if length < CHUNK_HEADER_SIZE || offset + length > blob.len() {
            return Err(format!("malformed chunk at 0x{offset:X}"));
        }
        let kind = u16::from_le_bytes([blob[offset + 4], blob[offset + 5]]);
        let param2 = u16::from_le_bytes([blob[offset + 6], blob[offset + 7]]);
        if param2 == 0xFFFF {
            break;
        }
        if kind == 3 && (param2 & 0xFF) as u8 == STYLE_SINGLE {
            if let Some(diff) = Difficulty::from_slot((param2 >> 8) as u8) {
                let n = u16::from_le_bytes([blob[offset + 8], blob[offset + 9]]) as usize;
                let body = &blob[offset + CHUNK_HEADER_SIZE..offset + length];
                let notes = parse_step_body(body, n, &tempo)
                    .map_err(|e| format!("{} {}: {e}", name, diff.name()))?;
                let taps = notes
                    .iter()
                    .filter(|n| n.kind == KIND_TAP && !n.is_shock())
                    .count() as u32;
                let shocks = notes
                    .iter()
                    .filter(|n| n.kind == KIND_TAP && n.is_shock())
                    .count() as u32;
                let freezes = notes.iter().filter(|n| n.kind == KIND_FREEZE_TAIL).count() as u32;
                out.push(Chart {
                    name: name.to_string(),
                    difficulty: diff,
                    tempo: tempo.clone(),
                    notes,
                    taps,
                    freezes,
                    shocks,
                });
            }
        }
        offset += length;
    }
    Ok(out)
}

/// Decode one step-chunk body (`docs/ssq_format.md` §5.2–§5.4) into notes.
pub fn parse_step_body(body: &[u8], n: usize, tempo: &TempoMap) -> Result<Vec<SimNote>, String> {
    if body.len() < n * 5 {
        return Err("step body too short".into());
    }
    let ticks: Vec<i32> = (0..n)
        .map(|i| {
            i32::from_le_bytes([
                body[i * 4],
                body[i * 4 + 1],
                body[i * 4 + 2],
                body[i * 4 + 3],
            ])
        })
        .collect();
    let steps = &body[n * 4..n * 5];
    let freeze_block = &body[n * 4 + ((n + 1) & !1)..];

    let mut notes: Vec<SimNote> = Vec::with_capacity(n);
    let mut freeze_index = 0usize;
    for i in 0..n {
        let tick = ticks[i];
        let step = steps[i];
        match step {
            0x00 => {
                // Freeze end marker — consume one (panels, kind) pair.
                let pair = freeze_block.get(freeze_index * 2..freeze_index * 2 + 2);
                freeze_index += 1;
                let Some(&[panels, kind]) = pair else {
                    continue;
                };
                if kind != 0x01 {
                    continue;
                }
                // Resolve each panel bit to its most recent earlier note that
                // struck that panel; set its length and promote it to REP.
                let mut head_states = [0i32; 8];
                let mut head_idx: Option<usize> = None;
                for p in 0..8 {
                    if panels & (1 << p) == 0 {
                        continue;
                    }
                    if let Some(hi) = notes.iter().rposition(|nn| {
                        nn.kind == KIND_TAP && nn.state[p] != 0 && nn.length[p] == 0
                    }) {
                        let head = &mut notes[hi];
                        let dur = (tick - head.beat).max(1);
                        head.state[p] = STATE_REP;
                        head.length[p] = dur;
                        // Other struck panels of the same step become part of
                        // the freeze (duration 1) — emit_freeze's promotion.
                        for q in 0..8 {
                            if q != p && head.state[q] != 0 && head.length[q] == 0 {
                                head.length[q] = 1;
                            }
                        }
                        head_states[p] = STATE_REP;
                        head_idx = Some(hi);
                    }
                }
                if let Some(hi) = head_idx {
                    notes.push(SimNote {
                        kind: KIND_FREEZE_TAIL,
                        beat: tick,
                        mc: tempo.ms_at_tick(tick),
                        state: head_states,
                        length: [0; 8],
                        head: Some(hi),
                    });
                }
            }
            0xFF | 0x0F | 0xF0 => {
                let mut state = [0i32; 8];
                let lo = step & 0x0F != 0;
                let hi = step & 0xF0 != 0;
                for p in 0..4 {
                    if lo {
                        state[p] = STATE_TRG;
                    }
                    if hi {
                        state[p + 4] = STATE_TRG;
                    }
                }
                notes.push(SimNote {
                    kind: KIND_TAP,
                    beat: tick,
                    mc: tempo.ms_at_tick(tick),
                    state,
                    length: [0; 8],
                    head: None,
                });
            }
            bits => {
                let mut state = [0i32; 8];
                for p in 0..8 {
                    if bits & (1 << p) != 0 {
                        state[p] = STATE_TRG;
                    }
                }
                notes.push(SimNote {
                    kind: KIND_TAP,
                    beat: tick,
                    mc: tempo.ms_at_tick(tick),
                    state,
                    length: [0; 8],
                    head: None,
                });
            }
        }
    }
    // Tails were pushed at their marker position, which is already in beat
    // order relative to taps (offsets ascend); keep taps before tails at an
    // equal beat, as the game's sort does.
    notes.sort_by_key(|nn| (nn.beat, nn.kind));
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an SSQ blob: tempo chunk (TPS, entries) + one SINGLE step chunk.
    fn ssq(
        tps: u16,
        tempo: &[(i32, i32)],
        slot: u8,
        ticks: &[i32],
        steps: &[u8],
        freezes: &[(u8, u8)],
    ) -> Vec<u8> {
        let mut b = Vec::new();
        // tempo chunk
        let n = tempo.len();
        let len = 12 + n * 8;
        b.extend((len as u32).to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(tps.to_le_bytes());
        b.extend((n as u16).to_le_bytes());
        b.extend(0u16.to_le_bytes());
        for &(t, _) in tempo {
            b.extend(t.to_le_bytes());
        }
        for &(_, s) in tempo {
            b.extend(s.to_le_bytes());
        }
        // step chunk
        let n = ticks.len();
        let mut body = Vec::new();
        for &t in ticks {
            body.extend(t.to_le_bytes());
        }
        body.extend(steps);
        if n % 2 == 1 {
            body.push(0);
        }
        for &(p, k) in freezes {
            body.push(p);
            body.push(k);
        }
        while (12 + body.len()) % 4 != 0 {
            body.push(0);
        }
        b.extend(((12 + body.len()) as u32).to_le_bytes());
        b.extend(3u16.to_le_bytes());
        b.extend((((slot as u16) << 8) | STYLE_SINGLE as u16).to_le_bytes());
        b.extend((n as u16).to_le_bytes());
        b.extend(0u16.to_le_bytes());
        b.extend(body);
        b.extend(0u32.to_le_bytes()); // terminator
        b
    }

    // 120 BPM at TPS 1000: one measure (4096 ticks) = 2000 ms.
    const T120: &[(i32, i32)] = &[(0, 0), (4096, 2000)];

    #[test]
    fn tempo_map_round_trips_and_normalises_tps() {
        let blob = ssq(150, &[(0, 0), (4096, 300)], 0x03, &[0], &[0x01], &[]);
        let charts = parse_single_charts("t", &blob).unwrap();
        let c = &charts[0];
        // 300 seconds-ticks at 150 TPS = 2000 ms per measure.
        assert_eq!(c.tempo.ms_at_tick(4096), 2000);
        assert_eq!(c.tempo.ms_at_tick(2048), 1000);
        assert_eq!(c.tempo.tick_at_ms(1000), 2048);
        assert_eq!(
            c.tempo.tick_at_ms(4000),
            8192,
            "extrapolates past the last entry"
        );
    }

    #[test]
    fn taps_jumps_and_difficulty() {
        let blob = ssq(1000, T120, 0x03, &[0, 1024, 2048], &[0x01, 0x0A, 0x04], &[]);
        let charts = parse_single_charts("t", &blob).unwrap();
        assert_eq!(charts.len(), 1);
        let c = &charts[0];
        assert_eq!(c.difficulty, Difficulty::Expert);
        assert_eq!(c.notes.len(), 3);
        assert_eq!(c.notes[0].state, [1, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(c.notes[1].state, [0, 1, 0, 1, 0, 0, 0, 0]);
        assert_eq!(c.notes[1].mc, 500);
        assert_eq!((c.taps, c.freezes, c.shocks), (3, 0, 0));
    }

    #[test]
    fn freeze_head_gets_rep_and_length_tail_is_kind_2() {
        // head at 0 on panel 2 (0x04) + panel 0 struck too (0x05); end at 2048.
        let blob = ssq(
            1000,
            T120,
            0x01,
            &[0, 1024, 2048],
            &[0x05, 0x02, 0x00],
            &[(0x04, 0x01)],
        );
        let c = &parse_single_charts("t", &blob).unwrap()[0];
        assert_eq!(c.difficulty, Difficulty::Basic);
        assert_eq!(c.notes.len(), 3);
        let head = &c.notes[0];
        assert_eq!(head.state[2], STATE_REP);
        assert_eq!(head.length[2], 2048);
        assert_eq!(head.state[0], STATE_TRG);
        assert_eq!(head.length[0], 1, "co-struck panel promoted to length 1");
        assert!(head.is_freeze_head());
        let tail = &c.notes[2];
        assert_eq!(tail.kind, KIND_FREEZE_TAIL);
        assert_eq!(tail.beat, 2048);
        assert_eq!(tail.mc, 1000);
        assert_eq!(tail.head, Some(0));
        assert_eq!(tail.state[2], STATE_REP);
        assert_eq!((c.taps, c.freezes, c.shocks), (2, 1, 0));
    }

    #[test]
    fn freeze_marker_with_kind_zero_is_ignored() {
        let blob = ssq(1000, T120, 0x01, &[0, 2048], &[0x04, 0x00], &[(0x04, 0x00)]);
        let c = &parse_single_charts("t", &blob).unwrap()[0];
        assert_eq!(c.notes.len(), 1);
        assert!(!c.notes[0].is_freeze_head());
    }

    #[test]
    fn shocks() {
        let blob = ssq(1000, T120, 0x06, &[0, 1024], &[0xFF, 0x0F], &[]);
        let c = &parse_single_charts("t", &blob).unwrap()[0];
        assert_eq!(c.difficulty, Difficulty::Challenge);
        assert!(c.notes[0].is_shock());
        assert!(c.notes[1].is_shock());
        assert_eq!(c.notes[1].state[..4], [1, 1, 1, 1]);
        assert_eq!((c.taps, c.freezes, c.shocks), (0, 0, 2));
    }

    #[test]
    fn double_chunks_are_skipped() {
        let mut blob = ssq(1000, T120, 0x03, &[0], &[0x01], &[]);
        // Rewrite the style byte to DOUBLE.
        let idx = blob.len() - 4 - (12 + 4 + 1 + 1 + 2) + 6; // param2 low byte of the step chunk
        blob[idx] = 0x18;
        let charts = parse_single_charts("t", &blob).unwrap();
        assert!(charts.is_empty());
    }

    #[test]
    fn beginner_slot_maps_to_index_zero() {
        assert_eq!(Difficulty::from_slot(0x04), Some(Difficulty::Beginner));
        assert_eq!(Difficulty::Beginner as u8, 0);
        assert_eq!(Difficulty::Challenge as u8, 4);
        assert_eq!(
            Difficulty::from_slot(0x05),
            None,
            "slot 5 is not a World difficulty"
        );
    }
}
