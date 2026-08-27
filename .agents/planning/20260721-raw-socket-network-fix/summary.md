# Summary — `raw-socket-network-fix`

> **REMOVED (2026-07-22):** the in-process `arkGetNetworkStatus` CHECKING→ONLINE promotion
> described here has been deleted from `src/mods/non_native_os_support.rs`. spice2x's
> **`-icmphook`** flag fakes the raw-ICMP keepalive game-agnostically at the socket layer,
> so the game boots fully online (PASELI included) with no hook DLL — making this
> in-process fix (and its EACoin sibling) redundant. This directory is retained as the RE
> record of the boot network-check gate. **Use `-icmphook` instead.**

> **Superseded (same day, before release):** this mod was merged into the broader
> **`non-native-os-support`** mod (`src/mods/non_native_os_support.rs`, id
> `non-native-operating-system-support`) as its sub-fix (a), alongside a new
> background-movie DirectShow crash fix (sub-fix b). The hook design described
> here is unchanged inside the merged mod. See
> `.agents/planning/20260721-non-native-os-support/`.

> **Status: DONE** — implemented and validated on the Macbook + CrossOver install
> (2026-07-21): the game boots ONLINE and connects to the server. Build gates green.
> Uncommitted (maintainer commits themselves). See `progress.md` for the live record.

## What this is

A PDD spec for a new mod that lets DDR World boot **ONLINE** under CrossOver/Wine (and any
environment that can't create raw ICMP sockets). It hooks `arkmdxbio2!arkGetNetworkStatus`
and promotes the reported status **CHECKING(4) → ONLINE(5)**, unblocking the boot
network-check state machine that otherwise stalls ~30 s and falls back to offline.

## Artifacts created

```
.agents/planning/20260721-raw-socket-network-fix/
├── rough-idea.md                 # Concept + full RE investigation record (evidence, gate mechanism, root cause)
├── idea-honing.md                # Requirements Q&A (Q1–Q5 + Q3.5 default-ON revision)
├── design/
│   └── detailed-design.md        # Standalone design: requirements, architecture (mermaid), components, ABI, error handling, testing, appendices
├── implementation/
│   └── plan.md                   # 4-step checklist + steps (each demoable; validation folded in)
└── summary.md                    # This file
```

## Design in one paragraph

New single-file mod `src/mods/raw_socket_network_fix.rs`, modeled on `timing_offsets.rs`.
Resolves `arkGetNetworkStatus` via `resolve_ark_module()` + `GetProcAddress`, installs one
`retour::GenericDetour` (`install_enabled`) whose body calls the original then rewrites the
status out-param `4→5` (panic- and null-guarded, promotes only CHECKING; all other states
pass through). Fail-open + self-disable (`is_active()`) if the export/detour can't be
installed. **Default ON** via the pack's existing omitted→enabled behavior (disable with
`"raw-socket-network-fix": false` or the mod menu) — no `mod_trait.rs` change. Registration
in `lib.rs` + `mods/mod.rs` is the only wiring outside the new file. One hook fixes both the
boot gate (`LoadCommonEventSequence`) and the on-screen `CHECKING`/`ONLINE` text.

## Implementation approach

4 incremental, demoable steps: (1) scaffold + register; (2) resolve the export in `init`;
(3) install the promote-4→5 detour in `enable` (core end-to-end payoff); (4) docs. No unit
tests (pack convention) — validation is `cargo check`/`fmt`/`./build.sh` + live CrossOver
boot observation (clears CHECKING, `musicdata_load` fires, bar reads ONLINE, one-shot
promotion log).

## Next steps

1. Review `design/detailed-design.md` and `implementation/plan.md`.
2. When ready to build: start a `progress.md` in this dir (AGENTS.md live-resume convention)
   and work the plan checklist. Could hand the plan to `/code-assist` or `/get-going`.
3. Deploy via `./scripts/deploy.sh` to the CrossOver install; confirm the Step-3 acceptance
   criteria (boot ONLINE, backend traffic flows). This deploy also **confirms the diagnosis**
   from the investigation (that the network-status value was the sole boot blocker).

## Areas that may need refinement

- **Healthy-box default-ON residual:** on a normal box mid-handshake with a slow server,
  forcing 4→5 could let the boot gate advance early. In practice the handshake completes
  well before the gate polls (per the working-boot timeline), so this is low-risk — but if
  it ever bites, the fallback is to flip to opt-in (the dropped `DEFAULT_OFF_MODS` route).
- **ABI assumption:** `arkGetNetworkStatus(*mut i32, *mut u8, *mut u32) -> u64` is taken
  from the Ghidra decompile of `arkmdxbio2_20260324`; re-confirm on the exact deployed
  arkmdxbio2 build if it differs. Only the first out-param is touched, so the risk is low.
