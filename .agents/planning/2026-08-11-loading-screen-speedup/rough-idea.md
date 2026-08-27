# Rough idea

Two changes aimed at speeding the scene 18 (LANGUAGE_TO_MODE_INTERSTITIAL) and
scene 21 (CAUTION) asset-preload loading screens, which are dominated by
LayeredFS-injected `custom_options` textures served per-file from disk through
Wine.

1. **Option 2 (RAM cache preload):** preload the `data_mods/_cache/` entries into
   RAM at boot and serve the LayeredFS open/read redirect from memory instead of
   re-reading the cache file from disk on every game open. (This was "option 2"
   of three the agent proposed; the user picked it over the ARC-overlay repack
   and the atlas-packing options.)

2. **Remove the `seop_op_item_*` value-ribbon textures entirely** and convert the
   options that rely on them — the `RenderMode::EnumIndexed` WebUI cosmetic
   categories (CHARACTER, APPEAL BOARD, BACKGROUND ×2, LANE ×2, LANE COVER ×2) —
   to **scalar** options instead of enums. Scalar rows render the value with the
   game's native digit sprites (`seop_num_*`), needing no per-value texture.

3. **Update `scripts/gen_option_labels.py`** to stop generating the
   `seop_op_item_<NNN>` (ITEM #NNN) ribbon set.

## Context (from the diagnosis session that preceded this)

- The user's original report was slow scenes 18/21; root cause of the *regression*
  was `layeredfs.developer_mode: true` (per-open filesystem storm under Wine),
  now fixed by turning it off. These two changes are a follow-up optimization
  pass, NOT the regression fix.
- Healthy CAUTION load ≈ 7 s (verbose off). `select_music_option_lang_eng_v3`
  alone is ≈ 4 s of that, because ~229 of its textures are our injected
  `custom_options` rows served from `_cache/` via the LayeredFS→Wine disk path
  (~9 ms/open) vs stock ramfs textures (300–700/s).
- Of those ~229 injected textures, **150 are `seop_op_item_001..150`** — removed
  outright by change 2. The rest are `seop_item_<id>` labels, `seop_op_<key>`
  ribbons, and `seop_image_*` previews (still needed; sped up by change 1).
