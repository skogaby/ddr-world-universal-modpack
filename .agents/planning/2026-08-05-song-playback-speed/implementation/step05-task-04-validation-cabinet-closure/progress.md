# Task 04 Progress — validation extension + cabinet pass + closure

Updated: 2026-08-08
Status: HALTED by maintainer pivot (2026-08-08) — the agent half completed and is
checkpoint-committed (`ee0368f`), but the cabinet matrix was deliberately stopped
mid-run and Steps 5+6 will NEVER be ticked under this model: the whole-file
generation + disk-cache design was rejected for release (25 % refused on the
128 MiB admission ceiling for a ~129 s song; the 30 s deadline would also bite;
cabinets are slower than the test hardware). The feature is being redesigned
STREAMING-ONLY via a fresh PDD cycle. Partial matrix evidence (50 % cold/warm
end-to-end, score containment, 100 % stock, 25 % fail-open) is transcribed in the
canonical progress.md Deploy & test log.

## Checklist

- [x] TDD red: python schema check requires `on_demand`; validator failed on ONLY the
      missing section (exit 1, zero FAIL checks, "report on_demand section is missing")
- [x] `validate_on_demand` implemented; full validator green
- [x] Req-24 diagnostics on `redirect_dance_bank_open` (thread + wall time)
- [x] Docs: AGENTS.md row, mod-config.json `song_playback_speed.cache_limit_gib`, README
      (feature table row + config example + reference section)
- [x] All five gates green
- [x] Cabinet run sheet written (runsheet.md)
- [ ] MAINTAINER: deploy + cabinet matrix legs a–j
- [ ] Closure: tick plan Steps 5+6, canonical progress.md updated, NEXT ACTION → Step 7

## Record

- 2026-08-08: Setup + explore complete (script structure, conversion_tests fixture
  pattern, req-24 site in runtime.rs, config/docs surfaces surveyed).
- Maintainer asked (in-session) what the validation scripts are for; explained
  (host-side test harness, never runs on the cabinet) and confirmed continuing as
  specced.
- TDD red: extended the script's python schema check first (required `on_demand`
  section + per-rate fields + exactly {75,125} coverage); the full validator run
  failed on only that (logs/validator-red.log).
- Implemented the `on_demand` section in the harness: `OnDemandReport`/
  `OnDemandRateResult` structs; `validate_on_demand` drives the REAL Task-1 pipeline
  (`prepare_open_redirect` + `prepare_streaming_redirect` with a live TLS frame)
  against a temp `CacheStore` + `DiagnosticCoordinator` — per rate (75, 125): cold
  build (arm → open redirect; builds delta == 1; phase RedirectReady; generated bank
  on disk, digested), exposure-seam transaction (Exposed, convert status 7 forwarded
  verbatim, zero new builds — warm re-derivation), warm replay (fresh generation,
  zero builds, ≤ warm latency limit); then rate-key invalidation (the 125 cold build
  against the warm 75 cache: distinct keys, each cold_builds == 1) and source-content
  invalidation (a second synthesized bank at different sine frequencies via the new
  `build_source_with_frequencies` — new cold build at the already-warm rate); plus a
  platform cold-latency check. Report records the folded-in Step-4 extension: song
  code + song digest, source/generated digests, module digest, platform identity,
  exact rates, cold/warm latency, build counts, per-check status; no absolute paths.
- Green run evidence: rates exactly 64/85 (75 %) and 64/51 (125 %); cold 23 ms/16 ms,
  warm 0 ms with builds unchanged; all 9 on_demand checks PASS; `overall_pass` true
  (logs/validator-green.log; final re-run logs/validator-final.log with 156 tests).
- Req-24: the `open-redirect` INFO line now carries `thread <tid>, build wall <W> ms`
  (caller thread identity + the wall time the FileManager open actually paid — warm
  hits read near-zero on the same line); the refusal WARN carries the same fields.
  Non-detour side only (AVS hook path); detours still never log.
- Docs: AGENTS.md key-entry-points row (after Player perspective; verified no
  duplicate — 0 occurrences before, 1 after); mod-config.json gains
  `"song_playback_speed": { "cache_limit_gib": 10 }`; README gains the feature-table
  row (after Assist Tick), the config-intro mention, Complete Example entries
  (mods map + row_order + section), and the `#### Song Playback Speed cache
  (song_playback_speed)` reference section (incl. zero-footprint note, safe-to-delete
  cache note, retired `diagnostic` key note).
- Edit-tool hazard hit once (known repo gotcha): an insert-before edit clobbered
  `latency_limit_ms`'s body and (separately) replaced the README's Non-Native OS row
  instead of inserting; both caught immediately and repaired (verified by grep +
  green gates).
- 2026-08-08: cabinet `mod-config.json` staged at the maintainer's request
  (config-only; agent still deploys nothing): retired `song_playback_speed.diagnostic`
  block replaced with `{ "cache_limit_gib": 10 }`; `mods["song-playback-speed"]: true`
  added; `song_speed` inserted into `row_order` after `assist_tick`;
  verified `developer_mode: true` retained (leg j) and autoplay 0/0. JSON validated.
  Backups removed by the maintainer (recoverable from the recycle bin). Remaining
  staging is maintainer-owned per the run sheet: copy the new DLL (md5 fce4d538...),
  delete `data_mods/_diag/abdt-75.xwb`, optionally delete the cache dir for a
  guaranteed cold leg (a).

## Gate evidence (logs/)

- `./scripts/validate_song_playback_speed.sh`: PASS — 156 tests, all sections incl.
  the new `on_demand` (validator-final.log)
- `./scripts/validate_se_bank_synth.sh`: PASS (se-bank.log)
- `cargo check --target x86_64-pc-windows-msvc`: PASS, 0 warnings (cargo-check.log)
- `cargo fmt`: whole-crate, clean
- `./build.sh`: PASS — DLL md5 fce4d53859d40e66173fa600d1296026 (build.log)

## Deviations

- None from the spec. Design req 24's original text places the wait on
  fs_convert_path; the Task-1 open-redirect fix (maintainer-approved, recorded in the
  canonical progress) moved the primary waiting site to the FileManager open — the
  spec's TR-2 already reflects this and the diagnostic went where the wait actually is.
