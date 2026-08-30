# Idea Honing — S-Marvelous Judgement (decision register)

**Readiness Confirmed 2026-08-29** — register accepted (no Proposed/Open items);
research complete (orientation, AFP tooling, display-side RE). Proceeding to
detailed design.


| ID | Decision | Why it matters | Accepted answer | Status |
|----|----------|----------------|-----------------|--------|
| D1 | Architecture: Option C (presentation-layer discrete grade; engine grade space untouched) | Decides everything downstream | Option C per research verdict | Accepted |
| D2 | Gameplay flash + results implementation: direct AFP scene modification (C-afp), first-class from day one | Look/feel; tooling investment | **AFP edits are the primary mechanism, not a fallback.** Build first-class AFP tooling now — future mods need it anyway. Results screen must be indistinguishable from Konami's work. bemaniutils (sibling checkout) is the documentation/tooling basis | Overridden |
| D3 | Results-row exclusivity | Discrete-grade semantics | Exclusive: native S-MARVELOUS row added to the score tab; MARVELOUS shows (stock − S-Marv) | Accepted |
| D4 | Stock flash on an S-Marv step | No double display | Replaced natively: mod re-drives the same `dance_judge` clip to the new `in_smarvelous` label (same three calls as the stock 0x1028 case, post-broadcast) | Accepted |
| D5 | Enable surface | UX | **Global top-level mod toggle only** — "S-Marvelous Judgement (12ms)" in the mods map / overlay MODS tab. No per-player option row, no custom_options registration, no wire field, no seop label textures | Overridden |
| D6 | Window value | Tunability | Fixed ±12 as the product; operator config `s_marvelous.window_ms` (default 12, clamp 1..=17) retained as a tuning escape hatch, no UI | Accepted |
| D7 | PUS integration | Scope | **None.** No timing-stats line, no CSV column. (The judge_submit *tap* still physically lives in `data_feed.rs` per the one-detour rule — that's plumbing, not PUS integration; see D12) | Overridden |
| D8 | Art plan | Gates the visuals | **No placeholder widgets.** Maintainer supplies recolored Marvelous textures as day-one real textures inside the edited AFP packages | Overridden |
| D9 | Combo tint + S-MFC splash | Scope | **Fully in scope.** A full S-Marv combo gets its own accolade and splash, distinct from MFC. (Per-song PB persistence — saving your best S-Marv count per song across sessions — confirmed out of scope) | Overridden |
| D10 | Calibration-hide interplay: S-Marv flash hidden while calibration hides judgement feedback | Preserves auto-calibration D18 | Gate the re-drive on the same suppression state | Assumed |
| D11 | overlay_element_styling judge scale/opacity applies to the S-Marv flash | Visual consistency | Automatic: the flash IS the stock `dance_judge` clip (same layer), so existing styling applies with zero work | Accepted |
| D12 | Classification lives in `data_feed.rs` hook body (calibration-tap pattern) | One-detour-per-target; only place the delta exists | Hardwired atomics block; all policy in the new mod | Assumed |
| D13 | Counting semantics: tap grade-0 events only (opcode 0x1028); freeze-OK/shocks never S-Marv; no FAST/SLOW | Stock semantics inheritance | As stated | Assumed |
| D14 | Reset discipline: counters reset at GAMEPLAY entry (scene cb) + `song_reset` subscription | In-place restarts/training scrubs never leave scene 28 | Both reset points (PUS pattern) | Assumed |
| D15 | Autoplay behavior | Expected outcome | Autoplay + S-Marv enabled ⇒ full S-Marvelous combo (Δ≈0 classifies everything S-Marv; tint + S-MFC splash all fire). No suppression — this is the desired, accurate display | Overridden |
| D16 | Rate play | Correctness | Works at any playback speed (content-time ms ⇒ ±12 scales identically with stock windows). Explicitly supported | Accepted |
| D17 | Fail-open: data_feed unavailable ⇒ mod inert with one WARN; per-surface degradation elsewhere | House pattern | As stated | Assumed |
| D18 | Mod id `s-marvelous`, module `src/mods/s_marvelous/` | Naming | As stated; display name "S-MARVELOUS JUDGEMENT (12MS)" | Assumed |
| D19 | Modes covered: everywhere GAMEPLAY runs (normal, versus per-side, training, course) | Consistency | As stated | Assumed |
| D20 | AFP tooling shape | Reusable deliverable for future mods | **Client-side runtime synthesis in Rust** (shader_synthesis / atlas_cloner precedent): repo bundles ONLY net-new assets (the recolored PNGs); AFP modification/cloning happens on the end user's machine at mod init / lazily at arc open, fingerprint-cached under `data_mods/_cache/`. NO pre-authored modified arcs, NO Konami assets in the distribution. Python tooling permitted for research/exploration only — never in the shipped path. bemaniutils = format documentation source for the Rust implementation | Overridden |
| D21 | Native-representation surface list | Scope boundary for "fully representative" | In: score-tab S-MARV row (exclusive), gameplay flash, combo digit set, FC splash + accolade, results FC emblem (if present as a distinct element), **and the end-of-song per-step judgement graph** (must show S-Marvelous; mechanism pending RE). Out: server-side surfaces — song-select score popup renders server data and the server cannot distinguish S-Marv from Marvelous (maintainer-confirmed fine) | Accepted |
| D22 | S-MFC semantics | Display policy | All judged steps S-Marv + full combo ⇒ S-MFC accolade/splash shown instead of MFC. Engine still records clear kind 10 (MFC) — display-only override; S-MFC ⊂ MFC by construction | Accepted |
| D23 | Combo digit mechanism | Native look | New recolored digit texture set (`daco_combo<suffix>_{digit}` naming family) injected via texture pipeline; mod re-drives the digit bitmap loads to the S-Marv set while the combo's worst judgement is S-Marv (mirrors the stock suffix mechanism rather than color-tinting) | Accepted |
| D24 | Results population mechanism | The edited layout's new widgets need data | Stock code populates `*_num_usr` widgets by name; mod does the same for `smarvelous_num_usr` + rewrites `marvelous_num_usr` (stock − n) after stock population. Requires RE confirmation of a usable name-lookup/set-text path on the results scene | Accepted |
| D27 | Synthesis trigger point | Where the runtime AFP edit runs | AP2 byte edits via the shipped `afp_patcher` in-memory seam (`register_patch(template_name, fn)` at `afp_stream_do_create` — data arrives descrambled); textures via the existing atlas pipeline (cached under `data_mods/_cache/`). No disk arc synthesis needed | Assumed |
| D28 | Results/graph data source | Robustness | S-Marv counts + per-second graph series recomputed at results time from the STAGE RECORD's per-note streams (grade byte vector `record+0xB8`, signed ms-error i16 vector `record+0xD8`, timestamps in the note-entry vector `+0x98`): `grade==0 && \|ms\| ≤ window`. Live gameplay surfaces (flash/combo/splash) still use the judge-tap counters | Assumed |
| D29 | FC splash capture | The splash clip's creation is inlined — invisible to the existing CMovieClip::Create capture | New detour on the FullcomboActor onMessage (`FUN_180069c50`-analog; AOB `81 FA 34 10 00 00` verified module-unique on 20260721); post-original re-drive `afp_mc_op(0xF09, "s_marbelous_in")` when type==0 && all-S-Marv | Assumed |
| D30 | All-S-Marv combo bit vs freeze O.K. | Stock maps grade 6 (O.K.) to marvelous tier in worst-judgement tracking; O.K. carries no ms delta | Mirror stock: O.K. does not degrade the all-S-Marv status | Assumed |
| D25 | Disabled-mod safety of edited assets | LayeredFS serves edited arcs regardless of mod enable | AFP edits must be strictly additive (new labels/frames/textures stock code never references) so a disabled mod ⇒ byte-identical stock behavior | Assumed |
| D26 | Live toggle granularity | Mid-song correctness | Enable state latched per side at GAMEPLAY entry (house pattern); overlay MODS tab toggle takes effect next song | Assumed |

---

## Decision details

### D2 (overridden 2026-08-29)
Maintainer: AFP scene modification is the first implementation, not an upgrade
path. First-class AFP tooling is wanted independently (near-future mods need
it). Results modifications must be indistinguishable from Konami's work.
bemaniutils (sibling checkout at the same Projects level) carries extensive AFP
documentation to build on. Consequence: the C-widget variant is dead; the
tooling workstream is promoted from "optional M–L" to a core deliverable.

### D5 (overridden 2026-08-29)
No per-player option. Global mod toggle only. Removes: custom_options row,
PersistMode/wire field, bemani-buddy migration, seop label textures, per-side
option latching (enable is still latched per song, D26).

### D7 (overridden 2026-08-29)
No PUS-facing features. The physical tap location in `data_feed.rs` stands
(D12) because the ms delta only exists inside the one `judge_submit` detour.

### D8 (overridden 2026-08-29)
Maintainer already has recolored Marvelous textures to use as real textures
from the first deploy. No TextWidget placeholder phase.

### D9 (overridden 2026-08-29)
Combo tint and S-MFC splash promoted to core scope: "a fully representative
experience in terms of support." Per-song PB persistence (best S-Marv count per
song, persisted across sessions via the string-field registry) explained and
confirmed out of scope.

### D15 (clarified 2026-08-29)
Autoplay + S-Marv ⇒ full S-Marvelous combo is the *expected* outcome, not a
side effect to mitigate.

### D16 (clarified 2026-08-29)
Non-100% playback speed remains supported; no gating.

### D20 (overridden 2026-08-29)
Maintainer: no offline Python pipeline in the shipped path, no bundling of
Konami assets (or pre-authored modified/cloned arcs) in the modpack
distribution. Follow codebase precedent (atlas_cloner, shader_synthesis): repo
carries only the net-new assets (recolored PNGs); all AFP modification/cloning
happens client-side at runtime and is cached under `data_mods/_cache/`. Python
tooling is fine for research. Consequence: the "first-class AFP tooling" is a
**Rust** AP2 parse/edit/write module in the DLL (likely `core/` — game-agnostic
format layer), informed by bemaniutils documentation — the single biggest
engineering piece of the feature.

### D21 (amended + accepted 2026-08-29)
The end-of-song per-step judgement graph (results graph tab) is IN scope —
S-Marvelous must appear there. RE confirmed mechanism (see
`research/display-side-re.md` §3): the graph is a chart renderer over
per-second aggregate vectors rebuilt every frame; the mod participates via a
detour on the rebuild fn (append an S-Marv series + subtract from the
marvelous series + add a real-font "■S-MARVELOUS" legend line). Server-side
exclusion confirmed: the server cannot distinguish S-Marv from Marvelous, so
server-rendered surfaces (song-select score popup, incl. its FC emblem) stay
stock.

### D23 (refined post-RE 2026-08-29)
Combo mechanism confirmed: detour the digit-refresh fn (tint-immediates AOB)
post-patch — reload places {10,100,1000} on layer root1 with
`daco_combo_smarvelous_%d` via the stock traversal-6 walk + apply a new tint
constant pair on root2/root3 via wrapper vfunc+0x98. Stock quirk inherited:
the ONES place is always unsuffixed. Self-healing on bit drop (next stock
refresh restores marvelous art). Suffix table is 4 entries — never index OOB.

### D24 (resolved post-RE 2026-08-29)
Score-tab numbers are `sequence::SpriteLayer` (the class the modpack already
drives). Mechanism: AFP-patch `detail_result` (add `smarvelous_num_usr`
instance + row label art placement); detour the tab populate fn; mod-owned
SpriteLayer anchored on the new instance for the S-Marv count; exclusivity by
rewriting the stock marvelous widget's glyph list (spritelayer_set_names) to
(stock − n). Total results (scene 32) shows no per-grade counts — only the FC
emblem (bitmap `scre_total_player_%s`), covered by D22's S-MFC override.
