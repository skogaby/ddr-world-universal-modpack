# Rough idea — config-defined Series (VERSION) filter layout

Starting point: `docs/filter_menu_system_research.md`.

Add a new, **undocumented / experimental** capability inside the Series Expansion mod that lets
a user completely define the layout and filter predicates of the song-select FILTER overlay's
VERSION ("series") menu from `mod-config.json`, without recompiling the DLL.

## Context

- Today the VERSION menu has a hardcoded layout: row 1 = three group tabs (GROUP GOLD /
  GROUP WHITE / GROUP CLASSIC), then two-per-row series-range entries (WORLD, A3,
  A20–A20 PLUS, A, 2013–2014, X–X3 VS 2ndMIX, SuperNOVA–SuperNOVA2, MAX–EXTREME,
  1st–5thMIX). Other filter menus show the underlying layout is a generic grid: each row holds
  1–5 items whose width depends on the row layout (LEVEL = 5/row, MUSIC TITLE = 4/row,
  EVENT = 1/row).
- The user is part of a project adding a few thousand songs and many new series designators,
  so the series filter needs to be reworked completely, and collaborators need to experiment
  with layouts via config only.
- Must remain undocumented, including in `README.md`.

## Current config

```json
"series_expansion": {
  "custom_series": [
    { "series_value": 30, "label": "WORLD RUBY",     "texture_name": "world_ruby" },
    { "series_value": 31, "label": "WORLD SAPPHIRE", "texture_name": "world_sapphire" }
  ]
}
```

## Proposed config (open to iteration)

New key `custom_series_enhanced` under `series_expansion`. If present, `custom_series` is
overridden and ignored. If absent but `custom_series` is present, the existing series-expansion
behaviour is unchanged.

```json
"series_expansion": {
  "custom_series_enhanced": {
    "num_columns": 3,
    "filter_rows": [
      { "label": "1stMIX",    "series_start": 0, "series_end": 0,  "texture": "1stmix" },
      { "label": "2ndMIX",    "series_start": 1, "series_end": 1,  "texture": "2ndmix" },
      { "label": "3rd-MAX2",  "series_start": 2, "series_end": 7,  "texture": "3rdmax2" },
      { "label": "Extreme-A", "series_start": 8, "series_end": 17, "texture": "extremeace" }
    ]
  }
}
```

Rules:

- The first row is always the original three group buttons (GOLD, WHITE, CLASSIC); the user
  never lists them in config.
- The series entries are fully owned by the config. `"filter_rows": []` ⇒ nothing below the
  group buttons.
- `num_columns` drives the grid; the grid is filled sequentially by traversing `filter_rows`.
- Filtering must work for net-new injected series ids too (a core series-expansion goal).

## Per-column-count textures

`texture: "1stmix"` resolves on disk to `sefi_version_1stmix_{N}col.png` (N = 1..5) and the
modpack loads the variant matching the configured `num_columns` at boot, so layouts can be
switched without regenerating art.

## Label generator script

A Python script that generates series-name label textures:

- same font as the existing Options label generator script;
- the green text colour of the stock Series Filter labels;
- one label per canonical series DDR World knows about (1stMIX … WORLD);
- all five column layouts per series, named per the convention above.

Then pre-generate all labels and set up `mod-config.json` with one filter per series, so only
`num_columns` needs changing to experiment.
