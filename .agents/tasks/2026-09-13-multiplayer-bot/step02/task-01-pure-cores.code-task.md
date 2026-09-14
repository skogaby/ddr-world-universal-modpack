# Task: Pure cores — `eligibility.rs`, `skill.rs`, `planner.rs`

## Description

Implement the three dependency-free modules of the Multiplayer Bot (design §4.4, §4.6, §4.7)
with their full host test suites, under a stub `src/mods/multiplayer_bot/mod.rs` that declares
them so the crate compiles. No engine wiring, no `Mod` impl, no `lib.rs` registration yet
(plan Step 3).

## Background

The bot's decisions are a pure function of (level, seed, note stream, music count). Keeping
that logic free of `crate::` imports lets it run under the repo's temp-crate `#[path]` harness
on the macOS ARM host (plain `cargo test` cannot compile `retour`). Two RE facts shape the
planner and are non-negotiable (design A.5, research `bot-controller-re.md` §3–§4):

1. The judge has NO window test at match time and NO causality check: a held panel is
   attributed to the EARLIEST unjudged note carrying that arrow; an event outside every window
   leaves the note unjudged and the press unconsumed (so it may match the NEXT note on that
   panel). ⇒ per-panel event times must strictly increase.
2. A note becomes a Miss only when `mc > note.mc + 160`. ⇒ a decided Miss must block the
   following same-panel note's event until `note.mc + 161` or the next press "rescues" the
   missed note as a Good/Boo.

Grade windows (inclusive, ms): Marvelous ±17, Perfect ±34, Great ±84, Good ±124, Boo ±160.

## Reference Documentation

**Required:**
- Design: `.agents/planning/2026-09-13-multiplayer-bot/design/detailed-design.md` — §4.4
  (eligibility), §4.6 (skill curves, RNG, `decide`, `grade_for_offset`), §4.7 (planner
  algorithm + the properties the tests pin), §7.1 (host test list), A.5 (judge algebra).

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-13-multiplayer-bot/research/bot-controller-re.md` §2 (the stock
  `update` the planner mirrors for freeze bodies and shocks), §3 (judge algebra), §4 (planner
  rules), §6 (difficulty-model geometry).
- `src/mods/two_player_bpl_mode/logic.rs` (pure-module + `GateInputs`/`Gate::Unavailable`
  convention the eligibility module follows).
- `src/services/foot_panel_swap/layout.rs` (`BotPanelFlags` — the planner's `PanelFlags` must be
  field-for-field convertible to it).
- `src/types/game_note.rs` (the `GameNote` fields `NoteView` is built from; `state` values
  `TRG = 1`, `REP = 4`, freeze body `>= 2`).

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements

1. `src/mods/multiplayer_bot/mod.rs` — for THIS task only: module docs + `pub mod eligibility;
   pub mod skill; pub mod planner;`. Declare `pub mod multiplayer_bot;` in `src/mods/mod.rs`
   (alphabetical). No `Mod` impl, no `lib.rs` change.
2. `eligibility.rs` (pure):
   ```rust
   pub struct Inputs { pub entered: [Option<bool>; 2], pub style: Option<i32>,
                       pub course_word: Option<u64>, pub event_mode: Option<i32>,
                       pub versus: Option<i32>, pub option_on: [bool; 2], pub level: [i32; 2] }
   pub enum Refusal { Unavailable(&'static str), NotExactlyOneEntered, NotSingle, Course,
                      EventMode, AlreadyVersus, OptionOff }
   pub struct Plan { pub human: usize, pub bot: usize, pub level: u8 }
   pub fn evaluate(i: &Inputs) -> Result<Plan, Refusal>;
   pub fn clamp_level(level: i32) -> u8;   // 1..=10
   ```
   Order: every `Unavailable` before ordinary refusals (a broken service is never mistaken for
   "just a 2P game"); then exactly-one-entered, single (`style == 0`), `course_word == 0`,
   `event_mode == 0`, `versus == 0`, `option_on[human]`. `human` = the entered side, `bot =
   1 − human`, `level = clamp_level(level[human])`.
3. `skill.rs` (pure), design §4.6 verbatim: `Curve { sigma_ms, p_miss }`, `curve(level)` with
   `SIGMA_L1_MS = 75.0`, `SIGMA_L10_MS = 5.0`, `P_MISS_L1 = 0.05`, `P_MISS_EXP = 1.5` as named
   `pub const`s (cabinet tunables); `Rng` = xorshift64\* seeded through splitmix64 (a zero seed
   must not produce a stuck generator), `next_f64` in [0, 1), `gaussian` (Marsaglia polar or
   Box–Muller, no cached-second-value statefulness needed but allowed); `Plan { Hit { d_ms },
   Miss }`; `decide(rng, curve)` = `Miss` if `u < p_miss`, else `d = round(σ·z)`, `|d| > 160 ⇒
   Miss`; `grade_for_offset(d_ms) -> u8` by the inclusive windows, 5 beyond ±160; `pub fn
   seed(qpc: u64, mcode: i32, difficulty: i32, level: u8) -> u64` = `splitmix64(qpc ^ (mcode <<
   32) ^ (difficulty << 8) ^ level)`.
4. `planner.rs` (pure), design §4.7 verbatim: `NoteView { idx, music_count, beat_count, state:
   [i32; 8], length: [i32; 8], unjudged }`, `PanelFlags { is_held: [u8; 8], was_just_pressed:
   [u8; 8], event_mc: [i32; 8] }` (`#[repr(C)] Default`, field-identical to
   `foot_panel_swap::BotPanelFlags`), `SongState { plans: Vec<Option<skill::Plan>>, blocked_until:
   [i32; 8], last_event: [i32; 8], cursor: usize, tally: [u32; 6] }` with `SongState::new(capacity:
   usize)` (pre-sizes `plans` with `None`) and `SongState::tally(&self) -> [u32; 6]`; constants
   `LOOKAHEAD_MS = 8`, `MAX_EARLY_MS = 200`, `MISS_WINDOW_MS = 160`; `plan_frame(notes, st, rng,
   curve, mc, cur_beat, out)`. Algorithm exactly as §4.7 items 1–5, with these pinned details:
   - The walk starts at `st.cursor`, skips leading judged notes by advancing the cursor, and
     stops at the first note with `mc < music_count − LOOKAHEAD_MS − MAX_EARLY_MS`.
   - `plans` grows (with `None`) if a note's `idx >= plans.len()` — never index-panic.
   - Shock detection = the game's: all four panels of a pad (`state[0..4]` or `state[4..8]`)
     `== 1`. Arrow panels of a note = panels with `state ∈ {1, 4}`.
   - Freeze body (item 3) applies to EVERY note in the walk window (judged or not), as the
     stock `update` does.
   - For `Hit { d }`: `E = music_count + d`, then `E = max(E, blocked_until[p], last_event[p] + 1)`
     over the note's arrow panels; `E > music_count + MISS_WINDOW_MS ⇒` treat as Miss (rewrite
     `plans[idx] = Miss` so the decision is stable); else if `mc >= E − LOOKAHEAD_MS`: set
     `is_held/was_just_pressed = 1` and `event_mc = E` for every arrow panel, `last_event[p] = E`.
     The `last_event` update happens the FIRST frame the press is emitted and `E` is then fixed
     for the note (store the resolved `E` in the plan: `Plan::Hit { d_ms }` may become `Hit {
     d_ms: E − music_count }` on resolution — implementer's choice, but the emitted `E` must not
     change between frames).
   - `Miss`: for each arrow panel `blocked_until[p] = max(blocked_until[p], music_count +
     MISS_WINDOW_MS + 1)`.
   - Tally: the FIRST frame a note is observed `!unjudged` after having been seen unjudged (or
     having a plan), increment `tally[grade]` with `grade = grade_for_offset(E − music_count)`
     for a `Hit`, 5 for `Miss`. Track "already tallied" per note (a `bool` alongside the plan or a
     separate `Vec<bool>`).
5. Every module: `#![no_std]`-free but dependency-free (no `crate::`, no external crates, `std`
   only); `#[cfg(test)] mod tests`; doc comments cite the design section they implement.
6. The `--report` histogram: a `#[test] #[ignore] fn report_grade_histogram()` in `skill.rs` (or
   `planner.rs`) that, for each level 1..=10, drives `plan_frame` over a synthetic 500-note
   single-panel stream at 8 ms frame steps with a fixed seed and prints one line
   `L{n}: σ={:.1} p={:.3} marv={} perf={} great={} good={} boo={} miss={}` via `println!`.
   (Test-only code — the crate-wide no-`println!` rule is about runtime logging.)

## Dependencies

- None on engine code. `foot_panel_swap::BotPanelFlags` is referenced only by the field-parity
  test the harness task adds (task-02), not by these modules.

## Implementation Approach

1. Stub `mod.rs` + `src/mods/mod.rs` declaration; `cargo check` to confirm the crate still
   builds with empty modules.
2. `eligibility.rs` TDD: table tests first, then `evaluate`.
3. `skill.rs` TDD: window-boundary tests for `grade_for_offset`, monotonicity + determinism +
   1e6-sample endpoint tests (release-mode speed is fine; keep the sample loop tight), then the
   implementation.
4. `planner.rs` TDD: the single-note early/late/miss tests, the blocked-next-note test, jumps,
   freeze body, shocks, cursor/lookahead, then the 10k-song property test (small random
   streams, 1–4 panels, 50–200 ms spacing, random `d`), then the implementation.
5. Run through a temp-crate `#[path]` mount (the permanent harness is task-02 of this step —
   coordinate: task-02 mounts exactly these four files).
6. `cargo check --target x86_64-pc-windows-msvc` + `cargo fmt`.

## Acceptance Criteria

1. **Eligibility picks the empty side**
   - Given exactly one entered side (either), style 0, course 0, event 0, versus 0, option ON
     for the entered side, level 7
   - When `evaluate` runs
   - Then `Ok(Plan { human, bot: 1 − human, level: 7 })`.

2. **Eligibility refuses with the named reason**
   - Given each of: both entered / neither entered; style 1; course ≠ 0; event 1 or 2; versus 1;
     option OFF on the entered side
   - When `evaluate` runs
   - Then the corresponding `Refusal` variant; and any `None` input yields
     `Refusal::Unavailable(name)` regardless of the other inputs.

3. **Level clamps** — `clamp_level(0) == 1`, `clamp_level(11) == 10`, `clamp_level(5) == 5`; a
   plan's level is clamped.

4. **Grade windows exact** — `grade_for_offset` returns 0 at ±17, 1 at ±18 and ±34, 2 at ±35 and
   ±84, 3 at ±85 and ±124, 4 at ±125 and ±160, 5 at ±161.

5. **Curves and distributions** — σ and p strictly decrease with level; at L10 over 1e6 decides
   P(grade 0) ≥ 0.999 and P(Miss) == 0; at L1 P(grade 0) in 0.15..0.22 and total Miss rate
   (p_miss + |d| > 160 tail) in 0.06..0.10; two `Rng::new(same_seed)` streams are identical; a
   zero seed still advances.

6. **Planner single note** — a note at `mc0` with `Hit { d }`: no flags while `mc < mc0 + d − 8`;
   at `mc >= mc0 + d − 8` the arrow panels have `is_held = was_just_pressed = 1` and `event_mc
   = mc0 + d`; the same `E` on every later frame until judged. With `Miss`: never any flags,
   and `blocked_until[p] == mc0 + 161`.

7. **Miss blocks the next same-panel note** — note A (Miss) at 1000, note B (`Hit { 0 }`) on the
   same panel at 1100: B's emitted `E >= 1161`, i.e. B is downgraded/missed deterministically;
   with B at 1300 its `E == 1300` (unaffected).

8. **Monotonic events (property)** — over 10 000 random streams, for every panel the sequence of
   emitted `event_mc` values is strictly increasing, the cursor never decreases, and no note
   beyond `mc + LOOKAHEAD_MS + MAX_EARLY_MS` receives flags.

9. **Jumps, freezes, shocks** — a 2-panel note emits one `E` on both panels; a freeze body
   (`state >= 2`, `cur_beat < beat + max(length)`) sets `was_just_pressed` on those panels every
   frame regardless of judged state; a shock note sets `was_just_pressed` only on panels with
   `state != 1` and never sets `is_held`.

10. **Tally** — after judging a 10-note stream with known plans, `tally()` equals the expected
    per-grade counts and each note is counted exactly once.

11. **Build** — `cargo check --target x86_64-pc-windows-msvc` clean with the new modules
    declared; `cargo fmt` clean.

## Metadata
- **Complexity**: Medium
- **Labels**: pure-module, host-tested, skill-model, planner, multiplayer-bot
- **Required Skills**: Rust (std-only numerics, property-style tests), reading the judge
  algebra RE notes
- **Generated By**: code-task-generator 2026-09-13
- **Source Plan**: `.agents/planning/2026-09-13-multiplayer-bot/implementation/plan.md`
- **Plan Step**: Step 2: Pure cores — `eligibility`, `skill`, `planner` + `scripts/validate_multiplayer_bot.sh`
