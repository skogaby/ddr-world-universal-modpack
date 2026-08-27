# Progress — Overlay Element Styling

Updated: 2026-07-12
Status: DONE — feature complete and VERIFIED ON-CABINET (all 8 steps + 4 cabinet
refinements). Awaiting maintainer commit only.
NEXT ACTION: none required. Maintainer to commit when ready (do NOT commit
automatically). Optional low-priority follow-ups below if ever revisited.

Optional/unconfirmed follow-ups (feature accepted without them):
- Exact versus `X_SPLIT` (640) only matters in a 2P versus play; confirm from a bind
  log (`bind kind=… pos=(x,y) side=…`) if versus behaves oddly.
- Whether the `+0xB0` int-color path ever fires on tracked clips (one-shot INFO log
  `+0xB0 int color fired…`); if it never appears, +0xB0 is dead-path insurance.

Resume protocol: `implementation/plan.md` (all 8 steps ticked),
`design/detailed-design.md` (authoritative mechanism), RE doc
`docs/gameplay_overlay_elements_research.md` (§10 = impl status + open items).

## Done (all 8 steps, each cargo-check-clean)

1. Signatures + color-twin resolver — `src/core/signatures.rs`:
   `cmovieclip_create` + `cmovieclip_set_position` SignatureDefinitions;
   `derive_cmovieclip_color_twins` (called from `resolve_derived`) scans the two
   twin patterns, decodes each body's `CALL [RIP+disp]` IAT slot (float disp @
   match+0x21, int @ match+0x30), and disambiguates set_color vs set_acolor against
   `GetProcAddress` (helper `resolve_libafp_export`). Publishes
   `cmovieclip_set_color_float` / `_int`; any ambiguity → unresolved. **Ghidra-
   verified on both builds 2026-07-12**: create/setpos unique; both twin patterns
   match exactly 2 (order flips between builds, addresses match RE §8 table).
2. `bm2d_api` raw-id setters — `layer_set_scale_raw` (reuses the all-or-nothing
   LAYER_API's `afp_layer_set_matrix`), `layer_set_color_raw` +
   `layer_color_available` (independent, non-fatal `LAYER_SET_COLOR` cell for
   `afp_layer_set_color`; a miss does NOT disable the bg-preview set). Never wraps
   these game-owned ids in `AfpLayer`.
3. Mod skeleton + rows — `src/mods/overlay_element_styling/mod.rs`; registered in
   `mods/mod.rs` + `lib.rs`. Two `Scalar` rows (`overlay_scale` 25–150/5/25,
   `overlay_opacity` 0–100/5/25, both default 100, `PersistMode::Full`, identity
   transforms) via two `on_*_change` cbs mirroring to per-side atomics. Label PNGs
   generated (`scripts/gen_option_labels.py` + `seop_item_overlay_{scale,opacity}.png`).
4. Capture — `capture.rs`: 64-slot `static mut` registry + `REGISTRY_LEN` hot-path
   guard; Create detour (`install_enabled`, original-first, `catch_unwind`,
   defensive bounded name read, exact-before-prefix classify — `dance_effect` excluded,
   slot-reuse eviction, overflow warn); scene callback clears on GAMEPLAY enter/exit
   + logs per-song capture counts.
5. Side binding + scale — SetPosition detour (non-fatal) binds side at first position
   write: active-side via `player_array_anchor` presence read (ported from
   `center_arrows_single`), x<640→P1 in versus. Create-time single-side fallback when
   SetPosition detour absent. `bind_and_apply` does layer-id revalidation + scale
   one-shot (`layer_set_scale_raw`, skip @100) + bind debug log.
6. Opacity — `color_hook.rs` +0x90 float compose detour (load-bearing; alpha is the
   FIRST float arg; `a*=op/100` for tracked+bound, forward untracked/unbound). Per-kind
   color one-shots at bind: judge/freeze/fast_slow + pacemaker get
   `layer_set_color_raw(1,1,1,op)` (skip @100); **combo = compose-only** (no one-shot).
7. +0xB0 int compose detour (non-fatal, integer alpha math clamped ≥0, one-shot
   diagnostic log). Hardening audit vs design §6 passed (see below). `REGISTRY_LEN`
   occupancy fast-path added.
8. Docs — README mod-table row, AGENTS.md entry-points row, RE doc §10 impl-status +
   pending-cabinet items. Release build clean.

## §6 hardening audit (verified by review)

- Every detour body wraps OUR logic in `catch_unwind(AssertUnwindSafe)` and calls the
  original regardless (create/setpos call original first; color detours fall back to
  the unmodified alpha on panic). All 4 detours installed via `install_enabled`
  (store-before-enable). None-slot teardown race → benign default (return 0 / forward),
  matching scene_manager precedent.
- Name read: null-checked, ≤63 bytes + NUL, UTF-8 validated. Registry overflow: warn
  once, never overwrite live. Layer-id revalidated at bind (evict on mismatch).
  Untracked color writes forwarded unchanged. Hot path (color float) is
  allocation-free; bind-time logging (cold, ~13–21×/song) is fine.

## Deploy & test log

- **2026-07-12 deploy #1 (build 20260616): options did NOT appear.** Root cause
  in `log.txt`: `[-] cmovieclip_create -- pattern not found` → load-bearing set
  incomplete (`create=false`) → mod inert. All OTHER sigs resolved at the correct
  20260616 addresses (set_position +0x258DE0, set_color_float +0x25E790,
  set_color_int +0x259180; twin disambiguation worked). No PANIC.
  - Diagnosis: `cmovieclip_create`'s 20-byte prologue literal run shares its first
    18 bytes with the pre-existing `afp_layer_init_wrapper` signature (which
    resolved to the SAME function @ +0x257770). The batch scanner
    (`scan_patterns_batch`) builds ONE Aho-Corasick automaton over all needles and
    iterates NON-overlapping matches, so at 0x257770 the shorter afp needle is
    consumed first and the longer create needle is never reported.
  - **Fix (deployed build pending):** removed `cmovieclip_create` from the batch
    `SIGNATURES` array; resolve it standalone in `signatures.rs::derive_cmovieclip_create`
    (single-needle scan → no cross-pattern collision; full-pattern verify retained).
    `cargo check` + `./build.sh` clean. **Re-deploy and re-verify.**

- **2026-07-12 deploy #2: options appeared + writes landed, but all scoped elements
  rendered in the exact SCREEN UPPER-LEFT corner** (scale 30 / opacity 35). Live
  diagnosis via Cheat Engine MCP on a frozen gameplay frame:
  - Walked the CMovieClip pool + resolved each layer id → layerobj via libafp's
    `afp_layer_set_matrix` id-mapping (`layerobj = *(libafp+((id>>27&0xf)-1)*8+0x244fd0)[id&0xffff]`).
    Found exactly 12 non-identity layers (3 combo + 1 judge + 7 freeze + 1 pacemaker
    = a single-play set) — capture/bind/scale/color ALL landed on the right clips.
  - Each read `m[0]=0.300` and color mult `alpha=0.350` (correct), but the 4×4
    matrix translation row `(m[12],m[13]) @ +0x130/+0x134 = (0,0)`, while an
    untouched sibling layer read `(641,663)`.
  - **Root cause:** the layer translation is the 4×4 matrix's row-3, at the SAME
    address (`+0x130/0x134`) the RE doc wrongly called a "disjoint position field".
    `afp_layer_set_matrix` rewrites the WHOLE 4×4, so our `{0.3,0,0,0.3,0,0}` (tx=ty=0)
    zeroed the translation the game had set → everything drew at the origin. Confirmed
    `afp_layer_set_position` (`0x1800135e0`) writes ONLY the two translation dwords.
  - **Fix:** `bm2d_api::layer_set_scale_translate_raw(id,sx,sy,tx,ty)` writes
    `{s,0,0,s,tx,ty}`; `bind_and_apply` threads the SetPosition detour's `(x,y)` as
    `(tx,ty)` (Create-fallback passes `(0,0)` — the game's later set_position fills
    translation without disturbing scale). Also: converted the bind + `+0xB0`-hit
    diagnostics from `log_debug!`→`log_info!` (spice2x swallows debug). RE doc §1/§4/§9
    corrected. `cargo check` + `./build.sh` clean. **Re-deploy and re-verify** (elements
    should now scale about their centers and stay in place).

- **2026-07-12 deploy #3: works, but the element SPACING didn't scale** — at small
  scale the combo↔judge gap grew because each element scaled about its own center
  while its position stayed fixed (screenshots
  `capture/capture_20260712_11{4408,5148}.jpg`; 150% `_114651.jpg` looked ~normal
  because bigger text filled the fixed gap). User asked to scale the gaps too,
  anchored on the judgement text (bias toward the top).
  - **Fix (uniform cluster zoom about the judge anchor):** `capture.rs` now stores each
    clip's original position (`orig_x/orig_y`) and computes its placed Y as
    `new_y = anchor_y + s*(orig_y - anchor_y)` (X unchanged → per-panel freeze columns
    and lane-centered text keep their horizontal layout). The anchor is the judge
    element's position, captured on judge-bind; because bind order isn't guaranteed,
    `place_side` re-places all same-side clips once the judge sets the anchor. The
    SetPosition detour now RE-anchors on subsequent repositions (FAST/SLOW msg 0x1035)
    so gap-scaling survives dynamic moves. Identity at 100% (skipped → stock). Anchor
    resets per song in `clear()`. `TrackedClip.applied`→`bound`; added `orig_x/orig_y`.
    `cargo check` + `./build.sh` clean.
  - **Follow-up (freeze exclusion):** FreezeJudge (`dance_judge_for_freeze`) is now
    EXCLUDED from the judge-anchored Y compression in `place()` — its O.K./N.G.
    results are per-panel (above each lane column), so they keep their original
    position and only scale about their own center. All other scoped elements still
    get the gap compression. `cargo check` + `./build.sh` clean.

- **2026-07-12 deploy #4: VERIFIED ON-CABINET — everything works.** Options render and
  persist; per-player scale + opacity apply to combo/judgement/FAST-SLOW/pacemaker at
  song start; cluster gap-scales about the judge anchor (freeze results excluded, stay
  per-panel); receptor flashes untouched; coexists with center-arrows-single. Feature
  accepted. Maintainer to commit.

## Cabinet validation sweep (TODO — design §7)

- Boot log: 4 addresses resolved + twin line (`set_color=+0x…, acolor sibling=+0x…`);
  cross-check vs RE §8 for the cabinet's build. `afp_layer_set_color` resolved.
- Per-song capture counts log (3 combo, 1 judge, 7/15 freeze, 0–1 fast_slow, 0–1
  pacemaker); registry clears per song; NO `dance_effect` captured.
- Bind logs: single/double/versus — confirm/correct `X_SPLIT=640`.
- Scale: P1 50% shrinks combo/judge/freeze/fast_slow/pacemaker about centers, P2 stock;
  persists across a full song (sole-matrix-writer); 150% acceptable.
- Opacity: 0 → all 5 invisible, gameplay unaffected; 50 → combo only ≥4 combo at half
  alpha (gating preserved), pacemaker neg-delta at 0.5×op, judge pop fades; 100 =
  pixel-identical to stock.
- `+0xB0` verdict: does the int-hit diagnostic ever fire on tracked clips?
- Persistence round-trip (network + server-less JSON). Degradation drills (Q9 tiers).
- Regression: receptor flashes untouched; PUS pacemaker-swap coexists (both active in
  one song); bg-previews / options screens / autoplay / quick-restart unaffected;
  2-player persistence.
- **center-arrows-single coexistence (1P): both enabled → elements centered (X) AND
  scaled + top-anchored (Y), no clash.** Verified statically: disjoint detour targets
  (center-arrows hooks `hud_layout_builder`/`hud_layout_setter`; this mod hooks
  `cmovieclip_create`/`_set_position`/`_set_color_*`); orthogonal axes + phases
  (center-arrows rewrites layout `coord[0]`=X during layout build; this mod preserves
  that X as `orig_x` and transforms only Y+scale in the layer matrix at element
  positioning). Cabinet check: enable both, 1P, scale 30 — combo/judge/etc. should be
  centered horizontally and tight vertically about the judge. Documented in
  `overlay_element_styling/mod.rs` module doc.

## Deviations from design (documented, intentional)

- `required_signatures()` returns `&[]` (design §4.7 listed the two AOBs). Reason: the
  load-bearing set spans AOBs AND runtime bm2d service state, which the registry gate
  can't express — so ALL checks live in `init`/`enable` with `is_active()` self-disable,
  matching `center_arrows_single`. Keeps the mod visible/toggleable but inert (no rows,
  no hooks) when the set is incomplete.
- `disable()` does NOT reset the atomic mirrors; `enable()` re-seeds them from
  `custom_options::get_value` (register_option returns `Duplicate` on re-enable and
  doesn't re-fire `on_change`, so an enable-time reseed is the correct source of truth).
- `afp_layer_init_wrapper` (pre-existing, unconsumed signature) is a byte-prefix of
  `cmovieclip_create` — same function. No conflict today; noted in the new signature's
  comment.

## Key facts for a cold resume

- Files: `src/mods/overlay_element_styling/{mod,capture,color_hook}.rs`;
  `src/core/signatures.rs` (+2 sigs + `derive_cmovieclip_color_twins` +
  `resolve_libafp_export`); `src/services/bm2d_api.rs` (+3 raw fns + `LAYER_SET_COLOR`);
  `src/mods/mod.rs` + `src/lib.rs` (registration); `scripts/gen_option_labels.py`
  (+2 labels) + 2 new PNGs; README/AGENTS/RE-doc.
- Build: `cargo check --target x86_64-pc-windows-msvc` ✓; `./build.sh` ✓.
- Alpha is the FIRST float arg of wrapper SetColor. Combo opacity is compose-ONLY.
  Game-owned layers use the raw-id fns (never `AfpLayer`). Do NOT `git commit`.
