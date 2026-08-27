//! Concrete NoteType implementation for ITG-style mines.
//!
//! Parses the MINE_DATA chunk (kind=20, param2=<difficulty code>) per
//! `docs/ssq_mine_chunk_format.md`, converts each mine's `beat_count` to
//! `music_count` via the supplied TempoConverter, expands multi-bit panel
//! masks into one single-bit Note entry per panel, and appends them all to
//! the game's Notes vector in a single bulk-append.
//!
//! Per-difficulty: the chunk lookup uses the same `(type, param2)` scheme
//! as vanilla step chunks — `param2` is the difficulty+style code from
//! `docs/ssq_format.md §5.1` (e.g. `0x0318` = Double Expert). The DLL's
//! Analyze hook runs per-difficulty and invokes this method with the code
//! for the difficulty being parsed.
//!
//! Also populates a mod-owned `Vec<MineEntry>` sidecar sorted by
//! `music_count`. The sidecar is the source of truth for mine state
//! (consumed / pending) and drives the post-judge mine-hit detection in
//! `on_judge_tick`.

use crate::core::memory;
use crate::log_warn;
use crate::mods::note_types_expansion::hooks::{
    combo_offset, is_dead_offset, judge_counts_ok_offset, judge_submit_fn, max_combo_offset,
    shock_arrow_num_offset, JudgeSubmitFn,
};
use crate::mods::note_types_expansion::note_type::{NoteType, NoteTypeError, RenderBinding};
use crate::mods::note_types_expansion::notes_vec::GameNotesVec;
use crate::mods::note_types_expansion::ssq_chunk::find_chunk;
use crate::mods::note_types_expansion::timing::TempoConverter;
use crate::types::game_note::{
    actor_results_range, for_each_result, kind, result, state, GameNote,
};

/// SSQ MINE_DATA chunk kind — see docs/ssq_mine_chunk_format.md.
/// The `param2` is per-difficulty and supplied at call time.
const MINE_CHUNK_KIND: u16 = 20;
const MINE_ENTRY_STRIDE: usize = 8;

/// Judge code the engine uses for "shock arrow hit" — breaks combo,
/// displays NG, fires the 0x1032 event that drives life gauge damage.
/// Mines reuse this path verbatim so their penalty matches what the
/// engine already does for shock arrows.
const JUDGE_CODE_SHOCK_NG: u32 = 0x1031;
/// Grade value that pairs with `JUDGE_CODE_SHOCK_NG` — the shock-hit
/// Results are written with grade 7 (NG) before the call.
const GRADE_NG: u32 = 0x7;

/// Actor-layout offsets used to build the shock-NG scratch struct.
/// Observed via Ghidra in the judge's shock-hit site.
const ACTOR_OFFSET_PLAYER_INDEX: usize = 0x84;
const ACTOR_OFFSET_FRAME_COUNTER: usize = 0x188;
const ACTOR_OFFSET_JUDGE_SUPPRESSED: usize = 0x1E9;

/// Scratch struct the engine's judgment-submit helper reads on the
/// shock-hit path. Layout confirmed from Ghidra on the judge's shock-hit
/// call site: 0x1C bytes total starting from the scratch pointer passed
/// in R9.
#[repr(C)]
struct ShockScratch {
    /// `actor->0x84` — player index (0 = P1, 1 = P2).
    player_index: u32,
    /// Padding dword. Judge leaves this as zero.
    _pad04: u32,
    /// Bitmask of panels involved in this judgment (bit N = panel N).
    /// For a single-panel mine, exactly one bit is set.
    panel_bitmask: u32,
    /// Padding dword. Judge leaves this as zero.
    _pad0c: u32,
    /// Pointer to the underlying per-note record (i.e. the note-pointer
    /// stored at `result+0x00`).
    note_ptr: *const GameNote,
    /// `actor->0x188` — a per-frame counter the judge snapshots into
    /// the scratch. We mirror the same source field.
    frame_counter: u32,
}

/// One mine in the sidecar. Kept separately from the game's Note vector
/// because judgment wants `consumed` tracking and panel lookup without
/// mutating game-owned memory.
#[derive(Clone, Copy, Debug)]
pub struct MineEntry {
    pub music_count: i32,
    /// One-hot panel bitmask (exactly one bit set). Multi-panel chunk entries
    /// are expanded into multiple MineEntry records, one per set bit.
    pub panel: u8,
    /// True once a hit or miss has been resolved for this entry in the
    /// current chart. Reset between charts.
    pub consumed: bool,
}

pub struct MineNoteType {
    /// Sorted by `music_count` (ascending) at the end of on_chart_loaded.
    /// Per-frame judgment uses binary search on music_count to find the
    /// newly-crossed range.
    mines: Vec<MineEntry>,
    /// Music_count from the previous `on_judge_tick` call. Used with
    /// the current tick to compute the `(prev, current]` range of mines
    /// whose ticks were just crossed this frame. Initialized to
    /// `i32::MIN` so the first frame's range captures any mine whose
    /// tick is at or before the first frame's music_count.
    prev_music_count: i32,
}

impl MineNoteType {
    pub fn new() -> Self {
        Self {
            mines: Vec::new(),
            prev_music_count: i32::MIN,
        }
    }

    /// Borrow the sidecar entries. Judge-side code uses this in later tasks.
    pub fn entries(&self) -> &[MineEntry] {
        &self.mines
    }
}

impl NoteType for MineNoteType {
    fn id(&self) -> &'static str {
        "mines"
    }
    fn note_kind(&self) -> i8 {
        kind::MINE
    }

    fn on_chart_loaded(
        &mut self,
        ssq_blob: &[u8],
        tempo: &TempoConverter,
        notes_vec: &mut GameNotesVec,
        difficulty_code: u16,
    ) -> Result<usize, NoteTypeError> {
        // Fresh chart (or fresh difficulty load): any residual sidecar from
        // the previous chart is cleared. (reset() normally handles this on
        // scene exit; clearing again here is defense in depth for the case
        // where reset didn't fire — e.g. direct song-to-song transitions
        // without attract.)
        self.mines.clear();
        self.prev_music_count = i32::MIN;

        let chunk = match find_chunk(ssq_blob, MINE_CHUNK_KIND, difficulty_code) {
            Some(c) => c,
            None => return Ok(0), // no mines on this difficulty -- not an error
        };

        let declared_count = chunk.param3 as usize;
        let actual_body_count = chunk.body.len() / MINE_ENTRY_STRIDE;
        if chunk.body.len() % MINE_ENTRY_STRIDE != 0 || declared_count != actual_body_count {
            return Err(NoteTypeError::MalformedChunk(
                "chunk body length inconsistent with declared entry count",
            ));
        }

        // Build synthetic notes + sidecar entries together in one pass.
        // A multi-bit `panels` byte expands into N single-bit notes (the
        // renderer expects one render entry per set state bit, per the
        // spec's §3.2 shock-classifier constraint).
        let mut notes_buf: Vec<GameNote> = Vec::with_capacity(declared_count);
        let mut skipped_no_panels = 0usize;
        let mut skipped_shock_shape = 0usize;
        let mut skipped_nonzero_reserved = 0usize;
        let mut skipped_negative_tick = 0usize;

        for i in 0..declared_count {
            let entry_off = i * MINE_ENTRY_STRIDE;
            let beat_count = read_i32_le(chunk.body, entry_off);
            let panels = chunk.body[entry_off + 4];
            let flags = chunk.body[entry_off + 5];
            let reserved =
                u16::from_le_bytes([chunk.body[entry_off + 6], chunk.body[entry_off + 7]]);

            // Per-entry validation per docs/ssq_mine_chunk_format.md §3 and §4.
            if panels == 0 {
                skipped_no_panels += 1;
                continue;
            }
            if panels == 0xFF || panels == 0x0F || panels == 0xF0 {
                // Shock-arrow encodings in the step byte would trigger the
                // renderer's shock classifier if they reached the Notes
                // vector. Refuse.
                skipped_shock_shape += 1;
                continue;
            }
            if flags != 0 || reserved != 0 {
                skipped_nonzero_reserved += 1;
                continue;
            }
            if beat_count < 0 {
                skipped_negative_tick += 1;
                continue;
            }

            let music_count = tempo.beat_to_music_count(beat_count);

            // Expand to one single-bit entry per set panel.
            for bit in 0..8u8 {
                let mask = 1u8 << bit;
                if (panels & mask) == 0 {
                    continue;
                }
                notes_buf.push(GameNote::mine(beat_count, music_count, mask));
                self.mines.push(MineEntry {
                    music_count,
                    panel: mask,
                    consumed: false,
                });
            }
        }

        if skipped_no_panels > 0
            || skipped_shock_shape > 0
            || skipped_nonzero_reserved > 0
            || skipped_negative_tick > 0
        {
            log_warn!(
                "MineNoteType: skipped malformed entries (no_panels={}, shock_shape={}, reserved_nonzero={}, negative_tick={})",
                skipped_no_panels, skipped_shock_shape,
                skipped_nonzero_reserved, skipped_negative_tick,
            );
        }

        if notes_buf.is_empty() {
            return Ok(0);
        }

        // Keep sidecar sorted by music_count (panel as tiebreaker for
        // determinism). Judgment uses binary search on music_count.
        self.mines.sort_by(|a, b| {
            a.music_count
                .cmp(&b.music_count)
                .then(a.panel.cmp(&b.panel))
        });

        if let Err(e) = notes_vec.append_bulk(&notes_buf) {
            log_warn!(
                "MineNoteType: append_bulk failed: {:?} -- rolling back sidecar",
                e
            );
            self.mines.clear();
            return Err(NoteTypeError::InjectionFailed(
                "app-heap allocation for extended Notes vector failed",
            ));
        }

        Ok(notes_buf.len())
    }

    fn on_judge_tick(&mut self, actor: *mut u8, music_count: i32, foot_panel: *mut u8) {
        if self.mines.is_empty() {
            return;
        }
        let Some(submit) = judge_submit_fn() else {
            // Signature not resolved; mod enable should have already
            // warned. Silent no-op here — re-logging every frame would
            // flood log.txt.
            return;
        };
        if actor.is_null() || foot_panel.is_null() {
            return;
        }

        // Single-frame resolution at playhead crossing — matches
        // native shock-arrow semantics. Each mine is processed
        // exactly once, on the frame its tick is crossed (i.e.
        // `prev_music_count < mine.music_count <= music_count`). Hit
        // or avoided, it's consumed that frame.
        //
        // Benefits over a windowed approach:
        //   * Mines past the chart cutoff (bad chart data: note ticks
        //     past audio end) are never reached → never processed →
        //     invisible to score/combo. Transparent handling.
        //   * No "crediting drift": each mine's denominator and
        //     numerator contributions happen in the same frame.
        //   * Matches native shock's `m_lastMusicCount < note.mc <=
        //     musicCount` check — same hit-window semantics as the
        //     engine already uses for shock arrows.
        //
        // The `music_count <= prev_music_count` guard skips paused
        // frames (music_count stays put) and protects against
        // out-of-order or same-frame repeated calls.
        if music_count <= self.prev_music_count {
            return;
        }
        let prev_mc = self.prev_music_count;
        self.prev_music_count = music_count;

        // Binary-search the sidecar for the (prev_mc, music_count]
        // range: mines whose tick falls in that interval are
        // newly-crossed this frame.
        let start_idx = self.mines.partition_point(|m| m.music_count <= prev_mc);
        let end_idx = self.mines.partition_point(|m| m.music_count <= music_count);

        // Cache field offsets once per call — each accessor does an
        // atomic load.
        let shock_off = shock_arrow_num_offset();
        let ok_off = judge_counts_ok_offset();
        let combo_off = combo_offset();
        let max_combo_off = max_combo_offset();
        let dead_off = is_dead_offset();

        for idx in start_idx..end_idx {
            let mine = self.mines[idx];
            // Defensive: shouldn't happen under normal flow (range
            // query exits newly-crossed mines only), but guards
            // against any future caller re-entering this range.
            if mine.consumed {
                continue;
            }
            let panel_idx = match mine.panel.trailing_zeros() {
                n if n < 8 => n as i32,
                _ => {
                    self.mines[idx].consumed = true;
                    continue;
                }
            };

            // Query the current foot panel for "was this panel just
            // pressed?" via vtable[2]. In autoplay mode this returns
            // false (our post-judge callback runs after autoplay has
            // restored the original user foot panel, which reflects
            // actual hardware state, not autoplay's scripted presses).
            let pressed = unsafe { was_just_pressed(foot_panel, panel_idx) };
            // Arrow-takes-priority rule (US-3): if a non-mine Note at
            // the same (music_count, panel) has been judged, the
            // arrow absorbed the press. Treat the mine as avoided
            // (the player didn't step ON THE MINE as a mine — the
            // step was absorbed by the arrow) — so it contributes
            // to combo + score like any other avoided mine.
            let arrow_priority =
                unsafe { arrow_hit_at(actor, mine.music_count, panel_idx as usize) };

            // Always-on: bump the shock-arrow-count denominator by 1
            // for this mine. Whether hit or avoided, the mine now
            // participates in the score formula's total note count
            // AND the fullcombo threshold. Mines unreachable past
            // chart cutoff never get this bump (they're never
            // processed), so the chart's score/combo ceiling
            // naturally collapses to the played portion only.
            unsafe {
                if let Some(off) = shock_off {
                    let slot = actor.add(off);
                    let cur = memory::read_i32(slot);
                    memory::write_i32(slot, cur + 1);
                }
            }

            if pressed && !arrow_priority {
                // HIT path: the player pressed the mine's panel on
                // the frame its tick was crossed, and no arrow
                // absorbed the press. Submit via the engine's
                // shock-NG handler — increments NG count + breaks
                // combo through the engine's normal flow.
                let result_entry =
                    unsafe { find_mine_result_entry(actor, mine.music_count, mine.panel) };
                if let Some(result_entry) = result_entry {
                    unsafe {
                        memory::write_i32(
                            result_entry.add(result::OFFSET_JUDGE_TIMESTAMP),
                            music_count,
                        );
                        memory::write_u32(result_entry.add(result::OFFSET_GRADE), GRADE_NG);
                        let suppressed = memory::read_u8(actor.add(0x1E8));
                        memory::write_u8(
                            result_entry.add(result::OFFSET_VISIBLE),
                            (suppressed == 0) as u8,
                        );

                        // Synthesize a note record whose per-panel state
                        // marks every panel on the active player's side
                        // as triggered. The shock-hit message listener
                        // that drives the full-lane visual effect gates
                        // on the leftmost panel of each side being
                        // triggered (`state[NumPanels * sideIndex]`);
                        // real shock arrows satisfy this by setting all
                        // four per-side panels, while our single-panel
                        // mines only set the one struck panel. Without
                        // this synthetic override the lane effect would
                        // only fire when the mine happened to be on the
                        // leftmost panel. The beat-count field is also
                        // copied through because the talent-measurement
                        // listener reads it. Stack-allocated and safe
                        // for the duration of the submit call — the
                        // engine's message dispatch is synchronous, so
                        // every listener has consumed the pointer by
                        // the time the submit returns.
                        let real_note_ptr =
                            *(result_entry.add(result::OFFSET_NOTE_PTR) as *const *const GameNote);
                        let player_index = memory::read_u32(actor.add(ACTOR_OFFSET_PLAYER_INDEX));
                        let synthetic_note =
                            build_shock_lane_trigger_note(real_note_ptr, player_index);

                        let mut scratch = ShockScratch {
                            player_index,
                            _pad04: 0,
                            panel_bitmask: 1u32 << panel_idx,
                            _pad0c: 0,
                            note_ptr: &synthetic_note as *const GameNote,
                            frame_counter: memory::read_u32(actor.add(ACTOR_OFFSET_FRAME_COUNTER)),
                        };

                        // Respect the same suppression gate the
                        // engine does: if actor->0x1e9 is non-zero,
                        // the shock-hit path skips the submit call
                        // (purely a display/score suppression for
                        // test/demo modes).
                        if memory::read_u8(actor.add(ACTOR_OFFSET_JUDGE_SUPPRESSED)) == 0 {
                            submit_mine_hit(
                                submit,
                                actor,
                                result_entry,
                                JUDGE_CODE_SHOCK_NG,
                                &mut scratch,
                            );
                        }
                        // Keep the synthetic note alive across the
                        // submit call. Compiler sees the final read
                        // and keeps the storage rooted.
                        let _ = synthetic_note.beat_count;
                    }
                }
                // No OK/combo credits on the hit path — the engine's
                // shock-NG submit path increments the NG slot and
                // zeros the combo counter through its normal flow.
            } else {
                // AVOIDED path (includes arrow-priority case).
                // Mirrors how native shock arrows resolve via the
                // engine's auto-expire branch: grade=OK, the post-
                // judge handler increments judgment-count[OK] and
                // combo. We write those fields directly since the
                // pre-mark has taken the entry out of the engine's
                // judge loop.
                //
                // Fields bumped:
                //   * judgment-count array's OK slot — numerator
                //     partner to the denominator bump above. Gated
                //     on `!dead` to match the engine's post-judge
                //     handler behavior.
                //   * combo counter — matches native shock's combo
                //     advance on an avoided-shock judgment. NOT
                //     gated on dead (engine also advances combo
                //     when dead).
                //   * max-combo counter — updated if combo exceeds
                //     its prior peak.
                //
                // If any offset wasn't detected at install time,
                // the corresponding bump is silently skipped,
                // preserving graceful degradation.
                unsafe {
                    let dead = match dead_off {
                        Some(o) => memory::read_u8(actor.add(o)) != 0,
                        None => false,
                    };
                    if !dead {
                        if let Some(off) = ok_off {
                            let slot = actor.add(off);
                            let cur = memory::read_i32(slot);
                            memory::write_i32(slot, cur + 1);
                        }
                    }
                    if let (Some(c_off), Some(mc_off)) = (combo_off, max_combo_off) {
                        let combo_slot = actor.add(c_off);
                        let max_slot = actor.add(mc_off);
                        let new_combo = memory::read_i32(combo_slot) + 1;
                        memory::write_i32(combo_slot, new_combo);
                        let cur_max = memory::read_i32(max_slot);
                        if new_combo > cur_max {
                            memory::write_i32(max_slot, new_combo);
                        }
                    }
                }
            }

            self.mines[idx].consumed = true;
        }
    }

    fn render_binding(&self) -> RenderBinding {
        // Render hook currently rewrites mine notes to ARROW before the
        // game's collector runs, so mines emit as regular arrow quads
        // and pick up the arrow atlas. Texture-name and UV here are
        // reserved for a future render path that binds a dedicated
        // mine sprite.
        RenderBinding {
            texture_name: "note_types_mine00",
            uv: [0.0, 0.0, 1.0, 1.0],
        }
    }

    fn reset(&mut self) -> bool {
        let had_state = !self.mines.is_empty();
        self.mines.clear();
        self.prev_music_count = i32::MIN;
        had_state
    }
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

// ── Judge-time helpers ──────────────────────────────────────────────

/// Invoke `IFootPanel::wasJustPressed(panel_idx)` on `foot_panel` via
/// its vtable slot (index 2). The vtable layout is shared between
/// `UserFootPanel` and `AutoFootPanel`; both return non-zero when the
/// panel was pressed this frame. Safe to call with either concrete
/// type as long as the pointer is a valid IFootPanel.
unsafe fn was_just_pressed(foot_panel: *mut u8, panel_idx: i32) -> bool {
    if foot_panel.is_null() {
        return false;
    }
    let vtable = *(foot_panel as *const *const *const u8);
    if vtable.is_null() {
        return false;
    }
    // vtable[2] = wasJustPressed(this, panel_idx) -> u8
    let slot = vtable.add(2);
    let func: unsafe extern "C" fn(*mut u8, i32) -> u8 = std::mem::transmute(*slot);
    func(foot_panel, panel_idx) != 0
}

/// Scan the actor's Results vector for the entry whose underlying
/// `GameNote` matches `(music_count, panel_bits)` and has
/// `kind == MINE`. Returns the raw Result pointer on match.
unsafe fn find_mine_result_entry(
    actor: *mut u8,
    music_count: i32,
    panel_bits: u8,
) -> Option<*mut u8> {
    let (begin, end) = actor_results_range(actor);
    let mut found: Option<*mut u8> = None;
    for_each_result(begin, end, |entry, note| {
        if found.is_some() {
            return;
        }
        let n = &*note;
        if n.kind != kind::MINE || n.music_count != music_count {
            return;
        }
        // `GameNote::state[panel_idx] == TRG` (1) identifies the
        // panel(s) this mine covers. The bitmask expansion in
        // on_chart_loaded produces one Note per bit, so a mine Note
        // has exactly one state[] entry set to TRG.
        for bit in 0..8u8 {
            if (panel_bits & (1 << bit)) != 0 && n.state[bit as usize] == 1 {
                found = Some(entry);
                return;
            }
        }
    });
    found
}

/// Returns `true` if the Results vector contains a non-mine entry at
/// `music_count` covering `panel_idx` that has already been judged
/// (i.e. its judge-timestamp is non-negative). Implements the
/// arrow-takes-priority rule: when a mine and arrow share a
/// tick+panel and the player presses, the arrow absorbs the press.
unsafe fn arrow_hit_at(actor: *mut u8, music_count: i32, panel_idx: usize) -> bool {
    if panel_idx >= 8 {
        return false;
    }
    let (begin, end) = actor_results_range(actor);
    let mut hit = false;
    for_each_result(begin, end, |entry, note| {
        if hit {
            return;
        }
        let n = &*note;
        if n.kind == kind::MINE || n.music_count != music_count {
            return;
        }
        if n.state[panel_idx] != 1 {
            return;
        }
        // Non-mine, same tick, same panel, state = TRG. If it's been
        // judged this chart (judge-timestamp >= 0), the mine defers.
        let timestamp = memory::read_i32(entry.add(result::OFFSET_JUDGE_TIMESTAMP));
        if timestamp >= 0 {
            hit = true;
        }
    });
    hit
}

/// Thin wrapper that hides the raw function-pointer call behind a
/// named helper for readability at the call site. `scratch` is passed
/// by mutable reference so the caller's local scratch struct's
/// lifetime bounds the pointer's validity.
unsafe fn submit_mine_hit(
    submit: JudgeSubmitFn,
    actor: *mut u8,
    result_entry: *mut u8,
    judge_code: u32,
    scratch: &mut ShockScratch,
) {
    submit(
        actor,
        result_entry,
        judge_code,
        scratch as *mut ShockScratch as *mut u8,
    );
}

/// Build a synthetic note record whose per-panel state marks every
/// panel on the active player's side as triggered. The shock-hit
/// message listener that drives the full-lane visual gates on
/// `note.state[NumPanels * sideIndex]` being in the triggered state —
/// a check that real shock arrows satisfy trivially (all four
/// per-side panels set) but that single-panel mines cannot satisfy
/// unless they happen to sit on the leftmost panel of the side.
///
/// Returned by value, stack-allocated at the call site. The engine's
/// post-judge message dispatch is synchronous, so the pointer into
/// this storage that we hand the shock-hit submit remains valid for
/// every listener that consumes the message. Only two fields on the
/// note pointer are read anywhere in the shock-hit listener chain —
/// the per-panel state array and the beat-count — so the synthetic
/// mirrors those and leaves everything else zero.
unsafe fn build_shock_lane_trigger_note(
    real_note_ptr: *const GameNote,
    player_index: u32,
) -> GameNote {
    let beat_count = if real_note_ptr.is_null() {
        0
    } else {
        (*real_note_ptr).beat_count
    };
    // Zero-initialized GameNote, then fill state bits for the active
    // player's side. `GameNote: Copy` allows the zeroed construction
    // via a mem::zeroed cast — safe because the struct is #[repr(C)]
    // with all POD fields.
    let mut note: GameNote = std::mem::zeroed();
    note.kind = kind::ARROW;
    note.beat_count = beat_count;
    let side_base = (player_index as usize).min(1) * 4;
    for slot in 0..4 {
        note.state[side_base + slot] = state::TRG;
    }
    note
}
