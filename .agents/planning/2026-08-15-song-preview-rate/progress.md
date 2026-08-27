# Progress: Real-Time Rate Preview at Song Select

Updated: 2026-08-16
Status: FEATURE COMPLETE — all 6 plan steps ticked. Deploy #3 (forensics
build) passed the full-session regression (multiple sessions: gameplay,
card-out, every song's preview rate-bound; sticky refusal did NOT
recur). Uncommitted on the tree: the log-hygiene fix + parse-forensics
instrumentation + the preview stuck-read threshold bump (all
gates green).
**NEXT ACTION:** None required. Watch item: if a sticky
`UnsupportedProfile` refusal ever recurs, the log's "preview parse
forensics — …" line names the exact parse failure + the song (see the
classification key in the restart-executor scratchpad record). Optional
follow-up: move the completed planning dir to `_archive/` when
convenient.

Resume protocol: read `implementation/plan.md` (approved, 6 steps) →
`design/detailed-design.md` (approved, amended §Components 5) →
`research/preview-retrigger-re.md` (§6 inventory, §8 watchdog addendum,
§9 the validated four-build signature matrix).

## Done

- Deploy #2 PASSED (2026-08-16; committed as b76873c "Make playback
  speed adjustment also affect music previews on the songwheel in real
  time"): maintainer confirmed in-game; log verified — all four
  signatures + both derived vftables resolved at boot, "restart
  derivations resolved" at enable, ~15 clean live-edit restarts
  (debounced, correct rates in both DSP modes), the PREVIEW PLAY
  WATCHDOG re-arming WSOLA previews exactly as designed (generations 1,
  5, 9, 10, 11, 33–36, 38 …), chain-probe INFO, 38 preview generations
  all reclaimed, zero silence-fills/stuck-reads (the ~552 ms max
  deferrals ARE the WSOLA first packet the watchdog covers). Two WARNs,
  both understood: (a) one "restart declined — loader chain failed to
  resolve" at scene-25 ENTRY — the profile load seeds the persisted
  rate through `on_change` → `request_refresh`, firing the executor
  before any preview loader exists; benign but it consumed the
  once-per-class chain WARN latch → fixed post-deploy (below); (b) one
  `UnsupportedProfile` preview bind refusal — the shipped strict
  `parse_song_bank` rejecting one song's bank, the designed fail-open
  path (that song previews stock), not preview-specific.
- Post-deploy log-hygiene fix (2026-08-16, UNCOMMITTED, gates green —
  validator 245 / windows check / fmt / build.sh): `resolve_loader`
  split into `resolve_loader_detail() -> Result<LoaderChain,
  ChainDecline>` — `Absent` (no derivations / wrong scene / no
  TS/child/View / NO LOADER INSTALLED — nothing playing; expected,
  SILENT) vs `IdentityMismatch` (a non-null object whose first qword is
  not the derived vftable — real layout drift; keeps the latched WARN).
  `execute_restart` consumes the detail; the watchdog and chain probe
  keep the reason-less `resolve_loader()`.

- Plan Step 6 docs COMPLETE (2026-08-16, uncommitted): published
  `docs/song_preview_pipeline_research.md` (the preview-pipeline RE +
  the rate-binding/restart/watchdog mechanisms + the four-build
  signature table, file-relative addresses per steering conventions);
  AGENTS.md gained the "Song-select preview rate" feature row; README's
  Song Playback Speed entry's stale "preview keeps playing at normal
  speed" sentence replaced with the new behavior (+ the ~0.5 s
  pitch-preserved late-start caveat). Final host regression pass green
  (validator 245 / windows check / fmt / build.sh). Step 6's cabinet
  regression demo rides deploy #2.

- Plan Step 5 implementation COMPLETE (2026-08-16, uncommitted; gates
  green — validator 234 → 245): NEW `input_manager::on_frame` per-frame
  callback API (dispatched before the ark gate, panic-contained);
  `RefreshCell` 150 ms debounce + supersession (pure, host-tested;
  `Idle/Pending/SceneCleared/Superseded/Fire`); `request_refresh`
  un-stubbed (selected-song generation latch, atomics-only);
  `RestartIo` seam + `run_restart_sequence` (stop → unreg XSB→XWB →
  create XWB→XSB, abort-without-re-arm on create failure; ordering
  host-pinned); `init_restart` gained the 5th pointer (the PATCHED
  wavebank_unregister entry — detour-prelude retire; `GenericDetour::
  call` would bypass it); windows executor on the frame callback
  (preconditions: chain + `loader_sane` + rows ∈ {0,5,6,8} + cue `_s`;
  once-per-class WARN latch; one INFO per restart) + the PREVIEW PLAY
  WATCHDOG (produced ≥ `min(start+64KiB, end)` + failed-latched +
  file-id match ⇒ re-arm, ONE retry per preview generation). Task
  record + deploy-#2 checklist:
  `.agents/scratchpad/2026-08-15-song-preview-rate/restart-executor/`.
- Plan Step 4 COMPLETE (2026-08-16, uncommitted; host gates green —
  validator 232 → 234, windows check, fmt, build.sh): four new
  `SignatureDefinition`s in `core/signatures.rs` (`audio_loader_ctor`,
  `selectmusic_view_ctor`, `cue_handle_stop`,
  `sound_bank_create_router`) — each validated EXACTLY ONCE on all four
  builds via Ghidra (per-build match table + annotated byte authority:
  research §9; note the View vftable is the ctor's SECOND LEA,
  `4C 8D 1D` disp at match+30 — the first LEA is an inner interface
  vftable stored at +0x28); `derive_preview_restart` (uniqueness
  re-check + RIP-decode + in-module/slot-0 vftable validation, publishes
  `audio_loader_vftable` + `selectmusic_view_vftable`); `preview.rs`
  restart section (all-or-nothing `init_restart` stash from the mod's
  `Mod::init`, `restart_available`, game-thread `resolve_loader` with
  both vftable identity gates + the RE §1.3 field constants, pure
  host-tested `loader_sane`, post-publish `probe_loader_chain` →
  drain-reported one-shot per outcome class); mod enable() reports the
  restart half's availability. Demo (cabinet log lines) rides deploy #2.
  Task: `.agents/tasks/2026-08-15-song-preview-rate/step04/`.
- Plan Step 3 host half COMPLETE (2026-08-15, uncommitted): NEW
  `services/song_rate/preview.rs` (pure `qualify` matrix + feature gate +
  scene-exit force-retire + `maybe_bind_preview` windows glue) wired into
  the create detour's bind closure (preview branch on every Stock
  gameplay outcome; transaction never sees it), drain publish-INFO latch
  + preview refusal WARN, `ensure_maintenance_drain`, mod enable/disable
  wiring, `request_refresh` stub stamps from both change callbacks.
  Validator 227 → 232. Task record:
  `.agents/scratchpad/2026-08-15-song-preview-rate/preview-bind-branch/`.
- Plan Step 2 COMPLETE (2026-08-15, uncommitted): `BindingRegistry`
  preview slot (`publish_preview`/`with_preview`/`retire_preview`),
  both-slot `retire_by_file` (the unregister prelude now retires preview
  bindings for free), host-tested routing (`any_bound` +
  `with_bound_for_file`, active-first), preview refusal mailbox,
  `next_preview_generation` (R15); `bound_verdict` rewired to the
  registry routing. Validator 222 → 227. Task record:
  `.agents/scratchpad/2026-08-15-song-preview-rate/preview-registry-slot/`.
- Plan Step 1 COMPLETE (2026-08-15, uncommitted): `StretchTarget::{Main,
  Side}` through planner + binding runtime. task-01 (planner): enum +
  `VirtualBankLayout.target_entry_index` + 3-arg `plan_virtual_bank`,
  Main-plan value pin + 3 Side tests (validator 214→218). task-02
  (runtime): `Binding` target/verbatim vocabulary (ring base, dispatch,
  identity guard, generator accessors), `prepare_binding(..., target)`
  with rate from the target entry, Side-target replay/verbatim/retire/
  wiring tests against independent oracles (validator 218→222). Working
  records: `.agents/scratchpad/2026-08-15-song-preview-rate/`.
- Step-1 task generation (2026-08-15): two approved tasks under
  `.agents/tasks/2026-08-15-song-preview-rate/step01/` (planner
  `StretchTarget`; target-aware Binding runtime). Breakdown decisions:
  debug_assert on `percent==100 + Side`; content mapping generalizes to
  the target grid; internal fields rename `target_*`/`verbatim_*`.
- PDD planning end-to-end (2026-08-15): register settled (D1–D15),
  readiness confirmed, design approved, plan approved. All artifacts in
  this directory; overview in `summary.md`.
- Static RE complete on 20260721 (+ 20260616 cross-check): preview
  pipeline decoded (SelectMusicSequence `+0xB8` → View `+0xC8` AudioPlayer
  → single AudioLoader; tick replays when `handle == −1`); R-A
  (gameplay-header safety) resolved via the 2026-08-05 cabinet timeline.

## In flight

- Nothing.

## Deploy & test log

- Deploy #3 (2026-08-16 13:1x–13:3x, forensics build): PASS — feature
  close-out. Every song's preview rate-bound (maintainer-confirmed;
  watchdog steady across generations 3–43 on multiple songs); the
  sticky refusal did NOT recur (zero refusals, zero forensics lines —
  instrumentation stays armed); TWO full card-out sessions in the log
  (gameplay commits, assist tick, score-guard suppression/sanitisation,
  EAM_EXIT) = Step 6's full-session regression. Step 6 ticked. Log
  noise found + fixed: 4 "STUCK READ (preview)" WARNs at age
  ~574–594 ms — the KNOWN ~583 ms WSOLA first-packet latency crossing
  the 500 ms diagnostic threshold; preview slot now uses a 1.5 s
  threshold (`PREVIEW_STUCK_READ_NANOS`), active slot keeps 500 ms.
- Re-deploy session (2026-08-16 12:3x, log-hygiene fix build): PARTIAL —
  restarts/watchdog/debounce all working; NO scene-entry WARN (the
  hygiene fix verified). INCIDENT: ~32 s sticky-refusal window
  (12:35:06–12:35:38) — every create for file_id 1654 refused
  `UnsupportedProfile` (the strict `parse_song_bank` rejecting that ROW
  INCARNATION's resident bytes while the ENGINE played the same bank
  fine from disk ⇒ stock previews, edits "not taking effect"). Parked
  on the song, each live-edit restart re-parsed the SAME resident row
  (16 refusals); healed on its own when the wheel moved away long
  enough for the row to release and reload from disk (same file id
  bound fine at 12:37:47). Same signature as deploy #2's single
  one-shot refusal (file 1606) ⇒ PRE-EXISTING, not the hygiene fix;
  the restart stress just makes one bad row incarnation sticky.
  Fail-open held throughout (stock previews, no crash, clean
  reclamation). Root cause UNKNOWN — the refusal WARN is
  diagnosis-blind → parse-forensics instrumentation added (below).
- Deploy #2 (2026-08-16): PASS — see the Done entry above for the
  log-verified detail. Steps 3–5 ticked in the plan. Not exercised in
  this session's log (song select only, 04:32–04:35): song confirm →
  gameplay (C5) and card-out — folded into the next visit's re-test
  alongside the log-hygiene fix.
- Deploy #1 (2026-08-16, md5 2f757e76…): PARTIAL PASS. Machinery
  end-to-end correct: 21 preview bindings, correct rates/DSP modes, clean
  reclamation, zero refusals, gameplay + training scrubs unaffected.
  BUG found: inconsistent SILENT previews in pitch-preserved mode — the
  WSOLA first-packet latency (~583 ms constant, output-frame-bound) races
  the AudioLoader's prepare-blind `se_play`; a Play landing in the
  unprepared window fails and the loader latches `failed` (never
  retries). Resample mode fully working. Engine RE (xactengine) ruled out
  short completions (poll site discards the byte count) and smaller
  initial reads (engine-fixed 64 KiB). RESOLUTION (maintainer-approved
  2026-08-16): accept ~0.6 s late start for pitch-preserved previews; fix
  reliability via the PREVIEW PLAY WATCHDOG on the Step-5 executor
  (re-arm the loader tick when the binding's initial window is produced
  and the loader sits failed-latched; one retry per preview generation).
  Design amended (§Components 5); RE addendum in
  `research/preview-retrigger-re.md` §8. Drain diagnostics extended to
  the preview slot (stuck-read + silence-fill) as a Step-3 amendment.

## Deviations & open questions

- Remaining before Step 6 ticks: re-test the (uncommitted) log-hygiene
  fix + the full-session regression (C5 gameplay + card-out) — the
  deploy-#2 session never left song select.
- One-song `UnsupportedProfile` preview refusal observed (04:34:47):
  the shipped strict parser's designed fail-open (stock preview for
  that song). Not a regression; if it bothers in practice, identify the
  song via the drain's file_id + the bank timeline and check its `_s`
  entry's format profile.
- Cabinet-measured items (not blockers): restart audio cleanliness
  (compressed stop→unregister gap), per-settle source memcpy cost,
  25 %/175 % start latency — all subjectively fine per deploy #2.
- Implementation deviation (documented in the Step-5 task record): the
  watchdog implements only the `failed`-latched re-arm — the design's
  "(or handle == −1 with the rows loaded)" parenthetical is a no-op case
  (the tick is still armed and fires on its own).
- Implementation deviation (documented in the Step-5 task record): the
  watchdog implements only the `failed`-latched re-arm — the design's
  "(or handle == −1 with the rows loaded)" parenthetical is a no-op case
  (the tick is still armed and fires on its own).

## Key facts for a cold resume

- Two halves: (1) create-detour preview branch binds `StretchTarget::Side`
  virtual banks at scene 25 (controlling side = exactly one entered side,
  desired ≠ 100); (2) input-poll executor restarts the playing preview
  150 ms after the last option tick (stop cue → unregister ×2 → create ×2
  via router → `loader.handle = −1`; the game replays the cue itself).
- Preview bindings live in a separate registry slot; they NEVER touch
  Q31/score/movie/lifecycle/XactSlots. Fail-open to stock everywhere.
- No config surface (maintainer decision D11). Versus ⇒ stock (D3, mirrors
  `IdentityReason::LocalVersus`). Debounce 150 ms (D4).
- The wheel-settle "selection changed" signal IS the detoured
  `wavebank_create` (same event feeding `selected_song`) — no new hooks
  anywhere in this feature.
