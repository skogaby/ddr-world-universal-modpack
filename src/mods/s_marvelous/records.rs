//! Stage-record per-note stream recompute (design §5.2, display-side RE
//! §0.1): the game's stage record stores a grade byte AND a signed ms
//! error per judged note, so the results surfaces recompute S-Marvelous
//! counts purely from the record — independent of the live gameplay
//! counters (which reset on scene churn) and correct for every stage of a
//! multi-stage session.
//!
//! Layout (all offsets into a stage record; MSVC `std::vector` =
//! begin/end/cap-end pointer triple):
//!
//! | offset | contents |
//! |---|---|
//! | +0x28 | per-grade counts `[i32; 8]`, Marvelous first |
//! | +0xB8..0xC0 | `vector<u8>` grade class per judged note (0=M, 1=P, 2=Gr, 3=Gd, 6=OK) |
//! | +0xD8..0xE0 | `vector<i16>` signed ms error per judged note — `expected − actual`, so `> 0` = FAST/early (the INVERSE of the live judge delta; see [`count_marv_fast_slow`]) |
//!
//! Everything is FAIL-CLOSED: any structural surprise (length mismatch,
//! implausible sizes, null/misaligned pointers, stream-vs-counter
//! disagreement) returns `None` and the caller keeps the stock display
//! (design §6: "record streams malformed ⇒ results surfaces skip").
//!
//! This module is std-only (no crate imports) so the offline validation
//! harness mounts it directly (`scripts/validate_s_marvelous.sh`); the
//! pure core is host-tested, the raw-pointer readers are exercised on the
//! cabinet only.

/// Grade-class byte for Marvelous in the record's grade stream.
pub const GRADE_MARVELOUS: u8 = 0;
/// Grade-class byte for a freeze-arrow O.K. — the binary "hit" grade for
/// freezes. It carries no ms delta and the game maps it to the TOP tier
/// everywhere it is tiered (combo worst-judgement tracking, the results
/// graph's marvelous+O.K. series), so with the mod on it belongs to the
/// S-Marvelous tier.
pub const GRADE_OK: u8 = 6;

/// Record field offsets (design §5.2).
const REC_GRADE_COUNTS: usize = 0x28;
const REC_NOTES_VEC: usize = 0x98;
const REC_GRADES_VEC: usize = 0xB8;
const REC_ERRORS_VEC: usize = 0xD8;
/// Note-entry layout (0x60-stride vector at +0x98; graph-ingest RE
/// 2026-08-30): flag byte at +0x00 (only entries with flag ≥ 0 occupy a
/// grade/ms stream slot), timestamp ms at +0x08, unjudged flag at +0x18.
const NOTE_STRIDE: usize = 0x60;
const NOTE_TIMESTAMP: usize = 0x08;
const NOTE_UNJUDGED: usize = 0x18;

/// Sanity cap on the per-note stream length. The densest charts run to a
/// few thousand judged notes; anything past this is a misdecoded vector.
pub const MAX_NOTES: usize = 65_536;

/// Pure core: count S-Marvelous over aligned grade/ms streams.
///
/// `None` when the streams disagree in length (a length mismatch means the
/// two vectors are not the parallel per-note streams we think they are —
/// fail closed rather than guess at alignment).
pub fn count_smarv(grades: &[u8], errors_ms: &[i16], window_ms: i32) -> Option<u32> {
    if grades.len() != errors_ms.len() || window_ms <= 0 {
        return None;
    }
    let mut n = 0u32;
    for (&g, &ms) in grades.iter().zip(errors_ms.iter()) {
        if g == GRADE_MARVELOUS && (ms as i32).abs() <= window_ms {
            n += 1;
        }
    }
    Some(n)
}

/// Pure core: count occurrences of a grade class in the grade stream (the
/// cross-check anchor for [`read_streams`]' consistency gate, and the
/// Marvelous total the exclusive rewrite subtracts from).
pub fn count_grade(grades: &[u8], grade: u8) -> u32 {
    grades.iter().filter(|&&g| g == grade).count() as u32
}

/// Pure core: `(fast, slow)` counts over the NON-S Marvelous slots of
/// aligned grade/ms streams — grade 0 AND outside the S-Marvelous window.
/// This is the share the stock FAST/SLOW counters leave out
/// (`judge_submit` only counts grades 1..=4; research §2 step 2) that the
/// mod adds back: the highest tier is exempt from FAST/SLOW, and with the
/// mod on that tier is S-Marvelous, not Marvelous.
///
/// SIGN CONVENTION — the record stream is NOT the live judge delta. The
/// result commit writes `rec+0xD8` as `note.expected − result.actual`
/// (`FUN_1801e6ca0`, 20260825: `*(note+8) − result[+8]`, grade 6 → 0,
/// grade 7 → ±0xA0), so in the STREAM `ms > 0` = FAST (early) and `ms < 0`
/// = SLOW (late). The game's own graph ingest reads it that way (FAST is
/// the positive axis; `stream > 0` → the cyan FAST series). The live
/// `judge_submit` delta the stock `+0x1C4/+0x1C8` counters and the gameplay
/// indicator use is the INVERSE (`actual − expected`, `< 0` = fast); the
/// first port assumed the stream shared that sign and swapped the two
/// results widgets (tester report 2026-09). An S-Marv window ≥ 1 already
/// covers `ms == 0`.
///
/// `None` on a stream length mismatch or a non-positive window (fail
/// closed, like [`count_smarv`]).
pub fn count_marv_fast_slow(
    grades: &[u8],
    errors_ms: &[i16],
    window_ms: i32,
) -> Option<(u32, u32)> {
    if grades.len() != errors_ms.len() || window_ms <= 0 {
        return None;
    }
    let mut fast = 0u32;
    let mut slow = 0u32;
    for (&g, &ms) in grades.iter().zip(errors_ms.iter()) {
        if g != GRADE_MARVELOUS || (ms as i32).abs() <= window_ms {
            continue; // not Marvelous, or S-Marvelous (top tier: exempt)
        }
        if ms > 0 {
            fast += 1; // stream sign: expected − actual > 0 ⇒ early
        } else {
            slow += 1;
        }
    }
    Some((fast, slow))
}

/// Record field offsets of the stock FAST/SLOW counters (result commit
/// copies GamePlayActor `+0x1C4/+0x1C8` here; research §3.6).
const REC_FAST: usize = 0x6C;
const REC_SLOW: usize = 0x70;

/// The record's own stock FAST/SLOW counters — exactly the values the
/// populate wrote into `fast_usr/num_usr` / `slow_usr/num_usr`.
///
/// # Safety
/// `record` must point at a live stage record, on the game thread.
pub unsafe fn stock_fast_slow_from_record(record: *const u8) -> Option<(u32, u32)> {
    if record.is_null() {
        return None;
    }
    let fast = (record.add(REC_FAST) as *const i32).read_unaligned();
    let slow = (record.add(REC_SLOW) as *const i32).read_unaligned();
    if fast < 0 || slow < 0 || fast > MAX_NOTES as i32 || slow > MAX_NOTES as i32 {
        return None;
    }
    Some((fast as u32, slow as u32))
}

/// Read an MSVC `vector<T>` header (begin/end pair) at `record + offset`
/// into a bounded element count. `None` on null/backwards/oversized/
/// misaligned vectors.
///
/// # Safety
/// `record` must point at a live stage record (caller resolves it through
/// `stage_records` on the game thread).
unsafe fn read_vec_bounds<T>(record: *const u8, offset: usize) -> Option<(*const T, usize)> {
    let begin = (record.add(offset) as *const *const T).read_unaligned();
    let end = (record.add(offset + 8) as *const *const T).read_unaligned();
    // A default-constructed empty vector is null/null — legal, zero notes.
    if begin.is_null() && end.is_null() {
        return Some((std::ptr::NonNull::<T>::dangling().as_ptr(), 0));
    }
    if begin.is_null() || end.is_null() {
        return None;
    }
    let (b, e) = (begin as usize, end as usize);
    if e < b || !b.is_multiple_of(std::mem::align_of::<T>()) {
        return None;
    }
    let bytes = e - b;
    if !bytes.is_multiple_of(std::mem::size_of::<T>()) {
        return None;
    }
    let len = bytes / std::mem::size_of::<T>();
    if len > MAX_NOTES {
        return None;
    }
    Some((begin, len))
}

/// Copy the record's grade + ms-error streams out into owned buffers,
/// JUDGED SLOTS ONLY, fail-closed (design §5.2). The copy (a few KiB)
/// decouples every later computation from the live record.
///
/// Judged gating (Step-10 hardening): the streams carry one slot per
/// flag≥0 note entry ALLOCATED UP FRONT — on a partial play (quick fail)
/// unjudged slots keep their initial grade-0 value, which would both
/// poison the Marvelous count and trip the counter cross-check below
/// (stock tab after every quick-fail). The note-entry vector carries the
/// per-slot judged flag (stream-aligned by construction — the graph's
/// ingest mirror); slots without a judged note entry are dropped. Full
/// plays filter to identity (cabinet-validated behavior unchanged).
///
/// Consistency gate: the judged grade stream's Marvelous count must equal
/// the record's own per-grade Marvelous counter (`+0x28`). A disagreement
/// means the assumed layout drifted — refuse rather than render wrong
/// numbers.
///
/// # Safety
/// `record` must point at a live stage record, on the game thread.
pub unsafe fn read_streams(record: *const u8) -> Option<(Vec<u8>, Vec<i16>)> {
    if record.is_null() {
        return None;
    }
    let (g_ptr, g_len) = read_vec_bounds::<u8>(record, REC_GRADES_VEC)?;
    let (e_ptr, e_len) = read_vec_bounds::<i16>(record, REC_ERRORS_VEC)?;
    if g_len != e_len {
        return None;
    }
    // Empty streams are legal (a quick-failed song can end with zero judged
    // notes — the tab shows all zeros); the counter cross-check below still
    // applies (must be 0).
    let raw_grades = std::slice::from_raw_parts(g_ptr, g_len);
    let raw_errors = std::slice::from_raw_parts(e_ptr, e_len);
    let notes = read_note_refs(record)?;
    let (grades, errors) = filter_judged(raw_grades, raw_errors, &notes);

    let marv_counter = (record.add(REC_GRADE_COUNTS) as *const i32).read_unaligned();
    if marv_counter < 0 || count_grade(&grades, GRADE_MARVELOUS) != marv_counter as u32 {
        return None;
    }
    Some((grades, errors))
}

/// Keep only stream slots whose note entry was JUDGED (pure core of the
/// [`read_streams`] gating — see its docs for the partial-play rationale).
/// Slots past the note list are dropped (unjudged-unknown, mirroring the
/// graph ingest's `idx < len` gate).
pub fn filter_judged(grades: &[u8], errors: &[i16], notes: &[NoteRef]) -> (Vec<u8>, Vec<i16>) {
    let n = grades.len().min(errors.len()).min(notes.len());
    let mut g = Vec::with_capacity(n);
    let mut e = Vec::with_capacity(n);
    for i in 0..n {
        if notes[i].judged {
            g.push(grades[i]);
            e.push(errors[i]);
        }
    }
    (g, e)
}

/// The results-side recompute (design §4.7): S-Marvelous count for a stage
/// record, fail-closed.
///
/// # Safety
/// `record` must point at a live stage record, on the game thread.
pub unsafe fn smarv_count_from_record(record: *const u8, window_ms: i32) -> Option<u32> {
    let (grades, errors) = read_streams(record)?;
    count_smarv(&grades, &errors, window_ms)
}

/// The record's own Marvelous counter (`+0x28`) — the total the exclusive
/// MARVELOUS rewrite subtracts the S-Marv count from.
///
/// # Safety
/// `record` must point at a live stage record, on the game thread.
pub unsafe fn marv_count_from_record(record: *const u8) -> Option<u32> {
    if record.is_null() {
        return None;
    }
    let n = (record.add(REC_GRADE_COUNTS) as *const i32).read_unaligned();
    if n < 0 {
        return None;
    }
    Some(n as u32)
}

// ── Per-second bucketing (results graph, plan Step 8) ────────────────

/// One stream-aligned note reference: a grade/ms stream slot exists for
/// EVERY note entry whose flag byte is ≥ 0 — judged or not (the graph
/// ingest advances its stream index per flag≥0 entry and gates the series
/// adds on the judged flag; RE 2026-08-30). `t_ms` is the note's chart
/// timestamp.
#[derive(Clone, Copy, Debug)]
pub struct NoteRef {
    pub judged: bool,
    pub t_ms: i32,
}

/// Pure core: per-second counts for the results graph's VIOLET series —
/// S-Marvelous hits PLUS freeze O.K.s — mirroring the graph ingest's
/// bucketing exactly so our vector's buckets align 1:1 with the game's
/// judge series: `t_first` = the first JUDGED note's timestamp, bucket =
/// `(t − t_first) / 1000`, one stream slot per entry, judged-only.
///
/// O.K. rides the violet series because the stock ingest folds grade 6
/// into its marvelous+O.K. series (freezes are binary hit/miss, so the
/// game colours the hit as the highest tier it knows); with S-Marvelous
/// on, the highest tier is violet. The caller subtracts this vector from
/// that stock series, so every O.K. moves from opal to violet.
///
/// `None` when the streams disagree in length; an empty/never-judged
/// record yields an empty vector (nothing to draw — matches the tab's
/// has-data gate).
pub fn violet_per_second(
    notes: &[NoteRef],
    grades: &[u8],
    errors_ms: &[i16],
    window_ms: i32,
) -> Option<Vec<f64>> {
    if grades.len() != errors_ms.len() || window_ms <= 0 {
        return None;
    }
    let t_first = match notes.iter().find(|n| n.judged) {
        Some(n) => n.t_ms,
        None => return Some(Vec::new()),
    };
    let mut out: Vec<f64> = Vec::new();
    for (idx, note) in notes.iter().enumerate() {
        if !note.judged || note.t_ms < t_first || idx >= grades.len() {
            continue;
        }
        if is_violet_slot(grades[idx], errors_ms[idx], window_ms) {
            let bucket = ((note.t_ms - t_first) / 1000) as usize;
            if bucket >= out.len() {
                out.resize(bucket + 1, 0.0);
            }
            out[bucket] += 1.0;
        }
    }
    Some(out)
}

/// Whether a judged stream slot belongs to the graph's violet tier: an
/// S-Marvelous (grade 0 inside the window) or a freeze O.K. (grade 6).
fn is_violet_slot(grade: u8, error_ms: i16, window_ms: i32) -> bool {
    (grade == GRADE_MARVELOUS && (error_ms as i32).abs() <= window_ms) || grade == GRADE_OK
}

/// Pure core: split the violet per-second vector into the seconds that are
/// PURE top tier — every judged note of the second is S-Marvelous/O.K.,
/// i.e. every OTHER judge series is empty there — and the MIXED remainder.
///
/// This is the stock ingest's all-Marvelous post-pass condition
/// (`filler, miss, good, great, perfect ≤ 0` ⇒ the second's count moves
/// to the gradient series) transplanted to the new top tier: `others` are
/// the game's other judge series AFTER the mod's subtraction/fold, so a
/// second that still holds a loose Marvelous is MIXED. The pure seconds
/// draw with the stock shimmer gradient (violet → light violet), the mixed
/// ones flat violet. Both outputs have `violet.len()` entries; a series
/// shorter than `violet` counts as zero past its end (the game resizes all
/// judge series together, so this never triggers in practice).
pub fn split_pure_seconds(violet: &[f64], others: &[&[f64]]) -> (Vec<f64>, Vec<f64>) {
    let mut pure = vec![0.0; violet.len()];
    let mut mixed = vec![0.0; violet.len()];
    for (s, &v) in violet.iter().enumerate() {
        if v <= 0.0 {
            continue;
        }
        let alone = others
            .iter()
            .all(|series| series.get(s).is_none_or(|&o| o <= 0.0));
        if alone {
            pure[s] = v;
        } else {
            mixed[s] = v;
        }
    }
    (pure, mixed)
}

/// Pure core: per-second `(fast, slow)` counts of the LOOSE Marvelous
/// slots (grade 0 outside the S-Marvelous window) for the results TIMING
/// graph — the page whose stock series stop at PERFECT because Marvelous
/// was the exempt top tier. Same bucketing as [`violet_per_second`]
/// (`t_first` = first JUDGED note, bucket = `(t − t_first) / 1000`) so the
/// two vectors align 1:1 with the game's timing series, and the same
/// STREAM sign as [`count_marv_fast_slow`] (`ms > 0` = FAST). Both vectors
/// come back the same length (the last bucket either side touched).
///
/// `None` on a stream length mismatch or a non-positive window; a
/// never-judged record yields two empty vectors.
pub fn marvelous_fast_slow_per_second(
    notes: &[NoteRef],
    grades: &[u8],
    errors_ms: &[i16],
    window_ms: i32,
) -> Option<(Vec<f64>, Vec<f64>)> {
    if grades.len() != errors_ms.len() || window_ms <= 0 {
        return None;
    }
    let t_first = match notes.iter().find(|n| n.judged) {
        Some(n) => n.t_ms,
        None => return Some((Vec::new(), Vec::new())),
    };
    let mut fast: Vec<f64> = Vec::new();
    let mut slow: Vec<f64> = Vec::new();
    for (idx, note) in notes.iter().enumerate() {
        if !note.judged || note.t_ms < t_first || idx >= grades.len() {
            continue;
        }
        let (g, ms) = (grades[idx], errors_ms[idx]);
        if g != GRADE_MARVELOUS || (ms as i32).abs() <= window_ms {
            continue; // not Marvelous, or S-Marvelous (top tier: exempt)
        }
        let bucket = ((note.t_ms - t_first) / 1000) as usize;
        let target = if ms > 0 { &mut fast } else { &mut slow };
        if bucket >= target.len() {
            target.resize(bucket + 1, 0.0);
        }
        target[bucket] += 1.0;
    }
    let len = fast.len().max(slow.len());
    fast.resize(len, 0.0);
    slow.resize(len, 0.0);
    Some((fast, slow))
}

/// Copy the record's note-entry vector (+0x98, 0x60-stride) into
/// stream-aligned [`NoteRef`]s — ONLY flag≥0 entries, in order, so index
/// `i` here pairs with `grades[i]`/`errors[i]`. Fail-closed on structural
/// surprises.
///
/// # Safety
/// `record` must point at a live stage record, on the game thread.
pub unsafe fn read_note_refs(record: *const u8) -> Option<Vec<NoteRef>> {
    if record.is_null() {
        return None;
    }
    let begin = (record.add(REC_NOTES_VEC) as *const *const u8).read_unaligned();
    let end = (record.add(REC_NOTES_VEC + 8) as *const *const u8).read_unaligned();
    if begin.is_null() && end.is_null() {
        return Some(Vec::new());
    }
    if begin.is_null() || end.is_null() {
        return None;
    }
    let (b, e) = (begin as usize, end as usize);
    if e < b || !(e - b).is_multiple_of(NOTE_STRIDE) {
        return None;
    }
    let count = (e - b) / NOTE_STRIDE;
    if count > MAX_NOTES {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let entry = begin.add(i * NOTE_STRIDE);
        let flag = *(entry as *const i8);
        if flag < 0 {
            continue; // no stream slot for these
        }
        out.push(NoteRef {
            judged: *(entry.add(NOTE_UNJUDGED)) == 0,
            t_ms: (entry.add(NOTE_TIMESTAMP) as *const i32).read_unaligned(),
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_smarv_window_edges_inclusive() {
        // |ms| == window counts; |ms| == window+1 does not.
        let grades = [0u8, 0, 0, 0];
        let errors = [12i16, -12, 13, -13];
        assert_eq!(count_smarv(&grades, &errors, 12), Some(2));
    }

    #[test]
    fn count_smarv_only_marvelous_grades_count() {
        // Tight Perfect/Great/Good/OK never count, however small the error.
        let grades = [0u8, 1, 2, 3, 6];
        let errors = [0i16, 0, 0, 0, 0];
        assert_eq!(count_smarv(&grades, &errors, 12), Some(1));
    }

    #[test]
    fn count_smarv_rejects_length_mismatch() {
        assert_eq!(count_smarv(&[0u8, 0], &[0i16], 12), None);
        assert_eq!(count_smarv(&[0u8], &[0i16, 0], 12), None);
    }

    #[test]
    fn count_smarv_rejects_nonpositive_window() {
        assert_eq!(count_smarv(&[0u8], &[0i16], 0), None);
        assert_eq!(count_smarv(&[0u8], &[0i16], -5), None);
    }

    #[test]
    fn count_smarv_empty_streams_zero() {
        assert_eq!(count_smarv(&[], &[], 12), Some(0));
    }

    #[test]
    fn count_grade_counts_exactly() {
        let grades = [0u8, 1, 0, 6, 0, 3];
        assert_eq!(count_grade(&grades, 0), 3);
        assert_eq!(count_grade(&grades, 6), 1);
        assert_eq!(count_grade(&grades, 5), 0);
    }

    #[test]
    fn marv_fast_slow_excludes_smarvelous_and_uses_stream_sign() {
        // Window 12: |ms| ≤ 12 is S-Marvelous (top tier, exempt); only the
        // loose Marvelous count. STREAM sign (`expected − actual`):
        // positive = fast, negative = slow — the inverse of the live delta.
        let grades = [0u8, 0, 0, 0, 0, 0, 0];
        let errors = [-1i16, -12, 0, 3, 12, -13, 16];
        // -13 → slow, 16 → fast.
        assert_eq!(count_marv_fast_slow(&grades, &errors, 12), Some((1, 1)));
        let errors = [-13i16, -20, 16];
        assert_eq!(
            count_marv_fast_slow(&grades[..3], &errors, 12),
            Some((1, 2))
        );
    }

    #[test]
    fn marv_fast_slow_window_edge_is_smarvelous() {
        // Exactly |window| is S-Marvelous (inclusive, like count_smarv);
        // window+1 is a loose Marvelous.
        let grades = [0u8, 0, 0, 0];
        let errors = [16i16, -16, 17, -17];
        assert_eq!(count_marv_fast_slow(&grades, &errors, 16), Some((1, 1)));
        assert_eq!(count_marv_fast_slow(&grades, &errors, 12), Some((2, 2)));
    }

    #[test]
    fn marv_fast_slow_partitions_marvelous_with_count_smarv() {
        // Every Marvelous slot is exactly one of: S-Marv, loose-fast,
        // loose-slow (the results rows must sum to the stock totals).
        let grades = [0u8, 0, 0, 0, 0, 0, 1, 6];
        let errors = [-20i16, -12, -3, 0, 5, 15, 40, 0];
        let smarv = count_smarv(&grades, &errors, 12).unwrap();
        let (fast, slow) = count_marv_fast_slow(&grades, &errors, 12).unwrap();
        // -20 → slow (late), 15 → fast (early).
        assert_eq!((smarv, fast, slow), (4, 1, 1));
        assert_eq!(smarv + fast + slow, count_grade(&grades, GRADE_MARVELOUS));
    }

    #[test]
    fn marv_fast_slow_ignores_other_grades() {
        // Lower grades are already in the stock counters; OK carries no
        // delta. Only loose grade-0 slots contribute (-14 → slow).
        let grades = [1u8, 2, 3, 4, 6, 0];
        let errors = [-30i16, 40, -60, 90, 0, -14];
        assert_eq!(count_marv_fast_slow(&grades, &errors, 12), Some((0, 1)));
    }

    #[test]
    fn marv_fast_slow_rejects_length_mismatch_bad_window_and_handles_empty() {
        assert_eq!(count_marv_fast_slow(&[0u8, 0], &[1i16], 12), None);
        assert_eq!(count_marv_fast_slow(&[0u8], &[15i16], 0), None);
        assert_eq!(count_marv_fast_slow(&[], &[], 12), Some((0, 0)));
    }

    #[test]
    fn exclusive_marvelous_never_negative_by_subset() {
        // The exclusive rewrite computes stock − smarv; by construction
        // smarv ≤ stream marvelous count. Guard the arithmetic shape used
        // by the caller.
        let grades = [0u8, 0, 0];
        let errors = [5i16, -20, 3];
        let smarv = count_smarv(&grades, &errors, 12).unwrap();
        let marv = count_grade(&grades, GRADE_MARVELOUS);
        assert_eq!((marv - smarv, smarv), (1, 2));
    }

    fn note(judged: bool, t_ms: i32) -> NoteRef {
        NoteRef { judged, t_ms }
    }

    #[test]
    fn per_second_buckets_by_first_judged_timestamp() {
        // t_first = 1500 (first JUDGED — the unjudged slot 0 advances the
        // stream index but sets no origin). Buckets: (t−1500)/1000.
        let notes = [
            note(false, 1000), // slot 0: unjudged
            note(true, 1500),  // slot 1: bucket 0
            note(true, 2499),  // slot 2: bucket 0
            note(true, 2500),  // slot 3: bucket 1
            note(true, 4600),  // slot 4: bucket 3
        ];
        let grades = [0u8, 0, 0, 0, 0];
        let errors = [0i16, 3, -12, 13, 0];
        // slot 0 skipped (unjudged), slot 3 loose (13 > 12).
        let v = violet_per_second(&notes, &grades, &errors, 12).unwrap();
        assert_eq!(v, vec![2.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn per_second_counts_smarvelous_and_ok_only() {
        // Perfect (1) never counts; a tight Marvelous and a freeze O.K.
        // (grade 6, no ms delta) both land in the violet series.
        let notes = [note(true, 0), note(true, 100), note(true, 200)];
        let grades = [1u8, 0, 6];
        let errors = [0i16, 0, 0];
        let v = violet_per_second(&notes, &grades, &errors, 12).unwrap();
        assert_eq!(v, vec![2.0]);
    }

    #[test]
    fn per_second_ok_counts_regardless_of_ms_slot_and_loose_marvelous_does_not() {
        // O.K. is binary: whatever the (unused) ms slot holds, it's violet.
        // A loose Marvelous (|ms| > window) stays with the stock series.
        let notes = [note(true, 0), note(true, 1000), note(true, 2000)];
        let grades = [6u8, 6, 0];
        let errors = [0i16, 500, 13];
        let v = violet_per_second(&notes, &grades, &errors, 12).unwrap();
        assert_eq!(v, vec![1.0, 1.0]);
    }

    #[test]
    fn per_second_never_judged_is_empty() {
        let notes = [note(false, 0), note(false, 500)];
        let grades = [0u8, 0];
        let errors = [0i16, 0];
        assert_eq!(
            violet_per_second(&notes, &grades, &errors, 12).unwrap(),
            Vec::<f64>::new()
        );
    }

    #[test]
    fn per_second_stream_shorter_than_notes_is_tolerated() {
        // The ingest gates stream reads on idx < len — extra note entries
        // past the stream end contribute nothing.
        let notes = [note(true, 0), note(true, 1000), note(true, 2000)];
        let grades = [0u8, 0];
        let errors = [0i16, 0];
        let v = violet_per_second(&notes, &grades, &errors, 12).unwrap();
        assert_eq!(v, vec![1.0, 1.0]);
    }

    #[test]
    fn per_second_rejects_stream_length_mismatch() {
        let notes = [note(true, 0)];
        assert!(violet_per_second(&notes, &[0u8, 0], &[0i16], 12).is_none());
        assert!(violet_per_second(&notes, &[0u8], &[0i16], 0).is_none());
    }

    #[test]
    fn timing_per_second_splits_loose_marvelous_by_stream_sign() {
        // Window 12. Slot 1 (+15, early ⇒ FAST) bucket 0; slot 2 (−20,
        // late ⇒ SLOW) bucket 1; slot 3 is an S-Marvelous (exempt); slot 4
        // (+13) FAST bucket 3. Both vectors padded to the longer (4).
        let notes = [
            note(true, 1000),
            note(true, 1500),
            note(true, 2500),
            note(true, 3200),
            note(true, 4600),
        ];
        let grades = [1u8, 0, 0, 0, 0];
        let errors = [40i16, 15, -20, 3, 13];
        let (fast, slow) = marvelous_fast_slow_per_second(&notes, &grades, &errors, 12).unwrap();
        assert_eq!(fast, vec![1.0, 0.0, 0.0, 1.0]);
        assert_eq!(slow, vec![0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn timing_per_second_window_edge_and_zero_are_exempt() {
        // |ms| == window is S-Marvelous; ms == 0 is always inside a window
        // ≥ 1; only window+1 and beyond are loose.
        let notes = [note(true, 0), note(true, 0), note(true, 0), note(true, 0)];
        let grades = [0u8, 0, 0, 0];
        let errors = [12i16, -12, 0, -13];
        let (fast, slow) = marvelous_fast_slow_per_second(&notes, &grades, &errors, 12).unwrap();
        assert_eq!((fast, slow), (vec![0.0], vec![1.0]));
    }

    #[test]
    fn timing_per_second_partitions_with_violet_series() {
        // Every judged grade-0 slot is exactly one of violet / loose-fast /
        // loose-slow, so the per-second totals partition the Marvelous
        // count (the graph's stacked bars must sum to the stock height).
        let notes: Vec<NoteRef> = (0..8).map(|i| note(true, i * 400)).collect();
        let grades = [0u8, 0, 0, 0, 0, 0, 1, 6];
        let errors = [-20i16, -12, -3, 0, 5, 15, 40, 0];
        let violet = violet_per_second(&notes, &grades, &errors, 12).unwrap();
        let (fast, slow) = marvelous_fast_slow_per_second(&notes, &grades, &errors, 12).unwrap();
        let sum = |v: &[f64]| v.iter().sum::<f64>();
        // violet = 4 S-Marv + 1 O.K.; loose = 1 fast + 1 slow.
        assert_eq!((sum(&violet), sum(&fast), sum(&slow)), (5.0, 1.0, 1.0));
        assert_eq!(
            (sum(&violet) - 1.0 + sum(&fast) + sum(&slow)) as u32,
            count_grade(&grades, GRADE_MARVELOUS)
        );
    }

    #[test]
    fn timing_per_second_unjudged_and_empty() {
        let notes = [note(false, 0), note(true, 1500), note(false, 2500)];
        let grades = [0u8, 0, 0];
        let errors = [30i16, 30, 30];
        // Only slot 1 is judged; t_first = 1500 ⇒ bucket 0.
        let (fast, slow) = marvelous_fast_slow_per_second(&notes, &grades, &errors, 12).unwrap();
        assert_eq!((fast, slow), (vec![1.0], vec![0.0]));
        let (fast, slow) =
            marvelous_fast_slow_per_second(&[note(false, 0)], &[0u8], &[30i16], 12).unwrap();
        assert!(fast.is_empty() && slow.is_empty());
        assert!(marvelous_fast_slow_per_second(&notes, &[0u8, 0], &[0i16], 12).is_none());
        assert!(marvelous_fast_slow_per_second(&notes, &grades, &errors, 0).is_none());
    }

    #[test]
    fn filter_judged_drops_unjudged_and_tail() {
        // Partial play: slots 1 and 3 unjudged (grade-0 garbage), slot 4
        // past the note list (unjudged-unknown) — all dropped.
        let grades = [0u8, 0, 1, 0, 0];
        let errors = [3i16, 0, 20, 0, 0];
        let notes = [note(true, 0), note(false, 0), note(true, 0), note(false, 0)];
        let (g, e) = filter_judged(&grades, &errors, &notes);
        assert_eq!(g, vec![0, 1]);
        assert_eq!(e, vec![3, 20]);
    }

    #[test]
    fn filter_judged_full_play_is_identity() {
        let grades = [0u8, 1, 6];
        let errors = [1i16, 2, 3];
        let notes = [note(true, 0), note(true, 0), note(true, 0)];
        let (g, e) = filter_judged(&grades, &errors, &notes);
        assert_eq!(g, grades.to_vec());
        assert_eq!(e, errors.to_vec());
    }

    #[test]
    fn split_pure_seconds_mirrors_the_stock_post_pass() {
        // Second 0: violet only ⇒ pure (gradient). Second 1: violet + a
        // loose Marvelous left in the stock series ⇒ mixed (flat). Second
        // 2: violet + a Perfect ⇒ mixed. Second 3: no violet ⇒ neither.
        // Second 4: violet + unjudged filler ⇒ mixed (stock counts the
        // filler in its purity test too).
        let violet = [3.0, 2.0, 1.0, 0.0, 4.0];
        let filler = [0.0, 0.0, 0.0, 0.0, 1.0];
        let miss = [0.0; 5];
        let good = [0.0; 5];
        let great = [0.0; 5];
        let perfect = [0.0, 0.0, 2.0, 5.0, 0.0];
        let marvelous = [0.0, 1.0, 0.0, 0.0, 0.0];
        let (pure, mixed) = split_pure_seconds(
            &violet,
            &[&filler, &miss, &good, &great, &perfect, &marvelous],
        );
        assert_eq!(pure, vec![3.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(mixed, vec![0.0, 2.0, 1.0, 0.0, 4.0]);
        // Partition: pure + mixed == violet everywhere.
        for s in 0..violet.len() {
            assert_eq!(pure[s] + mixed[s], violet[s]);
        }
    }

    #[test]
    fn split_pure_seconds_short_series_count_as_zero_and_empty_is_empty() {
        // A stock series shorter than ours is treated as zero past its end
        // (defensive — the ingest resizes all judge series together).
        let violet = [1.0, 2.0];
        let short = [0.0];
        let (pure, mixed) = split_pure_seconds(&violet, &[&short]);
        assert_eq!((pure, mixed), (vec![1.0, 2.0], vec![0.0, 0.0]));
        let (pure, mixed) = split_pure_seconds(&[], &[&short]);
        assert!(pure.is_empty() && mixed.is_empty());
        // No other series at all ⇒ everything is pure.
        let (pure, mixed) = split_pure_seconds(&violet, &[]);
        assert_eq!((pure, mixed), (vec![1.0, 2.0], vec![0.0, 0.0]));
    }
}
