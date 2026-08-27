# Plan: Step 4 Task 1 — Build Diagnostic Policy and Score Safety

Status: Approved 2026-08-06 (inherits the approved generated task, source plan,
and design; maintainer approved the Step 4 breakdown in-session on 2026-08-06)

Host-only. No deployment. No generated-path exposure, no non-identity Q31, no
score taint production. Repository-relative paths throughout.

## Shape

### New pure module `src/services/song_rate/lifecycle.rs`

- `GenerationPhase` (Identity, Armed, Preparing, RedirectReady, XactInFlight,
  Committed, Completed, EarlyFailed, LateFailed) stored in an atomic lifecycle
  state (`AtomicU8` phase + `AtomicU64` generation + percent/mask/stage
  atomics + a CAS write guard). All transition entry points are lock-free and
  never wait; a contended arm returns `ArmOutcome::Busy` (caller falls closed
  to identity with a bounded warning).
- `DiagnosticSpec` + `validate_diagnostic(...)` — pure validation of the
  developer config (nonempty alnum song code, `requested_percent == 75`
  exactly, nonempty xwb path). Invalid ⇒ typed error ⇒ runtime treats as
  absent (identity).
- `EligibilityInputs` (services_ready, course_field: Option, entered:
  Option<[bool;2]>, stage_index: Option, diagnostic: Option<&DiagnosticSpec>)
  → `classify_scene26(...) -> EligibilityDecision` (Arm(ArmRequest) |
  Identity(IdentityReason)). Exactly one entered side arms; zero sides, two
  sides (local versus), nonzero course, unreadable pointers, missing services,
  or absent/invalid diagnostic resolve to identity. Matching/BPL/demo/event
  chains never enter normal scene 26 (design req 12) — documented as the
  structural exclusion; the classifier additionally fails closed on anything
  unreadable.
- Scene-transition engine: `classify_transition(prev, next)` +
  `apply_*` transitions writing effects through a minimal
  `LifecycleSink` trait (`publish_identity(gen, mask)`, `reset_identity()`,
  `set_movie_suppressed(bool)`), so host tests can record effect ORDER.
  Definitive rules (design "Lifecycle State Machine"):
  - scene-26 entry, no XactInFlight: re/arm (or identity);
    accepted non-100% arm suppresses the movie contributor tentatively.
  - pre-exposure phases leaving the {26 SONG_TO_STAGE_INTERSTITIAL,
    27 STAGE_INDICATOR, 28 GAMEPLAY} corridor without reaching gameplay:
    abandon → Identity, movie cleared.
  - GAMEPLAY→GAMEPLAY: Quick Restart — retain phase/generation/movie.
  - GAMEPLAY→non-GAMEPLAY: Completed — `reset_identity()` FIRST, then movie
    clear; score/pending state untouched.
  - title/attract/new-session reset (no XactInFlight): force identity, clear
    non-score generation state and movie contributor.
  - Arm attempted during XactInFlight: refused (`Deferred`), no state change.
  - Phase entry points for Task 2 (`on_preparing`, `on_redirect_ready`,
    `on_exposed`, `on_committed`, `on_late_failed`) validate legal transitions
    and are host-tested; the commit-ordering EFFECTS are Task 2 scope.
- Uses `crate::types::scenes::scene` constants (harness gains a `types` mod).

### New `#[cfg(windows)]` glue `src/services/song_rate/runtime.rs`

- `init(spec: Option<DiagnosticSpec>) -> bool`: latches boot readiness
  (identity readiness + clock installed + movie available +
  `score_guard::is_full_sanitization_available()`) into one atomic, stores the
  spec, registers ONE permanent scene callback.
- Callback hot path: atomics + raw validated reads only (stage_records
  course/entered/stage-counter, score_guard readiness re-check); calls the
  pure engine; production `LifecycleSink` writes `movie_policy::set_suppressed`
  and `clock_patch` publication. No mutex, no I/O, no game calls, no
  scene-manager re-entry (scene callbacks run under the manager lock).
  Readiness that would require a Mutex (avs_layeredfs flags) is latched once
  at init, never read in the callback.

### `src/services/score_guard.rs` additions (stays harness-pure)

- Per-side fixed 8-entry `PendingRateSave` ring: per-entry
  `generation: AtomicU64`, `stage_index: AtomicI32`, `sequence: AtomicU64`
  (append order), `state: AtomicU8` (Free/Pending/Claimed/Consumed).
  - `append_pending_rate_save(side, generation, stage)` — idempotent per
    generation (Quick Restart dedup); ring overflow sets a per-side sticky
    fail-closed flag (all stage saves suppressed until card-in reset).
  - `pending_rate_count(side)` counts Pending+Claimed.
  - `elect_stage_save_policy(side: Option<usize>, stage: Option<i32>)
    -> StageSavePolicy` — pure election:
    `LegacyAllow` / `LegacySuppress` (existing autoplay/quick-fail behavior,
    consumes nothing) / `SuppressConsume{..}` (exact oldest side+stage
    Pending match: claim CAS Pending→Claimed, consume Claimed→Consumed) /
    `SuppressNoConsume{reason}` (unknown side while any pending exists on
    either side — never default P1; unknown stage while side has pending;
    non-matching stage while side has pending; overflow flag; duplicate
    Consumed tombstone match).
  - `reset_rate_state_for_side(side)` — clears only that side's ring +
    overflow flag (positive card-in match ownership).
- `is_stage_suppressed(side)` gains `|| pending_rate_count(side) > 0 ||
  overflow(side)`.
- Readiness latches + `is_full_sanitization_available()`: save detour (existing
  `mark_hook_installed`), `mark_stage_records_ready`,
  `mark_scene_manager_ready`, `mark_sanitiser_registered`,
  `mark_league_strip_available` (+getter; single source of truth — the cop
  local `LEAGUE_STRIP_AVAILABLE` static migrates here).
- Pure logout league policy helper: tri-state `LeagueStripOutcome`
  {NodeAbsent, Removed, RemovalFailed} → forward/forward/fail-closed decision.

### `src/services/custom_options_persistence.rs` (windows)

- Save trampoline: playside decode becomes `Option<u8>`; savekind==STAGE path
  consults `elect_stage_save_policy(side, stage_records::stage_counter())`.
  Unknown side with NO pending rate state anywhere preserves today's
  default-to-0 + warn. Rate suppressions latch `mark_session_tainted` exactly
  as legacy suppressions do. Bounded logs keep the existing
  `score_guard: ...` formats.
- `strip_league_node` returns the tri-state; `RemovalFailed` logs an error and
  the trampoline returns 0 (sender-failure — the only post-build fail-closed
  lever; realistically unreachable).
- Latch calls: `mark_sanitiser_registered()` in `register_logout_sanitiser`,
  `mark_league_strip_available(true)` in `resolve_libavs_ordinals`.
- Deferred per-side rate reset: on successful load (`result != 0`,
  independent of persist gating) capture ddrcode; on SONG_SELECT drain,
  `side_from_ddrcode` → `reset_rate_state_for_side(side)`. Failed/unmatched
  loads clear nothing. The legacy broad `reset_session()` call is untouched.

### `src/services/stage_records.rs` (windows + one pure helper)

- Hoist the stage-counter decode: pure
  `decode_stage_counter_offset(bytes) -> Option<usize>` validating the literal
  `FF 41 0C` INC and extracting disp8; init stores it when
  `premium_free_stage_inc` resolves (optional — absence leaves only
  `stage_counter()` unavailable, not the whole service);
  `stage_counter() -> Option<i32>` reads GameWork+offset. Decode happens at
  init, before premium_free can NOP the site. premium_free is not modified.

### `src/mods/config.rs`

- `song_playback_speed: Option<SongPlaybackSpeedConfig>` with
  `diagnostic: Option<SongRateDiagnosticRawConfig>` (raw serde strings/ints;
  both fallback constructors updated). Honored only when LayeredFS developer
  mode is enabled (gate applied in lib.rs wiring; same gate family as the
  design's fault selector).

### `src/lib.rs`

- Latch `score_guard::mark_stage_records_ready()` /
  `mark_scene_manager_ready()` after the respective successful inits;
  call `song_rate::runtime::init(...)` after scene_manager init with the
  dev-gated validated spec; log armed-able vs unavailable (bounded).

### `scripts/validate_song_playback_speed.sh`

- Required-file list += `lifecycle.rs`, `lifecycle_tests.rs`,
  `src/services/score_guard.rs`, `src/services/score_guard_tests.rs`,
  `src/types/scenes.rs`.
- Generated harness main.rs: add `mod types { pub mod scenes; }`, services
  `score_guard` (+ `#[cfg(test)] score_guard_tests`). `song_rate/mod.rs`
  gains `lifecycle`(+tests) and `#[cfg(windows)] runtime` (skipped on host).
- NO report-schema change (schema stays `song-rate-validation/v1`; Task 3
  owns the diagnostic-transaction report extension).

### Module declarations

- `src/services/song_rate/mod.rs`: `pub mod lifecycle;`,
  `#[cfg(windows)] pub mod runtime;`, `#[cfg(test)] mod lifecycle_tests;`
- `src/services/mod.rs`: `#[cfg(test)] mod score_guard_tests;`

## Test scenarios (all host; TDD red via the validator's missing-file check first)

Acceptance criterion → scenarios:

**AC1 Eligibility exact and nonblocking** (`lifecycle_tests.rs`)
1. Solo P1 (services ready, course 0, entered [T,F], stage 0, valid spec) →
   `Arm{75, mask 0b01, stage 0}`.
2. P2-started doubles (entered [F,T]) → `Arm{mask 0b10}`.
3. Course nonzero → Identity(CourseMode).
4. Zero entered sides → Identity(NoSideEntered).
5. Two entered sides → Identity(LocalVersus).
6. Unreadable course/entered/stage (None) → Identity(UnknownSession /
   StageUnknown) — each input individually.
7. Services not ready → Identity(ServicesUnavailable).
8. No/invalid diagnostic (empty code, percent 100/125/74, empty path) →
   Identity(NoDiagnostic) / validation errors.
9. Contended arm (state guard held) → `Busy`, state unchanged.

**AC4 Tentative movie lifecycle + transition engine** (`lifecycle_tests.rs`,
recording sink + real `MoviePolicy`/`RatePublication` sink impl)
10. Identity → scene-26 accepted arm: Armed, movie suppressed, factor stays
    IDENTITY_Q31, snapshot committed=false, generation increments.
11. Re-arm at a later scene 26: new generation, movie stays suppressed.
12. Armed through 26→27→28: retained.
13. Pre-exposure abandonment (corridor exit, e.g. →25): Identity + movie
    cleared.
14. GAMEPLAY→GAMEPLAY: phase/generation/movie retained (Quick Restart).
15. GAMEPLAY→non-GAMEPLAY from Armed/EarlyFailed/Committed: Completed;
    sink order asserts `reset_identity()` strictly before movie clear.
16. Title/attract reset: Identity + movie clear; no score-state effect.
17. Identity arm (ineligible): movie never suppressed.
18. Arm during XactInFlight: refused, nothing changes (supersession refusal).
19. EarlyFailed retains movie suppression until the Completed boundary.
20. Illegal phase entries (`on_exposed` from Identity etc.): typed error,
    state unchanged. Legal chains Armed→Preparing→RedirectReady→XactInFlight→
    {Committed, LateFailed} accepted; LateFailed → clean scene-26/session
    reset → Identity.
21. NonNativeOs contributor unaffected by SongRate flips (real MoviePolicy).

**AC2 Pending-save identity fail-closed** (`score_guard_tests.rs`)
22. Append idempotent per generation (Quick Restart dedup).
23. `pending_rate_count` counts Pending+Claimed only.
24. Exact claim: oldest matching (side, stage) Pending consumed exactly once.
25. Duplicate sender retry after consumption: tombstone suppresses again, no
    second consumption.
26. Reordered: pending stage 2; save stage 3 → SuppressNoConsume (entry
    survives); save stage 2 → SuppressConsume.
27. Delayed save after later arms/scene changes: ring has no scene coupling —
    still suppressed.
28. Unknown side while any pending exists (either side) → SuppressNoConsume;
    NEVER SuppressConsume; never P1 default.
29. Unknown side, no pending anywhere → LegacyAllow/LegacySuppress path.
30. Unknown stage while side has pending → SuppressNoConsume.
31. Per-side reset: side 1 reset leaves side 0 ring intact (P2 load cannot
    erase P1).
32. Overflow (9th live generation): sticky fail-closed; reset clears.
33. `is_stage_suppressed` includes pending/overflow; autoplay/quick-fail
    interplay unchanged when rings are empty.

**AC3 Full-sanitization readiness** (`score_guard_tests.rs`)
34. All five latches → true; each latch individually absent → false
    (table-driven).

**League tri-state**
35. Pure policy: NodeAbsent→forward, Removed→forward, RemovalFailed→
    fail-closed (return-0 signal decision).

**Stage-counter decode** (pure helper)
36. `decode_stage_counter_offset`: exact `FF 41 0C` → Some(0xC); any other
    bytes → None.

**AC5 Host-only safety** — full gate suite passes; the existing
`identity_runtime` report checks (identity Q31, no dynamic redirect,
exactly-once originals) must stay green; `identity_conversion_path` untouched.

## Implementation order (TDD)

1. Extend the validator script (file list + harness mods) → run → RED on the
   intentionally absent files.
2. `score_guard` rings/latches/policy + `score_guard_tests` → green subset.
3. `lifecycle.rs` (+tests): validation, eligibility, state machine, sinks.
4. `stage_records` decode helper (+ pure test in score_guard_tests or a small
   `#[cfg(test)]` block — decide by file fit), windows accessor.
5. `config.rs` serde section.
6. `custom_options_persistence` trampoline/latch/reset changes (windows).
7. `runtime.rs` + `mod.rs`/`services/mod.rs`/`lib.rs` wiring (windows).
8. Full gates: `./scripts/validate_song_playback_speed.sh`,
   `./scripts/validate_se_bank_synth.sh`,
   `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`.
9. Update canonical progress.md (Task 2 as NEXT ACTION) + this dir's
   progress.md.

## Risks / notes

- Stage-identity decode strategy (GameWork counter at save time) is
  conservative under delay skew (over-suppresses, never leaks); upgrade path
  documented in context.md §Interpretations 1. Live proof lands in Task 3.
- The seqlock writer paths are lock-free but may spin briefly against a
  concurrent writer; the scene-callback arm uses the non-waiting guard and
  falls closed to identity when contended.
- No commit-path effects (score protection publication, movie confirmation,
  snapshot, Q31-last) are implemented here — Task 2 owns that ordering and its
  fault injection.
- `cargo fmt` always whole-crate (AGENTS.md).
