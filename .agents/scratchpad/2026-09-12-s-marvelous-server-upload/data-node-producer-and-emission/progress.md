# Progress — Step 3: DLL /data subtree-producer registry + s_marv emission

Status: Complete — host side (uncommitted — maintainer commits manually); CABINET VALIDATION PENDING

- `custom_options_persistence.rs`: `NodeLeaf`/`NodeSpec`/`DataNodeProducer`,
  `register_data_node_producer` (replace-by-name) / `unregister_data_node_producer`,
  `emit_data_node_producers(kbin_ctx, side, savekind)` called post-original after
  `emit_string_fields` under `PERSIST_NETWORK`: void container via ordinal 163 type 1 (value slot 0),
  s32/str leaves, partial failure ⇒ ordinal-164 removal + one latched WARN per process.
- `state.rs`: `ARMED_THIS_SONG` latch (set in `arm`, cleared only by `clear_song_armed` on disable).
- `upload_hook.rs`: producer `produce(side, savekind)` — gate ladder per design §4.3; reads
  `rec+0x00/+0x04/+0x08/+0x28/+0x50/+0x54/+0x6C/+0x70` after `is_readable(rec, 0x74)` (deploy #1 fix: `+0x270` is `folder`, the wire clearkind is `+0x54`);
  `read_raw_streams` → `upload::build_payload` → `NodeSpec`; INFO per node; `lamp::insert_local` on 11.
- `lamp.rs` (state + registration; pulled forward from Step 7 because D15 needs `insert_local`) +
  `lamp_codec.rs` (std-only codec, harness-mounted, 3 tests).
- `mod.rs`: `upload_hook::activate()/lamp::activate()` in enable; deactivate + `clear_song_armed` in disable.
- `cargo check --target x86_64-pc-windows-msvc` clean (0 warnings); harness 158 passed; `cargo fmt` run.

Cabinet checklist (do with Step 8's deploy):
- One play, mod on, packet log: `/data/s_marv` present with 11 leaves; values == results tab.
- Stock `<result>` byte-identical to a mod-off capture. No status-1 responses from bemani-buddy (pre-Step-5 server ignores the node).
- Failed-out song: `s_marv/ghost` same length as stock `ghost`, `'0'` tail.
- Mod off: no node, no `SMarvelous: upload` WARN lines. Log must NOT contain "Ordinal_163 (void) returned null".
