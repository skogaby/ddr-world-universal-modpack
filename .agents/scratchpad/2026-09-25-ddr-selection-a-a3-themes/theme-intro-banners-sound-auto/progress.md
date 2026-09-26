# Progress — theme-intro-banners-sound-auto

- [x] Failing tests: AUTO table × cabinet, explicit rows ignore the cabinet; `dance_message_vN`;
      theme banner packages + PRAY FOR ALL; theme crowd branches / guarding / order, theme callouts;
      theme stage voices — red: 14 compile errors on the missing API
- [x] Pure: `trigger` (`gold_cabinet`, `auto_skin(series, gold)`), `policy` (`dance_message` theme row),
      `banner_logic` (8-entry static package table, `package_cstr`, PRAY FOR ALL on themes),
      `sound/rules` (skin-0 crowd, 4 play slots, `CHEER` = 0.7, new cues), `sound/cues`
      (`ACE_TEPPAN3`), `panel_logic::stage_voice` (themes)
- [x] Engine: `services/cabinet.rs` (new, `machine_type` / `is_gold_cabinet`), `debug_ui.rs` on it,
      `mod.rs::resolve_song` fills `gold_cabinet` for AUTO rows only (arm INFO names the cabinet),
      `banner.rs` on `banner_logic::package_cstr`
- [x] Gate: harness 162 passed (real-bank leg incl. `ACE_TEPPAN3`), `cargo check` / `cargo fmt` /
      `./build.sh` clean
- [ ] Cabinet demo (maintainer)

## Deviations
- `ACE_TEPPAN3` added to the `dsel` manifest (the design assumed every theme cue was already there;
  DDR A's FAILED shutter embeds it).

Status: Complete (uncommitted — maintainer commits manually; cabinet demo pending)
