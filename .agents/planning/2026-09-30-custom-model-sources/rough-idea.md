# Rough idea — custom dancer / stage SOURCES

Enhance the Background Dancers custom-content support (`data_mods/custom_models/{dancers,stages}`).

Today one option row each for dancer and stage; the mod loads every custom dancer / stage and
appends it to the end of the stock list. With ~70+ custom dancers (115 folders on disk today) and
more dancers and stages coming, one flat list makes pinpointing an entry tedious.

Proposal:

- Two additional per-player option rows, **Dancer Source** and **Stage Source**, shown when custom
  model support is enabled.
- With custom models on, the stock models are selectable by choosing **STOCK** as the source;
  otherwise the dancer / stage set changes with the chosen source.
- Sources are determined dynamically from one more folder level under `data_mods/custom_models`:
  `data_mods/custom_models/dancers/DDR STRIKE/<dancer folders…>` makes `DDR STRIKE` a Dancer Source
  whose pool is only those dancers (plus RANDOM, always available for any source).
- RANDOM for both the Source rows and the Dancer / Stage rows:
  - Source = RANDOM ⇒ random across the entire pool, all sources.
  - Source = specific, Dancer/Stage = RANDOM ⇒ random within that source only, respecting the movie
    screen rules where possible (if e.g. no stage in the source has monitors and the song has a
    movie, still pick a monitor-less stage from the source's pool).
- Source = RANDOM ⇒ the corresponding Dancer / Stage row is hidden (child rows of the Source rows via
  show_when / hide_when).
