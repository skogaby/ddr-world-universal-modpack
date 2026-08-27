# Project Summary: 20260530-asset-loader-deprecation

## Artifacts produced

```
.agents/planning/20260530-asset-loader-deprecation/
├── rough-idea.md                  ← motivation: retire ARC packing for textures
├── design/
│   └── detailed-design.md         ← R1-R7, component changes, risk areas, success criteria
├── implementation/
│   └── plan.md                    ← 8 incremental steps (7 code/docs + cabinet acceptance)
└── summary.md                     ← this file
```

This feature went straight from rough-idea to design to plan — no separate
`idea-honing.md` or `research/` phase. The scope was already well-understood from
the LayeredFS work (the `ifs-layeredfs` predecessor) and the donor-clone injection
pattern established by `folder_expansion` and `custom_options`. The one open
uncertainty (R-A, below) was resolved by a verbose cabinet deploy during
implementation rather than upfront research.

## What this feature does

Retires the legacy **active** texture-delivery mechanism (`asset_loader::register_arc`
→ the game's `arc_load`) in favor of the **reactive** LayeredFS pipeline. Custom
textures now ride a host IFS the game already opens, served out of `data_mods/` —
no Python ARC packing, no files dropped into the game's `data/` tree.

Three coordinated removals + one migration:

1. **`asset_loader` service deleted** — `src/services/asset_loader.rs` removed,
   along with its `pub mod` line, the `arc_load` signature, and the derived
   `arc_file_open` block in `signatures.rs`. `lib.rs` no longer registers or
   inits it.
2. **series_expansion migrated to LayeredFS** — `enable()` now calls
   `generate_label_atlases()`, which uses `atlas_cloner::generate_cloned_atlases`
   to clone the `sefi_version_world` donor slot into `select_music_option_v3.ifs`
   for each custom label, then `mod_paths::init_mod_paths()` to rescan. Atlas
   prefix is `cser_version` (distinct from `custom_options`' `copt_mods` to avoid
   the shared-cache-blob collision documented in learnings.md).
3. **folder_expansion dead ARC branch removed** — its `arc_path`/`register_arc`
   block was already vestigial (config defaulted to `None`); deleted along with
   `FolderConfig::arc_path`.
4. **hello_world image loading removed** — the two `create_image_widget` calls and
   their `LOGO_TEXTURE`/`BANNER_TEXTURE` consts dropped (no host IFS for those
   demo textures). Text-bounce demo retained. `image_widget.rs` and
   `widget_renderer::create_image_widget` left intact for future callers; the
   image-widget resolver readiness gate switched from `asset_loader::is_loaded()`
   to `texture_resolver::is_available()`.

## Key design decisions

| # | Decision |
|---|---|
| Scope | Retire only the **active** `arc_load` path; keep `image_widget` infra. |
| Series | Migrate via donor-clone (`atlas_cloner`), not auto-inject — see correction below. |
| hello_world | Drop demo image loading rather than build a runtime IFS+kbin writer. |
| Tooling | Keep `scripts/build_ddr_package` as optional/legacy; README notes it's no longer required. |
| No new code | R7: no new game signatures, no new LayeredFS service code. |

## Correction discovered during implementation

The original plan (Step 2) assumed raw `sefi_version_{key}.png` PNGs dropped into
the IFS mod folder would be served by the auto-inject path
(`ifs_textures::inject_new_textures`). A verbose cabinet deploy disproved this:
the PNGs *were* injected ("injected N new textures" in the log) but auto-inject
builds each PNG as its own 1:1 atlas with full-coverage UV (0,0)–(1,1), while the
`filter_item` BM2D MovieClip applies the label **by name** and expects it at a
specific atlas UV slot — so the label rendered wrong/invisible.

The fix mirrors what `custom_options` and `folder_expansion` already do for this
exact IFS: donor-clone a `sefi_version_*` slot via `atlas_cloner` and emit a
`texturelist.merged.xml`. This is captured in `design/detailed-design.md` (the
post-test CORRECTION block) and generalized into `.agents/learnings/learnings.md`
→ "Name-referenced MovieClip textures need donor-clone injection, not auto-inject".

## Status (2026-05-31)

**Implementation complete and tested.** All 8 plan steps landed; verified working
on cabinet. Mapped to git history:

- `70ca21a` — *Migrate away from custom ARC files* (Steps 1–6: service/signature
  removal, series migration, hello_world image removal, widget_renderer decoupling).
- `996258b` — *Rename data_mods/series_expansion to data_mods/custom_series for
  consistency* (Step 7 asset/doc follow-up).

Verification against the design's success criteria:

1. ✅ `cargo check` clean — zero `asset_loader` / `arc_load` references remain in `src/`.
2. ✅ Custom VERSION filter labels render from `data_mods/custom_series/` with no ARC in `data/arc/bm2d/` (donor-clone path, R-A resolved).
3. ✅ Custom folders still render — folder_expansion unaffected.
4. ✅ hello_world text-bounce demo runs without image widgets or crashes.
5. ✅ No `arc_load` / `arc_file_open` warnings; init completes.

Docs updated: `README.md` Custom Series section rewritten to the `data_mods` PNG
workflow; `scripts/build_ddr_package/README.md` carries the "optional / legacy"
note; `mod-config.json` no longer carries `arc_path`.

## Out of scope (deferred)

- A runtime IFS + kbin **writer** to host truly free-floating textures (no stock
  IFS). Would be required to restore hello_world's demo images or any net-new
  texture with no host the game opens — explicitly not built for a demo.
- Removing `image_widget` / `create_image_widget` — kept for future callers even
  though hello_world was the only consumer.
