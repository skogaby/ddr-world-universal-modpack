# Context — Custom-Options JSON Persistence (code-assist execution)

This is the **execution scratchpad** for implementing the PDD feature. The
authoritative context lives in the PDD docs — do not duplicate them here:

- Design (R1–R14): `.agents/planning/20260531-custom-options-json-persistence/design/detailed-design.md`
- Plan (9 steps): `.agents/planning/20260531-custom-options-json-persistence/implementation/plan.md`
- Research (file:line map): `.agents/planning/20260531-custom-options-json-persistence/research/current-state.md`
- Requirements (D1–D4, Q1–Q7): `.agents/planning/20260531-custom-options-json-persistence/idea-honing.md`

## Scope for this session

Steps 1–8 (all code steps), auto mode. Step 9 (cabinet acceptance) is left for
the maintainer to run on hardware. After Step 8: produce an on-cabinet test plan
(incl. new `mod-config.json`) and refresh steering/summary/README/AGENTS docs.

## Files touched

| File | Steps |
|------|-------|
| `src/mods/config.rs` | 1 (schema), 3 (writer), 6 (migration), 8 (field removal) |
| `src/services/custom_options_persistence.rs` | 1 (reader), 2 (gates), 4 (JSON save), 5 (load timer), 6 (call migration) |
| `src/mods/webui_options/mod.rs` | 7 (slim) |
| `mod-config.json` (bundled), `README.md`, `AGENTS.md` | 8 |

## Verification

No unit-test harness (project convention). Per-step validation = `cargo check
--target x86_64-pc-windows-msvc` (logs to `logs/`). End-to-end behavior is a
cabinet deploy, captured in the final test plan.

## Re-verified research claims (per learnings.md discipline)

- `config.rs`: `CustomOptionsConfig { persist }` at L15–19; `ConfigFile.webui_options`
  at L48 + None inits L73/L86; `save_json_key` at L135. ✅ matches.
- `custom_options_persistence.rs`: persist read L78–81; save emit loop L404–426;
  load read loop L494–516; side from `savedata+0x90` L387–402. ✅ matches.
- `custom_options/mod.rs`: `snapshot_for_save` L268 (pub(crate)), `resolve_from_load`
  L180 (pub(crate)). ✅ both reachable from persistence service (same crate).
- `webui_options/mod.rs`: CONFIG_KEY L14, transforms L36–70, load_from_json call
  L140, default-idx block L171–184, P2 prime L219–224, save_to_json call L293,
  save_to_json/side_key/load_from_json defns L297–348. ✅ matches.
- `lib.rs`: persistence init at L217 (after custom_options L199, before mod
  enable L275). Splash-timer idiom (`thread::spawn` + `sleep`) at L320/358. ✅
