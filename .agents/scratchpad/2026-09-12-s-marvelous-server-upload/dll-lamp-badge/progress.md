# Progress — Step 8: DLL song-select S-MFC lamp
Status: Complete — host side (uncommitted — maintainer commits manually); CABINET VALIDATION PENDING
- `signatures.rs`: `selectmusic_card_refresh` (prologue + body head through `CMP [RCX+0xD0]`, register/frame/spill bytes wildcarded). Sweep ALL GREEN: +0x1452E0 / +0x147B90 / +0x15A420 / +0x15A450 (one hit per build).
- `lamp_badge.rs`: GenericDetour `fn(this, u8)`, post-original, catch_unwind; scene 25; per entered side with a non-empty set: chart from `PW+0x50/+0x54/+0x5C`, `layer = *(this+0xD0)` → `id = *(layer+0x08)` (probed), `layer_find_child(id, "fullcombo_{n}p_usr")` → `mc_load_bitmap(.., "muca_card_fc_smfc")` over traversal-6 siblings. Texture: FRESH clone (`smarv_smc`, donor `muca_card_fc_mfc`) into `select_music_card_v3_ifs`, PNG `data_mods/s_marvelous/select_music/muca_card_fc_smfc.png` (20×8 violet recolor, alpha identical).
- `./build.sh` release clean; `cargo fmt` run.
Cabinet checklist: see planning progress.md.
