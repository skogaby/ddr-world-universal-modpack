# Shadertoy Theme Pack — plan (light PDD)

Date: 2026-08-25 · Maintainer-approved requirements (chat session):

## Requirements

- Remove the `arrows`/RHYTHM and `wavefield`/WAVEFIELD themes entirely
  (shaders, blobs, palettes). Keep `bubbles`/BUBBLES and `minimal`/MINIMAL.
- Add 10 new shader-backed themes ported from Shadertoy (sources supplied
  by the maintainer at ~/Desktop/shaders; batch 1 approved + batch 2
  added same session after batch 1 landed cleanly):
  - TERMINAL — https://www.shadertoy.com/view/MlsGDs (glitchy green digit rain)
  - WAVEFORM — https://www.shadertoy.com/view/Wcc3z2 (@XorDev raymarched sine ocean)
  - SPECTRUM — https://www.shadertoy.com/view/tcyGDz (audio-bar visualizer,
    Marco van Hylckama Vlieg / Claude 4.0 Sonnet)
  - TUNNEL — https://www.shadertoy.com/view/MlsfWS (bal-khan ring tunnel, CC BY-NC-SA)
  - XMB — https://www.shadertoy.com/view/fcf3Dn (int_45h PS3 wave, MIT-ish)
  - SQUARES — https://www.shadertoy.com/view/MdVXzw (drifting squares + smoke)
  - MANDELBULB — https://www.shadertoy.com/view/MdXSWn (evilryu, CC BY-NC-SA)
    — CUT 2026-08-25 after two cabinet freezes (D3DMetal buildPipelineState
    failure -> whole-renderer software fallback; see progress.md)
  - CARD SWIRL — https://www.shadertoy.com/view/w3lGzH (Balatro paint vortex)
  - BLOBS — https://www.shadertoy.com/view/WctXD4 (metaballs)
  - PS2 — https://www.shadertoy.com/view/33KBz1 (PS2 startup orbs + trails)
  - PRIME CUBE — https://www.shadertoy.com/view/w3V3DG (prime-coordinate
    voxel lattice; MANDELBULB's replacement, the final theme)
- BUBBLES becomes the default theme (index 0).
- THEME row order: BUBBLES, TERMINAL, WAVEFORM, SPECTRUM, TUNNEL, XMB,
  SQUARES, CARD SWIRL, BLOBS, PS2, PRIME CUBE, MINIMAL.
- Audio-input shaders (SPECTRUM; WAVEFORM's is already commented out) get
  procedurally synthesized fake signals — approved.
- Licensing: maintainer approved shipping CC BY-NC-SA-derived ports in this
  noncommercial project; attribution headers retained in every ported .hlsl.

## Porting constraints (from design §4.6/§4.7 + theme_common contract)

- ps_3_0, fxc 9.29 golden path; entry `float4 ps_main(PSIn i) : COLOR`.
- No constant registers in the PS — time/rect/aspect via interpolators
  (TEXCOORD0/1/2, COLOR0); alpha = `rounded_coverage(i.pxr) * i.col.a`.
- No texture channels / buffers — everything procedural.
- GLSL→HLSL: uint hash → sin-dot hash (XMB), int math → float, iDate →
  slow time drift, AA loop dropped, raymarch step counts tuned down.
- Time wraps mod 3600 s. Pure oscillators are snapped wrap-seamless
  (n·2π/3600 rad/s). The unbounded-travel raymarchers (WAVEFORM, TUNNEL,
  XMB star pan) have no finite scene period — the hourly wrap is a
  jump-cut, documented per shader header (accepted deviation: menus are
  open for minutes; an abstract scene cut reads as intentional).
- Brightness tuned DOWN from Shadertoy (backgrounds sit behind the panel
  wash + menu text) — final colors scaled per shader.

## Lockstep touch points (theme count 3 → 6)

1. `shaders/src/themes/` — delete theme_arrows/theme_wavefield, add 5 new.
2. `scripts/build_shaders.sh` — manifest; rebuild blobs at
   `data_mods/shader_fixes/blobs/` (delete the two stale blobs).
3. `src/services/avs_layeredfs/shader_layout.rs` — `THEME_PROGRAM_COUNT`
   generalization (3→6) + tests.
4. `src/services/avs_layeredfs/shader_synthesis.rs` — blob consts,
   `THEME_BLOBS` [;7], tuple→slice assembly, fingerprint `v3`→`v4`,
   publish width.
5. `src/services/overlay_draw/mod.rs` — `THEME_PROGRAMS` `[u32; 6]`.
6. `src/mods/mod_menu/theme.rs` — `ThemeProgram` {Bubbles, Terminal,
   Waveform, Spectrum, Tunnel, Xmb} (slot = blob append order), THEMES
   table + palettes, `DEFAULT_THEME_INDEX` stays 0 (bubbles first), tests.
7. Docs: `src/mods/config.rs` overlay_menu.theme comment, AGENTS.md Mod
   Menu row, shader_layout/synthesis module docs.

Safety-critical invariant (unchanged): `ThemeProgram::slot()` order ==
`THEME_BLOBS` PS order == `default_theme_indices` order == published
array order. The SetShader handler has no bounds check; the emitter's
`progs >= program+1` gate plus these host tests are the protection.

## Palettes (first cut, maintainer tunes on cabinet)

- TERMINAL: green phosphor — green accent, near-black green panel.
- WAVEFORM: violet/pink accent over deep indigo (XorDev cos-palette).
- SPECTRUM: amber accent over navy-black (red→yellow→blue bars).
- TUNNEL: mint-green accent over dark blue-green (LIGHT glow tint).
- XMB: PS3 blue — pale blue accent, classic XMB blue panel.

## Validation

`./scripts/validate_mod_menu.sh` + `./scripts/validate_overlay_draw.sh`
(host tests), `./scripts/build_shaders.sh` (blob stats sane), `cargo
check` → `cargo fmt` → `./build.sh`; final look is a cabinet deploy
(maintainer judges brightness/perf, tunes palettes).
