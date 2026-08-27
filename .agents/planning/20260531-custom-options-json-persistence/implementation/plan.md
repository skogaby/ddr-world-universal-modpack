# Implementation Plan — Custom-Options JSON Persistence

Each step ends in `cargo check --target x86_64-pc-windows-msvc` and leaves the
tree compiling. There is no unit-test harness (per project convention), so
"tests" are the explicit **Demo** observation for each step — what to watch in
the persistence INFO logs / `mod-config.json` on a cabinet deploy. Steps are
ordered so core round-trip behavior is exercisable as early as possible and no
orphaned code accumulates.

References: design `design/detailed-design.md` (R1–R14), `idea-honing.md`
(D1–D4, Q1–Q7), `research/current-state.md` (file:line map).

## Checklist

- [x] Step 1 — Config schema: split `persist` → `persist_network` + `persist_json`
- [x] Step 2 — Gate the detours + trampoline network-emission on `persist_network`
- [x] Step 3 — Dirty-checked `custom_options` block writer in `config.rs`
- [x] Step 4 — JSON save on `save_sender` (gated on `persist_json`)
- [x] Step 5 — One-shot lazy JSON load timer (gated on `persist_json`)
- [x] Step 6 — One-time `webui_options` → `custom_options` migration
- [x] Step 7 — Slim WebUI mod: remove its JSON I/O, register plain defaults
- [x] Step 8 — Remove `ConfigFile.webui_options` field + docs/config updates
- [ ] Step 9 — Cabinet acceptance test (full gating matrix + precedence) — **pending maintainer deploy**

> Steps 1–8 implemented 2026-06-05 (auto mode via /code-assist). All compile
> clean under `cargo check --target x86_64-pc-windows-msvc`. Step 9 is the
> on-cabinet acceptance test — see the test plan handed to the maintainer (also
> mirrored in `.agents/scratchpad/20260531-custom-options-json-persistence/`).
> One small deviation from the design: the dirty-checked writer is **per-side**
> (`save_custom_options_values(side, values)`) rather than taking both sides at
> once, because `save_sender` fires once per carded-out side and must not clobber
> the absent player's block. Behaviorally equivalent; preserves the other side
> from the on-disk read.

---

## Step 1 — Config schema: split the gate
**Objective:** Replace `CustomOptionsConfig { persist }` with
`{ persist_network, persist_json }`, both `#[serde(default = "default_true")]`.
Remove the legacy `persist` field (Q5b).

**Guidance:** Edit `src/mods/config.rs` (L15–19). Update the one current reader in
`custom_options_persistence::init()` (L78–81) to read `persist_network` (same
behavior the old `persist` gated). Leave JSON wiring for later steps.

**Test/Demo:** `cargo check` clean. Deploy: with no config change, network
persistence still round-trips exactly as before (INFO line "save/load detours
installed"); a config with `persist_network: false` disables it. Confirms the
rename is behavior-preserving for the network path.

**Integration:** Self-contained; the only consumer is the persistence init gate.

## Step 2 — Gate detours + network emission on `persist_network`
**Objective:** Make `init()` install detours when *either* gate is on (R6), store
both gate values in `AtomicBool` statics, and guard the network child
emit/read inside the trampolines on `PERSIST_NETWORK`.

**Guidance:** In `custom_options_persistence.rs`: add `PERSIST_NETWORK` /
`PERSIST_JSON` statics; in `init()` read both gates, early-return only if *both*
false. Wrap the `<mod_{id}>` emit loop in `save_sender_trampoline` (L404–426) and
the read loop in `load_receiver_trampoline` (L494–516) in
`if PERSIST_NETWORK.load(...)`. `persist_json` is stored but not yet consumed.

**Test/Demo:** `cargo check` clean. Deploy matrix: `persist_network: true` →
network children emitted (existing log "emitting N mod options"); `false` → no
children, but detours still install if `persist_json: true` (new log: "detours
installed (network=off json=on)"). Confirms gating decoupled from network.

**Integration:** Builds on Step 1's struct; sets up the static flags Steps 4–5
consume.

## Step 3 — Dirty-checked custom_options block writer
**Objective:** Add `config::save_custom_options_values(p1, p2)` (R7) — a
read-modify-write that sets `root["custom_options"]["p1"]/["p2"]`, preserves the
gate keys and all other top-level keys, and **skips the disk write** if the
resulting `custom_options` block is byte-identical to the existing one.

**Guidance:** Model on `save_json_key` (L135). Compare new vs old
`root["custom_options"]` before `fs::write`. Keep it local to `config.rs` (Open
Q2 → local). On file-read failure, treat as "differs" and write (fail-safe).

**Test/Demo:** `cargo check` clean. Manual check via a temporary debug call
(removed before commit) or defer demo to Step 4. Confirms: calling twice with
identical values writes once. (Real demo arrives in Step 4 when it's wired.)

**Integration:** Pure addition; consumed by Step 4.

## Step 4 — JSON save on save_sender (gated on persist_json)
**Objective:** In `save_sender_trampoline`, when `PERSIST_JSON`, build the current
side's `{option_id: wire_value}` from `snapshot_for_save()` and persist via
`save_custom_options_values`, preserving the other side (R4, R5).

**Guidance:** Reuse the `snapshot_for_save()` call already present (L405) — it
applies `save_transform`, so wire values match the network path. Side comes from
`savedata+0x90` (L387–402). Read-modify-write preserves the opposite side. Wrap in
the surrounding null-guard style (R13).

**Test/Demo:** `cargo check` clean. Deploy: set a WebUI option for P1, card out →
`mod-config.json` gains `custom_options.p1.{...}` with asset_id values; card out
again with no change → file not rewritten (dirty-check; gate a one-shot "wrote
custom_options" log on the actual write). **First end-to-end JSON save.**

**Integration:** Wires Step 3 into the save path; first user-visible JSON output.

## Step 5 — One-shot lazy JSON load timer
**Objective:** When `persist_json`, spawn a one-shot background timer (~12s) that
re-reads `custom_options.{p1,p2}` from disk and calls
`custom_options::resolve_from_load(id, side, wire_value)` for each (R8, D1).

**Guidance:** Add `JSON_LOAD_DELAY_SECS = 12` const + `spawn_json_load_timer()` /
`json_load_once()` in the persistence module, matching the project's
`std::thread::spawn` + sleep idiom (lib.rs splash timer). **Re-read the file**,
not the `OnceCell` (research §C). Per Open Q1, the safest shape is: the timer
**primes the registry cache** via `resolve_from_load`; the actual game-state write
happens on the next scene-20 entry (WebUI's existing apply). Confirm on deploy
whether the off-thread `on_change`→`Customize` write is safe; if not, ensure
`resolve_from_load`'s callback effect is cache-only until scene 20.

**Test/Demo:** `cargo check` clean. Deploy: with a `custom_options.{p1,p2}` block
present, ~12s after boot an INFO line shows "primed N options from JSON"; entering
the options scene (scene 20) shows the cached values; no crash from the
background thread. **First end-to-end JSON load.**

**Integration:** Completes the JSON round-trip (save Step 4 ↔ load Step 5) using
the same registry APIs as network.

## Step 6 — One-time webui_options → custom_options migration
**Objective:** Add `config::migrate_webui_options_to_custom_options()` (R10) and
call it once in `init()` before the load timer is spawned.

**Guidance:** Read raw JSON; if `webui_options` exists and
`custom_options.{p1,p2}` absent, move contents under `custom_options.p1/p2` and
delete `webui_options`. Idempotent. Malformed `webui_options` → skip + warn, don't
delete.

**Test/Demo:** `cargo check` clean. Deploy with a pre-existing `webui_options`
block: after one boot, its contents appear under `custom_options.{p1,p2}` and
`webui_options` is gone; the Step 5 timer then primes those values. Second boot:
migration no-ops (already migrated).

**Integration:** Feeds legacy data into the Step 5 load path; runs before the
timer reads.

## Step 7 — Slim the WebUI mod
**Objective:** Remove all JSON I/O from `webui_options/mod.rs` (R11): delete
`load_from_json`, `save_to_json`, `side_key`, `CONFIG_KEY`, and the `save_to_json`
call in `try_apply_all`. Register options with plain `default_value(0)`; drop the
saved-default computation and P2 priming. Keep discovery, the scene-20 `Customize`
write, `on_value_changed`, and the registered transforms.

**Guidance:** `enable()` (L124–237): remove `load_from_json()` (L140) + the
default-idx block (L171–184) + P2 prime (L219–224). `try_apply_all` (L253–295):
drop `save_map` + `save_to_json(...)` tail (keep the field writes). The transforms
(`persist_save_transform`/`persist_load_transform`, L36–70) stay and remain wired
via `.persist_transform(...)` (L195).

**Test/Demo:** `cargo check` clean (no unused-import/dead-code warnings). Deploy:
WebUI options still apply at scene 20 and still save (now via the generic Step 4
path) and load (via Step 5). Behavior is unchanged from the user's perspective,
but the code path is the generic one. Confirms the move is complete.

**Integration:** WebUI now relies entirely on the framework for persistence;
closes the relocation.

## Step 8 — Remove ConfigFile.webui_options field + docs/config
**Objective:** Drop the now-unread `webui_options: Option<serde_json::Value>`
field from `ConfigFile` (config.rs L48, plus the two `None` initializers L73/L86).
Update the bundled `mod-config.json` (drop `persist`, add
`persist_network`/`persist_json`; the `webui_options` block is migrated at
runtime). Update `README.md` to document the two gates and the `custom_options`
offline-cache shape; update `AGENTS.md` config bullet if present.

**Guidance:** Safe because migration reads raw JSON, not the typed struct (so the
field's removal doesn't affect migration). Grep for any remaining `webui_options`
struct references in `src/`.

**Test/Demo:** `cargo check` clean; grep shows zero `webui_options` field reads in
`src/`. README/`mod-config.json` reflect the new schema. Docs-only + struct prune.

**Integration:** Final cleanup; the codebase no longer references the old key in
typed form.

## Step 9 — Cabinet acceptance test
**Objective:** Validate the full gating matrix and precedence on a live cabinet.

**Checks (from design Testing Strategy):**
1. Fresh config → both gates default on; card-out writes `custom_options.{p1,p2}`;
   no `webui_options`.
2. Pre-existing `webui_options` → migrated on first boot.
3. `persist_json: false` → no JSON write on card-out; network unaffected.
4. `persist_network: false, persist_json: true` → JSON works; no network children;
   detours still install.
5. Both false → no detours, options reset each swipe.
6. Dirty-check → identical card-outs write the file once.
7. Precedence (D2) → server-returned values win over JSON cache after swipe.
8. Lazy load (~12s) → INFO "primed N options"; scene 20 reflects cached values; no
   background-thread crash.

**Resolve Open Question 1 here:** confirm the timer-thread `resolve_from_load` →
`on_change` path is safe, or that the cache-only-prime + scene-20-apply shape is
in effect. If an off-thread `Customize` write is observed unsafe, fall back to
cache-only priming (already the leaning in Step 5).

## Notes
- Steps 1–8 each compile independently; 1–2 and 7–8 are individually revertable.
- The JSON round-trip is first demoable at Step 5 (save Step 4 + load Step 5).
- Highest-uncertainty item is Open Q1 (off-thread `on_change`), surfaced in Step 5
  and resolved in Step 9.
