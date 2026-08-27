# Rough Idea — `raw-socket-network-fix` mod

## One-liner

Add a mod that hooks `arkGetNetworkStatus` (arkmdxbio2.dll export) and forces the
game to see the network as **ONLINE**, bypassing the AVS keepalive / raw-ICMP-socket
requirement that otherwise hangs the boot sequence on **CHECKING** and drops the game
into offline mode when running under CrossOver/Wine (or any environment that can't
create raw ICMP sockets).

## Motivation / originating investigation (2026-07-21)

Running DDR World under CrossOver on macOS, the game boots, passes the hardware check
(`e-Amusement: OK`, some traffic reaches the backend), but then shows "please wait" in
the top-left for ~10-15s, displays `CHECKING` instead of `ONLINE`, sends **no further
backend requests during that window**, and finally boots offline. The **same** game
data + server connects fine from a Windows box.

Investigation established (evidence: `log_working.txt` / `log_not_working.txt`,
`packet_logs_working.jsonl` / `packet_logs_not_working.jsonl`, and Ghidra on
`gamemdx_20260616.dll`, `arkmdxbio2_20260324.dll`, `ess_20260324.dll`):

**Proven (game-side gate):**
- `sequence::network::LoadCommonEventSequence::onUpdate` (`gamemdx FUN_1800b4070`) is a
  state machine. In state 1 it reads `arkGetNetworkStatus()` and **loops while it
  returns 4 (`CHECKING`); it only advances to `ArkNetwork.Lock()` +
  `EssCallAndWaitBase3MusicDataLoad::request()` (the `playdata_3.musicdata_load`
  fetch) when it returns 5 (`ONLINE`)**, otherwise times out (~30s) and terminates →
  offline boot. This exactly matches the symptom (please-wait → CHECKING → no requests
  → offline).
- Logs corroborate: working boot logs `Lock() start → musicdata_load`; the CrossOver
  boot sets a timeout at 22:18:18, idles 30s, then `Unlock() not locked` (gave up) at
  22:18:48.
- `arkGetNetworkStatus` (`arkmdxbio2 180003ef0`) returns ONLINE(5) only when its base
  state `DAT_18014ae2c == 5` (set by `arkESSNetworkStatusUpdate FUN_180007710` from
  `ess_ea3_get_status` → AVS `ea3_get_status() == 3`) AND the secondary net-property
  gate `DAT_180bd4158 == 3`; a game-controlled blackout flag `DAT_180c436f8 == 1`
  (`arkSetNetworkIsBlack`) forces CHECKING. The status-bar `CHECKING/ONLINE/OFFLINE`
  text (`gamemdx FUN_180009640`) reads the SAME enum.

**Proven (differentiator):**
- The eamuse HTTP handshake is byte-identical and fully successful in BOTH boots
  (services/pcbtracker/message/facility/pcbevent/package/eventlog/tax all `status=0`).
  HTTP reachability is NOT the blocker.
- A full warning/error diff shows the ONLY substantive client-side failures unique to
  the CrossOver boot are raw-socket ones — both via `avs_net_socket` error `0x8008000d`:
  - `keepalive: failed to create raw socket.0x8008000d` → `KEEPALIVE IS DISABLE`
  - `traceroute: avs_net_socket: 8008000d` → `TRACEROUTE IS DISABLE`
  Working boot: neither fails, keepalive gets armed (`host=192.168.1.108, t1=2, t2=10`).
- Red herrings ruled out: `servurl: bad services host(...)` fires in BOTH logs (with
  each machine's IP), so the `127.0.0.1` vs `192.168.1.108` services URL (which is
  by-design: local server advertises localhost, LAN server advertises its IP) is not
  the cause. `GetIpAddrTable` hook, window-visibility, `msvcrt __argc`, `pkglist
  no list` are cosmetic/benign.

**Root cause (high confidence):** CrossOver/Wine cannot create raw ICMP sockets
(`avs_net_socket → 0x8008000d`), disabling AVS keepalive (and traceroute). Without a
healthy keepalive/net-check, the eamuse layer never reports ONLINE, so boot waits,
times out, and falls back to offline — even though HTTP to the server works.

**Honest gap:** the internal AVS wire from "keepalive/raw-socket disabled" to
"`ea3_get_status` < 3" was NOT traced — libavs is stripped and its log strings are
encrypted. The keepalive→CHECKING link is established by elimination + correlation, not
by reading the deciding AVS branch. Forcing `arkGetNetworkStatus` to ONLINE is therefore
BOTH the confirming experiment (if boot then proceeds and online play works, the
network-status value was the sole blocker) AND the fix.

## Proposed approach (starting point — to be refined via PDD)

- Hook the network-status read so it reports ONLINE(5) while active.
  - Candidate hook points: `arkGetNetworkStatus` (arkmdxbio2 export) directly; OR the
    gamemdx wrapper `FUN_18001bbe0`; OR the imported function pointer `DAT_1806f14b8`
    in gamemdx; OR force the base global `arkmdxbio2!DAT_18014ae2c = 5`.
- Because all subsequent traffic is HTTP (which works under CrossOver), forcing ONLINE
  should let the boot proceed and online features function on a private LAN server.

## Open questions for PDD

- Which hook point (arkmdxbio2 export vs gamemdx imported pointer vs global write)?
- Should it be always-on, config-gated, or auto-detected (only force ONLINE when
  keepalive/raw-socket is actually unavailable)?
- Should it only override CHECKING(4)→ONLINE(5), or also override other states
  (OFFLINE/MAINTENANCE)? Risk of masking genuinely-down networks.
- How to fail safe / degrade gracefully if the signature isn't found.
- Interaction with score submission / other network-gated features.
