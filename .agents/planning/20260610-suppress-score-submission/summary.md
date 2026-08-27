# Project Summary: 20260610-suppress-score-submission

## Artifacts produced

```
.agents/planning/20260610-suppress-score-submission/
├── rough-idea.md                       ← suppress score upload on autoplay / quick-fail
├── idea-honing.md                      ← Q1–Q8, all settled (Q8 emerged from research)
├── research/
│   ├── existing-hooks-and-triggers.md  ← what we hook today; trigger-state plumbing;
│   │                                     version context (ess frozen at 20260324)
│   ├── score-submission-re.md          ← the binary RE: save-trigger chain, ess
│   │                                     save_sender emits /result, chokepoint choice,
│   │                                     logout re-send, m_isDead claim debunked
│   ├── option-row-lifetime-crash.md    ← (side-quest) pre-existing dangling-row crash;
│   │                                     OptionForm::~OptionForm hook, cross-version AOB
│   ├── 2p-options-load-side.md         ← (side-quest) pre-existing 2P option-load
│   │                                     misrouting; ddrcode join + deferred apply
│   ├── filter-menu-crash.md            ← (side-quest) filter open/close dangling
│   │                                     FilterButton crashes; FilterButton::~FilterButton
│   │                                     hook + builder-entry reset
│   ├── clear-type-filter-crash.md      ← (side-quest) FC-filter crash; series_expansion
│   │                                     label-builder over-match → "DDR " disambiguation
│   └── caution-screen-slowdown.md      ← (side-quest) scene-21 slowdown; per-open
│                                         exists() stats → in-memory _cache index
├── design/
│   └── detailed-design.md              ← R1–R8, score_guard component, trampoline
│                                         change, risks R-A..R-E, acceptance scenarios
├── implementation/
│   └── plan.md                         ← 10 incremental steps + checklist
└── summary.md                          ← this file
```

> **Scope note:** the score-suppression feature was the starting point, but
> cabinet testing (and crash reports from another operator) surfaced **five
> pre-existing, unrelated bugs** in the custom-options / filter / LayeredFS
> machinery. All were root-caused and fixed in this same session — see the extra
> research notes above and the "Additional fixes" section below. They are
> independent of score suppression but shipped together. Recurring themes:
> **overlay teardowns that fire no scene transition** (3 of the bugs — options
> menu and filter menu both close as overlays within SONG_SELECT), **over-broad
> AOB signatures**, and **per-open filesystem cost at scale**.

## What this feature does

Hard-bakes score-submission suppression into the **autoplay** and
**quick_restart_or_fail** mods: when a play is **faked** (Autoplay on) or
**incomplete** (triple-`3` Quick Fail), the end-of-song server save for the affected
side(s) is suppressed so no score reaches the eamuse backend. No user toggle —
integrity by design.

## The load-bearing RE answer

The per-play **score IS uploaded through the exact ess.dll
`sys_playerdata_save_sender` (`+0x29E70`) we already detour** for custom-options
persistence — the `/result` block (`score`, `exscore`, `clearkind`, `maxcombo`,
`judge_*`, …) rides alongside `/option` in one per-side profile save. So suppression
is a small addition at a chokepoint we already own (one detour, per-side handle
already derived at `savedata+0x90`). ess.dll is frozen at 20260324 client-side even
for the current gamemdx 20260526, so the ess findings are cross-version stable.

Save-trigger chain (gamemdx 20260526): `SavePlayerDataActor` (`FUN_1800b4080` ctor /
`FUN_1800b4230` onUpdate) → per-stage `FUN_18001e390` / logout `FUN_18001e5c0` →
marshaller `ReflectSavePlayerData` (`FUN_180018580`, kind 1/2/3) → async →
ess `save_sender`.

## Key decisions

| Ref | Decision |
|-----|----------|
| Q1/R1 | Suppress the **entire** per-side end-of-song save (score-only and whole-save converge at this chokepoint). |
| Q2/R2 | **Autoplay → per-player**, **Quick-Fail → both players**. Suppress side X iff `autoplay[X] OR quick_fail`. |
| Q3/R3 | **Network upload only** (DDR has no local score persistence; results screen untouched). |
| Q4/R4 | Per-song taint resets at gameplay entry / quick-restart; autoplay read live at save time. |
| Q5/R5 | **Hard-baked into the two trigger mods** via a shared `services::score_guard`; **no user toggle**. |
| Q6/R6 | **Asymmetric failure:** Autoplay **fails closed** (won't enable without the hook); Quick-Fail **fails open**. |
| Q7/R7 | **Silent + logged**; no player-facing UI. |
| Q8/R8 | Logout (card-out) re-sends all stages; if any stage tainted, **suppress that side's logout save entirely** (clean stages already saved per-stage). Needs a **session-sticky per-side flag**. |

## Architecture (one new module, three small edits)

- **New:** `services::score_guard` — pure lock-free atomic taint state + readiness
  flag. Owns no detour (one-detour-per-target preserved).
- **Edit:** `custom_options_persistence` save trampoline — suppress before calling
  original, keyed on `savekind` (`savedata+0x74`: Stage→per-song, Logout→session);
  `init()` marks readiness; `load_receiver` resets session.
- **Edit:** `autoplay` — mirror taint on toggle; `enable()` fails closed on
  `!score_guard::is_available()`.
- **Edit:** `quick_restart_or_fail` — `trigger_fail` sets quick-fail taint;
  `trigger_restart` + gameplay-enter scene cb reset song taint; fails open.

## Implementation plan (10 steps)

Scaffolding without behavior change first (1–5), diagnostic-confirm the savekind enum
on cabinet (3) before branching on it, first end-to-end suppression at Step 6 (also
where the top risk R-A is validated live), logout + fail-closed layered on (7–8),
full cabinet acceptance (9), docs (10). Each step is `cargo check`-clean and
deployable; "tests" are `cargo check` + named log/behavior observations (no unit-test
harness in this repo).

## Open items deferred to early deploy (not design gaps — observe-before-trust)

- **R-A (highest):** does returning pretend-success (skipping the original
  `save_sender`) leave gamemdx's per-side busy flag uncleared and stall card-out?
  Resolved at Step 6 deploy; fallback specified (neuter the request via `savedata+0xF0`
  or clear the busy flag; trace save receiver `FUN_18002CA00` if needed).
- **R-B:** confirm `savekind` enum values (assumed 1/2/3) — Step 3 diagnostic.
- **R-C:** confirm per-stage (kind=2) is the authoritative score write, so logout-delta
  suppression of clean stages is harmless — Step 7 deploy; fallback is per-stage
  surgical drop (`ReflectSavePlayerData` kind=3 hook).

## Additional fixes (pre-existing bugs found during cabinet testing)

Five pre-existing bugs, all unrelated to score suppression, all root-caused via
diagnostic logging + Ghidra + live Cheat Engine, all fixed + cabinet-verified this
session.

### A. Custom-option row lifetime crash (`research/option-row-lifetime-crash.md`)

`EXCEPTION_ACCESS_VIOLATION` after rapid options-toggling + song-select scrolling.
The custom-options framework held raw pointers to game-allocated rows; the game
frees them on options-overlay close, but the framework only purged stale entries
lazily on the next menu open, so the scroll/visibility paths could write `+0xB8`
into freed rows. Fixed by detouring **`OptionForm::~OptionForm`** (new
`optionform_dtor` signature; AOB verified unique + cross-version on 20260526 and
20250805) to eagerly `clear_side` the closing side, plus defensive empty-guards on
the `+0xB8` writers. Files: `signatures.rs`, `custom_options/dtor_hook.rs` *(new)*,
`custom_options/mod.rs`, `custom_options/rows.rs`.

### B. 2-player custom-option load misrouting (`research/2p-options-load-side.md`)

In 2P sessions both players' network option loads routed to side 0 (P2 saw
defaults). The load receiver used `job[0]` as the side, which is always 0; the load
job carries no side index. Fixed with a **ddrcode join**: the load's ddrcode
(`*(*(job+0x18)+0x48)`) is matched to the per-side `PlayerWork+0x18`. Because
PlayerWork isn't populated until *after* the load, application is **deferred to
SONG_SELECT entry**. Files: `custom_options_persistence.rs` (+`player_work_table`
resolution via `init(signatures)`), `lib.rs` (pass `&signatures`).

### C. Filter-menu open/close crashes (`research/filter-menu-crash.md`)

Two near-identical crashes (reported by another operator): backing **out of** the
filter menu, and **loading into** it ("two filters active"). Same family as A —
`series_filter_scroll` caches raw filter-panel pointers (`STATE.entries[].this_ptr`)
and a per-frame loop derefs `+0x30`; the filter menu is an overlay (closes with no
scene change), so the panels free while we still point at them. **Close:** detour
**`FilterButton::~FilterButton`** (new `filterbutton_dtor` signature, unique +
cross-version on 0421/0526) → `deactivate_scroll()`. **Open:** `panel_builder_hook`
appended across opens with no reset, so reopening derefs stale entries — added a
**fresh-build-pass reset** at builder entry. Files: `signatures.rs`,
`services/series_filter_scroll.rs`.

### D. Clear-Type "FC" filter crash (`research/clear-type-filter-crash.md`)

Selecting the **Clear Type → FC** filter crashed (memcpy over-read, all gamemdx
frames). `series_expansion`'s `filter_label_builder_count` signature keys on
`MOV EDX,9`, matching **every** 9-entry filter-label builder — so it blanket-patched
the Clear Type builder too, repointing its label table at our version table.
Disambiguated: the VERSION builder is the one that seeds its result string with
`"DDR "` — patch only that site (new `builder_seeds_with_ddr` backward-scan). Files:
`mods/series_expansion.rs`.

### E. CAUTION-screen (scene 21) slowdown (`research/caution-screen-slowdown.md`)

After expanding WebUI preview images from ~30 to several hundred, scene 21 took
20–30s ~90% of runs, fast ~10% — nondeterministic across reboots with identical
assets. Cause: `handle_texture` did a per-open `Path::exists()` **filesystem stat**;
scene 21 preloads ~2,300 textures, so load time scaled with OS file-cache warmth.
Fixed with an **in-memory `_cache` index** built once at init (`cache_has` replaces
the stat), kept live on every cache write. Files: `avs_layeredfs/ifs_textures.rs`,
`atlas_cloner.rs`, `avs_layeredfs/mod.rs`.

## Status (2026-06-11) — ALL VERIFIED ON CABINET

**Score feature + all five additional fixes implemented and cabinet-verified.** All
code compiles clean under `cargo check --target x86_64-pc-windows-msvc`.

**Score-suppression code changes (5 files, all in `src/`):**
- `services/score_guard.rs` *(new)* — lock-free atomic taint state + readiness;
  owns no detour.
- `services/custom_options_persistence.rs` — `save_sender` trampoline derives
  `savekind` (`savedata+0x74`) + side, suppresses tainted saves (per-stage on
  per-song taint, latching session at actual suppression; logout on session-sticky
  flag) by returning pretend-success without calling the original; `init()` marks
  guard readiness; `load_receiver` resets session taint on card-in.
- `mods/autoplay.rs` — mirrors per-side taint on toggle; `enable()` fails closed.
- `mods/quick_restart_or_fail.rs` — `trigger_fail` sets quick-fail taint;
  `trigger_restart` + gameplay-entry scene cb reset per-song taint; fails open.
- `services/mod.rs` — registered `score_guard`.

**Verification (cabinet):**
- **Score suppression** — 4-song + 2P autoplay-vs-honest sessions: P1 autoplay
  suppressed every song + logout; P2 honest saved every song + successful logout.
  savekind enum confirmed (stage=2, logout=3); honest scores in backend DB. R-A
  resolved favorably (game retries 3×, gives up cleanly, brief "could not save"
  popup, no hang — accepted). R-C confirmed (per-stage saves authoritative).
- **Latch-timing fix** — confirmed: in the autoplay-P1/honest-P2 run, only side 0
  was latched (suppression fired only there), so P2's logout save succeeded. The
  per-side latch-at-suppression covers the toggle-then-honest edge case too.
- **Row-lifetime crash fix** — dtor detour confirmed installed; the crash no longer
  reproduces under deliberate rapid-toggle testing.
- **2P load fix** — confirmed: each player's network options load to the correct
  side, and network values override the JSON-primed cache as designed. (Two bugs
  fixed en route: load-time timing → deferred to SONG_SELECT; an init-order bug
  where the drain callback gated on `scene_manager::is_available()` before
  scene_manager init — fixed by dropping the gate.)
- **Filter open/close crashes** — confirmed: FilterButton dtor hook installs; close
  no longer crashes; reopen (incl. "two filters active") no longer crashes.
- **Clear-Type FC crash** — confirmed fixed; FC selects without crashing, PFC/others
  and the VERSION filter (custom series labels) still work.
- **CAUTION slowdown** — confirmed: scene 21 now loads reliably fast across reboots
  (the 90/10 nondeterminism is gone).

**Commit status:** the original score-suppression feature + 2P load + row-lifetime
crash were committed mid-session. The remaining fixes — filter open/close crashes,
Clear-Type FC crash, and the CAUTION-screen `_cache` index — are **pending commit**
by the maintainer.

## Deferred (not addressed this session)

- **Injected-series filter selection doesn't persist between songs** (stock series
  selections do). Reported by the operator; set aside to focus on the crashes.
  Likely related to how injected filter entries are rebuilt/freed per filter-open;
  no investigation done yet.

## Optional / deferred (not blocking)

- If the retry "could not save" popup on a suppressed save is ever undesirable, the
  noted alternative is a neutered-request or receiver-side approach — more RE, not
  currently planned.
- Network-vs-JSON precedence for mod options is "network wins" by design; revisit
  only if a JSON-wins policy is ever wanted.
