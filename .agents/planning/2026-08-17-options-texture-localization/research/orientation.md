# Orientation: Options Texture Localization (JPN/KOR)

Date: 2026-08-17

## Current texture inventory (eng IFS tex dir)

`data_mods/custom_options/select_music_option_lang_eng_v3_ifs/tex/` holds 93 PNGs.
Diffing against what `scripts/gen_option_labels.py` generates:

**Script-generated today** (fully regenerable):
- 34 row labels `seop_item_<id>` (176x16, black, Inclusive Sans SemiBold 16, baseline 13)
- 4 header labels `seop_item_header_*` (352x16 opaque blue bar, white text)
- 4 value ribbons `seop_op_{fullscreen,overhead,hallway,distant}` (132x24, teal #00ffbd)
- ~30 preview panels `seop_image_*` (368x172, white Sen SemiBold 13.5, WIDE layout)

**Hand-authored (NOT in the script)** — the unification targets:
| File(s) | Content | Baked image elements |
|---|---|---|
| `seop_image_autoplay_{on,off}` | text only (WIDE) | none — pure script take-over |
| `seop_image_premium_free_{on,off}` | SPLIT: text left, art right | white "1st STAGE/2nd STAGE/FINAL/EXTRA" stage-list art right of divider |
| `seop_image_center_arrows_1p_{on,off}` | SPLIT | gameplay screenshot art (centered vs default lane) |
| `seop_image_customize_movie_size_{on,off,fullscreen}` | SPLIT | gameplay screenshot art (3 variants) |
| 9 × `seop_image_customize_*_TEMPLATE` | baked text top-left + solid green marker rect (0,255,0,255) | marker rect = runtime art placement contract (preview_gen.rs) |
| `seop_return` (72x36) | floppy-disk icon, **no text** | language-independent icon |
| `seop_tab_title_mods` (124x30) | floppy icon + styled teal "Modpack" wordmark | brand wordmark, likely stays Latin |

Note: the script's PREVIEW_* metrics were already reverse-measured off the
hand-authored originals ("per-line baselines and ink edges fit against every
line of all 18 shipped panels") — so a Sen-based take-over of the hand-authored
text sides should reproduce their layout nearly pixel-exactly.

## DLL-side findings (explore-agent report, verified against src/)

The user's framing ("script work + translations") understates the scope: **jpn/kor
textures will not function without DLL changes.**

1. **LayeredFS is generic** — `data_mods/custom_options/select_music_option_lang_jpn_v3_ifs/`
   would be *seen* automatically (`file_hooks.rs::find_mod_replacement` does
   `.ifs`→`_ifs` expansion for any path; `ifs_textures.rs::parse_texturelist`).
   **But** net-new `seop_*` names ride the auto-inject path there, which builds
   1:1 atlases with wrong UVs for name-referenced MovieClip textures
   (`.agents/learnings/learnings.md` — donor-clone required).
2. **The donor-clone atlas build is eng-hardcoded**:
   `src/services/custom_options/asset_gen.rs:40-43` —
   `LANG_ENG_ARC/IFS/IFS_MOD_PATH/ATLAS_PREFIX` constants; `flush_label_atlas`
   reads the stock texturelist from the **eng ARC only** and injects
   `seop_tab_title_mods`, all `seop_item_*` (donor `seop_item_appearance`),
   net-new `seop_op_*` (donor `seop_op_on`), and `seop_image_*` panels (donor
   `seop_image_scroll_speed`, fresh-atlas mode). Must be parameterized/looped
   per language.
3. **preview_gen.rs is eng-hardcoded**: `PREVIEW_OUT_DIR` (line 49) — template
   lookup (`seop_image_<id>_TEMPLATE.png`), runtime chrome output
   (`seop_image_<id>.png`, skip-if-exists), and `marker_rect_for` all resolve
   in the eng tex dir. `generate_chrome` renders **no text** at runtime — all
   template text is pre-baked, so localization = per-language template PNGs +
   per-language chrome output.
4. **The DLL does not know the game's language.** Nothing reads it. Options:
   build all 3 languages unconditionally at init (atlas build is disk-cached —
   `generate_cloned_atlases_cached`), or react to which lang IFS the game opens
   (timing-fragile: the merged texturelist must exist before the open).
5. **Language-independent pieces**: `seop_tab_icon_mods` goes into the base
   (non-lang) `select_music_option_v3.ifs` — no work. `filter_hook.rs` binds
   `seop_tab_title_mods`/`seop_return` by name only — works in any language
   provided the names exist in the loaded lang atlas.
6. **Scene preload** (`docs/scene_load_analysis.md`): scenes 18/21 preload the
   package set plus `_lang_eng` variants **for the active language** — per-language
   injection shouldn't multiply runtime texture-open cost, only (cached) boot work.
7. Sibling pattern: `src/mods/folder_expansion.rs:114-126` has its own
   `select_music_folder_lang_eng_v3` constants — same hardcoding, **out of scope**
   here (folder textures are operator-authored config content, not shipped strings),
   but worth noting for a future pass.

## Font findings

`scripts/fonts/` already contains **game-extracted Konami KBF fonts** converted
to TTF (`2d_font_*.ttf`): bitmap-only (`sbix` strike at exactly 17px, no
outlines; Pillow loads them only at size 17).

| Font | JP kanji | JP kana | KR hangul |
|---|---|---|---|
| 2d_font_ui / system / ark_system / songtitle_{m,s} | Y | Y | **N** |
| 2d_font_player / rival | N | N | N |
| InclusiveSans / Sen (current script fonts) | N | N | N |

⇒ **No current font covers Korean; none of the vector fonts cover CJK at all.**
New JP+KR-capable vector fonts are needed (e.g. Noto Sans JP / Noto Sans KR,
OFL). The game's own bitmap UI font could theoretically serve JP at 17px but
not KR, and can't be resized cleanly — an external font pair is the coherent
choice.

## Verification constraints

- The stock jpn/kor ARCs (`data/arc/bm2d/select_music_option_lang_{jpn,kor}_v3.arc`)
  and their donor textures (`seop_item_appearance`, `seop_op_on`,
  `seop_image_scroll_speed`, `seop_tab_title_basic`) are assumed to exist with
  the same names — verifiable only against game data (cabinet or local dump).
- End-to-end validation requires running the game in Japanese/Korean (test menu
  language setting or the scene-41 language select).

## Scope shape

1. **Unify generation** (script): extract baked art → `scripts/templates/`,
   add the hand-authored panels + the 9 customize TEMPLATEs + (optionally)
   `seop_tab_title_mods` to the generator; regenerated ENG output must visually
   match shipped originals.
2. **Localize** (script): per-language string tables (agent-authored JA/KO),
   CJK fonts, per-language OUT_DIRs (`eng`/`jpn`/`kor`).
3. **DLL**: parameterize `asset_gen.rs` per language (loop 3 langs, fail-open
   per lang), parameterize `preview_gen.rs` template/chrome/marker paths.
4. **Verify**: cargo check/fmt/build + cabinet deploy in each language.
