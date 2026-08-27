# Progress — task-01 analyze-dispatcher (Step 4)

- [x] `src/services/analyze_hook.rs`: OnceLock-owned single detour on
      `step_reader_analyze`; `init`/`register_post`/`is_available`;
      dispatcher = original → post-subscribers (registration order,
      catch_unwind each) → return original's u8.
- [x] `services/mod.rs` + lib.rs init (right after judge_hook, before mods enable)
- [x] NTX migrated: kept SYMBOLS + actor-offset detection; dropped its
      GenericDetour; `analyze_dispatcher` → `analyze_post` adapter +
      verbatim `analyze_inject` body; `install()` now registers the
      subscriber (analyze_addr param removed; call site updated).
- [x] cargo fmt / check (win, 0 warnings) / build.sh clean

## Deploy & test (2026-08-24 02:05, local CrossOver, gamemdx 20260721)
- `AnalyzeHook: installed shared Analyze dispatcher` + `AnalyzeHook started`
- `NoteTypesExpansion hooks: registered Analyze post-subscriber` (NO
  "installed Analyze detour" line → exactly one detour, owned by the service)
- 4 boot-time `NoteType 'mines': injected …` lines → injection still fires
  post-original through the shared dispatcher (no regression)
- boot pass 2405 ms (cap 64), TITLE reached, 0 exceptions

## Deviations
- NTX keeps `step_reader_analyze` in required_signatures (unchanged) even
  though the service now also resolves it — harmless (same address), and it
  preserves NTX's clean "skip mod if unresolved" gating.

Status: Complete (uncommitted — maintainer commits manually)
