# Implementation Plan — `raw-socket-network-fix`

Design: `../design/detailed-design.md`. Requirements: `../idea-honing.md`. RE record:
`../rough-idea.md`.

This repo has **no unit tests** — validation is `cargo check`/`cargo fmt`/`./build.sh`
plus live deploy to the CrossOver install and log/behavior observation. Each step below
therefore folds its verification into the step (build gate + a specific observable), not a
separate "add tests" step. Steps are small and each leaves the tree building and the mod
in a demoable state.

Readiness gates before any commit (AGENTS.md): `cargo check --target
x86_64-pc-windows-msvc` clean → `cargo fmt` (whole crate, never file args) → `./build.sh`
clean.

## Checklist

- [ ] **Step 1** — Scaffold the mod (implements `Mod`, no hook yet) and register it
- [ ] **Step 2** — Resolve `arkGetNetworkStatus` (arkmdxbio2 export) in `init`
- [ ] **Step 3** — Install the promote-4→5 detour in `enable`, tear down in `disable`
- [ ] **Step 4** — Documentation (README mods table, AGENTS.md, config note)

---

## Step 1: Scaffold the mod and wire it into the registry

**Objective:** Create `src/mods/raw_socket_network_fix.rs` with a `RawSocketNetworkFixMod`
implementing the `Mod` trait (`id = "raw-socket-network-fix"`, name, description,
`required_signatures() = &[]`, `init` returns `true`, `enable`/`disable` log only for now,
`is_active()` default). Add `pub mod raw_socket_network_fix;` to `src/mods/mod.rs` and a
`Box::new(mods::raw_socket_network_fix::RawSocketNetworkFixMod::new())` entry to the
`mods_to_register` vec in `src/lib.rs`.

**Guidance:** Model the skeleton on `src/mods/timing_offsets.rs`. No `arkmdxbio2` access
yet — `enable()` just `log_info!`s that it activated. Keep `unsafe impl Send` on the struct
(matches other mods) even though it holds nothing raw yet, so Step 2/3 don't churn it.

**Verification:** `cargo check --target x86_64-pc-windows-msvc` clean; `cargo fmt` clean.
The mod appears in `ModRegistry` and (because omitted config keys default ON) is enabled at
boot.

**Demo:** Boot log shows `Mod registered: Raw Socket Network Fix (raw-socket-network-fix)`
and `Mod enabled: Raw Socket Network Fix`; the mod appears as a toggle row in the in-game
mod menu (triple-press 0), and toggling it logs enable/disable. No behavior change yet.

---

## Step 2: Resolve the `arkGetNetworkStatus` export in `init`

**Objective:** In `init`, resolve `arkmdxbio2!arkGetNetworkStatus` via
`core::module_resolver::resolve_ark_module()` + `GetProcAddress(module.handle,
"arkGetNetworkStatus")`, `transmute` to the `GetNetworkStatusFn` type, and store it on the
mod (`Option<GetNetworkStatusFn>`). Log the resolved address, or `log_warn!` and store
`None` if the module/export is missing. `init` still returns `true` (self-disable is
deferred to `enable`, matching `timing_offsets`).

**Guidance:** Copy the resolve closure shape from `input_manager::resolve_exports`
(`CString` + `PCSTR` + `GetProcAddress`). Define:
```rust
type GetNetworkStatusFn = unsafe extern "C" fn(*mut i32, *mut u8, *mut u32) -> u64;
```
Do **not** install anything yet.

**Verification:** `cargo check` + `cargo fmt` clean. On the CrossOver install, boot log
shows the resolved `arkGetNetworkStatus @ 0x…`. Confirm the address is inside arkmdxbio2's
range (sanity).

**Demo:** Boot log line `RawSocketNetworkFix: resolved arkGetNetworkStatus @ <addr>` (or a
warning + self-disable if it can't be found). Still no behavior change to the status.

---

## Step 3: Install the promote-4→5 detour (core functionality)

**Objective:** Add the detour body and lifecycle. In `enable()`: if the target is `None`,
`log_warn!` and return (self-disabled). Otherwise install via
`core::hooks::install_enabled(addr_of_mut!(STATUS_HOOK), target, status_detour)`; on `Err`,
log and return. The detour calls the original first, then — panic-guarded and
null-guarded — rewrites `*status` from `4` to `5`, logging the first promotion once
(`AtomicBool` one-shot). In `disable()`: `take()`+`disable()` the detour and reset the
one-shot latch. Implement `is_active()` as `unsafe { (*addr_of!(STATUS_HOOK)).is_some() }`.

**Guidance:** Mirror `timing_offsets`'s `install_hook`/`remove_hook`/`is_active` and
`input_manager`'s "call original then adjust out-param" order. Detour:
```rust
unsafe extern "C" fn status_detour(p1: *mut i32, p2: *mut u8, p3: *mut u32) -> u64 {
    let ret = if let Some(ref h) = *std::ptr::addr_of!(STATUS_HOOK) { h.call(p1, p2, p3) } else { 0 };
    let _ = std::panic::catch_unwind(|| {
        if !p1.is_null() && *p1 == 4 { *p1 = 5;
            if !ONE_SHOT_LOGGED.swap(true, Ordering::AcqRel) {
                log_info!("RawSocketNetworkFix: promoted network status CHECKING(4) -> ONLINE(5)");
            }
        }
    });
    ret
}
```
Constants `CHECKING = 4`, `ONLINE = 5`. Carry the disable-race note from
`timing_offsets::remove_hook` as a comment.

**Verification:** `cargo check` + `cargo fmt` + `./build.sh` clean. Then the primary live
test on the CrossOver install (mod ON): boot clears CHECKING without the ~30 s stall; log
shows `ArkNetwork.Lock() start … LoadCommonEventSequence::onUpdate` +
`EssCallAndWaitBase3MusicDataLoad::request()`; `playdata_3.musicdata_load` (then
playerdata/rivaldata) requests hit the server; bottom bar reads `ONLINE`; the one-shot
promotion line appears exactly once. Regression: set `"raw-socket-network-fix": false` →
reverts to stuck-CHECKING/offline (confirms gate + fail-open).

**Demo:** DDR World boots **ONLINE** under CrossOver against the local server — card-in,
profile load, and score save all work — where before it was stuck offline. This is the
feature's end-to-end payoff.

---

## Step 4: Documentation

**Objective:** Document the mod: add a row to the **Included Mods** table in `README.md`; add
a **Key Entry Points** row (and any relevant note) to `AGENTS.md`; ensure the `mods` config
example/notes mention `raw-socket-network-fix` and that it's default-ON (disable via
`false`). Note the CrossOver/raw-ICMP context and the "takes effect next boot" characteristic.

**Guidance:** Match the tone/format of existing entries. Keep the README row concise; the
AGENTS.md row should point at `src/mods/raw_socket_network_fix.rs` and summarize the hook
(`arkGetNetworkStatus` 4→5) and the root cause (raw ICMP unavailable under CrossOver).
Reference `.agents/planning/20260721-raw-socket-network-fix/` for the full record.

**Verification:** Docs render correctly; no build impact. `cargo fmt` still clean.

**Demo:** A reader of README/AGENTS.md can see the mod exists, what it does, when to enable
it, and where its code + design live.

---

## Notes

- **Per-feature progress tracking (AGENTS.md):** start a `progress.md` in this feature dir
  at implementation kickoff and update it after each step / before any handoff (live resume
  point).
- **No new signatures** are added to `core/signatures.rs` — resolution is a direct
  `GetProcAddress` on an arkmdxbio2 export, so nothing to register there.
- **Deploy:** `./scripts/deploy.sh` pushes the DLL; the CrossOver install picks it up on
  next launch (the status override is latched at the boot network-check, so it takes effect
  on the next boot).
