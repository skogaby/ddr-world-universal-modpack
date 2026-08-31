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
//! | +0xD8..0xE0 | `vector<i16>` signed ms error per judged note |
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

/// Pure core: per-second S-Marvelous counts, mirroring the graph ingest's
/// bucketing exactly so our vector's buckets align 1:1 with the game's
/// judge series — `t_first` = the first JUDGED note's timestamp, bucket =
/// `(t − t_first) / 1000`, one stream slot per entry, judged-only.
///
/// `None` when the streams disagree in length; an empty/never-judged
/// record yields an empty vector (nothing to draw — matches the tab's
/// has-data gate).
pub fn smarv_per_second(
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
        if grades[idx] == GRADE_MARVELOUS && (errors_ms[idx] as i32).abs() <= window_ms {
            let bucket = ((note.t_ms - t_first) / 1000) as usize;
            if bucket >= out.len() {
                out.resize(bucket + 1, 0.0);
            }
            out[bucket] += 1.0;
        }
    }
    Some(out)
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
        let v = smarv_per_second(&notes, &grades, &errors, 12).unwrap();
        assert_eq!(v, vec![2.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn per_second_counts_marvelous_grades_only() {
        let notes = [note(true, 0), note(true, 100), note(true, 200)];
        let grades = [1u8, 0, 6];
        let errors = [0i16, 0, 0];
        let v = smarv_per_second(&notes, &grades, &errors, 12).unwrap();
        assert_eq!(v, vec![1.0]);
    }

    #[test]
    fn per_second_never_judged_is_empty() {
        let notes = [note(false, 0), note(false, 500)];
        let grades = [0u8, 0];
        let errors = [0i16, 0];
        assert_eq!(
            smarv_per_second(&notes, &grades, &errors, 12).unwrap(),
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
        let v = smarv_per_second(&notes, &grades, &errors, 12).unwrap();
        assert_eq!(v, vec![1.0, 1.0]);
    }

    #[test]
    fn per_second_rejects_stream_length_mismatch() {
        let notes = [note(true, 0)];
        assert!(smarv_per_second(&notes, &[0u8, 0], &[0i16], 12).is_none());
        assert!(smarv_per_second(&notes, &[0u8], &[0i16], 0).is_none());
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
}
