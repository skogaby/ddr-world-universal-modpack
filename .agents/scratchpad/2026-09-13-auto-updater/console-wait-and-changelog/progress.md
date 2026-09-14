# Progress — Step 5 (consolidated): hardening
Status: Complete (uncommitted — maintainer commits manually)

## Done
- [x] `apply::recover_if_interrupted` (§6.4): journal-driven restore (backups back, created files removed, journal + stage/download deleted; unreadable journal → removed + WARN); wired into `main` after the log attaches, before any network; `Recovery` enum. Unit test over a synthetic journal/backup layout.
- [x] Empty-dir cleanup after prunes (`remove_emptied_dirs`): walks each pruned path's parents inside `data_mods/`, stops at the first non-empty ancestor, never removes `data_mods/`; `Summary.dirs_removed` + log line. Unit + e2e (H5).
- [x] `selfupdate.rs` (rename-swap `swap_in`/`undo`/`cleanup_stale`, restores the old image if the second rename fails) + `Action::SelfUpdate{rel}` (emitted when the staged exe hash ≠ on-disk; exe hash always recorded in the manifest) + `Done::SelfSwapped` (undo on rollback) + `cleanup_stale` at start-up. 3 unit tests; e2e H3 (swap, `.old`, next-run cleanup, no swap on identical) + H4 (rollback undoes the swap).
- [x] `fault.rs`: `crash-after:<n>` (exit 70 without rollback → journal left) for the recovery tests; e2e H1 (crash → next run recovers then completes) + H2 (recovery alone restores the pre-crash folder byte-identically).
- [x] `console.rs`: `should_wait(count) == 1` (pure), `GetConsoleProcessList` via `windows-sys` (Windows-only dep), `wait_for_enter` BOUNDED at 60 s (stdin read parked on a thread + `recv_timeout`) — a cabinet can never be held. `changelog.rs`: `preview(body, cap)` (emphasis/backticks stripped, fences skipped, bullets → `•`, indent kept, `…` on truncation); shown (cap 15) under the release name in `run_default`.
- [x] All Step-1/2 `#[allow(dead_code)]` markers removed except the documented one on the `#[path]`-mounted DLL csv module.
- [x] 111 unit + 26 e2e green; host + Win7 builds warning-free; exe 2 499 072 B, no `ProcessPrng`.
- [x] **CrossOver on the real install:** (1) self-update with the running image: zip carrying a 1-byte-padded copy of the new exe → `Installed: 369 files written, updater replaced`, running exe renamed to `.exe.old`, new exe in place; next run `removed the previous updater image left by a self-update`, `.old` gone. (2) bat path (`cmd /c`) does NOT wait; a bare `wine exe` launch DOES show `Press Enter to close (closes by itself in 60 s)...` (Wine reports the process alone on its console) and exits immediately on EOF. (3) crash recovery: `DDR_UPDATER_FAULT=crash-after:50` → exit 70 with journal + 50 backups; next run `restored 50 file(s)`, then completed the install; journal gone.

## Findings
- Under Wine a DIRECT launch (`wine ddr_world_hook_updater.exe` from a shell) counts as "alone on the console" → the 60 s bounded wait applies; via `cmd /c <bat>` it does not. The cap makes the worst case a 60 s delay, never a hang. README (Step 6) should tell CrossOver users to run the bat, not the exe.
- The Step 4 exe (no self-update code) cannot swap itself out when installing a zip that carries a newer exe — the FIRST release that ships the updater must be installed by the operator copying the exe by hand (as the README will say); from then on self-update carries it.

## Deviations
- `wait_for_enter` bounded at 60 s (design said "waits for Enter"): D7/R20 "never block the game" outranks R19's convenience.
- `crash-after:<n>` dev fault added (exit code 70) for recovery tests.
- `Recovery` treats `SelfUpdate` as a no-op (design §6.4) — the running image cannot be replaced from within; the `.old` is cleaned at the next start.
