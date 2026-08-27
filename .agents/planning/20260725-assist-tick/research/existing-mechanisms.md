# Existing Mechanisms — APIs the Assist Tick mod will need

**Date:** 2026-07-25
**Scope:** read-only survey of the current tree (no source modified). Every claim below is
either a direct quote/citation of shipped code, or explicitly marked as an inference.
**Feature:** `assist tick` — clap sound at each arrow's scheduled hit time, per-player
ON/OFF row on the in-game MODS options tab, following one side's chart.

### Confidence legend

- **[READ]** — read directly off the cited source line(s).
- **[INFERRED]** — deduced from code shape / comments, not proven by a single line.
- **[NEEDS RE]** — not answerable from this repo; requires binary reverse engineering.

---

# A. `custom_options` registration

Service entry points live in `src/services/custom_options/mod.rs`; the declarative types in
`api.rs`; the mutable per-player cache in `registry.rs`; the native row synthesis in `rows.rs`.
Service init happens at `src/lib.rs:245` (`custom_options::init(&signatures)`), **before**
mods are registered (`src/lib.rs:319`+) — so a mod's `enable()` can always register.

## A1. `RegisterSpec` — every field

`src/services/custom_options/api.rs:181-221`:

```rust
181: pub struct RegisterSpec {
187:     pub id: &'static str,
190:     pub ui_kind: UiKind,
193:     pub default_value: i32,
196:     pub on_change: OnChangeFn,
199:     pub show_when: ShowWhen,
204:     pub persist: PersistMode,
215:     pub save_transform: Option<fn(id: &str, value: i32) -> i32>,
220:     pub load_transform: Option<fn(id: &str, value: i32) -> i32>,
221: }
```

| Field | Type | What it does | Citation |
|---|---|---|---|
| `id` | `&'static str` | Stable identifier. Doubles as (a) the kbin wire element name `mod_<id>` on network persistence, (b) the row-label texture basename `seop_item_<id>`, (c) the `row_order` config key, (d) the `custom_options.p1/p2` JSON key. "Keep it snake_case and kbin-valid (letters, digits, underscore; must start with a letter)." | api.rs:183-187 |
| `ui_kind` | `UiKind` | `Enum` (left/right cycles a labeled list) or `Scalar` (numeric range with fine/coarse steps). | api.rs:190, 18-38 |
| `default_value` | `i32` | Primed into **both** players' caches at registration; `on_change` fires immediately for side 0 then side 1. | api.rs:193, mod.rs:203-204 |
| `on_change` | `fn(u8, i32)` | Change callback. See A5. | api.rs:171, 196 |
| `show_when` | `ShowWhen` | `Always` or `Equals { parent_id, value }`. See A4. | api.rs:199, 149-156 |
| `persist` | `PersistMode` | `Full` / `SaveOnly` / `None`. See A3. Builders default to `Full`. | api.rs:204, 112-123 |
| `save_transform` | `Option<fn(&str, i32) -> i32>` | in-memory → wire value (e.g. index → stable asset id). Identity when `None`. | api.rs:206-215 |
| `load_transform` | `Option<fn(&str, i32) -> i32>` | wire → in-memory. Must be the inverse of `save_transform`. | api.rs:217-220 |

Builders (all chainable): `bool_toggle` (api.rs:235), `enum_values` (api.rs:269),
`scalar` (api.rs:289); setters `default_value` (328), `on_change` (334), `show_when` (340),
`no_persist` (348), `persist_mode` (357), `persist_transform` (369), `save_transform` (383),
`step_coarse` (316).

Registration returns `Result<OptionHandle, RegisterError>`; all errors are recoverable
(`Duplicate`, `UnknownParent`, `NotInitialized` — api.rs:394-406).

### Copy-pasteable: 2-value OFF/ON row for assist tick

`RegisterSpec::bool_toggle(id)` **is** the 2-value OFF/ON enum row — it builds
`UiKind::Enum` with `{0 → "seop_op_off", 1 → "seop_op_on"}` and stock ribbon sprites
(api.rs:235-257). This is what every boolean mod option in the tree uses.

```rust
use crate::services::custom_options::{self, PersistMode, RegisterSpec};

const OPT_ASSIST_TICK: &str = "assist_tick";

fn on_assist_tick_change(side: u8, value: i32) {
    if let Some(a) = ASSIST_TICK_ON.get(side as usize) {
        a.store(value != 0, std::sync::atomic::Ordering::Release);
    }
}

fn register_rows() {
    let spec = RegisterSpec::bool_toggle(OPT_ASSIST_TICK) // 0 = OFF, 1 = ON
        .default_value(0)
        .on_change(on_assist_tick_change);
    // .persist_mode(PersistMode::Full) — already the builder default; shown for clarity
    match custom_options::register_option(spec) {
        Ok(_handle) => {}
        // Re-enable after a disable: rows stay registered (no unregister API).
        Err(custom_options::RegisterError::Duplicate { .. }) => {}
        Err(e) => log_warn!("assist-tick: option registration failed: {e}"),
    }
}
```

Explicit long form (identical result, if you want per-value previews with different keys):

```rust
use crate::services::custom_options::{EnumValue, RegisterSpec};

let spec = RegisterSpec::enum_values(
    "assist_tick",
    vec![
        EnumValue::with_preview(0, "seop_op_off", "off"), // seop_image_assist_tick_off
        EnumValue::with_preview(1, "seop_op_on",  "on"),  // seop_image_assist_tick_on
    ],
)
.default_value(0)
.on_change(on_assist_tick_change);
```

`EnumValue::new(value, label)` is the no-preview convenience (api.rs:64-70).

## A2. `UiKind` variants

`api.rs:18-38`:

```rust
18: pub enum UiKind {
21:     Enum { allowed_values: Vec<EnumValue> },
31:     Scalar { min: i32, max: i32, step_fine: i32, step_coarse: i32, format: ScalarFormat },
38: }
```

- **There is no `Bool` variant.** A boolean toggle in practice is `UiKind::Enum` with two
  values (api.rs:238-249). Confirmed by every consumer: `autoplay.rs:231`,
  `premium_free.rs:321`, `power_user_statistics/mod.rs:70,71,78`,
  `webui_options/profile_fields.rs` (`is_disp_weight`). **[READ]**
- `Scalar` renders through the game's native digit-sprite compositor and honors
  `Start`-held coarse steps; `ScalarFormat::{Integer, FixedPoint{decimals}}` (api.rs:89-95).
- Enum cycling **wraps** (`(cur_idx + 1) % n`, rows.rs:2054-2057); Scalar **clamps** and
  does not fire `on_change` at an endpoint (rows.rs:2080-2095).

## A3. `PersistMode` — and which one assist tick should use

`api.rs:112-123` with the authoritative matrix at api.rs:100-105:

| Mode | network save | network load | JSON cache (write + prime) |
|---|:-:|:-:|:-:|
| `Full` | yes | yes | yes |
| `SaveOnly` | yes | no | no |
| `None` | no | no | no |

**Recommendation: `PersistMode::Full`** — i.e. just use the builder default; write nothing.
**[READ + INFERRED]**

Evidence for the comparison the brief asked for:

- `autoplay`: `RegisterSpec::bool_toggle("autoplay").default_value(0).on_change(...)`
  — no `persist_mode` call, so it inherits `persist: PersistMode::Full` from the builder
  (`autoplay.rs:231-233` + `api.rs:253`). **[READ]**
- `premium_free`: same shape — `premium_free.rs:321-323`, no mode override → `Full`. **[READ]**
- `SaveOnly` exists only for options whose *loaded* state arrives through a game-native
  channel (the WebUI customize columns; api.rs:106-111, components.md:229). Assist tick has
  no such channel, so `SaveOnly` would make the value load-less.
- `Full` gets, for free: `mod_assist_tick` emitted on card-out, read back on card-in
  (`custom_options_persistence.rs`), and an offline `custom_options.p1/p2.assist_tick`
  cache in `mod-config.json` with a boot-time prime (~12 s after init).
  Load-side gate: `resolve_from_load` early-returns for non-`Full` (mod.rs:252-265);
  JSON gate: `json_persisted` (mod.rs:303-315); save snapshot: `snapshot_for_save`
  (mod.rs:389-406).

## A4. `ShowWhen` — conditional child rows

```rust
149: pub enum ShowWhen {
151:     Always,
155:     Equals { parent_id: String, value: i32 },
156: }
```
(api.rs:149-156)

Rules **[READ]**:

1. **The parent must already be registered** — validated synchronously in
   `FrameworkState::try_register` (registry.rs:141-148), rejected with
   `RegisterError::UnknownParent` (api.rs:403-405). Register parent first.
2. The predicate is evaluated **per side**: `state.options[parent_idx].values[side]`
   (rows.rs:292-294).
3. Hidden rows are excluded from the scroll list (`row_ptrs_for_side` filter,
   rows.rs:270-278) **and** explicitly suppressed by writing `row+0xB8 = 0`
   (`hide_show_when_excluded`, rows.rs:307-320).
4. Visibility updates **on the same frame** as the parent's value change
   (`update_children_visibility` → `options_scroll::reapply_mask_for_side`, rows.rs:2108-2125).

Working reference (a scalar child under a bool parent) —
`src/mods/power_user_statistics/mod.rs:72-77`:

```rust
72: RegisterSpec::scalar("pacemaker_threshold", 1, 50, 1, ScalarFormat::Integer)
73:     .default_value(10)
74:     .show_when(ShowWhen::Equals {
75:         parent_id: "pacemaker_to_mserror".into(),
76:         value: 1,
77:     }),
```

Note the spec array order at `mod.rs:69-79`: the parent `pacemaker_to_mserror` is listed
(and therefore registered) before its child.

## A5. `on_change` — signature, threads, what's legal inside

```rust
171: pub type OnChangeFn = fn(player_side: u8, new_value: i32);
```
(api.rs:171; doc block 158-170)

Fires on three events (api.rs:159-161): initial prime at registration, user advance in the
options menu, explicit programmatic set.

**Which thread — the doc comment is incomplete.** api.rs:166 says "The callback runs on the
game's render thread"; in practice there are four call sites **[READ]**:

| Call site | Thread |
|---|---|
| `register_option` initial prime (mod.rs:203-204) | the **registering mod's `enable()` thread** = the DLL init thread |
| user press → row slot-4 lambda (rows.rs:2106) | game render thread |
| network `load_receiver` → `resolve_from_load` (custom_options_persistence.rs:285) | the ess.dll save/load hook's thread (game thread at card-in) |
| JSON prime timer → `resolve_from_load` (custom_options_persistence.rs:300-301, 330) | a **spawned background thread** (`std::thread::spawn` + 12 s sleep) |

⇒ **an `on_change` body must be thread-agnostic.** The whole tree obeys this: every consumer
does nothing but store into an atomic (`autoplay.rs:58-71`, `playfield_styling/mod.rs:169-181`,
`player_perspective/mod.rs:65-72`). **[READ]**

What is legal inside:

- Atomic stores; cheap pure computation; logging (`log_debug!`).
- Cross-side mirroring via `custom_options::set_value` (mod.rs:221-228 documents this
  pattern; the "unchanged value" check terminates the recursion).
- **Illegal / discouraged:** re-entering `register_option` / `get_value` / other
  `custom_options` write paths (api.rs:164-170); blocking; panicking.
- **A panic is caught but costly:** the first panic logs at ERROR and then the option's
  callback is **permanently replaced with a no-op**, silently killing all future change
  notifications for that option (mod.rs:515-527). **[READ]**
- Touching game memory is *not* safe in general here, because of the background JSON-prime
  thread — `webui_options` uses `set_value_silent` precisely to avoid a game-memory write on
  a seed path (mod.rs:278-297). **[INFERRED from the two mechanisms]**

## A6. `get_value`, `set_value_silent`, and the `side` convention

```rust
213: pub fn get_value(player_side: u8, option_id: &str) -> Option<i32>
229: pub fn set_value(option_id: &str, player_side: u8, value: i32)          // fires on_change
287: pub fn set_value_silent(option_id: &str, player_side: u8, value: i32)   // does NOT fire
```
(mod.rs:213-219, 229-244, 287-297. Note the **argument order differs**: `get_value(side, id)`
vs `set_value*(id, side, value)`.)

- `get_value` returns `None` on unknown id, `side >= 2`, or a poisoned lock — it degrades
  rather than panicking, and is documented as safe for render-thread hot paths (mod.rs:209-212).
- `set_value_silent` is for read-only *seeding* from game state (mod.rs:278-286).

**Side convention [READ]:** `side: u8`, `0` = P1, `1` = P2 — `values[0]` = P1, `values[1]` = P2
(registry.rs:29-30); any `side >= 2` is rejected (registry.rs:175-177, 219-221);
api.rs:165 "`player_side` is `0` for P1 or `1` for P2".

Relation to `types::buttons::Player` (`src/types/buttons.rs:6-11`):

```rust
 6: #[derive(Clone, Copy, PartialEq, Eq, Debug)]
 7: #[repr(u8)]
 8: pub enum Player {
 9:     P1 = 0,
10:     P2 = 1,
11: }
```

The discriminants match the `custom_options` side indices, but **no conversion helper
exists and `custom_options` never mentions `Player`** — every call site passes a raw
literal `0`/`1` or `side as u8` (e.g. `autoplay.rs:256-257`, `playfield_styling/mod.rs:553`,
`csv_export.rs:17`). `Player` is only used by `input_manager` / the mod-menu overlay.
`Player as u8` is a safe cast if you want it. **[READ]**

## A7. Asset / label cost of one new option row

**Yes — one PNG must ship, and the atlas is regenerated.** Nothing else is required beyond
`register_option`; the framework auto-registers labels/previews/ribbons.

What `register_option` does behind the scenes (mod.rs:180-200):

```rust
183:     asset_gen::register_label_for(id);                      // seop_item_<id>
196:     asset_gen::register_preview_images(&preview_names);     // seop_image_<id>[_<key>]
199:     asset_gen::register_op_ribbons(&ribbon_names);          // seop_op_<key>, stock filtered
```

| Texture | Required? | Path (must be shipped by the mod) | Donor / size |
|---|---|---|---|
| `seop_item_assist_tick.png` (row label) | **Yes** — else a blank left column | `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/` (api.rs:176-180) | donor `seop_item_appearance` (asset_gen.rs:57); **176×16 RGBA** (verified: existing `seop_item_autoplay.png` is 176×16; `scripts/gen_option_labels.py:34`) |
| `seop_op_off` / `seop_op_on` (value ribbons) | **No** | — | stock, filtered out of injection by `STOCK_RIBBONS` (asset_gen.rs:80) |
| `seop_image_assist_tick_off/_on.png` (preview box) | **No** (optional) | same tex dir | donor `seop_image_scroll_speed` (asset_gen.rs:64); **368×172 RGBA** (verified against `seop_image_autoplay_on.png`) |

Preview-absence is handled gracefully: `preview_is_available` (asset_gen.rs:116-118) is
consulted in the slot-0 getter (rows.rs:1145-1149) and an unshipped name yields `""` →
the native binder **hides** the preview box rather than showing the previous row's art.
(`bool_toggle` always *requests* `_off`/`_on` preview keys — api.rs:239-248 — so shipping
them is purely opt-in.)

Label PNG generation is scripted: `scripts/gen_option_labels.py` — add
`("assist_tick", "ASSIST TICK")` to the `LABELS` list (lines 62-100; the list currently ends
with `("perspective", "PERSPECTIVE")` at line 99) and run it. Requires Pillow. Ribbon chips
(132×24, teal #00ffbd) are the `RIBBONS` list at line 126; not needed for an on/off row.

Atlas rebuild timing — **two gotchas [READ]**:

1. `flush_label_atlas()` is called **exactly once**, from `src/lib.rs:359`, after
   `enable_with_config` has run every mod's `enable()` (asset_gen.rs:226-255). A mod enabled
   *later* at runtime (mod-menu toggle) registers its label after the flush → **its row label
   texture is missing until the next launch.** No consumer re-flushes.
2. The flush is fingerprint-cached (`generate_cloned_atlases_cached`, asset_gen.rs:341-350).
   Adding one label changes the input hash → one slower boot, then cached.

Other conventions: **no length limit is enforced in code** — the label is a fixed 176×16
texture and the generator condenses over-long text horizontally
(`gen_option_labels.py:14-16`, `USABLE_WIDTH` at line 42). Id naming: snake_case,
kbin-valid, must start with a letter (api.rs:183-187).

## A8. Where a new option id must be listed

| Place | Required? | Citation |
|---|---|---|
| `scripts/gen_option_labels.py` `LABELS` | yes (to produce the label PNG) | gen_option_labels.py:62-100 |
| `data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/seop_item_<id>.png` | yes (committed artifact) | asset_gen.rs:296-307 |
| `README.md` "available ids" list under *Custom option row order* | yes (docs) | README.md:362-372 (toggles/scalars list) |
| `README.md` Complete Example `row_order` array | yes (docs; it ships the full built-in order) | README.md:120-140, 349-352 |
| `mod-config.json` `custom_options.row_order` (shipped cabinet config) | optional but the shipped file lists all 23 current ids | verified: `row_order` = `[premium_free, autoplay, timing_stats, …, customize_movie_size]` |
| `mod-config.json` `mods` map | optional — absent ⇒ enabled (`unwrap_or(true)`, mod_trait.rs:239) | mod_trait.rs:235-243 |
| `AGENTS.md` Key-Entry-Points table + `.agents/summary/components.md` | by repo convention | AGENTS.md:33-53 |

`row_order` itself is read once at `custom_options::init` (mod.rs:124-132) into
`ordering::set_configured_order`; unknown ids warn once and are ignored — so a new id is
never fatal anywhere. **[READ]**

---

# B. Two reference consumers

## B1. Idiomatic `Mod` impl that registers option rows — `src/mods/autoplay.rs` (whole file)

Structure **[READ]**:

- **Module-level per-side state** (hook callbacks are `fn` pointers and cannot capture):
  ```rust
  42: static AUTOPLAY_ENABLED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
  47: static ORIGINAL_FOOT_PANEL: [AtomicPtr<u8>; 2] = [ … ];   // one stash per side
  ```
- **`on_change` mirrors option → atomic** (autoplay.rs:58-71):
  ```rust
  58: fn autoplay_on_change(player_side: u8, new_value: i32) {
  59:     if player_side < 2 {
  60:         let enabled = new_value != 0;
  61:         AUTOPLAY_ENABLED[player_side as usize].store(enabled, Ordering::Release);
  64:         score_guard::set_autoplay_taint(player_side as usize, enabled);
  ```
- **`required_signatures()`** returns the AOB names the registry gates registration on
  (autoplay.rs:159-165) — a missing signature means the mod is never registered
  (mod_trait.rs:134-148).
- **`init(ctx)`** resolves addresses + one-time allocation, returns `false` to abort
  (autoplay.rs:167-198).
- **`enable()`** (autoplay.rs:200-247), in order:
  1. fail-closed gate: `if !score_guard::is_available() { … return; }` (206-211);
  2. `judge_hook::register_pre/post` → store handles (214-215), bail if either is `None` (217-224);
  3. `if custom_options::is_available()` → build spec → `register_option`, warn (not fail) on error (230-244).
- **`disable()`** (autoplay.rs:249-261): `judge_hook::unregister(h)` for each taken handle,
  then reset all per-side atomics.

**There is no `unregister_option` API.** Rows persist for the process; a re-enable gets
`RegisterError::Duplicate`, which consumers treat as success:
`playfield_styling/mod.rs:236-241`, `player_perspective/mod.rs:102-106`. Because
`register_option` does **not** re-fire `on_change` on the duplicate path, a re-enabling mod
must **reseed its atomics from the registry** — `playfield_styling/mod.rs:548-562`:

```rust
553: for side in 0u8..2 {
554:     on_scale_change(side, custom_options::get_value(side, OPT_SCALE).unwrap_or(DEFAULT_PCT));
558:     on_opacity_change(side, custom_options::get_value(side, OPT_OPACITY).unwrap_or(DEFAULT_PCT));
562: }
```

## B2. Latching per-side option values at gameplay entry — `playfield_styling`

The mod's own summary of the rule (`src/mods/playfield_styling/mod.rs:13-14`):

> "Purely visual — zero effect on timing, judging, or scoring. **Values latch at GAMEPLAY
> entry (one snapshot per side per song).**"

The latch state and its rationale (mod.rs:84-96):

```rust
84: // ── Per-song latch (requirement A4/R4) ──────────────────────────────
85: // One snapshot per side per song, taken at GAMEPLAY entry; the fill /
86: // guideline hooks and the cull float consume ONLY these for the whole
87: // song. Stored as f32 bit patterns in atomics so the hot-path reads are
88: // lock-free from any thread. Identity = 1.0/1.0.
89: static LATCHED_SCALE: [AtomicU32; 2] = [ AtomicU32::new(f32::to_bits(1.0)), … ];
93: static LATCHED_OPACITY: [AtomicU32; 2] = [ … ];
```

The latch itself (mod.rs:253-282):

```rust
253: fn on_scene_change(prev: i32, next: i32) {
254:     if next == scene::GAMEPLAY {
255:         fill_hook::clear_registry(false);
256:         lane_hook::reset();
257:         if !is_enabled() { return; }
260:         for side in 0u8..2 {
261:             let s = scale_pct(side).clamp(SCALE_MIN, SCALE_MAX) as f32 / 100.0;
262:             let o = opacity_pct(side).clamp(OPACITY_MIN, OPACITY_MAX) as f32 / 100.0;
263:             LATCHED_SCALE[side as usize].store(s.to_bits(), Ordering::Release);
264:             LATCHED_OPACITY[side as usize].store(o.to_bits(), Ordering::Release);
265:         }
266:         fill_hook::set_in_gameplay(true);
267:         cull_window::set_scale_contribution(latched_min_scale());
274:     } else if prev == scene::GAMEPLAY {
275:         fill_hook::set_in_gameplay(false);
278:         clear_latch();                        // back to identity
279:         cull_window::clear_scale_contribution();
281:     }
282: }
```

`scale_pct` / `opacity_pct` are the **authoritative read**: registry first, atomic mirror as
fallback (mod.rs:185-202):

```rust
185: fn scale_pct(side: u8) -> i32 {
186:     custom_options::get_value(side, OPT_SCALE).unwrap_or_else(|| {
187:         SCALE_PCT.get(side as usize).map(|a| a.load(Ordering::Acquire)).unwrap_or(DEFAULT_PCT)
191:     })
192: }
```

### WHY latch instead of reading live

Four reasons, all present in-tree:

1. **Repo convention: options apply next song.** Stated flatly in
   `player_perspective/mod.rs:45-48`: *"Written once per song at GAMEPLAY entry (scene
   callback), consumed by the pass_rewrite hot path for the whole song. **Values changed
   mid-song are deliberately not picked up (repo convention: options apply next song).**"*
   The game itself has no in-song option editor — `docs/mine_render_architecture.md:110`:
   *"The option is locked the moment a chart starts — the engine exposes no in-song option
   editor — so cache-once-per-chart is sufficient."* **[READ]**
2. **Hot-path cost & lock safety.** A live read is `Mutex::lock()` on the registry
   (mod.rs:217) — unacceptable per frame/per quad, and a lock on the render thread inside a
   detour. The latch is an `AtomicU32` read, "lock-free from any thread" (mod.rs:87-88). **[READ]**
3. **Consistency for the whole song.** Downstream state derived from the latch would
   otherwise desync — `playfield_styling` publishes `latched_min_scale()` into the shared
   `cull_window` at latch time (mod.rs:267); a mid-song change would leave the cull window
   inconsistent with the transform.
4. **Deterministic scoring/timing story.** For assist tick specifically this matters:
   a per-song latch means the audible behavior can't change mid-song, which keeps the
   feature auditable. **[INFERRED]**

**Assist-tick relevance:** the same latch shape gives you both required facts at song start —
*which side(s) enabled it* and (per C4) *which side's chart to follow* — and the brief's
"follow P1's chart" rule is a latch-time decision, not a per-frame one.

## B3. Scene wiring + gameplay reset — `power_user_statistics/mod.rs`

`enable()` (mod.rs:63-119) shows the plainest pattern: register the spec array
(69-88), enable sub-features (90-95), then one scene callback that does both edges
(97-116):

```rust
 97: if scene_manager::is_available() {
 98:     let id = scene_manager::on_scene_change(Box::new(|prev, next| {
 99:         if prev == scene::GAMEPLAY && next != scene::GAMEPLAY {
100:             csv_export::flush();
101:         }
103:         if next == scene::GAMEPLAY {
106:             let csv_p1 = custom_options::get_value(0, "step_data_export").unwrap_or(0) != 0;
107:             let csv_p2 = custom_options::get_value(1, "step_data_export").unwrap_or(0) != 0;
108:             data_feed::reset_buffers(csv_p1, csv_p2);
111:             calorie_feed::reset();
112:         }
113:         timing_stats_widget::on_scene_change(prev, next);
114:     }));
115:     self.scene_cb_id = Some(id);
116: }
```

`disable()` (mod.rs:121-128) removes the scene callback by id and calls each sub-feature's
`disable`. Note the gameplay-entry read of `get_value(side, id)` here is the *same* latch
idea in miniature (per-song booleans captured at entry).

Note also that PUS does **not** use `judge_hook` — it installs its own `GenericDetour` on
`judge_submit` (`data_feed.rs:146-177`), a different target, and its `enable`/`disable` do
not remove it (install is once, in `init`).

## B4. `judge_hook` subscribe / unsubscribe

`src/services/judge_hook.rs`:

```rust
61: pub type JudgeCallback = fn(actor: *mut u8, music_count: i32);
48: pub enum Priority { Early = 0, Normal = 1, Late = 2 }
228: pub fn register_pre(priority: Priority, callback: JudgeCallback) -> Option<CallbackHandle>
251: pub fn register_post(priority: Priority, callback: JudgeCallback) -> Option<CallbackHandle>
272: pub fn unregister(handle: CallbackHandle)
282: pub fn is_available() -> bool
217: pub fn foot_panel_offset() -> Option<usize>
```

- Exactly one detour on `judgeNotes` exists, installed at `src/lib.rs:290`; the dispatcher
  runs pre-callbacks (ascending priority) → original → post-callbacks (judge_hook.rs:112-148).
  Each callback is individually `catch_unwind`-wrapped (133-135, 144-146).
- `register_*` returns `None` when the service isn't installed; consumers must check
  (`autoplay.rs:217-224`).
- Subscribe in `enable`, `unregister` in `disable` — `autoplay.rs:214-215, 250-255`.
- Callbacks are plain `fn` (no captures) ⇒ all state in module statics (judge_hook.rs:14-17).

---

# C. Gameplay lifecycle + per-side chart facts

## C1. Detecting gameplay entry / exit

**Mechanism:** `scene_manager` hooks `TransitionSequence::createNextSequence`
(`src/services/scene_manager.rs:66-132`), decodes the 1-indexed scene id to 0-indexed
(line 67), and fires every registered callback with `(prev, next)` (114-118).

```rust
247: pub fn on_scene_change(callback: SceneChangeCallback) -> usize     // returns id
255: pub fn remove_callback(id: usize)
222: pub fn current_scene() -> i32
287: pub fn is_available() -> bool
 18: pub type SceneChangeCallback = Box<dyn Fn(i32, i32) + Send + Sync>;
```

Scene constants (`src/types/scenes.rs:48-60`) — the ones that matter here **[READ]**:

```rust
54:     pub const SONG_SELECT: i32 = 25;
55:     pub const SONG_TO_STAGE_INTERSTITIAL: i32 = 26;
56:     pub const STAGE_INDICATOR: i32 = 27;
57:     pub const GAMEPLAY: i32 = 28;
58:     pub const STAGE_RESULT: i32 = 29;
59:     pub const RESULTS_DETAIL: i32 = 30;
```

Idioms: `next == scene::GAMEPLAY` = entry; `prev == scene::GAMEPLAY` (playfield_styling) or
`prev == GAMEPLAY && next != GAMEPLAY` (PUS) = exit.

**Three gotchas [READ]:**

1. **Callbacks fire BEFORE the new scene is constructed.** `scene_hook` dispatches callbacks
   at scene_manager.rs:114-118 and only then calls the original at 127-131 (which creates the
   sequence). ⇒ at "GAMEPLAY entry" the `GamePlayActor` and its Results vector **do not exist
   yet**. Latching option values is fine; walking notes is not. Consumers that need live
   objects defer: `timing_stats_widget::on_scene_change` wraps its work in
   `widget_renderer::run_on_render_thread` (timing_stats_widget.rs:81-108), and
   `note_types_expansion` primes its per-chart cache on the **first pre-judge callback**
   instead (`note_types_expansion/mod.rs:288-294`, `mine_render.rs` "First pre-judge callback
   of the chart walks the chain").
2. **Gameplay entry can re-fire without a results screen.** Quick Restart installs a one-shot
   `STAGE_RESULT → GAMEPLAY` redirect (`scene_manager::add_redirect_once`, scene_manager.rs:270;
   components.md:259) — PUS's comment at mod.rs:104-105 calls this out: *"Entering gameplay —
   either fresh start or quick restart. Reset buffers."* Any per-song state must be reset on
   **entry**, not only on exit.
3. Redirects rewrite the reported scene, so `prev`/`next` are post-redirect values
   (scene_manager.rs:77-96).

## C2. Getting the `GamePlayActor` pointer — one per side, not shared

**Answer: one `GamePlayActor` instance per active play side; there is no single shared
actor.** They are siblings in the `DancePlaySequence`'s child list, and `judgeNotes` is
invoked per actor, so a judge callback is inherently per-side.

Evidence, strongest first:

1. **The tree literally enumerates a `Vec` of them** — `src/mods/quick_restart_or_fail.rs:263-295`:
   ```rust
   263: /// Walks the active TS → DPS → children chain and returns every child
   264: /// whose vtable matches `gameplay_actor_vtable`. …
   266: fn find_gameplay_actors() -> Vec<*mut u8> {
   284:     let mut child = *(dps.add(FIRST_CHILD_OFFSET) as *const *mut u8);
   285:     while !child.is_null() {
   286:         let vtable = *(child as *const *mut u8);
   287:         if vtable == target_vtable { out.push(child); }
   290:         child = *(child.add(NEXT_SIBLING_OFFSET) as *const *mut u8);
   ```
   and quick-fail iterates *all* of them, logging the count
   (quick_restart_or_fail.rs:318-335). **[READ]**
2. **Each actor carries exactly one play side.** `GamePlayActor + 0x84` is an `i32` play side:
   `autoplay.rs:52-56` (*"Offset of the play-side enum (0=left/P1, 1=right/P2) … In doubles
   mode the value is 0 (left side owns both pads)"*), read at `autoplay.rs:79`,
   `data_feed.rs:25,191`, `mine_render.rs:109,257`, `mines.rs:49`. A single shared actor
   could not carry one side value. **[READ]**
3. **Per-side scratch state is required for simultaneous play** —
   `autoplay.rs:45-50`: *"One stash per side so P1 and P2 can restore independently when both
   are in gameplay simultaneously (double-play or versus)."* **[READ]**
4. Corroborating RE note: `.agents/planning/20260610-suppress-score-submission/research/existing-hooks-and-triggers.md:92`
   — *"quick-fail ends the song for every active GamePlayActor (both sides)"* (stated as an
   assumption there).

**How a mod gets the pointer** (three routes, all in-tree):

| Route | Code | Notes |
|---|---|---|
| `judge_hook` pre/post callback arg (**preferred**) | judge_hook.rs:61 `fn(actor: *mut u8, music_count: i32)` | Per-frame, per-side, already dispatched. Side = `*(actor+0x84) as i32`. |
| Actor-tree walk from the captured `TransitionSequence` | quick_restart_or_fail.rs:266-295 + `scene_manager::current_transition_sequence()` (scene_manager.rs:238-245) | Needs the `gameplay_actor_vtable` signature; usable outside judge context. |
| Not available on render paths | `docs/mine_render_architecture.md:88` — *"The render hook receives the `ArrowRenderer` pointer, not the `GamePlayActor`"* | Renderer instances have **no side field**; `overlay_element_styling`/`playfield_styling` bind side by posX + presence instead (`.agents/planning/20260716-arrow-receptor-styling/research/existing-code.md:83-85`). |

**Caveat for a 2P-aware design [READ]:** `note_types_expansion` keeps a *single* global
`prev_music_count` in its judge tick (`mines.rs:95-101, 280-284`) — i.e. it implicitly assumes
one actor and would interleave in versus. Do not copy that pattern; key everything by
`side_idx`, as `autoplay` does.

## C3. Units of `music_count`

**Answer: milliseconds.** Both the `i32` the judge hook receives and the note record's
`music_count` at `+0x08` are integer milliseconds on DDR World. **[READ, multiple
independent sources]**

Evidence:

1. `.agents/planning/20260523-bulk-hack-porting/research/per-step-data-feed.md:97-103`:
   > "The `music_count` is the engine's per-frame integer time. **In DDR World this counter is
   > in milliseconds** — confirmed by the shock-arrow miss window check in `judgeNotes`:
   > `note.musicCount + 0xa0 <= playhead` (`0xa0 = 160` ms is a sensible shock-arrow miss
   > window). The cave's running-stats accumulator … multiplies by 10 for the sum buckets and
   > stores the ms-error directly as a signed byte (range ±127 ms)."
2. Same doc, Gotcha #1 (line 579-585): *"**`musicCount` is in milliseconds in DDR World**
   (not 1/60 ticks as in older Bemani titles) … No conversion needed when … computing
   `result.judgeTimestamp - note.music_count`."*
3. Shipped code treats `note+0x08` as ms — `power_user_statistics/data_feed.rs:216-228`:
   ```rust
   217: let note_ptr = *(result as *const *const u8);
   219: *(note_ptr.add(0x08) as *const i32)          // → `expected_ms`
   223: let actual_ms = expected_ms + ms_error;
   ```
   and the CSV header is literally `Expected,Actual,Delta (Ms Error)` (csv_export.rs:131).
   Adding a millisecond error to a tick count would be meaningless. **[READ]**
4. The engine's own judge windows read as ms: `docs/autoplay.md:161-162` — `+0xA0` (160) miss
   window, `-0x104` (260) early cutoff.
5. The offsets research confirms the surrounding arithmetic is ms:
   `.agents/planning/20260626-timing-offsets/research/r3-field-semantics.md:41-46` —
   `dispMusicCount = musicCount + RENDER_OFFSET − INPUT_OFFSET` (all ms; `BOMB_FRAME_OFFSET`
   is the only frame-unit field, converted via `(1000*offset)/60`).

**⚠ Latent inconsistency worth flagging (assist tick must not inherit it).**
`note_types_expansion/timing.rs::beat_to_music_count` interpolates over the SSQ tempo chunk's
**raw `tempo_data[]`, i.e. seconds-ticks at the file's TPS**, with no rescale
(timing.rs:68-80, 146-155). But the engine normalizes at chunk-load time —
`docs/ssq_format.md:172-179`:

```
normalized[i] = round(tempo_data[i] × 1000 / TPS + 0.5f)
… "This converts to a TPS-invariant millisecond-scale representation, which downstream code uses."
```

TPS is **not fixed**: `150` (760 files) or `1000` (763 files), roughly 50/50
(ssq_format.md:14, 879). So the mod's converter agrees with the engine only for TPS=1000
charts; on TPS=150 charts it would be off by a factor of ~6.67. **[INFERRED from reading both
— not verified on cabinet, and mines may simply never have been tested on a TPS=150 chart.]**
Practical consequence for assist tick: **do not derive tick times from the SSQ tempo chunk.**
Read `note.music_count` (`+0x08`) out of the game's own note records, which are already in
engine ms.

## C4. Reading the per-side difficulty / chart identity

Two independent, already-implemented pointer chains:

### (a) DancePlaySequence route — what `csv_export` actually ships **[READ]**

`src/mods/power_user_statistics/csv_export.rs:47-78`:

```rust
59:     // The actor's parent (at +0x08) is the DancePlaySequence.
60:     let dps = *(actor.add(0x08) as *const *const u8);
65:     // Basename std::string at DPS+0xA0 (standard MSVC layout).
66:     let string_base = dps.add(0xA0);
67:     let basename = read_msvc_string(string_base);
69:     // Difficulty index at DPS+0x50 (u8: 0=beg, 1=bas, 2=dif, 3=exp, 4=cha).
70:     let difficulty = *(dps.add(0x50) as *const u8) as i32;
```

MSVC `std::string` decode helper at csv_export.rs:83-99 (size at `+0x10`, capacity at `+0x18`,
heap pointer at `+0x00` when capacity ≥ 16). Difficulty names at csv_export.rs:101-110.
Because this is reached *through the per-side actor*, the values it yields are that side's.

### (b) Session/match-struct route — constants declared but currently unused **[READ]**

`src/mods/power_user_statistics/data_feed.rs:25-39`:

```rust
25: const ACTOR_PLAY_SIDE_OFFSET: usize = 0x84;
27: const ACTOR_SESSION_OFFSET: usize = 0x88;          // session/match struct ptr
29: const SESSION_SONGCODE_OFFSET: usize = 0x98;       // MSVC std::string body
31: const SESSION_SONGCODE_CAP_OFFSET: usize = 0xB8;
33: const SESSION_SONGCODE_SIZE_OFFSET: usize = 0xB0;
35: const SESSION_CHART_INFO_P0_OFFSET: usize = 0x118; // chart_info ptr, player 0
37: const SESSION_CHART_INFO_P1_OFFSET: usize = 0x120; // chart_info ptr, player 1
39: const CHART_INFO_DIFFICULTY_OFFSET: usize = 0x04;
```

These are dead constants in the shipped file (the `#![allow(dead_code)]` crate rule) but the
chain is documented and RE-verified in
`.agents/planning/20260523-bulk-hack-porting/research/per-step-data-feed.md:389-410`:
`session = *(actor+0x88)`; songcode at `session+0x98` (size `+0xB0`); **`chart_info` for
player 0 at `session+0x118` and player 1 at `session+0x120`, difficulty at `chart_info+0x04`.**
This is the route that gives **both** sides' difficulties from **one** actor — exactly what
the brief's "if the two players are on different difficulties" rule needs to detect.
**[READ, with the caveat that only route (a) is exercised on cabinet today.]**

### (c) Per-side player options / profile — `player_work_table` **[READ]**

Table derived from the `player_work_table_anchor` signature
(`core/signatures.rs:541-543, 2157-2195`). Two consumers:

- From an actor: `mine_render.rs:249-296` —
  `actor+0x84` → `player_work_table[side*8]` → `*wrapper` → `PlayerWork+0xE0` (Option) →
  `+0x60` (arrow shape). Offsets at mine_render.rs:109-112; chain also documented in
  `docs/mine_render_architecture.md:88-100`.
- Without an actor (song-select time): `webui_options/mod.rs:394-434` —
  `player_work_table[side]` → `*wrapper` → `PlayerWork + customize_offset`, with a null
  wrapper meaning "side not carded in" (mod.rs:406-409). Useful as a **presence test** for
  "is side N actually playing".

## C5. Are note timestamps already parsed per side? — No (except mines)

**Answer: there is no general, already-parsed per-side note-timestamp list. A new mod must
walk the game's Results vector itself (or maintain its own sidecar).** **[READ]**

What exists:

- **Mines only:** `note_types_expansion` builds a mod-owned sidecar of mine entries sorted by
  `music_count` (`mines.rs:79-101`, filled at chart load, sorted at mines.rs:227-231). It
  covers *only* `kind::MINE` notes and is private to that mod.
- **Generic walk helpers (reusable):** `src/mods/note_types_expansion/game_note.rs`
  ```rust
  185: pub unsafe fn actor_results_range(actor: *mut u8) -> (*mut u8, *mut u8)
  189:     let begin = *(actor.add(result::ACTOR_OFFSET_RESULTS_BEGIN) as *const *mut u8);  // 0xB0
  190:     let end   = *(actor.add(result::ACTOR_OFFSET_RESULTS_END)   as *const *mut u8);  // 0xB8
  154: pub unsafe fn for_each_result(begin, end, mut callback: impl FnMut(*mut u8, *mut GameNote))
  163:     if !span.is_multiple_of(result::STRIDE) { return; }   // stride 0x40; validates alignment
  ```
  Result-entry offsets: note ptr `+0x00`, judge timestamp `+0x08` (`-1` = unjudged), grade
  `+0x0C` (`0xFF` = unjudged), visible `+0x10` (game_note.rs:97-144).
  Note record (stride `0x60`): `kind` i8 `+0x00`, `beat_count` i32 `+0x04`,
  **`music_count` i32 `+0x08`**, per-panel `state[8]` `+0x1C`, `length[8]` `+0x3C`
  (game_note.rs:32-43, with a compile-time size assert at 45-49). Panel bit convention at
  game_note.rs:53-62; `kind::{ARROW=0, THINOUT=1, FREEZE_TAIL=2, MINE=20}` at 82-90.
- **Per-frame walk precedent:** `mine_render.rs:396-444` reads the Results vector from the
  renderer's own reference (`+0xB8`) and iterates every entry each frame with `for_each_result`
  (and a cull-window early-out). `registry.rs:94-95` does the same from the pre-judge callback.
- `autoplay.rs:28-29` shows the actor's own note-list view: `NOTE_LIST_PTR = 0x0B0`,
  `NOTE_COUNT = 0x168` (a separate count field), passed to `AutoFootPanel::update`.

**Practical shape for assist tick [INFERRED]:** one walk of `actor_results_range` +
`for_each_result` on the **first judge tick of the song** (per C1's "actors don't exist at
scene entry"), filtering `kind` to `{ARROW, THINOUT, FREEZE_TAIL}` and collecting
`note.music_count`, then advancing a cursor against the judge hook's `music_count` — which is
exactly the `(prev_music_count, music_count]` crossing test mines already implements
(`mines.rs:280-291`, `partition_point` on a sorted vec).

**⚠ Hook-conflict note [READ]:** the chart-load parse point (`step_reader_analyze`) is
**already detoured** by `note_types_expansion` (`hooks.rs:1-13, 45-54`), and the repo rule is
one detour per target (AGENTS.md:87). An assist-tick mod must not add a second detour there.

---

# D. Config plumbing

## D1. Adding a typed config section to `src/mods/config.rs`

The file is a single `ConfigFile` struct + `OnceCell`, with `save_json_key` for writes.
Closest templates: `FpsUnlockConfig` (config.rs:106-137) and `PlayerPerspectiveConfig`
(config.rs:143-175). Checklist **[READ]**:

1. **Define the struct** with `#[derive(Deserialize, Clone, Debug)]` and a `#[serde(default = "…")]`
   on every field (so a partial section still parses):
   ```rust
   // pattern copied from config.rs:146-175
   #[derive(Deserialize, Clone, Debug)]
   pub struct AssistTickConfig {
       #[serde(default = "default_assist_tick_offset_ms")]
       pub offset_ms: i32,
   }
   impl Default for AssistTickConfig {
       fn default() -> Self { Self { offset_ms: default_assist_tick_offset_ms() } }
   }
   fn default_assist_tick_offset_ms() -> i32 { 0 }
   ```
   Convention notes: free `default_*()` fns, not `#[serde(default)]` + `Default` on primitives,
   when a non-zero default is needed (config.rs:96-104, 130-137, 170-175); `#[serde(default)]`
   with `Option<T>` when "absent" is meaningful (config.rs:28-29, 36-37, 43-44, 54-55);
   `default_true()` helper at config.rs:139-141.
2. **Add the field to `ConfigFile`** (config.rs:199-221):
   ```rust
   #[serde(default)]
   pub assist_tick: Option<AssistTickConfig>,
   ```
3. **⚠ Update BOTH hand-written `ConfigFile { … }` literals in `init()`** — the parse-failure
   fallback (config.rs:237-248) and the file-missing fallback (config.rs:253-265). Both list
   every field explicitly, so omitting the new one is a compile error. (This is the one
   non-obvious step.)
4. **Accessor** — the idiom is a small private fn in the mod, not in `config.rs`
   (`player_perspective/mod.rs:181-185`):
   ```rust
   fn active_config() -> AssistTickConfig {
       config::get().and_then(|c| c.assist_tick.clone()).unwrap_or_default()
   }
   ```
   `config::get()` returns `Option<&'static ConfigFile>` (config.rs:276-278); availability
   probe `config::is_available()` (271-273). Config is loaded very early — `src/lib.rs:83`
   comment: "Must be loaded BEFORE the early_apply" phase.
5. **Writing back (only if the mod mutates it)** — `config::save_json_key(key, value)`
   (config.rs:489-507) is a read-modify-write that preserves all other keys. Reference use:
   `timing_offsets.rs:398-409` (`persist_all` builds a `serde_json::Map` and writes the whole
   section on each change). Operator-only sections are never written by the DLL — see the
   explicit comments at config.rs:52-53 (`row_order`) and config.rs:144-145
   (`player_perspective`).
6. **Docs to update:** `README.md` — add a subsection alongside e.g. *Custom option row order*
   (README.md:337-352) and extend the **Complete Example** JSON block (README.md:81-140);
   the shipped `mod-config.json` (top-level keys today: `mods`, `diagnostics`, `layeredfs`,
   `series_expansion`, `folder_expansion`, `custom_options`, `timing_offsets`, `fps_unlock`);
   plus `AGENTS.md`'s Config section (AGENTS.md:90-135) by repo convention.

## D2. The overlay alternative — `mod_menu::register_scalar_row`

`src/mods/mod_menu.rs`:

```rust
178: pub struct ScalarRowSpec {
180:     pub key: String,                       // stable row id (distinct from any mod id)
181:     pub label: String,
182:     pub hint: String,                      // secondary line under the label
187:     pub parent_row_key: Option<String>,    // gate: shown only while that row == 1
188:     pub min: i32,
189:     pub max: i32,
190:     pub step_fine: i32,                    // Left/Right
191:     pub step_coarse: i32,                  // Start-held Left/Right
192:     pub initial: i32,
193:     pub on_change: RowChangeCallback,      // = Arc<dyn Fn(i32) + Send + Sync>   (line 54)
194: }
224: pub fn register_scalar_row(spec: ScalarRowSpec)   // idempotent by key (243-247)
255: pub fn register_enum_row(spec: EnumRowSpec)       // labeled pick-list variant (201-218)
282: pub fn remove_rows_for(keys: &[&str])            // call from disable()
```

Semantics **[READ]**: `indent: 1` and `visible_when: parent_row_key.map(|p| (p, 1))` are set
for you (mod_menu.rs:236-237) — the row appears as a child, visible only while the parent
boolean row is ON; the parent is usually the mod's own registry id. `RowChangeCallback` is an
`Arc<dyn Fn(i32)>`, so unlike `custom_options::OnChangeFn` it **can capture** (see the
`move |v| set_offset(i, v)` closure at timing_offsets.rs:472). Overlay rows are
**cabinet-wide, not per player**, and adjustments arrive on the input/render thread.

Canonical consumer — `src/mods/timing_offsets.rs:463-493`:

```rust
473: mod_menu::register_scalar_row(ScalarRowSpec {
474:     key: field.row_key.to_string(),
475:     label: field.label.to_string(),
476:     hint: field.hint.to_string(),
477:     parent_row_key: Some(MOD_ID.to_string()),
478:     min: VALUE_MIN, max: VALUE_MAX,
480:     step_fine: STEP_FINE,      // 1
481:     step_coarse: STEP_COARSE,  // 20
482:     initial: configured[i],
483:     on_change: cb,
484: });
…
490: fn remove_overlay_rows() {
491:     let row_keys: [&str; FIELD_COUNT] = std::array::from_fn(|i| FIELDS[i].row_key);
492:     crate::mods::mod_menu::remove_rows_for(&row_keys);
493: }
```

Minor doc drift found: **`mod_menu::set_scalar_value` does not exist**
(`.agents/summary/components.md:278` mentions it in the `register_scalar_row`/
`set_scalar_value`/`remove_rows_for` trio; no such symbol anywhere in `src/`). Rows are
seeded via `ScalarRowSpec::initial` and re-registered by key to change the seed.

## D3. Which route for the latency offset?

| | `mod-config.json` section (D1) | `mod_menu::register_scalar_row` (D2) | `custom_options` scalar row (A1/A2) |
|---|---|---|---|
| Scope | cabinet-wide | cabinet-wide | **per player** |
| Adjust in-game | no (file edit + relaunch, unless the mod also pushes live) | yes (numpad-0 overlay, Left/Right, Start-held coarse) | yes (native MODS tab) |
| Persistence | the file itself | none unless the mod calls `save_json_key` | free via `PersistMode::Full` (server + JSON) |
| Asset cost | none | none (text widgets) | needs `seop_item_<id>.png` (A7) |
| Precedent | `player_perspective` (`hallway_focal`) | `timing_offsets` (4 rows) | `pacemaker_threshold`, `arrow_scale` |

`timing_offsets` is the direct precedent for "a cabinet-wide ms knob": it is deliberately
**not** on the per-player Options screen because the underlying values are process-wide
(components.md:275). The repo's established combination for that case is **both** D1 and D2 —
config file as the source of truth, overlay scalar rows for live adjustment, `save_json_key`
on change (timing_offsets.rs:398-409).

---

# E. Cross-cutting gotchas relevant to the design

1. **No audio API exists in this codebase.** No signature in `src/core/signatures.rs`
   (71 entries; full list checked) targets a sound/SE play function, and no service wraps
   audio. Playing `clap.ogg` needs either a new RE target (the game's own SE player) or an
   external audio path. **[READ / NEEDS RE]**
2. **Judge hook `music_count` is the *input/judge* reference, not the display or audio one.**
   `dispMusicCount = musicCount + RENDER_OFFSET − INPUT_OFFSET` is stored at
   `GamePlayActor+0x17C`, and `SOUND_OFFSET` shifts the music-count baseline
   (`.agents/planning/20260626-timing-offsets/research/r3-field-semantics.md:28-52`;
   the latched fields are `+0x16C` sound, `+0x170` input, `+0x184` render, `+0x188` bomb).
   Whichever clock assist tick keys off, the cabinet's own offsets are already in play —
   which is a strong argument for the mod's own `offset_ms` knob. **[READ]**
3. **Scene-entry ordering** (C1 gotcha 1): the actor doesn't exist when the GAMEPLAY scene
   callback fires. Latch options there; collect notes on the first judge tick.
4. **`custom_options` has no unregister**; treat `Duplicate` as success and reseed atomics on
   re-enable (B1).
5. **The label atlas flushes once per boot** (A7 gotcha 1) — a mod toggled ON at runtime has
   no row label until the next launch.
6. **Never install a second detour on an already-hooked target** (AGENTS.md:87): `judgeNotes`
   (judge_hook), `judge_submit` (power_user_statistics/data_feed), `step_reader_analyze`
   (note_types_expansion), `render_notes` (render_notes_hook), `render_sprite_final`
   (playfield_styling) are all taken.
7. **Don't reuse `note_types_expansion::timing::TempoConverter`** for tick times — its output
   is raw seconds-ticks at the file's TPS, not engine ms (C3). **[INFERRED]**
8. `PersistMode::Full` values land in `mod-config.json` under `custom_options.p1/p2` keyed by
   option id; a new id therefore shows up in the shipped config automatically after the first
   card-out (config.rs:335-372).
