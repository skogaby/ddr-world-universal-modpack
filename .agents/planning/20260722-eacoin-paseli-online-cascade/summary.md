# Summary — `eacoin-paseli-online-cascade`

> **SUPERSEDED / REMOVED (2026-07-22, later same day).** This fix was implemented,
> validated working on CrossOver, and committed — but then found redundant: spice2x's
> **`-icmphook`** flag fakes the raw-ICMP keepalive game-agnostically at the socket layer,
> so DDR World boots fully online *including PASELI* with no hook DLL injected. The
> in-process `ea3_get_status` DOWN→ONLINE promotion was therefore **removed** from
> `src/mods/non_native_os_support.rs` (along with the sibling `arkGetNetworkStatus`
> network-status fix). This directory is retained as the RE record of the EACoin/PASELI
> gate chain. **Use `-icmphook` instead.**

## What this is

PDD spec + implementation for **sub-fix (c)** of the Non-Native OS Support mod: make
**PASELI (EACoin)** work under CrossOver/Wine. It hooks libavs `ea3_get_status` and
promotes its **network-down (1) → online (3)** verdict, so the AVS eamuse online state
*appears online* and cascades to the EACoin subsystem (which the existing boot-ONLINE
fix, sub-fix (a), never reached).

## Root cause (one paragraph)

PASELI == Konami "EACoin". The whole game gate is `arkEACoinGetStatus() == 0`
(available). That chains arkmdxbio2 → ess → libavs `eacoin_get_status`, which only reports
available when `ea3_get_status() == 3` ("online"). `ea3_get_status` returns "online" only
if the network-up flag (`status+0x50`, set by AVS keepalive) is set — and under Wine
keepalive can't create the raw ICMP socket, so it stays 0 → `ea3_get_status` returns 1
("network down") → PASELI never offered. Same raw-socket root cause as the boot-ONLINE
issue, but a deeper layer than the `arkGetNetworkStatus` hook (a) touches.

## Fix (one paragraph)

`retour::GenericDetour` on libavs-ea3 `ea3_get_status` (resolved by AOB scan of
`libavs-win64-ea3.dll`, since exports are name-obfuscated with unstable ordinals); the
detour calls the original and remaps return **1 → 3**, forwarding the two out-params and
all other states unchanged. This cascades: `eacoin_get_status` → available → game offers
PASELI → per-card `eacoin.checkin`/`eacoin.consume` (HTTP xrpc, already working under Wine)
fire. The readiness gate `DAT_1800958f8` is armed locally by the eacoin worker thread
(not network-gated), so the single hook is sufficient. Fail-open, config-gated, installed
independently of sub-fixes (a)/(b) in the same mod.

## Artifacts

```
20260722-eacoin-paseli-online-cascade/
├── rough-idea.md              # concept + full RE record (chain, addresses, mechanism)
├── design/detailed-design.md  # requirements, architecture, components, ABI/AOB, testing
├── implementation/plan.md     # 4-step checklist
├── progress.md                # live resume point
└── summary.md                 # this file
```

## Validation

Build gates (`cargo check` → `cargo fmt` → `./build.sh`) then live CrossOver deploy:
PASELI shows available at the entry screen (one-shot `DOWN(1)->ONLINE(3)` log), then a
real PASELI consume succeeds end-to-end. If consume fails, fall back to the deeper
`XEyy2igh00001b` network-up hook (documented in rough-idea Open Questions).

## Relationship to prior work

- Extends `src/mods/non_native_os_support.rs` (sub-fixes (a) network-status promotion,
  (b) movie-graph stub). (c) is the PASELI analog of (a), one layer deeper.
- Likely also makes `arkGetNetworkStatus` report ONLINE (arkmdx derives from
  `ea3_get_status==3`), so it may subsume (a) later — kept separate for now (a is proven).
