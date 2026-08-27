# Music Wheel Song Length — Progress

Updated: 2026-08-16 (final)
Status: COMPLETE — cabinet-verified, shipped defaults settled
NEXT ACTION: none. Shipped defaults: offset_x 280 (maintainer-tuned live),
offset_y 0. Feature complete per maintainer 2026-08-16.

Resume protocol: read `research.md` (mechanism facts, address map) and
`design.md` (settled decisions D1–D7, architecture) in this directory.

## Plan checklist

- [x] 1. Setter ABI verified in Ghidra: `FUN_1801d3070(this, names)`
        COPY-assigns the names vector (`FUN_1801d3b20` → range copy
        `FUN_1801d41c0` reads src data+size only; source stays caller-owned);
        empty source takes the clear branch and still runs layout. Element
        stride 0x28 = std::string{buf 16, len, cap} + 8 pad.
- [x] 2. Signatures added (`src/core/signatures.rs`): `spritelayer_ctor`,
        `spritelayer_set_names`, `selectmusic_model_anchor` +
        `derive_selectmusic_model` (RIP at match+3; +0x1B0/+0x1B8 pinned as
        imm bytes). All three hit exactly once on 20260324/20260526/
        20260616/20260721 (Ghidra-verified).
- [x] 3. Art: friend's `muca_card_current_bg2.png` (LENGTH caption baked in)
        + his colon art as `muca_card_len_c.png` committed under
        `data_mods/music_wheel_song_length/select_music_card_v3_ifs/tex/`.
        Colon injected at enable via `atlas_cloner` (donor
        `muca_card_bpm_question`, prefix `mwsl`); first-boot mod-path cache
        staleness handled by a conditional `init_mod_paths()` rescan.
- [x] 4. Mod written (`src/mods/music_wheel_song_length.rs`, id
        `music-wheel-song-length`) + registered in `lib.rs`/`mods/mod.rs`;
        `bm2d_api::find_wrapper_by_name`/`wrapper_matches` pool helpers
        added; `MusicWheelSongLengthConfig` section (offset_x/offset_y/
        spacing/scale) in `mods/config.rs`.
- [x] 5. `cargo check` clean, `cargo fmt`, `./build.sh` clean.
- [ ] 6. Cabinet deploy: calibrate offsets; verify blank-on-scroll, value
        appears on wheel settle, folder cards blank, card fade tracks,
        options-modal view OK, stock BPM readout unaffected, ???-BPM songs
        still show `???`, scene exits release the glyphs (no bitmap-pool
        leak WARNs), no stutter from the per-frame poll.
- [x] 7. AGENTS.md entries (mod + chart_length service) updated.
- [x] 8. Promoted SSQ length to `services/chart_length.rs` (worker + cache
        + digest-stamped latest()); `core/ssq/` now owns ssq_chunk/timing
        (note_types_expansion re-exports). Training mode seeds rows from
        chart length (instant + tight), audio publication fallback.
- [x] 9. Live X/Y offset overlay rows (fine 1 / coarse 10), immediate
        apply + section persist.
- [x] 10. README user docs added ("Music Wheel Song Length" row);
        shipped default offset_x = 280 (maintainer-tuned live), offset_y 0.

## Done

- RE of the header card pipeline (research.md §2–4): SpriteLayer class, card
  tick, selection global, per-frame layout dispatch.
- Friend's pack diffed: exactly 2 changed textures (bg2 with baked LENGTH,
  `?` glyph → colon). Art borrowed with permission; colon is net-new so the
  stock `?` glyph survives (his build shows `:::` for ???-BPM songs).
- Design decisions D1–D7 settled with maintainer.
- Full implementation (see checklist).

## Deploy & test log

- Deploy 1: signatures + layer OK; wrapper never found — pool slots don't
  store clip names (+0x114 = root search path). Switched discovery to
  content-based (`find_wrapper_by_children`).
- Deploy 2: CRASH entering song select — wild call. Minidump → my
  `read_song_code` treated the +0x1B0 HOLDER as the music object; also the
  model global needed one more deref (`*(global)` → model). Fixed both;
  guards added (vtable/getter bounds-checked in-module).
- Deploy 3: chain worked to "code read failed" — guards over-strict on the
  correct object? No: the missing model deref was the real issue (found by
  static validation against the friend's modded 20250805 binary, which also
  decoded his 3-cave implementation: bg-baked label, `?`→colon glyph swap,
  length computed into `music::Info+0x14` padding by his own code).
- Deploy 4: WORKING (`LENGTH 2:13` on-screen, audio length, ~0.5 s late).
- Deploy 5 (label + SSQ rewrite): nothing rendered — the spacer glyph
  `muca_card_bpm_blank` is 19 chars, broke the ≤15 SSO rule, panicked in
  `GameString::set` (silently, panic-contained). Gap baked into label art
  instead; `set` hardened to clamp-not-panic.
- Deploy 6: WORKING — instant chart-derived lengths (SSQ). Fast-scroll
  crash after ~hundreds of songs: folder transitions recycle the card's
  pool slot; stale cached wrapper re-validated after a miss had nulled the
  sprite's parent → layout on NULL parent. Fixed: wrapper/parent lockstep
  + null-parent guard in apply_names. AWAITING RETEST.

## Deviations & open questions

- Digest matching reads the music object's code via vt+0x08 WITHOUT
  refcount traffic (plain reads; all mutators are on the same game thread —
  mirrors the card tick's access pattern). If a build ever moves selection
  mutation off-thread this needs the weak_ptr lock dance.
- Default offsets (350, 0) are a guess; calibrate on cabinet.
- `muca_card_current_bg2` replace collides with any other mod replacing the
  same texture (none today).

## Key facts for a cold resume

- SpriteLayer ctor `FUN_1801d2e00`, setter `FUN_1801d3070`, layout = object
  vtable slot 0; field map in research.md §3 (offsets +0xC0/+0xC8 = x/y
  pixel offset from anchor; spacing +0xE8; anchor SSO name at +0x68; parent
  wrapper at +0x60; priority +0x94 = 4; +0xD8=0 fixed scale +0xE0).
- Selection global `selectmusic_model` (+0x1B0 obj / +0x1B8 ctrl weak_ptr);
  song code via music object vt+0x08 (returns C string); digest via
  `song_rate::binding::song_code_digest` — matches the publication digest
  because both derive from the same `data/sound/win/dance/<code>` path.
- Length source: `song_rate::selected_song::selected_song()` (ms), publishes
  on wheel settle, installed unconditionally.
- Texture names ≤15 chars (SSO discipline): colon = `muca_card_len_c`,
  digits = stock `muca_card_bpm_0..9`.
- `music_info` wrapper found by scanning the BM2D CMovieClip pool (name at
  wrapper+0x114, layer id +0x08) — `bm2d_api::find_wrapper_by_name`,
  re-validated every frame; pool is a static array so stale pointers read
  safe memory.
- Friend's pack at `~/Desktop/Latest Patch (rev. 8-11-26)`; scratch
  extractions were in /tmp/dance_bpm (re-extract: `scripts/unpack_arc.py` +
  `ifstools`, arcs under `$DDR_WORLD_INSTALL/data/arc/bm2d/`).

