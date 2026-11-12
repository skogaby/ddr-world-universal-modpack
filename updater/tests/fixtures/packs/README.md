# Pack fixtures

`assets/` is a synthetic assets checkout (two dancers in one source, one stage in
another; made-up bytes, no game data). `out/` is what
`scripts/build_packs.py build --assets updater/tests/fixtures/packs/assets --out updater/tests/fixtures/packs/out`
produced from it: two pack zips and `catalog.json`.

The updater's tests read `out/`. `scripts/test_build_packs.py` rebuilds `assets/` and
fails if the result no longer matches `out/` (content ids, markers, zip entries), so the
two sides cannot drift apart silently. After an intended change to the pack format,
delete `out/` and run the command above again.
