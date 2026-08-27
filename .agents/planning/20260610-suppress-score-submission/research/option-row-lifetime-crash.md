# Research: Custom-Option Row Lifetime Crash (pre-existing bug)

A crash surfaced during cabinet testing of the score-submission feature, but RE
proved it is **independent of that feature** — a pre-existing dangling-pointer bug
in the custom-options / options-scroll machinery. Captured here because it must be
fixed before release.

**Binary:** `gamemdx_20260526.dll` (Ghidra base `0x180000000`; runtime base this
session `0x7FF819B50000`). Hook DLL `ddr_world_hook.dll` runtime base
`0x7FF81FBA0000` (preferred `0x180000000`).

## Symptom

`EXCEPTION_ACCESS_VIOLATION` on the game thread after rapid options-toggling then
song-select scrolling. spice2x stackwalk showed the two innermost frames in
`ddr_world_hook` (symbolizer labeled them "DllMain" — nearest-export guess, not
literal), called from `gameMain`.

## Symbolization

Hook DLL crash frames (runtime → RVA via base `0x7FF840000000` in the crash
session; the on-disk DLL is stripped so functions are `FUN_*` in Ghidra at base
`0x180000000`):

| Frame | Runtime | RVA | Ghidra fn |
|---|---|---|---|
| faulting | `0x7FF84003D334` | `0x3D334` | `FUN_18003ce70` |
| caller | `0x7FF840009655` | `0x9655` | `FUN_180008fb0` |

`FUN_18003ce70` decompiles to the options-scroll mask path: it takes the
custom-options lock, references the panic-location strings
`src/services/custom_options/rows.rs` and `src/services/options_scroll.rs`, probes
the per-(side) scroll `HashMap`, writes `+0xB8` (the row "active" byte) into a list
of row pointers, then frees a temporary `Vec<*mut u8>` via `HeapFree` — the
`HeapFree`-arg setup at `0x18003d334` is the faulting instruction. The fault is a
**bad/stale row pointer** flowing into that path.

## Root cause (static, confirmed)

`RowSlot.row_ptr` (in `rows.rs`) points at **game-allocated** option rows. The
game frees those rows when the options menu closes (donor dtor → CRT free). But
`ROWS` is only purged of stale entries lazily, at the **start of the next builder
pass** (`builder_hook.rs:210` → `rows::clear_side(side)`). So between menu-close
and next-open, every `row_ptr` in `ROWS` is **dangling**, and any `+0xB8`-writing
path that runs in that window dereferences freed memory:

- `side_for_container` is the most dangerous: it dereferences `*(row_ptr+0x60)` for
  **every** stored slot (to match a container), so a single stale pointer faults it.
- `hide_show_when_excluded`, `row_ptrs_for_side`, `apply_mask`,
  `reapply_mask_for_side` all write/read `row_ptr` similarly.

The code's own assumptions were self-contradictory: `RowSlot`'s SAFETY comment
claimed rows are "valid for process lifetime," while `clear_side`'s comment
correctly said the game frees them on menu close. The crash proves `clear_side`
right — there was just no eager teardown to drive it.

**Not caused by the score feature:** in the crash session the score-suppression
code did nothing (save was `allowed`, no suppression ran; autoplay enabled
normally). The only score-feature change on the toggle path was an atomic store in
`autoplay_on_change`, which cannot corrupt the custom-options heap.

## Teardown event — found + LIVE-VALIDATED (Cheat Engine)

**`OptionForm::~OptionForm` @ gamemdx `0x18018DDA0` (RVA `0x18DDA0`).**

Found via Ghidra: it's the only concrete caller of the OptionForm sub-object
release `FUN_18018e980`; its prologue loads the three OptionForm MI vtables via
`LEA` (`0x18037D018`, `0x18037D060`, `0x18037D078`) into `[RCX]`, `[RCX+0x28]`,
`[RCX+0xC0]` — the MSVC dtor vptr-reset pattern.

Validated live with a logging breakpoint at runtime `0x7FF819CDDDA0`:

| Test | Result |
|---|---|
| Options open (idle) | **0 hits** — does not fire while menu alive |
| 1P session, 1 close | **1 hit**, RCX = `OptionForm*` |
| 1P, open+close ×2 | **2 hits** (one per close) — fires reliably every close |
| 2P session, single close | **2 hits**, two distinct `this` (`0x1B9CACE0`, `0x28B22CC0`) — one OptionForm per side, both torn down together |
| `RCX+0x228` per side | P1 `this` → **0**, P2 `this` → **1** — confirms `OptionForm+0x228` = player side, and the field **survives into the destructor** |

`RAX` at entry = `0x7FF819ECD018` = `gamemdx+0x37D018` (matches the dtor's first
vtable LEA) — confirms we're in the right function.

**Key behavioral finding (maintainer-observed, consistent with 2× hit/close):** the
options menu is **synchronized across players** — either player opening/closing
opens/closes it for both. So the feared "P1 closes while P2's form is still live"
scenario **cannot happen**; both OptionForms always tear down together. This makes
per-side clearing on the dtor safe (no risk of yanking a still-live other-side form).

## Fix design (validated approach)

1. **Detour `OptionForm::~OptionForm`** (new signature `optionform_dtor`, AOB on the
   prologue + 3 vtable LEAs). On entry read `side = *(this+0x228)` (intact at entry),
   call the original dtor, then `rows::clear_side(side)` to drop that side's
   row pointers before any `+0xB8` path can run against them. Per-side (not both):
   `+0x228` is proven reliable, and per-side avoids disturbing any unrelated state.
   Owned by `custom_options` (one detour per target; no existing hook on this fn).
2. **Defense-in-depth guards** on every `+0xB8` writer
   (`side_for_container`, `hide_show_when_excluded`, `row_ptrs_for_side`,
   `apply_mask`, `reapply_mask_for_side`): early-return when no rows exist for the
   side, so a stale deref can't happen even via an unforeseen free path. (Maintainer
   chose "add guards too" alongside the teardown clear.)

## Implementation (landed, cargo check clean — pending cabinet re-test)

- `signatures.rs` — new `optionform_dtor` AOB (the unique prologue+body pattern
  above).
- `custom_options/dtor_hook.rs` *(new)* — `GenericDetour` on `OptionForm::~OptionForm`;
  reads side at `this+0x228`, calls `rows::clear_side(side)`, then the original dtor.
  `catch_unwind`-wrapped. Initialized from `custom_options::init`.
- `custom_options/mod.rs` — `pub mod dtor_hook;` + init call; corrected the
  `RowHandle` lifetime doc/SAFETY note.
- `custom_options/rows.rs` — corrected the now-wrong `RowSlot` SAFETY + `clear_side`
  comments (rows are valid only while tracked; dtor hook clears on close); added
  empty-side early-return guards to `hide_show_when_excluded` and (implicitly)
  `side_for_container`.

**Cabinet re-test needed:** reproduce the original crash sequence (rapid options
toggling, close, song-select scrolling) and confirm no access violation; verify the
`custom_options/dtor_hook: OptionForm dtor detour installed` log line at boot and
`cleared N stale row(s)` on options close.

## Addresses

### Signature (unique + cross-version verified)

The bare dtor prologue is the generic MSVC 3-vtable shape (3 matches on 20260526).
Disambiguated by the body: after the three vtable writes it does
`ADD RCX, 0xC0` then `CALL <sub-object release>; NOP; MOV RBX,[RSI+0x238]`
(shared_ptr release of the field at `+0x238`). The full pattern (3 vtable-LEA
disp32s + the CALL rel32 wildcarded):

```
48 89 4C 24 08 57 48 83 EC 30 48 C7 44 24 20 FE FF FF FF 48 89 5C 24 48
48 89 6C 24 50 48 89 74 24 58 48 8B F1 48 8D 05 ?? ?? ?? ?? 48 89 01
48 8D 05 ?? ?? ?? ?? 48 89 41 28 48 8D 05 ?? ?? ?? ?? 48 89 81 C0 00 00 00
48 81 C1 C0 00 00 00 E8 ?? ?? ?? ?? 90 48 8B 9E 38 02 00 00
```

| Build | Match | Unique? | Confirmed |
|---|---|---|---|
| 20260526 | `0x18018DDA0` | ✅ 1 hit | decompile + live CE |
| 20250805 stock | `0x1801786B0` | ✅ 1 hit | decompile shows `OptionForm::vftable` writes (same struct shape) |

### Addresses

| Symbol | Ghidra (file-rel) | Notes |
|---|---|---|
| `OptionForm::~OptionForm` (20260526) | `0x18018DDA0` | hook target |
| `OptionForm::~OptionForm` (20250805) | `0x1801786B0` | cross-version match |
| OptionForm MI vtable #1 | `0x18037D018` | LEA'd at dtor entry |
| OptionForm MI vtable #2 | `0x18037D060` | LEA'd at dtor entry |
| OptionForm MI vtable #3 (IOptionElement-side) | `0x18037D078` | LEA'd at dtor entry |
| `OptionForm + 0x228` | — | player side (0=P1, 1=P2), live-confirmed |
| row builder `FUN_180164710` | `0x180164710` | matched by existing `row_builder_fn_prologue` AOB |
| option panel builder `FUN_18018e060` | `0x18018e060` | sole caller of row builder; builds dummy_item_usr etc. |
