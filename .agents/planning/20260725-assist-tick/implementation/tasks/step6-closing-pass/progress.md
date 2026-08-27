# Progress — Step 6: diagnostic pass, docs, and final gates

Run directly at the maintainer's direction (2026-07-27): matrix re-run waived ("I've done enough
in-game testing"), closing pass executed without task-file decomposition. One working record
(this file) instead of per-task dirs.

## Done

- [x] **Per-tick logging demoted** to `log_debug!` (invisible at the logger's Info default; kept
      rather than deleted — re-enabling the scheduled-vs-actual measurement is one
      `set_log_level` call, re-inventing it is not). Once-per-song lines and warn-onces kept.
      Verified live: a full song produced **zero** `tick scheduled=` lines.
- [x] **NFR audit:** no `unwrap`/`expect`/`panic!` anywhere in the mod or service; all array
      indexing compile-time bounded (`[T; 2]` with `0..2` loops, literal indices into
      `[i32; 8]`); no hardcoded game addresses (offsets are documented struct constants;
      addresses come from the signature store); every failure path warns exactly once
      (verified across the step records); no new crate dependency (`serde_json` was already a
      dependency); config section exists by approved amendment (§5.3), not in violation of the
      original NFR.
- [x] **Docs:**
      - `README.md`: **Included Mods** table row (links `#assist-tick`); the Step 1 asset section
        expanded into the full `## Assist Tick` mod section (usage, what ticks and why,
        chart-driven-not-judgment-driven, one-relaunch label caveat, timing calibration via the
        overlay row + `offset_ms`, StepMania attribution, sound swapping, the regeneration
        subsection folded in — no overlapping second section, per carried item F);
        `assist-tick` + `assist_tick` added to the Complete Example (`mods` map, config section,
        `row_order`) and the available-ids list.
      - `AGENTS.md`: Key Entry Points row (mod + service + the never-destroy/file_id −1 trick +
        the predicate and side-selection invariants + RE-note pointer) and a Config bullet for
        `assist_tick.offset_ms`.
      - `docs/xact_audio_research.md` (new): the durable XACT consolidation — architecture, the
        manager's six slots and the slot-mapping gap, the file_id −1 immortality trick, the
        `se_play` XMM2 ABI trap, safe engine vtable indices, creation rules (wave bank first,
        leak both buffers, internal-name pairing, silent-failure behaviour), bank-format
        validator constraints, HRESULT vocabulary, cross-version anchors.
- [x] **§7.2 diagnostic questions, answers recorded:**
      1. *Sibling list at first dispatch:* verified complete for solo (`siblings=1 sides=[0]`,
         containment held, no DEGRADED marker). 2P/doubles observation waived with the matrix;
         the per-song diagnostic line reports it whenever those sessions happen.
      2. *Per-panel state values > 1:* nothing anomalous observed — `rej_panel=0` everywhere and
         shock counts reconciled exactly against the results screen's combo denominator.
      3. *Doubles from the P2 reader:* not exercised (waived). The code never assumes
         `doubles ⇒ side 0`; the diagnostic line settles it whenever a doubles session is played.
      4. *Real coalescing window on TPS-150:* `coalesced=0` on the charts played; `COALESCE_MS=4`
         stands, its provisional comment retained.
- [x] **Final gates:** `cargo check` clean → `cargo fmt` (whole crate) → `./build.sh` clean;
      installed (sha256 match).
- [x] **Final boot:** mod registers, row registers, gameplay entry → default-OFF song correctly
      inert; `custom_options.p1.assist_tick` present in the JSON cache (0 — correct: the cache
      snapshots on card-out, and the prior ON session ended via a scripted `control exit` with no
      card-out); crash log carries only install banners; the only ERROR line is the crash
      handler's banner.

## Notes carried out of the feature

- Label-script regeneration is NOT byte-stable on this machine (Pillow rendering drift rewrote 5
  unrelated PNGs — reverted; see the step05 task-01 record).
- `se_play_inner` stays resolved-but-unused (the mute-filter mitigation that proved unnecessary).
- ADPCM quantizer rounding (~5 dB SNR available) remains the maintainer's open call in the
  sibling repo (carried item E).

Status: Complete
