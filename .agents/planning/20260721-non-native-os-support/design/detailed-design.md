# Design — Non-Native OS Support (mod merge + movie-crash fix)

## Decision record

This feature was executed from a handoff (prior session did the environment
diagnosis and the winetricks/native-DLL dead-end investigation); decisions
below were either made in that handoff or during this implementation.

### D1 — One mod, two independent sub-fixes (handoff decision)

Merge the (unreleased) `raw-socket-network-fix` mod and the new movie-crash fix
into a single mod, id **`non-native-operating-system-support`**, struct
`NonNativeOsSupportMod` in `src/mods/non_native_os_support.rs` (git-mv'd from
`raw_socket_network_fix.rs` to preserve history). Rationale: both are
Wine/CrossOver-only workarounds; operators think of them as one switch
("support running on a non-native OS"). Each sub-fix owns its own `static mut`
detour storage and resolve/install/self-disable path:

- **(a) network-status promotion** — the existing `arkGetNetworkStatus`
  CHECKING(4)→ONLINE(5) detour, unchanged.
  See `.agents/planning/20260721-raw-socket-network-fix/`.
- **(b) movie-graph stub** — `GenericDetour` on gamemdx
  `DShowPlayer::BuildGraph` (see `research/movie-player-re.md`).

`enable()` installs both independently; `disable()` tears down both;
`is_active()` = union. `required_signatures()` stays `&[]` — deliberately
lenient: listing `movie_build_graph` would make a missing movie signature
abort the WHOLE mod at registration, taking the unrelated network fix with it.
Both targets resolve best-effort in `init` (`resolve_ark_module` +
`GetProcAddress` for (a); `ctx.signatures.get_address` for (b)).

### D2 — Stub target: `DShowPlayer::BuildGraph`, not the actor/factory levels

Candidates considered:

| Level | Verdict |
|---|---|
| Movie actor (`FUN_18007c560` `+0xE8==0` no-movie branch) | Rejected — two actor sites, the second (`FUN_18003fcc0`) has no no-movie branch; patching data flow is more invasive than one function stub |
| `DShow` factory (`FUN_180232ef0`) | **Unsafe** — `FUN_180215ee0` vcalls the DShow object without a null check |
| `DShow::Open` (`FUN_180232da0`) | Workable but shallower — leaves less of the game's own logic running |
| **`DShowPlayer::BuildGraph` (`FUN_18023ae40`)** | **Chosen** — single choke point (sole `CLSID_FilterGraph` user, covers all request types and both actor sites), function body is 100 % DirectShow work, callers verified tolerant |

### D3 — Stub semantics: fake success (state=3), not an error return

**First attempt (returned `0xC0260002`, live-tested, REJECTED):** left the
player state at 0 → `Dx9Movie` STATUS stuck at 1 ("opening") → the attract demo
loaded its assets and then waited forever on the movie-ready gate (soft-lock,
observed 15:59–16:04). The "same as a missing movie file" safety argument was
wrong: real cabinets take the no-movie path at the *actor* level, so the
player-level failure state is not naturally reachable.

**Final design:** the detour never calls the original; it writes player state
(`+0x08`) = 3 — the success epilogue's one observable side effect — leaves the
`opened` byte (`+0x14`) at 0, and returns 0. State 3 advances the STATUS
machine (1→6→7) so every poller proceeds; `opened`==0 keeps the per-frame
get-frame path on its guarded early-return so no null COM pointer is ever
touched (all vtable methods verified per-slot — see research doc). The movie
"plays" silently delivering no frames; the song runs with a static background.

### D4 — Config-key migration: clean rename (handoff decision)

`raw-socket-network-fix` was implemented 2026-07-21 and never released; no
back-compat shim. A leftover `"raw-socket-network-fix": false` in an old
config would become inert (and the new key defaults to enabled via the pack's
omitted-key-enables convention). The test cabinet's config had no entry at all.

### D5 — Cheap experiments (`-ddrsd`, `-audiodummy`) skipped as moot

The handoff suggested trying these spice flags before coding, as possible
non-code wins. By the time the environment was exercised, the durable code fix
was already implemented and live-validated, removing their decision value
(the handoff itself noted "the code fix proceeds regardless as the durable
solution"). Not tested; recorded here for completeness. If someone wants the
movies *back* under Wine someday, that's a WineD3D/GStreamer investigation,
not these flags.

## Error handling

Matches the pack norms: `catch_unwind` in both detour bodies, null-guarded
out-params/this pointers, `static mut` accessed via `addr_of!`/`addr_of_mut!`,
`unsafe impl Send`, fail-open self-disable per sub-fix with one-shot
provenance log lines, teardown race accepted (documented in `remove_hooks`).

## Testing (this repo's convention: live deploy + log observation)

See `progress.md` deploy log. Acceptance verified 2026-07-21:

1. Boot ONLINE: promotion log at 16:15:59, `musicdata_load` traffic flowed.
2. Movie crash: suppression fired at the exact former crash point (attract
   demo); demo song played through (~50 s), attract loop cycled 3×, zero
   `EXCEPTION_ACCESS_VIOLATION`.
3. Non-movie behavior: title/how-to/loop scenes cycled normally.
