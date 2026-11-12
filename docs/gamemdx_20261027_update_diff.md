# gamemdx 20260915 → 20261027 update diff

RE notes for the 2026-10-27 `gamemdx.dll` update, answering two questions:
(1) did the cabinet's default timing-offset tables change, and (2) what else
changed in the binary. Addresses are file-relative to `0x180000000`.

Method: Ghidra on both builds for the timing tables; the cross-build function
matcher in `scripts/build_diff/` (Ghidra inventory → position-independent
fingerprints → old/new pairing → normalised disassembly diff) for everything
else. See "Tooling" at the end.

---

## 1. Default timing-offset tables — UNCHANGED

Both builds carry byte-identical timing data. Nothing about the stock timing
presets, the preset selector, or the display-type deltas moved between
20260915 and 20261027. The only timing-related change is *when* the game
re-publishes the values (§2.2) — which is also Konami's "rev-C cabinets" fix —
not the values.

### 1.1 The ten presets (record builder)

`FUN_1800134f0` (20261027) / `FUN_180013950` (20260915) build ten `0x14`-byte
records on the stack and copy record `presetIndex` (clamped `0..9`, out of
range → record 0) to the caller. Records 0, 4 and 8 come from three 16-byte
`.rdata` constants via `MOVDQA`; the others are inline `MOV imm32`. The
`HIGH_PRECISION_INPUT` byte is written inline as `1` for every record.

| preset | SOUND_OFFSET | INPUT_OFFSET | RENDER_OFFSET | BOMB_FRAME_OFFSET | HIGH_PRECISION_INPUT | source |
|---:|---:|---:|---:|---:|---:|---|
| 0 | 87 | 28 | 17 | 0 | 1 | `.rdata` A |
| 1 | 87 | 28 | 36 | 1 | 1 | inline |
| 2 | 124 | 28 | 36 | 2 | 1 | inline |
| 3 | 105 | 28 | 17 | 0 | 1 | inline |
| 4 | 105 | 28 | 36 | 1 | 1 | `.rdata` B |
| 5 | 105 | 28 | 36 | 2 | 1 | inline |
| 6 | 105 | 28 | 15 | 1 | 1 | inline |
| 7 | 105 | 28 | 15 | 1 | 1 | inline |
| 8 | 90 | 32 | 13 | 1 | 1 | `.rdata` C |
| 9 | 105 | 28 | 30 | 2 | 1 | inline |

`.rdata` constants (A, B, C are contiguous, 48 bytes):

| build | A (rec 0) | B (rec 4) | C (rec 8) | bytes |
|---|---|---|---|---|
| 20260915 | `0x35b990` | `0x35b9a0` | `0x35b9b0` | `57 00 00 00 1C 00 00 00 11 00 00 00 00 00 00 00 \| 69 00 00 00 1C 00 00 00 24 00 00 00 01 00 00 00 \| 5A 00 00 00 20 00 00 00 0D 00 00 00 01 00 00 00` |
| 20261027 | `0x35b9a0` | `0x35b9b0` | `0x35b9c0` | identical |

Earlier builds for reference (`docs/hex_edit_porting.md` Hack 4): 20260324
`.rdata` A at `0x358960`, 20260526 at `0x35a950` — same bytes.

### 1.2 Preset selection

`FUN_1800133f0` (20261027) / `FUN_180013850` (20260915), normalised-identical.
Reads two machine-type values through function pointers
(`DAT_1806f3348` / `DAT_1806f3350` on 20261027) — call them `a` and `b`:

| `a` | `b` | preset |
|---|---|---|
| 0–1 | 0–1 | 0 |
| 0–1 | 2 | 3 |
| 2 | 0–1 | 1 |
| 2 | 2 | 4 |
| 3 | 2 | 5 |
| 4 | 2 | 6 |
| 4 | 3 | 7 |
| 4 | 4 | 8 |
| anything else | | 2 |

`Timing Init` (`FUN_18002bba0` / `FUN_18002c7c0`) overrides the selector with
preset **9** when the soft-id (via `DAT_1806f3460`, 32-byte buffer) matches one
of `MDX:A:F:B`, `MDX:Y:F:B`, `MDX:E:K:B` (`.rdata` `0x35d278` / `0x35d268`,
10-byte stride, 9-byte compare) — unchanged from
`docs/soft_id_rev_override_research.md`. It then publishes the four ints via
the config-map int setter (`FUN_1801af050` / `FUN_1801aede0`) and the bool via
`FUN_1801aefb0` / `FUN_1801aed40`, and logs `"Timing Init: %d"`.

### 1.3 Display-type delta (applied on top of the preset)

The `scene_transition` publish site (§2.2) re-reads the preset and adds a
per-`display_type` delta table (4 records × 16 bytes, `display_type` clamped
`0..3`) before publishing:

| display_type | ΔSOUND | ΔINPUT | ΔRENDER | ΔBOMB | `.rdata` 20260915 → 20261027 |
|---:|---:|---:|---:|---:|---|
| 0, 1, 2 | 0 | 0 | 0 | 0 | `0x35b940` → `0x35b950` (same 16 bytes used 3×) |
| 3 | 0 | 0 | **+27** | 0 | `0x35ba80` → `0x35ba90` |

Unchanged between builds.

---

## 2. What else changed (build diff)

Matcher summary (`scripts/build_diff/run_build_diff.sh 20260915 20261027`):
18,155 → 18,186 participating functions; after reconciling identical
duplicate stubs, **30 functions have a different instruction shape, 37 have
the same shape but different referenced data, 24 are new and 9 were
removed** (SEH funclets excluded). Of the 30 shape changes, 15 are relocation
noise the fingerprinter does not mask (`[reg + RVA]` displacements — see
Tooling) and diff to zero instructions. Signature sweep: 609 names resolve on
both builds; **`build_ssq_path` is the only name lost on 20261027** (it is
the function in §2.1). Matcher precision against the sweep: 255/257 pairs.

### 2.1 `build_ssq_path` rewritten: static table → disk probe + cache  ⚠ modpack impact

Old (`FUN_1801b3c70` @ `0x1b3c70`, 1984 bytes): the hardcoded per-build song
table documented in `docs/split_ssq_research.md`. **Removed.**

New (`FUN_1801b4590` @ `0x1b4590`, 723 bytes + helpers `0x1b6610`, `0x1b6b00`,
`0x1b7500`, `0x1b7f90`, `0x1b8580`, `0x1b8a00`, `0x1b92f0`; same callers:
`0x32310`, `0x577e0` (`dps_skin_table_read`), `0x61960`, `0x1e0a40`):

```c
void build_ssq_path(char out[0x100], const char* basename, int d) {
    static std::map<std::string, Entry> cache;       // lazily constructed, atexit-destroyed
    std::string b = basename ? basename : "";
    Entry* e = cache.find(b);
    uint8_t bits;
    if (e == end) {
        bits = 0;
        for (int i = 0; i < 5; i++) {               // probe _1.ssq .. _5.ssq
            snprintf(tmp, 0x100, "%s%s_%c.ssq", "data/mdb_apx/ssq/", b, '1' + i);
            if (avs_fs_lstat(tmp, &st) != 0) bits |= 1 << i;   // Ordinal_100
        }
        cache.emplace(b, bits);
    } else bits = e->bits;                          // Entry +0x40
    if (0 <= d && d < 5)
        for (int n = d; n >= 0; n--)
            if (bits & (1 << n)) { snprintf(out, 0x100, "%s%s_%c.ssq", "data/mdb_apx/ssq/", b, '1' + n); return; }
    snprintf(out, 0x100, "%s%s.ssq", "data/mdb_apx/ssq/", b);
}
```

i.e. **the stock game now implements exactly Rule A** from
`src/mods/split_ssq_auto_discovery/resolver.rs` ("highest `N ≤ d+1` whose
`<b>_N.ssq` exists, else `<b>.ssq`"), with a per-basename cache and an
`avs_fs_lstat` probe (which LayeredFS already redirects).

Consequences for the DLL on 20261027:

- `signatures.rs::build_ssq_path` (pattern on the old table-walker) does not
  match. `services::ssq_path::init` logs its WARN and `is_available()` is
  false; `split_ssq_auto_discovery` and `stepmania::ssq_split` refuse to
  register (graceful, already coded that way). Nothing crashes.
- Behaviourally the stock code now does what `split_ssq_auto_discovery` did,
  so that mod is redundant on this build *as long as the files are visible to
  `avs_fs_lstat`* (LayeredFS mounts are). StepMania-injected basenames get
  the same treatment for free.
- What is lost without the detour: the divergence oracle, the priority chain
  (a consumer can no longer force a choice that disagrees with the disk), and
  the **cache** is new — the first probe per basename is latched for the
  process lifetime, so a file that appears after first use (enable-time
  `*_ifs/` output, late pack indexing) will not be seen until restart.
- If a hook is still wanted: the new function is anchored by the format
  strings `"%s%s_%c.ssq"` / `"%s%s.ssq"` / `"data/mdb_apx/ssq/"` (all three
  referenced from `0x1b4590` only) and the `std::map` static at
  `DAT_180cf44b0` / guard `DAT_181238488`. Signature, prologue and the
  `(out, basename, d)` ABI are unchanged, so the existing detour type fits.

### 2.2 Timing re-publish moved from scene 26 to scene 4 — the "`***:*:*:C` cabinets" fix

Konami's release note for this update lists
「ソフトバージョンが「***:*:*:C」で始まる筐体で稀に意図しない挙動が起こる症状を軽減」
(mitigates rare unintended behaviour on cabinets whose soft version starts
`***:*:*:C` — the rev-C Universal/Platinum hardware). gamemdx has **no** code
that compares the rev letter other than the `MDX:A:F:B / Y:F:B / E:K:B`
preset-9 override (all nine `arkGetSoftIDCode` readers are unchanged — slot
`0x6f2440` → `0x6f3460`), so the fix is keyed on hardware probes instead, and
there is exactly one changed block gated on them:

`scene_transition` (`FUN_18002ed10` → `FUN_18002e240`; sigs
`scene_transition`, `gameplay_loader_masks`, `premium_free_stage_inc`)
contains a block guarded by `FUN_180013b00()` → `FUN_1800136a0()`, which is
`timing_preset() ∈ {6, 7, 8}` ⇔ `arkMDXGetMachineType() == 4` (with
`arkMDXGetPCType()` 2/3/4) — see §1.2. In `arkmdxbio2` (20260915, unchanged
by this update) the machine type is a per-I/O-driver constant, not a live
probe: driver table `0x145de0` (stride `0x1f` slots) index 0 = p3io → 2,
index 1 = p4io → 2, index 2 = **bio2 → 4**, except that the bio2 getter
(`0x88e00`) returns **3** when the flag `DAT_180c47f69` is set; `dll_entry_init`
sets that flag (via driver slot 2, `0x88e20`) exactly on the paths where it
rewrites the ident to spec/rev `F:B` after a positive BIO2 USB probe
(`FUN_1800d1490`, `"BIO2(VIDEO)"` / `"USB Serial Device"`). So machine type 4
= BIO2 cabinets that are not the `F:B` retrofit class — the new rev-C
Universal hardware. `arkMDXGetPCType` (`0xcdf30`) is derived once from the
machine type + `GetSystemInfo` + `FUN_1800d1700` and cached at `+0x27fc`.
The block re-derives the preset, adds
the display-type delta (§1.3) and publishes the four `*_OFFSET` keys through
the config-map int setter:

| | 20260915 | 20261027 |
|---|---|---|
| switch arm | `case 0x1a` — scene 26 (SONG_TO_STAGE_INTERSTITIAL), i.e. **re-published before every song** | `case 4` — early boot/attract-range scene, i.e. **published once at boot** |
| display_type source | global `DAT_180cf3340`, filled by the options load in the `case 5/6/7` arm | fresh `(*DAT_1806f3680)("/gameOptions/display_type/current", &DAT_180cf4350)` immediately before use (the cached copy does not exist yet at scene 4; the `case 5/6/7` load is still there too) |
| guard | `FUN_180013b00() != 0` | `FUN_1800136a0() != 0` (same predicate, relocated) |

Nothing else on the consumer side moved: the `GamePlayActor` constructor's
four-key subscription site (`FUN_18005b6e0` → `FUN_18005b250`, sig
`audio_sync_offset_layout`) and `Timing Init` are shape-identical, and the
preset / delta data are unchanged (§1). So on a rev-C cabinet the published
values are the same as before; what changed is that they are no longer
re-written into the live config map (and re-broadcast to subscribers) in the
middle of every song-to-stage transition. The exact failure mode is not
recoverable statically — the plausible reading of 「稀に」 is a race between
that per-song re-publish and whoever consumes the subscription (audio/input
latch vs. GPA construction order), which a once-at-boot publish removes by
construction. Treat that as inference, not a finding.

For `mods/timing_offsets` this is neutral: the setter detour still sees the
boot publish, and `set_offset` pushes live values via `call_original` rather
than waiting for a republish. The `stock` capture now has exactly two
publishes to latch from (Timing Init, then scene 4), both pre-gameplay; and an
operator changing DISPLAY TYPE in the test menu no longer gets the +27 RENDER
delta re-applied until the next boot (stock behaviour, not ours).

### 2.3 New genre folder "AUDITION"

`folder_init` (`0x141780` → `0x141630`, +100 instructions) adds a folder
between `touhou` (kind 5) and `gamer` (kind 1): name `"audition"`, voice
`"vo_select_folder_hot_music"`, folder kind **9**, display id **0xb**.
Companion changes:

- `0x59b50` → `0x59600` (callee of `0x58200`, the DPS/ready screen): new test
  `(musicinfo+0x174) & 0x200` mapped to folder id `0xb` — music flag **bit 9**
  marks AUDITION songs (bit 5 / id 6 is the pre-existing analogue).
- `0x14680` → `0x14220` and `0x1d6dc0` → `0x1d82c0`: folder-count bounds
  `0xe→0xf` / `0x10→0x11` (one more folder slot).

Any mod that indexes folders by position after `touhou` must re-check its
assumptions on this build (`docs/folder_system_research.md`).

### 2.4 GALAXY BRAVE expansion

- `GalaxyBraveData::GetSelectedBraveID` (`0x1d9c90` → `0x1db190`): id loop
  bound `0x10 → 0x11` (16 → 17 braves).
- Filtersort category 4 grows from 3 to 4 entries: `filter_entry_count_table`
  (`0x1d55b0` → `0x1d6940`, `case 4` now returns 4 instead of sharing the
  `3` arm with cases 9/10), its consumer `0x124ef0` → `0x124c30`
  (`MOV EDX,3 → 4`), and the per-entry predicate `0x124dc0` → `0x124ad0`
  gains `case 3`: `music::Info` flags `+0x1ac` bit 26 (`>> 0x1a & 1`). The
  selectmusic lambda host `0x125010` → `0x124d50` (+69 instructions)
  references a new `"galaxy_brave"` string. `services::filter_entry_count`
  only overrides category 1 and forwards the rest, so it is unaffected.

### 2.5 Smaller content-count bumps

Seven leaf setters (`param+0xc … +0x2c` range validators, `0x1dfc40…0x1dfd70`
→ `0x1e0fe0…0x1e1110`; matched as removed/new because their bodies are the
constants) raise their maxima: `0x8b→0x96`, `0x4d→0x56` (×2), `0x5b→0x5d`,
`0x6c→0x6e`, `0x75→0x7c`, `0x88→0x8f` — new customisation/content ids. The
`0x186a1 + n` special-id windows are unchanged. `0x1dfce0` → `0x1e1080`:
`CMP 0x49 → 0x4b` (same family).

`0x7ec10` → `0x7e950` (attract `demo_root` loader): a modulus table grows
`0x29 → 0x2b` (41 → 43 entries; the magic-divide constant changes
accordingly).

### 2.6 Unidentified

`0x14bbd0` → `0x14bbc0` (1404 → 1444 instructions, pass-4 fuzzy match): large
selectmusic-side function with three new call sites (`FUN_1800fe100`, one more
`FUN_18021adc0`) and no string changes. Not characterised.

### 2.7 Everything else

- The remaining 15 "changed" pairs (e.g. `0x190c0`, `0x1c450`, `0x9d8d0`,
  `0x23a0a0`, the `ark::network` timeout family) diff to **zero instructions**
  once `[reg + RVA]` displacements are masked — relocation only.
- 37 "same shape, different refs" pairs with all callees unchanged: tiny
  wrappers whose referenced constant/string/vftable moved or changed; none
  carries a modpack signature except `afp_layer_init_wrapper` /
  `cmovieclip_create` (`0x26e7d0` → `0x26f4b0`) and `ddr_sel_sprite_create`
  (`0x272f30` → `0x273d20`), both of which still resolve. Not inspected
  individually.
- `0x21f380` (FNV-1a over a byte range) removed — inlined; `0x4eab0` new —
  two-field destructor wrapper; `0x2d75f0` new — the `atexit` destructor of
  §2.1's cache.
- `__report_gsfailure` / `__C_specific_handler` show as refs-changed (CRT
  data moved).

---

## 3. Tooling — `scripts/build_diff/`

Copied from the sibling `dance-dance-rustolution-world` repo
(`scripts/build_diff.py`, `tools/GhidraExportInventory.java`,
`tools/GhidraDecompile.java`; the exporter now takes its output path as a
script argument) plus local glue. That repo is read-only from here: `pescan`
is built out of it with `CARGO_TARGET_DIR` pointing into the scratch dir.

| file | role |
|---|---|
| `run_build_diff.sh OLD NEW [modules-dir]` | whole pipeline → `$BUILD_DIFF_WORK/report_OLD__NEW.md` |
| `GhidraExportInventory.java` | headless: functions, bodies, callees → JSON |
| `build_diff.py` | the matcher (fp_deep → fp_shape → call-graph propagation → MinHash), validated against the signature sweep |
| `report_changes.py` | Markdown report; reconciles duplicate-stub leftovers, splits refs-changes into direct vs callee-induced, attaches Ghidra names + signature names |
| `asm_diff.py` | per-pair normalised disassembly diff (`GhidraDecompile.java` under the hood) — the "what changed inside" step |

Requirements: both programs analysed in the `DDRWorld_Ghidra` project and the
GUI closed; a stale `.lock` from a closed GUI does not block headless. All
outputs are binary-derived and stay in the scratch dir.

Known limitation (upstream, not fixed here): pescan masks RIP-relative
operands and absolute addresses but not MSVC's image-base-register idiom
(`lea rsi,[image_base]; mov rax,[rsi+rcx*8+0x6f2ed0]`), so a function whose
referenced tables merely moved is reported `changed`. `asm_diff.py` masks
those and labels such pairs "relocation-only" — half the `changed` set on
this update. Worth a `Mask::Rva`-style rule in `pescan/src/fingerprint.rs`
if the sibling repo's maintainer wants it.
