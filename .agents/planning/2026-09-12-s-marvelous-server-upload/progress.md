# Progress — S-Marvelous Server-Side Awareness

Updated: 2026-09-12
Status: Step 9 of 9 — DONE (cabinet-validated over six deploys; docs written). Feature complete, uncommitted on the DLL side (maintainer commits manually); bemani-buddy side committed by the maintainer (`128a499`, `4e40eb4`) + two doc edits pending.
NEXT ACTION: none — maintainer review + commit of the DLL changes (and the two bemani-buddy doc edits). Follow-up candidates listed under "Deviations".
Resume protocol: read `implementation/plan.md` (checklist), `design/detailed-design.md`, this file. Register: `idea-honing.md`. Per-step records: `.agents/scratchpad/2026-09-12-s-marvelous-server-upload/*/progress.md`.

## Done (all uncommitted — maintainer commits manually)

| Step | Repo | Result |
|---|---|---|
| 1 | bemani-buddy | model re-synced; regeneration byte-identical |
| 2 | DLL | `records::RawStreams/read_raw_streams`; `upload.rs` pure builder (9 tests, §5.1 invariants) |
| 3 | DLL | `/data` subtree-producer registry in `custom_options_persistence` (void node via ordinal 163 type 1); `upload_hook::produce`; `state::armed_this_song` |
| 4 | bemani-buddy | migration `019_ddr_world_smarv.sql`; `DdrWorldSmarv`; DAO 7 columns both tables; PB tie-break (`replaces_personal_best`) |
| 5 | bemani-buddy | `parse_smarv_node` in `handle_save_scores` (identity cross-check) |
| 6 | bemani-buddy | `option.smarv_scores` via model → regenerate; `build_smarv_scores` |
| 7 | DLL | `lamp_codec.rs` (std-only) + `lamp.rs` (set, string-field registration, card-in clear, local feed) |
| 8 | DLL | signature `selectmusic_card_refresh` (ALL GREEN on 4 builds); `lamp_badge.rs` detour; `muca_card_fc_smfc` art + FRESH clone |

Host validation: `cargo check` (Windows target) 0 warnings; `./build.sh` release clean; `scripts/validate_s_marvelous.sh` 158 tests; `scripts/validate_signatures.sh ~/Desktop/ddr_modules` ALL GREEN; bemani-buddy `cargo clippy` 33 warnings (pre-existing baseline), 66 game-server + 2 db tests.

## Cabinet checklist (first deploy — Steps 3 + 8 together)

Backend first: restart bemani-buddy (log should show migration 019 applied), enable packet logging.

1. **Boot (mod on):** log has `SMarvelous: song-select card-refresh detour installed`, `S-MFC lamp texture staged`, and — critically — NO `Ordinal_163 (void) returned null` / `/data/s_marv emit failed`. If the texture line says "not in mod-path cache — rescanning" on the first boot, a second boot may be needed for the lamp (atlas rebuilt this boot).
2. **One normal play:** log `SMarvelous: upload side=… smarv=… marv=… fast=… slow=… clearkind=… ghostlen=…`; bemani-buddy packet log shows `<s_marv>` beside `<result>` with 11 children; `judge_smarv + judge_marv == result/judge_marv`; `ghostsize` equal in both; results-tab S-MARV row == `judge_smarv`. `SELECT smarv_* FROM ddr_world_scores WHERE mcode=…` populated on both tables.
3. **Stock invariance:** diff the `<result>` block against a mod-off capture of the same chart — identical node set/order/types.
4. **Quick-fail (partial play):** `s_marv/ghost` length == stock `ghost` length; the unjudged tail is `'0'` in both.
5. **S-MFC:** `clearkind 11` in the node; log `+local S-MFC`; back at song select BOTH the header card (`SMarvelous: S-MFC lamp shown`) AND the side-info table's CLEAR RANK row (`SMarvelous: S-MFC side-info lamp shown`; the difficulty-picker's `S-MFC row lamp shown` only when entering the picker) show the violet lamp for that chart; other charts/rows keep stock lamps; scrolling keeps the correct lamp per song; boot log has `difficulty-panel detour installed`.
6. **Round trip:** card out, card in → `SMarvelous: lamp set side=0 loaded N S-MFC chart(s) from server` and the lamp still shows (packet log: `<smarv_scores>` under `<option>` in the load response, stock `score_str`s unchanged).
7. **Mod off:** no `s_marv` node, no lamp, no `SMarvelous: upload`/`lamp badge` WARN lines.
8. **PB semantics (optional):** MFC then S-MFC on the same chart at equal points → the S-MFC row replaces (smarv columns set); a later stock-client/mod-off higher score NULLs them.

## Deviations & open questions

- Test-build switch `score_guard::TESTING_ALLOW_AUTOPLAY_SCORES` was `true` for deploys #1–#6 and
  is REVERTED to `false` (guard test `autoplay_taint_alone_suppresses_its_side` green, 283 passed).
  Compile-time constant only — never a config/env/menu knob (maintainer directive).
- Deploy #6 (2026-09-12): maintainer confirmed everything correct in-game (jackets per-card,
  side-info table, round trip after card-out/in, DB `smarv_clear_kind = 11`).

## Key facts for a cold resume

- Wire: `/data/s_marv` (kind 2 only) — `mcode style difficulty window_ms judge_smarv judge_marv fastcount slowcount clearkind ghostsize ghost`; ghost `'8'` = S-Marv (stock `'0'..'7'`); clearkind 11 = S-MFC.
- Load echo: `option/smarv_scores` = `mcode:chart:clearkind|…` (sorted), only rows whose S-Marv clear kind differs from stock.
- Marshal facts (20260825): record `PW+0x590+stage*0x2B8`; ghost = ALL slots of `rec+0xB8`; wire clearkind `rec+0x54` (`rec+0x270` is `folder` — the deploy-#1 bug); kind-2 in course mode marshals the course record (v1 omits).
- Card lamp: `selectmusic_card_refresh` fn(this, u8); layer `*(this+0xD0)`, id `*(layer+0x08)`; widget `fullcombo_%dp_usr`; stock lamp table indexed by clearkind with NO bounds check (why stock fields are never widened).
