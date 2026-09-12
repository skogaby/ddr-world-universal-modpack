# Progress — Step 6: bemani-buddy load emits option/smarv_scores
Status: Complete (uncommitted — maintainer commits manually)
- `models/ddr_world/playdata_3.json`: `"smarv_scores": "str?"` appended to `PlayerdataLoadOption`; regenerated (diff = the one field, `skip_serializing_if`).
- `build_smarv_scores(&scores)` (entries where `smarv.clear_kind != clear_kind`, sorted, `mcode:chart:clearkind|…`, None when empty); set in `handle_playerdata_load`, None in new-player + test fixture.
- 2 unit tests sharing the DLL `lamp_codec` vector (`1234:8:11|38548:0:11|38548:3:11`). 66 tests green.
