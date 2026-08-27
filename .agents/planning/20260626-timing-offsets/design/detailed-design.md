# Detailed Design — Timing Offsets

## Overview

A new mod (`timing-offsets`) for the DDR World hook DLL that exposes the game's four
integer **timing-offset** values — `SOUND_OFFSET`, `INPUT_OFFSET`, `RENDER_OFFSET`,
`BOMB_FRAME_OFFSET` — as operator-configurable settings. It is the 64-bit hook-based port
of the 32-bit `patches.js` sound-offset hex hack, expanded to the whole integer record
(RE'd fresh this session in `docs/hex_edit_porting.md` Hack 4 + `research/r1`–`r4`).

The values are **global / cabinet-wide** (the game publishes them into one process-wide
config map), so per the maintainer's governing principle they are configured **only** in
the DLL-managed overlay menu (`mod_menu`, opened by triple-pressing numpad 0) plus the
`mod-config.json` file — **not** the game's native per-player options UI.

This is a **two-part effort**:

- **Part I — `mod_menu` overlay infrastructure (reusable):** add typed **scalar rows** and
  **parent/child visibility gating** to the overlay, switch navigation to the **cabinet
  menu buttons** (with **Start-held = coarse adjust**), and **suppress those menu-button
  inputs from the game** while the overlay is open (closing a gap that today exists only for
  the numpad).
- **Part II — the `timing-offsets` mod:** register a master enable toggle + four child
  scalar rows in the overlay, and apply/seed/persist the four offset values by hooking the
  game's config-map **int setter** so our values win at every publish; the game then latches
  them into the `GamePlayActor` at the next gameplay entry.

## Detailed Requirements (consolidated from idea-honing.md)

1. **Fields:** the four i32 offsets only — `SOUND_OFFSET`, `INPUT_OFFSET`, `RENDER_OFFSET`,
   `BOMB_FRAME_OFFSET`. `HIGH_PRECISION_INPUT` bool is **out of scope**.
2. **Config surface:** `mod-config.json` + the DLL **overlay menu** only; **not** the game's
   native Mods tab. (Principle: per-player → game options; global → DLL overlay.)
3. **Menu structure:** the mod's own enable toggle is the **master boolean**; the four
   scalar rows are **child rows** nested under it, shown only when master is ON
   (`ShowWhen`-style gating), navigated inline.
4. **Ranges:** all four uniform `[-1000, 1000]`, fine step **1**, coarse step **20**
   (Start-held). Negatives allowed. Plain signed-integer display. Clamped to range. Stock
   defaults 87/28/17/0.
5. **Overlay input:** cabinet menu buttons are **primary** nav (Up/Down navigate, Left/Right
   adjust, **Start-held = coarse**); `2/4/6/8` numpad retained as a **fine-only** alias;
   open/close stays **triple-0**; **suppress the five cabinet menu-button exports from the
   game** while the overlay is open.
6. **Liveness:** best-effort live — write the value on every change; **seed from config at
   boot**. (RE finding R2: all four are **latched at gameplay entry**, so a change applies
   on the **next song**, not mid-song. Documented; no forced re-latch.)
7. **Persistence/schema:** typed `timing_offsets` JSON section (four named integer keys).
   Master toggle = the mod's `mods["timing-offsets"]` entry; **OFF = revert to stock**.
   Scalar changes **persist immediately**.
8. **Safety:** graceful degradation, two tiers — the **offset-apply mechanism is
   load-bearing** (can't resolve ⇒ whole mod self-disables cleanly); the **overlay UI is
   non-fatal** (rows fail ⇒ mod still works via config-file boot-seed). Never panic across
   FFI; never crash.
9. **Overlay presentation:** labels `Sound Offset` / `Input Offset` / `Render Offset` /
   `Bomb Frame Offset`; child rows indented; plain signed-integer value column.
10. **Hints:** all four ship hint text (R3 confirmed all four semantics from the binary).
11. **Identity/scope:** id `timing-offsets`, name "Timing Offsets". Hack 6 preset selector
    out of scope.

## Architecture Overview

```mermaid
graph TD
    subgraph Game["gamemdx.dll (game thread)"]
        PUB["boot publisher\nFUN_18002bbd0\n(once at subsystem init)"]
        PUB2["settings re-publisher\nFUN_18002e2b0\n(on operator timing-adjust)"]
        SET["config-map int setter\nFUN_1801acbf0(key, value)\n(8 call sites, all timing)"]
        MAP["config map\nDAT_1806ebcf0"]
        CTOR["GamePlayActor ctor\nFUN_18005b4c0\nlatches map -> actor+0x16c..+0x188"]
        PUB --> SET
        PUB2 --> SET
        SET --> MAP
        MAP -->|"getter, at gameplay entry"| CTOR
    end

    subgraph ModII["timing-offsets mod (Part II)"]
        HOOK["setter detour\nif key in {4 offsets} & master ON:\n  substitute our value\n  capture stock (first boot write)"]
        STATE["TIMING_STATE (static)\n{configured[4], stock[4], master_on}"]
        CFG["mod-config.json\ntiming_offsets{...}"]
    end

    subgraph ModI["mod_menu overlay (Part I)"]
        MASTER["master toggle row\n(timing-offsets enable)"]
        SCAL["4 scalar child rows\nShowWhen master==ON"]
        SUPP["menu-button suppression\n(arkMDXGetStart/Up/Down/Left/Right)"]
    end

    SET -. detour .-> HOOK
    HOOK --> STATE
    STATE --> CFG
    MASTER -->|on/off| STATE
    SCAL -->|on_change(value)| STATE
    STATE -->|"push live via original setter"| SET

    classDef game fill:#1d3b53,color:#fff;
    classDef mod fill:#2d5016,color:#fff;
    class PUB,PUB2,SET,MAP,CTOR game;
    class HOOK,STATE,CFG,MASTER,SCAL,SUPP mod;
```

### Why hook the int setter (one detour, complete coverage)

R1 established the int setter `FUN_1801acbf0` (`FUN_1801ae460` on the cabinet build) has
**exactly 8 call sites, all timing** (4 in the boot publisher, 4 in the settings
re-publisher). Hooking it and filtering on the four keys intercepts **every** write to the
offsets and nothing else — so our override survives both the boot publish and any operator
re-publish, with a single detour and no `.text`/`.rdata` byte-patching. The game's
`GamePlayActor` ctor then latches our value at the next gameplay entry (R2). This obeys
"one detour per target function" (the setter is hooked once, by this mod alone).

## Part I — `mod_menu` overlay infrastructure

### I.1 Typed rows + parent/child visibility

Today `mod_menu` renders a flat list of registry mods, each a boolean `[ON]/[OFF]`. We
generalize it to a list of **typed rows** while leaving the existing mod-toggle rows
behaving exactly as before.

New row model (in `mod_menu.rs`). `RowKind` is a deliberately small, **extensible** enum —
two variants now (`Boolean`, `Scalar`); a future `Enum` variant (mirroring the in-game
`custom_options` enum rows) can be added later without restructuring. We do **not**
speculatively design future variants now (maintainer guidance: extensible, not
over-generalized).

```rust
enum RowKind {
    Boolean { value: bool },                   // on/off row (the existing mod toggles ARE these)
    Scalar  { value: i32, min: i32, max: i32,  // numeric input row
              step_fine: i32, step_coarse: i32 },
    // future: Enum { value: i32, choices: Vec<(i32, String)> } — not built now
}
struct MenuRow {
    key: String,                               // stable row id (e.g. "timing-offsets", "sound_offset")
    label: String,
    hint: String,                              // rendered in the desc widget
    kind: RowKind,
    indent: u8,                                // 0 = top-level, 1 = child (visual indent)
    // ShowWhen analog: visible only when the named row's value matches.
    visible_when: Option<(String /*parent row key*/, i32 /*want value; bool as 0/1*/)>,
    on_change: Option<Arc<dyn Fn(i32) + Send + Sync>>,   // fires on toggle/adjust; bool passes 0/1
}
```

- **The existing mod-list toggles become `Boolean` rows.** Today's flat list of registry
  mods is just a list of `Boolean` rows whose `on_change` flips the mod's enabled state (via
  the existing `toggle_callback`) and persists via `config::save_mod_states`. This is a clean
  generalization — "toggle a mod" stops being a special row type and becomes a `Boolean` row
  with a particular `on_change`. (The registry-sourced rows keep `key = mod id`.)
- **Row sourcing.** The overlay's row list = the registry-derived `Boolean` rows (built from
  `entries_callback`, as today) **plus** rows contributed by mods via a new registration API
  on `mod_menu` (see I.4). The timing mod's master toggle is just its registry `Boolean` row;
  it contributes the **four `Scalar` child rows** with `indent=1` and
  `visible_when=Some(("timing-offsets", 1))`.
- **Visibility filtering.** When (re)building the visible list each refresh, drop any row
  whose `visible_when` parent row's value doesn't match. The existing
  `selected_index`/`scroll_offset`/`adjust_scroll`/cursor logic then runs over the filtered
  list unchanged (so collapsing the children when master is OFF Just Works).
- **Rendering.** Reuse a `Slot`'s three widgets: name = (indented) label, desc = hint,
  status column = `[ON]/[OFF]` for `Boolean` (unchanged) or the signed integer for `Scalar`
  (plain, per Q9). Indent via leading spaces or a small X-offset on the name widget when
  `indent>0`.

### I.2 Cabinet-button navigation + Start-held coarse adjust

`handle_exclusive_input` already accepts `MENU_UP/DOWN/LEFT/RIGHT` alongside `NUM_8/2/4/6`.
Changes:

- On **Up/Down** (cabinet or numpad alias): navigate (unchanged).
- On **Left/Right** when the selected row is a `Scalar`: adjust `value` by `step_coarse` if
  `input_manager::get_button_state(player) & button::START != 0`, else `step_fine`; clamp to
  `[min,max]`; if changed, fire `on_change(value)` and refresh the row. When the selected row
  is a `Boolean`: toggle it (Left=off/Right=on, as today) and fire its `on_change(0|1)`.
- **Player for the hold check:** the overlay is global; use whichever side the navigation
  event arrived on (the `InputEvent.player`) to read `get_button_state`. Start on either side
  counts as coarse (read both sides, OR the START bit) so it works regardless of which pad
  the operator uses.
- Open/close stays triple-0 (`on_zero_pressed`, unchanged). Numpad `4/6` adjust is fine-only
  (no hold semantics) — acceptable per Q5.

### I.3 Game-side suppression of the five menu-button exports

Today only `arkMDXGet10Key` is detoured for game-side suppression. Add the same treatment
to the five menu-button getters so cabinet-button nav doesn't bleed into the game while the
overlay is open.

In `input_manager`:

- Keep `GenericDetour<TriggerHoldFn>` handles for `arkMDXGetStart/Up/Down/Left/Right`
  (signature `fn(i32, *mut u32, *mut u32)` — already resolved in `ArkExports`), installed in
  `init()` next to the existing get_10key detour.
- Each detour calls the original, then **if** `IS_INPUT_SUPPRESSED && !IN_MODPACK_POLL`,
  zeroes `*trigger` and `*hold` for the game-side caller. The modpack's own poll already sets
  `IN_MODPACK_POLL` around its reads, so it keeps seeing real state.
- No API change: `mod_menu::open/close` already calls `set_input_suppressed(true/false)`.

> Runtime caveat: whether zeroing those out-params fully stops the game from acting on the
> buttons is the one thing static analysis can't guarantee — confirm on the cabinet (numpad
> suppression via the identical mechanism is strong precedent). Ship a diagnostic build that
> logs when suppression is active and verify no game-side menu movement underneath the open
> overlay.

### I.4 New `mod_menu` registration API (for mods to contribute rows)

```rust
// mod_menu.rs (called from a mod's enable(), on or before first menu open)
pub fn register_scalar_row(spec: ScalarRowSpec) -> Result<(), MenuError>;
pub fn set_scalar_value(row_key: &str, value: i32);   // reflect external/config changes
pub fn remove_rows_for(mod_id: &str);                 // cleanup on mod disable
```

`ScalarRowSpec { key, label, hint, parent_mod_id, min, max, step_fine, step_coarse,
initial, on_change }`. Rows are stored in `ModMenuState` and merged into the visible list
after the mod-toggle rows. (Kept intentionally small; only what Part II needs.) If
registration fails or the service is unavailable, the caller degrades to config-only
(Part II / Q8).

## Part II — the `timing-offsets` mod

### New module: `src/mods/timing_offsets.rs`

Single-file mod implementing the `Mod` trait. Registered in `src/lib.rs`.

#### Signature + derivation (added to `src/core/signatures.rs`)

The int setter must be resolved **semantically**, not by its prologue: the prologue AOB is
NOT unique (it matches the setter AND a byte-identical twin that sets a different config map;
they differ only in the RIP-relative map global — see R1 correction). Instead, anchor on the
publisher's `LEA RCX,[SOUND_OFFSET]; CALL setter` site.

| Name | Kind | Resolves |
|---|---|---|
| `timing_set_call_landmark` | AOB signature | the publisher's 4 consecutive config-set pairs; first match = the SOUND_OFFSET set-pair |
| `timing_config_set_int` | derived | `decode_call_rel32(landmark + 0x0A)` → the int setter |

Landmark pattern (verified **unique to the publisher** on both builds — 3 overlapping hits of
the 4-call run, all inside the publisher):
```
8B 55 ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ?? 8B 55 ?? 48 8D 0D ?? ?? ?? ?? E8 ?? ?? ?? ??
```
Each pair = `MOV EDX,[RBP+d]; LEA RCX,[rip+key]; CALL setter`. The first `E8` (landmark +0x0A)
is the SOUND_OFFSET set-call; `decode_call_rel32` it to get the setter. A
`derive_timing_config_setter()` helper in `resolve_derived()` does this (mirrors the existing
`derive_*` idiom). `timing_config_set_int` is the **load-bearing** address: if the landmark
doesn't resolve or the derived call target is null, the whole mod self-disables (Q8).

> Secondary anchor (provenance / fallback only): record-builder inline pair
> `C7 45 ?? 57 00 00 00 C7 45 ?? 1C 00 00 00` (R1).

#### State (module statics; game-thread + config-thread access)

```rust
struct TimingState {
    configured: [i32; 4],     // SOUND, INPUT, RENDER, BOMB — current desired values
    stock: [Option<i32>; 4],  // captured stock (value the game first tried to publish)
    master_on: bool,          // mirrors mods["timing-offsets"] enable
}
static TIMING_STATE: ... // static mut + addr_of! guard idiom, or Mutex; see note
static SETTER_HOOK: Option<GenericDetour<SetIntFn>>
static KEY_HASHES: [u32; 4]   // FNV-1a of the four key names, computed once at init
```

Key index order is fixed: `[SOUND, INPUT, RENDER, BOMB]`. The setter detour runs on the
game thread; the overlay `on_change` and master-toggle callbacks run on the render/input
thread. **Decision: use a small `Mutex<TimingState>`** — the setter hook is not a per-frame
hot path (it fires only on publish + our explicit pushes, a handful of times), so lock cost
is negligible and it's clearer than per-field atomics. The CLAUDE.md "no Mutex across
`run_on_render_thread`" rule does not apply (we never schedule render work while holding it).
The setter hook must not allocate or block; it takes the lock briefly to read/update and
releases before forwarding to the original.

#### Setter detour

```
type SetIntFn = unsafe extern "C" fn(*const u8 /*key*/, i32 /*value*/) -> i64;

fn set_int_hook(key, value) -> i64:
    catch_unwind:
        idx = match_key(key)            // FNV-1a hash compare against KEY_HASHES (or strcmp)
        if idx is Some:
            st = lock(TIMING_STATE)
            if st.stock[idx].is_none(): st.stock[idx] = Some(value)   // capture genuine stock
            if st.master_on:
                value = clamp(st.configured[idx], -1000, 1000)        // substitute our value
    return original(key, value)
```

- `match_key` hashes the incoming key with the same FNV-1a (seed `0x811c9dc5`, prime
  `0x1000193`) and compares to the four precomputed hashes; falls back to `strcmp` against the
  four ASCII names if desired (the key arg is a readable C-string — R1). Non-timing keys
  never reach this setter (R1 census), but matching defensively is cheap.
- **Stock capture:** the first time we observe a write for a key (the boot publish), record
  the pre-substitution value as `stock[idx]` — the genuine value the game would have used,
  preset/settings-accurate (R4).
- **Master ON:** substitute our configured value. **Master OFF:** pass through unchanged.

#### Applying changes (live push + boot seed)

- **Boot seed:** the boot publisher always runs at subsystem init and writes all four keys
  via the (now-hooked) setter; with `master_on` true the hook substitutes our configured
  values — so the boot publish *is* the seed. (The hook must be installed before subsystem
  init; the mod's `enable()` runs well before that — confirm via a one-shot log on first
  hook hit.)
- **Live change (overlay):** on a scalar `on_change(value)`, store
  `configured[idx]=clamp(value)`, persist (below), and **call the original setter directly**
  `set(key, value)` to push the new value into the live map immediately (update-only is fine
  — the key exists post-boot). Effect latches next song (R2).
- **Master toggle:** ON → push all four configured values via the setter (live), so the map
  reflects them. OFF → push each `stock[idx]` (or the known default 87/28/17/0 if never
  captured) via the setter to **revert to stock**. Effect latches next song.

#### Persistence + config schema

Add a typed section to `ConfigFile` (`src/mods/config.rs`):

```rust
#[derive(Deserialize, Clone, Debug, Default)]
pub struct TimingOffsetsConfig {
    #[serde(default = "default_sound")]  pub sound_offset: i32,   // 87
    #[serde(default = "default_input")]  pub input_offset: i32,   // 28
    #[serde(default = "default_render")] pub render_offset: i32,  // 17
    #[serde(default)]                    pub bomb_frame_offset: i32, // 0
}
// ConfigFile gains: timing_offsets: Option<TimingOffsetsConfig>
```

- **Load at boot:** in `init()`/`enable()`, read `config::get().timing_offsets` into
  `configured` (clamped); absent → stock defaults 87/28/17/0.
- **Save on change:** each scalar change writes the whole `timing_offsets` object via
  `config::save_json_key("timing_offsets", json)` (read-modify-write, preserves other keys),
  matching the immediate-persist decision (Q7).
- **Master toggle persistence:** unchanged — it's the mod's entry in the `mods` map, saved by
  the existing `mod_menu` → `config::save_mod_states` path.

#### Mod lifecycle (`Mod` trait)

- `id() = "timing-offsets"`, `name() = "Timing Offsets"`,
  `description() = "Adjust the game's global timing offsets (sound/input/render/bomb)"`.
- `required_signatures() = &[]` (graceful: resolve best-effort in `init`, self-disable in
  `enable` if the setter AOB is missing — matches `center_arrows_single`).
- `init(ctx)`: resolve `timing_config_set_int`; precompute `KEY_HASHES`; load config into
  `configured`. Return true.
- `enable()`:
  1. If the setter address didn't resolve → log warning, **self-disable** (no hook, no rows),
     return (this is the load-bearing failure, Q8).
  2. Install the setter detour. On failure → self-disable.
  3. Set `master_on = true` (the mod is enabled). Push configured values live (so a
     mid-session enable applies next song).
  4. **Best-effort** register the four scalar rows in `mod_menu` (Part I API). If that fails
     or `mod_menu` is unavailable → log, continue (config-only mode; the offsets still apply).
- `disable()`: revert to stock (push `stock`/defaults via setter), remove the setter detour,
  `remove_rows_for("timing-offsets")`, set `master_on=false`. (Disable = master OFF.)

> Note the master toggle and the mod's enable/disable are the same thing (Q3-A): toggling
> the `timing-offsets` row in the overlay enables/disables the mod, which is what reveals/
> hides the child scalar rows and applies/reverts the offsets.

### Wiring (`src/lib.rs`, `src/mods/mod.rs`)

- `pub mod timing_offsets;` in `src/mods/mod.rs`.
- `reg.register(Box::new(mods::timing_offsets::TimingOffsetsMod::new()), &ctx);` in `lib.rs`.

## Data Models

### Timing record (R1) — for reference; the mod reads/writes via the config map, not this

| Off | Key | Type | Stock default |
|---|---|---|---|
| `+0x00` | SOUND_OFFSET | i32 | 87 |
| `+0x04` | INPUT_OFFSET | i32 | 28 |
| `+0x08` | RENDER_OFFSET | i32 | 17 |
| `+0x0C` | BOMB_FRAME_OFFSET | i32 | 0 |
| `+0x10` | HIGH_PRECISION_INPUT | bool | on (out of scope) |

### Config-map int setter signature

`i64 set(const char* key /*RCX*/, i32 value /*EDX*/)` — FNV-1a hashes `key`, updates the
existing node's `+0x1c`; returns success/fail in RAX (we ignore it, just forward).

### Field semantics (R3 — drives hint text; all four binary-confirmed)

| Field | Role | Units | Hint (shipped) |
|---|---|---|---|
| Sound Offset | global audio sync | ms | "Global audio offset (ms). Higher = audio plays later." |
| Input Offset | input/judge ("SSQ") offset | ms | "Input/judge timing offset (ms)." |
| Render Offset | display latency comp | ms | "Display latency offset (ms). Higher = arrows drawn later." |
| Bomb Frame Offset | shock-arrow effect timing | frames | "Shock-arrow effect timing (frames, 60fps)." |

### Overlay scalar row constants

- Range `[-1000, 1000]`, `step_fine=1`, `step_coarse=20` (all four).
- Labels: `Sound Offset`, `Input Offset`, `Render Offset`, `Bomb Frame Offset`.
- Row keys: `sound_offset`, `input_offset`, `render_offset`, `bomb_frame_offset`.

## Error Handling

- **Setter signature/detour failure (load-bearing):** log a warning, self-disable the mod
  (no hook, no rows, no effect). Game and other mods unaffected. No panic. (Q8.)
- **Overlay infra failure (non-fatal):** if `mod_menu` row registration fails or the service
  is unavailable, log and continue — the mod still applies config-seeded values at boot and
  reverts on disable. The player loses the in-overlay tuning UI but not the feature. (Q8.)
- **FFI safety:** the setter detour wraps its body in `catch_unwind`; no
  `unwrap`/`expect`/panicking-index; the key pointer is null-checked before hashing/reading.
  It performs no allocation and forwards to the original in all paths.
- **Suppression detours:** each menu-button detour is `catch_unwind`-guarded and null-checks
  its out-params before zeroing; on any detour-install failure, log and leave that button
  un-suppressed (degraded, not fatal) — overlay still navigable.
- **Clamping:** all writes (config load, overlay adjust, live push) clamp to `[-1000,1000]`
  so an out-of-range config value or arithmetic can't push a wild offset.
- **Re-entrancy:** the live-push call from `on_change`/master-toggle calls the **original**
  setter (via the detour's `.call()`), not the hooked entry, so it won't recurse; and it runs
  off the game's publish path. Stock capture only records on the *first* observed write per
  key (the boot publish), so our own pushes don't overwrite captured stock.

## Testing Strategy

No unit harness; validation is deploy + observe (CLAUDE.md), diagnostic-build-first
(learnings):

1. **Setter-hook diagnostic:** log once per key on first hook hit: `{key, incoming_value,
   master_on, substituted_value}`. Confirms the hook fires at boot, captures stock, and
   substitutes. Verify on both 20260324-class and the cabinet 20260526 build.
2. **Boot-seed test:** set non-default values in `mod-config.json`, boot, enter a song →
   timing reflects configured values (e.g. obvious large SOUND_OFFSET audibly desyncs).
3. **Live-change test:** open overlay, master ON, adjust a scalar, enter a song → new value
   in effect; confirm it does **not** change mid-song (latch semantics, R2) — documented.
4. **Master-OFF revert:** toggle master OFF → next song uses stock (audio sync back to
   normal). Confirms stock capture + revert.
5. **Overlay UX:** child rows appear only when master ON; Up/Down navigate; Left/Right adjust
   (fine); Start-held Left/Right adjusts by 20 (coarse); values persist across reboot.
6. **Suppression:** with overlay open, mash Start/Up/Down/Left/Right → no movement/credit/
   effect in the game underneath (compare against today's numpad behavior). Diagnostic logs
   confirm `IS_INPUT_SUPPRESSED` active.
7. **Degradation:** simulate row-registration unavailable (or just verify the code path) →
   config-seeded offsets still apply.
8. **Cross-version:** smoke-test setter AOB resolves uniquely on both builds.
9. **Build gate:** `cargo check --target x86_64-pc-windows-msvc` after each change; full
   `./build.sh` before each deploy.

## Appendices

### A. Technology / approach choices

- **Hook the int setter vs. byte-patch the defaults:** chose the setter hook (R4). One
  detour, covers both publishers, no `.text`/`.rdata` patching, version-robust via AOB,
  matches project conventions (no hardcoded offsets). Default byte-patching would fix only
  preset 0 and miss the settings re-publisher.
- **Global overlay config vs. per-player game-UI option:** the values are process-global, so
  the per-player `custom_options` framework doesn't fit; the overlay is the correct global
  surface (maintainer principle). This necessitates the Part I overlay upgrade.
- **Mutex vs. atomics for state:** Mutex chosen for clarity — the setter hook is not a
  per-frame hot path (fires only on publish/our pushes), so lock cost is negligible; the
  CLAUDE.md "no Mutex across run_on_render_thread" rule doesn't apply (we don't schedule
  render work while holding it).
- **Latched-not-live accepted:** real-time mid-song change is out of scope (R2/Q6); "applies
  next song" is documented. Avoids reaching into live GamePlayActor fields.

### B. Research findings (see research/ for full detail)

- **R1:** record/publisher/builder/setter all re-confirmed on both builds; int setter has 8
  timing-only call sites; byte-identical setter AOB authored with a getter-distinguishing
  byte.
- **R2:** all four offsets latched into GamePlayActor at gameplay entry → changes apply next
  song, uniformly.
- **R3:** all four field semantics binary-confirmed (SOUND higher=later ms; disp = music +
  render − input; BOMB ×1000/60 = frames) → ship hints for all four.
- **R4:** apply lever = setter hook + key filter; boot publish = seed; stock = first observed
  write; OFF reverts to stock.
- **R5:** menu-button suppression is a direct mirror of the working get_10key detour;
  hold-state already exposed via `get_button_state`; scalar/parent-child rows modeled on
  `custom_options` shape.

### C. Alternative approaches considered

- **Byte-patch `.rdata`/builder defaults:** rejected (preset-0 only; misses re-publisher;
  hardcoded offsets).
- **Game's native Mods tab (per-player option):** rejected — values are global (Q2).
- **Forcing mid-song live effect** by writing live GamePlayActor fields: rejected, out of
  scope (Q6).
- **Exposing HIGH_PRECISION_INPUT:** out of scope (Q1).
- **Folding in the preset selector (Hack 6):** out of scope (Q10).

### D. Key constraints

- Setter detour is `extern "C"` on the game thread → panic-safe, no allocation, forwards
  always.
- Offset changes apply next song (latch), not mid-song — a documented limitation, not a bug.
- Two-tier degradation: offset-apply load-bearing; overlay UI optional.
- Overlay menu-button suppression must not starve the modpack's own poll (guarded by the
  existing `IN_MODPACK_POLL` re-entry flag).
