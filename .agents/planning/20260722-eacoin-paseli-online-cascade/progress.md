# Progress — `eacoin-paseli-online-cascade` (Non-Native OS Support sub-fix (c))

Updated: 2026-07-22
Status: SUPERSEDED / REMOVED — the fix worked and was committed, but is redundant with
spice2x's `-icmphook` (fakes the raw-ICMP keepalive game-agnostically → game boots fully
online incl. PASELI with no DLL). The in-process `ea3_get_status` promotion has been
removed from `src/mods/non_native_os_support.rs` (fix-forward commit). This dir is kept as
the RE record only.
NEXT ACTION: none. Use spice2x `-icmphook` for PASELI/online under CrossOver.

Resume protocol: read `implementation/plan.md` (4-step checklist),
`design/detailed-design.md` (hook target/ABI/AOB + cascade), `rough-idea.md` (RE record).

## Checklist

- [x] Step 1 — `resolve_libavs_ea3_module()` in `src/core/module_resolver.rs`
- [x] Step 2 — resolve `ea3_get_status` (AOB) in the mod's `init`
- [x] Step 3 — install promote-1→3 detour in `enable`, teardown in `disable`, `is_active`
- [x] Step 4 — docs (README, AGENTS.md)
- [x] Build gates: `cargo check` (exit 0) → `cargo fmt --check` (clean) → `./build.sh` (exit 0)
- [x] Cabinet/CrossOver validation — PASELI usable (confirmed by maintainer)

## Done

- **Step 1** — `src/core/module_resolver.rs`: added `LIBAVS_EA3_DLL_NAMES` +
  `resolve_libavs_ea3_module()` (mirrors `resolve_ark_module`).
- **Steps 2–3** — `src/mods/non_native_os_support.rs`: sub-fix (c). `Ea3GetStatusFn` type,
  `EA3_NETWORK_DOWN`/`EA3_ONLINE` consts, `EA3_GET_STATUS_SIG` AOB, `EA3_STATUS_HOOK` +
  `EA3_STATUS_ONE_SHOT`, `ea3_status_detour` (call original → promote 1→3, one-shot log,
  panic-guarded), `resolve_ea3_status_target()` (AOB scan of libavs-ea3). Wired into the
  struct (`ea3_status_target`), `init` (resolve), `enable` (install, count), `remove_hooks`
  (teardown + latch reset), `is_active` (union), plus module-doc header, `description()`,
  and `disable()` log. Landed in one pass (small, self-contained); each increment distinct.
- **Step 4** — README Non-Native OS Support row (Two→Three sub-fixes + (c) PASELI/EACoin);
  AGENTS.md Key Entry Points row (title + (c) description + planning-dir ref).
- **Build gates** — `cargo check --target x86_64-pc-windows-msvc` exit 0
  (`/tmp/eacoin_check.log`); `cargo fmt --check` clean; `./build.sh` exit 0 →
  `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll` (`/tmp/eacoin_build.log`).

## Deploy & test log

- **2026-07-22 — CrossOver (macOS), mod ON:** ✅ PASS. PASELI-only features are usable when
  booting under CrossOver (previously PASELI was not offered). Confirms the diagnosis end to
  end: `ea3_get_status` reporting "network down" was the sole PASELI blocker, and promoting
  it DOWN(1)→ONLINE(3) cascades to a working EACoin subsystem — including consume (the
  checkin/consume HTTP path works under Wine once the online gate is satisfied). The deeper
  `XEyy2igh00001b` fallback was not needed.

## Deviations & open questions

- Uncommitted (maintainer commits themselves). Exclude the pre-existing unrelated tree
  change `scripts/game_nav/login.sh` (a rename already present before this feature) from
  any commit of this work.
- Consume-safety is the one item static analysis can't fully close; validated live per the
  repo's model. Fallback documented (deeper `XEyy2igh00001b` hook) if needed.
- Sub-fix (a) intentionally left in place (not consolidated) until (c) is validated.

## Key facts for a cold resume

- **Root cause:** libavs `ea3_get_status` (`XEyy2igh00000b` @ 0x18000b980 on 20260721)
  returns 1 ("network down") under CrossOver because the keepalive-driven network-up flag
  (`status+0x50`, read via `XEyy2igh00001b` @ 0x1800100e0) is never set (raw ICMP sockets
  unavailable). `eacoin_get_status` (0x18001bdd0) needs `ea3_get_status()==3`, so PASELI is
  never offered. Same raw-socket root cause as sub-fix (a); a *deeper* layer (a) never
  reached.
- **Fix:** `GenericDetour` on `ea3_get_status`, promote return **1→3** (all other states
  pass through). Resolve by AOB scan of `libavs-win64-ea3.dll`
  (`48 83 EC 58 49 89 C9 0F B7 05 ?? ?? ?? ?? 85 C0 74 15 33 C0 48 8D 15 ?? ?? ?? ?? 48 89
  15 ?? ?? ?? ?? 48 83 C4 58 C3`). Sub-fix (c) of `src/mods/non_native_os_support.rs`;
  fail-open, config-gated, independent of (a)/(b).
- **Why sufficient:** readiness flag `DAT_1800958f8` is armed locally by the eacoin thread
  (not network-gated); checkin/consume are HTTP xrpc that already work under Wine. So
  online→cascade→PASELI available→consume works.
- **Fallback if consume fails live:** hook the deeper network-up reader `XEyy2igh00001b`
  (force out-param) instead.
- ABI: `unsafe extern "C" fn(*mut u32, *mut u32) -> u32` (two optional out-params
  forwarded; only return value remapped).
