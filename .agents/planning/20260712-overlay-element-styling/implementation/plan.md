# Implementation Plan — Overlay Element Styling

Design: `../design/detailed-design.md` (authoritative for all mechanism
details; steps below reference its sections). RE facts:
`docs/gameplay_overlay_elements_research.md`.

Project validation reality: no unit tests — each step's "Validation" is the
observable proof this repo uses (cargo check, boot/gameplay log lines, cabinet
behavior). Every step ends deployable and observable. Maintain
`../progress.md` after each step (per AGENTS.md PDD convention).

## Checklist

- [x] Step 1: Signatures + color-twin resolver
- [x] Step 2: `bm2d_api` raw-id setters
- [x] Step 3: Mod skeleton + option rows (values settable + persisted)
- [x] Step 4: Capture detour + clip registry
- [x] Step 5: Side binding + scale one-shots (first visual)
- [x] Step 6: Opacity — one-shots + float compose detour
- [x] Step 7: Int-variant compose + hardening + degradation drills
- [x] Step 8: Docs, regression sweep, close-out

---

## Step 1: Signatures + color-twin resolver

**Objective**: All four hook-target addresses resolve at boot on the cabinet's
build, with the color twins correctly disambiguated.

**Guidance** (design §4.1; RE doc §6): add `SignatureDefinition`s for
`cmovieclip_create` and `cmovieclip_set_position` (unique patterns) to
`SIGNATURES`. Add the two color-twin patterns via a custom resolver fn (model
on `find_judge_notes`): `scan_all` each pattern, require exactly 2 matches,
decode each match's `CALL [RIP+disp]` IAT slot (`decode_rip_relative`; float
form disp at match+0x21, int form at match+0x30), read the loader-patched
pointer, compare against `GetProcAddress("libafp-win64.dll",
"afp_layer_set_color")` / `"afp_layer_set_acolor"`. Publish
`cmovieclip_set_color_float` / `cmovieclip_set_color_int` (ord-49 members);
any ambiguity → unresolved (log why).

**Validation**: `cargo check` clean. Deploy; boot log shows the four resolved
addresses plus a disambiguation line (`set_color=+0x…, acolor sibling=+0x…`).
Cross-check the logged offsets against the RE doc §8 table for the cabinet's
build.

**Integration**: pure `core/signatures.rs` addition; nothing consumes the
names yet.

**Demo**: boot log lines proving resolution on real hardware.

---

## Step 2: `bm2d_api` raw-id setters

**Objective**: Non-owning scale/color primitives for game-owned layers.

**Guidance** (design §4.2): resolve `afp_layer_set_color` alongside the
existing named exports — **non-fatal for bm2d_api** (a miss must not disable
the AFP-layer wrapper set; use the `resolve_opt!`/`Option<fn>` pattern like
`mc_get_param`). Add `layer_set_scale_raw(id, sx, sy)`,
`layer_set_color_raw(id, r, g, b, a)`, `layer_color_available()`. Do NOT wrap
these ids in `AfpLayer` (destroy-on-drop is an ownership bug here — document
on the fns).

**Validation**: `cargo check` clean. Deploy; boot log shows
`afp_layer_set_color` resolved. Existing bg-preview feature still initializes
(no regression in `afp_layers_available()`).

**Integration**: extends the service the mod consumes in Steps 5–6.

**Demo**: boot log; bg previews still animate (service untouched
functionally).

---

## Step 3: Mod skeleton + option rows

**Objective**: The mod exists, is toggleable, and both players can set and
persist OVERLAY SCALE / OVERLAY OPACITY — before any hooks exist.

**Guidance** (design §4.7, §4.8, §5): create
`src/mods/overlay_element_styling/mod.rs`; register in `src/mods/mod.rs` +
`lib.rs`. `init()` checks the Step-1/2 load-bearing set (signatures +
`layer_color_available` + `afp_layer_set_matrix` availability) and
self-disables with a clear log if missing. `enable()` registers the two
`Scalar` rows (`overlay_scale` 25–150/5/25 default 100; `overlay_opacity`
0–100/5/25 default 100; `PersistMode::Full`, identity transforms);
`on_change` mirrors into the per-side atomics. `disable()` unregisters rows
and resets atomics. No detours yet.

**Validation**: `cargo check`. On cabinet: rows render on the Mods tab for
both players with correct ranges/steps/defaults; values survive card-out →
card-in (network) and a server-less reboot (`custom_options.p1/p2` JSON
entries appear); mod toggles cleanly from the DLL overlay menu.

**Integration**: consumes Step 1/2 availability checks; provides the value
atomics Steps 5–6 read.

**Demo**: set P1 scale 50 / opacity 25 on the options screen; card out/in;
values return. (No gameplay effect yet — expected.)

---

## Step 4: Capture detour + clip registry

**Objective**: Every scoped element wrapper is identified per song, with
correct counts, and the registry lifecycle (eviction/clearing) is sound.

**Guidance** (design §4.3, §5.1, §6): create `capture.rs` with the fixed
64-slot registry and the Create detour (`install_enabled`; `catch_unwind`;
original-first; defensive name read; exact-before-prefix matching per design
§5.3; slot-reuse eviction; overflow warn). Register the scene callback that
clears the registry on leaving GAMEPLAY. Per-song debug log of capture counts
by kind. Enable gating: Create-detour install failure → mod refuses enable
(rows not registered).

**Validation**: `cargo check`. Cabinet: play single, double, and versus songs;
logs show expected counts (3 combo, 1 judge, 7/15 freeze, 0–1 fast_slow, 0–1
pacemaker — per side in versus), no captures of `dance_effect`, registry
clears at song end, second song re-captures cleanly.

**Integration**: builds on Step 3's mod lifecycle; provides the tracked set
Steps 5–7 act on.

**Demo**: gameplay log showing named captures per song and clean per-song
reset.

---

## Step 5: Side binding + scale one-shots (first visual)

**Objective**: OVERLAY SCALE visibly works, per player, in all play modes.

**Guidance** (design §4.4, §4.5): add the SetPosition detour (non-fatal
install) binding side at first position write — active-side path when one
side is active (reuse the existing player-context accessors per design §4.4),
x-threshold (`X_SPLIT = 640`, instrumented) for versus. At bind: read
`custom_options::get_value(side, …)`, apply `layer_set_scale_raw` (skip at
100), mark applied. Fallback path: if the SetPosition detour is down and one
side is active, bind+apply at Create. Debug log every bind
(`kind, x, side, scale, op`).

**Validation**: `cargo check`. Cabinet: P1 scale 50 → P1's combo, judgement,
freeze O.K./N.G., FAST/SLOW, pacemaker all shrink about their centers; P2
stock. Scale persists across the full song (sole-matrix-writer invariant).
150 % renders. Versus bind logs confirm/correct `X_SPLIT`; double play
attributes to the active side. Receptor flashes unaffected.

**Integration**: consumes Step 4's registry + Step 3's values + Step 2's
`layer_set_scale_raw`.

**Demo**: side-by-side versus play with P1 at 50 % scale, P2 at 150 %.

---

## Step 6: Opacity — one-shots + float compose detour

**Objective**: OVERLAY OPACITY works with correct game-semantics composition.

**Guidance** (design §4.5, §4.6): create `color_hook.rs` with the +0x90 float
compose detour (**alpha is the first float arg**; multiply for tracked clips,
including tracked-but-unbound combo writes per design §4.6; forward untracked
unchanged). Install is load-bearing → failure refuses enable. Extend Step 5's
bind-time application with the per-kind color one-shots: judge/freeze/
fast-slow + pacemaker get `layer_set_color_raw(1,1,1,op)`; **combo gets
compose-only** (no one-shot — would un-hide a 0-combo counter).

**Validation**: `cargo check`. Cabinet, per design §7.5: opacity 0 → all five
groups invisible, gameplay unaffected; 50 → combo appears only at ≥4 combo at
half alpha (gating preserved), judgement pop fades proportionally, pacemaker
negative-delta renders at 0.5×op; per-player isolation in versus; opacity
100 = pixel-identical to stock.

**Integration**: completes the core feature end-to-end (capture → bind →
scale + opacity), all prior steps wired.

**Demo**: P1 opacity 0 (clean playfield), P2 at 50 — one song.

---

## Step 7: Int-variant compose + hardening + degradation drills

**Objective**: Close the +0xB0 coverage hole and prove the failure modes.

**Guidance** (design §4.6, §6): add the +0xB0 int-percent compose detour
(non-fatal; integer alpha math; one-shot debug log on first tracked hit).
Harden: layer-id revalidation at bind, registry-overflow behavior, name-read
bounds — audit against design §6 line by line. Degradation drills: build-time
or config-driven forced skip of (a) SetPosition signature → versus stock +
single/double still styled via the Create-time fallback; (b) +0xB0 → no
visible opacity resets; (c) color-twin ambiguity → mod self-disables, no rows,
clean log.

**Validation**: `cargo check`. Cabinet: normal play logs whether +0xB0 ever
fires on tracked clips (records the §C.2 open question's answer); each drill
behaves per Q9's tiers with clear log lines.

**Integration**: completes the hook set; no new consumer-facing behavior.

**Demo**: drill logs + a normal song showing unchanged behavior with the full
hook set live.

---

## Step 8: Docs, regression sweep, close-out

**Objective**: Ship-ready state — documented, regression-clean, resumable.

**Guidance**: README mod-table row + a short config note (options are
per-player rows; `mods` toggle). Update `.agents/summary/components.md`
(mods table) if in refresh scope, RE doc gotchas with any cabinet-learned
corrections (final `X_SPLIT`, +0xB0 verdict), and idea-honing deferred items.
Full regression sweep per design §7.8: PUS pacemaker-swap coexistence (both
active in one song), autoplay + this mod, bg-previews, quick-restart (registry
clears on restart redirect), 2-player persistence round-trip. Finalize
`progress.md` (status DONE + close-out notes).

**Validation**: sweep passes on cabinet; `cargo check` and a release
`./build.sh` clean.

**Integration**: n/a — closure.

**Demo**: one full mixed-mode session (single → versus → double) with styling
active, everything else stock-correct.
