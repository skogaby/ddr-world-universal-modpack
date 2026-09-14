# Option Framework Facts (Step 2 orientation sub-report)

How a per-player bool parent + conditional scalar child is added to the in-game 9-options
menu and the 0-0-0 PLAYER SETTINGS tab. All in `src/services/custom_options/`.

## Registration API

| Item | Location |
|---|---|
| `RegisterSpec` | `api.rs:353-405` |
| `RegisterSpec::bool_toggle(id)` — `UiKind::Enum` OFF/ON with `seop_op_off/on` ribbons, default 0, `PersistMode::Full`, `ShowWhen::Always` | `api.rs:419-444` |
| `RegisterSpec::scalar(id, min, max, step_fine, format)` | `api.rs:479-505` |
| Builders `.step_coarse .default_value .on_change .show_when .no_persist .persist_mode .persist_transform .save_transform .menus .in_game_only .overlay_only .display_name .description` | `api.rs:536-648` |
| `OnChangeFn = fn(player_side: u8, new_value: i32)` — plain fn ptr, render thread, must not panic | `api.rs:342`, `:329-341` |
| `ScalarFormat` (`Integer`, `FixedPoint`, `OffsetInteger`, `SignedUnit`, `Unit`, `MinutesSeconds`, `PrefixedIndex`) | `api.rs:156-201` |
| `register_option(spec)` — primes both sides to default, registers `seop_item_<id>` label + previews, fires `on_change(0,…)`/`(1,…)` outside the lock | `mod.rs:216-283` |
| `get_value` / `set_value` (no-op if unchanged) / `set_value_silent` (no `on_change`) | `mod.rs:289-395` |
| `row_injection_available()` STRICT (scalar allocator + builder detour + filter hook) | `mod.rs:185-190` |
| `set_option_available(id, bool)` — hide/show at next form rebuild (no unregister exists) | `mod.rs:198-205` |
| `overlay_snapshot(side)` | `mod.rs:486-500` |

Precedents: bool parent + scalar child `src/mods/assist_tick.rs:1531-1558` (parent), `:1091-1133` (child), callbacks `:1045-1074`; `SaveOnly` variant `src/mods/webui_options/profile_fields.rs:121-151`; `ShowWhen::NotEquals` `src/mods/song_playback_speed.rs:215-233`; refuse-enable + `set_option_available(false)` on disable `song_playback_speed.rs:177-180, 342-344`.

Re-enable path: `Err(RegisterError::Duplicate)` is success but does NOT re-fire `on_change` — reseed atomics from `get_value` (`assist_tick.rs:1543-1552`).

## PersistMode (`api.rs:212-217`, `:254-288`)

| Mode | net save (`mod_<id>`) | net load | JSON cache `custom_options.p1/p2.<id>` | card-in reset |
|---|---|---|---|---|
| `Full` | yes | yes | yes | no |
| `SaveOnly` | yes | no | no | no |
| `None` | no | no | no | no |
| `Session` | no | no | no | yes (→ default) |

Wire name `mod_<id>` (`custom_options_persistence.rs:1444`). A new `Full` field needs a bemani-buddy migration `opt_mod_<id>`; until then the JSON cache carries it. `load_transform` runs on network load AND JSON prime (`mod.rs:345-349`).

## ShowWhen

- `Always | Equals{parent_id, value} | NotEquals{…}` (`api.rs:317-327`).
- Parent MUST already be registered (`registry.rs:198-207` ⇒ `UnknownParent`).
- Evaluated per side against the parent's value (`registry.rs:377-393`).
- In-game: hidden children get `row+0xB8 = 0` after the native filter pass (`rows.rs:322-335`); a parent press re-applies immediately (`rows.rs:2416-2435`).
- Overlay: `visible` in the snapshot; `model::build_player_tab` omits it (`mod_menu/model.rs:272-274`).
- Row order = registration order (register the child right after the parent).

## Textures (bool parent + scalar child)

Scripts: `scripts/option_strings.py` (data) + `scripts/gen_option_labels.py` (renderer). Output `data_mods/custom_options/select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/`.

| Texture | Needed for | Edit |
|---|---|---|
| `seop_item_<id>.png` (176×16 label) | both rows — mandatory | `LABELS["<id>"] = {"en","ja","ko"}` (`option_strings.py:64`; example `:80-89`) |
| `seop_op_on/off` | bool parent | nothing (stock) — never add to `RIBBONS` |
| scalar ribbon | none — scalars render as digit text through the native compositor (`api.rs:47-49`) | |
| `seop_image_<id>_off/on.png` (368×172) | bool parent, optional | two `PreviewSpec(...)` entries (`option_strings.py:491-525`) |
| `seop_image_<id>.png` | scalar child, optional | one `PreviewSpec("<id>", None, WIDE, …)` (`:526-544`) |

Run `python3 scripts/gen_option_labels.py`. All three languages required (`gen_option_labels.py:562-566`). Deploying only the DLL leaves a blank label. In-game label text ALL CAPS; overlay `display_name` Title Case (`api.rs:636-638`).

SSO ≤15-byte rule applies to the formatted scalar VALUE text (`api.rs:178-179`), not to `seop_item_<id>` names.

## Center Arrows (1P) anatomy — `src/mods/center_arrows_single.rs`

- Option `center_arrows_1p`, `display_name("Center Arrows (1P Only)")`, `PersistMode::Full`, registered ONLY after hooks install (`:677-704`); per-side `OPTION_ENABLED[side]`, no mirror.
- "1P only" is a **runtime gate**, not a visibility rule: `compute_pass_state` (`:223-266`) reads presence `*(*(*slot)+0x4) != 0` from the player array (`:275-291`); `single_player = exactly one present`; `maybe_center` gate `single_player ∧ side == active_side ∧ styles[side] == SINGLE ∧ OPTION_ENABLED[side]` (`:338-358`).
- Three detours: `hud_layout_builder`, `hud_layout_setter`, song-info card builder (`:425-480`). `required_signatures()` empty.
- Planning: `.agents/planning/20260612-center-arrows-single/` (`design/detailed-design.md:180-204`).

## Overlay PLAYER SETTINGS

- `tabs.rs:101-118` builds from `overlay_snapshot(side)`; all row kinds automatic (`registry.rs:446-494`).
- Non-editable side (not entered, or attract band) ⇒ rows greyed (`tabs.rs:19-31`, `model.rs:395-424`).
- Edit path marshals to the render thread and calls `set_value` (`input.rs:288-313`).

## `option_menu_settings` (`mod-config.json:77-283`)

Array order = display order for both menus; unlisted ids append in registration order; placement override wins over `RegisterSpec.menus`. Next-launch semantics. `center_arrows_1p` at `:139` under `header_playfield_styling_options`; `assist_tick`/`_volume` at `:189-194` under `header_training_options`.

## Traps

1. Parent before child. 2. `Duplicate` = success, reseed. 3. bool rows on `is_available()`, scalar rows on `row_injection_available()`. 4. `register_option` fires `on_change` for both sides at enable time from a non-render thread. 5. `on_change` must not panic (a panic permanently no-ops it). 6. Cross-side recursion terminates only via the unchanged-value no-op. 7. `set_value_silent` for seeding from game memory. 8. Per-side values outlive the player — gate on `side_entered`. 9. Formatted scalar ≤15 bytes. 10. `load_transform` must clamp+snap. 11. Label atlas flushed ONCE at boot (`lib.rs:600-607`) — a default-OFF mod enabled from the menu has no in-game label until next launch. 12. Never add stock ribbon names to `RIBBONS`.
