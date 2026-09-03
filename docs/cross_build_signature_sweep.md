# Cross-Build Signature Sweep (2026-09-03)

**Scope:** every AOB signature, derivation, inline pattern and consumer-side
fixed offset in the DLL, checked against the four supported `gamemdx.dll`
builds — **20250805**, **20260224**, **20260721**, **20260825** — plus the
three `arkmdxbio2.dll` builds that shipped (20250805 / 20260224 / 20260721;
20260825 shipped without one) and the shared AVS libraries (`libafp-win64`,
`libavs-win64`, `libafputils-win64`).

**Trigger:** the 2026-09-03 field fixes (commit `475d735`, validated against
20250805 + 20260224) were all driven by user bug reports. This sweep was the
proactive pass to find what the reports hadn't hit yet.

**Headline:** 20 base signatures are version alternates by design (every miss
covered by a resolving sibling). Every derivation resolves on all four builds.
The AOB layer was healthy — **every real finding was a consumer that hit on
all four builds and then read or wrote game memory at an offset that had
moved**, i.e. exactly the class of bug the harness's post-match shape diff was
built to expose. Six fixes landed (three functional RISKs on the old builds,
one build-independent dead diagnostic, one hardening, one cosmetic).

---

## 1. Method

### 1.1 `scripts/validate_signatures.sh` — the real resolver, offline

Plain `aob_check.py` only tells you whether a byte pattern is unique. The
DLL's actual compatibility surface is `SignatureStore::resolve_all` +
`resolve_derived` — ~128 patterns and ~45 derivation functions with their
own byte-shape validation. So the harness mounts the DLL's **real**
`src/core/scanner.rs` + `src/core/signatures.rs` (+ `profiling.rs`) via
`#[path]` into a throwaway std-only host crate (`target/sig_harness/`, like
the other `validate_*.sh` scripts — plain `cargo test` can't compile
`retour` on the ARM host), and runs them over each DLL **mapped the way the
Windows loader maps it**:

- sections copied to their virtual addresses (SizeOfImage-sized buffer),
- `.reloc` DIR64 entries applied against the host allocation (the RTTI
  walks and vftable identity checks read absolute pointers out of
  `.rdata` — un-relocated images make every `find_vtable_by_rtti` fail),
- gamemdx's **libafp IAT slots patched with synthetic pointers**, resolved
  through libafp's export table (gamemdx imports libafp **by ordinal**; the
  CMovieClip colour-twin disambiguation compares IAT targets against
  `GetProcAddress("afp_layer_set_color")`, so the harness needs one
  `libafp-win64*.dll` of any build alongside the game DLLs — ordinals 49/50
  are identical across the 2025 and 2026 libafp). The `#[cfg(not(windows))]`
  `resolve_libafp_export` in `signatures.rs` reads that table.

One process per build (a bad derivation on a foreign build must not take
the others down). The stub logger prints the exact `[+]`/`[-]` lines the
game would print into `log.txt`, and `scripts/sig_harness/report.py` joins
them with the crate's own consumer graph (`required_signatures()` lists +
`get_address` / `require_address` / `get_all_matches` call sites, extracted
live from `src/`) to print, per mod/service, which addresses it loses on
which build and whether that loss is HARD (required / `require_address`,
no resolving alternate) or soft.

```
./scripts/validate_signatures.sh ~/Desktop/ddr_modules [--json out.json] [--raw]
```

Exit 0 = every name that resolves on any build resolves on every build, or
its miss is covered by a version alternate (`_vN` stem or an entry in
`report.py::ALT_GROUPS`).

### 1.2 `scripts/sig_harness/shape_diff.py` — post-match shape diff

An AOB hitting on every build says nothing about the bytes AFTER the
literal prefix, and consumers routinely read/patch `match+N`. The shape
diff disassembles (capstone) a 0x200 window after every resolved match on
every build, normalizes away what legitimately differs (RIP-relative
displacements, direct branch/call targets, module-RVA displacements in
`[reg + reg*n + RVA]` jump-table addressing) and reports the first offset
at which the instruction stream diverges from the reference build — plus
the consumer list, so each divergent signature can be reviewed against the
offsets its consumer actually reads.

```
python3 scripts/sig_harness/shape_diff.py --json out.json --dir ~/Desktop/ddr_modules [--verbose]
```

### 1.3 Consumer review

For every divergent signature the consumer code was read for fixed-offset
reads/patches and hardcoded struct offsets, and the verdict cross-checked
against raw DLL bytes where the disassembly window was insufficient. The
review was split into three groups (results/records/judge; folders/series/
HUD/scene/shutter; bm2d/options/playfield/audio/song-rate/fast-bootup) and
is summarized in §3.

---

## 2. Results

### 2.1 Resolver

| Build | Base signatures | Names resolved incl. derived | Notes |
|---|---|---|---|
| 20250805 | 115 / 127 | 209 | 13 misses, all `_v1`/alternate-covered |
| 20260224 | 115 / 127 | 209 | identical to 20250805 |
| 20260721 | 116 / 127 | 207 | reference build |
| 20260825 | 116 / 127 | 207 | identical to 20260721 |

The 20 by-design gaps: `timer_show_call`, `stage_record_accessor`,
`flare_gauge_ctor_layout`, `playdata_row_write`, `result_window_build`,
`total_result_populate`, `song_rate_clock_anchor`, `player_option_ctx_load`
(each with a `_v1` twin), `folder_register`/`_v2`,
`series_label_lookup_inlined`/`_standalone`,
`textlayer_bind_anchor`/`_direct`, `hud_layout_builder` (old builds derive
the entry from `hud_layout_builder_style_cluster`).

`arkmdxbio2`: all 16 `arkMDX*` exports the DLL uses exist on all three
builds; the `MENU_OVERRIDE_PATTERN` (module-relative digest-override AOB)
is unique on all three. `libavs` ordinals 162/163/164/175/176 map to the
same functions in the 20250805 and 2026 libavs (same DLL version).
`song_limit_expansion`'s two inline patterns hit exactly 3× on all four
gamemdx builds as required.

### 2.2 Shape diff

20260825 vs 20260721: no consumer-relevant divergence (only past-function-
end noise). 20250805 / 20260224 vs 20260721: 60-odd signatures diverge
somewhere in the window; after review all but the items in §3 were either
past the function's `ret`, inside a pattern-wildcarded/decoded operand, or
in a struct field the consumer detects from content.

**The one structural fact behind three of the findings:** every
`GamePlayActor` field at or above ~`+0x208` sits **8 bytes lower** on
20250805 / 20260224 than on 20260324+ (the cluster ≤ `+0x1E9` — side,
judge counts, combo, is_dead — is identical). Visible in `judge_submit`
(`+0x2AC`↔`+0x2B4`), `judge_notes` (`+0x270`↔`+0x278` — the foot-panel
pointer `judge_hook` already detects), `result_commit` (`+0x278`↔`+0x280`,
already decoded) and the ctor's seed block (§3.1). Likewise `PlayerWork`
grew 0x20 (record base `0x570`→`0x590`, handled by
`stage_record_accessor_v1`) and `ddr::player::Option` moved
`0xF0`→`0xE0` (handled by `player_option_offset`).

---

## 3. Findings and fixes

### 3.1 RISK — GamePlayActor speed / gauge / death-flag offsets (old builds)

Hardcoded `+0x290/+0x294/+0x29C` (`song_rate::real_speed`),
`+0x2A0..+0x2B0` + `+0x2B7/+0x2B8` (`song_reset` gauge-cluster + death
restore, training-loop death-bypass gate) and `+0x2B7/+0x2B8`
(`quick_restart_or_fail` fallback death simulation) are the 20260324+
values; on 20250805 / 20260224 they are `+0x288/+0x28C/+0x294`,
`+0x298..+0x2A8` and `+0x2AF/+0x2B0`. Effects on the old builds: the
rate-aware Real Speed write landed on the wrong fields (multiplier stock at
rate, int copy clobbered with a float bit pattern); the in-place reset
restored the wrong five floats and left the real death-result flag alone;
the fallback death wrote two bytes past the ctor-seeded block.

**Fix:** `SignatureStore::derive_gameplay_actor_layout` (anchor = the
ctor's course-flag seed `MOV RAX,[rip]; MOV RCX,[RAX]; CMP qword
[RCX+0x70],0; SETNE AL`, unique on all four) decodes the five stores after
the anchor generically (any base register — the actor is `RDI` on 2026 and
`R12` with SIB encoding on 20250805), requires the `+1/+3/+4` adjacency,
then demands each ctor seed immediate (speed 1.0 / target 1.0 / int 100 /
gauge-min 1.0) at the predicted displacement in the 0x80 bytes before the
anchor, exactly once each. Published as `gpa_speed_cluster` /
`gpa_gauge_cluster` / `gpa_death_gate` pseudo-addresses; consumers read
`signatures.gameplay_actor_layout()` at init. `song_reset` refuses to be
available without it; `real_speed` skips the write with one WARN;
`quick_restart_or_fail` still forces `m_isDead` + `STEP_GAME_OVER` but
skips the two gate/result bytes. Derives `0x288/0x298/0x2AF` on the old
builds and `0x290/0x2A0/0x2B7` on the new ones.

### 3.2 RISK — ShutterActor kind fields / stage kind (old builds)

`quick_restart_or_fail`'s bannerless fast path read the active/pending
kind at `+0x310/+0x314` and expected the stage panel to be kind **3**. On
20250805 / 20260224 the fields are `+0x2E0/+0x2E4`, the shutter has **6**
kinds (not 9) and the stage panel is kind **1**; `+0x310..` there is an SSO
`std::string` buffer. Every trigger therefore silently fell back to the
natural-death path on the old builds (no crash — the range checks caught
it), pre-song quick-fail was a no-op, and the select-residency patch /
ready-dwell poke never engaged.

**Fix:** `SignatureStore::derive_shutter_actor_layout` — anchor = the
onUpdate per-kind layer lookup `MOVSXD RCX,[RSI+kind]; ADD RCX,RCX; LEA
RDX,[RSI+RCX*8+0x88]` (unique on all four) → active-kind disp32; the
`CMP dword [RSI+kind], imm8` within 0x40 bytes (must reuse the SAME
displacement, exactly one hit) → stage kind. Published as
`shutter_active_kind` / `shutter_stage_kind`; the mod reads
`signatures.shutter_actor_layout()` at init and every shutter read fails
(→ fallback) without it. Derives `0x2E0`/kind 1 and `0x310`/kind 3.

### 3.3 RISK — fast_bootup `hasChart` vtable slot (old builds)

The step-data cache replay called music-DB entry vtable slot `+0x70` as
`hasChart(mode, difficulty)`. On 20250805 / 20260224 `hasChart` is slot
`+0x58`; `+0x70` there is `isShock` — same argument shape, so no crash but a
silently wrong answer feeding `compute_slot`'s corruption-flag decision on
cache-hit boots (the `ME1529` reporter is never touched — D9 — so it could
only mis-set the `+0x1B0` flag on charts where shock ≠ hasChart).

**Fix:** `derive_ultrafast_boot` step (5) publishes `entry_has_chart_vslot`
from onUpdate's own vcall `MOV R8D,EBX; MOV EDI,[RSP+0x48]; MOV EDX,EDI; MOV
RCX,RSI; CALL qword [RAX+disp8]` (unique in the body on all four); the mod
reads it at init, `has_chart` returns false on an underived slot, and
`enable` refuses to arm replay (capture + loader pacing stay on). Derives
`0x58` / `0x70`.

### 3.4 Dead diagnostic — premium_free `result_commit` skip-offset decode (all builds)

`diag.rs::decode_commit_skip_offsets` read the second early-out `CMP qword
[RSI+d32],0` at match+48 (d32 at +51); the instruction is at **+56** (d32 at
**+59**) — `+48..+50` is `33 D2 FF` (`XOR EDX,EDX; CALL [rip]`). The opcode
check therefore failed on EVERY build and the BUG-1 pre-commit early-out
tap has been disabled since it shipped. Fixed (+56/+59); the signature
comment corrected. Build-independent, fail-open — found only because the
review compared the documented offsets against real bytes.

### 3.5 Hardening — series_expansion label-builder LEA

Patch 6 wrote a disp32 at `site+13−0x64+3` with no check that the bytes
there are the `LEA RCX,[vanilla table]` it assumes. Correct on all four
builds, but a future instruction reordering would have written into
arbitrary code. Now validated (`48 8D 0D` + RIP target == the predicate
LEA's table) per site; a mismatching site is skipped with one WARN.

### 3.6 Cosmetic

`derive_gameplay_obj_addresses` logged "expected E8 at alloc+20" for a read
at +22.

---

## 4. Reviewed and cleared (selected)

- `stage_record_accessor`/`_v1`, `ghost_local_slot_copy_site` (LEA
  `0x628`↔`0x648` = the 0x20 PlayerWork growth), `playdata_tab_update`
  (`+0x92` record arithmetic — consumer goes through `stage_records`),
  `final_stage_probe` — all decoded, cross-checked against real operands.
- `judge_submit` counts base `+0x1A0`, combo `+0x1DC`, is_dead `+0x1E8`,
  side `+0x84` identical on all four; `note_types_expansion`'s
  `detect_actor_field_offsets` content scan finds exactly one RDI-based INC
  per build.
- `folder_expansion`: `folder_init` call-walk + `detect_difficulty_offsets`
  content scan emulated on all four (old: size 0x1D0, max_diff +0x100,
  restriction flags +0x104 ×3 value 0; new: 0x208, +0xC0, +0x1FC ×7 value
  1) — both consistent with each build's `folder_init` writes.
- `series_expansion` patches 1–7: every read/patch offset is pinned by
  pattern bytes or decoded; flare tables `cats=[1,2,3] thrs=[1,14,18]` and
  the `F8` loop bound identical on all four.
- `center_arrows_single`: style-cluster → entry derivation lands at
  cluster−0x1DC on all four; `player_array_anchor`'s 4 matches per build
  decode to one global.
- `music_wheel_song_length`: the old `SpriteLayer` ctor omits two field
  inits but the layout vfunc reads the same offsets and the mod writes them
  all itself.
- `pacemaker_swap`: exactly two 0.5f RIP loads in the window on all four;
  `NoteResultActor+0xC0` attested on 20250805 too.
- `song_rate` clock anchor (`_v1` window +0x23, literal 8-byte
  re-execution check), io-callback regsite, preview-restart vftables:
  byte-identical inputs, fail-closed validations hold.
- `fast_bootup`: every music-DB entry field (`+0x94..+0x1B4`) and actor
  accumulator offset the mod hardcodes appears at the same offsets in
  onUpdate on 20250805 and 20260721; `music_db_global` cross-checked
  against `find_music_by_mcode`'s own RIP load.
- `custom_options` lambda slots / row builder / textlayer trio: function
  pointers only; `row_builder_fn`'s `+0x228` parent-side field present on
  both generations.

Not verifiable from this data (anchor signatures byte-identical, so no
divergence to compare): renderer-object fields in `fill_hook` /
`guideline_hook` / `pass_rewrite` (`posY@+0x34`, reverse flags,
`shaderObj+4`, `JudgeEffectRenderer+0x98`), `SpriteLayer` internals beyond
the ctor, `data_feed`'s `note+0x08`. These were validated on 20250805 /
20260224 by the field reports that drove commit `475d735` where they
appear in the reported mods; anything else remains cabinet-validation
territory.

---

## 5. Re-running the sweep

1. Drop the module set under one directory (`gamemdx_<build>.dll` ×N,
   `arkmdxbio2_<build>.dll` ×N, ONE `libafp-win64*.dll`). Sub-directories
   named per build with a bare `gamemdx.dll` also work.
2. `./scripts/validate_signatures.sh <dir> --json /tmp/sweep.json`
3. `python3 scripts/sig_harness/shape_diff.py --json /tmp/sweep.json --dir <dir> [--verbose]`
4. For each divergent signature whose divergence offset is inside the
   function body, read the consumer(s) listed and compare against the
   offsets they touch. `--verbose` prints the per-build disassembly around
   the divergence.

When adding a new derivation that publishes a NON-address (an offset, a
vslot, a kind id), use `SignatureStore::publish_value` so it appears in
the boot log and the report as `name (derived) = 0x…`.
