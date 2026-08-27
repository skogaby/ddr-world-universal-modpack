# Idea Honing — Overlay Menu Rewrite

Decision register. Status: `Proposed` | `Accepted` | `Overridden` | `Assumed` | `Open`.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | Overlay label source for mirrored options | custom_options registrations carry NO display strings at runtime (labels are `seop_item_*` textures) | Add `display_name` (+ optional `description`, + per-`EnumValue` display string) to the registration API; overlay renders text. English-only. | Accepted |
| D2 | Placement parameter on registration | Controls which menu(s) a row appears in | New `menus` field on `RegisterSpec` (bitflag-ish: in_game / overlay), default BOTH; idempotent runtime setter like `set_option_available` | Accepted |
| D3 | Config schema for placement + order | Replaces `row_order`; hand-edited by operators | `custom_options.option_menu_settings`: ordered array of `{ "id": "...", "overlay": bool?, "in_game": bool? }`; omitted keys inherit the registration default; array order = display order for BOTH menus | Accepted |
| D4 | Overlay⇄in-game value sync mechanism | The registry's `on_change` is single-owner; overlay must reflect in-game edits live | Add a multicast `subscribe_value_changed(id, side, value)` observer API to the custom_options registry; overlay edits go through existing `set_value` (fires owner callback + observers) | Accepted |
| D5 | Fate of the existing overlay row API (`ScalarRowSpec`/`EnumRowSpec`) | Five mods register cabinet-wide rows through it | Keep it as the cabinet-wide registration path; its rows render on the GLOBAL SETTINGS tab. Internally unify onto one row model. **Addendum:** see D19 — the music_wheel_song_length X/Y rows are removed outright | Accepted |
| D6 | Tab set | Core information architecture | Four tabs, labeled **MODS** (enable/disable only), **GLOBAL SETTINGS** (cabinet-wide config), **PLAYER SETTINGS** (mirrored per-player options), **THEME** (theme picker + appearance options) | Accepted |
| D7 | PLAYER SETTINGS tab side selection | In-game options are inherently per-side; the overlay has one cursor | Pinned P1/P2 selector at the top of the tab; defaults to the side whose pinpad opened the menu; only entered sides selectable | Accepted |
| D8 | Session-live gating semantics | Per-player edits during attract could be clobbered by login | Gate per side on `stage_records::side_entered(side)==true` (fail-closed). No side entered ⇒ tab still selectable but rows greyed + "NO ACTIVE SESSION" banner. Exact predicate verified in research | Accepted |
| D9 | WebUI cosmetic rows in the overlay | Their UX depends on live preview boxes; a bare "3/54" number is useless without art | Mechanism default stays BOTH, but `webui_options` registers its cosmetic rows IN_GAME-only (operator can override via config) | Accepted |
| D10 | Theme model + persistence | Ship 3–4 built-in themes | Theme = Rust-defined `{ name, palette (panel/text/accent/greyed/cursor colors), background kind }`. New `overlay_menu` config section `{ theme, animate_background, opacity }`, DLL-written on change (fps_unlock pattern). **Opacity is a first-class appearance option**: default 80 %, adjustable on the THEME tab alongside the other appearance rows | Accepted |
| D11 | Animated background implementation | Shader path needs new RE; sprite path works today | Abstraction over background renderers. Front-load a shader RE spike (all-scene emission site + program extension of a resident container — judged moderate). Fallback/degrade path: sprite-animated backgrounds (scrolling arrows buildable today) + static gradient. Fail-open everywhere | Accepted |
| D12 | Animation disable + low-end degrade | Accessibility/perf | `ANIMATIONS` toggle on THEME tab (persisted in `overlay_menu`); OFF ⇒ static themed gradient. Shader-init failure degrades the same way | Accepted |
| D13 | Modal chrome rendering | Rounded corners, configurable opacity, edges | ImageWidgets + runtime-synthesized PNGs (9-slice rounded panel, tab bar, scrollbar) via the existing loose-PNG asset_loader path (training strip-HUD precedent). Panel opacity from `overlay_menu.opacity` (default 80 %); game visible around edges | Accepted |
| D14 | Layout density + description placement | Only 6–7 oversized rows render today | Single-line rows (~10–12 visible), name left / value right; selected row's description + key hints in a fixed footer; scrollbar + "N/M" position indicator | Accepted |
| D15 | Input mapping | Tabs need keys | Keep 8/2/4/6 + menu buttons + Start-coarse + triple-0 close; add 1/3 = prev/next tab | Accepted |
| D16 | Header (decorative) rows in the overlay | User wants headers replicated | Headers follow the same placement metadata; ordering from `option_menu_settings`. Overlay renders them as styled section separators | Accepted |
| D17 | `row_order` deprecation path | Existing operator configs | **Remove `row_order` support entirely** — delete the reading code (`ordering.rs` consumes `option_menu_settings` only); an existing `row_order` key is silently ignored like any unknown key. Maintainer will tell the small user base to replace their configs; sets the precedent for the public release | Overridden |
| D18 | Module structure | mod_menu.rs is 1170 lines already; rewrite is bigger | `src/mods/mod_menu/` subdirectory (state/render/tabs/input/theme/backgrounds) per AGENTS.md rule 7 | Accepted |
| D19 | Remove the music_wheel_song_length live-tuning rows | Values are settled; rows are dead weight in the new menu | Delete the `Length X/Y Offset` overlay rows and their `save_json_key` wiring from `src/mods/music_wheel_song_length.rs`; the mod keeps reading `music_wheel_song_length.offset_x/offset_y` from config (still operator-tunable by hand) | Assumed |
| D20 | Opacity row shape | Follows from D10 | THEME tab scalar row, 25–100 % in 5 % steps (never fully invisible), default 80, persisted to `overlay_menu.opacity` | Assumed |
| D21 | Shadertoy shader incorporation | Maintainer wants to port liked Shadertoy shaders as theme backgrounds; project is going open-source | Yes, with three hard constraints: (1) single-pass "Image" shaders only (no multi-pass/Buffer feedback — no render-to-texture path exists); (2) each shader is hand-ported GLSL→HLSL SM3 and must compile through fxc 9.29 within ps_3_0 budgets (the compile is the per-shader feasibility test; heavy raymarchers are out, plasma/tunnel/geometric-class fits); (3) license vetting per shader — Shadertoy defaults to CC BY-NC-SA 3.0; only redistribute permissively-licensed (CC0/MIT/attribution) shaders, with author/URL/license header retained in the ported source | Accepted |
| D22 | Theme extensibility model | Determines whether new shader backgrounds need a DLL recompile | Built-in themes compiled into the repo this pass, BUT the synthesis input is structured as a list of (name, compiled blob) so adding a ported shader = drop an `.hlsl` in `shaders/src/themes/`, run `build_shaders.sh`, add one table entry. Operator-side loading of arbitrary blobs from `data_mods/` deferred (security/support surface for a public release) | Accepted |

## Details & rationale

Register accepted by the maintainer 2026-08-24 with overrides: D6 tab labels fixed
(MODS / GLOBAL SETTINGS / PLAYER SETTINGS / THEME — dedicated THEME tab confirmed),
D10 gained the opacity appearance option, D17 overridden from "deprecated fallback" to
full removal, D5 gained the D19 addendum. D19/D20 recorded as assumptions (raised, not
objected to).

### D1 — Overlay label source
Rejected alternative: rendering the same `seop_item_*` label textures via ImageWidgets
(visual parity + ja/ko localization for free) — rejected because texture resolution
requires the owning IFS resident (not guaranteed in all scenes the overlay can open in),
and registry mod rows/hints are strings anyway, so the overlay needs a text path
regardless. Localization of overlay strings recorded as a non-goal this pass.
Sub-choice: strings supplied in Rust at registration (NOT codegen'd from
`scripts/option_strings.py`) — simpler; overlay is operator/power-user-facing.

### D3 — Config schema
Booleans over the `"OVERLAY"|"IN_GAME"|"BOTH"` enum: partial overrides are natural
(specify only the axis you're changing), "neither" is expressible (`both false`), and a
typo'd enum string needs error paths a missing bool key doesn't. Array order doubles as
display order for both menus (each menu filters to its subset) — one list, no per-menu
ordering complexity.

### D9 — WebUI cosmetics
Marked as a decision the user likely hasn't considered: replicating "ALL" in-game rows
includes ten cosmetic pickers whose values are meaningless without the preview art the
in-game modal provides. Recommendation keeps the blanket mechanism but has that one mod
opt out by default.

### D11 — Backgrounds
Shader spike scope (from research): (1) find a per-frame command-list emission site valid
in every scene with late/high z (layer-slot walker documented in
docs/custom_arrow_renderer_research.md §3); (2) bind custom programs outside gameplay by
extending a resident shader container (multi-program containers first-class; default
shader global already derived). CPU-side cost near zero vs per-frame sprite updates
(relevant to hot-path rule 4).

## Readiness

**Readiness Confirmed 2026-08-24.** Register fully accepted (D1–D18 accepted/overridden
2026-08-24 first pass; D21/D22 accepted second pass; D19/D20 stand as unobjected
assumptions). Research backing: `research/orientation.md` (current implementation +
custom_options constraints + rendering inventory), `research/widget-rendering.md`
(pool/z-order/PNG synthesis), `research/shader-spike.md` (spike scope + Shadertoy
portability). No decisions remain Open. Maintainer approved proceeding to design
("Approved, let's continue").
