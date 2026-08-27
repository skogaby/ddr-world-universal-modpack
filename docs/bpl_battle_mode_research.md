# In-Shop Battle Mode (BPL) — Reverse Engineering Research

## Overview

"In-shop battle mode" (internally referenced as "BPL Battle Mode") is a local multiplayer mode in DDR World that allows up to 4 players across 2 linked cabinets to play simultaneously in a versus format. Despite the "BPL" name in the code (referencing Bemani Pro League), this is a general-purpose feature available to any arcade with 2+ networked DDR cabinets — it is not exclusive to Konami tournaments.

The mode uses Konami's LibComm library for direct cabinet-to-cabinet communication over the local network.

## How It Works (High-Level)

```
    ┌──────────────┐            ┌──────────────┐
    │  Cabinet A    │◄──────────►│  Cabinet B    │
    │  (Side 1)     │  LibComm   │  (Side 2)     │
    │               │  TCP+UDP   │               │
    │  Port 6198    │  Port 6198 │  Port 6198    │
    └──────────────┘            └──────────────┘
```

1. Each cabinet derives its hardware type locally from boot parameters. MachineType must be 4 (gold cab) for matching to activate.
2. The operator sets matching_group and matching_side in the test menu on each cabinet.
3. When a player starts a game, the game initiates a local network search via LibComm.
4. Cabinets discover each other via UDP broadcast on port 6198, then establish TCP connections for game data sync.
5. The mode select screen shows "In-shop battle mode" as a third option alongside Solo and Duo.

---

## Configuration Sources

### 1. Hardware Type Gate (Local — Boot Parameters)

The matching system is **hardware-gated**. It only activates on specific cabinet types. The game derives `MachineType` and `PCType` locally from boot parameters and hardware detection — the server is not involved.

During initialization (`FUN_180001420` at `gamemdx.dll+0x1420`), the game calls:
- `arkMDXGetMachineType` (exported from `arkmdxbio2.dll`) → must return **4**
- `arkMDXGetPCType` (exported from `arkmdxbio2.dll`) → must return **2, 3, or 4**

These two values are combined by `FUN_180013310` into a single return value used throughout the codebase to determine cabinet capabilities (referred to here as the "config code").

| MachineType | PCType | FUN_180013310 returns | Matching enabled? |
|---|---|---|---|
| 0-1 | 0-1 | 0 | ❌ |
| 0-1 | 2 | 3 | ❌ |
| 2 | 0-1 | 1 | ❌ |
| 2 | 2 | 4 | ❌ |
| 3 | 2 | 5 | ❌ |
| **4** | **2** | **6** | **✅** |
| **4** | **3** | **7** | **✅** |
| **4** | **4** | **8** | **✅** |

Only return values **6, 7, 8** enable the matching system. All three require `MachineType == 4` (DDR 20th anniversary gold cabinet).

When `FUN_180013310` returns 6/7/8, the initialization function calls `FUN_1801a4b00(1)` which:
- Queries network interfaces via `XCnbrep7000076(7, ...)`
- Creates a matching network handle via `XEyy2igh000061`
- Stores the handle in `DAT_1806b5b68` (non-zero = matching system active)
- Registers a matching data callback via `XEyy2igh000063`

### 2. Operator Menu Settings (Local — Test Menu)

These are set per-cabinet by the operator in the in-game test menu. They are not sent by the server.

| Setting | Values | Description |
|---|---|---|
| `networkOptions/matching_group/current` | 0-N | Matching group ID. Both cabinets must have the same non-zero value. 0 = disabled. |
| `networkOptions/matching_side/current` | 1 or 2 | Which "side" this cabinet is. Cabinet A = 1, Cabinet B = 2. |

All properties starting with `networkOptions/` are local operator settings, not server-provided values.

### 3. Server Role (ESS Matching Protocol)

The server does not control hardware type or operator settings. Its role is as a **matchmaking broker** — cabinets register with the server, the server tells them each other's IPs/ports, and then they connect directly via LibComm (port 6198). The server does not relay game data.

See the **ESS Matching Server Protocol** section below for the complete request/response format derived from ess.dll analysis.

---

## Game-Side Data Flow

### Global Variables (gamemdx.dll .data section)

| RVA | Size | Name | Description |
|-----|------|------|-------------|
| `+0x6B5B10` | dword | matching_network_id | Network ID from interface query |
| `+0x6B5B34` | dword | matching_state | Matching state machine (0=idle, 3=searching, 4=connected) |
| `+0x6B5B38` | dword | matching_substep | Sub-step within current state |
| `+0x6B5B40` | byte | matching_group | From operator menu `networkOptions/matching_group/current` |
| `+0x6B5B41` | byte | matching_side | From operator menu `networkOptions/matching_side/current` |
| `+0x6B5B42` | byte | matching_flag_2 | Unknown matching flag |
| `+0x6B5B43` | byte | matching_ready | Non-zero when matching system is initialized and ready |
| `+0x6B5B44` | handle | matching_mutex | Mutex for thread-safe matching data access |
| `+0x6B5B68` | qword | matching_network_handle | Network handle from `XEyy2igh000061`. Non-zero = matching system active. This is the master gate. |
| `+0x6B5C58` | byte | matching_enabled | The `param_1` passed to `FUN_1801a4b00`. 1 if config code is 6/7/8. |
| `+0x6B6648` | buffer | matching_data_buffer | Buffer for matched player data |

### Activation Chain

```
Boot parameters
  → arkMDXGetMachineType() == 4 AND arkMDXGetPCType() ∈ {2,3,4}
    → FUN_180013310() returns 6, 7, or 8
      → FUN_1801a4b00(1) called (matching_enabled = 1)
        → XEyy2igh000061() creates network handle (matching_network_handle != 0)
          → Matching system active
            → Operator menu matching_group/matching_side are read
              → If matching_group != 0: battle mode UI appears on mode select
                → LibComm searches for partner cabinet on port 6198
                  → If partner found: "In-shop battle mode" becomes selectable
```

### Conditions for Battle Mode UI

```c
// In SelectStyleSequence::onUpdate (gamemdx.dll+A8EC0)
if (matching_group != 0          // +6B5B40: operator set a group
    && matching_ready != 0       // +6B5B43: matching system initialized
    && matching_data_available()) // FUN_1801a5560: partner cabinet found
{
    // Show battle mode as available and selectable
} else if (matching_group != 0 && matching_ready != 0) {
    // Show battle mode UI but as "Not available"
} else {
    // Hide battle mode entirely
}
```

### BPL Flag (SelectStyleSequence + 0x288)

When a player actually enters battle mode, a byte flag at offset `0x288` of the `SelectStyleSequence` object is set to 1. This changes:
- Mode logging string: "Start STANDARD MODE" → "Start BPL BATTLE MODE"
- Global state at `*DAT_1806b42a8 + 0xD0`: set to 1 (affects downstream gameplay)
- Background music: switches to `bgm_bpl`
- UI elements: BPL-specific headers, ranking screens, etc.

---

## Local Network Protocol (LibComm)

### Network Configuration
- **Port**: 6198 (0x1836) — UDP for discovery, TCP for game data
- **Library**: LibComm (Konami internal, part of gamemdx.dll)
- **Discovery**: UDP broadcast on the local network segment
- **Data sync**: TCP connections between matched cabinets

### Matching Flow

```
Cabinet A (Host/Side 1)                    Cabinet B (Guest/Side 2)
─────────────────────────                  ─────────────────────────
StartLocalMatchingSearch()                 StartLocalMatchingSearch()
  └─ LibComm::Start(port=6198)              └─ LibComm::Start(port=6198)
  └─ UDP broadcast: "I'm here"              └─ UDP broadcast: "I'm here"

HOST_ACCEPT                                GUEST_CONNECT
  └─ TCP accept connection                   └─ TCP connect to host

HOST_COMMUNICATE_WAIT                      GUEST_COMMUNICATE_WAIT
  └─ SendPlayerInfoNotice ──────────────►    └─ ReceivePlayerInfoNotice
  ◄──────────────── SendPlayerInfoReport ──  └─ SendPlayerInfoReport

HOST_FINISH_NOTICE                         GUEST_FINISH_WAIT
  └─ SendMatchingSearchFinishNotice ────►    └─ ReceiveMatchingSearchFinishNotice

MATCHED ✓                                  MATCHED ✓
```

### In-Game Sync Messages (TCP)

| Message | Direction | Purpose |
|---------|-----------|---------|
| `PlayerInfoReport` | Guest → Host | Player profile data |
| `PlayerInfoNotice` | Host → Guest | Player profile data |
| `MatchingSearchFinishNotice` | Host → Guest | Match confirmed |
| `KeepAliveRequest/Reply` | Both | Connection health check |
| `SceneSkipReport/Notice` | Both | Scene transition sync |
| `DecidedMusicReport/Notice` | Both | Song selection sync |
| `DecidedRuleNotice` | Host → Guest | Game rules |
| `MusicStartSyncNoticePrepare` | Host → Guest | Pre-start sync |
| `MusicStartSyncNoticeForced` | Host → Guest | Force start |
| `StageResultReport/Notice` | Both | Score/result sync |

---

## ESS Matching Server Protocol (from ess.dll)

The ESS matching system uses e-amusement XML-based request/response. There are three operations: **request** (register for matching), **query** (poll for matches), and **finish** (complete matching). The server acts as a matchmaking broker — it does not relay game data. Cabinets connect directly to each other via LibComm after the server provides connection info.

This protocol natively supports internet matching — the cabinet sends both public and local IPs, and the server returns both. The client picks the appropriate one based on whether the partner is on the same network.

### 1. Matching Request

**Purpose**: Cabinet registers itself as available for matching.

**Request fields** (cabinet → server):

| Field | Type | Description |
|---|---|---|
| `/info/version` | s32 (6) | Protocol version (hardcoded 1) |
| `/data/matchtyp` | s32 (6) | Match type |
| `/data/matchgrp` | s32 (6) | Match group (from operator menu) |
| `/data/matchflg` | s32 (6) | Match flags |
| `/data/waituser` | s32 (6) | Number of users waiting to match |
| `/data/waittime` | s32 (6) | Wait timeout value |
| `/data/joinip` | str (0xb) | Cabinet's public/WAN IP (`%d.%d.%d.%d`) |
| `/data/joinport` | s32 (6) | Cabinet's public port |
| `/data/localip` | str (0xb) | Cabinet's local/LAN IP |
| `/data/localport` | s32 (6) | Cabinet's local port |
| `/data/dataid` | str (0xb) | Data identifier |
| `/data/gamekind` | str (0xb) | Game identifier |
| `/data/locationid` | str (0xb) | Shop/location ID |
| `/data/lineid` | str (0xb) | Network line ID |
| `/data/locationcountry` | str (0xb) | Location country code |
| `/data/locationregion` | str (0xb) | Location region code |

**Response fields** (server → cabinet):

| Field | Type | Description |
|---|---|---|
| `hostid` | u64 (8) | Assigned host ID for this matching session |
| `result` | s32 (6) | Result code. 1 = success (triggers `/coin/match` billing) |
| `hostip_g` | str (0xb) | Host cabinet's global/public IP |
| `hostip_l` | str (0xb) | Host cabinet's local/LAN IP |
| `hostport_g` | s32 (6) | Host cabinet's global port |
| `hostport_l` | s32 (6) | Host cabinet's local port |

**NAT traversal**: The receiver compares `hostip_g` to the local cabinet's own IP. If they match (same network), it uses `hostip_l`/`hostport_l`. Otherwise it uses `hostip_g`/`hostport_g`.

### 2. Matching Query

**Purpose**: Cabinet polls for available matches in its group.

**Request fields** (cabinet → server):

| Field | Type | Description |
|---|---|---|
| `/info/version` | s32 (6) | Protocol version (hardcoded 1) |
| `/data/hostid` | u64 (8) | Host ID (from the request response) |
| `/data/locationid` | str (0xb) | Shop/location ID |
| `/data/lineid` | str (0xb) | Network line ID |

**Response fields** (server → cabinet):

| Field | Type | Description |
|---|---|---|
| `result` | s32 (6) | Result code: <0 = error, 0 = no match yet (keep waiting), ≥1 = match found |
| `prwtime` | s32 (6) | Remaining wait time (only when result=0) |

When `result ≥ 1`, the response includes `/matchlist` with match records (max 8):

| Field | Type | Description |
|---|---|---|
| `/matchlist/record_num` | u32 (7) | Number of matched records |

For each `/matchlist/record`:

| Field | Type | Description |
|---|---|---|
| `pcbid` | str (0xb) | Partner cabinet's PCB ID (21 bytes max) |
| `statusflg` | str (0xb) | Partner's status flag |
| `matchgrp` | s32 (6) | Match group |
| `hostid` | u64 (8) | Partner's host ID |
| `jointime` | u64 (9) | Join timestamp |
| `connip_g` | str (0xb) | Partner's global/public IP |
| `connport_g` | s32 (6) | Partner's global port |
| `connip_l` | str (0xb) | Partner's local/LAN IP |
| `connport_l` | s32 (6) | Partner's local port |

**NAT traversal** (same logic as request): If `connip_g` matches the local cabinet's own IP, the client uses `connip_l`/`connport_l`. Otherwise uses `connip_g`/`connport_g`.

**Billing**: When `record_num > 1`, triggers `/coin/match` billing event (once per session).

### 3. Matching Finish

**Purpose**: Cabinet signals matching is complete (success or timeout).

**Request fields** (cabinet → server):

| Field | Type | Description |
|---|---|---|
| `/info/version` | s32 (6) | Protocol version (hardcoded 1) |
| `/data/hostid` | u64 (8) | Host ID |
| `/data/locationid` | str (0xb) | Shop/location ID |
| `/data/lineid` | str (0xb) | Network line ID |

**Response fields** (server → cabinet):

| Field | Type | Description |
|---|---|---|
| `result` | s32 (6) | Result code |

### Server Implementation Summary

1. **Request**: Cabinet registers itself with its IPs/ports, match group, and location info. Server stores the entry and assigns a `hostid`. If another cabinet in the same `matchgrp` is already waiting, the server can return that cabinet's connection info immediately.
2. **Query**: Cabinet polls with its `hostid`. Server returns the list of other cabinets in the same `matchgrp` with their connection info (both global and local IPs/ports). Returns `result=0` + `prwtime` if no match yet.
3. **Finish**: Cabinet signals done. Server cleans up the session entry.
4. **Billing**: `/coin/match` is fired client-side when a match is confirmed. The server does not need to handle this.
5. **Internet matching**: Works natively. The server just needs to store and return the correct global IPs/ports. Cabinets with port 6198 forwarded can connect directly across the internet without a VPN.

---

## Enabling Matching on Non-Gold Cabinets

The matching system is gated by `FUN_180001420` (`gamemdx.dll+0x1420`) which calls `FUN_180013310` and only enables matching if the result is 6, 7, or 8 (all requiring MachineType 4 / gold cab).

`FUN_1801a4b00` is the matching network initialization function. It receives a single argument: `1` to enable, `0` to disable. Normally only called with `1` when `FUN_180013310` returns 6/7/8. Forcing the argument to `1` unconditionally removes the hardware gate.

Everything downstream — operator menu settings, LibComm discovery, cabinet-to-cabinet TCP/UDP on port 6198, matching state machine — works normally. Only the hardware gate is removed; no matching state is faked.

### Signature Scan Approach

Rather than hardcoding the `+0x1A4B00` offset, the function can be found via AOB:

| Function | Suggested Approach |
|---|---|
| `FUN_1801a4b00` | Search for the `"%s:FPV1"` format string xref — it is the only function that uses it. Or scan for the `XEyy2igh000061` call pattern within the function. |
| Alternative: `FUN_180001420` | Find the caller instead — it contains 3 consecutive calls to `FUN_180013310` with comparisons to 6, 7, 8. Patch the argument at the call site. |

---

## Key Functions Reference

| Ghidra Address | RVA | Name | Purpose |
|---|---|---|---|
| `0x180001420` | `+0x1420` | MatchingSystemInit | Checks config code, calls FUN_1801a4b00 |
| `0x180013310` | `+0x13310` | GetHardwareConfig | Combines MachineType + PCType into a capability value |
| `0x180001590` | `+0x1590` | BootUpdate | Reads operator menu matching settings |
| `0x1801a4b00` | `+0x1A4B00` | InitMatchingNetwork | Creates network handle, initializes LibComm |
| `0x1801a4f80` | `+0x1A4F80` | SetMatchingConfig | Writes matching_group and matching_side globals |
| `0x1801a5560` | `+0x1A5560` | IsMatchingDataAvailable | Returns true if partner cabinet found |
| `0x1801a4e20` | `+0x1A4E20` | FetchMatchingData | Reads matching data buffer (mutex-protected) |
| `0x1801a4d60` | `+0x1A4D60` | UpdateMatchingState | Periodic matching state machine update |
| `0x1801a66e0` | `+0x1A66E0` | StartLocalMatchingSearch | Initiates LibComm on port 6198 |
| `0x1801adca0` | `+0x1ADCA0` | LibComm::Start | Initializes TCP + UDP communication |
| `0x1801ae6c0` | `+0x1AE6C0` | LibComm::TCPComm::Init | TCP socket setup |
| `0x1801b0fc0` | `+0x1B0FC0` | LibComm::UDPComm::Init | UDP socket setup (receives port param) |
| `0x1800a6bf0` | `+0xA6BF0` | SelectStyleSequence::onInitialize | Mode select screen setup |
| `0x1800a8ec0` | `+0xA8EC0` | SelectStyleSequence::onUpdate | Mode select per-frame logic |
| `0x180045700` | `+0x45700` | Component::SetEnabled | Controls battle mode UI visibility |

---

## Open Questions

1. **Is the ESS matching flow required for local matching?** — Or does LibComm handle local discovery independently? The ESS flow might only be needed when cabinets are not on the same broadcast domain.
2. **What data is in the LibComm PlayerInfo packets?** — The TCP packets exchanged directly between cabinets during gameplay are separate from the ESS server protocol.
3. **What are the valid values for `matchtyp` and `matchflg`?** — These are sent in the matching request but their meaning is unknown.

## Verified Through Live Testing

- ✅ Setting `matching_group=1`, `matching_side=1`, `matching_ready=1` in memory makes the "In-shop battle mode" option appear on mode select
- ✅ Patching `FUN_1801a5560` to return true makes the option show its full graphic instead of "Not available"
- ✅ The BPL flag at `SelectStyleSequence+0x288` controls the "Start BPL BATTLE MODE" vs "Start STANDARD MODE" logging
- ✅ The mode cannot be navigated to without the full matching system being active
