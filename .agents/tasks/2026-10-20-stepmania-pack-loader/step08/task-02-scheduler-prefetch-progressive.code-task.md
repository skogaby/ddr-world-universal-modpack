# Task: Scheduler, prefetch and progressive banks

## Description
Replace Step 6's synchronous-on-open bank/jacket builds with a prioritised worker pool fed
by wheel-card events and the highlight, and make the full gameplay bank a progressive file
whose reads the LayeredFS read hook holds at the write watermark — so previews are ready
within the settle and stage load never waits on an encode.

## Background
Step 6 builds banks inside the FileManager open (one non-render I/O worker, E2): previews
take 85–145 ms (fine), full banks 1.1–3.2 s at stage load (a visible stall, and it blocks
every other FileManager read). Measured on the cabinet: the game opens the XSB+XWB at
`DancePlaySequence::onSetup` and plays ~5 s later; the XWB is read in 1 MiB chunks; the
wheel is 3 songs per row. The XWB layout is fully known from the frame counts
(`SongBankPlan`), so a pre-sized file with the header can exist before any audio is encoded;
`SongBankWriter` already appends blocks in file order with `written_watermark()`.

## Reference Documentation
**Required:**
- Design: .agents/planning/2026-10-20-stepmania-pack-loader/design/detailed-design.md (§3.3, §3.4, §4.7 scheduler/providers, §4.8 item 3, §5.6, §6)

**Additional References (if relevant to this task):**
- .agents/planning/2026-10-20-stepmania-pack-loader/progress.md (E2, read chunk sizes, build timings)
- src/services/stepmania/{providers,cache,taint}.rs (what exists), src/core/xact/song_bank.rs (`SongBankWriter`, `written_watermark`), src/services/avs_layeredfs/file_hooks.rs (open/read hooks; the Step 2 fd→family diag map is a shape precedent), src/services/selectmusic_highlight.rs (`highlighted_mcode`), src/services/input_manager.rs (`on_frame`) or scene_manager callbacks for dwell timing

**Note:** Read any document listed above before beginning implementation.

## Technical Requirements
1. `services/stepmania/scheduler.rs`: pool of `max(3, available_parallelism/2)` worker threads (below-normal priority via `SetThreadPriority`); job kinds `Preview(b)`, `Jacket(b)`, `FullBank(b)`, priority order highlighted-preview > visible previews (binding order) > highlighted jacket > visible jackets > speculative full bank; dedupe by `(kind, basename)`; `cancel_speculative(b)` when the highlight leaves a song after a 1.5 s dwell without commit; `PendingTicket { done: Arc<AtomicBool> }` for resolver waits; INFO per job with duration.
2. Triggers: `wheel_cards::on_card_bound` → enqueue Preview + Jacket for injected mcodes; highlight change (`selectmusic_highlight`, polled per frame or via an existing callback) → re-prioritise that song's preview/jacket to the head, start the speculative FullBank after 1.5 s of dwell; the full-variant bank request (gameplay load) → `FullBank` at top priority if not already complete.
3. Progressive full bank: `FullBank` creates `xwb/<b>.xwb.part` pre-sized to `plan.total_len()` with the header, then appends blocks via `SongBankWriter`, publishing the watermark to a registry `progressive::register(final_path, ProgressiveFile { watermark: Arc<AtomicU64>, done: Arc<AtomicBool> })`; on completion rename to the final path and mark done. The resolver serves a progressive file the moment it exists (it has the header) — the path it returns is the `.part` path until done, then the final path (both registered).
4. `file_hooks`: on open, if the resolved path is a registered progressive file, record `fd → ProgressiveFile`; in the read hook, for such fds ONLY (checked before anything else; one `AtomicBool` "any progressive open" fast-path when none), wait until `watermark >= cursor + nbytes` or `done`, bounded 5 s (then pass through); track the cursor per fd from read lengths (lseek is not hooked — document; the FileManager reads sequentially in 1 MiB chunks). Drop the entry when `done` and cursor ≥ len, or when the same fd value is re-opened.
5. Resolver changes: never block the open on a full bank (progressive file always available within ms of the job start — start it synchronously-create-then-encode-on-worker if not started); preview: if not ready and `!is_render_thread()`, wait on the ticket ≤ 300 ms, else serve the silent variant; jacket: if not ready wait ≤ 300 ms else placeholder arc (`.ph` marker → upgraded later, existing mechanism).
6. LRU unchanged (`audio_cache_mb`); `.part` files count toward it and are swept at boot (`sweep_tmp_files` extended).
7. Tests (pure, harness-mounted where possible): scheduler priority/dedupe/cancel over a fake worker; progressive watermark wait logic with a fake clock; cursor tracking. `./scripts/validate_stepmania.sh` legs.
8. Gates: `cargo check --target x86_64-pc-windows-msvc`, `cargo fmt`, `./build.sh`, harness green, path-leak grep.

## Dependencies
- Task 01 (`wheel_cards`).

## Acceptance Criteria
1. **Prefetch** — Given a vertical scroll onto a row of three injected songs, then three Preview jobs run concurrently and are complete before the 0.4 s settle on any of them (cabinet: no `built preview bank ... on open` stalls; log shows prefetch lines before the settle).
2. **No stage-load stall** — Given decide on an injected song with no cached bank, then the open returns within ms (progressive), the game starts on its normal schedule, and the read hook's wait count is logged (expect a few waits of < 1 MiB-chunk duration or none).
3. **Correctness** — Then the full song plays with no dropouts; the final file equals a `write_complete` build byte-for-byte.
4. **Quiet path** — Given no progressive file open, then the read hook costs one atomic load.

## Metadata
- **Complexity**: High
- **Labels**: scheduler, audio, layeredfs, threads
- **Required Skills**: Rust concurrency, hook discipline
- **Generated By**: code-task-generator 2026-10-23
- **Source Plan**: .agents/planning/2026-10-20-stepmania-pack-loader/implementation/plan.md
- **Plan Step**: Step 8: Audio delivery — `wheel_cards` service, scheduler/prefetch, progressive banks, LRU, jackets
