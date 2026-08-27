# Context — `raw-socket-network-fix` (code-assist)

Working doc for the code-assist implementation. Planning spine lives alongside:
`../rough-idea.md`, `../idea-honing.md`, `design/detailed-design.md`,
`implementation/plan.md`.

## Project

- Rust `cdylib` hook DLL, target `x86_64-pc-windows-msvc`, cross-compiled from macOS
  (cargo-xwin). **No unit-test harness** — validation = `cargo check` → `cargo fmt` →
  `./build.sh`, then live deploy + log observation (AGENTS.md). No `CODEASSIST.md`.
- Logging via `log_info!`/`log_warn!` macros (crate root) → `OutputDebugStringA`.

## Task

Add mod `raw-socket-network-fix`: `GenericDetour` on `arkmdxbio2!arkGetNetworkStatus` that
promotes network status **CHECKING(4) → ONLINE(5)** so the boot network-check
(`LoadCommonEventSequence`) reaches ONLINE under CrossOver (raw ICMP sockets unavailable →
AVS keepalive disabled → status never flips). Config-gated, **default ON**, fail-open.

## Key existing code (verified by reading)

- **Template:** `src/mods/timing_offsets.rs` — single-file mod; `static mut HOOK:
  Option<GenericDetour<Fn>>`; detour body wraps `catch_unwind`, calls `h.call(...)` for the
  original; `install_hook`/`remove_hook`; `is_active()` = hook present; `required_signatures()
  = &[]`; self-disables in `enable` when the load-bearing target is unresolved.
- **Detour install:** `core::hooks::install_enabled(addr_of_mut!(HOOK), target, cb)` — stores
  handle before enabling (race-safe). `GenericDetour` works on any address incl. cross-DLL.
- **Export resolution:** `core::module_resolver::resolve_ark_module() -> Option<GameModule>`
  (`.handle: HMODULE`); then `GetProcAddress(handle, PCSTR("arkGetNetworkStatus\0"))`, pattern
  from `src/services/input_manager.rs::resolve_exports` (`CString` + `PCSTR` + `transmute`).
- **Registration:** add `pub mod raw_socket_network_fix;` to `src/mods/mod.rs`; add
  `Box::new(mods::raw_socket_network_fix::RawSocketNetworkFixMod::new())` to the
  `mods_to_register` vec in `src/lib.rs` (~line 107-126).
- **Default ON:** `mod_trait.rs::enable_with_config` uses `unwrap_or(true)` → omitted key
  enables the mod. No change needed there (per Q3.5).

## ABI (from Ghidra `arkmdxbio2_20260324` @ 180003ef0)

`undefined8 arkGetNetworkStatus(int* status /*p1*/, undefined1* p2, undefined4* p3)` — writes
status to `*p1`, plus two secondary out-params; returns 0. Rust:
`unsafe extern "C" fn(*mut i32, *mut u8, *mut u32) -> u64`. Touch only `*p1` (4→5); forward
p2/p3 and the return value untouched.

## Status enum

0 LOCAL, 1 OFFLINE, 2 MAINTENANCE, 3 blank, 4 CHECKING, 5 ONLINE, 7 NOT AVAILABLE. Map 4→5
only; all others identity.
