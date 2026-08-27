# Idea Honing — `raw-socket-network-fix`

Requirements clarification Q&A. One question at a time; answers appended below.

---

## Q1 — Activation model: when should the mod force the network status?

Options considered:
- **Always-on** — whenever the mod is enabled, always report ONLINE.
- **Config-gated toggle** — a `mods` entry (default off, like other mods); when on,
  always report ONLINE.
- **Auto-detect** — the mod watches for the raw-socket/keepalive failure (e.g. the
  `KEEPALIVE IS DISABLE` condition / `avs_net_socket 0x8008000d`) and only forces
  ONLINE when raw sockets are genuinely unavailable; otherwise it stays completely
  passive and lets the real status through.

Trade-off: auto-detect is safest (never masks a genuinely-down network on a normal
setup) but requires a reliable signal that keepalive was disabled; always-on/gated is
simplest and most predictable but will report ONLINE even when the network is really
down.

**Answer:** **Config-gated toggle, always forces.** A standard `mods` entry (default
off, id `raw-socket-network-fix`), consistent with the pack's per-mod config
convention. When enabled it unconditionally reports ONLINE; off elsewhere. Auto-detect
was rejected as needing extra RE for a reliable keepalive-disabled signal; always-on
(no gate) rejected for breaking the config convention.

## Q2 — Override scope: what does "force ONLINE" do to the status enum?

`arkGetNetworkStatus` yields an enum: 0=LOCAL MODE, 1=OFFLINE MODE, 2=MAINTENANCE,
3=(blank), 4=CHECKING, 5=ONLINE, 7=NOT AVAILABLE. Options:
- **Unconditional 5** — always return ONLINE(5) regardless of the real value.
- **Promote CHECKING only** — only rewrite 4→5, pass every other state through
  unchanged (so a server-driven MAINTENANCE(2), a genuine OFFLINE(1), or LOCAL(0) is
  still honored; only the stuck-on-CHECKING symptom is fixed).

Trade-off: promote-only is more surgical and preserves MAINTENANCE/offline semantics;
unconditional-5 is simplest and guarantees ONLINE but would mask a real
MAINTENANCE/outage even on this cabinet.

**Answer:** **Promote CHECKING(4)→ONLINE(5) only.** Pass every other state through
unchanged, honoring server-driven MAINTENANCE(2)/OFFLINE(1)/LOCAL(0). Minimal blast
radius; fixes exactly the stuck-on-CHECKING symptom.

## Q3 — Hook point / mechanism

Both the boot gate (`gamemdx FUN_18001bbe0`, used by `LoadCommonEventSequence`) and the
bottom-bar status text (`gamemdx FUN_180009640`) read the status by calling through the
imported pointer `DAT_1806f14b8` → `arkmdxbio2!arkGetNetworkStatus`. So a single hook at
`arkGetNetworkStatus` fixes BOTH the boot progression and the on-screen `CHECKING` text.
Options:
- **GenericDetour on `arkGetNetworkStatus`** (arkmdxbio2 export; address via
  `GetProcAddress`, exactly like `input_manager` resolves `arkMDX*` today). Detour calls
  the original, then rewrites the out-param status 4→5. Fixes boot + display; matches
  the pack's cross-DLL export pattern; one-detour-per-target (nothing else hooks it).
- **Swap the gamemdx imported pointer `DAT_1806f14b8`** to a trampoline. Also fixes both,
  but requires locating that data global via signature and is a less common pattern here.
- **Detour only `gamemdx FUN_18001bbe0`** (the boot-gate wrapper). Fixes boot but leaves
  the status bar showing CHECKING (cosmetic mismatch).
- **Force the base global `arkmdxbio2!DAT_18014ae2c = 5`** — fragile (fights the updater
  thread + the `DAT_180bd4158` downgrade); rejected.

**Answer:** **Detour `arkGetNetworkStatus` (arkmdxbio2 export).** Resolve via
`GetProcAddress` (same pattern as `input_manager`'s `arkMDX*`), `GenericDetour`, call
original then rewrite out-param 4→5. Single point fixes boot progression + status-bar
text. (Log note: the non-working boot DID enter the state-1 status wait and time out
after ~30s, which means `arkIsNetworkFree` (state 0) returned truthy in both boots — so
the status value is the sole divergence and this one hook is sufficient; no need to also
hook `arkIsNetworkFree`.)

## Q4 — Graceful degradation when the hook can't be installed

Pack convention is "missing signature → skip the dependent mod, never crash." For this
mod, if `arkmdxbio2.dll` / the `arkGetNetworkStatus` export can't be resolved (or the
detour fails to install), the intended behavior is **fail-open**: the mod logs a warning
and self-disables (does not register the detour), leaving the game exactly as it is
today (stuck offline under CrossOver). No fabricated state, no crash. Config default is
**off** (must be explicitly enabled), matching the pack's opt-in convention for anything
that alters network behavior.

**Answer:** **Fail-open + self-disable.** If the export/detour can't be installed, log a
warning and self-disable — game behaves as today, no fabricated state, no crash.

> **Revised during design review (Q3.5 below):** the config **default was changed from
> OFF to ON**. The mod is a normal `mods` entry that follows the pack's "omitted key →
> enabled" convention; disable it by setting `"raw-socket-network-fix": false`. This drops
> the originally-proposed `DEFAULT_OFF_MODS` change to `mod_trait.rs` (no shared-registry
> change). Rationale: simpler diff, and promote-4→5-only is benign on a healthy box (by the
> time the boot gate polls, the real status is already ONLINE; it only shortcuts the
> transient CHECKING).

## Q3.5 — Config default (design-review revision)

During design review the default was reconsidered. Options: (a) default OFF via a new
`DEFAULT_OFF_MODS` list in `mod_trait.rs`; (b) default ON, no shared-code change, matching
every other mod's omitted→enabled behavior; user accepts forcing 4→5 by default.

**Answer:** **(b) Default ON, no `mod_trait.rs` change.** Registration in `lib.rs`/
`mods/mod.rs` is the only wiring; disable via `"raw-socket-network-fix": false` in config
or the mod menu.


## Q5 — Success criteria / validation

This repo has no unit tests; validation is live cabinet deploy + log observation.
Proposed acceptance criteria:
1. **CrossOver, mod ON:** boot proceeds past `CHECKING` without the ~30s stall;
   `LoadCommonEventSequence` logs `Lock() start` + `EssCallAndWaitBase3MusicDataLoad`;
   `playdata_3.musicdata_load` (and playerdata/rivaldata) requests reach the server;
   the bottom bar reads `ONLINE`; card-in / profile load / score save work (all HTTP).
2. **CrossOver, mod OFF (or unresolved):** unchanged from today (stuck CHECKING →
   offline). Confirms fail-open.
3. **Normal box (Windows, raw sockets OK), mod OFF:** no behavioral change (regression
   guard).
4. Known characteristic (documented, not a failure): the override is read at boot; the
   status the game acts on is decided during `LoadCommonEventSequence`, so toggling the
   mod mid-session doesn't retroactively reconnect — it takes effect on the next boot.

Non-goals: making keepalive/traceroute actually work (raw ICMP under CrossOver is out of
scope); forcing participation/matching or any state beyond the CHECKING→ONLINE gate.

**Answer:** **Accepted as written.** The four criteria + non-goals define "done".
Validation is live CrossOver-cabinet deploy + log observation (grep for `Lock() start`,
`EssCallAndWaitBase3MusicDataLoad`, `musicdata_load`, and bottom-bar `ONLINE`), plus a
regression check on a normal Windows box with the mod OFF.

---

## Requirements status

Requirements clarification considered **complete** (Q1–Q5). Summary:
- **Q1** Config-gated toggle (`raw-socket-network-fix`, default ON — omitted→enabled like
  every other mod; disable via `false`); when enabled, always forces.
- **Q2** Promote CHECKING(4)→ONLINE(5) only; pass all other states through.
- **Q3** `GenericDetour` on `arkmdxbio2!arkGetNetworkStatus` (export via `GetProcAddress`);
  rewrite out-param 4→5; fixes boot gate + status-bar text.
- **Q3.5** Default ON; no `mod_trait.rs`/`DEFAULT_OFF_MODS` change (registration only).
- **Q4** Fail-open + self-disable if unresolved; no crash.
- **Q5** Acceptance criteria + non-goals accepted.

Design-time verifications (fold into design, not a separate research round):
- Confirm the pack's hook infra (`core/hooks.rs` `install_enabled` / `GenericDetour`)
  supports a target in `arkmdxbio2.dll` (cross-DLL), and how `input_manager` resolves
  `arkmdxbio2` exports (`GetProcAddress` after locating the loaded module).
- Confirm the exact `arkGetNetworkStatus` prototype/out-param layout to rewrite (3 out
  pointers: status @ param_1, plus param_2/param_3 passthrough).
- Confirm enable timing (mod init) is before `LoadCommonEventSequence` runs (~90s into
  boot in the observed log — comfortably after DLL init).






