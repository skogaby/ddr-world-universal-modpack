# Progress — Split SSQ Auto-Discovery

Updated: 2026-09-03
Status: Step 3 of 3 — done (uncommitted — maintainer commits manually); cabinet boot validation PASSED 2026-09-03 (rows 1, 3); gameplay check + OFF-toggle check (rows 2, 4) pending
NEXT ACTION: maintainer plays "SPLIT SSQ TEST (casr clone)" Expert + a `toho` chart; then `scripts/setup_split_ssq_test.sh remove` and commit.
Resume: plan `implementation/plan.md`, design `design/detailed-design.md`, RE `docs/split_ssq_research.md`.

## Done
- Step 1 — `src/mods/split_ssq_auto_discovery/resolver.rs` (pure Rule A, filename parse, level bitmask, path formatter, NUL-aware compare, listing filter) + `scripts/validate_split_ssq.sh`: 14 host tests green, incl. the 39-file stock-table fixture.
- Step 2 — `build_ssq_path` AOB in `signatures.rs` (sweep ALL GREEN; resolves at +0x19E8D0 / +0x1A1730 / +0x1B43F0 / +0x1B4090 on the four builds, exactly the RE-predicted addresses); `discovery.rs` (stock ∪ LayeredFS mod dirs, content via `find_first_modfile`); `mod.rs` (index built in `enable()` BEFORE the detour installs; passthrough flag on disable); registered in `lib.rs`/`mods/mod.rs`. `cargo check` clean, `cargo fmt`, `./build.sh` clean.
- Step 3 — divergence oracle in the callback (original into scratch, dedup + cap 64), AGENTS.md Key Entry Points row.

## In flight
Nothing. Working tree uncommitted.

## Deploy & test log
| # | Build | Scenario | Expected in log | Result |
|---|---|---|---|---|
| 1 | 2026-09-03 13:57 | stock data (+ `yuwo` files present since 08-25), matched binary (20260721) | `indexed 33 split song(s) from 40 file(s)` (34/41 incl. zzzt); 0 `INVALID SSQ`/`ME1529`; divergence lines | **PASS.** 12 divergence lines: `sabm d=4` (expected) + 11 of the class "ours=base, stock=_3/_5 at d=4 where NEITHER file has a Challenge chart" (buco flor rabb eoth kjnf2 gogg scre fizz casr smin danz) — harmless (both loads find no chart; DB says none ⇒ no ME1529; equivalent zero analysis), but the design's "none or only sabm" claim was wrong: my host simulation only compared pairs where the stock file HAS the chart. Documented; log stays as-is. |
| 2 | (pending) | play `casr` Expert (pattern E) + `toho` (any diff) | charts load; no divergence line for `toho1..4` | — |
| 3a | 2026-09-03 13:57 | first fixture attempt: `zzzt` via `gamedata/musicdb.merged.xml` | zzzt divergence lines | **INVALID FIXTURE** — index saw `zzzt: [-,-,3,3,-]` but the game never asked for zzzt: `.merged.xml` fragments don't reach the game's music DB (served out of startup.arc by the FileManager, no AVS open — see learnings 2026-09-03). Fixture rewritten as a startup.arc overlay. |
| 3 | 2026-09-03 14:10 | `scripts/setup_split_ssq_test.sh install` (v2) — `data_mods/split_ssq_test` cloning `casr` as `zzzt`: charts, renamed audio banks + jacket arc, `arc/startup_arc/data/gamedata/musicdb.xml` (stock + entry, mcode 39901). Re-installed 2026-09-03. | **PASS.** `arc: regenerating cache for arc/startup.arc` → game DB picked up zzzt (1470 songs); boot pass asked for it: `zzzt d=2` + `zzzt d=3` divergence lines (ours `_3`, stock base); `judgement_offsets: song identity 'zzzt' (ssq open)`; 0 `INVALID SSQ`/`ME1529`; fast_bootup: 1508/1510 files verified, 7345/7350 items replayed, 2 new files (zzzt, zzzt_3) captured → cache self-healed as designed. Gameplay of "SPLIT SSQ TEST" Expert still to be confirmed by the maintainer. Expected next boot: `indexed 33 split song(s) from 40 file(s)`, `zzzt: [-,-,3,3,-]`, divergence lines `zzzt d=2` + `zzzt d=3` (ours `_3`, stock base); no ME1529; "SPLIT SSQ TEST" plays Expert. **Run `remove` before booting with the mod OFF** (stock builder ⇒ `zzzt.ssq` for Expert ⇒ ME1529 — the bug itself). | — |
| 4 | (pending) | mod-menu toggle OFF → boot | `disabled (stock path builder passthrough)`, no index lines | — |

## Deviations & open questions
- Steps 1–3 were implemented in one code-assist run (maintainer-authorized autonomous pass); the oracle (Step 3) landed with the callback rather than as a follow-up edit. No design deviation.
- One test expectation fixed during Step 1 (`paths_differ` no-NUL buffer semantics — the implementation matched the doc comment; the test was wrong).

## Key facts for a cold resume
- Hook = full replacement; original is called ONLY as the diagnostic oracle (into our scratch).
- Rule A needs the CONTENT check — a filename-only rule can name a file lacking the chart ⇒ boot-blocking `ME1529`.
- Basename-opaque ⇒ `toho1..4` resolve to base = stock. Never add a musicdb lookup.
- Nothing past the function prologue is build-stable; never read `match+N`.
- `chart_length.rs` still builds `<code>.ssq` itself (documented follow-up).
