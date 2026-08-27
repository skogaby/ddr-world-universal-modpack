# Research: Filter-Menu Crash (series_filter_scroll dangling panel pointers)

**Status:** Root cause identified statically (Ghidra); fix implemented +
`cargo check`-clean; awaiting live repro on a faithful copy of the reporter's
install to confirm.

## Implementation (landed, pending live confirmation)

- `signatures.rs` — new `filterbutton_dtor` AOB (verified unique + cross-version,
  see below).
- `series_filter_scroll.rs` — detour on `FilterButton::~FilterButton`
  (`filterbutton_dtor_hook`): on entry calls `deactivate_scroll()` (clears
  `STATE.entries` + `TRACKED_LAYERS`, stops the loop), then the original.
  `catch_unwind`-wrapped; best-effort (graceful WARN if the signature/​detour
  fails — scene-change deactivation stays as a partial backstop).

Soundness: the dtor fires on the render thread (same as the scroll loop), so the
`STATE` mutex is uncontended same-thread; `deactivate_scroll` touches only
`entry.layer_id` (a BM2D id libafp validates), never the dangling `entry.this_ptr`;
gated on `state.active` so repeated per-button dtor calls are no-ops after the
first. One-detour-per-target preserved (new signature, sole consumer).

## Report

A friend's crash (his build = renamed `OmniMAX.dll`, hook **v1.25.0**, gamemdx
**2026042100**). Clarified scenario: *"Played a normal credit, went into the Filter
Menu and selected an Expanded Version (a net-new injected series from the Series
Expansion mod) for the final song. Crashed when backing out of the filter menu."*
Also reported: an injected-series filter selection **doesn't persist between
songs**, while stock series selections do (secondary, possibly related).

Crash log: `EXCEPTION_ACCESS_VIOLATION`, two innermost frames in OmniMAX (our DLL),
called from gamemdx. The stackwalk ended with `StackWalk64-Endless-Callstack!`, so
**only the innermost ~2 frames are trustworthy**; outer frames are unreliable.

## Binaries

- `crashbuild.dll` = the reporter's exact OmniMAX build (Ghidra; stripped, 3.6 MB).
  **Predates this session's fixes** (no `dtor_hook`, no `score_guard`).
- gamemdx **0421** (`gamemdx.dll` in Ghidra) matches the reporter's `soft_id_code`
  2026042100 and all crash-chain return addresses resolve to valid function bodies
  there → crash-chain symbolication is against 0421.
- Could not cleanly recover OmniMAX's runtime base from the log (no base print;
  bracketing from observed addrs left too many candidates), so the two faulting
  frames inside our DLL were **not** individually symbolized. The gamemdx caller
  chain was symbolized instead and is decisive.

## Crash chain (gamemdx 0421, innermost trustworthy frames)

| Frame | Fn | Role |
|---|---|---|
| fault | (in OmniMAX) | virtual method dispatched into our DLL |
| caller | `FUN_18004b4e0` (+0x4B6A7) | **focus-cursor**: walks a UI-item vector (`[this+0x68]..[+0x70]`, stride 8), on focus change calls `(**(code**)(*(item+0x28)+8))(item+0x28, 0/1)` and toggles focus bytes `+0x30`/`+0x31` |
| … | `FUN_1800456c0` (appears 2×) | **filter category panel** item update — references AFP children `"category"`, `"insert_picture_usr"`, `"category_name_usr"`, `"cursor_left_usr"`, `"cursor_right_usr"`. Unambiguously the **filter menu** category strip (VERSION = category 2). |

This confirms the crash is in the **filter-menu** code path (not the options menu),
corroborating the reporter's wording.

## Root cause (static, confirmed)

`series_filter_scroll` (`src/services/series_filter_scroll.rs`) captures the filter
category panel objects in `panel_builder_hook` (keyed on `this+0xF0 == 2` = VERSION)
and stores their raw pointers in `STATE.entries[].this_ptr`. A scheduled
`scroll_update_frame` loop (via `widget_renderer::run_on_render_thread`,
re-scheduling itself) dereferences **`*(entry.this_ptr + 0x30)`** every frame to
find the focus cursor.

The filter menu is an **overlay inside SONG_SELECT** — closing it does **not** change
the scene. But `series_filter_scroll` only deactivates (`deactivate_scroll` /
clearing `entries`) on a **scene change away from SONG_SELECT** (its
`on_scene_change` callback) or if `scroll_update_frame`'s liveness probe (only checks
`entries[0].layer_id` via `for_each_active`) happens to miss. So on filter-menu
close-without-scene-change, the game frees the panel objects while
`STATE.entries[].this_ptr` still point at them and the update loop is still
scheduled → `*(this_ptr+0x30)` reads freed memory → access violation.

### Teardown event (the fix's hook target) — identified

The filter category panel is the shared_ptr at `panel + 0x178/0x180`. Its
enter/leave handler is **`FUN_180135a70(this, enter_flag)`** (slot 3 of the filter
panel's vtable at `0x180370b60`, build 0421):
- `enter != 0` → `FUN_180134a30` (= our hooked `filter_panel_builder`, builds entries)
- always → **`FUN_180134e00(this)`** releases the panel shared_ptr at `+0x178/+0x180`
  (refcount dec → dtor vtable slots `[0]`/`[+8]` when zero). **Structurally identical
  to `OptionForm`'s teardown `FUN_18018e980` that we hooked for the options crash.**

So `FUN_180134e00` (or the slot-3 handler with `enter==0`) is the close/teardown
signal — the analogue of `OptionForm::~OptionForm`.

## Relationship to the already-fixed options crash

**Same family, different overlay/service.** The options-row crash (fixed this
session via the `OptionForm::~OptionForm` dtor hook + row-pointer invalidation) and
this filter crash are both "overlay closes with no scene change → we still hold raw
pointers the game just freed → deref-after-free." The options fix does **not** cover
`series_filter_scroll`; this needs its own teardown-driven invalidation.

Note Series Expansion itself is **not** the dangling-object source: it extends a
data table and patches the UI loop to build `8+N` entries, so the game allocates the
FilterButton objects with the game's own vtable (no mod-synthesized vtable to
dangle). The dangling pointers are the **panel `this_ptr`s held by
`series_filter_scroll`**, not anything Series Expansion injects.

## Teardown hook — VERIFIED (cross-version)

Found the precise destructor, analogous to `OptionForm::~OptionForm`:
**`FilterButton::~FilterButton`** (the destructor body, called by the scalar
deleting dtor at vtable slot 1). It writes `sequence::selectmusic::FilterButton::
vftable` into the object's two vptr slots, then tears the button down. It fires
**per filter button** as the category panel is destroyed on filter-menu close.
`FilterButton` is exactly the per-entry object `series_filter_scroll` tracks
(`entry.this_ptr`), so this is the right invalidation signal.

- 0421: `FUN_180134260` (real dtor body); scalar-deleting dtor `FUN_180134230`
  (slot 1 of panel vtable `0x180370b60`).
- 0526: `FUN_180133ba0` (verified — same `FilterButton::vftable` writes + structure).

**Unique cross-version AOB** (prologue + two `FilterButton::vftable` LEA/writes +
the distinctive `CALL <release>; NOP; MOV RCX,[RBX+0x1B0]` tail; 3 vtable-LEA/​CALL
disp32s wildcarded):
```
48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 48
48 89 6C 24 50 48 89 74 24 58 48 8B D9 48 8D 05 ?? ?? ?? ?? 48 89 01
48 8D 05 ?? ?? ?? ?? 48 89 41 28 E8 ?? ?? ?? ?? 90 48 8B 8B B0 01 00 00
```
Verified unique: 0421 → `0x180134260`, 0526 → `0x180133ba0` (1 match each).

**Fix (chosen): clear-all on any FilterButton dtor.** Detour `FilterButton::
~FilterButton`; on entry call `series_filter_scroll::deactivate_scroll()` (clear
`STATE.entries` + `TRACKED_LAYERS`, stop the update loop), then call the original.
The panel's buttons all free together on close, so the first dtor reliably signals
"panel going away" and fully closes the deref-after-free window. Owned by
`series_filter_scroll` (new `filterbutton_dtor` signature).

## Second crash: OPENING the filter menu ("two filters active at once")

A later report: crash *loading into* the filter menu, "related to two filters
being active at the same time." Log (`log2.txt`, pre-fix build — no FilterButton
dtor hook installed). Symbolized against gamemdx 0421 (base `0x7ffc01150000`):

The crash chain is **the same as the close crash except one frame**:
- close-filter frame 4 = `0x16091F` (`FUN_1801606b0`)
- open-filter  frame 4 = `0x135AA8` (`FUN_180135aa0` — a filter-panel open-path
  method in the same `0x134xxx`–`0x135xxx` cluster as the builder/enter handler)

Everything else identical, including the faulting caller `FUN_18004b4e0` (the
focus-cursor) and the in-DLL fault. ⇒ **same dangling-FilterButton-deref root
cause**, reached via the open path instead of the close path.

### Accumulation bug (root cause of the open crash)

`panel_builder_hook` **appends** to `STATE.entries` every build pass and only
arms scroll when `entries.len() == total_expected` — with **no reset between
filter opens**. The game frees the previous open's FilterButtons without notice,
so on reopen `entries` still holds the prior pass's freed pointers and the new
pass appends to them. Consequences: (a) the open path's focus cursor dereferences
the stale `this_ptr`s → crash; (b) `entry_count` overshoots `total_expected` so
scroll silently never re-activates. "Two filters active at once" = a build pass
re-entering while stale entries persist.

The close-side `FilterButton::~FilterButton` hook only helps if the old buttons
are destroyed *before* reopen; the open crash needs the builder itself to start
each pass clean.

### Fix (chosen): reset on fresh build pass

In `panel_builder_hook`, before pushing, detect a fresh pass and clear stale state
inline (entries, TRACKED_LAYERS, scroll offset, active flag). "Fresh" =
`entries.len() >= total_expected` (prior pass completed) **or** `layer_id` already
tracked (rebuild re-entered). Mirrors the options "clear_side at builder entry"
pattern. Cleared inline (not via `deactivate_scroll`) because the hook already
holds the `STATE` lock — calling `deactivate_scroll` would deadlock. Lock order
audited: every site taking both locks uses STATE→TRACKED_LAYERS; `set_position_hook`
takes TRACKED_LAYERS alone. Together with the close-side dtor hook, both the open
and close crash paths start/​end with clean state.

## Fix direction (to design after live repro confirms)

Mirror the options fix: stop the per-frame loop from dereferencing freed panels on
filter close. Options:
1. **Teardown hook** — detour `FUN_180134e00` (filter panel close; find a
   cross-version AOB analogous to `optionform_dtor`) and `deactivate_scroll()` there,
   so `entries`/`TRACKED_LAYERS` are cleared the instant the panel frees.
2. **Harden the loop** — make `scroll_update_frame`'s liveness check robust (validate
   every entry, not just `entries[0]`) and/or stop relying on scene-change for
   deactivation. Cheaper but less precise than (1); likely do both.

## Open verification (live repro on faithful copy of reporter's install)

1. Reproduce on the current build: credit → filter menu → select injected version →
   back out. Current build has the options fix but `series_filter_scroll` is
   unchanged, so it **should still crash** if this analysis is right.
2. (If reproour build differs) confirm the teardown via the same approach used for
   OptionForm: log/breakpoint `FUN_180134e00` on filter close.
3. Investigate the secondary "injected series filter doesn't persist between songs"
   report — may share a cause (injected entries rebuilt/freed per filter-open).
