# Context + progress — release-docs

Task: `.agents/tasks/2026-09-25-ddr-selection-a-a3-themes/step09/task-01-release-docs-and-readme-sweep.code-task.md`.
Maintainer request (2026-09-26): in the README Highlights, at most two reasonably sized end-user
paragraphs per mod, with no implementation details.

Details dropped from the README, and where they still live:
- the custom-stage `offscreen1` screen naming: `tools/blender_ddr_addon/README.md`;
- the movie-camera clip naming / generator: `background_dancers/movie_camera.rs`;
- the bot's ghost / S-Marvelous / Details-tab behaviour: the Full Feature List row and the
  `multiplayer_bot` docs;
- the audio-clock measurements: gone; the mode knob is in the configuration table;
- the resolution D3D / present notes: gone (not user-actionable).

Paragraph audit after the sweep (words per paragraph):
- Versus Bot 112 / 78;
- Background Dancers 81 / 104;
- DDR SELECTION 131 / 125;
- Gameplay Timing Fixes 50 / 67;
- Power User Statistics 92 / 41;
- Custom Resolution 44 / 41;
- every other mod has one paragraph (the longest is 2-Player BPL at 122).

The generated option-label set rewrote `seop_item_ddr_selection.png` (eng) with identical pixels
(a different PNG encoding). The file was restored to HEAD to avoid churn.

Progress:

- [x] README Highlights sweep + DDR SELECTION rewrite + Full Feature List row + S-Marvelous
      mention.
- [x] `option_strings.py` preview (en / ja / ko), `gen_option_labels.py` (no overflow), previews
      eyeballed.
- [x] `options.rs` description, the mod description, `mod.rs` Surfaces.
- [x] The research-note status line.
- [x] Gate: check / fmt / build clean; harnesses ddr_selection 186, s_marvelous 172 + Leg H,
      custom_options 59, custom_resolution 24, mod_menu 40, multiplayer_bot 98; signature sweep ALL
      GREEN.
- [ ] `.agents/summary` rows: waiting on the maintainer (codebase-summary refresh vs hand edit).
- [ ] §7.3 cabinet matrix (maintainer).

Status: Complete (uncommitted — maintainer commits manually); cabinet matrix pending
