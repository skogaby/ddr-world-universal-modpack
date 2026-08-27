# Research — Current State of Custom-Options Persistence

Rust-layer only (no RE). All findings cite `file:line` against the working tree
as of 2026-05-31.

## 1. The two persistence mechanisms today

### Network persistence (generic, framework-owned)
`src/services/custom_options_persistence.rs` — installs two retour detours on
`ess.dll`'s playerdata sender/receiver:
- **Save** (`save_sender_trampoline`, L334): after the native option block,
  appends one `<mod_{id}>` s32 kbin child per registered option. Pulls the list
  via `custom_options::snapshot_for_save()` (L405), keyed by player side derived
  from `savedata+0x90` (L387–402).
- **Load** (`load_receiver_trampoline`, L433): after native parse, reads each
  `<mod_{id}>` back and calls `custom_options::resolve_from_load(id, side, value)`
  (L509).
- **Gate**: read at L78–81 — `config.custom_options.persist`, default `true`. When
  false, no detours installed; options reset to defaults each card swipe (L83–86).
- This path is **already fully generic** — it round-trips *every* registered
  option from *every* mod, not just WebUI. It is the template for what JSON
  persistence should become.

### JSON persistence (today: WebUI-mod-owned, NOT generic)
Lives entirely inside `src/mods/webui_options/mod.rs`:
- **Key**: `const CONFIG_KEY = "webui_options"` (L14).
- **Shape**: `webui_options: { p1: { <option_id>: <asset_id>, ... }, p2: { ... } }`
  — per-player objects mapping option_id → **asset_id** (the post-`save_transform`
  wire value, not the in-memory sequential index).
- **Write** (`save_to_json`, L305–327): called from `try_apply_all` (L293), which
  runs on every value change (`on_value_changed` → L249) and on scene 20 entry
  (L227–234). Reads the existing `webui_options` JSON, merges the current side's
  `{option_id: asset_id}` map in, writes back via `config::save_json_key`.
- **Read** (`load_from_json`, L329–348): called once at `enable()` (L140). Returns
  `HashMap<option_id, [p1_asset_id, p2_asset_id]>`. Used to compute each option's
  `default_value` for P1 (L171–184) and to prime P2 via `resolve_from_load`
  (L219–224).
- **Important nuance — transforms**: WebUI stores **asset_id** in JSON, not the
  raw in-memory index. It does this manually: `save_to_json` is fed asset_ids
  from `try_apply_all` (L280–289), and `load_from_json` results are converted
  back to indices via `asset_ids.iter().position(...)` at registration. This is
  the *same* asset_id↔index mapping the network path does via
  `persist_save_transform`/`persist_load_transform` (L36–70). So the JSON and
  network wire formats already agree (both store asset_id), but JSON does the
  conversion inline rather than reusing the registry's `save_transform`.

## 2. The custom_options registry — generic value model

`src/services/custom_options/registry.rs`:
- `RegisteredOption` (L24–39): `id: String`, `ui_kind`, `on_change`, `show_when`,
  `values: [i32; 2]` (index 0=P1, 1=P2), `persist: bool`, `save_transform`,
  `load_transform`.
- `FrameworkState.options: Vec<RegisteredOption>` (L55) — append-only, handles
  never shift.
- `get_value(id, side)` / `set_value(id, side, value)` (L111–134).
- Guarded by one `Mutex` (`STATE`, L137).

`src/services/custom_options/mod.rs` public API:
- `snapshot_for_save() -> Vec<(String, [i32;2])>` (L268–285): filters `persist`,
  applies `save_transform`, returns id + both sides' wire values. **This is
  exactly the generic enumeration JSON save needs.**
- `resolve_from_load(id, side, value)` (L180–199): applies `load_transform`, sets
  cache, fires change callback. **Exactly what JSON load needs.** Currently
  `pub(crate)`.
- `get_value` / `set_value` (public).
- `is_available()`.

### Key takeaway
The registry already supports everything a generic JSON persister needs:
- Enumerate all persistable options + per-side values → `snapshot_for_save()`.
- Restore a loaded value (with transform + callback) → `resolve_from_load()`.

So genericizing JSON persistence ≈ "do what the network path already does, but
read/write `mod-config.json` instead of kbin children."

## 3. Config plumbing

`src/mods/config.rs`:
- `ConfigFile` (L35–51): root struct. Relevant fields:
  - `custom_options: Option<CustomOptionsConfig>` (L46)
  - `webui_options: Option<serde_json::Value>` (L48) — untyped blob the WebUI mod
    parses by hand.
- `CustomOptionsConfig` (L15–19): currently just `persist: bool` (`#[serde(default
  = "default_true")]`, default true).
- Load: `init()` (L54–92) reads the file once into a `OnceCell`; parse failure or
  missing file → all-`None` defaults.
- Read access: `config::get() -> Option<&'static ConfigFile>` (L100).
- Write: two writers, both **read-modify-write the whole JSON object** preserving
  other keys:
  - `save_mod_states(states)` (L106) — only the `mods` key.
  - `save_json_key(key, value)` (L135) — arbitrary top-level key. WebUI uses this.

### Config is loaded once into an immutable OnceCell
`config::get()` returns a `&'static ConfigFile` — the in-memory parse is **never
mutated** after `init()`. Writers go straight to disk via read-modify-write of a
fresh `serde_json::Value`. So the in-memory `CustomOptionsConfig.persist` is
read-only at runtime; adding `persist_network`/`persist_json` is a struct change
plus a re-parse, nothing stateful.

## 4. Init ordering (`src/lib.rs`)

1. `mods::config::init()` (L77) — config available very early.
2. `custom_options::init()` (L199, step 4g).
3. `options_scroll::init()` (L208, 4h).
4. `custom_options_persistence::init()` (L217, 4i) — installs ess.dll detours.
5. `scene_manager::init()` (L228), `input_manager` (L235), `judge_hook` (L244).
6. **Mods registered** (L262–269) then **enabled** (`enable_with_config`, L275).

### Critical ordering fact
`custom_options_persistence::init()` runs at **step 4i, BEFORE mods are
registered/enabled (step 7–8)**. At 4i, **zero options are registered yet** —
WebUI registers its options in its `enable()` at step 8.

Implication for JSON load timing: the network path doesn't care (its load fires
later, on a card swipe / server round-trip, by which point options are
registered). But a **generic JSON load** that wants to prime values must run
*after* all mods have registered their options — i.e. it cannot happen inside
`custom_options_persistence::init()` at 4i. Today WebUI sidesteps this by doing
its JSON read inside its own `enable()` (after it registers). A genericized
loader needs an explicit "all mods enabled, now load JSON" hook point after step
8, or a lazy "load on first need" trigger. **This is the main new sequencing
question for design.**

## 5. What moves where (preliminary)

| Concern | Today | Target |
|---|---|---|
| JSON key name | `webui_options` | `custom_options` (same `{p1,p2}` shape) |
| JSON read/write code | `webui_options/mod.rs` | framework (likely `custom_options_persistence.rs` or a new submodule) |
| Options covered | WebUI categories only | all registered persistable options |
| Config gate | single `persist` (network only) | `persist_network` + `persist_json` |
| Wire value | asset_id (via inline transform) | asset_id (reuse registry `save_transform`/`load_transform`) |

## 6. Open questions surfaced by the code (for idea-honing)

1. **Load timing / precedence.** Network load fires on card swipe (late). JSON
   load currently fires at WebUi `enable()` (early). With both
   `persist_network` and `persist_json` enabled, which wins? Options:
   (a) JSON primes defaults at startup, network overwrites on swipe (network
   wins — matches "JSON is the offline fallback" framing); (b) JSON only loads
   when network is unavailable/disabled.
2. **JSON save trigger.** Network saves on the ess.dll save_sender (card-out).
   Today JSON saves on every value change + scene-20 entry. Keep that, or save
   JSON on the same trigger as network (and how do we detect "card out" generically)?
3. **Per-option opt-out.** `RegisterSpec.persist` currently gates network only.
   Does it also gate JSON, or do we want independent per-option control? (Likely:
   reuse the same `persist` flag for both — simplest, matches `snapshot_for_save`.)
4. **Migration.** Existing users have a `webui_options` block. On first run after
   the rename: silently ignore it (lose offline cache once), or one-time migrate
   `webui_options` → `custom_options`?
5. **Defaults** for `persist_network` / `persist_json` (both true? — preserves
   current behavior since `persist` defaulted true and WebUI JSON was always on).
6. **Transform reuse.** Should the generic JSON path reuse the registry's
   `save_transform`/`load_transform` (so it stores asset_id like network does),
   making WebUI's inline conversion go away? (Strongly implied yes.)
7. **WebUI's per-change/scene-20 apply.** WebUI writes the live game `Customize`
   struct on scene-20 entry independent of persistence. That game-write logic
   stays in WebUI; only the JSON read/write relocates. Confirm the split.
