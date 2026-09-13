# Software-identity (soft-id code) rev override — research notes

Date: 2026-09-13. Builds: gamemdx 20260825 (+ 20260224 / 20250805 / 20260721
sibling arks), arkmdxbio2 20260721, ess 20260324, libavs-win64 /
libavs-win64-ea3 **2.16.3 r7106** (both ea3 DLLs in the reference set are
byte-identical at the sites below), spice2x source (`avs/ea3.cpp`,
`avs/core.cpp`, `launcher/launcher.cpp`). Addresses are file-relative to each
module's `0x180000000` base.

Implementation: `src/services/ident_override.rs` (always-on service, lib.rs
step 0a). This document records WHERE the identity flows, WHO reads the rev
letter, and therefore what changing it does.

## 1. The identity string and its five copies

`prop/ea3-ident.xml` → `<ea3_conf><soft>{model,dest,spec,rev,ext}`. The soft-id
code is `MODEL:DEST:SPEC:REV:EXT`, e.g. `MDX:J:F:A:2026082500`. Char indices:
`[0..2]` model, `[4]` dest, `[6]` spec, `[8]` rev, `[10..19]` ext. The
**security code** `G*MDXJFA` (spice2x fakes the dongle's `G?` prefix) has the
same four letters at `[2..7]`, rev at `[7]`.

spice2x's `avs::ea3::boot` (after loading `-k` hook DLLs, and after `-z` early
hooks even earlier — see §4) does, in order:

1. `config_read("prop/ea3-ident.xml")` — **its own CRT `fopen`/`fread`**
   feeding `property_read_query_memsize` / `property_insert_read`. The AVS
   fs API is NOT used, so the modpack's LayeredFS hooks never see this read
   (and `mod_paths::normalise_path` rejects `prop/` anyway).
2. `avs_std_setenv("/env/profile/security_code", "G*MDXJFA")`,
   `("/env/profile/soft_id_code", "MDX:J:F:A:2026082500")` (pre-init copies).
3. `dll_entry_init("MDXJFA2026082500", app_param)` on the game DLL
   (`arkmdxbio2.dll`). For MDX spice2x does NOT re-read the init code
   afterwards (it only does for LDJ/L44/T44/M39/KFC).
4. Rebuilds `/ea3/soft/{model,dest,spec,rev,ext}` in the ea3-config property
   from its `EA3_*` buffers, logs `soft id code:`, re-`setenv`s
   `/env/profile/soft_id_code`, then calls **`ea3_boot(/ea3 node)`** via the
   export pointer it captured at `avs::ea3::load_dll()`.

`ea3_boot` = `XEyy2igh000007` @ `libavs-win64-ea3+0x9050` (2.16.3):

- `FUN_1800096a0(ea3_node, &g_config /*0x1800932a0, 0x1F8 bytes*/)` —
  `property_psmap_import` through the table @ `0x18008f0e0`: `soft/model` →
  `+0xC2` (4), `soft/dest` → `+0xC6`, `soft/spec` → `+0xCA`, `soft/rev` →
  `+0xCE`, `soft/ext` → `+0xD2` (16). Then `check_soft_id_code`
  (`FUN_1800099b0`): model = 3 alnum, dest/spec/rev = **exactly one uppercase
  letter** (`'A'..='Z'`, `FUN_180009d20`), ext = 8–11 chars digits (+ optional
  trailing `X`). Anything else is a FATAL `boot: bad rev`. So `X` is a legal rev.
- `FUN_180009ea0(&g_config)` — `setenv("/env/profile/soft_id_code",
  "%c%c%c:%c:%c:%c:%s")` composed from the struct: **this overwrites
  spice2x's value** (log line `ea3: setenv : /env/profile/soft_id_code=…`).
- `FUN_18000a150(&g_soft_id /*0x1800934b0, 0x20*/, …)` — `"%s:%s:%s:%s:%s"`
  composed string; `FUN_18000a250` → `ea3_new` → `xrpc_new` keeps a POINTER
  to it: every request's root `<call model="…">` attribute is written from
  `*(client+0x20)` in `FUN_18003ebc0` (`model@` / `srcid@` / `tag@`).
- `ea3-share` default rule `'MDX:J:F:A:-'` comes from the same struct.

Downstream game reads: `gamemdx` imports `arkGetSoftIDCode` from the ark
(`FUN_1800042c0` table, slot `DAT_1806f2448`); `arkmdxbio2!arkGetSoftIDCode`
(`0x3830`) = `ess_soft_info_get(1, buf)`; `ess!ess_soft_info_get(idx)`
(`0x16790`) = `avs_std_getenv(table[idx])` with `1 = /env/profile/soft_id_code`,
`0 = /env/profile/security_code`, 2..8 = system/hardware/license/software/
account ids. So **every game-side read is a live getenv of the value
`ea3_boot` wrote**, and all of them happen after boot (`dll_entry_main` /
scene code). `gamemdx!gameInit(hInstance, sidcode, app_param)` (`0x3e60`)
only logs the module handle — the init-code buffer has no game consumer;
the ark stores a pointer to it (`MainDllInfo+0x08`) that nothing reads.

Hence the single seam: rewrite `/ea3/soft/rev` in the property **before the
original `ea3_boot`** and every copy that matters (wire `model=`, env,
title screen, ea3-share) agrees. Not rewritten (no post-boot consumer):
spice2x's `avs::game::REV` / `soft id code:` log line / troubleshooter
"game version", the `security_code` env, the init-code buffer.

## 2. Who reads the REV letter (the safety question)

Method: every `ess_soft_info_get` caller in arkmdxbio2 (33) and every
`arkGetSoftIDCode` caller in gamemdx (8) decompiled; local-variable stack
offsets mapped back to string indices; plus a literal sweep of all three ark
builds and gamemdx for `MDX:?:?:?` strings of length ≥ 9.

### 2.1 gamemdx 20260825

| Site | Reads | Verdict |
|------|-------|---------|
| `FUN_180082f40` (title scene, `title_root`), `FUN_1800ad6b0`, `FUN_180084dd0` (`main_root`) | whole string → text widget | **display only** — this is the title/mode-select footer version string |
| `FUN_18001b9b0` / `FUN_18001bb90` / `FUN_1801de420` | cached copy `DAT_180cf34e8`, byte `+6` (spec) `== 'Z'` | spec only |
| `FUN_1801ba8f0` | `"%s"` + `"%s:FPV1"` → libavs-ea3 ordinals 98/100 (ea3-share client) | whole string forwarded (matching/pairing filter) — consistent after the override because it reads the post-boot env |
| **`FUN_18002bcd0` "Timing Init"** | `strncmp(soft_id, tbl[i], 9)` against `MDX:A:F:B`, `MDX:Y:F:B`, `MDX:E:K:B` (@ `0x18035d248`, 16-byte stride) → timing preset **9**; else `FUN_180013690()` machine-type preset | **REV-SENSITIVE** for those three idents only |

Preset table `FUN_180013790` (20-byte records `{sound, input, render, bomb,
flag}`): preset 9 = `{105, 28, 30, 2}`; the machine-type default for unknown
types (2) = `{124, 28, 36, 2}`. These are only the game's DEFAULTS — the
`timing_offsets` mod's setter hook overrides whichever keys it has configured.

### 2.2 arkmdxbio2 20260721 (all 33 `ess_soft_info_get` callers)

- **`FUN_1800172d0`** — `dest == 'J'` then `strncmp(soft_id, "MDX:J:I:{A,B,C}", 9)`
  → premium / "galaxy" pricing eligibility (gates coin-option item01/item02
  price limits in `FUN_18000fca0` and the boot-time
  `need_premium_setting` / `need_galaxy_setting` migrations in
  `FUN_18001fe10` `ArkDiagnosisMode::updateEAmusement`). **REV-SENSITIVE for
  J gold cabinets (spec I) only**; for spec F the compare already fails at
  index 6.
- **`FUN_180004810`** — `strncmp(soft_id, "TDX:U:J:C", 9) || (dest != 'E' &&
  (spec < 'J' || (spec > 'K' && spec != 'Z')))` selects
  `/prop/ark-config.xml` vs `/prop/ark-config_nopp.xml`. **REV-SENSITIVE for
  TDX:U:J only** (not an MDX ident).
- `arkMDXGetLicenceKeyVersion` (`0xd08b0`) — truncates at
  `strlen("MDX:*:*")` = 7 before its 7-char table compare (`MDX:J:{A,B,C,F,I,Z}`
  → 0, `MDX:A:*` → 3, …). Rev cut off.
- `FUN_180004720/17240/1d590/1dc90/eae0/11720/400b0/3ff40/5a380/5bdf0/65e90/
  66030/76690/35f10/c9130/c91e0/d1fb0`, `arkGetTargetAreaCode` — dest `[4]`
  and/or spec `[6]` only.
- `FUN_18001b960`, `FUN_180023a40`, `FUN_18007c440` (SYSTEM INFORMATION test
  page), `FUN_1800c4840` (QC MODE title label), `arkGetSerialNumber` — whole
  string displayed / forwarded, never compared.
- `FUN_18005d3b0`, `FUN_18005d800` — dead copies.
- **No caller uses index 0** (`security_code`) post-boot.

`dll_entry_init` (`0xd1670`) itself reads the security code directly
(`Ordinal_212` = `avs_std_getenv`) BEFORE ea3 boot: for dest `U`/`E` with spec
`B`/`C` the rev (A/B) picks a replacement spec letter (E/G/J/K) written into
the init-code buffer; `"GZMDXJGB"` / `"GCMDXEDC"` → `ZZ` / `JC`; everything
else (incl. `G*MDXJFA`) sets a flag consumed by `ess_network_opt_update`. The
override does not touch the security code, so this path is unchanged. The
rest of `dll_entry_init` rewrites the init code's spec/rev from hardware
probes (`io` param `bio2`/`p3io`/`p4io`, BIO2 USB probe `FUN_1800d0dd0`) —
e.g. `MDX:J:F:A` + `io=p4io` becomes spec `C` in that buffer — and then hands
it to `gameInit`, which ignores it.

### 2.3 libavs-ea3

Only `check_soft_id_code` (letter validity) and the two string composers
above. No branch on the letter's value.

### 2.4 Server side

- **bemani-buddy** (`crates/bemani-protocol/src/envelope.rs`
  `parse_model_string`): parses `rev`, no handler branches on it.
- **bemaniutils** `bemani/backend/ddr/base.py`: `self.omnimix = model.rev ==
  "X"` → `music_version = OMNIMIX_VERSION_BUMP + version`. A DDR game
  reporting rev `X` to a bemaniutils server is filed as an **omnimix** —
  scores/profile version keys move to the omnimix bucket (existing stock
  scores are not visible from it, new ones don't land in the stock bucket).
  That is the community convention `X` carries ("modified song data").
  **Decision (2026-09-13): the modpack reports `M`, not `X`**, precisely to
  stay out of that bucket — a modpack cabinet should keep the same score
  space as a stock one on every known server. No server in the reference set
  (bemani-buddy, bemaniutils) keys any behaviour on `M`; it is treated as an
  ordinary stock revision.

### 2.5 Conclusion

For the stock DDR World idents (`MDX:J:F:*`, and any `MDX:?:F/I/…` other
than the three rows below) the rev letter is **display + wire only**; any
single uppercase letter (incl. `M`) is a legal value; nothing in the game,
ark, ess or ea3 library changes behaviour. The known rev-sensitive stock
idents (each produces a boot WARN from `ident_override::rev_sensitive_reason`):

| Stock ident | What the override changes |
|-------------|---------------------------|
| `MDX:A:F:B`, `MDX:Y:F:B`, `MDX:E:K:B` | gamemdx default timing preset 9 → machine-type preset (moot with `timing_offsets` configured) |
| `MDX:J:I:{A,B,C}` | ark premium/galaxy pricing eligibility → ineligible |
| `TDX:U:J:C` | ark loads `ark-config_nopp.xml` instead of `ark-config.xml` |

## 3. Mechanism

- Resolve libavs-win64 **2.16.[3-8]** property exports by name
  (`XCnbrep70000a1` property_search, `…a2` property_node_create, `…a3`
  property_node_remove, `…ae` property_node_get_desc = node → owning
  property, `…af` property_node_refer), gated on the `XCnbrep700013c`
  marker LayeredFS uses (2.16.1 shares the prefix with different numbering).
  Ghidra-verified: search/create/refer accept a NULL `property` when a node is
  given; `node_remove` returns 0 on BOTH success and the not-removable error
  path, so success is verified by re-searching.
- Resolve `ea3_boot` by spice2x's per-version export names (2.16.3+
  `XEyy2igh000007`; 2.17 `XEmdwapa000024`; 2.16.1 `XEyy2igh000006` — where
  `…007` is `ea3_shutdown`!), each candidate VALIDATED by its prologue
  `LEA RCX,["ea3-boot"]` + `LEA RDX,["startup"]` within the first 0x80 bytes;
  AOB fallback `57 41 54 48 81 EC 58 02 00 00 48 89 CF E8 …` (2.16.3
  prologue) with the same validation.
- Detour: `search(soft)` → read model/dest/spec/ext/rev for the log →
  `node_create(property, soft, str, "rev", "M")` → `node_remove(old)` →
  `search("rev") == new` → read-back `"M"` → original. Any failure undoes the
  addition and boots stock (one WARN). Already-`M` files are a no-op INFO.
- Timing: installed as lib.rs step 0a before anything else; under `-z` the
  service waits for the two libavs modules (bounded by gamemdx appearing,
  which the launcher loads after them). A 30 s watchdog WARNs if `ea3_boot`
  was never observed.

## 4. Launcher order (spice2x `launcher.cpp`)

`-z` early hooks (1798) → `avs::core::load_dll` (2496) → `avs::ea3::load_dll`
(2497, captures the boot export pointer — an inline detour on the function
body still intercepts it) → `avs::core::boot` (2510) → `avs::game::load_dll`
(2524; DDR's `attach` also `LoadLibrary`s gamemdx) → `-k` hooks (2620) →
`patcher::apply_patches_on_start` → `avs::ea3::boot` (2652: ident read,
`dll_entry_init`, `ea3_boot`) → `avs::game::entry_main` (2744).
