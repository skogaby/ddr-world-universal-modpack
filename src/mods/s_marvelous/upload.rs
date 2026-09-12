//! S-Marvelous score-upload payload (server-upload design §4.3, §5.1):
//! the S-Marvelous-aware duplicates of the stock `<result>` fields that a
//! capable backend stores beside the stock ones. The stock packet is never
//! touched — this module only COMPUTES what goes into the separate
//! `/data/s_marv` node; the persistence service materialises it.
//!
//! Everything here is a pure function of the stage record's per-note
//! streams ([`super::records::RawStreams`]) plus the record's own stock
//! counters and the window the side was armed with — the exact inputs the
//! results screen uses, so packet == results screen by construction.
//!
//! Wire contract (leaf order is load-bearing only for readability — every
//! consumer reads by name):
//!
//! | leaf | source |
//! |---|---|
//! | `mcode`, `style`, `difficulty` | record identity echo (`rec+0x00/+0x08/+0x04`) |
//! | `window_ms` | the armed S-Marvelous window |
//! | `judge_smarv` | S-Marvelous count |
//! | `judge_marv` | EXCLUSIVE Marvelous = stock Marvelous − S-Marvelous |
//! | `fastcount` / `slowcount` | stock `+0x6C/+0x70` + the loose-Marvelous share |
//! | `clearkind` | wire clear kind (`rec+0x270`), or [`CLEAR_KIND_SMFC`] |
//! | `ghostsize` / `ghost` | stock ghost (every stream slot, `'0'+grade`) with [`GHOST_SMARV_CHAR`] at S-Marv slots |
//!
//! Invariants (host-tested): `judge_smarv + judge_marv == stock marv`;
//! `judge_smarv + (fastcount − stock fast) + (slowcount − stock slow) ==
//! stock marv`; `ghost.len() == stream length`; `'8'` appears only where the
//! stock ghost has `'0'`.
//!
//! Std-only (no `crate::` imports) so `scripts/validate_s_marvelous.sh` can
//! mount it beside `records.rs`.

use super::records::{count_grade, count_marv_fast_slow, count_smarv, RawStreams, GRADE_MARVELOUS};

/// Wire clear kind for a Marvelous Full Combo (bemaniutils
/// `GAME_HALO_MARVELOUS_COMBO`; the game marshals `rec+0x270`).
pub const CLEAR_KIND_MFC: i32 = 10;
/// The new top tier this mod adds ON THE WIRE (inside `s_marv` only): every
/// judged Marvelous was an S-Marvelous. Extends the stock enum
/// (6 none, 7 FC, 8 GFC, 9 PFC, 10 MFC) without collision.
pub const CLEAR_KIND_SMFC: i32 = 11;
/// Ghost designator for an S-Marvelous step. The stock alphabet is
/// `'0' + grade_class` for classes 0..=7 (`GetGhostData` decodes
/// `c − 0x30`), so `'8'` is one past it. It never reaches a stock decoder:
/// the stock `<ghost>` is untouched and the server never serves this one.
pub const GHOST_SMARV_CHAR: u8 = b'8';

/// Everything the builder reads off the stage record (design §4.3).
#[derive(Clone, Debug)]
pub struct RecordInputs<'a> {
    pub mcode: i32,
    pub style: i32,
    pub difficulty: i32,
    /// `rec+0x28` — the stock Marvelous counter (judged notes only).
    pub stock_marv: i32,
    /// `rec+0x6C` / `rec+0x70` — the stock FAST/SLOW counters (grades 1..=4).
    pub stock_fast: i32,
    pub stock_slow: i32,
    /// `rec+0x270` — the clear kind the marshal puts on the wire.
    pub wire_clearkind: i32,
    pub streams: &'a RawStreams,
}

/// The computed node contents, in wire order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SMarvPayload {
    pub mcode: i32,
    pub style: i32,
    pub difficulty: i32,
    pub window_ms: i32,
    pub judge_smarv: i32,
    pub judge_marv: i32,
    pub fastcount: i32,
    pub slowcount: i32,
    pub clearkind: i32,
    pub ghost: String,
}

impl SMarvPayload {
    /// `ghostsize` — the stock field is the ghost's length, so ours is too.
    pub fn ghostsize(&self) -> i32 {
        self.ghost.len() as i32
    }

    /// Chart index in the backend's convention (`style == 0 ? difficulty :
    /// difficulty + 5`) — the key the S-MFC lamp set uses.
    pub fn chart(&self) -> i32 {
        chart_index(self.style, self.difficulty)
    }

    /// True when this play is an S-MFC (the only case a lamp cares about).
    pub fn is_smfc(&self) -> bool {
        self.clearkind == CLEAR_KIND_SMFC
    }
}

/// A leaf of the `/data/s_marv` node — the shape the persistence service's
/// node builder consumes (kept std-only here; the service re-exports it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Leaf {
    S32(&'static str, i32),
    Str(&'static str, String),
}

/// `chart = style == 0 ? difficulty : difficulty + 5` (bemani-buddy /
/// bemaniutils convention: 0..4 single, 5..9 double).
pub fn chart_index(style: i32, difficulty: i32) -> i32 {
    if style == 0 {
        difficulty
    } else {
        difficulty + 5
    }
}

/// Build the payload, or `None` when the record cannot be trusted (design
/// §4.3 rules):
///
/// - the streams must be parallel, and the JUDGED slots' Marvelous count
///   must equal `stock_marv` (the same consistency gate the results tab
///   applies — a disagreement means the assumed layout drifted);
/// - `window_ms` must be positive;
/// - the stock counters must be non-negative.
pub fn build_payload(inp: &RecordInputs<'_>, window_ms: i32) -> Option<SMarvPayload> {
    let s = inp.streams;
    if s.grades.len() != s.ms.len() || s.judged.len() != s.grades.len() || window_ms <= 0 {
        return None;
    }
    if inp.stock_marv < 0 || inp.stock_fast < 0 || inp.stock_slow < 0 {
        return None;
    }
    let (grades, ms) = s.judged_only();
    if count_grade(&grades, GRADE_MARVELOUS) != inp.stock_marv as u32 {
        return None;
    }
    let judge_smarv = count_smarv(&grades, &ms, window_ms)? as i32;
    let (loose_fast, loose_slow) = count_marv_fast_slow(&grades, &ms, window_ms)?;
    let judge_marv = inp.stock_marv - judge_smarv;
    let clearkind = if inp.wire_clearkind == CLEAR_KIND_MFC
        && inp.stock_marv > 0
        && judge_smarv == inp.stock_marv
    {
        CLEAR_KIND_SMFC
    } else {
        inp.wire_clearkind
    };
    Some(SMarvPayload {
        mcode: inp.mcode,
        style: inp.style,
        difficulty: inp.difficulty,
        window_ms,
        judge_smarv,
        judge_marv,
        fastcount: inp.stock_fast + loose_fast as i32,
        slowcount: inp.stock_slow + loose_slow as i32,
        clearkind,
        ghost: overlay_ghost(s, window_ms),
    })
}

/// The stock ghost string (every stream slot, `'0' + grade`) with
/// [`GHOST_SMARV_CHAR`] substituted where the slot is judged, grade 0 and
/// inside the window. Unjudged slots keep the stock character even though
/// their `ms` reads 0 — that is exactly the mistake the judged mask exists
/// to prevent.
pub fn overlay_ghost(s: &RawStreams, window_ms: i32) -> String {
    let n = s.grades.len().min(s.ms.len()).min(s.judged.len());
    let mut out = String::with_capacity(n);
    for i in 0..n {
        let g = s.grades[i];
        let smarv = s.judged[i] && g == GRADE_MARVELOUS && (s.ms[i] as i32).abs() <= window_ms;
        out.push(if smarv {
            GHOST_SMARV_CHAR as char
        } else {
            b'0'.wrapping_add(g) as char
        });
    }
    out
}

/// The stock ghost for the same streams — what the marshal emits. Test
/// oracle and the "'8' only where stock has '0'" invariant's reference.
pub fn stock_ghost(s: &RawStreams) -> String {
    s.grades
        .iter()
        .map(|&g| b'0'.wrapping_add(g) as char)
        .collect()
}

/// The node's leaves in wire order (design §5.1).
pub fn to_leaves(p: &SMarvPayload) -> Vec<Leaf> {
    vec![
        Leaf::S32("mcode", p.mcode),
        Leaf::S32("style", p.style),
        Leaf::S32("difficulty", p.difficulty),
        Leaf::S32("window_ms", p.window_ms),
        Leaf::S32("judge_smarv", p.judge_smarv),
        Leaf::S32("judge_marv", p.judge_marv),
        Leaf::S32("fastcount", p.fastcount),
        Leaf::S32("slowcount", p.slowcount),
        Leaf::S32("clearkind", p.clearkind),
        Leaf::S32("ghostsize", p.ghostsize()),
        Leaf::Str("ghost", p.ghost.clone()),
    ]
}

/// The node name under `/data`.
pub const NODE_NAME: &str = "s_marv";

#[cfg(test)]
mod tests {
    use super::*;

    /// Build streams from `(grade, ms, judged)` triples.
    fn streams(slots: &[(u8, i16, bool)]) -> RawStreams {
        RawStreams {
            grades: slots.iter().map(|s| s.0).collect(),
            ms: slots.iter().map(|s| s.1).collect(),
            judged: slots.iter().map(|s| s.2).collect(),
        }
    }

    fn inputs<'a>(
        s: &'a RawStreams,
        stock_fast: i32,
        stock_slow: i32,
        wire_clearkind: i32,
    ) -> RecordInputs<'a> {
        let (g, _) = s.judged_only();
        RecordInputs {
            mcode: 38548,
            style: 0,
            difficulty: 3,
            stock_marv: count_grade(&g, GRADE_MARVELOUS) as i32,
            stock_fast,
            stock_slow,
            wire_clearkind,
            streams: s,
        }
    }

    /// The §5.1 invariants, asserted on every successful build.
    fn assert_invariants(inp: &RecordInputs<'_>, p: &SMarvPayload) {
        assert_eq!(
            p.judge_smarv + p.judge_marv,
            inp.stock_marv,
            "exclusive marv"
        );
        assert_eq!(
            p.judge_smarv + (p.fastcount - inp.stock_fast) + (p.slowcount - inp.stock_slow),
            inp.stock_marv,
            "smarv + loose fast + loose slow == stock marv"
        );
        let stock = stock_ghost(inp.streams);
        assert_eq!(p.ghost.len(), stock.len(), "ghost length == stream length");
        assert_eq!(p.ghostsize() as usize, p.ghost.len());
        for (ours, theirs) in p.ghost.bytes().zip(stock.bytes()) {
            if ours == GHOST_SMARV_CHAR {
                assert_eq!(theirs, b'0', "'8' only where stock has '0'");
            } else {
                assert_eq!(ours, theirs, "non-S-Marv slots are stock");
            }
        }
        assert_eq!(
            p.ghost.bytes().filter(|&b| b == GHOST_SMARV_CHAR).count() as i32,
            p.judge_smarv
        );
    }

    #[test]
    fn all_smarv_full_combo_is_smfc_11() {
        let s = streams(&[(0, 3, true), (0, -12, true), (0, 0, true), (6, 0, true)]);
        let inp = inputs(&s, 0, 0, CLEAR_KIND_MFC);
        let p = build_payload(&inp, 12).unwrap();
        assert_invariants(&inp, &p);
        assert_eq!(p.judge_smarv, 3);
        assert_eq!(p.judge_marv, 0);
        assert_eq!(p.clearkind, CLEAR_KIND_SMFC);
        assert!(p.is_smfc());
        assert_eq!(p.ghost, "8886");
        assert_eq!(p.ghostsize(), 4);
    }

    #[test]
    fn mfc_with_a_loose_marvelous_stays_10() {
        // |13| > 12 ⇒ loose Marvelous ⇒ not every Marvelous is S-Marv.
        let s = streams(&[(0, 3, true), (0, 13, true), (0, -2, true)]);
        let inp = inputs(&s, 0, 0, CLEAR_KIND_MFC);
        let p = build_payload(&inp, 12).unwrap();
        assert_invariants(&inp, &p);
        assert_eq!((p.judge_smarv, p.judge_marv), (2, 1));
        assert_eq!(p.clearkind, CLEAR_KIND_MFC);
        assert_eq!(p.ghost, "808");
    }

    #[test]
    fn loose_marvelous_splits_fast_slow_by_stream_sign() {
        // Stream sign: expected − actual ⇒ ms > 0 = FAST, ms < 0 = SLOW.
        // Stock counters already hold the grades-1..4 share (here 5/7).
        let s = streams(&[
            (0, 15, true),  // loose, FAST
            (0, -16, true), // loose, SLOW
            (0, 17, true),  // loose, FAST (stock Marvelous edge)
            (0, 12, true),  // S-Marv (inclusive edge)
            (1, 30, true),  // Perfect — already in stock fast
            (2, -50, true), // Great — already in stock slow
        ]);
        let inp = inputs(&s, 5, 7, 7);
        let p = build_payload(&inp, 12).unwrap();
        assert_invariants(&inp, &p);
        assert_eq!(p.judge_smarv, 1);
        assert_eq!(p.judge_marv, 3);
        assert_eq!(p.fastcount, 5 + 2);
        assert_eq!(p.slowcount, 7 + 1);
        assert_eq!(p.clearkind, 7, "non-MFC clear kinds pass through");
        assert_eq!(p.ghost, "000812");
    }

    #[test]
    fn unjudged_tail_slots_keep_stock_char_and_do_not_count() {
        // A quick-failed song: the last three slots were never judged and
        // sit at their grade-0 / ms-0 initial values. The marshal still
        // emits them as '0'; the overlay must not turn them into '8'.
        let s = streams(&[
            (0, 2, true),
            (1, 20, true),
            (0, 0, false),
            (0, 0, false),
            (0, 0, false),
        ]);
        let inp = inputs(&s, 1, 0, 1);
        assert_eq!(inp.stock_marv, 1, "counter counts judged only");
        let p = build_payload(&inp, 12).unwrap();
        assert_invariants(&inp, &p);
        assert_eq!(p.judge_smarv, 1);
        assert_eq!(p.ghost, "81000");
        assert_eq!(p.ghost.len(), stock_ghost(&s).len());
    }

    #[test]
    fn consistency_gate_refuses_counter_disagreement() {
        let s = streams(&[(0, 1, true), (0, 1, true)]);
        let mut inp = inputs(&s, 0, 0, CLEAR_KIND_MFC);
        inp.stock_marv = 3; // record says 3, judged stream says 2
        assert!(build_payload(&inp, 12).is_none());
    }

    #[test]
    fn structural_refusals() {
        let s = streams(&[(0, 1, true)]);
        let inp = inputs(&s, 0, 0, 6);
        assert!(build_payload(&inp, 0).is_none(), "non-positive window");
        let bad = RawStreams {
            grades: vec![0, 0],
            ms: vec![1],
            judged: vec![true, true],
        };
        let inp2 = RecordInputs {
            streams: &bad,
            ..inp.clone()
        };
        assert!(build_payload(&inp2, 12).is_none(), "stream length mismatch");
        let mut neg = inputs(&s, -1, 0, 6);
        neg.stock_fast = -1;
        assert!(build_payload(&neg, 12).is_none(), "negative stock counter");
    }

    #[test]
    fn zero_marvelous_is_legal_and_never_smfc() {
        let s = streams(&[(1, 5, true), (5, 0, true)]);
        let inp = inputs(&s, 1, 0, CLEAR_KIND_MFC); // wire says MFC (nonsense but harmless)
        let p = build_payload(&inp, 12).unwrap();
        assert_invariants(&inp, &p);
        assert_eq!((p.judge_smarv, p.judge_marv), (0, 0));
        assert_eq!(
            p.clearkind, CLEAR_KIND_MFC,
            "stock_marv == 0 never promotes"
        );
        assert_eq!(p.ghost, "15");
        // Empty streams: legal, empty ghost.
        let empty = streams(&[]);
        let inp = inputs(&empty, 0, 0, 1);
        let p = build_payload(&inp, 12).unwrap();
        assert_eq!(p.ghost, "");
        assert_eq!(p.ghostsize(), 0);
    }

    #[test]
    fn leaves_follow_the_wire_order_and_names() {
        let s = streams(&[(0, 1, true)]);
        let inp = inputs(&s, 0, 0, CLEAR_KIND_MFC);
        let p = build_payload(&inp, 12).unwrap();
        let names: Vec<&str> = to_leaves(&p)
            .iter()
            .map(|l| match l {
                Leaf::S32(n, _) | Leaf::Str(n, _) => *n,
            })
            .collect();
        assert_eq!(
            names,
            [
                "mcode",
                "style",
                "difficulty",
                "window_ms",
                "judge_smarv",
                "judge_marv",
                "fastcount",
                "slowcount",
                "clearkind",
                "ghostsize",
                "ghost"
            ]
        );
        assert_eq!(to_leaves(&p)[10], Leaf::Str("ghost", "8".to_string()));
        assert_eq!(to_leaves(&p)[9], Leaf::S32("ghostsize", 1));
        assert_eq!(NODE_NAME, "s_marv");
    }

    #[test]
    fn chart_index_matches_backend_convention() {
        assert_eq!(chart_index(0, 3), 3);
        assert_eq!(chart_index(1, 3), 8);
        let s = streams(&[]);
        let mut inp = inputs(&s, 0, 0, 1);
        inp.style = 1;
        inp.difficulty = 4;
        assert_eq!(build_payload(&inp, 12).unwrap().chart(), 9);
    }
}
