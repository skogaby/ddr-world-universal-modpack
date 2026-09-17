# Task: `schedule.rs` — the A3 dance-cut and camera sequencing as pure, seed-deterministic functions of time

## Description
Port A3's `SceneManageActor`/`CharaActor`/`CameraActor` sequencing to two pure schedules (design §4.3.3):
`DanceSchedule` (each dancer's shuffled playlist chained into segments whose length is
`min over dancers of the current clip's duration − 1.5 s` — the shared hard cut — and a `t → DancePos`
lookup) and `CameraSchedule` (main list cycling on finish, switching frozen while any dance cut is within
2.0 s, a `_non` cut-away shot at the cut held `1 + U[0,1)` s from the per-song seed, then back to the next
main clip; NO beat gate in v1). Both are pure functions of `(seed, t)` so a rewind or a scrub simply
re-simulates from 0. Host-tested per design §7.1 item 6.

## Background
A3 (research `a3-runtime-rules.md` §1, §4): every frame the SceneManageActor takes the MOST-URGENT dancer
(`remaining = dur − t`), broadcasts `0x104f` at `remaining < 2.0 s` (camera switching frozen) and `0x1050` at
`remaining < 1.5 s` — on which EVERY dancer advances to its next clip (`idx % n`, cycling forever) and the
camera, if the `_non` list is non-empty, cuts to `non[0]` with `hold = now + 1.0 + U[0,1)` and rotates the
`_non` list by one; after the hold (A3 also waits for a beat boundary — deferred) it resumes the main list
at the next index; main clips otherwise switch when their own finished bit sets (`t ≥ dur`, camanms are
~450 f = 7.5 s). Consequences: the last 1.5 s of every dance clip are never shown; segment k of the song
runs from `Σ_{j<k} len_j` where `len_k = min_i dur(playlist_i[k mod n_i]) − 1.5`; both dancers cut on the
same instant. The design clamps a segment to ≥ 0.05 s (a sub-1.5 s clip cannot stall the chain) and drives
all camera randomness from `rng_seed ⊕ k` so `at(t)` is pure.

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.3.3 — the
  API; §4.3.4 — how the director consumes `DancePos`/`CameraState`; §7.1 item 6 — the tests; §9 Appendix B)
- Plan: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 6

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-16-enable-background-dancers/research/a3-runtime-rules.md` §1 (advance rule,
  constants 1.5 / 2.0), §4 (stage-mode camera rules), §6 (clip time advance: loop/finish)
- `src/core/anm/sample.rs::clip_time` (the per-clip time mapping the director applies AFTER `DancePos`)
- `tests/fixtures/anm/dance_clips.json` (`frame_count`/`fps` of the real pool clips: 18–23 s each) and
  `stage_cameras.json` (camanm durations)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/mods/background_dancers/schedule.rs`, std-only, registered in the mod's `mod.rs`, mounted in the
   harness. Depends on `selection::Rng` for the `_non` hold draws (or an equivalent local xorshift on
   `rng_seed ^ k` — document which; determinism is the requirement).
2. `pub const CUT_LEAD: f32 = 1.5;` `pub const CAMERA_FREEZE_LEAD: f32 = 2.0;` `pub const MIN_SEGMENT: f32 =
   0.05;` `pub const NON_HOLD_BASE: f32 = 1.0;`
3. `ClipRef { name: String, duration_s: f32, loops: bool }` (from `Anm::duration_s()`/`loops` at session
   build). `DanceSchedule::new(per_dancer: Vec<Vec<ClipRef>>) -> Option<Self>` (None when any dancer has an
   empty playlist); `segment_len(k)` = `max(MIN_SEGMENT, min_i dur(playlist_i[k mod n_i]) − CUT_LEAD)`;
   `at(dancer, t) -> DancePos { clip, local_t, segment, segment_start, segment_end }` walking segments from
   0 (t < 0 ⇒ segment 0 with `local_t = t`, negative — the director hides before the edge anyway);
   `cut_times(until) -> Vec<f32>` = segment ends `< until`; `segments_until(t)` helper. Provide
   `dancer_count()`.
4. `ClipSel { Main(usize), Non(usize) }`; `CameraState { clip: ClipSel, clip_start: f32, hold_until:
   Option<f32>, frozen: bool, non_rotation: usize, main_index: usize }`; `CameraSchedule::new(main:
   Vec<ClipRef>, non: Vec<ClipRef>, rng_seed: u64) -> Option<Self>` (None when `main` is empty; an empty
   `non` list disables cut-aways). `initial() -> CameraState` = `Main(0)` at 0. `advance(&self, st, from,
   to, dance) -> CameraState` applies, in time order, every event in `(from, to]`:
   - main-clip finish: `clip_start + dur ≤ t` while not frozen and not in a `_non` hold ⇒ `Main((idx+1) % n)`
     starting at the finish instant (chained so several short clips can pass in one step);
   - freeze: `frozen = any cut c with 0 ≤ c − t < CAMERA_FREEZE_LEAD` (evaluated at `to`, and it also
     suppresses main finishes inside the window);
   - cut (t crosses a `cut_times` instant): if `non` non-empty ⇒ `Non(non_rotation)` with `clip_start = cut`,
     `hold_until = Some(cut + NON_HOLD_BASE + U_k)` where `U_k ∈ [0,1)` comes from `Rng::new(rng_seed ^
     (k as u64 + 1)).next_f32()` (k = the cut's segment index), `non_rotation += 1`; else stay;
   - hold end: `hold_until ≤ t` ⇒ `Main((main_index + 1) % n)` starting at the hold end, `hold_until = None`.
   `at(t, dance)` = `advance(initial, 0, t)`. Purity: `advance(advance(s, a, b), b, c) == advance(s, a, c)`.
5. Tests (§7.1 item 6): segment lengths equal `min(dur) − 1.5` for a two-dancer table with unequal
   playlists (e.g. 3 vs 4 clips; check the `k mod n_i` wrap), `MIN_SEGMENT` clamp for a 1.0 s clip; both
   dancers' `DancePos.segment_start/_end` are identical at every sampled `t`; `at(t)` equals stepping
   `advance` in 1/60 s increments over 200 s (dance AND camera, exact equality); re-simulation after a rewind
   (`at(t2)` with `t2 < t1`) equals the forward path; camera timeline on a synthetic set (main durations
   7.5 s, cuts at 19.7 s…): main index advances at 7.5/15.0, frozen from 17.7, `Non(0)` at 19.7 with a hold in
   `[20.7, 21.7)`, back to `Main(next)` at the hold end, `non_rotation` cycles modulo the list; with an empty
   `non` list the camera simply stays frozen through the cut and resumes its own cycle; seeds change the hold
   but not the cut instants; `--nocapture`-friendly `#[test] fn print_example_timeline()` that prints a seeded
   song's segment table and camera events (the plan's "Demo").

## Dependencies
- Task 01 (`selection::Rng`).

## Implementation Approach
1. `DanceSchedule` + tests → `CameraSchedule::advance` as an event loop over `[from, to]` with the next
   pending event chosen by time (finish / cut / hold-end) → `at` → purity + timeline tests.
2. Harness mount; `cargo check --target x86_64-pc-windows-msvc`; `cargo fmt`.

## Acceptance Criteria

1. **Shared cuts**
   - Given dancer A playlist durations `[21.2, 22.0, 19.8]` and dancer B `[20.9, 22.5, 18.9, 21.4]`
   - When `at(0, t)` and `at(1, t)` are sampled every 0.25 s over 300 s
   - Then `segment`, `segment_start`, `segment_end` agree between dancers at every sample and the segment
     lengths are `min(dur_A[k mod 3], dur_B[k mod 4]) − 1.5`

2. **Purity**
   - Given any seed and the synthetic camera set
   - When `CameraSchedule::at(t)` is compared with 1/60 s incremental `advance` stepping up to 200 s and with
     a rewind re-simulation
   - Then every field of `CameraState` is identical

3. **A3 camera timeline**
   - Given main clips of 7.5 s and the dance cuts of criterion 1
   - When the timeline is stepped
   - Then the main index advances at 7.5 s and 15.0 s, `frozen` is true from `cut − 2.0`, the `_non` shot
     starts exactly at the cut and ends within `[cut + 1.0, cut + 2.0)`, and the resume picks `Main(next)`

4. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./scripts/validate_background_dancers.sh` run
   - Then all are clean/green

## Metadata
- **Complexity**: Medium
- **Labels**: background-dancers, step-6, pure, schedule, camera
- **Required Skills**: Rust, discrete-event simulation over floats (careful with `<` vs `<=` at boundaries)
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 6: Pure selection and schedule
