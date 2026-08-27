# Requirements: 20260506-custom-options-support

## Overview

Add a client-side framework that lets any mod in the DDR World modpack inject custom player options into the native in-game options menu for both P1 and P2, with per-player state, registration-time defaults, change callbacks, optional dependent-row visibility, and optional persistence to the private eAmusement backend via the existing ess.dll save/load pipeline. Also add a hardcoded 6th "Mods" tab to both options menus so mods have a dedicated home for their options, and ship an autoplay-toggle proof-of-concept that exercises the framework end-to-end.

The full reverse-engineering substrate for this feature is already documented in `docs/custom_player_options_research.md` (class hierarchy, vtable decode, tab virtualization, AFP patch plan, save/load pipeline, minimum-viable override set).

## User Stories

### US-1: Mod authors can register custom options

**As a** mod author in the DDR World modpack
**I want** a framework API that lets my mod declare custom player options at init time (name, UI template kind, default value, allowed values/range, tab placement, change callback)
**So that** I can expose mod configuration through the native in-game options UI without each mod having to re-implement allocation, vtable management, reactive wiring, or save/load participation

**Acceptance Criteria:**

- [ ] The framework exposes a public Rust API (in a single well-named module under `src/services/` or similar — exact placement is a design decision) that mods call during their `init` or `enable` lifecycle to register options.
- [ ] Registration accepts at minimum: a unique `&'static str` option id (snake_case, must be a valid kbin element name), a UI-template-kind enum (`Enum` or `Scalar`), the option's `"PageN"` tag(s) (one or more of `"Page1".."Page6"`), a default value (s32), and a change-callback function pointer with signature `fn(player_side: u8, new_value: i32)`.
- [ ] For `Enum` options, registration additionally accepts an ordered list of allowed values, each with a display label texture name that points at a sprite in the game's `seop_op_*` value-ribbon atlas family (e.g. `"seop_op_on"`, `"seop_op_normal"`, `"seop_op_mirror"` — stock sprites Konami already ships; or mod-supplied `seop_op_<custom>.png` for bespoke enum values). A convenience `RegisterSpec::bool_toggle()` builder lets mods declare a simple on/off toggle without specifying textures — it auto-points at stock `seop_op_on` / `seop_op_off`.
- [ ] For `Scalar` options, registration additionally accepts `min: i32`, `max: i32`, `step: i32`, and a display-format hint (e.g. integer vs fixed-point rendering).
- [ ] Registering two options with the same id logs an ERROR and rejects the second registration; the first-registered option wins.
- [ ] The framework instantiates one on-screen row per registered option **per active player side** — mods never supply a `player_side` parameter at registration time.
- [ ] Options can be tagged with multiple `"PageN"` tags simultaneously, in which case the row appears under all tagged tabs (matches the native multi-key metadata-map capability documented in the research doc).
- [ ] Change callbacks fire under two conditions: (a) every time the player changes the value via the options UI, and (b) every time a value is resolved from a backend load response (including the initial boot load; a missing field in the load response resolves to the registered default).
- [ ] Every framework operation that can fail at runtime (address resolution, AFP patch, detour install, allocation) degrades gracefully: logs a `WARN` and continues; failures in one option do not prevent other options from being registered or rendered.

### US-2: Players see mod options in their native in-game options menu

**As a** player using a DDR World cabinet running this modpack
**I want** mod-supplied options to appear as first-class rows in my native in-game options menu, with independent P1 and P2 menus
**So that** I can configure mod behavior through the same interface I use for native options, without learning a separate mod-config mechanism

**Acceptance Criteria:**

- [ ] A registered option's row appears at the bottom of its tagged tab's visible row list (ordering: native rows first, mod rows appended in registration order).
- [ ] The option's row renders using the appropriate native AFP template (`option_item` for enum, `option_item_highspeed` or `option_scroll_speed_num` for scalar) — the row is visually indistinguishable from native rows of the same kind, aside from the mod-supplied label and value textures.
- [ ] Pressing left/right on a registered enum option cycles through the declared allowed values with the same input behavior as native enum rows (SFX on advance, error SFX at endpoint, correct `seop_image_*` texture swap).
- [ ] Pressing left/right on a registered scalar option increments/decrements by the declared `step` value, clamped to `[min, max]`.
- [ ] P1 and P2 see only their own options menus; changing an option on one side does not affect the other side's state (matches native options' per-player independence).
- [ ] All player-facing text (row label, value label for enum options) is driven by LayeredFS-served textures in `data_mods/` — no game-binary patching to add labels.

### US-3: Framework supports dependent-row visibility

**As a** mod author
**I want** a declarative way to mark a custom option as "visible only when another option equals a specific value"
**So that** I can build hierarchical option menus (like the native Speed Rate / Real Speed mutual-exclusion) without writing reactive-stream wiring code myself

**Acceptance Criteria:**

- [ ] Registration accepts an optional `show_when: Predicate` parameter. The initial `Predicate` grammar supports at minimum `Equals(parent_option_id, i32_value)`.
- [ ] When the parent option's value changes, the dependent row's visibility updates on the next frame (hides if the predicate becomes false, shows if it becomes true).
- [ ] The dependent-visibility mechanism works for mod options referencing either native options or other mod options as the parent.
- [ ] The dependent row is hidden (not just disabled) when its predicate is false, matching the native MULTIPLIER-vs-real-speed swap behavior.
- [ ] The framework is declarative-complete for v1: mods can also query option values via the framework's read API (so custom runtime logic can branch on option state), but they do NOT need to wire reactive subscribers themselves.
- [ ] The autoplay POC does NOT exercise dependent visibility; the feature is validated with a throwaway development-only option during implementation, removed before the feature ships.

### US-4: Options persist to the private eAmusement backend (client-side only)

**As a** player on a cabinet with a persistent eAmusement card
**I want** my mod-option choices to save to my profile and reload when I scan my card on any cabinet running the same modpack
**So that** mod options behave the same way as native options — scan in, tweak once, travel with my profile

**Acceptance Criteria:**

- [ ] The modpack installs a retour detour on `sys_playerdata_save_sender` at the address located via `ess.dll`'s dispatcher-table walk (primary) or AOB scan (fallback). On save, the detour invokes the original function, then appends one `<mod_{option_id}>` kbin `s32` child per registered option into the current player's `<option>` block.
- [ ] The modpack installs a retour detour on `sys_playerdata_load_receiver` at ess.dll `+0x25D70`. On load, after the original parses, the detour extracts any `<mod_{option_id}>` children from the `<option>` block and populates per-player state for each registered option.
- [ ] Field names on the wire use a `mod_` prefix (e.g. `<mod_autoplay>`) to namespace mod-owned state away from native options and avoid any risk of a future Konami option reusing the same name.
- [ ] Per-player scoping on the wire is IMPLICIT, not explicit: the framework emits ONE `<mod_{option_id}>` per save invocation, scoped to whichever PlayerdataEntry the sender is processing. There is no `_p1` / `_p2` suffix in the wire format.
- [ ] If the backend's load response omits a `<mod_{option_id}>` field (e.g. because the backend feature has not shipped yet, or because a new mod option was added), the framework populates that option with its registered default and fires the change callback with the default.
- [ ] Every `libavs-win64.dll` kbin primitive used (`Ordinal_162`, `Ordinal_163`, `Ordinal_175`, `Ordinal_176`) is resolved at init time via `GetProcAddress` with the numeric ordinal, NOT by name — ordinals are stable across Konami game versions whereas named exports can drift.
- [ ] The framework must not cause the game to reject its own native `<option>` block: the existing 29 native fields continue to save and load unchanged; injected `<mod_*>` children are strictly additive.

### US-5: A dedicated "Mods" tab is added to both players' options menus

**As a** player
**I want** a 6th tab labeled "Mods" in my options menu, distinct from the native Basic / Arrows / Lane / Judge / Assist tabs
**So that** mod-supplied options have a dedicated home when they don't logically fit under an existing tab, and I can find all mod options in one place regardless of which mod registered them

**Acceptance Criteria:**

- [ ] Both P1 and P2 options menus show 6 content tabs: Basic, Arrows, Lane, Judge, Assist, and Mods — in that left-to-right order — plus the Back tab on the far right.
- [ ] All 6 content tabs + the Back tab fit within the existing `tab_usr` 368 × 28 px container without resizing the parent OptionForm.
- [ ] The Mods tab uses the internal category key `"Page6"` (following the native `"Page1".."Page5"` convention).
- [ ] The Mods tab is rendered via a runtime AFP detour on `option_tab_list` using the existing `services::afp_patcher::register_patch` mechanism — NO cloned AFP files in `data_mods/`.
- [ ] Texture assets for the Mods tab are shipped via LayeredFS in `data_mods/<select_music_option_v3_ifs_mount>/tex/` with `texturelist.merged.xml` registration. Required assets and target dimensions:
  - `seop_tab_title_mods.png` — ~156 × 36 atlas px (displayed ~78 × 18)
  - `seop_tab_icon_mods.png` — ~88 × 56 atlas px (displayed ~44 × 28)
  - If individual tab backgrounds are per-tab: `seop_tab_mods_on.png` / `seop_tab_mods_off.png` / `seop_tab_mods_on_alt.png` at ~104 × 44 atlas px each (designer's choice; framework supports either shared or per-tab backgrounds).
- [ ] Placeholder versions of the above PNG assets are committed with the feature so the full tab renders before the final art lands. Placeholder style: simple solid color + readable text label; same dimensions as the final assets.
- [ ] Switching to the Mods tab (scrolling right past Assist) activates the tab-filter with `active_tab = 6`, the tab-filter handler detour iterates the flat row vector and shows only rows tagged `"Page6"`, and tab strip highlights update to show Mods as selected.
- [ ] The tab-filter handler re-implementation (a detour that replaces `FUN_180168d10`) is functionally equivalent to the native logic for tabs 1..5, with the addition of the `"mods"` entry at index 6 in the tab-names array.
- [ ] The tab-count constant at `OptionForm + 0x08` is patched from `5` → `6` so all tab-iteration loops respect the expanded count.

### US-6: Autoplay proof-of-concept demonstrates the framework

**As a** maintainer of this modpack
**I want** the existing autoplay mod to be converted to per-player operation and to expose its on/off state through the new framework, registered under both the Mods tab and the Assist tab
**So that** the feature ships with a working end-to-end example that exercises every critical framework capability in a real mod

**Acceptance Criteria:**

- [ ] `AutoplayMod`'s single global enable/disable flag is replaced with per-player state (P1 on/off and P2 on/off are independent).
- [ ] The `judge_hook` subscriber registration for autoplay consults the per-player state at judgment time and applies autoplay only for sides where the option is enabled.
- [ ] `AutoplayMod::enable()` (the mod-registry enable path, controlled by `mod-config.json`) gates whether the in-game autoplay rows appear AT ALL. With the mod disabled via config, no autoplay row appears in either tab on either player's menu.
- [ ] When the mod IS enabled via config, one "Autoplay: On/Off" row appears on both the Mods tab (`Page6`) AND the Assist tab (`Page5`) of each player's menu, demonstrating the multi-page tagging capability.
- [ ] Toggling the option from either tab updates that player's autoplay state; the row on the other tab reflects the new value immediately (both tags point to the same underlying state).
- [ ] Toggling P1's option does not affect P2's state, and vice versa.
- [ ] If backend persistence is working (post-backend-feature-deployment), the chosen autoplay state persists across card swipes; if backend persistence is not yet working (pre-backend-feature), the state resets to default (off) on card swipe but can still be toggled in-menu.
- [ ] The row's label is "Autoplay" (or similar; exact wording is asset-layer decision, not framework decision), with values "Off" (0) and "On" (1).
- [ ] Label and value textures are shipped in `data_mods/` as LayeredFS assets (placeholder art acceptable at ship time, with follow-on task to replace with final art).

### US-7: Options-tab viewports become scrollable when row count exceeds capacity

**As a** player viewing an options tab that now contains more rows than fit in the original fixed-height viewport (because mods have registered additional options into that tab)
**I want** the options panel to become scrollable on tabs where the row count exceeds the visible area, with the same scroll UX the series-filter screen already has
**So that** all registered options remain reachable even when mods add enough options to overflow a tab's native row budget

**Acceptance Criteria:**

- [ ] Every tab in the options menu (Page1..Page6 on both P1 and P2 menus) supports scrolling when the visible row count exceeds the tab's native viewport capacity.
- [ ] When the row count fits within the viewport, no scroll chrome renders and the panel behaves exactly as it does pre-feature (no visible regression for tabs that stay at or under capacity).
- [ ] When the row count exceeds viewport capacity, scroll chrome appears: up/down scroll indicator arrows (`tri_l_usr` / `tri_r_usr` equivalents, rotated for vertical where appropriate), a position-indicator track (`scroll_usr`), and a moving thumb (`move_usr`) whose position reflects the currently-focused row.
- [ ] Pressing up/down navigates row-by-row; when focus reaches the top/bottom edge of the visible area, the panel auto-scrolls to keep the focused row on screen (matches the series-filter scroll feel).
- [ ] Off-screen rows are hidden (not just translated outside the clipping region) so they don't render outside the tab's bounding box.
- [ ] Scroll state is per-tab (switching tabs and returning preserves that tab's prior scroll position within the same menu session) and per-player-side (P1 and P2 have independent scroll positions for the same tab).
- [ ] The implementation reuses the precedent set by `mods/series_expansion.rs` + `services::series_filter_scroll.rs`: an AFP runtime detour (via `services::afp_patcher::register_patch`) injects the scroll-chrome children (`scroll_usr`, `move_usr`, `tri_l_usr`, `tri_r_usr`) into the option panel template at load time, and a scroll driver (new or extended existing service) manages row visibility + position shifting on input events.
- [ ] If scroll-chrome injection fails (AFP patch error, resource not found, etc.), the tab falls back to non-scrolling display — rows beyond the viewport render but may be partially or fully clipped off-screen. Failure logs `WARN` and does not block other framework functionality.

## Out of Scope

- **Backend-side work** in the `bemani-buddy` repo: schema updates to `models/ddr_world/playdata_3.json`, codegen regeneration, DB migration for new `opt_mod_*` columns, `DdrWorldProfile` struct additions, MySQL DAO changes, and `handle_playerdata_{save,load}` handler updates. These are deferred to a separate `bemani-buddy` feature spec.
- **Per-option "migration from `mod-config.json`" logic**: no tooling to convert existing mod-config.json values into the new framework's state. New options default to their registered defaults on first card swipe.
- **Non-s32 value types** at the wire level: all mod-option values are stored and transmitted as `s32`. Mods needing richer types (strings, blobs, structured data) are out of scope for v1 and should use a separate persistence path.
- **Options UI theming** beyond placeholder-quality Mods-tab assets: no custom fonts, no new color schemes, no animation tweaks. Production-quality art for the Mods tab is a follow-on asset-creation task, not part of this feature.
- **A second example mod** beyond autoplay: only the autoplay POC is in scope. The throwaway dependent-visibility scratch option used during implementation is explicitly excluded from the shipped feature.
- **Free-form numeric input** (direct number entry via a keyboard/keypad): scalar options accept only left/right increment/decrement within `[min, max]`.
- **Option removal / re-registration at runtime**: registration is expected to happen during mod init; mods that dynamically add/remove options mid-session are out of scope.
- **Migration across modpack versions**: if a mod option is removed or its allowed-values change between modpack versions, the framework does not migrate stored values. Stale backend-persisted values are ignored or clamped via defaults.

## Open Questions

- **Where exactly (under `OptionForm` allocation) does the `max_tab_count = 5` value get written?** The research doc pinpoints the read site but leaves the write site as an implementation-phase finding (Cheat Engine data breakpoint at impl time). This is a known unknown, captured here so the design phase can either locate it proactively or plan to surface the answer during early implementation.
- **Does the framework need a `change_callback` variant that also receives the previous value?** The current proposal passes only `(player_side, new_value)`. If mods commonly need delta detection, we could extend to `(player_side, old_value, new_value)`. Flagging for the PE to consider during design.
- **Reactive-stream lambda lifecycle**: for scalar rows we don't yet have decoded lambda bodies equivalent to `FUN_18017c870`. If scalar row display updates work differently from enum (likely a separate text-rendering path), the framework may need a second set of mod-owned lambda subscribers. Research in the `Dependencies` section covers this; flagging it as an open question because the exact implementation surface depends on the RE finding.

## Dependencies

- **Pre-implementation reverse engineering (scalar-row support)** — currently missing from the research doc; must be completed before scalar-row framework code can be written. Specifically:
  - Identify `OptionHispeed::ctor` and `OptionRealSpeed::ctor` in gamemdx.dll (analogous to the 20 enum-kind ctors in `FUN_180163970`)
  - Decode the vtable shape of scalar rows — determine whether it shares the 4-vtable 19-slot layout or differs
  - Map the numeric-input path in the scalar row's slot-4 equivalent (`advanceValue`) — how +/- increments apply, bounds enforcement
  - Map the display-formatting path — how the current numeric value is rendered into the clip's text field (likely a different function than `FUN_18013b850`)
  - Determine field offsets for scalar-specific state (min/max/step/current-value) within the 0x330 OptionElement
  These findings must be captured back into `docs/custom_player_options_research.md`.
- **Tab-count storage site** — the location where `5` gets written to `OptionForm + 0x08`, so the framework can patch the immediate value from 5 → 6. Must be found via CE at impl time and documented.
- `services::afp_patcher` — existing service, reused for runtime AFP template modifications (Mods tab injection, option-panel scroll chrome injection).
- `services::avs_layeredfs` — existing service, reused for texture injection and `texturelist.merged.xml` merging.
- `services::series_filter_scroll` + `mods/series_expansion.rs` — direct architectural precedent for scrollable-panel injection. This feature either extends/generalizes that scroll driver into a reusable service or copies its pattern for the options panel.
- `mods/autoplay.rs` — existing mod that will be modified to become per-player-aware and register its toggle with the new framework.
- `docs/custom_player_options_research.md` — the authoritative RE document; supplementary findings from this feature's implementation work should be captured back into it.
- Backend server (`bemani-buddy`) will need a companion feature to accept, persist, and echo `<mod_*>` fields; client ships and functions independently (defaults apply when fields are absent).

## Assumptions

- The modpack's existing `services::afp_patcher`, `services::avs_layeredfs`, and `services::judge_hook` services are sufficient to support the new framework without architectural changes.
- The private eAmusement backend (bemani-buddy) does NOT reject kbin XML payloads containing unknown child elements under `<option>`. Validated expectation: serde's default deserialization behavior ignores unknown fields unless `#[serde(deny_unknown_fields)]` is explicitly set. Design-phase work should sanity-check bemani-buddy's serde posture and document the finding.
- All mod options in v1 are s32 on both the wire and in memory. This matches the native 29 option fields. Boolean options are represented as 0/1 s32.
- Registration order is stable across modpack runs because mods initialize in a deterministic order (per `src/lib.rs` init sequence). This gives users a predictable row ordering per tab.
- The LayeredFS mechanism can serve both new texture additions (for `seop_tab_*_mods`) and in-place replacements (if we choose to narrow standard tab sprites for uniform 52-px width, though that's a design-phase decision).
- All "test it end-to-end" validation is via deployment to a physical cabinet or emulated build running the modpack against a running `bemani-buddy` server. There is no unit test harness; validation is manual.
- `libavs-win64.dll` export ordinals 162, 163, 175, and 176 remain stable across the DDR World builds the modpack currently supports (20260324 and 20250805 per the reference pattern). If a future game update renumbers these ordinals, the framework will fail ordinal resolution at init time and degrade gracefully per US-1's error-handling criterion.
