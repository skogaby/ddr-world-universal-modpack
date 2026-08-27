# On-Cabinet Test Plan — Custom-Options JSON Persistence (Step 9)

Steps 1–8 are implemented and compile clean. This is the live acceptance test
(plan Step 9). All observation is via the `[DDR-Hook]` log channel (spice2x log
file / DebugView) plus inspecting `mod-config.json` on the cabinet.

## 0. Build & deploy

```bash
cargo check --target x86_64-pc-windows-msvc   # already green
./scripts/deploy.sh                            # build release + SCP the DLL
```

Turn on persistence-relevant logging by leaving `layeredfs.verbose` off but
watching for `custom_options_persistence:` and `Config:` lines. (The key INFO
lines below are emitted at INFO, so no verbose flag needed.)

## 1. Log lines to watch for

| Line | Meaning |
|------|---------|
| `custom_options_persistence: save/load detours installed (network=… json=…)` | init succeeded; shows both gate states |
| `custom_options_persistence: both gates off … — no detours` | both gates false → no detours installed |
| `custom_options_persistence: disabled by config (persist_network=false)` | *should NOT appear* — this string was removed; if you see it the wrong build is loaded |
| `Config: migrated webui_options → custom_options.{p1,p2}` | one-time migration fired |
| `custom_options_persistence: save — emitting N mod options for side S` | network children emitted (card-out) |
| `custom_options_persistence: save — wrote N option(s) to mod-config.json custom_options.pX` | JSON cache write happened (dirty-check passed) |
| `custom_options_persistence: JSON load — primed N option value(s) from mod-config.json` | lazy timer (~12s) primed the registry |
| `custom_options_persistence: JSON load — no cached custom_options values found` | timer ran, nothing cached |

## 2. The 8 acceptance checks

Use the matching `mod-config.json` from §3 for each. Restart the game between
config edits (config is read once at boot).

### Check 1 — Fresh config, both gates default on
- **Config:** `custom_options` section absent entirely (or `{}`).
- **Do:** boot, card in, set a WebUI option (e.g. lane skin) for P1, card out.
- **Expect:** `custom_options.p1.{...}` appears in `mod-config.json` with asset_id
  wire values; no `webui_options` key anywhere; INFO "wrote N option(s) … p1".
  Detours-installed line shows `network=true json=true`.

### Check 2 — Migration of a pre-existing `webui_options` block
- **Config:** include a legacy `webui_options: { p1: {"lane_skin_single": 12}, p2: {} }`
  and **no** `custom_options.p1/p2` data (see §3.2).
- **Do:** boot once. Inspect `mod-config.json`.
- **Expect:** `Config: migrated webui_options → custom_options.{p1,p2}` in log;
  `webui_options` key gone; its contents now under `custom_options.p1`. ~12s later
  "JSON load — primed N". Second boot: migration does **not** fire again (idempotent).

### Check 3 — `persist_json: false` (network only)
- **Config:** `custom_options: { persist_network: true, persist_json: false }`.
- **Do:** card in, change an option, card out.
- **Expect:** NO "wrote … to mod-config.json" line; `custom_options.p1/p2` NOT
  written/updated; network still emits ("emitting N mod options"). Detours line:
  `network=true json=false`.

### Check 4 — `persist_network: false, persist_json: true` (JSON only)
- **Config:** `custom_options: { persist_network: false, persist_json: true }`.
- **Do:** card in, change an option, card out; reboot.
- **Expect:** detours STILL install (line: `network=false json=true`); NO
  "emitting N mod options" line (network children suppressed); JSON write happens
  ("wrote … p1"); after reboot, ~12s "JSON load — primed N"; option reflects the
  cached value at the options screen (scene 20).

### Check 5 — Both gates off
- **Config:** `custom_options: { persist_network: false, persist_json: false }`.
- **Do:** boot.
- **Expect:** `both gates off … — no detours`; no save/load activity; options reset
  to defaults each card swipe; no JSON writes.

### Check 6 — Dirty-check
- **Config:** §3.1 (both on).
- **Do:** card in, **don't change anything**, card out. Note `mod-config.json`
  mtime. Card in/out again with no change.
- **Expect:** on the no-change card-outs, NO "wrote N option(s)" INFO (you'll see
  the DEBUG "JSON cache unchanged … skipped write" only if debug logging is on);
  file mtime does not advance. Change a value → next card-out DOES write.

### Check 7 — Precedence (network wins over JSON)
- **Setup:** a server/profile that actually returns custom-option values, plus a
  JSON cache (§3.1 with some `custom_options.p1` values that DIFFER from the
  server's).
- **Do:** boot; wait past ~12s (JSON primes); card in (network load_receiver fires).
- **Expect:** after card-in the option reflects the **server** value, not the JSON
  value. (Timer is one-shot and fired before the swipe; load_receiver re-applies.)

### Check 8 — Lazy load + off-thread safety (**resolves Open Q1**)
- **Config:** §3.1 with non-default `custom_options.p1` values (e.g. a distinctive
  lane skin) and `p2` values too.
- **Do:** boot, do NOT card in. Watch the log around the 12s mark. Then card in P1
  and open the options menu (scene 20).
- **Expect:** ~12s: `JSON load — primed N option value(s)`. **No crash / no access
  violation** from the background timer thread while in attract mode (this is the
  off-thread `on_change` safety check — WebUI's apply early-returns on the null
  pre-login player pointer). On entering options after card-in, the rows show the
  primed values. If you DO see a crash or an `EXCEPTION_ACCESS_VIOLATION` near the
  12s mark, that's the Open-Q1 risk materializing → report it; the fix is to make
  `resolve_from_load`'s WebUI callback cache-only until scene 20 (it already
  *should* be, via the null-pointer guard).

## 3. Ready-to-use `mod-config.json` variants

The repo's bundled `mod-config.json` already has the §3.1 shape. Swap the
`custom_options` block for the others as needed. (The `mods`/`layeredfs`/
`series_expansion`/`folder_expansion` blocks are unchanged from the repo copy —
shown abbreviated here; keep yours as-is.)

### 3.1 — Default (both gates on) — use for Checks 1, 6, 7, 8
```json
{
  "mods": { "...": "(keep your existing mods block)" },
  "layeredfs": { "verbose": false, "developer_mode": false, "mod_folder": "./data_mods" },
  "series_expansion": { "...": "(keep yours)" },
  "folder_expansion": { "...": "(keep yours)" },
  "custom_options": {
    "persist_network": true,
    "persist_json": true,
    "p1": {},
    "p2": {}
  }
}
```
For Checks 7/8, pre-seed `p1`/`p2` with real values, e.g.:
```json
  "custom_options": {
    "persist_network": true,
    "persist_json": true,
    "p1": { "lane_skin_single": 12, "lane_skin_double": 4 },
    "p2": { "lane_skin_single": 3 }
  }
```
(Option ids are the WebUI category ids; the values are **asset_ids** — the same
wire values the network path stores. Easiest way to get valid values: let Check 1
write them for you, then reuse.)

### 3.2 — Migration fixture — use for Check 2
```json
  "custom_options": {
    "persist_network": true,
    "persist_json": true
  },
  "webui_options": {
    "p1": { "lane_skin_single": 12 },
    "p2": {}
  }
```
(Note: `custom_options` has NO `p1`/`p2` here — that's what triggers the migration.
After boot, expect `webui_options` gone and its data under `custom_options.p1`.)

### 3.3 — JSON only — use for Check 4
```json
  "custom_options": { "persist_network": false, "persist_json": true, "p1": {}, "p2": {} }
```

### 3.4 — Network only — use for Check 3
```json
  "custom_options": { "persist_network": true, "persist_json": false }
```

### 3.5 — Both off — use for Check 5
```json
  "custom_options": { "persist_network": false, "persist_json": false }
```

## 4. Pass criteria

All 8 checks behave as described, with particular attention to:
- **Check 8** — no background-thread crash at ~12s (Open Q1). This is the one item
  that could still require a follow-up change; everything else is mechanical.
- **Check 7** — server values win (precedence).
- **Check 6** — no redundant writes (dirty-check).

If Check 8 surfaces an off-thread crash, the contained fix is already scoped in the
design (Open Q1): keep the timer's `resolve_from_load` effect cache-only and rely
on the scene-20 apply — which the current null-pointer guard in `try_apply_all`
should already provide. Re-deploy and re-run Check 8 to confirm.
