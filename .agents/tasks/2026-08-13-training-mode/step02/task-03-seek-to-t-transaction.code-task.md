# Task: Seek-to-T transaction in song_reset

## Description
Make `request_reset(t_ms != 0)` real (the shipped `Unsupported` arm):
gates + seek clamp, content-mapping publication between cue stop and
replay, back-dated anchor, post-broadcast record rebuild at `t_q`,
spanning-freeze neutralization, an `AccumulatorPolicy` parameter, and
`on_song_reset(t_q)` subscriber notification. This is the gameplay-state
half of the seek — the audio half rides Step 1's mapping API (with
task-01's O(1) seeded seeks at rate).

## Background
The transaction mirrors the shipped reset (research §5.4): gates →
stop cue → publish the mapping (`song_rate::runtime::set_content_mapping
(B(t_q), B(delay_ms))`; no live binding ⇒ `Refused`, callers fall back per
R6) → replay cue → poll prepared → ONE synchronous frame block: `0x1043` +
`0x1044 {now_tick − wall(t_q)}` broadcast (the handler rebuilds at 0 and
re-anchors), then per GamePlayActor re-run the record-rebuild trio
(clear / reserve / rebuild) DIRECTLY with playhead `t_q`, apply the
spanning-freeze neutralization writes, apply the accumulator policy, and
notify subscribers. The rebuild workers are reached by deriving the three
call sites from inside the `0x1044` handler (`FUN_18005bac0` — research
§2.1; one anchor, fail-closed: unresolved ⇒ seeks `Refused`). The seek
clamp needs ControlMessageActor (RTTI child walk of each GamePlayActor,
`.?AVControlMessageActor@dance@sequence@@`): refuse when
`t_q ≥ chart_end_raw@+0x98 − MARGIN` or the end cascade already fired
(StackStep ≥ 3/4 — research §4.3: the cascade is one-way; a seek past the
thresholds hard-ends the song unresettably).

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-08-13-training-mode/design/detailed-design.md (§4.4, §6 error ladder)

**Additional References (if relevant to this task):**
- docs/training_mode_research.md §2 (primitives), §4.3 (clamp), §5.4 (transaction order), §6 (anchor math), §7 (0x1044 subscribers all-clear)
- src/services/song_reset/ (the shipped t=0 transaction this extends; task-02's pure module)
- src/services/song_rate/runtime.rs (`set_content_mapping`)
- src/core/signatures.rs (derivation patterns: `find_vtable_by_rtti`, `scan_first_call_rel32`, `decode_call_rel32`)

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `request_reset(t_ms, delay_ms, policy, on_recovery)` — the
   `AccumulatorPolicy` enum (`Zero` = the shipped zeroing; `Keep` defined,
   reserved for v2 FF/RW, refused/unused in v1). The single existing call
   site (`quick_restart_or_fail`) updates to `Zero`. `t_ms == 0` behavior
   stays bit-for-bit shipped.
2. Rebuild-trio derivation: locate clear/reserve/rebuild from the `0x1044`
   handler's call sites at init (new derived signature(s); declare in the
   appropriate `required`/availability surface). Unresolved ⇒ nonzero-T
   requests return `Refused` (fail-open; t=0 unaffected).
3. ControlMessageActor discovery per side (RTTI vtable walk of the
   GamePlayActor children) + the seek clamp (raw `+0x98` − MARGIN, cascade
   StackStep unfired). Expose a `chart_end_raw(side)` accessor for the mod
   (task-04's marker clamps; Steps 3–4 consume it further).
4. Transaction order exactly per research §5.4; the mapping publication
   sits BETWEEN stop and replay; a `false` from `set_content_mapping`
   (no live binding) ⇒ `Refused` before any state is touched.
5. Post-broadcast per actor: trio at `t_q`, then task-02's neutralization
   writes, then the policy application (Zero = the shipped accumulator/
   gauge block), then `on_song_reset(t_q)` (subscribers already handle
   nonzero T — assist_tick's rewind conversion, PUS, score_guard).
6. Delayed seeks compose: `delay_ms` future-dates the anchor exactly like
   the shipped v4 countdown while the mapping's lead serves the silent
   approach; the prepared→anchor adjacency protocol is preserved.
7. Every game-memory read range-validated; no locks across engine calls;
   generation-tokened like the shipped driver; one bounded WARN per
   degraded path.

## Dependencies
- task-01 (seeded seeks make the mapping change O(1) at rate).
- task-02 (T_q/anchor math + neutralization planner + module layout).

## Implementation Approach
1. Signature/derivation work first (trio + CMA vtable), logged availability.
2. Extend the transaction along the shipped t=0 skeleton; keep the
   instant/delayed split.
3. Wire the pure planners; cabinet-validate via task-04's demo (this task
   alone is engine-facing — no host harness beyond what task-02 covers).

## Acceptance Criteria

1. **t=0 unregressed**
   - Given the shipped instant and delayed restart paths
   - When a quick restart runs on the cabinet
   - Then behavior is unchanged (field reset, cue replay, anchor adjacency, subscribers)
2. **Seek gates fail closed**
   - Given no live binding, an unresolved trio, a clamp violation, or a fired cascade
   - When `request_reset(t_ms != 0, …)` is called
   - Then it returns `Refused` with no game state touched and one bounded WARN
3. **Seek lands exactly**
   - Given a live (identity or rate) binding mid-song
   - When a seek to T fires
   - Then audio resumes at content T_q after the lead, the clock reads T_q (claps aligned via assist_tick), pre-T notes are consumed-neutral (never mass-missed), and a spanning freeze is neutralized
4. **Policy parameter**
   - Given `AccumulatorPolicy::Zero`
   - When the seek completes
   - Then score/combo/gauge reset exactly like the shipped restart; `Keep` exists but is refused/unreachable in v1

## Metadata
- **Complexity**: High
- **Labels**: song-reset, seek, signatures, engine-facing
- **Required Skills**: Rust, the repo's hook/derivation conventions, song_reset transaction model
- **Generated By**: code-task-generator 2026-08-13
- **Source Plan**: .agents/planning/2026-08-13-training-mode/implementation/plan.md
- **Plan Step**: Step 2: Seek-to-T in song_reset + A/B gestures + restart-from-A
