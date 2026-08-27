# Idea Honing — Custom-Options JSON Persistence

Q&A log refining the rough idea into requirements. Decisions settled during the
research review precede the formal Q&A.

## Pre-Q&A decisions (settled during research review)

### D1. JSON load timing — lazy timer trigger
The generic JSON load must run after all mods have registered their options
(which happens at mod `enable()`, step 8 in `lib.rs`), so it cannot live in
`custom_options_persistence::init()` (step 4i). **Decision:** use a lazy
time-delayed trigger — fire the JSON load ~10–15 seconds after init. This is
well after all mods load but well before a user could be logged in and viewing
options. (Acceptable for now; not tied to a specific game event.)

### D2. Network-vs-JSON precedence — network wins
When both `persist_network` and `persist_json` are enabled and the network sends
option values, the **network values override** whatever the JSON primed. JSON is
the offline fallback; a server that actually returns custom-option values takes
precedence.

### D3. Transform reuse — yes
The generic JSON path reuses the registry's `save_transform` / `load_transform`
(via `snapshot_for_save()` / `resolve_from_load()`), so it stores the same wire
value the network path does (e.g. asset_id for WebUI options). WebUI's inline
asset_id↔index conversion in its own JSON read/write goes away.

---

## Formal Q&A

### Q1. JSON save trigger
**Q:** When should the generic JSON persister write custom-options values to
`mod-config.json` — on every value change, on the network `save_sender`
(card-out), or a debounced hybrid?

**A:** On the network `save_sender`. JSON write happens at the same moment as the
network save (card-out), piggybacking on the same ess.dll detour.

**Implication to confirm (see Q2):** the `save_sender` detour is currently only
installed when network persistence is enabled. For `persist_json` to work
independently, the detour must install when *either* gate is on, and emit
network children / write JSON conditionally inside the trampoline.

### Q2. Detour gating matrix
**Q:** How should `persist_network` and `persist_json` control the ess.dll
`save_sender` / `load_receiver` detours?

**A:** Install the detours if **either** gate is on. Inside `save_sender`: emit
network `<mod_{id}>` children only if `persist_network`; write the JSON block
only if `persist_json`. Inside `load_receiver`: read network children only if
`persist_network`. If **both** gates are off, install no detours (preserves
current no-op behavior). JSON save thus rides the same card-out moment as network
save, independent of the network gate.

**Refinement (D4): dirty-check before writing.** The JSON write must only touch
disk when the serialized `custom_options` block actually differs from what's
already on disk. Build the new `{p1,p2}` block, compare it against the current
value (read from `config::get()` cache or re-read from file), and skip the
`fs::write` entirely if they're equal. Avoids redundant writes when a card-out
produces identical values to what's already persisted.

### Q3. Migration of the existing `webui_options` block
**Q:** What happens to a user's existing `webui_options: {p1,p2}` block after the
rename to `custom_options: {p1,p2}`?

**A:** One-time auto-migrate. On first run after the rename: if a `webui_options`
block exists and no `custom_options` `{p1,p2}` data does, copy its contents into
the new `custom_options` block and delete the old `webui_options` key. Preserves
the offline cache seamlessly. The migration code path can be removed in a future
cleanup.

**Note:** the `custom_options` key already exists as a config *section* (holding
the `persist` gate). The migration / persistence block (`p1`/`p2`) co-habits that
same `custom_options` object alongside the gate keys (see Q4).

### Q4. Shape of the `custom_options` config section
**Q:** How should the gates and persisted p1/p2 values be arranged within the
`custom_options` section?

**A:** Flat co-habitation — gates and player data are sibling keys:
```jsonc
custom_options: {
  persist_network: true,
  persist_json: true,
  p1: { <option_id>: <wire_value>, ... },
  p2: { <option_id>: <wire_value>, ... }
}
```
Matches the rough idea's "same shape as before" — `p1`/`p2` are simply promoted
from `webui_options` to `custom_options`, with the two gate booleans added
alongside.

**Serde implication:** `CustomOptionsConfig` keeps typed gate fields
(`persist_network`, `persist_json`) and the `p1`/`p2` data is handled out-of-band
via read-modify-write (the gates deserialize into the struct; `#[serde(default)]`
or a flattened catch-all keeps the `p1`/`p2` keys from breaking the parse). The
writer preserves the gate keys when rewriting `p1`/`p2`, and vice-versa.

### Q5. Gate defaults and legacy `persist` key
**Q5a:** Defaults for `persist_network` / `persist_json` when absent?
**A:** Both default `true`. `persist_network=true` preserves current network
behavior; `persist_json=true` preserves the current WebUI-JSON-always-on
behavior. A user with no `custom_options` section gets both on — fully
backwards-compatible.

**Q5b:** How to handle the legacy single `persist` key?
**A:** Drop it entirely. The old `persist` key is no longer read. (Note: a user
who had set `persist: false` to disable network will silently revert to network
ON until they switch to `persist_network: false`. Accepted as a clean break since
this is a solo-maintainer repo with a known config.) Update the bundled
`mod-config.json` and README accordingly.

### Q6. WebUI mod responsibilities after the move
**Q:** How should WebUI register its options and apply game state once JSON
persistence is generic?

**A:** Register plain, loader primes later. WebUI's `enable()` discovers assets,
registers options with their **plain defaults** (no JSON read at enable), and
subscribes its scene-20 `Customize`-struct apply. The generic loader fires
~10–15s later, reads `custom_options.{p1,p2}`, and calls `resolve_from_load` for
each cached value — which fires WebUI's `on_change`, writing the `Customize`
struct. **All JSON read/write logic leaves WebUI entirely**; WebUI keeps only
(a) asset discovery and (b) the live game-state write (scene-20 apply +
`on_change`). The transforms (`persist_save_transform`/`persist_load_transform`)
stay in WebUI as registered transform fns — the framework calls them generically.

**Accepted consequence:** between WebUI `enable()` and the lazy loader (~12s),
options momentarily show plain defaults rather than the JSON-cached values. Since
the loader fires well before a user could be logged in and viewing options, this
window is not user-visible in practice.

### Q7. Precedence guard for network-wins (D2)
**Q:** How does the JSON loader guarantee network values win if timing overlaps?

**A:** Loader is one-shot; network re-applies. The JSON loader runs exactly once
at ~12s. The network `load_receiver` always calls `resolve_from_load` on each
card swipe regardless of whether JSON ran — so a later network swipe naturally
overwrites JSON-primed values. Because the timer fires before any card swipe is
possible, JSON never clobbers a network value in practice. No extra
network-loaded tracking state needed; relies on (a) timer-before-login ordering
and (b) network always re-applying on swipe.

---

## Requirements clarification — COMPLETE

All seven questions plus the four pre-Q&A decisions (D1–D4) are settled. Summary
of the agreed design shape:

1. **Generic JSON persistence** moves from WebUI into the custom_options
   framework, covering every registered persistable option (reusing
   `snapshot_for_save()` / `resolve_from_load()` + registry transforms).
2. **Config section** `custom_options` gains `persist_network` + `persist_json`
   (both default true), drops the legacy `persist` key, and co-habits with
   `p1`/`p2` data (flat shape).
3. **Save** rides the ess.dll `save_sender` (card-out); detours install if either
   gate is on; JSON write is dirty-checked (skip if unchanged).
4. **Load** is a one-shot lazy timer (~10–15s); network load overrides JSON.
5. **Migration**: one-time `webui_options` → `custom_options` copy + delete old
   key.
6. **WebUI** keeps asset discovery + game-state apply; sheds all JSON read/write;
   transforms remain as registered fns.
