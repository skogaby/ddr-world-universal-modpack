# Idea Honing — Ultrafast Boot (fast-bootup refactor)

Decision register for the ultrafast-boot feature. See
`research/orientation.md` and `docs/ultrafast_boot_research.md` for backing.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Delivery shape | Module structure, config surface | Refactor of `fast-bootup` (same id/toggle), no new mod | Accepted |
| D2 | Cache-hit behavior | The core value proposition | Skip file reads AND analysis: flip queued records 1→6 (empty shape), replay writes, release via stock machinery | Accepted |
| D3 | First-boot capture mechanism | Changes module structure (NTX refactor) and replay exactness | Convert NTX's Analyze detour into a shared dispatcher service; capture subscriber reads result/radar/ret post-original | Accepted |
| D4 | Completion mechanics | Correctness of global accumulators + done flag | Leave the FINAL work item to the stock path; replay writes actor accumulators directly beforehand | Accepted |
| D5 | Cache-miss loading speed | The "as fast as disk allows" half | Raise `mgr+0x70` open cap (4 → 64) while the boot actor is live, restore at completion; bounded drain deferred until measurement shows starvation | Accepted |
| D6 | Bit-exactness validation | Trusting the replay before shipping it | No shipped verify mode. Parity validated during implementation via a temporary diagnostic build (removed before completion) + host tests on pure layers | Overridden |
| D7 | Cache identity / invalidation | Staleness = silently wrong metadata | Per file: game path + resolved backing file + size + mtime. Header: format version + gamemdx build. NTX config NOT an invalidator | Accepted |
| D8 | Identity verification timing | 1466 stats could stutter a frame | Background pre-verification thread at mod enable using host `std::fs` + `mod_paths` resolution (never AVS off game threads) | Accepted |
| D9 | Corruption-flag parity | ME1529 is a hard boot blocker on real hardware | Replay ONLY the `+0x1B0` flag. NEVER call the game's error reporter from replay; fresh (stock-path) analysis keeps stock behavior | Overridden |
| D10 | Cache file format & location | Ops/debugging ergonomics | Hand-rolled little-endian bin, versioned header, `data_mods/_cache/step_data/v1.bin`; delete-to-rebuild | Accepted |
| D11 | Cache write timing | I/O on hot paths | Serialize + write once at work-list completion on a background thread (atomic tmp+rename); merge-rewrite on partial-miss boots | Accepted |
| D12 | In-flight entries at first onUpdate | Race window before our batch runs | Process stock and use them to refresh the cache (self-healing); ≤ open-cap files affected | Accepted |
| D13 | Failed/absent SSQs | ~hundreds of chartless customs in the field | Cache absent-file outcomes keyed as "absent"; file appearing later = identity mismatch = stock path | Accepted |
| D14 | Config surface | Operator complexity | Zero new config keys; cache always on when `fast-bootup` enabled | Accepted |
| D15 | Failure semantics | Crash-safety in someone else's process | Fail-open everywhere, with explicit cache-integrity tiers (per-entry bad ⇒ that chart goes stock + refresh; whole file unparseable ⇒ treat as empty, full rebuild); derivation failures ⇒ stock fast-bootup behavior. One WARN per class | Accepted |
| D16 | Percent-bar UX on full hit | Cosmetic | Accept the 0→100 jump | Assumed |
| D17 | Split-file songs | Correct keying for `_1.._5` charts | Cache schema keyed file → (difficulty, side) → payload handles them naturally; no special casing beyond storing per-item file identity | Assumed |

## D1 — Delivery shape

**Q:** New mod or refactor? **A (maintainer, 2026-08-24):** Refactor of
`fast-bootup`. Same mod id, same enable toggle; cache + pacing removal are the
new internals. **Rationale:** it is the same feature (fast boot) done better;
two toggles for one concern is operator noise.

## D2 — Cache-hit behavior

**Q:** Skip only the Analyze CPU, or skip the file loads too?
**Recommendation:** skip both. The measured window is load-bound
(cap × fps), so caching parse output alone saves little. Mechanism (research
§5.5): queued records flip status 1→6 with null buffer — the stock pump's own
"empty file" shape, handled by both pumps without opening — then replay the
music-DB writes and queue releases so the manager reaches its stock end-state.

## D3 — First-boot capture mechanism

**Q:** Where do we capture the analyzer outputs on a cache-building boot?
The result/radar blocks are onUpdate stack locals — invisible to our onUpdate
detour. Options:

- **(i) Analyze-boundary dispatcher (recommended).** Convert NTX's existing
  `GenericDetour` on Analyze into a shared dispatcher service
  (`services/` — judge_hook/render_notes_hook model). Subscribers: NTX's mine
  injection (unchanged semantics), and a boot-capture subscriber active only
  while the boot actor is live. Exact: captures `result[14]`, `radar[5]`,
  `ret` per call. Cost: a mechanical NTX refactor.
- **(ii) Music-DB read-back.** After `hook.call()` per item, read the written
  fields back from the DB. No NTX refactor, but loses exact per-chart radar
  (only max-accumulated values survive) and per-slot analyzable bools —
  approximations in the replay. Rejected: exactness is the whole game here.

## D4 — Completion mechanics

**Q:** Who runs the completion block (accumulator copy to
`*DAT_1806F14F8+0x30..0x54`, done flag + parent-chain walk, final percent)?
**Recommendation:** leave exactly the final work item to the stock gated path
(costs one file load, ~ms with D5); our replay max-accumulates into the actor
fields (`+0xA8..+0xB8`) as it goes, so the stock final item completes on top of
correct state. Self-heals any future drift in the completion code.
**Alternative** (full replication of the completion block) is deterministic
but reimplements the parent-chain flag walk; keep as fallback if the last-item
shape proves awkward.

## D5 — Cache-miss loading speed

**Q:** How to reach disk speed for loads that still happen?
**Recommendation:** write `mgr+0x70` (u32, stock 4) to 64 on the first hooked
onUpdate call, restore at completion. Evidence (research §6): the window is
open-cap × frame-rate bound; the manager is built for 0x1000 in-flight
records; memory peak stays tens of MB because the batch drains every frame.
The bounded per-frame drain (mirroring stock `0x1801FE380`) is designed but
NOT built until a cabinet measurement shows the cap raise alone leaves the
device idle. Value 64 hardcoded (no config, D14).

## D6 — Bit-exactness validation (Overridden 2026-08-24)

**Original recommendation:** a shipped `DDR_FAST_BOOT_VERIFY=1` env-var mode.
**Maintainer override:** no permanently shipped verify gate — "I'll just scrap
the feature if it becomes infeasible, rather than keeping dead code around
behind a gate." **Accepted answer:** parity is validated during the
implementation phase with a temporary diagnostic build (the codebase's
standard dev-loop pattern: one deploy that captures fresh values AND diffs
them against the cache, logging mismatches; the diff code is removed before
the feature is complete). Pure layers (bin serialization, identity keying,
replay arithmetic) get permanent host tests via `cargo test`.

## D7 — Cache identity / invalidation

**Per-file key:** registered game path (`data/mdb_apx/ssq/x.ssq`) + resolved
backing file (LayeredFS mod-folder override or stock, via `mod_paths`
resolution) + file size + mtime. **Header invalidators:** cache format
version; gamemdx build string (the analyzer arithmetic could change between
builds; the split-file specials table is also per-build). **Not**
invalidators: DLL version (format version covers layout changes), NTX
config/mine datasets (research §8.1: boot-time injection affects nothing
persistent), musicdb.xml content (cache is keyed by file, mcode mapping
happens live at replay).

## D8 — Identity verification timing

**Q:** When do the ~1466 file stats happen? **Recommendation:** background
thread spawned at mod enable (DLL init, well before the boot actor exists):
read the bin, resolve + stat every cached file via host `std::fs` +
`mod_paths::find_first_modfile` (the chart_length / judgement-offsets
bootstrap pattern — AVS calls are game-thread-only), publish a ready
verdict map. First onUpdate call consumes it; if the thread hasn't finished
(it will have — it races a ~1 s window against ~100 ms of stats), items fall
back to stock. Never stat on the game thread.

## D9 — Corruption-flag parity (Overridden 2026-08-24)

**Context:** for a chart whose SSQ fails analysis while `musicdb` says it
should exist, stock onUpdate (a) sets the music-DB entry's `+0x1B0` flag and
(b) calls the game's own error reporter
(`(*DAT_1806F2858)("ME1529", "MDX1529", "FILE CORRUPTION ERROR", mcode)`).
**Original recommendation** was to replay both for parity.
**Maintainer override:** ME1529 is Konami's **hard boot blocker** on real
hardware — the replay must NEVER invoke the game's error reporter.
**Accepted answer:** replay ONLY the `+0x1B0` flag write (the cache stores
per-slot `ret` + counts, so the trigger condition evaluates identically);
never call the reporter from the replay path. Charts processed fresh through
the stock path keep whatever behavior the game has today — we neither add nor
suppress stock error reporting. (Current cabinet log shows zero INVALID SSQ
charts, so nothing regresses on a clean library either way.)

Note the terminology split this override clarified: "corruption" of a CHART
FILE on disk is the game's concern (this decision); corruption of OUR CACHE
file is D15's concern (per-entry bad ⇒ stock + refresh; whole bin unparseable
⇒ treat as empty and fully rebuild).

## D10 — Cache file format & location

`data_mods/_cache/step_data/v1.bin` (shader-synthesis precedent directory).
Hand-rolled little-endian format, no new crate deps, pure read/write layer in
`core/` or the mod dir — host-testable (`cargo test`). Header {magic, format
version, gamemdx build string}; entries {game path, resolved path, size,
mtime, basename, per-(difficulty, side) payload {result[14] i32, radar[5]
i32, ret u8}} (~1.2 MiB for ~7205 items). Recovery story: delete the file.

## D11 — Cache write timing

Capture accumulates in memory during the boot pass; one serialize + atomic
write (tmp + rename, judgement-offsets writer pattern) on a background thread
at work-list completion. Partial-miss boots merge fresh captures over the
loaded cache and rewrite. No writes on the game thread.

## D12 — In-flight entries at first onUpdate

The tick pumps before actor dispatch, so up to open-cap records may be status
2/3 before our first batch. These are processed stock (the existing gated
path) and their fresh captures refresh the cache. No cancel logic needed.

## D13 — Failed/absent SSQs

Charts whose file is missing produce a zeroed outcome + corruption flag via
the stock path. Cache them keyed as "absent" (no size/mtime); replay matches
stock (zeros + flag + report per D9). If the file appears later, the identity
check fails → stock path → cache refresh.

## D14 — Config surface

No new `mod-config.json` keys. Cache always on with the mod; open-cap value
hardcoded. Rejected: `ultrafast` sub-toggle (two knobs for one feature),
configurable cap (nothing to tune until a measurement says otherwise).

## D15 — Failure semantics (Accepted, extended 2026-08-24)

Fail-open at every seam, with explicit cache-integrity tiers (maintainer
direction):

- **Per-entry corruption** in our bin (bad payload for one chart, identity
  mismatch, mcode not found at replay) ⇒ that chart takes the game's normal
  load+analyze path and its cache entry is refreshed.
- **Whole bin unreadable/unparseable** (bad magic, truncation, version or
  gamemdx-build mismatch) ⇒ treat as empty: fully stock boot, full rebuild.
- **Derivation failures** (release fn, music-DB global, by-mcode lookup, any
  RIP-decode from onUpdate) ⇒ the entire cache/pacing feature disables and
  the mod behaves exactly like today's fast-bootup.
- One WARN per failure class; the existing race/EOL gates stay on the stock
  path unchanged.

## D16 — Percent-bar UX (Assumed)

Full-hit boots jump the bar 0→100 in one frame. Accepted; no fake ramp.

## D17 — Split-file songs (Assumed)

`_1.._5` split charts register distinct files per difficulty; the
file-keyed → (difficulty, side) schema and per-item identity lookup cover
them with no special casing. The hardcoded specials table lives in the game;
its per-build variance is covered by the gamemdx-build header invalidator.

---

Readiness Confirmed 2026-08-24 — register accepted in full (D6/D9 as overridden); proceeding to detailed design.
