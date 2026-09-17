# Task: `selection.rs` — seeded RNG, stage/dancer candidates and picks, choreography playlists, camera lists

## Description
The pure random-selection layer of the Background Dancers mod (design §4.3.2): a deterministic xorshift64*
RNG, stage candidates from `map_resources.rlist` (minus `dummy00`, requiring `mapset_<key>.arc`), the
distinct-key-then-row stage pick, dancer candidates from `chara_resources.rlist` (requiring `pl_<key>.arc`;
all rows regardless of unlock id), independent dancer picks, the A3 fixed choreography pools per sex with
Fisher–Yates playlists, the `_non` main/cut-away camera split + shuffle, the camanm path rule, and the
developer `DDR_DANCERS_PIN` override parser. Host-tested; no engine dependency.

## Background
A3 (research `a3-runtime-rules.md` §1, §4, §5): the choreography pool is a CODE list, not a directory
listing — male `br01 br02 br03 hh01 hh02 ht01 ht02 ht03 ht04 ja01 ja02 sf01 sf02 sf03` (14), female
`br01 br02 hh01 hh02 hh03 ht01 ht02 ht03 ja01 ja02 sf01 sf02 sf03` (13); `mc_male_tu01_exec.anm` exists on
disk but is NOT in the pool; `ne01_loop` is dead data. Every set is Fisher–Yates shuffled once per song.
Camera stage mode: the stage row's camanm names split on the substring `_non` into a MAIN list and a
`_non` list, both shuffled. World's `map_resources.rlist` (34 rows) carries `boom00` at rows 0 (`bg:-2 …
stage:-1`, no footpanel) AND 32 (with `footpanel`, no priorities), `monitor00` at 18/24, seven `dummy00`
rows; `stage_camera_resources.rlist` is row-parallel (row 6/8–13 are dummies with real camera sets). The
requirement is a RANDOM stage per song with every distinct stage equally likely (D-choice in idea-honing),
hence "distinct KEY uniform, then row uniform" — the row then decides the part list and the camera set.
`chara_resources.rlist` rows: `key → [pl, sex(M/F), class(A/B/C), model_scale, shadow_scale, unlock_id]`,
26 rows; all rows with a body arc are candidates (unlock ids ignored — the game's unlock table is a
different concern, and the maintainer wants every dancer).

## Reference Documentation
**Required:**
- Design: `.agents/planning/2026-09-16-enable-background-dancers/design/detailed-design.md` (§4.3.2 —
  the API; §7.1 item 5 — the tests; §2.1 FR-15 — the PIN env)
- Plan: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md` Step 6

**Additional References (if relevant to this task):**
- `.agents/planning/2026-09-16-enable-background-dancers/research/a3-runtime-rules.md` §1 (pools), §4 (camera
  lists), §5 (placement — `x = (i − (n−1)·0.5)·1.6`)
- `.agents/planning/2026-09-16-enable-background-dancers/research/formats-and-data.md` §1 (rlist contents;
  camanm path rule `data/camera/long/<name[:5]>/<name>.camanm`)
- `tests/fixtures/anm/rlists.json` (the real rows — use them in tests via the Step 5 fixture loader pattern,
  or inline the relevant rows)
- Pattern: `src/core/anm/*` (dependency-free module + `#[cfg(test)]` tests)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `src/mods/background_dancers/selection.rs`, std-only (no `crate::`), registered in
   `src/mods/background_dancers/mod.rs` as `pub mod selection;`, mounted in
   `scripts/validate_background_dancers.sh` (`MODULE_NAMES`/`MODULE_PATHS`).
2. `Rng(u64)` xorshift64* (`x ^= x >> 12; x ^= x << 25; x ^= x >> 27; x * 0x2545F4914F6CDD1D`), `new(seed)`
   maps a zero seed to a fixed non-zero constant; `next_u64`, `next_f32() ∈ [0,1)` (top 24 bits / 2^24),
   `below(n) -> u32` (unbiased: multiply-high with rejection or Lemire), `shuffle(&mut [T])` (Fisher–Yates,
   `i` from `len−1` down to 1, `j = below(i+1)`), `pub fn seed_from(qpc: u64, scene_id: u32) -> u64` =
   `qpc ^ (scene_id << 32)`.
3. `enum Sex { Male, Female }` with `from_field("M"/"F")`, `arc_stem()` → `"mc_male"`/`"mc_female"`;
   `StageCandidate { key, row, parts: Vec<(String, Option<i32>)> }` (parts parsed from fields `[2..]` as
   `name[:prio]`; a field that fails to parse as `name:int` is taken as a plain name);
   `DancerCandidate { key, row, sex, class: String, model_scale: f32, shadow_scale: f32 }`.
4. `stage_candidates(rows, exists)` drops `dummy00`, requires `exists("mapset_<key>.arc")`, requires ≥ 1
   part; `pick_stage(rng, cands)`: uniform over DISTINCT keys (first-appearance order), then uniform over
   that key's rows. `dancer_candidates(rows, exists)`: requires ≥ 5 fields, a parseable sex, parseable
   scales, `exists("pl_<key>.arc")`; `pick_dancers(rng, cands, n)`: `n` independent uniform picks (repeats
   allowed — A3's random kinds), empty when `cands` is empty.
5. `POOL_MALE`/`POOL_FEMALE` constants; `pool(sex)`; `playlist(rng, sex) -> Vec<String>` = shuffled pool
   mapped to `"mc_<sex>_<name>_exec"` (the `.anm` member name is `data/chara/mc_<sex>/<that>.anm` —
   provide `clip_member_path(sex, clip)`); `camera_lists(rng, camera_row_fields) -> (main, non)` split on
   `contains("_non")`, both shuffled; `camanm_member_path(name)` = `data/camera/long/<name[..5]>/<name>.camanm`
   (`st001x2_st06` → dir `st001`; names shorter than 5 chars use the whole name).
6. `dancer_x(i, n) -> f32` = `(i − (n−1)·0.5)·1.6` (A3 placement; n=1 → 0, n=2 → ∓0.8).
7. `Pin { stage: Option<String>, dancers: Vec<String> }`, `parse_pin(&str) -> Option<Pin>` over
   `<stage>[,<chara>[,<chara>]]` (empty segments ignored, `""`/all-empty → None); `apply_pin(pin, stages,
   dancers, n) -> Option<(StageCandidate, Vec<DancerCandidate>)>`: the pinned stage key (first row of that
   key), the pinned dancer keys in order (repeating the last one to fill `n`; unknown key ⇒ `None` so the
   caller logs and falls back to random).
8. Tests (§7.1 item 5): `dummy00` excluded and existence required (a stub `exists` closure); uniformity —
   χ² over 10⁵ `pick_stage` draws on the REAL 34-row table (from the rlist fixture when present, else an
   inlined copy of the key/row multiset) is within the 99.9 % critical value for 25 distinct keys, AND the
   two `boom00` rows are each hit ≈ 50 % of `boom00`'s draws; `playlist` is a permutation of the pool for
   both sexes and differs between two seeds; `camera_lists` on the `boom00` camera row gives 6 main + 4
   `_non` names; `camanm_member_path` for `st001x2_st06`, `floor_st04`, `chara_in01`; `parse_pin` shapes;
   `apply_pin` fill/unknown; `shuffle` on 0/1/2-element slices; `below(1) == 0`; `seed_from` determinism.

## Dependencies
- Step 5 (`core/anm::rlist` is where the rows come from at runtime; the tests may load
  `tests/fixtures/anm/rlists.json` through the same env conventions as `core/anm/tests/fixtures.rs`).

## Implementation Approach
1. RNG + helpers → candidates/picks → pools/playlists → camera lists/paths → placement → pin.
2. Tests; harness mount; `cargo check --target x86_64-pc-windows-msvc`; `cargo fmt`.

## Acceptance Criteria

1. **Exclusions and existence**
   - Given the real `map_resources` rows and an `exists` that reports every `mapset_*.arc` except `cyber00`
   - When `stage_candidates` runs
   - Then no `dummy00` and no `cyber00` candidate exists, and both `boom00` rows (0 and 32) are present with
     their own part lists (`bg:-2` parsed as `("bg", Some(-2))`)

2. **Uniform distinct-key pick**
   - Given 10⁵ draws of `pick_stage` on the real table
   - When the per-key counts are tallied
   - Then the χ² statistic over the 25 distinct keys is below the 99.9 % critical value and each `boom00`
     row receives 50 % ± 5 % of the `boom00` draws

3. **Playlists are permutations**
   - Given two seeds
   - When `playlist` runs for each sex
   - Then each result is a permutation of the sex's pool (14 / 13 entries, `tu01` absent) and the two seeds
     give different orders

4. **Gates**
   - Given the finished change
   - When `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./scripts/validate_background_dancers.sh` run
   - Then all are clean/green

## Metadata
- **Complexity**: Low
- **Labels**: background-dancers, step-6, pure, selection, rng
- **Required Skills**: Rust, basic statistics for the χ² test
- **Generated By**: code-task-generator 2026-09-16
- **Source Plan**: `.agents/planning/2026-09-16-enable-background-dancers/implementation/plan.md`
- **Plan Step**: Step 6: Pure selection and schedule
