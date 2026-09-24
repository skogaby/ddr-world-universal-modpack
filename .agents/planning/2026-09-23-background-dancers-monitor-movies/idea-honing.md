# Decision register — movies on the stage monitors

Readiness Confirmed 2026-09-23 — the maintainer approved every decision as recommended (D1–D17); no decision is Open.

Ordered by blast radius. `*` = likely not yet considered.

| ID | Decision | Why it matters | Recommendation | Status |
|---|---|---|---|---|
| D1 | Values, labels and config keys of the Background Movies row | user-visible + persisted config schema | 5 values in this display order: OFF / THUMBNAIL / **STAGE SCREENS** / FULLSCREEN (NO STAGE) / **MOVIE ONLY (NO DANCERS)**; config keys `stage_screens`, `movie_only`; existing row ints 0/1/2 kept, new 3/4 | Accepted |
| D2 | Default value | first impression for every operator | keep THUMBNAIL until STAGE SCREENS is cabinet-proven, then revisit | Accepted |
| D3 | MOVIE ONLY semantics | user-visible; decides what "has a movie" means | player's VIDEO SIZE untouched; scene still loads; hidden ENTIRELY (dancers, parts, shadows, stage, outlines) only while a movie is actually being drawn (the existing probe: MovieActor opening/opened ∧ real build ∧ not suppressed); 2D background left to the game | Accepted |
| D4 | STAGE SCREENS semantics | user-visible; interacts with VIDEO SIZE | stage has screens ⇒ movie routed to the screens, VIDEO SIZE written as FULLSCREEN for the song (0/2 → 1), full scene + stage cameras; stage without screens ⇒ exactly THUMBNAIL; VIDEO SIZE OFF stays off (black screens) | Accepted |
| D5 | How a stage "has screens"; custom-stage contract | covers stock + custom + LayeredFS overrides | computed once at mod enable: the stage arc's header lists a member named `offscreen1.dds`; custom stages opt in by naming the screen image `offscreen1` in Blender | Accepted |
| D6 | Routing mechanism | code patch in the game | the 1-byte imm `09 → 0A` in the MovieActor layer select (new optional signature), written at window entry only for a routed song, restored at window exit and at disable | Accepted |
| D7 | Framing on the screens | what the player sees | A3's exact contain fit: origin (0, 0) / size (1280, 1280) written into the MovieActor fit fields (new optional AOB) while its step ≤ 2, per MovieActor instance; no per-stage "cover" | Accepted |
| D8 | Unlit screens: scope of the restyle/outline exemption | Lighting Style / outlines | ALWAYS (every Background Movies mode, every style): a material whose texture is `offscreen1` keeps its stock shader and gets no hull records | Accepted |
| D9 | Songs without a movie on screen stages | rotation | screens stay black; screen stages stay in the random rotation | Accepted |
| D10 | Griffin TV re-export | POC content | TV image renamed `offscreen1`; UVs = the 16:9 band of the square (u 0…1 unmirrored as seen by the dancer, v 0.21875…0.78125); shader stays `mdl_bg_constant_vc`; placeholder pixels tiny; saved as a new `living_room_game_v3.blend` (v2 kept); stale `lr_screen.dds` removed from `data_mods` | Accepted |
| D11 | Blender add-on support | future custom stages | document the `offscreen1` convention in the add-on README; exporter writes a tiny black placeholder for the `offscreen1` stem regardless of the Blender image | Accepted |
| D12 | Degradation | fail-open rule | STAGE SCREENS needs movie-size + probe + both new AOBs, MOVIE ONLY needs the probe; missing ⇒ THUMBNAIL + one WARN per boot | Accepted |
| D13 | Diagnostics | cabinet triage | one INFO per song (route, stage has screens, sizes), one INFO per MovieActor framed, one-shot INFO of entry 10's walk-gate bytes the first time a song is routed | Accepted |
| D14 | Options-menu stage previews | consistency | unchanged — screens show black there (no movie at song select) | Accepted |
| D15 | Where the fit writer runs | correctness | its own per-frame driver, independent of the scene build (`drive_live` returns early until something is built — the fit must be written before the movie's step 2→3) | Accepted |
| D16 | Documentation | repo convention | update `docs/background_dancers_research.md` §8, README (Background Movies paragraph + config table), AGENTS.md (Background Dancers row + config entry), add-on README | Accepted |
| D17 | Validation | repo convention | host tests for every pure addition in `scripts/validate_background_dancers.sh`; `./scripts/validate_signatures.sh` all green with the two new AOBs; cabinet checks per `docs/background_dancers_research.md` §8.7 | Accepted |

## Details

**D1 — values and labels.** A new value between THUMBNAIL and FULLSCREEN reads as "a bit more movie"; MOVIE
ONLY at the end is "all movie". Labels stay within the 24-byte limit the host test enforces. Rejected:
reusing OFF's value for MOVIE ONLY (OFF means "no movie"). Row ints are stable so a cached `WINDOW_MOVIE_MODE`
and the persisted keys never change meaning.

**D2 — default.** STAGE SCREENS behaves like THUMBNAIL everywhere except the 10 stock screen stages (and
Griffin House), so it is a natural future default, but it is the only mode that patches game code.

**D3 — MOVIE ONLY.** *Not considered:* with VIDEO SIZE OFF the player sees no movie, so the dancers stay (the
probe finds no MovieActor on 20260224+). A song whose movie is suppressed or faked (song rate without SYNC
BACKGROUND VIDEO, the non-native "suppress" mode) also keeps its dancers — there is nothing else to show.
Rejected: deciding from the music DB at window entry and skipping the load — it would show a blank 2D
background whenever the movie then fails to draw, and saves only the arc load.

**D4 — STAGE SCREENS.** *Not considered:* once routed there is no fallback — a movie that then fails or is
faked draws nothing, which is exactly what it would have drawn in the thumbnail too, so no gating is needed.
The game disables the 2D BackgroundFrame for a fullscreen-size movie; the mod hides it anyway. VIDEO SIZE OFF
stays off because every other mode honours it.

**D5 — detection.** The placeholder `offscreen1.dds` is in exactly the 10 stock screen stages (§8.2 of the RE
doc) and the add-on always writes one for an image named `offscreen1`. One 64 KiB header read per distinct
stage at enable (≈ 27 reads, stock + custom through `arc_set` mounts / LayeredFS). Rejected: a static key list
(misses custom stages), parsing `.model` texture tables (the parse thread starts after the route must already
be decided).

**D6 — mechanism.** Same thread as the reader (MovieActor::onInitialize, DPS step 2, game thread), checked
before/after (`09` ↔ `0A`). Rejected: swapping layer-table entries 9 and 10 (data write into a live table
the dispatcher walks every frame); relinking the movie's node after registration (§8.6 of the RE doc).

**D7 — framing.** 16:9 movies show small bars on monitor00's 4:3 screen, 4:3 movies bars on the 16:9-ish
monitor01 screens — A3's own look. A per-stage cover is ill-defined anyway (monitor02/03 have two different
bands on one stage).

**D8 — exemption.** Screens show black outside STAGE SCREENS; shading a black surface is invisible, but a
restyled screen would shade the movie in STAGE SCREENS, and one rule is simpler. Key: the resource
texture-table entry hash == `fnv1_name_hash("offscreen1")` for any masked slot of the material. Additive /
alpha screen meshes are already exempt (blend group).

**D10 — Griffin TV.** Panel aspect 1.72. Mapping exactly the 16:9 band: a 16:9 movie fills the TV (a 3 %
horizontal squeeze), a 4:3 movie is cropped 12.5 % top and bottom but fills it; mapping the panel's own
aspect instead leaves 1.7 % bars on 16:9 movies. The Blender material keeps an image node so the stage still
previews in Blender.

**D11 — add-on.** The DDS for `offscreen1` is never bound in game (the boot registration of the name wins);
writing the Blender image would ship dead pixels (2.7 MB for the current TV image).
