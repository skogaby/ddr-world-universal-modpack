# Shader Replacement Research (DDR World, `gamemdx` 2026061600)

Feasibility record for replacing the game's pixel/vertex shaders via LayeredFS,
motivated by the scaled-arrow aliasing problem (see
`playfield_styling_research.md` §7 — the lane art is palette-indexed, so the
pixelation of off-grid scaled arrows can only be fixed in the shader, not with
sampler state). This note also establishes shader replacement as a reusable mod
class.

All addresses are file-relative to image base `0x180000000` unless noted.
Every claim here is grounded in the shipped binary, the on-disk `.gsp`
containers, and a cabinet deploy — no external sources.

---

## 1. Verdict

**Shader replacement is feasible and cabinet-proven.** A modified
`gs_screencommand_arrow` pixel shader, delivered as a LayeredFS arc overlay,
loads and executes on the cabinet GPU with **no new hooks and no DLL code
changes** — it rides the existing arc-overlay path that texture packs already
use.

Proven end-to-end on 2026-07-19 (see §6): a one-byte PS edit (swap the output
color's R/G channels) visibly recolored the arrows/receptors in gameplay, and
the boot log confirmed the overlay was consumed.

---

## 2. Shader storage — the `.gsp` container (GSPW)

Shaders live in `data/arc/shader.arc` (string @ `0x180387238`). The arc is the
standard Konami ARC container (magic `0x19751120`), holding 36 entries named
`data/shader/<name>.gsp` (+ `data/shader/dam/<name>.gsp`) plus one
`data/shader/hold16.bin`. Entries are AVSLZ-compressed in the stock arc; the
LayeredFS repack rewrites uncompressed, which the loader accepts.

Each `.gsp` is a `GSPW` container wrapping plain D3D9 bytecode. Layout validated
against **all 35** `.gsp` files (parser in this repo's scratch work):

```
off  size  field
0x00  4    magic 'GSPW'  (0x57505347 LE)
0x04  4    name hash  = FNV-1 (32-bit) of the bare shader name, e.g.
                        fnv1("gs_screencommand_arrow") = 0x9E93AC7B
0x08  4    reserved (0)
0x0C  4    ptrA -> program table
0x10  4    ptrB -> VS table
0x14  4    ptrC -> PS table
0x18  1    program-entry count      (cA)
0x19  1    VS-entry count           (cB)
0x1A  1    PS-entry count           (cC)
0x1B  1    reserved (0)
0x1C  4    reserved (0)
...        three tables of 8-byte entries at ptrA/ptrB/ptrC.
           VS/PS entries = { u32 blob_offset, u32 blob_size }.
           blobs are 16-byte aligned, in file order; trailing slack after the
           last blob is ignored by the loader.
```

- The program table (cA entries, 8 bytes each: `{flags:u8 @+0, pad*3,
  vsIndex:u8 @+4, psIndex:u8 @+5, pad*2}`) pairs a VS index with a PS index —
  **entry bytes +6/+7 are never read** by the parser (disassembly-verified;
  an earlier revision of this doc wrongly placed the indices at +6/+7).
  `flags` bit0 = "no pixel shader" (VS-only program; PS index ignored). The
  parser dedupes entries on the triple `{u32@+0, u8@+4, u8@+5}` — identical
  entries share one created program (handle copied + addref'd). Blobs are
  shared by table index: a VS/PS used by several programs appears once in the
  file. Most shaders have 1 program / 1 VS / 1 PS. `dam/app_lightdebug.gsp`
  has 2 programs sharing 1 VS (entry 1 = `00 00 00 00 00 01 00 00`, i.e.
  ps_idx=1 at +5); the `mdl_*` shaders carry 4 identical program entries.
- VS blobs begin `00 03 FE FF` (`vs_3_0`), PS blobs `00 03 FF FF` (`ps_3_0`),
  compiled by "Microsoft (R) HLSL Shader Compiler 9.29.952.3111", each with a
  standard `CTAB` constant table.
- **The engine does not parse `CTAB`** (no `CTAB`/`_gs_ps` byte sequences exist
  anywhere in `gamemdx`). Constant/sampler binding is by fixed register
  convention, not by name. The CTAB is inert metadata that `fxc` emits.

### Editing rule
D3D9 SM3 bytecode has **no checksum** (unlike SM4+ DXBC). Byte-patching a blob
in place is valid as long as the token stream stays well-formed. Changing blob
size is also fine — just rebuild the `{offset,size}` table entries and keep
16-byte alignment; the repo's `ArcArchive` writer handles the arc-level repack.

---

## 3. Load path (how a `.gsp` becomes a live D3D9 shader)

1. `FUN_1801f1cf0` (graphics init) registers `data/arc/shader.arc` with the
   FileManager (`DAT_1806f1f50`) via `FUN_1801fe8e0`, then spins pumping the
   loader until the arc's file-entry state settles.
2. `FUN_1801fe8e0` hashes each name with **FNV-1** and inserts a FileManager
   record. (This is the file-registry hash, distinct from the GSPW name hash,
   though both are FNV-1.)
3. The FileManager pump `FUN_1801fd5a0` walks entries; for each `.gsp` it
   creates a reader (`FUN_1801fe2b0`) that opens the path through the AVS
   filesystem (`FUN_180201390` → AVS `open`), reads the bytes, then hands the
   GSPW buffer to the shader-registry acquire `FUN_18025f3a0` →
   `FUN_18025ee30` (the GSPW parser).
4. `FUN_18025ee30` reads ptrA/ptrB/ptrC + counts, dedupes programs, and for each
   program calls `FUN_180256150(vsBlob, vsSize, psBlob, psSize)`. That function
   **`memcpy`s the blobs verbatim** into buffers and posts render-thread command
   `6` (`FUN_180254160`) which is what ultimately calls
   `CreateVertexShader`/`CreatePixelShader`. No validation, transform, or
   re-hash of the bytecode.
5. Shaders are looked up by GSPW name-hash at draw time via `FUN_18025f110` /
   `FUN_18025f590` (spinlock `DAT_1806f1038`, hash fn ptr `DAT_1806f1040`).
   `FUN_1801f5d10` resolves and caches `gs_screencommand_default`
   (`DAT_1806f0558`); the ArrowRenderer ctor `FUN_1800268d0` resolves
   `gs_screencommand_arrow`.

**Consequence:** whatever bytes LayeredFS serves for a `.gsp` are the bytes that
reach `CreatePixelShader`. Identity is the GSPW header hash, not the filename, so
the overlay must preserve the stock hash (it does — we edit only bytecode).

---

## 4. Delivery (LayeredFS arc overlay — no new hooks)

`data/arc/shader.arc` open → `mod_paths::normalise_path` → `arc/shader.arc` →
`file_hooks::find_mod_replacement` sees `.arc` → `arc_handler::handle_arc`.
`handle_arc` looks for mod folders at `arc/shader_arc/` (`.arc`→`_arc`), scans
them recursively, and repacks a cached arc with `ArcArchive::add_or_replace`.

The scan key must equal the stock entry name **exactly** (BTreeMap keyed by
name; a mismatch appends a duplicate instead of replacing). Stock entries are
`data/shader/<name>.gsp`, so the mod file goes at:

```
data_mods/<modfolder>/arc/shader_arc/data/shader/gs_screencommand_arrow.gsp
```

Cache lands in `data_mods/_cache/arc/shader.arc`, hash-guarded on (original arc +
overlay files) — regenerates only when an overlay changes.

> **2026-07 update:** the shipped shader containers no longer travel as
> committed overlay files — `arc_handler::handle_arc` calls
> `shader_synthesis::synthesize` for `arc/shader.arc` and injects the
> runtime-synthesized `.gsp` entries into the same repack (an operator's
> explicit overlay of the same entry name still wins). See §8.

**Boot order (verified, verbose log 2026-07-19 15:40):**
`all AVS hooks installed` @ 15:40:05 → `open /local/data/arc/shader.arc` @
15:40:06. Hooks are in place ~1 s before the shader arc is opened, so the
overlay is always seen. (LayeredFS hooks install during EA3/AVS boot, well
before the D3D9 device/graphics init that pumps `shader.arc`.)

> **2026-09-03 correction — that margin was a single-machine accident, not
> a guarantee.** A Win7 tester's cabinet (real p4io/BMPU, LAN e-amusement
> server answering `services.get` instantly, Haswell-era CPU) lost the race:
> `overlay_draw diag … progs=1` all session, zero `shader_synthesis:` lines,
> animated menu backgrounds static. Root cause: `lib.rs` installed LayeredFS
> only AFTER `resolve_all` (127 AOB scans over 19 MB, 11 of them full-module
> misses) + `early_apply` + `resolve_derived`, while the game's
> `Application::onBoot` (`FUN_1800020b0` on 20260721) runs on its own thread
> from the moment gamemdx loads: `startup.arc` drain → `FUN_1801f2420` (D3D
> device init + **synchronous `shader.arc` drain**) → `FUN_1801b3d30`
> (license/musicdb/coursedb `createLi`) → sound init → `soundbanks.arc`. The
> shader open landed between our gamemdx wait and our hook install.
> `musicdb.xml` (custom series/folder merges) sits in the same window a few
> hundred ms later. Fix: LayeredFS now initializes at lib.rs **step 0b —
> before the gamemdx wait and the signature scan** (it needs only libavs
> exports, which `avs_resolver::wait_for_avs_dll` waits for, and
> `./data_mods`). Detection: `overlay_draw::check_shader_arc_race` WARNs once
> when the default container is live while `shader_synthesis::status()` is
> still `NotSeen`; `ShaderFixes: enabled (…; synthesis: …)` reports the real
> outcome instead of asserting it.

---

## 5. `gs_screencommand_arrow` — the palette pipeline, decoded from bytecode

The stock `.gsp` (772 bytes, VS blob @ 0x40 size 0x190, PS blob @ 0x1D0 size
0x134) decodes to:

**Vertex shader** (`vs_3_0`):
```
dcl_position v0 ; dcl_texcoord v1 ; dcl_color v2
def c0 = (1,0,0,0)
mov  o0, (v0.x, v0.y, 0, 1)                 ; position passthrough
mad  o1.xy, v1, c32.xy, c32.zw              ; texcoord' = uv*c32.xy + c32.zw
mul  o2, v2, c22                            ; color * ScreenCommandBaseColor
```
CTAB names: `_gs_vs_parameter_SamplerParameters` (c32, 1 reg),
`_gs_vs_parameter_ScreenCommandBaseColor` (c22).

**Pixel shader** (`ps_3_0`):
```
dcl_2d s0 (Material[0] = atlas) ; dcl_2d s1 (Material[1] = palette)
dcl v0.xy (texcoord) ; dcl v1 (color)
mov   r0.y, v1.x                 ; palette row V = vertexColor.x
texld r1, v0, s0                 ; atlas sample
mov   r0.x, r1.x                 ; palette U = atlas.RED (the index)
texld r0, r0, s1                 ; palette lookup -> RGBA
mul   r0.w, r1.w, r0.w           ; a = palette.a * ...
mov   oC0.xyz, r0                ; RGB = palette color   <-- PoC patched swizzle here
mul   oC0.w,  r1.w, r0.w         ; a = atlas.a * palette.a * vColor.a
```
This matches §7's description exactly: atlas RED → U into a 256×16 palette on s1,
palette V from vertex color. Point sampling on the atlas is load-bearing because
LINEAR would blend palette *indices*.

### Texel-size facts for the index-aware filter
- **Vertex UVs are normalized [0,1].** `render_notes` (`FUN_180026b00`) builds
  each quad's UV rect as `col·(384.0/texW)`, `row·(96.0/texH)` — cells are
  384×96 px, and it divides by the bound texture's `ushort` width@`tex+8` /
  height@`tex+10`. (Constants: `DAT_180399338=384.0`, `DAT_18035a710=96.0`.)
- Bound sheets (all confirmed from install data, `data/arc/2d/`, via the
  renderer setup fn `FUN_18005cca0`): arrow/receptor/freeze art `arrow%02d`
  (renderer `this+0x20`, ALSO the judge renderer's sheet) = **768×192**;
  shock-arrow electric crackle `shock_effect00_{l,m,s}` (`this+0xD0`) =
  **768×384**; lane notice `lane_notice00` (`this+0xE0`) = **192×384**.
  (Correction: the live-CE labels for +0xD0/+0xE0 were guesses — +0xD0 is the
  shock sheet, +0xE0 is lane_notice; the measured dims were right.)
- Therefore `c32` (`SamplerParameters`) is a half-texel/identity fixup applied to
  already-normalized UVs — **not** a `(1/W,1/H)` scale — so it cannot be reused
  directly as the PS texel size.
- **Live-verified (CE, frozen attract-gameplay frame, 2026-07-19):** every
  `SetVertexShaderConstantF(reg 32)` record in the gs command buffers carries
  **(1, 1, 0, 0)** — `GS_TransformTexCoord` is an identity at runtime. Arrow
  quad records (cmd id 4) show integer pixel positions with edge-aligned uv
  rects (receptors: pos 88..184 px, uv 0..0.125 = 96 texels over 96 px). That
  uv rect is the quad EXTENT; per-pixel sample points sit at D3D9 pixel
  centers (k+0.5) and interpolate to texel-coordinate k+0.5 — a texel
  **center**. So at 1:1 a replacement bilinear filter (`uv/TEXEL − 0.5`)
  already collapses to the stock texel (`frac==0`, `UV_BIAS = 0`); confirmed
  by stock POINT being stable/crisp at 1:1 (edge samples would seam-flicker)
  and the LINEAR shock/lane sheets being crisp at 1:1 (edge samples would be
  permanently 50/50-blurred). (Command-record layout: `{u16 id, u16 size,
  payload}`; id 0x2E = SetVSConstantF `{u16 reg, u16 count, float[4]×count}`,
  id 4 = quad batch `{u32 count, u64 ptr, quads[0x34 each: 8 pos floats, 4 uv
  floats, u32 color]}`, id 8/9 = texture bind `{u32 stage, u32 tex_id}`,
  id 0x13 = shader select.)

---

## 6. Proof of concept (2026-07-19)

- Edit: `gs_screencommand_arrow.gsp` offset `0x2EE` `0xE4`→`0xE1` — the final
  `mov oC0.xyz, r0` source swizzle `.xyzw`→`.yxzw` (R↔G swap). One byte, same
  file size, stock GSPW header/hash.
- Delivered at
  `data_mods/shader_poc/arc/shader_arc/data/shader/gs_screencommand_arrow.gsp`.
- Result: arrows/receptors rendered with swapped R/G in gameplay; log showed
  `arc cache up to date for arc/shader.arc` → `using
  ./data_mods/_cache/arc/shader.arc`. **Pipeline confirmed end-to-end.**
- Cross-version: the arrow `.gsp` is byte-identical between builds 20260324 and
  20260616 (good signal the shader set is stable across updates).

---

## 7. The real fix — index-aware bilinear PS (IMPLEMENTED, cabinet-accepted 2026-07-19)

**Status:** Shipped for BOTH `gs_screencommand_arrow` and
`gs_screencommand_judge`. Sources in `shaders/src/*.hlsl`, built by
`scripts/build_shaders.sh`, committed at
`data_mods/shader_fixes/arc/shader_arc/data/shader/*.gsp`. Design +
implementation record: `.agents/planning/20260719-shader-injection/`. Cabinet
result: 100 % scale unchanged from stock; 50 %/125 % significantly reduced
aliasing on arrows/receptors/judge text, no banding.

Replace `color = palette[atlas[uv].r]` with: take 4 taps at the atlas texel
centers surrounding `uv`, palette-lookup **each**, and bilinearly blend the four
resulting **colors** by the sub-texel fractions. This blends colors (safe), not
indices (banding), and is well within `ps_3_0`'s instruction budget.

**Stock-identical at aligned scales — the key correctness property.** At 100 %
(and any integer-aligned) scale the sprite samples at exact source-texel
centers, so the sub-texel fraction is 0 → the 4 taps collapse to the single
stock tap → byte-identical output. This holds as long as the texel-size constant
makes `uv·size` land on integers at those scales.

**Texel-size acquisition (the one remaining decision):**

- **Option A — `def` constants `du=1/768, dv=1/384`.** Width is exactly 768 for
  all known lane textures, so `du` is exact. `384` is a multiple of the other
  known height (`192`), so `uv·384` is still integral for the 768×192 sheet →
  frac=0 at aligned scales → stock-identical preserved for *both* textures. The
  only effect of a "wrong" `dv` on the 192 sheet is a slightly smaller off-grid
  blend radius (softer, never banded) — graceful degradation. **Risk:** any
  bound texture whose width ≠ 768 would break stock-identity; needs a cabinet
  check that all shader-arrow draws use 768-wide sheets.
- **Option B — exact per-draw texel size.** ~~Confirm `SamplerParameters` (c32)
  carries a half-texel offset and forward `texSize` from the VS.~~ **Dead:**
  c32 is live-verified as identity (1,1,0,0) — it carries no texture
  information, so there is nothing to forward. Exact per-draw texel size would
  need engine-side help (a DLL hook injecting a constant), out of scope.

**Chosen: Option A**, and cabinet-confirmed. Shipped constants are
`TEXEL=(1/768, 1/384)`, `UV_BIAS=(0,0)`. All three sheets the arrow shader
binds are 768-wide-or-a-divisor and 384-or-a-divisor tall (768×192, 768×384,
192×384), so the stock-identity collapse holds for every one — the earlier
"needs a cabinet check that all draws are 768-wide" risk is now closed by the
install-data extraction (§5) and the 100 % regression pass.

**Scope note:** `gs_screencommand_judge` (judge text / receptor hit flash) got
the same treatment (it shares the 768×192 sheet); its only stock-contract
differences are a fixed palette row (V=0.15625) and a full-vertex-color
multiply.

---

## 8. Toolchain (as shipped)

> **2026-07 restructure (player-perspective feature):** the committed `.gsp`
> containers were RETIRED in favor of **runtime synthesis** —
> `src/services/avs_layeredfs/shader_synthesis.rs` builds the containers at
> boot (lazily, inside `arc_handler`'s interception of the game's
> `data/arc/shader.arc` open) from the game's OWN stock blobs plus the
> committed mod blobs at `data_mods/shader_fixes/blobs/*.d3dbc`. Program 0
> of every touched container is the game's own stock VS (the recompiled
> replica `vs_main` is retired); the anti-aliasing PS is a **cabinet-wide
> toggle** (`shader_fixes.anti_aliasing`, mod-overlay row, next-launch);
> the player-perspective mod's perspective VS rides as **program 1** of
> the arrow/default/judge containers (multi-program GSPW — counts at
> hdr+0x18, 8-byte program entries `{flags@+0, vs@+4, ps@+5}`, blobs
> shared by table index; the judge container's program 1 reuses the ARROW
> perspective VS blob — the stock judge VS is byte-identical to the stock
> arrow VS and the judge PS reads the same v0/v1 contract).
> Minimal-overlay rule: arrow+judge iff AA∨persp, default
> iff persp; nothing enabled ⇒ nothing overlaid ⇒ literal stock bytecode.
> Synthesized files are fingerprint-cached at
> `data_mods/_cache/shader_synthesis/` and validate with
> `gsp_pack.py inspect` (the Rust packer is byte-compatible with
> `gsp_pack.py pack`, verified against live artifacts).
>
> **Compiler golden path is now Microsoft `fxc` 9.29.952.3111** (the stock
> shaders' own compiler lineage), checked into the repo at `tools/fxc/` and
> run under the CrossOver bottle by `scripts/build_shaders.sh`. Rationale:
> vkd3d's SM3 backend does not optimize — our AA PS compiles to 133
> instructions under vkd3d vs 30 under fxc (4.4×), and the same-function VS
> hits instruction-for-instruction stock parity. The Docker/vkd3d path
> remains as the no-wine fallback (`--vkd3d`).

Historical (pre-restructure) pipeline: HLSL → `ps_3_0`/`vs_3_0` d3dbc via
**`vkd3d-compiler` 1.14** (Wine project) inside a Docker image built from
`fedora:42`; `scripts/gsp_pack.py` wrapped the blobs into committed GSPW
containers delivered as plain LayeredFS arc overlays.

---

## 9. Key addresses (20260616)

| What | Address |
|------|---------|
| `"data/arc/shader.arc"` string | `0x180387238` |
| graphics init (registers shader.arc) | `FUN_1801f1cf0` |
| FileManager register-by-name (FNV-1) | `FUN_1801fe8e0` |
| FileManager loader pump | `FUN_1801fd5a0` |
| reader create (AVS open) | `FUN_1801fe2b0` → `FUN_180201390` |
| shader-registry acquire | `FUN_18025f3a0` |
| **GSPW parser** | `FUN_18025ee30` |
| VS/PS create (memcpy blob → CreateShader) | `FUN_180256150` → cmd6 `FUN_180254160` |
| shader lookup by hash | `FUN_18025f110` / `FUN_18025f590` |
| shader spinlock / hash fn ptr | `DAT_1806f1038` / `DAT_1806f1040` |
| default shader handle | `DAT_1806f0558` |
| ArrowRenderer ctor (binds arrow shader) | `FUN_1800268d0` |
| render_notes (builds quad UVs, normalized) | `FUN_180026b00` |
| gs command executor (SetVS/PSConstF cases) | `FUN_18024c310` |
| cell-size consts (384.0 / 96.0 / 1.0) | `DAT_180399338` / `DAT_18035a710` / `DAT_180358f64` |
