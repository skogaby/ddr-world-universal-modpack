# Rough Idea: Options Texture Localization (JPN/KOR)

The modpack injects custom option rows into the game's native options menu via
custom textures. Today only English textures exist, but the game also supports
Korean and Japanese. Goal: provide Japanese and Korean translations for all the
injected textures.

Currently *most*, but not *all*, of those textures are generated through
`scripts/gen_option_labels.py`. A number of custom options textures were
provided directly as-is by someone else who hand-authored them in Photoshop.

Task breakdown per the maintainer:

1. **Unify generation.** Extract the image elements from the hand-authored
   textures, turn those into templates, and integrate those templates plus the
   pre-baked strings into the generation script — so every texture (including
   any image data baked into the right sides for in-game preview) can be fully
   regenerated from the script. No more split between hand-authored and
   programmatically-authored content.

2. **Localize.** Add Korean and Japanese translations to the script and
   generate textures for all 3 languages.

Paths: English options are dumped to
`data_mods/custom_options/select_music_option_lang_eng_v3_ifs`. The other
languages have their own dedicated IFS paths — replace `eng` with `jpn` / `kor`
(e.g. `select_music_option_lang_jpn_v3_ifs`).

Translations: the maintainer has no dedicated translation team — the agent
should produce the JA/KO translations itself.
