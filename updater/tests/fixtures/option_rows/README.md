# Option-row fixtures

The ids of `custom_options.option_menu_settings`, in order, as the `mod-config.json` of a
published release shipped them (`v1.3.txt`, `v1.4.txt`). The option-row merge tests use
them as "what a user who never re-ordered has" and check that merging the repository's
current `mod-config.json` into each reproduces the current order exactly.
