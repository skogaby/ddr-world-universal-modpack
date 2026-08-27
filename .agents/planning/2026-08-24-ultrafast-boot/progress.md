# Progress — Ultrafast Boot (feature)

Updated: 2026-08-24
Status: ✅ COMPLETE & CLOSED — Steps 1–8 done, locally validated, and
maintainer-confirmed on the cabinet (loading sequence "effectively instant").
Uncommitted — maintainer commits manually.
NEXT ACTION: none — feature complete and accepted. (Optional future: re-verify
the four `derive_ultrafast_boot` decodes on gamemdx 20260616 before shipping to
that build.)

Resume protocol: `implementation/plan.md` (checklist, all ticked), `design/
detailed-design.md`, `idea-honing.md` (register incl. D6/D9),
`docs/ultrafast_boot_research.md` (addresses/layouts + §11 implementation
status). Source: `src/mods/fast_bootup/{mod,cache,identity,capture,replay,
plan}.rs`, `src/core/signatures.rs::derive_ultrafast_boot`,
`src/services/analyze_hook.rs`.

## Done (Steps 1–8)

- **Step 1** — `fast_bootup.rs` → directory; pure layers `cache.rs` (bin v1),
  `replay.rs` (compute_slot + fold_radar), `plan.rs` (BootPlan invariants).
  Host tests via NEW `scripts/validate_fast_bootup.sh`.
- **Step 2** — `core/signatures.rs::derive_ultrafast_boot`: music_db_global
  (+0x6F2D78), variable_bpm_threshold (+0x393F40), find_music_by_mcode
  (+0x1B4290), step_data_release (+0x1FF1B0) — decoded from onUpdate, soft-fail.
- **Step 3** — `mgr+0x70` open cap 4→64 during the boot pass. A/B: **cap4
  6939 ms vs cap64 2382 ms (2.9×)**. Dev knob `DDR_FAST_BOOT_OPEN_CAP`.
- **Step 4** — `services/analyze_hook.rs` shared Analyze dispatcher; NTX
  migrated from its own detour to a post-subscriber.
- **Step 5** — capture-only cache. Pure helpers in `cache.rs`
  (`normalize_ssq_rel`/`resolve_identity`/`identity_matches`/`merge`). New
  impure `identity.rs` (host-`std::fs` resolve + off-thread verifier →
  replay index + PE stamp/size invalidators) and `capture.rs` (boot-gated
  Analyze subscriber + per-item stash → per-mode slots → STORE + completion
  writer thread, tmp+rename to `data_mods/_cache/step_data/v1.bin`). `mod.rs`
  wiring; `game_path` resolved from the manager name records (inline C-string
  @ `*(mgr+0x28) + entry_index*0xA0 + 0x11`).
- **Step 6** — TEMPORARY parity diff (`parity.rs` + `capture::parity_diff`),
  cabinet-proven **0 field mismatches across all 1499 files** (the hard gate),
  then REMOVED in Step 8 (D6: no shipped verify gate).
- **Step 7** — replay path in `mod.rs`: build the boot plan + flip eligible
  records 1→6 on the first call; per cursor item Replay (`compute_slot` writes
  to the entry from `find_music_by_mcode`, per-side radar fold into
  +0xA8/+0xBC, release via `step_data_release`, cursor advance, percent) vs
  Stock (existing gated path + capture). `merge` UNION fix so a replay boot's
  partial re-capture of the final song never truncates its cache entry.
- **Step 8** — mutation drills (all pass, see log below); removed the Step-6
  parity diff (`parity.rs` deleted, harness un-mounted) + the temporary
  Step-7 entry-dump; docs updated (`mod.rs` header, AGENTS.md key-entry row +
  dir map, README, `docs/ultrafast_boot_research.md` §5.3 correction + §11).

## Deploy & test log (local CrossOver, gamemdx 20260721, backend up)

- Step 1–4: baseline/derivations/pacing A/B/NTX subscriber — all as before.
- Step 5 FIRST boot (no cache): verifier 0; processed 7305 in 2517 ms;
  **wrote v1.bin = 1,262,453 bytes (1499 entries)**; TITLE+ATTRACT; 0 excns.
- Step 5 SECOND boot: verifier **1499/1499 verified**; would-hit 100 %;
  behavior unchanged (replay OFF); cache deterministic.
- Step 6 boot (cache present): **parity CLEAN — 1499 files / 14610 payloads,
  0 field, 0 shape mismatches** (the hard gate).
- Step 7 replay boot: **replay plan 7304/7305, 1498 flipped, boot pass ~42 ms,
  1 SSQ open**; entry-dump A/B stock-vs-replay **BYTE-IDENTICAL**; 0 INVALID
  SSQ/ME1529; TITLE+ATTRACT plays the replayed data. Cache stable at
  1,262,453 bytes (union fix).
- Step 8 drills: touch abdt.ssq ⇒ **1498 verified, 6 stock items, abdt
  re-analyzed**; corrupt header ⇒ **WARN "bad magic" + full rebuild (0/7305)**;
  delete bin ⇒ full rebuild; disable mod ⇒ **stock slow boot, 1469 SSQ opens**.
- Final build (diagnostics removed): "1499/1499 file(s) verified for replay",
  7304 replays, 40 ms pass, **0 diagnostic/stale/crash lines**, TITLE+ATTRACT.
- **Maintainer manual cabinet test (2026-08-24): loading sequence "effectively
  instant" — feature accepted.** Same-machine boot-pass deltas: original
  fast-bootup batch @ stock cap 4 = 6,939 ms → cap-64 raise (no cache) =
  ~2,400 ms → cache hit (replay) = **~40 ms**. SSQ file opens: 1,476 (stock /
  mod-off) → **1** (cache hit). Wall-clock on the fast dev rig moved less
  (~1 s) because fixed splash/hardware-check dwell dominates there; the SSQ
  window is a far larger share of boot on the reference cabinet (research
  §1: ~15.5 s of ~28 s), which is where the "instant" win lands.
- Post-run state left clean: `mods["fast-bootup"]=true`, `v1.bin` present
  (1,262,453 bytes), final DLL deployed.

## Deviations & open questions

- **Actor radar accumulators are PER-SIDE (10 total), not 5** — onUpdate's
  `local_228 += 5` each side ⇒ side 0 `+0xA8..+0xB8`, side 1 `+0xBC..+0xCC`.
  Research §5.3 understated it; corrected there + handled in Step 7's applier
  (fold each side's radar into its own 5-int window via `fold_radar`).
- `merge` semantics extended beyond the design's entry-level "fresh wins":
  for an UNCHANGED file (identity matches) it UNIONs payloads (fresh wins per
  slot) instead of replacing wholesale, so a replay boot's partial re-capture
  of the final song (its 4 non-final diffs replay, only the final diff is
  stocked+captured) never truncates the cached entry. Changed/new files still
  replace/insert. Host-tested (`merge_unions_payloads_when_identity_unchanged`,
  `merge_replaces_wholesale_when_identity_changed`).
- LayeredFS-override mutation drill covered by composition (host-tested
  `identity_matches` resolved-path change + the touch drill's proven
  miss→stock→refresh flow + the shared `mod_paths` precedence), not a
  separate cabinet cycle.
- Implemented by DIRECT edits per the approved design (the exactness contract),
  not via code-task-generator/code-assist SOP scaffolding — full context was
  in hand and the design IS the task spec. Per-step cabinet validation + host
  tests were the gates. No `.agents/tasks/step05+` files generated.
- Host tests via `scripts/validate_fast_bootup.sh` (ARM can't build retour for
  plain `cargo test`); 26 tests (mount cache/replay/plan).

## Key facts for a cold resume

- Feature COMPLETE + validated; uncommitted (maintainer commits manually).
- Boot addresses/layouts: `docs/ultrafast_boot_research.md` (20260721) incl.
  §11 implementation status.
- Cache: `data_mods/_cache/step_data/v1.bin` — delete to force rebuild;
  auto-invalidates on gamemdx update (PE stamp/size header).
- Fail-open everywhere; replay NEVER calls the ME1529 reporter (D9); the final
  work item is always Stock so the game's completion block runs natively.
