# Custom Shader-Based In-Game Backgrounds — Feasibility & Implementation Strategy

Status: **RESEARCH COMPLETE — GO, phased** (2026-08-30). No code written; this
document is the design-input record for a future PDD cycle.

Goal: user-selectable, fully custom **D3D shader animated backgrounds**
(Milkdrop-style visualizations included) for menus and gameplay, presented as
additional options alongside the game's stock AFP backgrounds, ideally
**audio-reactive** to whatever is currently audible (menu BGM, song-select
preview, the chosen song in gameplay).

Addresses are file-relative to `gamemdx.dll` base `0x180000000`. New RE in this
document was performed on **20260721** (backdrop manager) and **20260616**
(render-graph boot, RT/texture registry); build noted per finding. Builds on:

- `docs/overlay_draw_research.md` — command-list emission, layer dispatcher, anchor z
- `docs/custom_arrow_renderer_research.md` — full command-list tag map (§3), shader bind
- `docs/shader_replacement_research.md` — GSPW container format, shader load path
- `docs/bm2d_background_preview_research.md` — background packages, create recipe, crash classes
- `docs/player_customization_system_research.md` — Customize object, background fields, wire
- `docs/xact_audio_research.md` / `docs/xact_streaming_research.md` — audio engine, taps
- `docs/chart_strip_hud_research.md` §"dynamic texture" — per-frame CPU→GPU texture path

## 1. Executive summary

**Everything needed for a single-pass fullscreen animated shader background is
already production-proven in this codebase.** The mod menu's animated themes
render custom HLSL through the game's own pipeline today: runtime-synthesized
GSPW shader containers (`avs_layeredfs/shader_synthesis.rs`), per-frame
command-list emission of `SetVSConstantF + SetShader + quad` blocks
(`services/overlay_draw`), and per-frame time constants. The two genuinely new
problems are:

1. **z-placement** — drawing at the *bottom* of the frame (under all game UI)
   instead of the overlay's near-top position. This is smaller than it looks:
   the failure mode that plagued the overlay menu ("segment-start appends get
   buried under the layer's own content", `docs/overlay_draw_research.md:274-282`)
   is precisely the success mode for a background. The layer-dispatcher detour
   — already installed as a passthrough — can append pre-original at the head
   of a walked entry's list, placing a quad below everything that layer draws.
   One diagnostic deploy is needed to learn which layer-table entry composes
   beneath the stock background art.
2. **audio reactivity** — solved offline: the modpack already gets the entire
   song XWB in RAM at every dance-bank create (gameplay AND preview), already
   has a pure ADPCM→PCM decoder, and already has a content-domain per-frame
   music clock. An FFT-timeline service (chart_length-style worker) plus a few
   extra VS constant registers delivers Milkdrop-lite reactivity with **zero
   new hooks** for gameplay and previews. Menu-BGM reactivity is the one weak
   area (position-blind today); v1 should ship time-only animation there.

True Milkdrop (previous-frame feedback) is **plausible but experimental**: this
session's RE found the engine's offscreen render targets are registered as
*named textures in the same registry `asset_loader` already resolves from*
("offscreen1", "render_back" — §5.3), and the command list has a working
SetRenderTarget record (tag 0x17). That upgrade path exists but is v3 material
with real unknowns (segment state, game contention for the offscreen surfaces,
D3DMetal behavior).

Recommended architecture (§7): a **hybrid** — a black/minimal placeholder AFP
arc keeps the game's own background machinery happy and suppresses stock art,
while the DLL emits the shader quad at background depth keyed off the same
selection. Selection rides the existing WebUI BACKGROUND rows' dynamic
discovery, with DLL-side persistence for the shader slots (the native
Customize setter rejects out-of-range ids at card-in — §4.3).

## 2. Requirements decomposition

| # | Requirement | Verdict | Mechanism |
|---|---|---|---|
| R1 | Custom animated backgrounds during menus | **Proven-adjacent** | overlay_draw emission at background z (§6) |
| R2 | Custom animated backgrounds during gameplay | **Feasible** | same; gameplay background layer RE needed (§6.3) |
| R3 | Selectable by the user like stock backgrounds | **Proven** | webui_options BACKGROUND rows, dynamic discovery (§4) |
| R4 | Fully shader-driven (not AFP clip animation) | **Proven** | theme-shader pipeline (§5.1) |
| R5 | Audio-reactive: gameplay song | **Feasible, zero new hooks** | offline FFT + content-domain clock (§8.2) |
| R6 | Audio-reactive: song-select preview | **Feasible** | same bytes; wall-clock position estimate (§8.3) |
| R7 | Audio-reactive: menu BGM | **Weak** | position-blind without new mixer tap (§8.4) |
| R8 | Milkdrop-style feedback (prev-frame trails/warp) | **Experimental (v3)** | tag 0x17 + RT-as-texture lead confirmed (§9) |

## 3. What exists today (survey of the three pillars)

### 3.1 Background selection machinery (webui_options)

- The game stores background choices in the `ddr::player::Customize` object
  (RTTI-derived offset from PlayerWork; version-unstable, always derived —
  `src/core/signatures.rs:4289-4398`): `Customize+0x10` = result/special
  context, `Customize+0x14` = gameplay context, both naming assets
  `background_%04d` (`docs/player_customization_system_research.md:137-138`).
- The two BACKGROUND option rows (`src/mods/webui_options/discovery.rs:115-137`)
  discover their range **dynamically**: `discover_from_filesystem` merges
  `data/arc/custom/background/background_<id>.arc` with every
  `data_mods/<mod>/data/arc/custom/background/` (`discovery.rs:259-285`).
  Dropping new arcs into data_mods extends both rows with zero code changes.
- Value writes go **directly to the Customize field** (raw asset id,
  `mod.rs:407-443`), bypassing the native setter's accept-ceiling; scene-25
  entry seeds rows back from the fields (`mod.rs:359-399`).
- The game's backdrop machinery notices the applied value, releases the
  previous `background_%04d` package by name and loads/creates the new one
  (`docs/bm2d_background_preview_research.md:71-77`) — the reason the preview
  overlay uses private `bgprev_` alias arcs.
- Preview overlay (`bg_preview_overlay.rs`) already animates any discovered
  background id inside the options modal — custom slots preview for free if
  they are AFP arcs; shader slots need a small special case (§7.4).

### 3.2 Shader machinery (overlay_draw + shader_synthesis)

- **Authoring/compiling:** HLSL SM3 (`vs_3_0`/`ps_3_0`) in `shaders/src/themes/`,
  compiled by fxc 9.29 under the CrossOver bottle (`scripts/build_shaders.sh`),
  committed as `.d3dbc` blobs. fxc compile success is the per-shader
  feasibility gate.
- **Packaging/loading:** `shader_synthesis.rs` runs when the game opens
  `data/arc/shader.arc`, slices the stock VS/PS from the game's own GSPW,
  appends theme programs LAST to the boot-resident DEFAULT container
  (`gs_screencommand_default`, global derived by `derive_render_globals`),
  fingerprint-cached. Containers hold up to 255 programs (u8 counts,
  `docs/shader_replacement_research.md:41-77`). Program indices are published
  via `overlay_draw::publish_theme_programs` under a strict lockstep invariant
  (blob manifest order == synthesis order == published array order).
- **Emission:** `overlay_draw` encodes self-contained records (tags 0x03/0x04
  quads, 0x07 context, 0x0C scissor, 0x11 texture, 0x13 SetShader, 0x14
  SetVSConstantF — `encode.rs`, host-tested) and emits per frame through three
  surfaces: the identity-gated anchor (menu background), the aux anchor (SMX),
  and the topmost post-dispatcher append. The **layer-dispatcher detour is
  already installed as a pure passthrough** (`overlay_draw/mod.rs:18-39`) —
  its successful install is `emitter_ready()`.
- **Constants:** c48+ is the mod window (tag 0x14; `reg_off` relative to c48).
  Theme contract: c48 = `{time_s, rect_x, rect_y, -}`, c49 = `{rect_w, rect_h,
  p0, p1}`; the VS forwards everything the PS needs through interpolators —
  **there is NO pixel-shader-constant record** (`overlay_draw_research.md:109`).
  ps_3_0 has 10 interpolators; 4 used today → ~6 spare float4s.
- **Textures:** custom quads CAN bind textures — proven by the SMX overlay
  atlas (`smx_hardware/overlay.rs:362-391`) and mine_render. Handles come from
  `asset_loader` (FileManager load → resolve by FNV of the PNG stem).
- **Platform ceiling:** D3DMetal under CrossOver requires SHALLOW dynamic flow
  control (a dynamic loop inside a conditional inside a loop freezes the game
  in software rasterization — MANDELBULB was cut after two freezes;
  `.agents/planning/2026-08-25-shadertoy-themes/progress.md:44-64`). Working
  themes run 53–588 fxc static instructions filling a 1160×600 modal; a
  1280×720 fullscreen fill is only ~32 % more pixels.

### 3.3 Audio machinery (game_audio + song_rate + core/xact)

- One XACT engine instance mixes everything; song banks are strict 2-entry
  MS-ADPCM XWBs (`<code>` main + `<code>_s` preview). Slots {0,3,5} stream
  (menu BGM = slot 0, per-song dance bank = slot 5).
- **Whole-song bytes, in RAM, at the right moment:** the `wavebank_create`
  detour fires on every dance-bank create (gameplay AND preview, any rate incl.
  100 %) and can read the FileManager's resident XWB via
  `file_table_source(file_id)` (`song_rate/wavebank_hook.rs:472-477`);
  `prepare_binding` proves copying multi-MB there is fine (loading screen).
  Off-thread re-read from disk via host `std::fs` + LayeredFS precedence is the
  established alternative (`chart_length.rs:165-171`,
  `fast_bootup/identity.rs:54-79`).
- **Pure decode:** `core/xact/adpcm.rs` — `decode_interleaved` and
  `BlockCachePcm` (random-access block-decoded view; blocks are self-contained
  2.90 ms units) — directly reusable for FFT windows.
- **Per-frame clock (gameplay):** `song_reset::current_raw_music_count()`
  (GamePlayActor `+0x178`, i32 ms judge domain) gated by
  `first_anchored_frame()`; under SONG SPEED the count is **content-domain**,
  which indexes an FFT timeline of the *original* audio correctly at any rate
  (`song_rate/tick_domain.rs`). Audible-time alignment model already proven by
  assist_tick: `wall = content_to_wall(t − m0) − SOUND_OFFSET`
  (`assist_tick.rs:510-531`). Seeks/restarts re-sync via `song_reset`
  subscriptions (movie_sync consumes exactly this today).
- **Song identity at select:** `song_rate::selected_song()` seqlock (code
  digest + length) published on every slot-5 create; the wheel poll gives the
  code directly (`music_wheel_song_length` pattern).
- **No tap exists near the final mix** (no DirectSound/WASAPI hooks in-repo);
  spice2x's audio hook occupies that layer and is a known Wine crash source.

## 4. Selection & persistence integration

### 4.1 Presenting custom slots to the user

Two workable shapes; both reuse `custom_options`:

- **(a) Extended BACKGROUND rows (recommended).** Ship one placeholder AFP arc
  per shader background (e.g. `background_0900.arc`, `background_0901.arc`…)
  under `data_mods/<mod>/data/arc/custom/background/`. Discovery auto-extends
  both rows; the id range ≥ 900 (or any reserved band) is interpreted by the
  DLL as "shader slot N". The arc itself is a minimal black `bg_root` clip —
  see §7.1 for why shipping a real arc (not just a virtual row entry) is
  load-bearing.
- **(b) Separate enum rows** ("MENU SHADER BG" / "GAMEPLAY SHADER BG") that
  override the AFP background when non-OFF. Cleaner separation, but two extra
  rows, and the stock background still renders underneath unless also forced
  black — worse UX than (a).

### 4.2 What the reserved-id trick already gets right

- In-session apply works: the row write path pokes `Customize+0x10/+0x14`
  directly, bypassing the native setter's accept-ceiling
  (`webui_options/mod.rs:437-438`).
- The game's own loader serves the placeholder arc through LayeredFS exactly
  like stock (`bm2d_background_preview_research.md:41-46`), provided the inner
  IFS basename matches the arc name (FNV rule) and it exports `bg_root`
  (uniform across all 51 stock arcs, authored 1280×720).
- The options-modal preview comes for free for the AFP part; the shader part
  needs a custom preview treatment (§7.4).

### 4.3 The persistence catch (card-in round trip)

`mod_customize_background[_gameplay]` stores the asset id verbatim server-side,
but on the next card-in the game's own category-3 dispatch applies it through
the **vtable setter, which rejects ids above its per-build ceiling**
(reject-and-skip; `player_customization_system_research.md:148,184-186`). The
field then holds the default, and scene-25 seeding maps unknown → index 0
(`webui_options/mod.rs:385-389`). Options:

1. **DLL-side persistence for shader slots** (recommended): keep the shader
   selection in the DLL's JSON store (`save_json_key`, overlay_menu pattern) or
   as a `PersistMode::Full` mod option (wire `mod_shader_background`), and
   re-apply the Customize field after the native load (a card-in callback —
   the persistence service already has `register_card_in_callback`).
2. Patch the two setters' `CMP imm/JA` bounds — per-build imm, two sites,
   fragile; not justified when (1) exists.
3. Accept non-persistence — poor UX; rejected.

Note the stock rows are `SaveOnly` (no JSON cache, no network load) — shader
slots choosing path (1) deliberately diverge from that model, which is fine:
they are not native customize values.

## 5. New RE this session

### 5.1 The backdrop manager (20260721)

The game's background loader is a lazily-initialized registration in the
`sequence::(anonymous AC78A3CF)` namespace, built almost entirely from
`std::tr1::function` lambdas:

- **`FUN_18003d350`** — one-shot init (guard byte at `owner+0xC0`): registers
  the "background" custom-parts loader (base string `"background"`, **display
  priority 99** captured at registration) and two "character" loaders
  (`%s/%04d_usr` per player, `result_%dp` variants). Reached via functor
  `FUN_180043d10`.
- **Category descriptor table** at `0x18035f4d0..` (.rdata): per-category
  tr1::function vftables — dir builder (`FUN_1800438e0` → `"custom/background"`),
  name composer (`FUN_18003df00` → `"%s_%04d"`), create driver.
- **`FUN_18003dfa0`** — the create path (invoked through functor
  `FUN_180043110` with the customize id):
  1. compose `%s_%04d` (base + id),
  2. look up the BM2D package registry `DAT_1806f2d68` (begin/end pair;
     20260721 twin of the documented `DAT_1806f1d68` on 20260526), package at
     `entry+0x30`,
  3. claim a free slot in the **0x400-entry × 0x48 MovieClip pool at
     `DAT_1806fa600`** (vcall `+0x138` = slot-free probe),
  4. `FUN_18026ecb0(slot, pkg, "bg_root", 0)` — create the clip from the
     package's `bg_root` export (error string paths log `"bg_root"` on
     failure),
  5. wrap in a shared_ptr and store into **`owner+0x140`** (the live background
     clip slot; `FUN_180031c70` is the store-with-release — this is the release
     mechanism behind the documented "backdrop manager releases the previous
     background on change"),
  6. vcalls `+0xE0(value−1)` / `+0xE8(0)` on the stored clip (post-create
     configuration; exact semantics unconfirmed — likely frame/variant select).

Result-screen backgrounds use a sibling path with root export
`"result_bg_root"` (`FUN_18003e740`, string `0x18035f218`).

Implication for us: the game's own machinery is name-based and validation-free
past the setter — any discovered id whose arc exists loads fine; a missing arc
just never becomes ready. The placeholder-arc plan (§4.1a) rides this path
unchanged.

### 5.2 The screen-command-list render graph (20260616) — the 8 named lists

**`FUN_1801f5d10`** (called from render init `FUN_1801f1cf0`) creates the 8
global screen command lists with names and per-list `gs::Viewport` scenes:

| idx | Name | Dimensions | Notes |
|---|---|---|---|
| 0 | `FRONT` | 1280×720 | ordinary layer-table entries draw here (list_index 0/1) |
| 1 | `MIDDLE` | 1280×720 | |
| 2 | `BACK` | 1280×720 | also the placeholder index installed while an override list is active |
| 3 | `SYSTEM` | screen dims | **entry 7 (the DLL's widget layer) maps here** |
| 4 | `OFFSCREEN0` | 1280×720 | override entry 9 |
| 5 | `OFFSCREEN1` | 1280×1280 | override entry 10 |
| 6 | `DEBUG_DIALOG` | screen dims | |
| 7 | `RENDER_CAPTURE` | 1280×720 | screenshot/capture path |

Each gets its own render-target id (`FUN_180251410(&DAT_1802e0990, 3)` —
gd resource type 5), list arena `0x400000` cap (`FUN_18026a720`), and a debug
name `SCREENCOMMANDLIST %s`. The walker's tag-0x17 default
(`DAT_1806f226c`) is written here. List pointer array: `DAT_1806f0620..`
(20260616) = the 8-pointer private-list array `0x1806f1620` documented on
20260721 (`overlay_draw_research.md:236-237`) — same structure, per-build
address.

**`FUN_1801f01a0`** builds the compositing surfaces and — critically — the
**priority-ordered viewport graph** (attach via `FUN_1802662a0(parent, viewport,
prio)`; lower priority composes earlier):

```
0x65 OFFSCREEN1 pass → 0x66 RENDER (3D) → 0x67 AFTER RENDER 3D
   → 0x68 RENDER_2D → 0x69 DISPLAY → 0x6a PRESENT
```

### 5.3 Render targets ARE named textures (the feedback lead)

`FUN_1801f01a0` creates offscreen color/depth surfaces (`FUN_18024f610(w, h,
fmt)`), wraps them in texture views (`FUN_180249ba0(surface_id, flags)`), and
**registers the views in the global texture registry by name**
(`FUN_180202f00(view_id, name, 0)`):

- `"OFFSCREEN1"` — view of the 1280×1280 surface (fmt 0x15)
- `"render_back"` — view of a 1280×720 surface (fmt 0x16; which physical
  surface depends on the AA config `DAT_1806f050c`)

`FUN_180202f00` inserts into the map at `RM_singleton+0xB8` keyed by the FNV
hash from `FUN_180201b70` (lowercase, strip `'_'`) — **the exact registry and
hasher the modpack's `asset_loader` resolves through**
(`resource_manager_get_texture_data` walks the same `+0xB8` tree;
`src/core/signatures.rs:804-808`). Teardown (`FUN_1801f2170`) releases
`"offscreen1"`, `"render_color"`, `"render_back"` by name, confirming
liveness. (`"render_color"`/`"render_depth"`/`"display"` go into the separate
render-graph binder `DAT_1806f1f00`, not the texture registry.)

**Consequence:** a mod-emitted quad can, in principle, bind the engine's own
offscreen render output as its input texture via the existing
`asset_loader::resolve("render_back")` + tag 0x11 — and tag 0x17 can retarget
mid-list. This is the statically-confirmed foundation for feedback effects
(§9). Runtime contention and content-timing are the open unknowns.

## 6. z-placement: drawing UNDER the game

### 6.1 The mechanism already exists

The per-frame layer dispatcher (`FUN_18002af10` on 20260721; AOB + derivations
in `overlay_draw_research.md:206-212`) walks the 11-entry layer table once per
frame; the modpack's detour on it is installed and passthrough today. The
overlay-menu investigation established (cabinet-validated, 16.8k emissions):

- Pre-original appends into a walked entry's list land at the **head of that
  layer's segment** — drawn *below everything the layer itself records that
  frame* (`overlay_draw_research.md:274-282`). For the menu this was the bug;
  for a background it is the requirement.
- Entries 0–5 are ordinary slot layers (list_index 0/1 = FRONT/MIDDLE);
  entries 7–10 are override entries whose private lists are SYSTEM/MIDDLE/
  OFFSCREEN0/OFFSCREEN1; game content (`BM2DGroupWithPan` wrappers = the BM2D
  render groups) is boot-installed into entries 7–10's managers
  (`FUN_18002aa60`; `overlay_draw_research.md:226-240`).
- The **blind slot-table walk is NOT safe** (crashed in-engine); only the
  dispatcher-detour moment and `active_command_list()` are verified append
  surfaces (`overlay_draw_research.md:169-184`, `.agents/learnings/learnings.md:528-539`).

So the background emitter is: *dispatcher detour, pre-original, append the
shader block to the list of the entry that composes beneath the stock
background content* — same thread, same moment, no torn-list risk, once per
frame by construction.

### 6.2 The one unknown: which entry is "the bottom"

Composition order across entries/lists — and which entry's walk records the
song-select background (AFP group 0) vs gameplay's background — is unmapped.
The `docs/overlay_draw_research.md:179-184` follow-up ("real RE of the layer
walk/composition order") is exactly this. It is a **diagnostic-deploy
question, not a design risk**: a probe build logging, per scene, each walked
entry's identity, list index, arena growth, and first-record tags will map
entry → content in one attract+gameplay cycle (the survival-probe/tag-dump
technique from `overlay_draw_research.md:249-255` applies unchanged).

Expected shape (to be confirmed): song-select background = BM2D group 0 →
one of the override entries 7–10; emitting pre-original into that entry's
private list puts the quad under the background art and above nothing — the
frame floor. If the game background instead renders through an earlier
ordinary entry, the same emission strategy applies to that entry.

### 6.3 Gameplay specifics

- Gameplay backgrounds are started once by `SceneManageActor` and free-run
  (`docs/training_mode_research.md:455-459`). AFP-based and movie-based
  (DirectShow `.wmv`) backgrounds are distinct paths sharing the BgMovieActor
  readiness gate.
- For **movie-backed songs**, a shader background wants the movie *absent*:
  the `movie_policy` suppressor already implements exactly this ("no movie,
  static art" — the `fake_opened` shape). A per-song "shader background
  active" contributor to `MovieSuppressor` (alongside `SongRate` /
  `NonNativeOs`) is a few lines on existing machinery.
- The loading-screen interstitials (scenes 21/26/27) render full-screen art
  through the widget layer's walk — the identity-anchor lesson. The background
  emitter should simply not care: bottom-of-frame emission under loading art
  is invisible and harmless.
- Fill-rate note: gameplay adds a fullscreen shader fill under the entire
  HUD/lane stack. Budget shaders accordingly (§10); a per-context "gameplay
  uses the cheap variant" policy is worth considering.

### 6.4 Hiding the stock background

The placeholder-arc trick (§4.1a) makes the game render a black `bg_root`
clip — the stock machinery stays happy (package resident, layer live, scene
transitions natural) and nothing draws over our quad except intended UI.
This is strongly preferred over suppressing/hiding the game's background layer
(fighting the backdrop manager's release-on-change lifecycle — a known crash
class, `bm2d_background_preview_research.md:59-77`).

## 7. Recommended architecture

### 7.1 The hybrid (placeholder AFP + bottom-emitted shader quad)

```
user picks "SHADER: PLASMA" (background_0901 in the BACKGROUND row)
  ├─ webui_options writes 901 → Customize+0x10/+0x14 (existing path)
  ├─ game's backdrop manager loads background_0901.arc (black bg_root clip)
  │    └─ stock art suppressed naturally, transitions stay native
  └─ DLL: id ≥ 900 ⇒ shader_backgrounds mod activates program for that slot
       └─ dispatcher-detour pre-original append into the bottom entry's list:
            context(1280,720) → SetVSConstantF(c48..) → SetShader(default, prog)
            → fullscreen quad → SetShader(default, 0)
```

- Shader programs ship exactly like mod-menu themes: HLSL in
  `shaders/src/backgrounds/`, blobs appended to the DEFAULT container by
  `shader_synthesis` (indices published alongside the theme array — same
  lockstep invariant, same `progs >= idx+1` bind gate).
- Emission block byte-shape is `emit_background`'s
  (`overlay_draw/mod.rs:695-827`) minus the opacity/rounded-corner treatment;
  vertex alpha = 0xFF (it IS the background).
- Config/selection state: per-context (menu row +0x10 / gameplay row +0x14)
  atomics latched from the option rows; scene gating mirrors where stock
  backgrounds exist.

### 7.2 Why not pure AFP (Route A alone)

AFP packages carry 2D clip animation only — no shader hooks exist in the
BM2D layer path. Rejected for R4 by construction.

### 7.3 Why not "replace the background layer's SetShader" (pass-rewrite style)

player_perspective-style pass rewriting (flip the background layer's own draw
records to a custom program) would inherit the stock art's UVs/geometry, not
give a free-form procedural canvas, and adds per-record walking cost for no
benefit over head-of-list emission. Rejected.

### 7.4 Options-modal preview for shader slots

`bg_preview_overlay` previews AFP packages. For shader slots, either:
(a) accept the black placeholder preview + a labeled thumbnail PNG in the
preview box (static image via the existing template/chrome machinery), or
(b) emit the shader quad scissored to the preview rect while the row is
focused (the modal's own rect plumbing exists; tag 0x0C encoder exists).
(a) is v1; (b) is polish.

## 8. Audio reactivity

### 8.1 Architecture decision

Three candidates were assessed against the codebase:

- **A. Offline/precomputed FFT timeline** — decode the song once at bank
  create, precompute band energies over time, index per frame by the music
  clock. **All hard pieces exist** (bytes, decode, identity, worker pattern,
  clock, rate conversion, seek re-sync). **CHOSEN.**
- **B. Live final-mix tap** (DirectSound buffer / engine mixer detour) — only
  path to true "what's audible now" incl. menu BGM; all-new machinery, collides
  with spice2x's audio-hook layer (known Wine crash source), per-platform
  validation burden. **Deferred** (the only future justification is menu BGM).
- **C. Packet-read reconstruction** (io_callback tap) — sees buffering, not
  playback (~0.25–1.5 s lead, 1.3 s granularity); allocation-free detour
  context; needs the one-detour-rule dispatcher conversion. **Rejected as
  primary**; possible position-blind menu-BGM supplement.

### 8.2 Gameplay (excellent)

- **Trigger:** `wavebank_create` detour → song code + resident XWB bytes
  (`create_source`) → hand to an `audio_analysis` worker (chart_length-style:
  latest-wins queue, digest-stamped result cell, background thread).
- **Analysis:** `BlockCachePcm` windows → real FFT (or Goertzel banks) →
  N bands × T frames energy timeline (e.g. 16–24 log-spaced bands at 30–60 Hz
  frame rate; a 2-minute song ≈ 16 bands × 7200 × f32 ≈ 450 KB) + derived
  scalars (bass/mid/treble/beat envelope). Pure, host-testable.
- **Per-frame index:** `current_raw_music_count()` (content domain — correct
  at any SONG SPEED by construction) gated by `first_anchored_frame()`;
  optional audible-time compensation `− SOUND_OFFSET` (actor `+0x16C`) per the
  assist_tick model. Seeks/loops re-sync free via `song_reset` subscription.
- **Delivery:** extend the emission block's tag-0x14 upload — c50..c57 (≈32
  floats: 16–24 smoothed bands + beat/energy scalars), forwarded to the PS
  through the ~6 spare interpolators. Enough for Milkdrop-lite (pulsing
  geometry, band bars, beat-driven palette). This is a VS-side contract change
  only for background programs; theme programs are untouched.

### 8.3 Song-select preview (good)

- Same create detour fires for preview banks; analyze the `_s` entry (or the
  main entry's window). Identity from `selected_song()`/wheel poll.
- Position: no engine cue-position getter exists (`IXACT2Cue` layout provably
  deviates from public headers — `game_audio.rs:313-321`). Estimate: latch
  wall time when the preview loader's cue handle (`loader+0x10`) transitions
  from −1 (per-frame poll site already exists in `song_rate/preview.rs`), then
  integrate wall clock. ±1 frame + backend latency (~125–150 ms worst case
  under Wine) — fine for visualization.

### 8.4 Menu BGM (weak — accept for v1)

Slot-0 `bgm_menu` is a multi-entry streaming bank with no in-repo cue/entry
map and no position source. v1: time-only animation outside song-select/
gameplay. Future options: parse `bgm_menu.xwb`/XSB offline + a C-style
read-cursor hint, or Architecture B.

### 8.5 Stretch: data textures

If per-band constants prove too coarse:

- **Per-song spectrogram PNG** — the analysis worker writes a time×freq PNG to
  the cache dir, loaded via `asset_loader` (chrome_loader pattern, ~0.7 s),
  bound with tag 0x11, sampled by the PS with the time constant. No new
  engine RE; costs a texture-sampling background-shader variant.
- **Per-frame waveform texture** — the engine's dynamic-texture path
  (`FUN_1802488e0` create / `FUN_180248eb0` lock / `FUN_1802492e0` unlock —
  fully RE'd in `docs/chart_strip_hud_research.md:93-117`, no DLL wrapper yet)
  enables a true oscilloscope strip. Well-scoped new RE; v2+.

## 9. Feedback effects (true Milkdrop) — experimental track

Statically confirmed this session (§5.3): offscreen RTs exist, are switchable
mid-list (tag 0x17 → gd tag 0xD), and their texture views are resolvable
through the modpack's existing `asset_loader` by name. Two candidate shapes:

1. **`render_back` sampling (cheap, try first):** bind `"render_back"` as tex0
   on the background quad and sample last frame's composited content —
   screen-feedback warps/trails without any RT switching. Unknown: at
   background-draw time, does `render_back` hold the previous frame's final
   image or something mid-pipeline? One diagnostic build answers it (draw a
   quad textured with it and look).
2. **Private ping-pong via tag 0x17:** retarget to an offscreen RT, draw the
   feedback pass sampling the other buffer, retarget to default (0), composite.
   Unknowns: segment-start RT state (c13/PS c1 are set per segment —
   `custom_arrow_renderer_research.md:175`), game contention for
   OFFSCREEN0/1 (they are live parts of the viewport graph, §5.2), no
   mod-facing RT-creation wrapper yet (gd resource type 5 via
   `FUN_180251410`), and D3DMetal behavior under CrossOver.

Both are v3: the single-pass procedural library (plasma, tunnels, spectrum
bars, starfields — everything the theme pack already proves) does not need
them. Classify all feedback work as cabinet-gated experiments with fail-open
static fallback.

## 10. Performance & platform constraints (carry-over rules)

- SM3 only; fxc 9.29 golden path; compile success = feasibility gate.
- **Shallow dynamic flow control** on every background shader (D3DMetal
  freeze class); every new shader is its own cabinet deploy test — static
  analysis cannot predict `buildPipelineState` failures.
- Instruction budget: existing themes 53–588 static instructions are proven at
  near-fullscreen fill; treat ~600 as the review threshold for gameplay-context
  shaders, looser for menus.
- Time constant wraps at 3600 s — wrap-seamless frequencies or accept the
  hourly cut (`theme_common.hlsl:25-27`).
- Emission size: the background block is ~0x100 bytes/frame against 0x400000
  arenas — negligible. Audio constants add ~128 bytes.
- `progs >= idx+1` SetShader gate is mandatory (boundless handler);
  `MAX_PLAUSIBLE_PROGRAMS` sanity stays.
- All emission/anchor work on the render thread; analysis on background
  threads; no locks across `run_on_render_thread`.

## 11. Phased implementation strategy

| Phase | Deliverable | New RE | Risk |
|---|---|---|---|
| **0** | Layer-composition diagnostic build: per scene, map layer-table entries → content (which entry hosts song-select bg / gameplay bg); confirm pre-original append renders at frame bottom | dispatcher-detour probe only (existing technique) | low |
| **1** | MVP: 2–3 shader backgrounds at song select. Placeholder black arcs (reserved id band) + discovery ride-along; bottom emission block; blobs appended via shader_synthesis; DLL-side persistence + card-in re-apply | none beyond Phase 0 | low |
| **2** | Gameplay + results coverage: per-context activation (+0x10 vs +0x14 rows), movie-suppressor contributor for shader-bg songs, cheap-variant policy for gameplay | gameplay bg entry confirmation (Phase 0 data) | low-med |
| **3** | Audio reactivity A: `audio_analysis` service (worker + FFT timeline, host-tested), c50+ band upload, reactive shader variants; preview position latch | none (all existing taps) | med |
| **4** | Stretch data: spectrogram texture and/or dynamic waveform texture wrapper | dynamic-texture wrappers (documented primitives) | med |
| **5** | Experimental feedback: `render_back` sampling probe → ping-pong RT prototype | RT semantics at draw time; gd type-5 creation | high |

Kill switches at every layer (mod disable ⇒ zero emission; missing blobs ⇒
slots degrade to the black placeholder — which is a *valid stock-machinery
background*, the cleanest fail-open in the modpack).

## 12. Open questions

1. Which layer-table entry/list hosts the song-select background walk, and
   which hosts gameplay's? (Phase 0 probe.)
2. Are the walked lists' arenas reset before the dispatcher or inside each
   walk? (Known-open from `overlay_draw_research.md:221-224`; the anchor path
   sidestepped it, the background path should log it in Phase 0.)
3. `FUN_18003dfa0` post-create vcalls `+0xE0(value−1)`/`+0xE8(0)` — semantics?
   (Only matters if we ever animate the placeholder clip; low priority.)
4. `render_back` content at background-draw time (Phase 5 gate).
5. Menu-BGM entry/cue map — needed only if R7 is promoted.
6. Exact per-build ceilings of the two background setters — only relevant if
   persistence option §4.3(2) is ever chosen.

## 13. Key addresses (new findings)

| What | Build | Address |
|---|---|---|
| Backdrop create (`%s_%04d` → registry → `bg_root` create → owner`+0x140`) | 20260721 | `FUN_18003dfa0` |
| Custom-parts loader init (background prio 99, character loaders; guard `+0xC0`) | 20260721 | `FUN_18003d350` |
| MC-from-package create (pkg, `"bg_root"`, 0) | 20260721 | `FUN_18026ecb0` |
| BM2D package registry begin/end | 20260721 | `DAT_1806f2d68` |
| MovieClip pool (0x400 × 0x48) | 20260721 | `DAT_1806fa600` |
| Category descriptor/functor tables | 20260721 | `0x18035f4d0..0x18035f8xx` (.rdata) |
| Screen-command-list boot (8 named lists + per-list Scene/RT) | 20260616 | `FUN_1801f5d10` |
| Render-surface init + viewport graph + RT-as-texture registration | 20260616 | `FUN_1801f01a0` |
| Surface create `(w,h,fmt)` / texture-view wrap / view-register-by-name | 20260616 | `FUN_18024f610` / `FUN_180249ba0` / `FUN_180202f00` |
| Texture-name hasher (lowercase, strip `_`) | 20260616 | `FUN_180201b70` |
| gd RT resource create (type 5) | 20260616 | `FUN_180251410` |
| Walker tag-0x17 SetRenderTarget handler | 20260616 | `FUN_180269880` |
| Default RT id global | 20260616 | `DAT_1806f226c` |
| 8-list pointer array | 20260616 / 20260721 | `DAT_1806f0620` / `0x1806f1620` |
| Render init / teardown (releases `"offscreen1"`, `"render_back"`, `"render_color"`) | 20260616 | `FUN_1801f1cf0` / `FUN_1801f2170` |
