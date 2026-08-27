# Progress — `raw-socket-network-fix`

Updated: 2026-07-21
Status: DONE — feature complete and VALIDATED on the Macbook + CrossOver install (boots
ONLINE, server connectivity confirmed). Build gates green. Awaiting maintainer commit only
(maintainer commits themselves).
NEXT ACTION: none for the agent. Maintainer to commit the feature files (new mod + the two
wiring edits + README + AGENTS.md); exclude the unrelated pre-existing tree changes noted
below.

Resume protocol: read `implementation/plan.md` (4-step checklist), `design/detailed-design.md`
(mechanism + ABI), `context.md` (verified code sites). RE record: `rough-idea.md`.

## Checklist

- [x] Step 1 — scaffold mod + register (mod.rs, lib.rs)
- [x] Step 2 — resolve `arkGetNetworkStatus` in `init`
- [x] Step 3 — promote-4→5 detour in `enable`, teardown in `disable`
- [x] Step 4 — docs (README, AGENTS.md, config note)
- [x] Build gates: `cargo check` → `cargo fmt` → `./build.sh` (all clean)
- [x] Cabinet/CrossOver validation — connectivity confirmed

Note: Steps 1–3 implemented together in one file write (small mod, shared struct); each
increment is distinct in the code and verified by the build gate. TDD adapted to build-gates
(no unit-test harness; behavioral test = live CrossOver deploy per plan Step 3).

## Done

- **Step 1–3** — `src/mods/raw_socket_network_fix.rs` (new): `RawSocketNetworkFixMod`
  implementing `Mod`. `init` resolves `arkmdxbio2!arkGetNetworkStatus` via
  `resolve_ark_module()` + `GetProcAddress`; `enable` installs a `GenericDetour`
  (`install_enabled`) whose body calls the original then promotes `*status` 4→5
  (panic/null-guarded, one-shot log); `disable` tears down + resets the log latch;
  `is_active()` reflects hook presence (self-disable). Registered in `src/mods/mod.rs`
  (alphabetical) and `src/lib.rs` `mods_to_register`.
- **Step 4** — README Included-Mods row + `mods` config example entry
  (`"raw-socket-network-fix": true`); AGENTS.md Key Entry Points row.
- **Build gates** — `cargo check --target x86_64-pc-windows-msvc` exit 0
  (`logs/cargo_check.log`); `cargo fmt --check` clean; `./build.sh` exit 0 →
  `target/x86_64-pc-windows-msvc/release/ddr_world_hook.dll` (`logs/build.log`).

## Deploy & test log

- **2026-07-21 — CrossOver (macOS), mod ON:** ✅ PASS. Game boots ONLINE and connects to the
  local server from the Macbook + CrossOver install (previously stuck on CHECKING → offline).
  Confirms the fix and retroactively confirms the investigation's diagnosis: the
  `arkGetNetworkStatus` value was the sole boot blocker; the raw-ICMP/keepalive failure was
  the root cause, and promoting CHECKING→ONLINE bypasses it cleanly.

## Deviations & open questions

- Default changed OFF→ON in design review (idea-honing Q3.5); no `mod_trait.rs` change.
- ABI from `arkmdxbio2_20260324` decompile; re-confirm on the exact deployed build if it
  differs (only `*p1` is touched, low risk).
- Uncommitted (per user: stop before commit). Unrelated pre-existing changes in the tree
  (`docs/custom_arrow_renderer_research.md`, the `log_*`/`packet_logs_*` investigation files)
  are NOT part of this feature and must be excluded from any commit.

## Key facts for a cold resume

- Hook target: `arkmdxbio2!arkGetNetworkStatus`, `unsafe extern "C" fn(*mut i32, *mut u8,
  *mut u32) -> u64`; promote `*p1` 4→5 only.
- Template: `src/mods/timing_offsets.rs`. Install: `core::hooks::install_enabled`.
- Resolve export: `resolve_ark_module()` + `GetProcAddress(handle, "arkGetNetworkStatus")`.
