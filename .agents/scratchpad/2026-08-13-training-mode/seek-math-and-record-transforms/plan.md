# Plan: seek-math-and-record-transforms

Status: Approved 2026-08-13 (verified upstream approval — same chain as task-01)

## Implementation shape

### 1. Directory conversion (mechanical, first)
`git mv src/services/song_reset.rs src/services/song_reset/mod.rs`; add
`pub mod seek;` + `#[cfg(test)] mod seek_tests;`. No call-site changes
(`services::song_reset::…` paths unchanged). Gate: cargo check + harness green
before the pure module lands (harness untouched at this point).

### 2. Pure module — `src/services/song_reset/seek.rs`
No logging, no game reads, host-mountable. Contents:

**Layout constants** (pub — task-03's engine caller consumes them):
note stride/field offsets (0x60; kind +0x00, display +0x04, raw +0x08, flags
+0x1C, durations +0x3C, 8 panels), record stride/field offsets (0x40;
judgedAt +0x08, grade +0x0C, hold +0x14, wobble +0x34), grade values
(PENDING 0xFF, CONSUMED_TAP 0, ARMED 5, OK 6, ARMED_SHOCK 7).

**Quantization**:
```
pub struct SeekQuantization { pub blocks: u64, pub t_q_ms: i32 }
pub fn quantize_seek(t_ms: i32, samples_per_block: u32, sample_rate: u32,
                     max_blocks: u64) -> Option<SeekQuantization>
```
frame-domain floor to the block grid, clamp [0, max_blocks], half-up ms of the
grid point.

**Anchor**:
```
pub fn anchor_tick(now_tick: u64, delay_ms: u64, t_q_ms: i32,
                   snapshot: &RateSnapshot) -> u64
```
`now + delay − wall(T_q)`; identity/uncommitted ⇒ literal `now + delay − t_q`
(legacy bit-identical, the tick_domain pattern); conversion failure ⇒ identity
fallback (never a panic).

**Note decode + rebuild expectations**:
```
pub struct NoteView { kind, display_time, raw_time, panel_flags: [i32;8], durations: [i32;8] }
pub fn decode_notes(bytes: &[u8]) -> Option<Vec<NoteView>>       // len % 0x60 == 0
pub struct RecordExpectation { note_index, judged_at, grade, hold_progress: [i32;8], freeze_participant }
pub fn rebuild_expectations(notes: &[NoteView], playhead_ms: i32) -> Vec<RecordExpectation>
```
per-kind semantics from research §3.2 incl. the kind-2 pre-T back-patch
(backward match on participation mask + freeze participant).

**Neutralization planner**:
```
pub struct RecordWrite { pub byte_offset: usize, pub value: i32 }
pub fn neutralization_writes(notes: &[NoteView], t_q_ms: i32) -> Vec<RecordWrite>
```
spanning freezes only (head raw < T_q < end raw, strict): head hold-progress
8 writes (full durations) + end grade 6 + end judgedAt.

### 3. Tests — `src/services/song_reset/seek_tests.rs` (harness-mounted)
- T1 quantization: 0, negative, exact boundary, just-below/above boundary,
  past max_blocks clamp, invalid params → None; grid floor property
  (`blocks·spb ≤ t·rate/1000 < (blocks+1)·spb`); t_q half-up exactness. [AC-1]
- T2 anchor: identity bit-exact (`now + delay − t_q` incl. delay 0); 50 %
  snapshot (`RateRatio{1,2}` ⇒ wall = 2·t_q) exact vs a direct
  `content_to_wall_ms` call; uncommitted-at-rate falls to legacy. [AC-2]
- T3 rebuild expectations: synthetic vector with control notes, taps pre/post
  T, a shock pre-T (grade 6), kind-1 markers, a fully-pre-T freeze (back-patch
  lands on the head), record indexing skips control notes.
- T4 neutralization: no freezes / fully-before / spanning single-panel /
  spanning multi-panel / after-T / back-to-back (first spans, second pending) —
  exact offsets and values, nothing else. [AC-3]
- Harness mount added for `song_reset::seek` (+ tests).

## Test data
Synthetic note byte builder (`note_bytes(kind, display, raw, flags, durations)`),
hand-computed expectations.

## Risks
- The record-field value model (grade as i32, end-consumed = grade 6 + judgedAt)
  is the design's letter; live semantics get proven by task-03/04's cabinet
  demo — the planner is the single tuning point if the engine disagrees.
