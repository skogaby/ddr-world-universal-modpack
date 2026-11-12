//! Self-check of a synthesised SSQ before any of its slots is declared.
//!
//! Mechanism: re-parse the bytes with the DLL's own read-side walkers
//! (`ssq_chunk::find_chunk`, `timing::TempoConverter`) — the same code
//! `fast_bootup`, `chart_length` and `note_types_expansion` trust at
//! runtime — and verify the invariants whose violation the game turns
//! into a lock-up at READY or the boot-blocking `ME1529` service error
//! (design §4.2, `docs/stepmania_runtime_packs_feasibility.md` §8.6):
//!
//! - the chunk list walks to the zero-length terminator with every
//!   length ≥ 12, dword-aligned and inside the buffer;
//! - the tempo chunk starts at tick 0, is monotonic in both columns and
//!   has no segment with Δtick > 0 and Δms = 0 (infinite BPM);
//! - the event chunk carries FINISH `(2,3)` and END `(2,4)`, both
//!   bracketed by tempo anchors;
//! - every declared slot has a step chunk whose layout is consistent,
//!   whose ticks are sorted, whose last step (freeze ends included) is at
//!   or before END, and which has at least one judged row — a tap, jump
//!   or shock; rows that are only freeze heads do not count (the freeze
//!   resolution mirrors the game's per-panel backward walk,
//!   `docs/ssq_format.md` §5.4);
//! - no row carries a shock byte the writer could not have meant: on a
//!   Single chart any `0x0F`/`0xF0` or high-nibble bit (legitimate
//!   single shocks are `0xFF`), on any chart a full side nibble with
//!   extra bits (`0x1F`). A plain `0x0F`/`0xF0`/`0xFF` on a Double chart
//!   is indistinguishable from a real shock at the byte level, which is
//!   why the writer splits quads before the bytes exist.
//!
//! Pure: no logging, no `windows`; failures are returned as data.

use crate::core::ssq::ssq_chunk::{self, CHUNK_HEADER_SIZE};
use crate::core::ssq::timing::TempoConverter;
use crate::core::ssq::writer::slot_code;

/// Upper bound on chunks walked (stock files have ≤ ~25).
const MAX_CHUNKS: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckFailure {
    ChunkListNotWalkable {
        offset: usize,
    },
    TempoMissing,
    TempoFirstTickNonZero {
        tick: i32,
    },
    TempoNotMonotonic {
        index: usize,
    },
    TempoZeroDeltaMs {
        index: usize,
    },
    EventsMissing,
    EventMissing {
        code: u8,
        arg: u8,
    },
    FinishNotBracketed {
        finish_tick: i32,
        last_tempo_tick: i32,
    },
    EndNotBracketed {
        end_tick: i32,
        last_tempo_tick: i32,
    },
    SlotMissing {
        slot: u8,
    },
    StepChunkMalformed {
        slot: u8,
        reason: String,
    },
    StepsNotSorted {
        slot: u8,
        index: usize,
    },
    EndBeforeLastNote {
        slot: u8,
        end_tick: i32,
        last_tick: i32,
    },
    NoSteps {
        slot: u8,
    },
    ShockByteOnNoteRow {
        slot: u8,
        tick: i32,
        byte: u8,
    },
}

impl std::fmt::Display for CheckFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChunkListNotWalkable { offset } => {
                write!(f, "chunk list not walkable at byte {offset}")
            }
            Self::TempoMissing => f.write_str("no tempo chunk"),
            Self::TempoFirstTickNonZero { tick } => write!(f, "first tempo anchor at tick {tick}"),
            Self::TempoNotMonotonic { index } => write!(f, "tempo anchor {index} not monotonic"),
            Self::TempoZeroDeltaMs { index } => {
                write!(
                    f,
                    "tempo anchor {index}: ticks advance with zero elapsed ms"
                )
            }
            Self::EventsMissing => f.write_str("no event chunk"),
            Self::EventMissing { code, arg } => write!(f, "event ({code},{arg}) missing"),
            Self::FinishNotBracketed {
                finish_tick,
                last_tempo_tick,
            } => write!(
                f,
                "FINISH {finish_tick} past last tempo anchor {last_tempo_tick}"
            ),
            Self::EndNotBracketed {
                end_tick,
                last_tempo_tick,
            } => write!(f, "END {end_tick} past last tempo anchor {last_tempo_tick}"),
            Self::SlotMissing { slot } => write!(f, "slot {slot}: no step chunk"),
            Self::StepChunkMalformed { slot, reason } => {
                write!(f, "slot {slot}: malformed step chunk ({reason})")
            }
            Self::StepsNotSorted { slot, index } => {
                write!(f, "slot {slot}: step {index} out of order")
            }
            Self::EndBeforeLastNote {
                slot,
                end_tick,
                last_tick,
            } => write!(
                f,
                "slot {slot}: END {end_tick} before last step {last_tick}"
            ),
            Self::NoSteps { slot } => write!(f, "slot {slot}: no tap/jump/shock row"),
            Self::ShockByteOnNoteRow { slot, tick, byte } => {
                write!(
                    f,
                    "slot {slot}: step byte 0x{byte:02X} at tick {tick} reads as a shock"
                )
            }
        }
    }
}

/// Verify `bytes` for the given musicdb slots (0..9). `Ok(())` means
/// every declared slot is safe to declare.
pub fn check_ssq(bytes: &[u8], declared_slots: &[u8]) -> Result<(), Vec<CheckFailure>> {
    let mut failures: Vec<CheckFailure> = Vec::new();

    if let Err(offset) = walk_chunk_list(bytes) {
        failures.push(CheckFailure::ChunkListNotWalkable { offset });
    }

    let tempo = TempoConverter::from_ssq(bytes);
    let mut last_tempo_tick: Option<i32> = None;
    match &tempo {
        None => failures.push(CheckFailure::TempoMissing),
        Some(t) => {
            let entries: Vec<(i32, i32)> = t.entries().collect();
            if let Some(&(first, _)) = entries.first() {
                if first != 0 {
                    failures.push(CheckFailure::TempoFirstTickNonZero { tick: first });
                }
            }
            for (i, w) in entries.windows(2).enumerate() {
                let (t0, s0) = w[0];
                let (t1, s1) = w[1];
                if t1 < t0 || s1 < s0 {
                    failures.push(CheckFailure::TempoNotMonotonic { index: i + 1 });
                } else if t1 > t0 && s1 == s0 {
                    failures.push(CheckFailure::TempoZeroDeltaMs { index: i + 1 });
                }
            }
            last_tempo_tick = entries.last().map(|e| e.0);
        }
    }

    let mut end_tick: Option<i32> = None;
    match ssq_chunk::find_chunk(bytes, 2, 1) {
        None => failures.push(CheckFailure::EventsMissing),
        Some(chunk) => {
            let events = parse_events(chunk.param3, chunk.body);
            let find = |code: u8, arg: u8| {
                events
                    .iter()
                    .rev()
                    .find(|e| e.1 == code && e.2 == arg)
                    .map(|e| e.0)
            };
            let finish = find(2, 3);
            let end = find(2, 4);
            if finish.is_none() {
                failures.push(CheckFailure::EventMissing { code: 2, arg: 3 });
            }
            if end.is_none() {
                failures.push(CheckFailure::EventMissing { code: 2, arg: 4 });
            }
            if let Some(last) = last_tempo_tick {
                if let Some(ft) = finish {
                    if ft > last {
                        failures.push(CheckFailure::FinishNotBracketed {
                            finish_tick: ft,
                            last_tempo_tick: last,
                        });
                    }
                }
                if let Some(et) = end {
                    if et > last {
                        failures.push(CheckFailure::EndNotBracketed {
                            end_tick: et,
                            last_tempo_tick: last,
                        });
                    }
                }
            }
            end_tick = end;
        }
    }

    for &slot in declared_slots {
        let Some(code) = slot_code(slot) else {
            failures.push(CheckFailure::SlotMissing { slot });
            continue;
        };
        let Some(chunk) = ssq_chunk::find_chunk(bytes, 3, code) else {
            failures.push(CheckFailure::SlotMissing { slot });
            continue;
        };
        let single = slot < 5;
        match analyse_steps(chunk.param3, chunk.body) {
            Err(reason) => failures.push(CheckFailure::StepChunkMalformed { slot, reason }),
            Ok(a) => {
                if let Some(index) = a.first_unsorted {
                    failures.push(CheckFailure::StepsNotSorted { slot, index });
                }
                if let (Some(end), Some(last)) = (end_tick, a.last_tick) {
                    if last > end {
                        failures.push(CheckFailure::EndBeforeLastNote {
                            slot,
                            end_tick: end,
                            last_tick: last,
                        });
                    }
                }
                if a.judged_rows == 0 {
                    failures.push(CheckFailure::NoSteps { slot });
                }
                for &(tick, byte) in &a.rows {
                    if is_shock_leak(byte, single) {
                        failures.push(CheckFailure::ShockByteOnNoteRow { slot, tick, byte });
                    }
                }
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

/// Walk every chunk to the terminator. `Err(offset)` names the byte
/// where the walk failed.
fn walk_chunk_list(bytes: &[u8]) -> Result<(), usize> {
    let mut offset = 0usize;
    for _ in 0..MAX_CHUNKS {
        let Some(len_bytes) = bytes.get(offset..offset + 4) else {
            return Err(offset);
        };
        let length =
            u32::from_le_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
        if length == 0 {
            return Ok(());
        }
        if length < CHUNK_HEADER_SIZE || length % 4 != 0 {
            return Err(offset);
        }
        match offset.checked_add(length) {
            Some(next) if next <= bytes.len() => offset = next,
            _ => return Err(offset),
        }
    }
    Err(offset)
}

/// `(tick, code, arg)` triples of a type-2 body (tolerates pad bytes).
fn parse_events(count: u16, body: &[u8]) -> Vec<(i32, u8, u8)> {
    let n = usize::from(count);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (Some(t), Some(c), Some(a)) = (
            read_i32(body, 4 * i),
            body.get(4 * n + 2 * i),
            body.get(4 * n + 2 * i + 1),
        ) else {
            break;
        };
        out.push((t, *c, *a));
    }
    out
}

struct StepAnalysis {
    /// Non-zero step rows `(tick, byte)`.
    rows: Vec<(i32, u8)>,
    /// Rows the game counts: shocks, or rows with a panel that no freeze
    /// end resolved as a hold head.
    judged_rows: usize,
    /// Largest tick among steps and freeze-end markers.
    last_tick: Option<i32>,
    first_unsorted: Option<usize>,
}

/// Decode a type-3 body (`docs/ssq_format.md` §5.2) and resolve freezes
/// the way the game does (§5.4): each `0x00` step consumes one freeze
/// entry; for each panel bit of a kind-1 entry the most recent earlier
/// row hitting that panel becomes a freeze head for that bit.
fn analyse_steps(count: u16, body: &[u8]) -> Result<StepAnalysis, String> {
    let n = usize::from(count);
    let steps_start = 4 * n;
    let freeze_start = steps_start + n + (n % 2);
    let step_bytes = body
        .get(steps_start..steps_start + n)
        .ok_or_else(|| format!("body {} B too short for {n} steps", body.len()))?;
    let zero_steps = step_bytes.iter().filter(|&&b| b == 0).count();
    let freeze_block = body
        .get(freeze_start..)
        .ok_or_else(|| "freeze block start past body".to_string())?;
    if freeze_block.len() < 2 * zero_steps {
        return Err(format!(
            "{zero_steps} freeze ends but only {} freeze-block bytes",
            freeze_block.len()
        ));
    }

    // (tick, bits, head_bits)
    let mut notes: Vec<(i32, u8, u8)> = Vec::with_capacity(n);
    let mut rows: Vec<(i32, u8)> = Vec::with_capacity(n);
    let mut last_tick: Option<i32> = None;
    let mut first_unsorted: Option<usize> = None;
    let mut prev_tick: Option<i32> = None;
    let mut freeze_index = 0usize;

    for (i, &byte) in step_bytes.iter().enumerate() {
        let tick = read_i32(body, 4 * i).ok_or_else(|| "tick table truncated".to_string())?;
        if let Some(p) = prev_tick {
            if tick < p && first_unsorted.is_none() {
                first_unsorted = Some(i);
            }
        }
        prev_tick = Some(tick);
        last_tick = Some(last_tick.map_or(tick, |t| t.max(tick)));
        if byte == 0 {
            let panels = freeze_block.get(2 * freeze_index).copied().unwrap_or(0);
            let kind = freeze_block.get(2 * freeze_index + 1).copied().unwrap_or(0);
            freeze_index += 1;
            if kind != 0x01 {
                continue;
            }
            let mut pending = panels;
            for note in notes.iter_mut().rev() {
                if pending == 0 {
                    break;
                }
                let hit = note.1 & pending;
                if hit != 0 {
                    note.2 |= hit;
                    pending &= !hit;
                }
            }
            continue;
        }
        notes.push((tick, byte, 0));
        rows.push((tick, byte));
    }

    let judged_rows = notes
        .iter()
        .filter(|(_, bits, heads)| is_exact_shock(*bits) || (bits & !heads) != 0)
        .count();

    Ok(StepAnalysis {
        rows,
        judged_rows,
        last_tick,
        first_unsorted,
    })
}

fn is_exact_shock(byte: u8) -> bool {
    matches!(byte, 0x0F | 0xF0 | 0xFF)
}

/// A step byte the game would classify as a shock although the writer
/// cannot have meant one (see the module header for the byte-level limit).
fn is_shock_leak(byte: u8, single: bool) -> bool {
    if single {
        return (byte & 0xF0 != 0 && byte != 0xFF) || byte == 0x0F;
    }
    let low_full = byte & 0x0F == 0x0F;
    let high_full = byte & 0xF0 == 0xF0;
    (low_full || high_full) && !is_exact_shock(byte)
}

fn read_i32(buf: &[u8], at: usize) -> Option<i32> {
    let b = buf.get(at..at + 4)?;
    Some(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sm::model::{
        Beat, Bpm, Chart, Difficulty, Note, NoteKind, PanelSet, Rational, ShockSide, Style, Timing,
    };
    use crate::core::ssq::writer::{build_file, prepare_chart, PreparedChart, WriteOptions};

    fn timing() -> Timing {
        Timing {
            bpms: vec![(
                Beat::zero(),
                Bpm::from_rational(Rational::from_integer(120)),
            )],
            ..Timing::default()
        }
    }

    fn note(beat: i64, kind: NoteKind, bits: u8) -> Note {
        Note {
            beat: Beat::from_integer(beat),
            kind,
            panels: PanelSet::from_bits(Style::Double, bits),
        }
    }

    fn prepared(style: Style, notes: Vec<Note>) -> PreparedChart {
        let c = Chart {
            style,
            difficulty: Difficulty::Basic,
            meter: 5,
            description: String::new(),
            timing: None,
            scrolls: None,
            speeds: None,
            notes,
            warnings: Vec::new(),
        };
        prepare_chart(&c, &timing(), &WriteOptions::default()).unwrap()
    }

    fn good_file() -> Vec<u8> {
        let p = prepared(
            Style::Single,
            vec![
                note(0, NoteKind::Tap, 0x01),
                note(
                    2,
                    NoteKind::HoldHead {
                        length: Beat::from_integer(2),
                    },
                    0x02,
                ),
                note(5, NoteKind::Tap, 0x0C),
            ],
        );
        build_file(&timing(), &[p], &WriteOptions::default()).unwrap()
    }

    fn step_chunk_offset(bytes: &[u8]) -> usize {
        let mut off = 0;
        loop {
            let len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
            let kind = u16::from_le_bytes(bytes[off + 4..off + 6].try_into().unwrap());
            if kind == 3 {
                return off;
            }
            off += len;
        }
    }

    #[test]
    fn well_formed_file_passes() {
        assert_eq!(check_ssq(&good_file(), &[1]), Ok(()));
    }

    #[test]
    fn undeclared_slot_and_missing_slot() {
        let f = good_file();
        assert_eq!(
            check_ssq(&f, &[1, 3]),
            Err(vec![CheckFailure::SlotMissing { slot: 3 }])
        );
        assert!(check_ssq(&f, &[12]).is_err());
    }

    #[test]
    fn holds_only_fails_no_steps_but_shock_only_passes() {
        // Bypass the writer's own NoSteps rule by hand-building rows.
        let mut p = prepared(Style::Single, vec![note(0, NoteKind::Tap, 0x01)]);
        p.steps = vec![(0, 0x01), (1024, 0x00)];
        p.freezes = vec![(0x01, 0x01)];
        let bytes = build_file(&timing(), &[p], &WriteOptions::default()).unwrap();
        assert_eq!(
            check_ssq(&bytes, &[1]),
            Err(vec![CheckFailure::NoSteps { slot: 1 }])
        );
        let p = prepared(
            Style::Single,
            vec![note(
                0,
                NoteKind::Shock {
                    side: ShockSide::BothSides,
                },
                0,
            )],
        );
        let bytes = build_file(&timing(), &[p], &WriteOptions::default()).unwrap();
        assert_eq!(check_ssq(&bytes, &[1]), Ok(()));
    }

    #[test]
    fn mines_only_file_has_no_steps() {
        let mut p = prepared(Style::Single, vec![note(0, NoteKind::Tap, 0x01)]);
        p.steps.clear();
        p.freezes.clear();
        p.mines = vec![(0, 0x01)];
        let bytes = build_file(&timing(), &[p], &WriteOptions::default()).unwrap();
        assert_eq!(
            check_ssq(&bytes, &[1]),
            Err(vec![CheckFailure::NoSteps { slot: 1 }])
        );
    }

    #[test]
    fn zero_delta_ms_tempo_is_flagged() {
        let mut bytes = good_file();
        // Tempo chunk is first: header 12, then N ticks, then N seconds.
        let n = u16::from_le_bytes([bytes[8], bytes[9]]) as usize;
        assert!(n >= 2);
        // Make seconds[1] == seconds[0].
        let s0 = bytes[12 + 4 * n..12 + 4 * n + 4].to_vec();
        bytes[12 + 4 * n + 4..12 + 4 * n + 8].copy_from_slice(&s0);
        let err = check_ssq(&bytes, &[1]).unwrap_err();
        assert!(
            err.contains(&CheckFailure::TempoZeroDeltaMs { index: 1 }),
            "{err:?}"
        );
        // Going backwards is "not monotonic".
        let mut bytes = good_file();
        bytes[12 + 4 * n + 4..12 + 4 * n + 8].copy_from_slice(&(-5i32).to_le_bytes());
        let err = check_ssq(&bytes, &[1]).unwrap_err();
        assert!(
            err.contains(&CheckFailure::TempoNotMonotonic { index: 1 }),
            "{err:?}"
        );
    }

    #[test]
    fn end_before_last_note_and_unbracketed_finish() {
        let mut bytes = good_file();
        // Events chunk follows the tempo chunk; rewrite END (event 5) and FINISH (event 4).
        let tempo_len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let ev = tempo_len;
        let end_at = ev + 12 + 4 * 5;
        bytes[end_at..end_at + 4].copy_from_slice(&4096i32.to_le_bytes());
        let err = check_ssq(&bytes, &[1]).unwrap_err();
        assert!(
            err.iter()
                .any(|f| matches!(f, CheckFailure::EndBeforeLastNote { slot: 1, .. })),
            "{err:?}"
        );
        let mut bytes = good_file();
        let fin_at = ev + 12 + 4 * 4;
        bytes[fin_at..fin_at + 4].copy_from_slice(&(1i32 << 24).to_le_bytes());
        let err = check_ssq(&bytes, &[1]).unwrap_err();
        assert!(
            err.iter()
                .any(|f| matches!(f, CheckFailure::FinishNotBracketed { .. })),
            "{err:?}"
        );
    }

    #[test]
    fn shock_byte_leak_on_single_chart_is_flagged() {
        let mut bytes = good_file();
        let off = step_chunk_offset(&bytes);
        let n = u16::from_le_bytes([bytes[off + 8], bytes[off + 9]]) as usize;
        // First step byte → 0x0F.
        bytes[off + 12 + 4 * n] = 0x0F;
        let err = check_ssq(&bytes, &[1]).unwrap_err();
        assert!(
            err.iter().any(|f| matches!(
                f,
                CheckFailure::ShockByteOnNoteRow {
                    slot: 1,
                    tick: 0,
                    byte: 0x0F
                }
            )),
            "{err:?}"
        );
        assert!(is_shock_leak(0x1F, false));
        assert!(is_shock_leak(0xF1, false));
        assert!(!is_shock_leak(0x0F, false));
        assert!(!is_shock_leak(0xFF, true));
        assert!(is_shock_leak(0x10, true));
    }

    #[test]
    fn truncated_and_misaligned_files_are_not_walkable() {
        let f = good_file();
        let err = check_ssq(&f[..f.len() - 4], &[1]).unwrap_err();
        assert!(
            err.iter()
                .any(|x| matches!(x, CheckFailure::ChunkListNotWalkable { .. })),
            "{err:?}"
        );
        let mut g = f.clone();
        g[0] = 0x0E; // tempo chunk length no longer a multiple of 4
        let err = check_ssq(&g, &[1]).unwrap_err();
        assert!(err.contains(&CheckFailure::ChunkListNotWalkable { offset: 0 }));
        assert!(check_ssq(&[], &[1]).is_err());
    }
}
