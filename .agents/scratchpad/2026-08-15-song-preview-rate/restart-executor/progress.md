# Task record: restart executor + debounce + watchdog (plan Step 5)

Status: Complete — deploy #2 PASSED (2026-08-16, maintainer-confirmed +
log-verified; committed as b76873c). One post-deploy log-hygiene fix is
uncommitted (see the bottom section).
Task file: `.agents/tasks/2026-08-15-song-preview-rate/step05/task-01-restart-executor.code-task.md`

## What landed

- `src/services/input_manager.rs`: NEW per-frame callback API —
  `on_frame(Arc<dyn Fn() + Send + Sync>) -> usize` +
  `remove_frame_callback(id)`; dispatched at the TOP of `poll()` (before
  the ark-exports gate, so frame consumers run even on boots where ark
  I/O init failed), snapshotted out of the lock, each dispatch
  `catch_unwind`-contained. Idle cost with none registered: one
  uncontended lock + empty check per frame.
- `src/services/song_rate/preview.rs`:
  - `RefreshCell` (cfg-free, host-tested): `{requested, stamp_nanos,
    settle_generation}`; `stamp_at` / `clear` / `has_request` /
    `poll_at(now, scene, settle) -> RefreshPoll {Idle, Pending,
    SceneCleared, Superseded, Fire}`; 150 ms `REFRESH_DEBOUNCE_NANOS`;
    documented benign stamp-vs-clear race (≤1 lost restart, fail-open).
    Timebase = `OnceLock<Instant>` epoch + elapsed nanos (repo idiom —
    no QPC wrapper exists).
  - `request_refresh` un-stubbed: feature gate + selected-song seqlock
    generation latch + stamp (atomics-only; option-callback legal).
    `set_feature_active(false)` clears the cell.
  - Pure helpers (host-tested): `row_state_loaded` (the tick's own
    {0,5,6,8} set), `cue_is_preview` (`_s` suffix, byte-wise),
    `watchdog_cover(start,end) = min(start+0x10000, end)`
    (`INITIAL_PACKET_BYTES` = the engine's fixed first ADPCM read).
  - `RestartIo` seam + `run_restart_sequence` (pure, host-tested):
    stop(handle≠−1) → unregister XSB→XWB (stock order) → create
    XWB→XSB (abort WITHOUT re-arm on first failure) → re-arm.
  - `init_restart` gained the 5th pointer: the PATCHED
    `song_rate_wavebank_unregister` entry (calling it flows through the
    installed detour whose prelude retires the preview binding —
    `GenericDetour::call` would BYPASS the detour). Still
    all-or-nothing.
  - `GameRestartIo` (windows): stashed stop/unregister/router fns +
    loader re-arm writes (`handle=−1`, `failed=0`).
  - `executor_frame` (windows, registered by `preview::init` on
    `input_manager::on_frame`): feature gate → `has_request` fast path →
    debounce poll → `execute_restart` (chain + sanity + rows-loaded +
    cue-shape preconditions, once-per-class WARN latch bitmask, one INFO
    per successful restart) → `watchdog_step` every frame.
  - `watchdog_step` (deploy-#1 fix): preview binding Active + produced ≥
    `watchdog_cover` + loader failed-latched + `xwb_id == file_id` ⇒
    clear `failed`/re-arm `handle=−1`; ONE retry per preview generation
    (`WATCHDOG_RETRIED_GENERATION`). `handle==−1 && !failed` needs no
    help (tick still armed).
  - `read_msvc_string` (windows; the song_reset pattern) for the cue at
    `loader+0x48`.
- `src/services/song_rate/preview_tests.rs`: +11 tests — RefreshCell
  matrix (debounce/coalesce/fire-once/scene-clear/supersede/clear),
  restart ordering + skip-stop + two abort positions under a recording
  mock, row-state / cue-shape / watchdog-cover helpers.

## Design decisions made at implementation time

- The design's watchdog parenthetical "(or handle == −1 with the rows
  loaded)" is a no-op case (the tick is still armed and fires on its
  own) — implemented the `failed`-latched case only, documented.
- Idle-frame cost trimmed: the selected-song seqlock read only happens
  when a request is pending (`has_request` fast path).
- Panic containment: outer net at the frame dispatch (input_manager) +
  the module's own `catch_unwind` around the two game-facing halves.

## Gates

- validator: 245 passed (234 → 245)
- `cargo check --target x86_64-pc-windows-msvc`: clean
- `cargo fmt` (whole crate): applied
- `./build.sh`: clean

## Deploy #2 checklist (maintainer)

Full design §Testing C1–C9 matrix, plus:
- Step-4 demo lines at boot: `[+] audio_loader_ctor` /
  `selectmusic_view_ctor` / `cue_handle_stop` /
  `sound_bank_create_router` + the two derived vftables; at enable:
  "preview restart derivations resolved"; while browsing at ≠100 %:
  "preview loader chain resolved … restart half ready".
- The deploy-#1 bug: pitch-preserved previews at 75 % reliably AUDIBLE
  across a long browse (~0.6 s late start is ACCEPTED); watch for
  "preview play watchdog re-armed the loader" INFO lines on the settles
  that would previously have gone silent. Resample previews unchanged.
- C2/C3: edit SONG SPEED / PRESERVE PITCH while a preview plays → ONE
  "preview restarted at the desired settings" INFO ~150 ms after the
  last tick, preview restarts from the beginning at the new settings.
- C4: back to 100 % → one restart to the literal stock preview.
- C9: edit then immediately confirm → no restart, gameplay untouched.

## Deploy #2 result (2026-08-16)

PASS. Log-verified: all four signatures + both derived vftables at
boot; "restart derivations resolved" at enable; ~15 clean live-edit
restarts (both DSP modes, rates tracking the scrubbed values); the
watchdog re-arming WSOLA previews across generations 1–38; chain-probe
INFO; all 38 preview generations reclaimed; zero silence-fills /
stuck-reads. Not exercised this session (log never left song select):
C5 song-confirm gameplay + card-out — folded into the next re-test.

## Post-deploy log-hygiene fix (UNCOMMITTED)

Deploy log showed one "restart declined — loader chain failed to
resolve at fire time" WARN at scene-25 ENTRY: the profile load seeds
the persisted 75 % through `on_song_speed_change` → `request_refresh`,
so the executor's first fire lands before any preview loader exists.
Benign fail-open, but it consumed the once-per-class chain WARN latch —
a REAL chain failure later in the session would have been silent.

Fix: `resolve_loader` split into `resolve_loader_detail() ->
Result<LoaderChain, ChainDecline>` with `Absent` (derivations missing /
wrong scene / TS/child/View missing-or-dying / loader unique_ptr empty
— nothing playing; expected, SILENT) vs `IdentityMismatch` (a non-null
object whose first qword is not the derived vftable — real layout
drift; keeps the latched WARN). `execute_restart` consumes the detail;
the watchdog and the chain probe keep the reason-less
`resolve_loader()`. Gates green (validator 245 / windows check / fmt /
build.sh). Re-test: no WARN at song-select entry with a persisted
non-100 % rate; restarts unchanged.

## Sticky UnsupportedProfile incident (2026-08-16 12:35, re-deploy session)

Log-hygiene fix VERIFIED (no scene-entry WARN). New incident: for ~32 s
(12:35:06–12:35:38) every create for file_id 1654 refused
`UnsupportedProfile` — `parse_song_bank` rejecting that ROW
INCARNATION's resident bytes while the engine played the same bank fine
from disk (stock previews; "edits not taking effect" while parked on
the song since each restart re-parsed the same resident row — 16
refusals). Self-healed when the wheel moved away long enough for the
row to release + reload from disk (same id bound fine at 12:37:47,
gen 51). Same signature as deploy #2's single one-shot refusal (file
1606, 04:34:47) ⇒ pre-existing; the restart loop just makes one bad
incarnation sticky. Fail-open held (no crash, clean reclamation,
correct rates before/after).

Instrumentation added (uncommitted, gates green — validator 245 /
windows check / fmt / build.sh):
- `preview.rs`: on an `UnsupportedProfile` preview refusal, re-run the
  header parse and stash a `ParseForensics` packet (file_id, path,
  buffer ptr/len, first 32 bytes, row load-state, `XwbError` Debug;
  try_lock cell, detour-legal — allocation OK, no logging).
- `runtime.rs`: drain emits one "preview parse forensics — …" WARN per
  cycle with the packet.

Classification key for the next occurrence: garbage magic in `head` ⇒
row-buffer lifecycle (freed/reused/mid-load); plausible header +
SegmentOutOfBounds/UnexpectedEof ⇒ truncated load or short size dword;
clean header + deep entry error ⇒ genuine format oddity in that song.
`reparse-succeeded` ⇒ the buffer changed between the two reads (load
race).

## Deploy #3 (2026-08-16 13:1x, forensics build) — FEATURE CLOSE-OUT

PASS. Every song's preview rate-bound; sticky refusal did NOT recur
(zero refusals/forensics — the packet stays armed for any future
occurrence); two full card-out sessions in the log (gameplay commits,
assist tick, score guard, EAM_EXIT) = Step 6's full-session regression.
All 6 plan steps ticked.

Post-deploy polish (uncommitted, gates green): preview-slot stuck-read
diagnostic threshold raised 500 ms → 1.5 s (`PREVIEW_STUCK_READ_NANOS`)
— the 4 observed "STUCK READ (preview)" WARNs at ~574–594 ms were the
KNOWN WSOLA first-packet latency (expected behavior the watchdog
covers), not actionable; the active (gameplay) slot keeps 500 ms where
a stuck read IS a real problem.
