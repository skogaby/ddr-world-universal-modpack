# Progress: Training Mode v1 (section practice)

Updated: 2026-08-15
Status: **Step 8 of 9 COMPLETE (plan ticked with an "As landed" note —
6 demo rounds, round 6 PASS, maintainer-verified; task record closed).
Steps 1–6 committed as `4413cc8`; Step 7 + amendments + Step 8 committed
by the maintainer as `ff8aa39` (only the Step-8 close-out doc edits
remain uncommitted).**
**NEXT ACTION:** Step 9 (docs, default config, regression pass) — run
code-task-generator on plan Step 9 when the maintainer continues.
Step 9 must fold in: README feature row + config section; the grouped
default `row_order` example with `header_training_options` leading the
training block (R9 — note the maintainer's live install groups it as
header → song_speed → preserve_pitch → assist_tick → assist_tick_volume
→ training_* — use that as the shipped example's shape, incl.
`step_data_export`); AGENTS.md training-mode feature row (Step-7 scrub
notes + `training_mode.{ff,rw}_increment_ms` config keys + the Step-8
header/R10 mechanism incl. the options_scroll `selectable` skip and the
UiKind::Header framework surface); assist-tick README taint note; the
full design-§7 cabinet regression checklist end-to-end.
Step-8 as landed (details in the plan's "As landed" note + the task
record `.agents/scratchpad/2026-08-13-training-mode/header-rows-and-grouping/`):
FULL-height header row (half-height objective dropped after 5 rounds —
layout-slot halving doesn't shrink clip art; full-box label bitmaps
always bled below the box grid line); the header's entire look is the
opaque 352x16 dark-blue/white label texture centered in the text zone;
render hides `choice_usr` + `invalid_usr` (the default-state gray
cover); cursor skip lives in options_scroll's `predict_target` via
`RowHandle.selectable` (the driver replaces the native `+0x28` scan on
the overflowing Mods tab); R10 exclusion in `compute_order` (unlisted
headers absent, normal rows byte-identical); harness 324/324 (18 new
tests; `ordering.rs` + `header_rows_tests.rs` mounted).
Step-7 as landed (details in the task record + the plan's "As landed"
note): single-press pinpad 7=RW / 9=FF, delay-0 music-player scrub
(rewind-past-start = instant t=0 restart; TRAINING_LEAD_MS stays
section-practice-only); pure `section_math::scrub_target` clamp
(margin after the min, aligned with the seek gate) +
`normalize_scrub_increment_ms` (None→5000, 250..=60000);
`training_mode.{ff,rw}_increment_ms` config block latched at enable;
RW/FF indicator icons (`scrub_indicator.rs`, toast fade, left/right,
repo PNGs via `scripts/gen_training_scrub_icons.py`); cooling =
`SCRUB_COOLING` + the new `song_reset::reset_in_flight()` +
`driver::loop_reset_in_flight()`; scrub requires a live binding (versus
never disturbed); taint = SESSION_ACTIVE + set_training_taint on
Started (covers the t=0 edge). Plus three shipped-machinery fixes the
demos surfaced: assist_tick reset re-shift (Playing→Ready demotion, no
mid-song resynthesis — also closed the checkpoint-4 loop clap gap +
negative-skip wait for future-dated restarts); reset clap floor
(`mute_head_bytes` on `rewrite_tick_wave` — consumed pre-target notes
never clap, loop lead + R15 skip-first lead silent); LOOP ON death
bypass (`+0x2B7` gate stash/arm at the loop latch, driver death-fire
"death revive", reset's flag-clear + gauge restore = the revive,
stock death on every degraded path). 5 new host tests (306 total).
Step-7 as landed (details in the task record): single-press pinpad 7=RW /
9=FF in `bounds::on_input_event` (no GestureBuffer; GAMEPLAY +
GESTURES_ACTIVE gated, per-side); pure target math
`section_math::scrub_target` = `clamp(current+delta, 0,
min(b_live?, chart_end) − margin)` (margin AFTER the min — aligned with
the seek transaction's own `min_end − 1000` gate; degenerate ⇒ None) +
`normalize_scrub_increment_ms` (None→5000, clamp 250..=60000); config
block `training_mode.{ff,rw}_increment_ms` (`TrainingModeConfig`,
QuickRestartConfig shape), latched at enable via
`bounds::load_scrub_increments` (one INFO on out-of-range); dispatch
`request_reset(t_q, TRAINING_LEAD_MS, Zero, None)` after
`quantize_marker`; **t_q ≤ 0 ⇒ the plain t=0 delayed restart** (loop-
driver precedent — anchor-equivalent to a seek-to-0; AC 4's
rewind-to-start); cooling = own `SCRUB_COOLING` latch (set at Started,
lazily cleared via the NEW `song_reset::reset_in_flight()` accessor —
covers completion, every recovery path, scene changes) + yield to the
NEW `driver::loop_reset_in_flight()` (`LOOP_COOLING` reader); the scrub
requires a live binding on EVERY path (versus/course can never be
disturbed — the classifier never arms them); taint = `SESSION_ACTIVE` +
`set_training_taint(side)` on Started (the set_marker pattern —
load-bearing for the t=0 edge, where `notify_subscribers(0)` fires
BEFORE the store); fail-open `warn_scrub_once` (one WARN/song),
cooling/transient drops at debug level; scrub latches cleared in
`clear_session_state`. 5 new host tests (306 total).
Step-6 as landed (details in the plan's "As landed" note + the three
task records): BAR-MODE strip (offline-ramp colors, live row selection,
live palette walk parked behind USE_LIVE_PALETTE=false), overlay
(yellow cursor / A+B lines with start/end fallbacks / always-on blue
veil / 0.4-scale clamped readout), placement row OFF/LEFT/RIGHT default
OFF = the sole visibility control (wire mod_training_progress_pos,
bemani-buddy migration 015, round-trip verified), loop fires AT the
user's end marker, strip widget created before overlay widgets (z =
creation order), per-song stem cache + paired release.
Task-03 shape as landed (post fix round): `strip_synth::section_veil`
(pure, veil = either marker set — the maintainer's amendment superseding
the loop gate); overlay on strip_hud's render pump (track/blue-tint
veil/A green/B red — B falls back to the timeline end when unset,
line tops clamped inside the strip — /yellow outlined cursor
ImageWidgets + m:ss readout TextWidget at scale 0.4, center-clamped
on-screen; marker asset = repo-shipped 4x4 outline-baked PNG via
asset_loader, UV
center-row trick for tall stretches; fail-open ladder); TIMELINE
PLACEMENT enum row (RIGHT default; PersistMode::Full -> wire
mod_training_progress_pos; per-song entered-side latch; the strip's own
x follows it; LEFT/RIGHT value ribbons are the game's STOCK atlas
entries — never generate stock ribbon names); option textures
generated; bemani-buddy migration 015 +
protocol/db/handler plumbing + 5 verbatim tests.
Task-02 shape as landed (post bar-mode rework): `src/mods/training_mode/
strip_hud.rs` — GAMEPLAY-entry arm → first-judge-dispatch snapshot
(side, decoded_notes, chart_end_raw; per-note rows by CALLING the
resolved `arrow_row_selector` with the live RTTI-validated ArrowRenderer
at actor+0x148 — the task text's +0x138 was WRONG (SpotRenderer;
actor-init decompile); palette by walking the live ArrowPalette manager
at actor+0x130: table POINTER at mgr+0x28, rows {1,2,3,4,8} with the
8..15→slot-7 fold, phase mgr+0x18, evaluate = vtable slot 1 →
0xAARRGGBB) → background synthesis (**BAR MODE** — R7 second amendment:
`strip_synth::render_strip_bars`, colors from the live rows via
`row_bar_color`; no sheet/lightning reads on the live path; 4096-tick
measure guidelines via seek::raw_for_display; doubles → 8 columns;
per-song cache PNG `data_mods/_cache/training_hud/training_strip_<gen>.png`)
→ asset_loader load/poll/bind on a toast-style render-thread pump → ONE
reused ImageWidget, visible iff Resolved && training_session_active &&
GAMEPLAY; teardown releases + deletes per song; generation-tokened;
`DDR_STRIP_FAULT` ∈ selector|palette|synthesis|load; fail-open ladder,
one WARN/song. Signatures: `arrow_row_selector` AOB (unique +
byte-identical on 0324/0616/0721) + `derive_strip_hud_anchors` (RTTI
vtables). Pure layer additions: `render_strip_bars`/`row_bar_color`/
`BAR_H`/`SHOCK_MINE_RGBA` + the retained noteskin rasterizer and
sheet/shock-strike/mine-lightning extraction (future views). Host tests
298. The bar-mode A/B used an EMBEDDED casr Single Expert fixture in the
temp harness (transient SSQ parse; expert charts live in `casr_3.ssq`
chunk 0x0314) — nothing machine-specific committed.
Housekeeping notes: Steps 1–4 committed as Checkpoint 4 (`e682e0c`);
Step 5 + the amendment docs uncommitted on top. Session-B regression
legs (Autoplay / quick-fail / rate — shipped policy) deliberately
DEFERRED by the maintainer to Step 9's end-of-feature regression pass.
Step-5 shape as landed: task-01 = `score_guard` `TRAINING_TAINT` (one-way
per song, per side) + `ASSIST_TICK_TAINT` (level-written, the autoplay
model), OR'd into `is_stage_suppressed` after the existing terms;
`reset_song_taint` clears TRAINING only (NEVER assist-tick — cross-mod
scene-callback ordering), `reset_session` clears both. 6 host tests
(serialized on a taint test lock, order-independent), 249 → **255**.
task-02 = producers: bounds' two SESSION_ACTIVE latch sites (loop latch
pre-digest-gate + row engagement → entered side), `set_marker(which,
side)` (gesture side threaded from on_input_event), the training mod's
`on_song_reset` subscriber (registered enable()/removed disable()), and
assist_tick's GAMEPLAY-entry level-write (both sides every song; disable()
also level-writes false — staleness guard). **One design-driven deviation
from the task text** (recorded in the task record): the subscriber
re-taints `t > 0 || training_session_active()` — the bare t>0 predicate
would launder a triple-1 restart during a B-engaged song (the trigger
wipes the taint, the restart lands at t=0, but the truncated thresholds
persist across the in-place reset ⇒ the partial replay would submit).
Honest replays of untouched songs still submit (latch false —
LIVE-PROVEN, demo leg 7/8). Docs: assist_tick module doc + README
(assist-tick row + sanitised-logout row) + training_mode module doc.
Suppression umbrella as shipped (PUS deliberately NOT in scope — it is a
read-only observer, zero score_guard calls): Autoplay (per side) /
Quick Fail (both sides) / rate ≠ 100 % (ledger) / bound rows / LOOP ON /
marker gestures / seeks / Assist Tick (per side, level).
Known open item (non-blocking): assist-tick
claps miss the first 1–3 arrows of a SECTION grind pass (first play and
loops alike) — likely tick-cue start timing after rewrite/replay, not
the tick-list math.
Step-4 final shape (three cabinet-found corrections over the original
breakdown): LOOP ON PARKS the end cascade (CMA `+0x94` raised to the
sane-max sentinel at the loop latch, stock pair stashed; `+0x98` kept
stock) because `0x104A` is one-way song-scoped state that strikes the
lane furniture and breaks freeze scoring on later passes; fire bound =
`min(b_live?, +0x98) − 1000` on every reset path (raise failure ⇒
conservative below-`+0x94` bound); loop disarm restores the stash
(mandatory — parked cascade + no loop = the song could never end);
`reset_side_state` restores the DYNAMIC population counter `+0x19C` to
its snapshot-latched song-start baseline (the loop score-cap root cause
— also fixes the same latent quick-restart undercount); triple-5 =
clear-to-whole-song (maintainer decision, replaces restore-to-rows).

Resume protocol: read `implementation/plan.md` (checklist),
`design/detailed-design.md` (§4.1–§4.3 + §4.6–§4.7, §5, §6 — R2 carries
the 2026-08-14 timestamp amendment), research §§2–4, §5.4, §6, §8, and
the per-task records under
`.agents/scratchpad/2026-08-13-training-mode/<task>/progress.md`.

## Done

- **PDD (2026-08-13):** register D1–D17 all Accepted; design + plan Approved;
  RE docs committed (`docs/training_mode_research.md`,
  `docs/option_header_rows_research.md`); 3 Step-1 task files generated.
- **Step 1 / task-01 — identity passthrough serving:**
  `virtual_bank::plan_identity_bank` (both entries `passthrough_plan` — never
  `plan_entry(100)`); `binding::ServeMode` {Stretch, IdentityPassthrough};
  packed `{shift_blocks, lead_blocks}` mapping + epoch/applied handshake;
  `Binding::new_identity_passthrough` (4 KiB dummy ring, NO producer);
  mode-aware serving (`copy_mapped_main` lead/content/tail); generator remap
  restart at output 0 via `ring_rewind`. 7 tests, 182 → 189.
- **Step 1 / task-02 — training-arm lifecycle + mapping API:**
  `EligibilityInputs.training_arm` (classifier accepts an eligible 100 %
  entry when set; gates unweakened); identity arms suppress NO movies and
  their commits skip ledger/taint/movie-confirm (arm ≠ taint);
  `prepare_binding` identity split (no spawn); `identity-bind-refused` fault
  leg; `runtime::set_training_arm` / `set_content_mapping` +
  `BindingRegistry::set_active_content_mapping`. 7 tests, 189 → 196.
- **Step 1 / task-03 — mod skeleton + demo knob:**
  `src/mods/training_mode/mod.rs` (id `training-mode`, default enabled,
  `TRAINING_LEAD_MS = 2500`, integration_ready gate, standing arm request,
  TEMPORARY `DDR_TRAINING_TEST_SHIFT_MS` knob — removed in Step 2) +
  lib.rs/mods registration. **Task-02 addendum pulled in:** bind-time
  pre-shift (`BindContext.initial_mapping_ms`, applied pre-publication;
  `Binding::ms_to_blocks`; `runtime::set_initial_content_mapping_ms`) — the
  demo's never-audible-beginning needs the mapping before bank prepare's
  buffering reads. 1 test, 196 → **197/197**. All gates green
  (check / fmt / build.sh; release DLL at
  `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`).

- **Step 2 — seek-to-T + A/B gestures + restart-from-A (all 4 tasks;
  cabinet demo PASSED 2026-08-13, plan Step 2 ticked).** Per-task records
  under `.agents/scratchpad/2026-08-13-training-mode/`
  (`seeded-wsola-o1-seeks/`, `seek-math-and-record-transforms/`,
  `seek-to-t-transaction/`, `ab-markers-gestures-restart-from-a/` — all
  Status: Complete). Highlights:
  - task-01 seeded WSOLA (fresh stretch per shift>0 epoch, O(1);
    full-tail cyclic ALIGNMENT context — bare `None` starves the search
    ≤ ~75 %; epoch checkpoint invalidation; discards count into
    `frames_produced`).
  - task-02 pure seek math (`song_reset/` conversion; `seek.rs`:
    layout constants, `quantize_seek`, `anchor_tick`, `wall_ms`/
    `content_ms`, `rebuild_expectations`, R14 `neutralization_writes`;
    harness mount).
  - task-03 seek transaction (`judge_rebuild_anchor` AOB +
    `derive_judge_rebuild_trio` — SCAN_LIMIT 0xC0 after the attempt-1
    truncation finding, trio at match+0x37/+0x5F/+0x93;
    `control_message_actor_vtable`; `active_content_grid`/`_mapping`;
    nonzero-T `request_reset` with fail-closed gates + `AccumulatorPolicy`;
    `chart_end_raw`/`seek_available`/`current_raw_music_count`; t=0
    leftover-shift guard; wall-domain quantization at rate;
    lead-as-approach for delayed seeks).
  - task-04 markers + gestures (triple-4/5/6 = A/clear/B on the middle
    pinpad row — D3 amended; `bounds.rs` + `toast.rs` feedback toasts;
    restart-from-A via `active_section_start()` with
    `max(TRAINING_LEAD_MS, restart_delay)` lead; demo knob removed).
  - Cross-cutting findings banked: BmpfontSimpleString has TWO alignment
    fields (`+0xA8` horizontal per-line with engine-measured widths,
    `+0xAC` vertical — `TextWidget::set_alignment` fixed at the source;
    learnings.md entry).

## In flight

- **Step 5 — score containment: implementation complete (2026-08-14, both
  tasks via code-assist; gates green: harness 255/255, check, fmt,
  build.sh → release DLL at
  `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`). Cabinet +
  server demo pending — plan Step 5 unticked until it passes.**
  Per-task records: `score-guard-taint-sources/`,
  `wire-taints-and-suppression-matrix-demo/` (both Status: Complete
  (uncommitted)). Highlights:
  - task-01 (zero behavior change): `TRAINING_TAINT`/`ASSIST_TICK_TAINT`
    statics + `set_training_taint(side)` / `set_assist_tick_taint(side,
    on)` in score_guard; predicate + reset extensions; 6 host tests behind
    a shared `TAINT_TEST_LOCK` (poison-tolerant, clean-baseline restore —
    the suite stays order-independent). 249 → 255.
  - task-02: producers wired (bounds' loop-latch + row-engagement sites,
    `set_marker` side-threaded, the mod's `on_song_reset` subscriber with
    the design-driven `t>0 || training_session_active()` predicate —
    see the Status note; assist_tick GAMEPLAY-entry level-write + disable
    staleness guard); docs (assist_tick module doc score-suppression
    section, README assist-tick + sanitised-logout rows, training_mode
    module doc Step-5 paragraph). Enforcement path untouched.

- **Step 4 — LOOP SONG: implementation complete (2026-08-14, all 3 tasks
  via code-assist; gates green: harness 249/249, check, fmt, build.sh →
  release DLL at `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll`).
  Cabinet demo pending — plan Step 4 unticked until it passes.**
  Per-task records: `end-domain-converters-and-threshold-surface/`,
  `loop-song-row-and-early-natural-end/`, `loop-driver-and-step-demo/`
  (all Status: Complete (uncommitted)). Highlights:
  - task-01 (zero behavior change): `seek::display_for_raw`/
    `raw_for_display` (the game's note-vector interpolation replicated;
    full vector incl. control notes; linear extrapolation past the edges
    from the nearest distinct-key pair; ±1 round-trip slop; degenerate
    vectors ⇒ None); `section_math::EndPolicy`+`end_policy` (LOOP ON
    never WriteThresholds); `song_reset` surface —
    `CMA_CHART_END_DISPLAY_OFFSET = 0x94`, `chart_end_thresholds(side)`,
    `set_chart_end_thresholds(display, raw)` (all actors,
    refuse-before-write), `decoded_notes(side)` (plan_side_rebuilds'
    validation MIRRORED — the shipped transaction untouched). +5 tests.
  - task-02: `training_loop_song` row ("LOOP SONG", bool, default OFF,
    `PersistMode::Session`, registered after the bound rows; PLAIN
    session row — survives song switches, no seeder/digest, card-in
    resets it); per-side atomics + per-song `LOOP_LATCHED` (entered side,
    latched at resolution BEFORE the digest gate — valid even for
    stale-stamped bound rows); session-active + gameplay-entry
    `rows_engaged` gain loop-ON; LOOP OFF apply
    (`bounds::apply_end_policy` behind the pure
    `section_math::apply_action` table): stash stock thresholds once →
    `set_chart_end_thresholds(display_for_raw(notes,b), b)` at
    resolution/gesture-B/triple-5, restore the stash when b clears;
    failures ⇒ one WARN per song, thresholds untouched (natural end).
    3 new PNGs generated (`seop_item_training_loop_song`,
    `seop_image_training_loop_song_{off,on}`) — **must be deployed to
    the cabinet's `data_mods/` before the demo**. +1 test.
  - task-03: `section_math::loop_fire_bound(b?, t94_raw?, t98, margin)`
    (min of present terms − margin; ≤0 ⇒ None; +1 test); driver loop leg
    (`loop_step`): compute-once fire bound (recompute when the live
    section end moves; `+0x94`→raw conversion failure drops the term
    with one WARN — t98−margin still guards step 5), per-frame count vs
    bound → the SHIPPED `request_reset(active_section_start().unwrap_or(0),
    TRAINING_LEAD_MS, Zero, None)`, cooling latch until the count
    rewinds (one in-flight; absorbs the prepare window), Refused ⇒ one
    retry ⇒ disarm + one WARN; initial-compute degeneracy disarms
    (bound ≤0 / behind the count / start at-or-above); the 60 s driver
    timeout RE-SCOPED to the pre-anchor phase (checked below the
    work-done exit, skipped while the loop leg is live — grinds run
    indefinitely; wedged startups still time out).

- **2026-08-14 R2 relabel note:** the task highlights below describe the
  rows as originally shipped (SKIP FIRST / OMIT LAST, relative seconds);
  the 2026-08-14 deploy-log entry supersedes them — the rows are now
  absolute timestamps (`training_start_time`/`training_end_time`, mutual
  MIN_SECTION nudge, END-cap sentinel). The mechanism underneath
  (Session persistence, publication clamp, resolution, driver, adjust) is
  unchanged.
- **Step 3 — implementation complete (2026-08-13, all 3 tasks; gates green:
  harness 232/232, check, fmt, build.sh → release DLL). Cabinet demo
  pending — plan Step 3 unticked until it passes.** Per-task records:
  `persist-mode-session-and-bound-rows/`,
  `song-publication-clamp-and-bound-resolution/`,
  `driver-and-silent-skip-first-start/` (all Status: Complete). Highlights:
  - task-01: `PersistMode::Session` (the Explore question resolved WITHOUT
    a maintainer halt: `None` has NO card-in reset anywhere — its doc claim
    was fiction, now corrected; Session = None's exclusions + a real
    reset). Exhaustive matrix METHODS on PersistMode (`saved_to_network`/
    `loaded_from_network`/`json_cached`/`session_scoped`) replace the ad-hoc
    comparisons — the old `!= None` save filter would have MISFILED Session
    onto the wire. Card-in reset = `FrameworkState::reset_session_values`
    (kernel, host-tested) driven from the SONG_SELECT card-in drain
    (renamed `apply_pending_card_in_resets`) beside the rate-ledger reset.
    Rows `training_skip_first`/`training_omit_last` (0–599 s, fine 5 /
    coarse 30, default 0) registered from the mod's enable path
    (row-injection unavailability degrades to gestures-only, unlike SONG
    SPEED's refuse-enable); per-side atomics + accessors in bounds.rs.
    4 label/preview PNGs generated (maintainer must deploy).
  - task-02: `song_rate::selected_song` publication cell (seqlock; ever
    torn-read impossible — host-tested incl. a frozen mid-write state) fed
    from the create detour top on EVERY dance-bank create (both degraded +
    full paths; publish-nothing on failure keeps the previous publication);
    `section_math` pure layer (`MIN_SECTION_MS = 5_000`,
    `effective_bound_seconds` use-time audio cap, `resolve_bounds` §4.2
    formula with 0-sentinels, b ≥ chart_end ⇒ none); bounds.rs row-derived
    latches + PENDING resolution (actors don't exist at the scene-change
    instant — the driver retries) + triple-5 now RESTORES row-derived
    bounds (zero rows degenerate to Step-2 clear) + `SESSION_ACTIVE` latch
    (rows OR gestures) for the driver arm and Step 5's taint.
  - task-03: `perform_adjust` factored out of `perform_seek`
    (behavior-identical, gates re-run on the refactor alone; 0x1043 stays
    in the shared core per research §5.4);
    `song_reset::adjust_run_to(t_q, lead_wall)` (seek-identical gates, no
    accumulator block, ends in notify_subscribers) +
    `first_anchored_frame()` (DPS step 7 + actors step 4 + anchor +0x160
    ≠ 0); pre-shift arming from rows (entered side via new
    `stage_records::side_entered`; wall conversion at DESIRED percent —
    `section_math::pre_shift_wall_ms`; refreshed at row edits + scene
    25/26); `training_mode/driver.rs` render-thread generation-tokened
    loop: resolution retries + the ONE-shot adjust deriving t_q from the
    LIVE mapping read-back (`seek::blocks_to_wall_ms`, host-tested — the
    desired-vs-committed epsilon never reaches the anchor) + the §6
    fallback ladder (missed pre-shift ⇒ WARN + stop/replay seek; no
    binding ⇒ WARN + plays from 0).

## Deploy & test log

- **2026-08-15 Step-7 round 4 (FINAL): ALL LEGS CONFIRMED — plan Step 7
  TICKED, task record CLOSED.** Loop lead-in clap-silent with the first
  post-A clap exact; LOOP ON death = instant restart at A with the
  gauge restored; LOOP OFF death still fails out; quick-fail exits the
  grind; everything from rounds 1–3 still green.

- **2026-08-15 Step-7 round 3 + two loop fixes (demo build v4
  INSTALLED).** Round 3: FF/RW perfect, claps resume immediately ✅.
  Fixes: (a) **reset clap floor** — the assist-tick subscriber stores
  the reset target; commits mute the served track before the target's
  clap position (`tick_track_positions` on the one-element target →
  block-aligned `mute_head_bytes`, new 4th param on
  `rewrite_tick_wave`) so the loop's 2.5 s lead is clap-silent while
  the note AT A keeps its clap (strictly-before = consumed = muted,
  matching the rebuild); also fixes the R15 skip-first lead leak.
  (b) **LOOP ON bypasses death** — at the loop latch
  `bounds::arm_death_bypass` stashes + sets the engine's own
  instant-death gate (`GamePlayActor+0x2B7`; 20260721 decompile: the
  `0x103C` STEP_GAME_OVER advance AND the DPS finish-poll death arm
  both require it 0), so a gauge death latches `m_isDead` without
  ending the run; the loop driver fires the normal reset on
  `any_actor_dead()` (`death revive` tag) and the shipped completion
  block (flag clears + gauge snapshot restore) revives. Disarm/session
  end restore the stash; refusals fall back to stock death.
  Quick-fail still exits (step ≥ 5 unaffected). Gates green (harness
  306/306, check, fmt, build); v4 installed.

- **2026-08-15 Step-7 round 2 (ALL legs) + assist-tick clap-resume fix
  (demo build v3 INSTALLED).** Round 2: every leg correct ✅ except
  claps taking 2–3 s to resume after FF/RW. Root cause (pre-existing,
  also the checkpoint-4 loop "first few beats" gap): assist_tick's
  `on_song_reset` subscriber did `clear()` + full rebuild —
  re-synthesizing the entire clap track (background mix + ADPCM
  encode) after EVERY completed reset, though the retained `encoded`
  track is content-authored and a reset only moves the wall anchor.
  Fix (src/mods/assist_tick.rs): subscriber demotes a committed track
  `Playing → Ready` (keeps encoded/m0/rate); the next judge dispatch
  re-commits shifted to the live count (the shipped rewind mechanism)
  — claps resume within a frame. The Ready arm now waits while
  `restart_skip_ms < 0` (future-dated delayed restarts commit exactly
  when the count reaches m0; fresh-song path unchanged, skip ≥ 0).
  Gates green (harness 306/306, check, fmt, build); DLL installed.
  Round-3 spot-check: claps after FF/RW + after a loop iteration.

- **2026-08-15 Step-7 round-1 feedback + fixes (demo build v2
  INSTALLED).** Round 1 (partial legs): FF/RW both correct ✅; two
  maintainer amendments implemented same-day: (1) scrub lead REMOVED —
  `request_reset(t_q, 0, Zero, None)` (was TRAINING_LEAD_MS = 2.5 s of
  silent lead-in per skip); the scrub is now a pure timeline adjuster
  (music-player FF/RW; rewind-to-start = the instant restart path);
  TRAINING_LEAD_MS unchanged for restart-from-A/loop. (2) NEW
  `scrub_indicator.rs` — RW/FF double-triangle icons (repo-shipped PNGs
  `data_mods/training_mode/tex/training_scrub_{rw,ff}.png`, generator
  `scripts/gen_training_scrub_icons.py`) flashed left/right mid-height
  with the toast fade on every dispatched scrub; asset_loader chrome
  model, primed at enable, dismissed at disable. Gates re-run green
  (harness 306/306, check, fmt, build); DLL + the two PNGs copied to
  `$DDR_WORLD_INSTALL`. Remaining round-2 legs: rate interaction, end
  clamp, rewind-to-start, one-in-flight, score containment, fail-open,
  indicator look/placement sign-off.

- **2026-08-15 Step-7 demo build INSTALLED (awaiting maintainer demo).**
  FF/RW scrobbling implemented (task record:
  `.agents/scratchpad/2026-08-13-training-mode/ff-rw-scrobbling/`); all
  gates green (harness 306/306 incl. 5 new scrub tests, check, fmt,
  `./build.sh`); fresh DLL copied to `$DDR_WORLD_INSTALL`. Demo legs
  (plan Step 7 paragraph + the task record's checklist): 7/9 skip by the
  configured increment at 100% AND at rate; claps/judging aligned after
  every skip; FF near the end clamps (no early end cascade; with a live
  B, clamps margin below B); rewind within one increment of 0 restarts
  from the song start; rapid presses / press mid-loop-reset dropped (one
  in flight); scrubbed song suppressed, untouched song submits; optional
  out-of-range config key → one INFO. Launch with a CLEAN environment
  (no stale `DDR_*_FAULT` — the Step-6 lesson). Expected log lines:
  `TrainingMode: scrub FF/RW -- <from> ms -> <to> ms`, then
  `SongReset: seek started`/`seek complete`.

- **2026-08-14 Step-5 cabinet demo (maintainer-run, 17:44–17:56): SESSION
  A FULL PASS — plan Step 5 TICKED. Log-verified end to end by the
  agent.** Seven songs, one carded P1 session, Autoplay OFF:
  1. 17:45:56 assist tick alone → 17:46:42 `savekind=2 save SUPPRESSED`
     (×3 sender retries — the shipped retry shape).
  2. 17:47:12 tick OFF next song → 17:47:49 `save allowed
     (stage_taint=false, logout_taint=true)` — level semantics exact;
     the sticky logout taint from song 1 persisting is designed behavior.
  3. 17:48:22 bounds alone (END 10 s, LOOP OFF): `bounds resolved a=0,
     b=9999` → `early natural end armed` → SUPPRESSED ×3.
  4. 17:50:04 marker gesture alone (`B set at 11906 ms (triple-6)`) →
     SUPPRESSED ×3.
  5. 17:51:28 restart-from-A (`A set at 14228` → `seek complete t_q
     14225`) → SUPPRESSED ×3 — the trigger's `reset_song_taint` wipe +
     the subscriber's t>0 re-taint PROVEN LIVE.
  6. 17:53:06 LOOP ON sectioned grind (65–70 s, cascade parked, 5
     iterations at fire bound 68999, quick-fail out) — no stage save
     fired at all (the fast-path fail skips results; nothing reached the
     server; the latch taint stood had one fired).
  7. 17:54:57 honest replay: untouched song, triple-1 in-place reset at
     t=0 → 17:55:31 `save allowed (stage_taint=false)` — the
     session-active-gated subscriber correctly did NOT taint (the
     deviation predicate's clean side, live-proven).
  8. 17:55:51 card-out: `logout sanitiser: P1 records virginised` →
     `logout save SANITISED — scores stripped (Removed), profile
     forwarded`.
  Zero WARN/ERROR from Step-5 machinery beyond the intended suppression
  lines; server-side verified by the maintainer. **Session B (shipped
  Autoplay/quick-fail/rate regression + PUS-submits check) deliberately
  deferred to end-of-feature regression testing (maintainer decision —
  shipped policy untouched by this step).**

- **2026-08-14 Step-5 demo script (as run; kept for the record).**
  Suppressed per-stage saves log
  `score_guard: ... savekind=2 save SUPPRESSED`; run carded-in with
  **Autoplay OFF**:
  1. **Assist tick alone**: ASSIST TICK ON, ordinary song, play clean →
     per-stage save suppressed (no score server-side). Next song with it
     OFF submits normally (the taint is level-written per song).
  2. **Bounds alone / LOOP OFF partial results**: SONG END TIME below the
     song length, LOOP OFF → early natural end + partial results →
     suppressed.
  3. **Marker gesture alone**: untouched rows, mid-song triple-4 (or
     triple-6) → suppressed.
  4. **Restart-from-A**: set A, triple-1 → the replayed run stays
     suppressed (the trigger's `reset_song_taint` wipe is re-covered by
     the on_song_reset subscriber at seek completion).
  5. **LOOP ON grind**: any grind (whole-song or sectioned) → suppressed.
  6. **Clean song still submits**: same session, untouched song (rows
     default/seeded, no gestures, assist tick OFF) → score submits
     normally, server-verified.
  7. **Honest replay stays clean**: untouched song, triple-1 mid-song,
     finish the replay → submits (the subscriber only re-taints t>0 or
     session-active resets).
  8. **Regression**: one Autoplay-ON song + one quick-fail → the shipped
     suppression behavior unchanged.
  9. **Card-out** after the tainted legs: the logout save is sanitised,
     not suppressed — profile/option changes persist server-side, scores
     stripped.

- **2026-08-14 Step-4 demo attempt 7 (baseline-restore build,
  maintainer-run): FULL PASS — plan Step 4 TICKED.** Log-verified:
  whole-song LOOP ON on blli looped 3–4 times, every post-reset pass
  summary `pop [438, 18, 0]` (baseline restored) and **score 1,000,000
  at every fire**; mid-grind gestures all correct (A/B sets recompute
  the bound; a B set behind the cursor fired next frame — the accepted
  "end here"; triple-5 → whole-song grind resumed at the 126718 bound);
  sectioned LOOP OFF regression clean (early natural end armed at
  16:29:27, stock tail). Diagnostic sampling stripped from the driver
  afterwards (`song_reset::judge_diag` RETAINED as a read-only
  diagnostic surface — useful again for Step 5's containment checks);
  final gates green on the stripped build.

- **2026-08-14 Step-4 demo attempt 6 (diagnostic build, maintainer-run):
  triple-5 clear fix CONFIRMED; score-cap mechanism CAUGHT EXACTLY —
  fix shipped, diagnostic left in for the verification run.**
  Diag data (blli, LOOP ON whole-song): baseline `pop [438, 18, 0]`;
  during pass 1 `+0x19C` grew 0→34 in exact lockstep with the grade-6
  judge count (`grades[6] − freeze_ok == pop[2]` at every sample — the
  engine's freeze-head "arm" conversions, each adding +1 numerator AND
  +1 denominator; pass-1 summary: grades[6]=52, D=490, score exactly
  1,000,000). Pass-2 summary: `pop [438, 18, 34]` PERSISTED (the engine
  never rewinds `+0x19C`) while the conversions' one-shot state lives
  in the NOTE vector (also never rebuilt) — grades[6]=18 only, score
  456×100,000/490 = **930,610 exactly**. Same skew explains the earlier
  989,150 (smaller partial conversion count under the old build).
  **Fix (song_reset, shared with quick restart — the same latent
  undercount shipped with the in-place restart):** the per-song gauge
  snapshot now latches `+0x19C`'s song-start baseline
  (`SideSnapshot.note_pop_baseline`, read at the probe — before the
  music, so pre-conversion) and `reset_side_state` restores it after
  every rebuild. This makes replayed passes score-identical to natural
  starts; the engine's own start-of-song reserve undershoots the record
  count the same way (vector append growth is its everyday path), so
  ordering/allocation behavior is exactly stock. `GPA_DYNAMIC_POP_OFFSET
  = 0x19C` documented in song_reset. The `TrainingMode[diag]` lines stay
  in for ONE more cabinet run — expect: per-loop pass summaries with
  `pop [438, 18, 0/…baseline]` right after each reset and score
  1,000,000 at every whole-song fire; then the diagnostic gets removed.
  Gates: harness 249/249, check clean, fmt, build.sh → fresh DLL.

- **2026-08-14 Step-4 demo attempt 5 (cascade-parked build,
  maintainer-run): leg 2 PASS on parked build; leg 5 score cap CHANGED
  (930,610, lane intact); triple-5 partially broken. Diagnostic build
  shipped.**
  - **Leg 5 score cap — deterministic scoping done, mechanism pending
    the diagnostic:** `judge_submit` = `FUN_18005fd30` (20260721;
    matched via the shipped `judge_submit` pattern). Money score =
    `floor(weights·200000 / (D·10) …) · 10` with **D = sum of the three
    note-population counts at GPA `+0x194/+0x198/+0x19C`** (the same
    trio the rewind worker sums for its reserve — read-only there).
    blli Expert (SSQ parse, docs/ssq_format.md): 438 rows (8 jumps) +
    19 freeze ends = 457 events; last freeze end tick 364544 = the
    stock `+0x94` exactly (display domain = SSQ ticks, confirmed).
    Exact-fit solutions: pass 1 = 1MM ⇒ (J=457, D=457); old build's
    989,150 ⇒ **(J=456, D=461)**; parked build's 930,610 ⇒
    **(J=456, D=490)** — the DENOMINATOR grows on replayed passes
    (+4 stock-thresholds vs +33 with `+0x94` raised), plus exactly one
    event unjudged. The rewind worker never writes the trio ⇒ another
    site increments them; the raise value scales the growth ⇒ the
    grower reads `+0x94`. **TEMPORARY diagnostic added** (remove with
    the fix): `song_reset::judge_diag(side)` (populations + per-grade
    judges + freeze-OK + judged events + score + combo);
    `driver::diag_sample_populations` logs EVERY population-sum change
    with the music count (cap 60 lines/song) and
    `diag_pass_summary` logs the full state + live thresholds right
    before each fire (`TrainingMode[diag]:` prefix).
  - **Triple-5 "only clears the start marker" — ROOT CAUSE FOUND +
    FIXED:** `clear_live_bounds` (and latent in `clear_markers` since
    Step 2, masked by double boundary clears) used
    `A_MS.swap(0) > 0 || B_MS.swap(0) > 0` — the `||` SHORT-CIRCUITS
    past the B swap whenever A was set. Log-confirmed: after the 14:40:21
    clear the loop kept firing at the OLD bound 73998 with target 0
    (B never cleared, A gone). Both sites now run both swaps
    unconditionally (`|`).
  - Also reported (not yet addressed, tracked): assist-tick claps
    missing for the first 1–3 arrows of a SECTION grind pass (first
    playthrough and loops alike; tick kicks in after). Investigate
    after the score cap — likely tick-cue start timing after
    rewrite/replay, not the tick-list math.
  Gates: harness 249/249, check clean, fmt, build.sh → fresh DLL.
  **Re-test (diagnostic build): whole-song LOOP ON on blli, let it loop
  2–3 times, then grab the `TrainingMode[diag]` lines — they name the
  counter that grows, when (music count), and by how much.**

- **2026-08-14 Step-4 demo attempt 4 (maintainer-run, legs 2/5/7 +
  triple-5): leg 2 PASS; leg 5 still broken (new root cause); triple-5
  semantics rejected. All addressed same session; fresh DLL awaiting
  re-test.**
  - **Leg 2 (LOOP OFF early end): PASS** — song ended at the truncated
    time, results showed correctly, no freeze (the scene-callback
    deadlock fix confirmed live).
  - **Leg 5 (whole-song loop): pass 1 reached 1MM + MARVELOUS FULL
    COMBO, but crossing `+0x94` fired `0x104A` and its fallout is
    one-way song-scoped state:** the lane-notice actor STRUCK the lane
    furniture (filter/background/guidelines) permanently, and every
    subsequent pass capped deterministically below 1MM (e.g. 989,150)
    with a full marvelous combo — the freeze-OK score class stops
    completing under the latched "chart over" state. The morning's
    t=0-path exemption (firing past `+0x94`) was therefore WRONG.
    **Fix — park the cascade:** LOOP ON's apply now RAISES `+0x94` to
    the sane-max sentinel (stock pair stashed; `+0x98` kept stock so
    marker/seek clamps and the fire bound stay honest) — the cascade
    sits at its normal mid-song step forever, every pass plays and
    scores the FULL chart, the lane survives, seeks stay legal on every
    path. Fire bound simplifies to `min(b?, +0x98) − 1000` everywhere
    (log: `cascade parked`); raise failure ⇒ WARN once + the
    conservative below-`+0x94` bound. Loop DISARM now restores the
    stash and drops the latch (`bounds::on_loop_disarmed` — mandatory:
    a parked cascade with no loop would soft-lock the song's natural
    end). Known cosmetic trade: no MFC celebration during grinds
    (`0x104A` never fires); normal play untouched. `apply_action`
    reworked: ArmLoop ⇒ `RaiseThresholds` (host test updated).
    Research §4.3 refinement REWRITTEN accordingly; design §4.2
    amended.
  - **Triple-5 (maintainer decision): now clears the LIVE bounds to
    none** — the rest of the run plays the whole song (LOOP OFF
    restores the stock end via the end policy; LOOP ON grinds
    whole-song). The Step-3 restore-to-row-values semantics is retired;
    rows still re-resolve next song. Toast: "Cleared markers".
  Gates: harness 249/249, check clean, fmt, build.sh → fresh DLL.
  **Re-test: leg 5 (expect `end cascade parked` + `cascade parked`
  bound lines, 1MM + intact lane on EVERY pass, MFC animation absent),
  triple-5 mid-song (section → whole song), leg 3 (B-set → triple-5 →
  stock end restored under LOOP OFF), legs 7/8.**

- **2026-08-14 Step-4 demo attempt 3 (maintainer-run, legs 2 + 5):
  BOTH FAILED — both root-caused and fixed same session; fresh DLL
  awaiting re-test.**
  - **Leg 2 (LOOP OFF early natural end, log_freeze.txt): the early end
    itself worked** (`early natural end armed -- raw=14997,
    display=32761 (stock raw=133125)`, cascade fired, CLEARED banner,
    scene 28→29) — then the game FROZE at the banner, unresponsive even
    to the test menu. Root cause: **self-deadlock in the scene-change
    callback.** `scene_manager` fired callbacks while HOLDING the
    `SCENE_MANAGER` mutex (its "outside the lock" comment was false),
    and `clear_session_state`'s threshold-restore condition called
    `current_scene()` — a reentrant lock of a non-reentrant mutex from
    the frame thread. The `&&` short-circuit hid it until the first run
    with `THRESHOLDS_WRITTEN` (this leg is the only path that sets it).
    Log signature: every earlier-registered mod's exit line printed,
    zero TrainingMode lines, then total frame-thread silence with spice
    api/keepalive still logging. **Fixes (both):** scene_manager now
    snapshots `Arc` callbacks under the lock and fires OUTSIDE it (the
    song_reset SUBSCRIBERS pattern — kills the whole deadlock class);
    `clear_session_state` no longer reads `current_scene()` at all (the
    restore attempt is fail-closed on `set_chart_end_thresholds`' own
    live-actor gates). learnings.md entry added.
  - **Leg 5 (whole-song loop at 175 %, log_whole.txt): looped ~1 s
    early** — score reached ~955k, the finale arrows never crossed the
    receptors (`fire bound 114356 = min(raw(+0x94)=115356, +0x98=118570)
    − 1000`; iterations fired at 114363/114365). Root cause: the
    clamp-below-BOTH-thresholds rule (breakdown decision #1) exists to
    protect the SEEK path's cascade gate (refuses at step ≥ 4) — but the
    whole-song loop resets via `request_reset(0, …)` = the t=0 DELAYED
    path, which has NO cascade gate; `0x104A` at `+0x94` is the game's
    normal post-last-arrow event (cosmetic — it fires at every natural
    song end), and the cascade parking at step 4 afterward only
    threatens seeks. **Fix (decision #1 refined to its true scope):**
    the `+0x94` term joins the fire-bound min ONLY when the fire will
    take the seek path (live section start > 0); the t=0 path clamps on
    `min(b?, +0x98) − 1000` — this song now grinds to 1MM with ~2.2 s of
    outro before each loop. Recompute triggers gain A-presence flips
    (`LOOP_BOUND_FROM_A_PRESENT`); the bound log line now names the
    path. Documented edge: a gesture-A set mid whole-song grind AFTER
    the cascade passed step 4 makes the next fire a refused seek →
    retry → disarm + one WARN → natural end at `+0x98` (the §6 ladder).
  Gates re-run: harness 249/249, check clean, fmt, build.sh → fresh
  DLL. **Re-test legs 2 + 5 on the new build, then legs 3/7/8.**

- **2026-08-14 Step-4 demo attempt 2 (fixed build, maintainer-run
  03:21–03:33): PASS on the sectioned-grind legs — log verified
  end-to-end by the agent.** Four LOOP ON sessions, same song
  (chart end 118570 ms), all clean:
  1. **100 % (identity), START 50 / END 55** — pre-shift 50000 ms,
     silent start adjusted (a=49998, lead 2499, no stop/replay), LOOP
     latched, `loop fire bound 53999 ms (b Some(54999), display raw
     Some(115356), raw 118570, margin 1000)` — the min/margin math
     exact. **~60 iterations over a 7-MINUTE grind** (03:23:08 →
     03:30:13, one every ~6.6 s = 4 s content + 2.5 s lead), every
     iteration `seek started → loop iteration → seek complete` in
     12–34 ms. No timeout (the pre-anchor re-scope proven live on a
     grind 7× the old 60 s limit). Ended by triple-3 quick-fail fast
     path; markers cleared at scene change.
  2. **175 % (gen 2)** — pre-shift 28571 ms (= 50 s·100/175, wall
     conversion exact), fire bound 53995, iterations ~4.6 s apart
     (content at 1.75× + lead ✓).
  3. **50 % (gen 3)** — pre-shift 100000 ms ✓, fire bound 54000,
     iterations ~10.5 s ✓.
  4. **60 % (gen 4), START 40 / END 50** — pre-shift 66666 ms ✓,
     a=39998 / b=50000, fire bound 49000, iterations ~17.5 s ✓;
     session ended by game shutdown.
  **Aggregates: 75 loop iterations / 75 seek completes (1:1), zero
  `end cascade at step` refusals (the clamp held every iteration —
  AC 5), zero disarms, zero driver timeouts, zero TrainingMode WARNs.**
  Fires landed at count 53999–54016 vs bound 53999 (within a frame).
  Benign observation: `song_rate: STUCK READ` WARNs (500–725 ms,
  several per rate session, right after iterations) — each loop seek
  opens a new mapping epoch and the engine's read-ahead briefly
  outruns the restarted WSOLA producer; every read completed, zero at
  identity, no audible impact reported (the 2.5 s lead absorbs it).
  The shipped streaming diagnostic, not a training bug.
  **Legs still to run before ticking plan Step 4:** LOOP OFF early
  natural end + partial-stats results (leg 2), threshold restore on
  clear (leg 3), whole-song loop re-test on the FIXED build (leg 5 —
  attempt 1's failing leg), mid-grind gestures (leg 7), LOOP OFF
  regression (leg 8).

- **2026-08-14 Step-4 demo attempt 1 (maintainer, partial — LOOP ON +
  whole song + 175 %): FAIL, fixed same session.** The song played
  through once and ran to results — the loop disarmed at song start:
  `degenerate section (count 304644 ms / start 0 ms vs fire bound
  123911 ms)`. Root cause: the initial fire-bound compute ran the
  moment the resolution completed, BEFORE the run's first `0x1044`
  anchor — the unanchored `+0x178` count reads as the raw frame tick
  (minutes-since-boot scale), tripping the `count >= bound` degeneracy
  disarm. Rate-independent; the computed bound was correct. Fix:
  `driver::loop_step` now gates the initial compute on
  `first_anchored_frame()` + a count-credibility check
  (`count < chart_end_raw` — the `+0x178` cache can lag the anchor by a
  frame), with a three-state `LoopState` so the anchor wait stays under
  the 60 s pre-anchor timeout while a live grind remains exempt. Gates
  green (harness 249/249, check, fmt, build.sh → fresh DLL).
  **Re-test needed: the whole demo script below on the NEW DLL.**

- **2026-08-14 Step-4 cabinet demo (maintainer-run): PENDING.**
  Pre-reqs: deploy the fresh release DLL AND the 3 new loop-row PNGs
  (`seop_item_training_loop_song.png`,
  `seop_image_training_loop_song_{off,on}.png`) into the cabinet's
  `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/`.
  NOTE (plan-approved ordering): score containment is Step 5 — LOOP OFF
  partial results and loop-practiced plays WILL submit during this demo
  (research §4.4: the truncated record undercounts). With Autoplay ON the
  shipped `score_guard` suppression lines are Autoplay's policy, not
  Step 4.
  Demo script:
  1. MODS tab shows LOOP SONG (OFF/ON, default OFF) under the bound
     rows; it keeps its value across song switches within the session
     and resets to OFF at card-in.
  2. **LOOP OFF early natural end**: LOOP OFF + SONG END TIME below the
     song's length (e.g. 60 on a 2-min song) → at 1:00 the game runs its
     OWN stock tail (FAILED/CLEARED banner → results with the partial
     stats) — no scene surgery, no hang. Log:
     `TrainingMode: early natural end armed -- thresholds raw=... ms,
     display=... (stock raw=... ms)`.
  3. Threshold restore: same setup, then mid-song triple-5 after a
     triple-6 B-set... simpler leg: set END row, enter, mid-song
     triple-5 (restores row bounds — B unchanged, write re-applied);
     for the RESTORE path set no rows, triple-6 mid-song (log shows the
     armed line), then triple-5 (clear) → log
     `TrainingMode: stock end thresholds restored (section end cleared)`
     and the song plays to its natural end.
  4. **The grind (headline)**: LOOP ON + START 30 / END 60 → the run
     resets in place to 0:30 behind the 2.5 s approach every time it
     reaches ~0:59 — combo/score/gauge zeroed, claps re-aligned —
     indefinitely; triple-3 (or triple-1) exits as shipped. Log per
     iteration: `TrainingMode: loop iteration -- reset to 30000ish ms at
     count 59000ish ms (fire bound ... ms)`; once at entry:
     `TrainingMode: loop fire bound ... ms (b Some(...) ms,
     display-threshold raw Some(...) ms, raw threshold ... ms, margin
     1000 ms)` + `TrainingMode: LOOP SONG latched for this song`.
  5. **Loop-whole-song**: LOOP ON, no bounds → the song loops back to
     0:00 strictly before its ending banner ever shows, repeatedly.
     Verify seeks keep working across many iterations (the cascade never
     trips — no `end cascade at step` refusals in the log).
  6. Rate composition: leg 4 at 75 % and 125 % SONG SPEED.
  7. Mid-grind gestures: triple-6 moves B → next iteration fires at the
     new end (log shows a recomputed fire bound); triple-4 moves A → next
     reset lands at the new A.
  8. LOOP OFF regression: an untouched song and a bounds-only song behave
     exactly as the Step-3 demo (no loop lines in the log).
  9. Long-grind soak: leave a grind running well past 60 s — the driver
     must NOT log `driver timed out` (timeout is pre-anchor-scoped now).

- **2026-08-14 Step-3 refinement spot-check (maintainer-run): PASS —
  "everything works perfectly". Plan Step 3 TICKED.** Per-song row-range
  clamping + the 200 cap confirmed on-cabinet alongside the earlier
  full-leg pass.
- **2026-08-14 Step-3 refinements (post-PASS, maintainer-requested):**
  (a) `BOUND_ROW_MAX_S` 600 → **200 s** (no DDR chart runs longer).
  (b) **The ROW RANGES themselves are re-bounded per highlighted song**
  (the maintainer's option-bound clamp: the stepper must not even be able
  to express a timestamp past the song). New framework primitive
  `custom_options::set_scalar_bounds(id, min, max)` (kernel
  `FrameworkState::set_scalar_bounds` — live-mutable scalar bounds;
  press-time stepping clamp AND the row's position marker read the
  registry per frame, so the new range is effective the same frame;
  out-of-range stored values clamp with deferred callback dispatch;
  refuses unknown/enum/inverted — 4 kernel host tests). The highlight
  seeder now sets END over `[MIN_SECTION, seed_end]` and START over
  `[0, seed_end − MIN_SECTION]` before the value re-seed; registration
  baselines follow the same shape at the 200 cap. Card-in reset restores
  the abstract 200 default verbatim (may sit above the live max for ONE
  frame at select — the stamp-clear → seeder round-trip self-heals; the
  200-detector for the stamp-clear is now unreachable by user edits, as
  the stepper cannot reach the cap on a re-bounded row). Gates: harness
  **242/242** (+4), check clean, fmt, build.sh → fresh DLL.
- **2026-08-14 Step-3 demo attempt 2 (song-scoped build,
  maintainer-run): PASS — all legs confirmed working** (seeded honest
  END timestamps, nudge coupling, silent start at 100 % and at rate,
  END-below-length resolution, song-switch isolation, triple-5 restore,
  card-in re-seed, regressions). Plan Step 3 tick awaits the refinement
  spot-check above.
- **2026-08-14 R2 second amendment — song-scoped bounds (maintainer UX
  decision; design R2 amended again):** END now tracks the HIGHLIGHTED
  song: a select-scene watcher (driver `select_step`, per-frame publication
  poll) re-seeds both rows on every highlighted-song change — START 0,
  END = `seed_end_seconds(len)` (audio length rounded UP to the 5 s step:
  ≥ the real end, never truncates, honest to within 5 s; ≥600 s songs seed
  at the cap sentinel). The menu always opens showing the highlighted
  song's timeline, and an untouched menu never carries one song's bounds
  into another — in either direction (the naive clamp-down-only version
  would have CUT a longer song after a shorter one; START is included for
  the same reason — a leftover START would silently skip into the next
  song's start). Card-in reset clears the song stamp (detected in the END
  callback at the abstract 600) so the watcher re-seeds for the new
  player. **Fast-confirm race closed by digest coherence**
  (`selected_song::digests_coherent`, fail-open without publications):
  the pre-shift is digest-stamped (`set_initial_content_mapping_ms` gained
  the stamp arg; mapping-then-digest write order ⇒ torn pairs can only
  decline) and the create detour binds via
  `initial_content_mapping_coherent(created_digest)`; the resolution
  declines mismatched rows (resolves as defaults); the driver skips the
  adjust on the same test. `rows_engaged` now compares END against the
  seeded value (seeded rows keep zero footprint). Gates: harness
  **238/238** (+2: seed rounding, digest coherence), check clean, fmt,
  build.sh → fresh DLL.
- **2026-08-14 R2 timestamp relabel (maintainer UX decision at Step-3
  validation; design R2/§4.1/§4.2 amended in place):** the bound rows are
  now absolute timestamps — `training_start_time` "SONG START TIME (s)"
  (0–600, default 0) and `training_end_time` "SONG END TIME (s)" (0–600,
  default 600 = the "whole song" sentinel; END at/past the chart end
  resolves to natural end) — no mental subtraction from the song length.
  Row-level coupling keeps the window ≥ `MIN_SECTION_S` (5 s): raising
  START nudges END up; lowering END bumps START down (mutual nudge via
  `section_math::nudge_end_after_start`/`nudge_start_after_end`, sibling
  written with `set_value_silent` — no dispatch, no recursion; the nudged
  row repaints the same frame via the scalar slot-7 render tick, which
  re-reads the registry per frame). Effective audio clamp now applies to
  START only (flooring END to whole audio seconds could fabricate a
  phantom section end — END is governed solely by the chart-end
  normalization). Old ids/PNGs retired (Session rows serialize nothing —
  no migration). Gates: harness **236/236** (+4: nudge unit/property
  tests), check clean, fmt, build.sh → fresh DLL.
- **2026-08-14 Step-3 demo attempt 1 (laptop/CrossOver, maintainer-run,
  PRE-relabel build): silent start PASS.** Maintainer-confirmed SKIP FIRST
  worked correctly; log verified end to end: `driver armed (pre-shift
  60000 ms, bounds resolution pending)` → `row-derived bounds resolved --
  a=59997 ms, b=93568 ms (chart end 118570 ms, side 0, skip 60 s /
  omit 25 s)` (a = 60 000 and b = 118 570 − 25 000 block-quantized ✓) →
  `SongReset: run adjusted in place -- t_q 59997 ms, lead 2499 ms (1
  actor(s), no stop/replay)` → `silent skip-first start adjusted`. ZERO
  WARN/ERROR from any Step-3 machinery; zero-footprint confirmed live
  (two earlier songs with default rows: no driver-armed lines). The
  observed `score_guard ... SUPPRESSED (stage_taint, logout_taint)` lines
  were Autoplay's shipped policy (maintainer had Autoplay ON throughout —
  laptop testing), not Step-3 behavior. Legs still to validate on the
  relabeled build: row labels/stepping + the mutual nudge, non-100 %
  SONG SPEED leg, triple-5 restore toast, card-in reset of the rows
  (START→0, END→600 + `custom_options: card-in reset` line), no
  `mod_training_*` wire fields server-side.
- **2026-08-14 Step-3 demo attempt 2 (relabeled + song-scoped build):
  script (executed same day — PASS recorded above).** Pre-req: the
  cabinet texture swap (remove 4 old / deploy 4 new PNGs). Demo script:
  1. Card in → highlight a song → MODS tab shows SONG START TIME 0 /
     SONG END TIME ≈ the song's length rounded up to 5 s (log:
     `TrainingMode: bounds seeded for the highlighted song -- end N s`).
     Highlight a different song with the menu closed, reopen → END
     tracks the new song's length.
  2. Nudge check: raise START to within 5 s of END → END follows up on
     screen; lower END below START + 5 → START bumps down.
  3. START 60 → silent start, music enters exactly at 1:00 (same log
     signature as attempt 1, now `start 60 s / end ... s`).
  4. Same at a non-100 % SONG SPEED (e.g. 75 %).
  5. END set below the song end (e.g. 90) → `b=90000`-class value in the
     resolution line (consumption arrives in Step 4).
  6. Song-scoping: set bounds on song A, switch to song B WITHOUT
     touching the menu, play B → B plays whole from 0 (no silent start,
     no section end); switch back to A → bounds re-seeded (defaults).
  7. Triple-4 mid-song, then triple-5 → "Restored markers" toast.
  8. Card out → in on the same song → rows re-seed (START 0 / END = song
     length; `custom_options: card-in reset` in the log); no
     `mod_training_*` fields server-side.
  9. Regression: untouched rows bit-for-bit normal (no driver armed
     lines); triple-1 on a START-set song restarts from A.
  NOTE (plan-approved ordering): score containment is Step 5 — demo
  scores WILL submit (and Autoplay's own taint suppresses saves while
  it's on, as seen in attempt 1).
- **2026-08-13 Step-2 demo attempt 3 (cabinet, maintainer-run): PASS —
  plan Step 2 ticked.** Full flow at 50 %, 100 %, AND 175 %: set marker →
  restart-from-A a couple of times → clear marker → restart-at-0 — all as
  expected; toast exactly centered (native +0xA8 alignment). Log
  evidence: trio derived at boot (`judge_rebuild_clear/reserve/rebuild @
  +0x60990/+0x608D0/+0x60D40`); seeks completed in 22–64 ms stop→anchor;
  the wall-domain math verified numerically — 100 %: A=34813 → t_q 34810,
  shift 11993 blocks; 175 % (rate 6214401/3551104): A=44861 → shift 8831
  (≈ 0.57×); 50 %: A=28634 → shift 19730 (≈ 2×); lead 861 blocks
  ≈ 2499 ms everywhere; triple-5 clears + song-change clears both firing.
- **2026-08-13 Step-2 demo attempt 2 (cabinet, maintainer-run): PASS on
  the mechanics — restart-from-A fully working, loop-at-A working; toast
  centering approximate (estimate-based).** Follow-up fix same session:
  decompiled the bmpfont render fn (FUN_18020cca0 via the
  `render_function` match) — the line desc has TWO alignment fields:
  `+0xA8` = HORIZONTAL per-line alignment (offsets each line by its own
  pre-measured width; 1 = center via ×−0.5, `DAT_18035a700`), `+0xAC` =
  VERTICAL block alignment (what `set_alignment` wrote — the "CREDIT: 0"
  question answered: the native centering mechanic exists and this is
  it). `TextWidget::set_alignment` now writes +0xA8; the toast reverted
  to `set_position(640, 630)` + Center — exact centering for ANY text,
  no width estimation. learnings.md corrected. Gates green; fresh build
  awaiting a quick visual re-check.
- **2026-08-13 Step-2 demo attempt 1 (cabinet, maintainer-run): FAIL —
  two findings, both fixed same session.**
  - (a) Restart-from-A always fell back to restart-at-0. Log:
    `[-] judge_rebuild_trio -- fewer than three calls after the anchor` at
    boot → `SongReset: seek-to-T unavailable` → every seek refused. Root
    cause: the derivation's `SCAN_LIMIT = 0x60` truncated the E8 scan —
    the trio sits at match+0x37/+0x5F/+0x93 (the rebuild call at +0x93 was
    past the window; my own window arithmetic, not a shape problem — the
    anchor matched uniquely at +0x5BB04 and the CMA vtable resolved).
    Fix: `SCAN_LIMIT = 0xC0` (next unrelated call is at +0xE0; the scan
    stops at three targets regardless).
  - (b) Toast rendered LEFT-anchored at x=640 (left edge at screen
    center): the native alignment field does NOT anchor the string about
    its position (likely per-line alignment within multi-line blocks).
    Fix: default left alignment + computed anchor
    `x = 640 − chars·14.8·scale/2` (the watermark's px/char calibration);
    finding recorded on `TextWidget::set_alignment`'s doc + in
    `.agents/learnings/learnings.md`.
  - Everything else on script behaved: gestures latched + logged (A at
    19026/24051 ms), triple-5 cleared, fallback ladder worked exactly as
    designed (WARN + shipped restart-at-0), t=0 in-place resets completed
    in 17–34 ms, quick-fail fast path unaffected.
- **2026-08-13 Step-2 demo attempt 3 (toast-centering re-check): PENDING**
  (build ready; deploy is the maintainer's). Only the toast placement
  needs eyes — mechanics passed attempt 2. If the toast is now exactly
  centered (native +0xA8 alignment), plan Step 2 can tick. Remaining
  demo legs if not yet covered: 75 %/125 % rate restarts-from-A.
- **2026-08-13 Step-1 demo (cabinet, maintainer-run): PASS.**
  - (a) mod on, 100 % song, no knob: fine — identity arm/commit, audio
    normal, score submitted.
  - (b) `DDR_TRAINING_TEST_SHIFT_MS=60000` at 100 %: exactly the designed
    audio-only proof — 2.5 s silent lead, content entered at ~1:00 (true
    beginning never audible), chart played from its own start (60 s
    off-sync vs audio, expected — clock/notes are Step 2/3), final 60 s
    silent arrows (the silent tail, expected).
  - (b') same knob at **90 % playback speed**: the song took noticeably
    longer to start. Root cause (analysis): the bind-time pre-shift on a
    STRETCH binding restarts production at output 0 with the feed
    positioned at `shift` — WSOLA has no checkpoint below the target, so
    `Feed::positioned_at` produce-and-discards ~54 s of stretched output
    (~2.4× realtime ⇒ ~20 s stall) before the first content packet exists.
    **Open item for Step 2/3** (see Deviations).
  - (c) not explicitly re-tested (trusted); maintainer will run a full
    stock regression at the end.
  - Plan Step 1 ticked.

## Deviations & open questions

- **Step-4 deviations (details in the per-task records):** converter
  edge behavior = linear extrapolation from the nearest distinct-key
  pair (clamping would fabricate boundary equality; documented in
  seek.rs); `decoded_notes` MIRRORS `plan_side_rebuilds`' validation
  instead of factoring it (the seek transaction is cabinet-validated
  shipped code — untouched); `apply_action`'s defensive
  ArmLoop+written ⇒ Restore arm (unreachable — loop latches before any
  write and cannot toggle mid-song); loop-bound degeneracy disarms apply
  to the INITIAL compute only — a mid-grind gesture-B behind the cursor
  recomputes and fires next frame (the loop-ON mirror of LOOP OFF's
  accepted "end here"); `clear_session_state` attempts a stash restore
  when thresholds are written and the scene is still GAMEPLAY (the
  mid-song mod-disable edge; boundary clears skip it via the write
  gates).
- Host-test count after Step 4: **249** (242 + 7).
- **Step-5 deviations (details in the per-task records):** the
  on_song_reset subscriber's re-taint predicate is
  `t > 0 || bounds::training_session_active()` (not the task text's bare
  `t > 0`) — design §4.1's per-song session-active latch is the approved
  taint predicate, and the bare form would launder a triple-1 restart
  during a B-engaged song (taint wiped at the trigger, reset lands at
  t=0, truncated thresholds persist across the in-place reset ⇒ the
  partial replay would submit, violating R5 + task AC 1); assist_tick's
  disable() level-writes the taint false for both sides (the producer's
  scene callback disappears — without this the last clapped song's taint
  goes stale and suppresses an honest later song); README's
  sanitised-logout policy row also updated (consistency — it enumerated
  the taint sources).
- Host-test count after Step 5: **255** (249 + 6).

- **Step-3 deviations (details in the per-task records):** `RegisteredOption`
  gained a `default_value` field (the reset needs it); the reset kernel
  returns option ids (dispatch panic-suppression needs them); the PersistMode
  matrix lives as exhaustive METHODS on the enum (one source of truth) rather
  than per-site match conversions; preview panels added beyond task-01's
  letter (house style); 0x1043 stays in the shared adjust core (research
  §5.4's pair — perform_seek byte-identical); mod-side ineligibility clearing
  of the pre-shift covers versus only (course/event stay the classifier's —
  Step-1's no-duplicated-predicate model; an unconsumed mapping is inert);
  the driver also retries bound resolution with no pre-shift (OMIT LAST
  alone needs the b_ms log).
- Mapping storage: ONE packed AtomicU64 + epoch/applied pair (vs. two bare
  AtomicU64s in the design data model) — torn-pair guard + closes the
  set→producer staleness window through the existing deferral machinery.
- Bind-time pre-shift plumbing moved from "Step 3 will drive it" into Step 1
  (task-03 needs it for the demo); ms-domain on `BindContext`, converted at
  bind time where the format is known.
- Standing-request eligibility model: the mod sets `set_training_arm` at
  enable/disable; the scene-26 classifier applies the (unchanged) gate set —
  no duplicated session predicate in the mod.
- Known cosmetic: any arm enables the diagnostic bank-event timeline → a few
  INFO log lines per song while the mod is enabled. Revisit later if noisy.
- **RESOLVED (maintainer decision 2026-08-13, recorded as the design §4.5
  amendment + research §5.2 note): O(1) seeded WSOLA seeks.** `shift > 0`
  mappings in pitch-preserved mode are served by a FRESH stretch seeded at
  the shift-mapped source position — frame count/duration exact by
  construction (`output_total − shift` frames), byte-level alignment
  deliberately unpinned ACROSS epochs (imperceptible over the cue
  stop/replay discontinuity). Within-epoch determinism still required;
  generator checkpoints must be INVALIDATED per epoch. Supersedes the
  rolling-checkpoints idea (loops/FF-RW each open a new epoch → O(1)).
  Step-2 task-01 implements it.

## Key facts for a cold resume

- **Host tests do NOT run via `cargo test` in the repo root** on this host
  (retour is x86-only; aarch64 compile fails — also the cause of stale
  rust-analyzer "no such field" noise). Use the temp-dir `#[path]` harness
  mirroring `scripts/validate_song_playback_speed.sh`'s mounts — this
  session's copy:
  `/var/folders/31/yq10yrk557l1q0wyb1nx4vg40000gp/T/opencode/ddr-host-harness`
  (`cargo test --quiet`; deps: once_cell). Recreate from the validator
  script's `main.rs` heredoc (~line 235) if lost — and ALSO add the mounts
  the heredoc does not have: (a) `services::song_reset` as a wrapper mod
  containing `#[path] pub mod seek;` (src/services/song_reset/seek.rs)
  + `#[cfg(test)] #[path] mod seek_tests;` (seek_tests.rs);
  (b) inside the harness's `services::custom_options` wrapper, `#[cfg(test)]
  #[path] mod persist_matrix_tests;` (Step-3 task-01) and `#[cfg(test)]
  #[path] mod scalar_bounds_tests;`;
  (c) a `mods::training_mode` wrapper mounting
  `src/mods/training_mode/section_math.rs` (its tests are inline) AND
  `src/mods/training_mode/strip_synth.rs` (Step 6 — inline tests too);
  (d) strip_synth's dependencies: `image = "0.25"` in the harness
  Cargo.toml, a crate-root `#[macro_export] log_warn!` stub
  (format-and-discard), `core::arc` mounted as `core { pub mod arc; }`,
  and `services::avs_layeredfs::avslz` mounted inside the services
  wrapper (arc/avslz bring 3+9 inline tests of their own).
  `selected_song(_tests)` ride the song_rate/mod.rs mount for free.
- Host-test count after Step-6 task-03: **301** (Step 5: 255; delta =
  34 strip_synth (incl. bar mode, reverse, section_veil) + 3 arc +
  9 avslz). The temp harness additionally carries the embedded casr
  Single Expert fixture + strip_experiment.rs (A/B renders — not
  counted, env-gated).
- Gates order: harness `cargo test` → `cargo check --target
  x86_64-pc-windows-msvc` → `cargo fmt` (whole crate) → `./build.sh`.
- Mapping semantics (block units, main entry's served-stream grid): virtual
  block `v < lead` ⇒ silent; else stream block `v − lead + shift`; silent
  tiling past content end. Layout/header NEVER changes (engine parses it
  once per bank — research §5.1). `Binding::ms_to_blocks` is the B(T)
  conversion (floor).
- Identity pin: without a training-arm request, 100 % never arms (pinned by
  tests); identity commits carry no taint/ledger/movie
  (`training_identity_arm_commits_without_taint_ledger_or_movie`).
- No commits: the maintainer handles all git.
- Step-2 approved decisions: spanning-freeze neutralization IN (R14);
  `request_reset` gains `AccumulatorPolicy` (Zero now, Keep reserved);
  rebuild trio derived from the 0x1044 handler's call sites (fail-closed
  to Refused); score containment for seeks/markers deliberately waits for
  Step 5 (plan-approved ordering — flagged in the task-04 demo notes);
  the `DDR_TRAINING_TEST_SHIFT_MS` knob is REMOVED in Step-2 task-04.
