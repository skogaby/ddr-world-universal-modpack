# Progress: mod-skeleton-bootstrap

Updated: 2026-08-17
Status: Complete (uncommitted — maintainer commits manually); cabinet demo pending

- PerSongJudgementOffsetsMod (id per-song-judgement-offsets) registered in
  lib.rs; enable() fully gated on custom_options::row_injection_available()
  (D20 inert rule); required_signatures = player_option_table +
  selectmusic_model.
- bootstrap.rs: "judgement-offsets" thread (panic-contained, OnceLock
  once-guard) — DISK-BASED musicdb union (v2, deploy-#1 fix): whole-file mod
  override else stock musicdb parsed straight out of ./data/arc/startup.arc
  via core::arc (+kbin guard), unioned with every mod's musicdb.merged.xml
  fragment; append-merge into judgement_offsets.csv via tmp+rename; baseline
  into store; then serves the coalesced CSV upsert channel
  (queue_csv_upsert) forever, re-reading the file per batch so operator
  hand-edits survive.
- Deploy #1 finding (2026-08-18): the design's merge_xmls /
  load_xml_from_avs_path reuse FAILED at runtime — the AVS trampoline reads
  only work for in-hook game-thread callers; our background thread got
  handle<0 on every attempt while the game itself read musicdb fine.
  Rewrote the crawl disk-based (no AVS calls); reverted the xml_merger
  visibility widening (no longer consumed).
- Known window: upserts queued before bootstrap finishes are dropped
  (UPSERT_TX unset) — unreachable in practice (menu opens well after boot);
  noted for Step 4.
- Validation: cargo check (win target) clean; harness 23/23; ./build.sh
  release clean (logs/build.log).
- Cabinet demo (plan Step 3): CSV self-creation, append-only diff, log
  review — PENDING first deploy.

Status: Complete (uncommitted — maintainer commits manually)
