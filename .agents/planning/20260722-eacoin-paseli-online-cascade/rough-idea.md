# Rough Idea — `eacoin-paseli-online-cascade` (Non-Native OS Support sub-fix (c))

## One-liner

Under CrossOver/Wine, DDR World won't offer **PASELI** as a payment option even though
the same client + server allow it under real Windows. Root cause: the AVS eamuse layer's
own **network-up** determination (keepalive / raw ICMP sockets) reports the network as
**down**, so `ea3_get_status()` never reaches `ONLINE(3)`, and the EACoin (PASELI)
subsystem therefore reports **not-available**. Fix: hook libavs `ea3_get_status` and
promote its "network down" verdict to ONLINE, so the online state cascades to EACoin
(and the rest of the eamuse layer) and PASELI initializes normally.

This is the same raw-socket root cause as the boot-ONLINE issue (sub-fix **(a)**,
`arkGetNetworkStatus` CHECKING→ONLINE), but at a **deeper layer** that sub-fix (a) never
reaches — hence PASELI stayed broken after (a) shipped.

## Symptom

- macOS + CrossOver, spice2x, private server (bemani-buddy). Boot reaches ONLINE (sub-fix
  (a) active), profile/score traffic flows — but **PASELI is not offered / not usable** as
  a payment method at the entry screen.
- The **identical** game client + server offer PASELI normally when booted on real Windows.

## Investigation record (2026-07-22, Ghidra)

Binaries: `gamemdx_20260721.dll`, `arkmdxbio2_20260721.dll`, `ess_20260324.dll`,
`libavs-win64-ea3_20260721.dll` (the deployed CrossOver-bottle set + ess proxy).

### PASELI == EACoin, and the whole game gate is one value

"EACoin" (e-Amusement Coin) is Konami's internal name for PASELI. The gamemdx
`arkmdxbio2` import binder (`FUN_1800042f0`) resolves the family
`arkEACoinGetStatus/QueryBalance/Consume/QuerySessionState/…` — the PASELI API.

Every place the game decides whether to **offer/allow PASELI** keys off a single value:
`arkEACoinGetStatus(&s)` returning **0 = available**, `1 = NOT AVAILABLE`, `2 = hidden`
(clamped to `[0,2]`). Confirmed readers (gamemdx_20260721):

| Site | Role |
|------|------|
| `FUN_180009660` | bottom-bar status: 0→balance, 1→"PASELI: NOT AVAILABLE", 2→blank |
| `FUN_180081160` | entry screen; `paseli_status_usr`. **case 3 requires network==ONLINE AND eacoin==0** |
| `FUN_1800ae780` | mode-select; per-side payment/decision availability flags |
| `FUN_18001f890` | small `int isPaseliAvailable()`-style predicate (clamps to [0,2]) |
| `FUN_180014410` | test/network reflect (case 0x65) — same 0/1/2 handling |

So: **PASELI is offered iff `arkEACoinGetStatus() == 0`.**

### The chain down to the deciding value

```
gamemdx:  arkEACoinGetStatus            (arkmdxbio2!export @ 0x180039f70)
   └─ arkmdx-level enable gate DAT_180d4cd14 (0 → returns 2 "disabled"); else:
ess:      ess_eacoin_get_status         (0x18000ed50)
   └─ ESSeACoinAVS::getStatus  (vtable 0x18006b558 +8 → thunk 0x1800349f0 → import XEyy2igh00003d)
libavs-ea3: eacoin_get_status           (XEyy2igh00003d @ 0x18001bdd0)
```

`eacoin_get_status` reaches its "available (0)" result only through the branch guarded by:

```
ea3_get_status() == 3   ("online")
```

`ea3_get_status` = **`XEyy2igh00000b` @ 0x18000b980** (Ordinal_12). Its return enum:
`0 booting, 1 (resume / network down / can't-get-status), 2 boot-code-not-ok, 3 online,
4 maintenance, 5 runtime error`. The path to **3 ("online.")** is:

```
ea3_get_status → XEyy2igh00001b (0x1800100e0, network-status reader)
   reads status_obj + 0x50  (network-UP flag, populated by keepalive)
   if +0x50 == 0  →  ea3_get_status returns 1  ("network down.")
   else           →  ea3_get_status returns 3  ("online.")
```

Maintenance(4)/boot(0,2)/error(5) are decided **before** the network-up read, so they are
independent of it.

### Why it fails under CrossOver (root cause)

The `+0x50` network-up flag is set by AVS **keepalive**, which needs a raw ICMP socket.
Under Wine that fails (`avs_net_socket 0x8008000d` → `KEEPALIVE IS DISABLE`), so `+0x50`
stays 0 → `ea3_get_status()` returns **1 ("network down")** → `eacoin_get_status()` never
reaches available → PASELI is not offered. On real Windows keepalive works → `+0x50` set →
online → PASELI available. **Same raw-socket root cause as the boot-ONLINE issue.**

### Why sub-fix (a) did NOT cover PASELI

Sub-fix (a) hooks `arkmdxbio2!arkGetNetworkStatus` — a value at the **arkmdxbio2** layer.
But `eacoin_get_status` calls **libavs `ea3_get_status` directly**, underneath arkmdxbio2.
Two independent readers of the same broken keepalive signal; (a) only fixed the arkmdx one.

### Why forcing the status is safe (consume works)

- The actual `eacoin.checkin` / `eacoin.consume` are **HTTP xrpc** requests driven by
  libavs's `eacoin_thread` (`FUN_1800210b0`, a generic request-dispatch worker). **HTTP
  works under CrossOver** (the RE record for (a) proved the full eamuse HTTP handshake is
  byte-identical and successful). So once the system-level online gate is satisfied, the
  per-card checkin/consume ride the HTTP path that already works.
- The EACoin **readiness** flag `DAT_1800958f8` (that `eacoin_get_status` also checks
  before the online check) is **not** network-gated: the eacoin worker thread sets
  `*(param_1+0x98)=1` where `param_1 = &DAT_180095860`, and `0x180095860+0x98 =
  0x1800958f8` — armed once the eacoin module is booted (server sent `eacoin/enable`,
  `ea3_eacoin_boot` ran). So under CrossOver readiness is already set; `eacoin_get_status`
  reaches the `ea3_get_status` check, and the **only** divergence from Windows is
  `ea3_get_status != 3`.

∴ Making `ea3_get_status` report ONLINE is the complete root fix; the rest (readiness,
checkin, consume) either is already satisfied locally or rides working HTTP.

## Proposed approach

Add **sub-fix (c)** to `src/mods/non_native_os_support.rs`: a `retour::GenericDetour` on
libavs-ea3 `ea3_get_status` that **promotes return 1 → 3** (network-down → online),
passing every other value through unchanged (preserves boot/maintenance/error). This
cascades: `eacoin_get_status` → available → game offers PASELI → card-in checkin / pay
consume ride HTTP → work.

- **Resolution:** libavs exports are name-obfuscated (`XEyy2igh…`) with unstable ordinals,
  so resolve by **AOB scan** of the `libavs-win64-ea3.dll` module (via a new
  `module_resolver::resolve_libavs_ea3_module()` + `scanner::scan_pattern`). Prologue AOB
  (wildcarding the three RIP disp32s):
  `48 83 EC 58 49 89 C9 0F B7 05 ?? ?? ?? ?? 85 C0 74 15 33 C0 48 8D 15 ?? ?? ?? ?? 48 89 15 ?? ?? ?? ?? 48 83 C4 58 C3`
- **Fail-open + config-gated** exactly like (a)/(b): self-disables if the module/AOB can't
  be resolved or the detour can't install; `is_active()` unions all sub-fixes.

## Open questions / to confirm on the cabinet (this repo's validation model)

- PASELI shows **available** at the entry screen with the mod on (primary acceptance).
- An actual PASELI **consume** succeeds end-to-end (validates the checkin cascade). If the
  initial checkin turns out to poll a different online signal than `ea3_get_status`, the
  fallback is the deeper hook on `XEyy2igh00001b` (force the network-up out-param).
- Bonus/possible consolidation: promoting `ea3_get_status`→3 likely also makes
  `arkGetNetworkStatus` report ONLINE (arkmdx derives from `ea3_get_status==3`), so it may
  subsume sub-fix (a). Keep (a) initially (it is proven); consider consolidating after (c)
  is validated.

## Deviation notes vs the reflex "surface" fix

Hooking `arkEACoinGetStatus` (arkmdxbio2) and forcing `0` was the obvious surface fix, but
it would only satisfy the game's display/offer gate — it would NOT make the eacoin
subsystem believe the network is online, so a genuine `checkin`/`consume` might not fire.
The deeper `ea3_get_status` hook makes the online state cascade so EACoin initializes
properly (the maintainer explicitly chose the root fix).
