# DDR World Full Decompilation & Rust Port — Feasibility Assessment

**Date:** 2026-10-10
**Status:** Feasibility study only — no work started. This document assesses whether a full decompilation, analysis, and Rust reimplementation of every code module in DanceDanceRevolution World is achievable, and what the high-level strategy would be.

**Inspiration / prior art:**

- [SK8-ENGINE/skate-3-rust-engine](https://github.com/SK8-ENGINE/skate-3-rust-engine) — Rust + Bevy rewrite of Skate 3, built on 2+ years of RE research by "dumbad" (tools, formats, animation data), then a recompilation/renderer effort, then the Rust rewrite. Assets not included; users point it at their own Xbox 360 ISO. Gameplay parity still in progress after 449 commits by multiple contributors.
- [LibertyFlux](https://github.com/monstercameron/LibertyFlux) ([site](https://monstercameron.github.io/LibertyFlux/)) — GTA IV's engine rewritten in Rust **one function at a time by AI agents**, with a machine check — not an agent's opinion — deciding when each function is done. Its development model (measured function inventory → easiest-first work queue → per-agent rewrites → side-by-side checker → per-function swap into the running game → standalone build → 64-bit lift) is the primary methodology reference for this effort, and is analyzed in §5.1.
- openOMSI — full OMSI 2 rewrite in Rust claiming 100% mod/map compatibility.
- Classic precedents (SM64, OoT decomps) establish that community decompilation of commercial games is technically achievable, but those were byte-matching C reconstructions of older, simpler binaries with custom tooling and multi-year multi-person effort.

---

## 1. Verdict up front

**Technically feasible — yes, with high confidence.** The binary is a single-generation MSVC x64 C++ target with intact RTTI, a cross-build corpus of ~11 distinct 64-bit `gamemdx` builds to diff against, and — critically — 7 months of prior RE in this repo that has already solved the hardest 20%: every shipped asset format, the timing/judge core, the scene framework, and the audio pipeline.

**Scope is multi-year.** `gamemdx.dll` alone is 18,613 functions across ~3.0 MB of `.text`, plus six sibling Konami DLLs that must be reimplemented or replaced. A realistic solo effort with heavy AI-agent assistance is ~12–18 months to a *bootable core-loop milestone* (attract → song select → gameplay → results against a user-provided asset install), and 2.5–4+ years to full module-by-module parity, if pursued to completion.

**"Open source" is the constraint, not the code.** A code-only Rust port in the SK8-ENGINE / LibertyFlux posture (no assets, no trademark use, user supplies data) sits in the same legal gray zone those projects occupy — viable to develop, genuinely risky to publish. DDR World differs from both inspirations in ways that materially raise the risk profile: it is a **live, actively-updated, online arcade service** owned by a rights-holder (Konami) that actively monetizes it, its 30 GB asset base is **licensed music/video content** that almost no end user legitimately owns, and its online half (e-amusement) is the revenue stream a port most directly threatens. Section 8 covers this honestly; it is the part of the plan that most deserves a lawyer's eyes before anything public happens.

**Strategically, "full decompilation" should mean behavioral reimplementation, not byte-matched reconstruction.** Byte-matching MSVC x64 C++ buys nothing for a Rust port and multiplies effort. The recommended strategy is the LibertyFlux development model adapted to this repo: a machine-checked, agent-driven, function-by-function pipeline (§5.1–5.4) whose verification is two-tier — a side-by-side checker for pure functions, plus an integration gate that LibertyFlux lacks: trace-diff verification against the original game using this repo's existing hook DLL as an in-vivo instrument. That verification harness is an asset neither prior project had, and it is the single strongest argument that this specific game, in this specific repo, is more feasible than the average "rewrite it in Rust" idea.

---

## 2. What "the entire game" actually consists of

### 2.1 Code modules (`$DDR_WORLD_INSTALL/contents/modules` + `com/`)

| Module | Size (approx.) | Role | Port disposition |
|---|---|---|---|
| `gamemdx.dll` | 5.3 MB file / 19.7 MB image / **18,613 functions** | The game: framework, sequences (58 scenes), actors, UI, judge, scoring, assets, DShow glue | Full reimplementation — the project proper |
| `libafp-win64.dll` | 2.6 MB | AFP ("Animation Flash Player") + BM2D MovieClip runtime — the entire menu/HUD presentation layer | Largest sibling lift. Format already decoded in-crate (`core/afp.rs`, AP2 editor); the *runtime* (tag interpreter, bytecode VM, object pool) is not |
| `libavs-win64(-ea3).dll` | 1.6 + 0.8 MB | AVS filesystem, property trees, `ea3_boot`, persistence wire | Reimplement the ~30 exported entrypoints this game actually uses (already mapped by the LayeredFS service) |
| `arkmdxbio2/p3/p4.dll` | 12.2 MB (identical size — same driver per cabinet profile) | Cabinet I/O: pads, lamps, 10-key, card readers, entry-flow state | Reimplement surface is small (dozens of `arkMDX*` exports); bulk of the binary is I/O board firmware/data not needed |
| `ess.dll` | 0.5 MB | e-amusement save/load senders, test-mode settings | Reimplement locally first; online side deferred (see §8) |
| `libacio/libacio2.dll` | ~0.35 MB each | Low-level I/O card access | Thin; reimplement or stub |
| `xactengine2_10.dll` | (MS, in `com/`) | XACT2 audio engine | **Already effectively replaced in pure Rust in this repo** — MS-ADPCM decode, WSOLA stretch, resampler, XWB, cue/bank math (`core/xact/`) |
| `d3dx9_43.dll`, `d3d9`, DShow | (MS/system) | Rendering, math helpers, video | Replaced wholesale by the port's own renderer; not a decompilation target |
| `mfplat.dll` fix | — | Wine-only VC-1 fix | Becomes irrelevant (native video decode in the port) |

### 2.2 Assets (`data/`, `arkdata/`, `prop/`)

- `data/` ≈ 30 GB: 191 ARC archives (UI, jackets, fonts, mapsets, models, 2D/3D sets), `mdb_apx/` with **1,604 SSQ charts** and **396 WMV background movies**, `sound/` XACT banks.
- `arkdata/` ≈ 5 MB (testmode, xml, ea3license, qc, sound), `prop/` = XML configs (eamuse, ea3-ident, avs, ark, ess).
- **Every one of these formats has a working codec already in this crate** (`core/arc.rs`, `ifs.rs`, `afp.rs`, `ap2/`, `ssq/`, `xact/`, `anm/` for KTMDL/B2IT/ANM/CAMANM, kbf fonts, AVSLZ, kbin) or in the community corpus (bemaniutils' ~11k-line AFP parser). The asset-encoding problem that consumed years in the Skate 3 project is **already solved here**. The remaining asset work is tooling polish (converters to modern textures/audio), not reverse engineering.

### 2.3 Ghidra state

- Project holds 14 `gamemdx` variants (11 distinct 64-bit builds 2024-04→2026-09, two x86 builds, modified variants) plus siblings and 30+ other DDR-era binaries.
- Current build (`gamemdx_20260915`): 18,613 functions, 116,845 symbols, ~8,100 functions already carrying recovered names (RTTI + demangler recovery), ~10,200 auto-named `FUN_*`. Sections: `.text` 0x180001000–0x1802d71ff (~2.96 MB), `.rdata` ~1.5 MB, `.data` ~19 MB image.
- Class inventory recovered from RTTI is dense and well-structured: `framework::Application`, `me::fw::hook::*`, 58 `sequence::*` classes (one per scene), actor families (GamePlayActor, ArrowRenderer, GaugeActor, ComboActor, …), the DShow player classes, GS/GPU resource wrappers. This is a *modern RTTI-rich C++ codebase*, the best-case for decomp tooling — a very different situation from the stripped binaries of older console games.

### 2.4 Prior RE in this repo (the 7-month head start)

- **93 research documents under `docs/`** covering: scene manager & 58-scene dispatch, judge/gauge/scoring math, timing windows, SSQ format (byte-exact, validated against all 1,523→1,604 charts), AFP system & BM2D pool, widget registration, option-row framework, input polling, event flags, folder/filter systems, audio clock & XACT streaming, song-rate engine, stage records/persistence wire model, musicdb, DShow pipeline, plus feasibility studies proving the methodology.
- **217 AOB signature definitions** in `src/core/signatures.rs`, swept offline against every supported build — a continuously-refreshed map of stable landmarks.
- Working in-process automation: `scripts/game_nav/` (CrossOver cabinet driven over SpiceAPI), `tools/bot_sim` (offline judge simulator), judgement CSV export — i.e., **replayable behavioral test drivers already exist**.

---

## 3. Why this is more tractable than it sounds

1. **A rhythm game is a small game, systems-wise.** No open world, no physics simulation, no streaming terrain, no vehicle handling, no NPC AI. The core gameplay (judge, gauge, arrows, timing) is a few hundred functions — and it is *the* best-documented area of the game in this repo already.
2. **The generic-engine share is reusable community knowledge.** The `me::fw` framework, AVS, AFP, ARC/IFS, and XACT glue are shared across the bemani ecosystem; bemaniutils/spicetools knowledge transfers directly, and this repo's own service layer has already mapped the used surface of each.
3. **Cross-build diffing is available and cheap.** Eleven builds spanning ~2.5 years separate stable core code from build-churn (the repo already relies on this for signature portability). Function-level diffing across builds is the classic accelerator used by every successful decomp project.
4. **The verification harness problem is already solved — uniquely.** SK8-ENGINE and LibertyFlux had to eyeball parity against gameplay video. Here, the existing hook DLL can run *inside* the original process and dump per-frame behavioral traces (judge decisions per input event, scene transitions, gauge deltas, widget states, audio clocks) that the Rust port must reproduce. The port and the original can be driven by identical input scripts (bot_sim / game_nav) and diffed automatically. This converts "does it feel the same" into a regression-testable property.
5. **The agent-tooling loop is already operational.** Ghidra MCP (171 tools) + this repo's steering docs + the AOB infrastructure + the cabinet deploy loop is precisely the workflow that produced the recent wave of RE-driven Rust ports.

---

## 4. What makes it hard — the honest list

| Difficulty | Item | Notes |
|---|---|---|
| Highest | **AFP/BM2D runtime reimplementation** | The menu/HUD is a Flash-like VM: tags, bytecode, masks, filters, object pools, fonts. Format decode is done; the *interpreter* is the single biggest reimplementation unit (~`libafp` 2.6 MB). Must be near-exact for UI parity. Affects nearly every scene. |
| High | **Rendering parity** | The `gs` layer over D3D9 with custom HLSL (`gs_screencommand_*`, `mdl_*`), command lists, viewports, 3D model pipeline (KTMDL), camera clips. Porting to wgpu (or D3D12/Vulkan) is a full renderer project; pixel-approximate is achievable, pixel-exact is not worth chasing. |
| High | **Timing/audio-clock parity** | Gameplay feel lives here (DAC-authority clock research exists in `docs/audio_clock_research.md`). The pure XACT2 layer in-crate already implements the rate/WSOLA math; the risk is scheduling semantics, not codecs. |
| Medium | **Persistence & unlock systems** | GameWork/PlayerWork layouts, stage records, event flags, the `mod_*` wire model — all documented; long tail of content-unlock tables to map. |
| Medium | **Service/online layer** | e-amusement protocol, ESS senders, licenses. Technically mappable; legally the hottest surface (§8). Defer. |
| Medium | **Long tail of 58 scenes × options** | Each sequence is individually simple (RTTI-named, template-driven), but 58 scenes of option sheets, filters, customizers, and special modes is breadth work — the volume that makes "every module" a multi-year phrase. |
| Low | **I/O drivers** | `arkMDX*` surface is small; PC input/SMX already supported in-repo. |

---

## 5. Strategy options

### 5.1 Prior art: the LibertyFlux development model, and why it fits here

The intended operating model is agent-driven, like [LibertyFlux](https://github.com/monstercameron/LibertyFlux) (GTA IV, 2,364 commits, phase 3). Its pipeline is worth stating in full, because it is the template being adopted:

1. **Measure.** Ghidra analyzes the original executable; functions are counted, library/runtime code is set aside, class names recovered from RTTI. → *Here: already done (§2.3) — the Ghidra corpus, 217 signatures, and 93 docs are the measurement.*
2. **Queue.** A script orders functions easiest-first and hands them out; agents never choose their own work. → *Here: a queue generator over the Ghidra export, prioritized by the milestone phases of §6, and seeded with the functions already documented under `docs/`.*
3. **Rewrite.** One agent per function, studying behavior with nearest already-finished functions as reference, writing Rust under strict repo rules (no decompiler output committed; layouts/names/behavior only — the "committed vs never committed" table LibertyFlux enforces via a publication check script).
4. **Compare.** The original function and the Rust version run side by side in a test process from identical inputs and starting memory; return value, memory writes, stack adjustment, and outgoing calls must agree over 1,000+ generated inputs. A **deliberately wrong mutation must fail the same comparison** (mutation testing of the checker itself). Accepted rewrites move `unverified/ → verified/` under a ledger; static lints catch what the checker can't see; an issue log tracks every known narrowing.
5. **Swap in.** Verified functions are assembled into an injectable library with a **per-function on/off switch**, loaded into the *original game*; misbehavior is bisected by flipping switches in halves.
6. **Cut loose.** Only when every function has a replacement does the Rust code build standalone, then lift to 64-bit/new renderer/platform targets. "Nothing is modernized until the rewritten game runs on its own, because mixing the two goals stalls both."

Why this model transfers to DDR World *better* than it fits GTA IV:

| LibertyFlux need | DDR World answer | Advantage |
|---|---|---|
| A loader that swaps functions into the live game | **Already exists** — this repo's hook DLL is an injection-and-detour platform with dispatcher services, checked patches, and a proven deploy loop | Phase 1 "harness" is weeks, not months |
| Side-by-side comparison infrastructure | The cabinet (CrossOver bottle + game_nav automation over SpiceAPI) plus the pure-logic harness model (`scripts/validate_*.sh` mounting dependency-free files) | Checker harness + integration environment both exist |
| Deterministic, checker-friendly functions to rewrite first | Rhythm-game logic is unusually deterministic: judge windows, gauge math, grade tables, tempo/tick conversion (`core/ssq/`'s engine-exact tempo converter is a reference), SSQ parsing | The "easiest first" queue is genuinely easy here — pure functions with exact expected outputs |
| Behavioral ground truth beyond unit-level checks | `bot_sim` (offline judge simulator), judgement CSV export, audio-clock observers, and the trace-diff concept (§5.4) | An **integration-level gate LibertyFlux does not have** — its own README admits nothing has run in-game yet |
| Multi-build confidence | 11 `gamemdx` builds in Ghidra for cross-diffing | Function stability across builds is already a practiced discipline (the signature sweep) |
| Agent governance (no decomp commits, machine-decided done, .artifacts/ isolation) | The repo already runs this posture: generated artifacts never hand-edited, maintainer-only commits, absolute-path hygiene | Direct adoption; add the publication check + per-function ledger as new scripts |

Two honest cautions from LibertyFlux's own experience, which apply equally here: (a) its "verified" is per-function with **callees stubbed** — integration truth only arrives at the swap-in gate, which is why our existing in-game harness is worth its weight in gold and should be the *primary* done-criterion; (b) easiest-first ordering **overstates progress early** — the % verified will climb fast through thunks/getters while the hard 20% (AFP VM, renderer, audio clock) remains, so milestone gates (§6), not raw function counts, must drive scope decisions.

### 5.2 Option A — Clean-room behavioral reimplementation (SK8-ENGINE model)
Reimplement systems from the RE documentation, prioritized by playable milestone. Fastest path to something running; per-function coverage is emergent, not tracked. Risk: silent behavioral drift in judge/grading details (mitigated here by the trace-diff harness, which A/B-tests exactly those).

### 5.3 Option B — Function-by-function systematic parity (raw LibertyFlux model)
Work through `gamemdx`'s 18.6k functions systematically, tracking completion per function, porting in queue order. Highest rigor and a measurable burn-down; slowest to first playable; risk of never crossing the "runs" threshold (LibertyFlux itself is public evidence of both properties — 2,364 commits, phase 3, nothing in-game yet).

### 5.4 Recommended — Hybrid: LibertyFlux mechanics, milestone-shaped queue (Option C)
- **Adopt the full LibertyFlux mechanics**: measured inventory → script-generated easiest-first queue (but *seeded/weighted by milestone*: attract-mode functions before deep-system functions, so the port boots early) → one-agent-one-function rewrites → side-by-side checker for pure functions (1000+ generated inputs, mutation-tested) → verified/unverified ledger + static lints + issue log → assembled library swapped into the live game **through the existing hook DLL** with per-function switches (bisection-ready).
- **Keep the pure/impure split**: the checker's scope is exactly the pure functions this repo already isolates (`*_logic.rs`, `*_math.rs`, format codecs, judge math). Impure/engine-coupled functions (rendering, audio threads, scene glue) can't be input/output checked in isolation — those are verified at the **integration gate**: trace-diff against the original game via scripted cabinet runs, using the same input scripts in both the original (record) and the port (replay). This two-tier verification (checker for pure, trace-diff for impure) is the main structural deviation from LibertyFlux, and it's only possible because of the in-vivo instrumentation this repo already owns.
- **Ledger as generated artifact**: export the Ghidra function inventory (name, address, size, xref degree, class namespace, already-documented flag) into a generated coverage ledger; per-function status (queued / analyzed / rewritten / checker-verified / swapped-in-live / trace-verified). Progress badges in the same style.
- **Phase discipline from LibertyFlux**: faithful reconstruction only, no modernization, until the standalone build runs — then renderer/platform work begins.

The Rust workspace shape: `formats/` (the in-crate codecs, promoted to first-class host-tested crates), `avs/`, `afp-vm/`, `gs/` (engine-side rendering types; renderer swap deferred per phase discipline), `xact2/`, `sim-core/` (judge/gauge/grade — bot_sim's lineage), `scenes/`, `tools/` (checker, queue, ledger, assembler, trace recorder/differ), `rewrites/{verified,unverified}/` (harness-form function rewrites, exactly LibertyFlux's tree), `.artifacts/` (git-ignored: decompiler output, caches, agent scratch).

### 5.5 The trace-diff harness (the integration gate — this repo's unique advantage)

1. Define a canonical **trace format**: timestamped stream of (input events, scene transitions, judge outputs with exact windows/gauge deltas, widget/actor state snapshots, audio-clock samples).
2. **Recorder mode in the hook DLL**: subscribe to the existing dispatchers (`judge_hook`, `scene_manager`, `input_manager`, `render_notes_hook`) and dump traces from the real game under scripted input (game_nav).
3. **Replay mode in the port**: consume the same input scripts through the Rust scene/judge stack, emit the same trace format.
4. **Differ**: automated per-frame comparison; tolerance bands for floats where the original's FMA/ordering differs; golden traces per song/scene checked into the repo.
5. Per-module parity = checker pass (pure functions) + swap-in survival (impure functions) + trace match. This gives the project the same regression discipline the hook repo already practices (signature sweeps, host harnesses) — extended from "the mod still works" to "the port is the game."

---

## 6. Phased roadmap (LibertyFlux-style gates; estimates assume solo maintainer + heavy agent assistance)

Adopting LibertyFlux's phase-gate language ("finished when"), mapped onto this repo's realities:

| Phase | LibertyFlux analog | Duration | Finished when |
|---|---|---|---|
| 0 — Measure & harness | Phases 0–1 (done there) | ~2–3 mo | Ghidra inventory exported to the coverage ledger; queue generator live; checker + mutation testing running on host; trace format + hook-DLL recorder + differ built; rewrites tree (`verified/`, `unverified/`) + publication check in place; codecs promoted to host-tested `formats/` crates. |
| 1 — Pilot | Phase 2 (done there) | ~1–2 mo (overlaps 0) | 50–100 functions accepted end-to-end (checker-verified pure functions swapped into the live game via the hook DLL with per-function switches, bisection demonstrated). Accept rate and false-accept rate measured — these numbers decide whether to scale up. |
| 2 — Rewrite, milestone-shaped | Phase 3 (in progress there) | ~9–15 mo | The game **plays attract → title → song select → one full song → results** with every swapped-in function enabled and trace-diff green on the core loop. This is the credibility event and the honest re-scope point. (Queue ordered easiest-first *within* milestone systems, not globally, so the port boots early rather than 42k-functions-later.) |
| 3 — Standalone | Phase 4 | ~3–6 mo | Rust code builds and runs the same core loop without `gamemdx.dll` present, reading the stock asset install through `formats/`/`avs/`. |
| 4 — Breadth to full coverage | Phase 3 tail | ~12–24 mo | All 58 scenes, options framework, customizers, unlocks/event flags, special modes, movies, 3D backgrounds, profile/persistence; ledger at 100% of the meaningful-function set, trace suite green. |
| 5 — Modernization | Phases 5–7 | open-ended | 64-bit-first everywhere (trivial — already the target), new renderer (wgpu/Vulkan), platform spread, fps-unlock-class improvements — *only after* Phase 4 per the don't-modernize-mid-reconstruction rule. |

Notes: `gamemdx` is already 64-bit (no LibertyFlux Phase-5 lift needed — a real saving vs. GTA IV); sibling DLLs enter the queue as their own subsystems (AFP VM earliest among them, per §4). The "encrypted megabyte" problem LibertyFlux has (code only readable at runtime) does not exist here — the on-disk DLL is fully analyzable, another relative advantage.

---

## 7. Effort model

- `gamemdx`: ~18.6k functions, but experience with this binary suggests a large share are thunks, lambdas, STL/compiler artifacts, and tiny getters (avg ~160 bytes/function). Estimated *meaningful logic*: roughly 7–10k functions, of which the high-value core (judge, sequences, options, scoring) is maybe 1.5–2.5k — the area already 30–50% documented by prior RE. (LibertyFlux's 37,413 game functions vs. our 18.6k — and no encrypted-on-disk portion — put this project at under half the raw scale of the GTA IV effort.)
- Agent-assisted analysis throughput on this codebase (already demonstrated across the 93 docs): a few complex functions/hour, dozens/hour for simple ones with the Ghidra loop. The LibertyFlux-style queue/checker model industrializes exactly this: the checker eliminates review as the bottleneck, and accept-rate data from the pilot phase calibrates all later estimates. Sustained, that supports the Phase 0–2 timeline; Phase 3+ is breadth-dominated and community-friendly if ever opened up.
- Sibling effort is dominated by the AFP VM (≈ a quarter to a third of total project effort); everything else is thin by comparison.

## 8. Legal & distribution reality (read before any public step)

This is not legal advice; it is a risk inventory. The two inspiration projects survive in a gray zone that has *so far* tolerated RE-driven Rust rewrites of **abandoned, offline, consumer-owned** titles (Skate 3: 2010, no live service; GTA IV: 2008, no live service). LibertyFlux's posture is instructive as the maximal-care template: buy-the-game framing, no code/assets/trademarks ever, publication-check script blocking decompiler output from tracked files, and an explicit "the people who made the game should be paid" stance — yet even that posture leans on users being *able* to buy the game. DDR World is materially different on every axis:

1. **Live commercial service.** DDR World is Konami's currently-shipped, actively-updated arcade DDR with an online e-amusement subscription business. A functional port competes with current revenue. Rights-holders defend live revenue far more aggressively than abandonware (cf. the regular takedown history across decomp and server-emu projects).
2. **Asset ownership is the broken link.** SK8-ENGINE's shield is "bring your own ISO." Almost nobody can bring their own DDR World install legitimately — it's an arcade product distributed to operators. The port's asset-agnostic code is only usable against data the user has no clean legal way to possess. This makes the *usable artifact* (converted assets, prebuilt packs) the hottest object in the whole project — equivalent to shipping the game's content.
3. **Licensed content.** The 30 GB data tree is dominated by licensed music/video with third-party rights that no port can ever clear.
4. **e-amusement.** Reimplementing the client's online half (ESS, protocol, licensing) is where RE crosses from "interoperable reimplementation" toward interfering with a live service — the category that has historically drawn the fastest and most serious responses. Keep it out of scope indefinitely.
5. **Clean-room posture helps but does not immunize.** Documented RE + independent reimplementation is the strongest practical posture (and the one this repo already practices), but the DMCA §1201 anti-circumvention exposure (however slim for a game with no meaningful access controls) and the practical economics of being an unresourced individual vs. a litigious rights-holder both argue for caution.

**Practical recommendations, in order:**
- Keep the project **code-only**: no assets, no converted asset packs, no trademark/trade dress (new name, no DDR branding), explicit "requires data from a legally obtained installation" stance. This is the SK8-ENGINE/LibertyFlux posture.
- Develop privately through at least Phase 2; get actual counsel before any public release, since this fact pattern (live service + non-consumer distribution + licensed content) has no good precedent to lean on.
- Never touch e-amusement emulation in the public artifact. Local profile persistence only.
- Accept the likely end state honestly: a technically complete, privately usable research port that can be shared as *code and methodology*, with distribution of anything asset-adjacent remaining a personal-risk decision.

## 9. Risks & unknowns

| Risk | Impact | Mitigation |
|---|---|---|
| AFP VM complexity explodes | Timeline ×2 on Phase 1/3 | Timebox a "BM2D subset" interpreter against trace diffs; expand per-scene |
| Game updates churn `gamemdx` mid-project | Coverage drift | Pin to one build (20260915) as the port target; cross-build diffing already proven |
| Judge/timing float parity | Feels-not-right gameplay | Trace-diff harness with tolerance bands; existing audio-clock research as ground truth |
| Legal attention | Project-killing | §8 posture; private development; counsel before publication |
| Solo-scale burnout | Abandonment (the norm for this genre of project) | Phase 2 as the explicit re-scope gate; coverage ledger makes progress legible and resumable — the same PDD discipline this repo already uses |
| "Full parity" is a moving goal | Never done | Define done = coverage ledger + trace suite green on the pinned build; accept a documented residue list |

## 10. Bottom line

- **Feasible as a technical project:** yes — unusually so, given the RE head start, RTTI-rich target, multi-build corpus, solved formats, and an existing in-vivo verification harness no comparable project has had.
- **Feasible as a public open-source project:** conditionally — the code can be open; the *game* cannot, because the game's substance is licensed content distributed through a live service. The honest framing for the public artifact is "an open reimplementation engine + research corpus," exactly like SK8-ENGINE, and with a sharper legal edge than either inspiration due to DDR World's live-service status.
- **Recommended next step when work does begin:** Phase 0 — export the Ghidra function/class inventory into the coverage ledger, stand up the checker + queue skeleton, and build the trace recorder into the hook DLL. All three are low-cost, immediately useful to the existing modding work, and they de-risk every later phase.
