//! Software-identity (ea3 ident) **rev override** — always-on, no config.
//!
//! Every DDR World running this modpack identifies itself with the software
//! revision letter [`OVERRIDE_REV`] (`M`) instead of whatever `<rev>` the
//! operator has in `prop/ea3-ident.xml` (stock cabinets ship `A`), so the
//! soft-id code becomes `MDX:J:F:M:<ext>` everywhere the game presents it:
//! the `model=` attribute of every eamuse request, the ea3-share filter, the
//! `/env/profile/soft_id_code` AVS environment value, and therefore the
//! title-screen / test-menu version string (`arkGetSoftIDCode` →
//! `ess_soft_info_get(1)` → `avs_std_getenv("/env/profile/soft_id_code")`).
//!
//! ## Why the seam is `ea3_boot`
//!
//! spice2x reads `prop/ea3-ident.xml` with its OWN CRT `fopen` (not the AVS
//! fs API — LayeredFS never sees it), builds the `/ea3/soft/{model,dest,spec,
//! rev,ext}` nodes into the ea3-config property, passes the `/ea3` node to
//! libavs-ea3's `ea3_boot`, and it is `ea3_boot` that (1) `psmap_import`s
//! the five soft-id fields into its global config struct, (2) composes the
//! `"%s:%s:%s:%s:%s"` soft-id code the xrpc layer points its `model=`
//! attribute at, and (3) re-`setenv`s `/env/profile/soft_id_code` from that
//! struct — overwriting spice2x's pre-init value. gamemdx itself never
//! consumes the `dll_entry_init` init-code string (`gameInit` only logs its
//! module handle) — every game-side read goes through the env value, which
//! is only read lazily after boot. So rewriting the ONE `soft/rev` node
//! before the original `ea3_boot` runs makes every downstream copy agree.
//!
//! Mechanism: one `GenericDetour` on the ea3 boot export (resolved by
//! spice2x's per-AVS-version export-name table, then VALIDATED by the
//! function body's `LEA rcx,["ea3-boot"]; LEA rdx,["startup"]` log-call
//! prologue, with an AOB fallback), and libavs-win64's property API resolved
//! by its 2.16.[3-8] export names (`property_search` / `node_create` /
//! `node_remove` / `node_get_desc` / `node_refer`), gated on the same
//! `XCnbrep700013c` disambiguator LayeredFS uses (2.16.1 shares the prefix
//! with a different numbering). Node rewrite is create-new-THEN-remove-old
//! and verified by re-searching `soft/rev`, so any failure leaves the stock
//! node in place (fail-open: stock identity, one WARN).
//!
//! Untouched on purpose: spice2x's own `avs::game::REV` copy (its
//! `soft id code:` / troubleshooter log lines keep the file's letter), the
//! `/env/profile/security_code` value (`G*MDXJFA` — read only by
//! `arkmdxbio2!dll_entry_init` BEFORE ea3 boot, for dest U/E spec B/C
//! cabinets), and the init-code buffer (no consumer). Timing: with `-z`
//! (early hook) the DLL loads before libavs exists, so `init` waits for the
//! two libavs modules (bounded by gamemdx's appearance — it loads after
//! them); with `-k` they are already resident. Either way the detour is in
//! place hundreds of ms before the launcher reaches `ea3_boot`
//! (`dll_entry_init` + patcher sit in between); a 30 s watchdog WARNs if the
//! boot was never observed.
//!
//! Safety analysis of changing the rev (which stock code paths read it) is
//! in `docs/soft_id_rev_override_research.md`; the known rev-sensitive stock
//! idents produce a boot WARN via [`rev_sensitive_reason`].

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
use std::ffi::CString;
#[cfg(windows)]
use std::panic::{catch_unwind, AssertUnwindSafe};
#[cfg(windows)]
use std::ptr::{addr_of, addr_of_mut};

#[cfg(windows)]
use retour::GenericDetour;
#[cfg(windows)]
use windows::core::PCSTR;
#[cfg(windows)]
use windows::Win32::Foundation::HMODULE;
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetProcAddress;

#[cfg(windows)]
use crate::core::module_resolver::{self, GameModule};
#[cfg(windows)]
use crate::core::{hooks, memory, scanner};
#[cfg(windows)]
use crate::{log_info, log_warn};

/// The revision letter every modpack cabinet reports. Must be a single
/// uppercase ASCII letter — libavs-ea3's `check_soft_id_code` rejects
/// anything else with a fatal "bad rev" at boot. `M`, not the community's
/// `X`: bemaniutils files a DDR rev of `X` as an omnimix (separate
/// `music_version` bucket), while no known server keys on `M`.
pub const OVERRIDE_REV: u8 = b'M';

/// kbin node type id for `str` (spice2x `NODE_TYPE_str`; the persistence
/// service emits its string fields with the same id).
const KBIN_TYPE_STR: i32 = 11;

static INSTALLED: AtomicBool = AtomicBool::new(false);
static BOOT_OBSERVED: AtomicBool = AtomicBool::new(false);

#[must_use]
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::Acquire)
}

/// Whether the detour has seen the launcher's `ea3_boot` call (i.e. the
/// override had its chance to apply).
#[must_use]
pub fn boot_observed() -> bool {
    BOOT_OBSERVED.load(Ordering::Acquire)
}

// ── Pure helpers (host-testable) ────────────────────────────────────

/// Compose the soft-id code the game will present, `MODEL:D:S:R:EXT`.
pub fn compose_soft_id(model: &str, dest: &str, spec: &str, rev: &str, ext: &str) -> String {
    format!("{model}:{dest}:{spec}:{rev}:{ext}")
}

/// Stock idents whose rev letter SELECTS behaviour in the shipped binaries
/// (from the RE sweep of every `soft_id_code` consumer in gamemdx 20260825 +
/// arkmdxbio2 20260721 — see `docs/soft_id_rev_override_research.md`).
/// Returns the consequence of overriding the rev for such an ident, so the
/// boot log can warn the operator; `None` for every other ident (including
/// the stock `MDX:J:F:A`), where the rev is display/wire-only.
pub fn rev_sensitive_reason(
    model: &str,
    dest: &str,
    spec: &str,
    rev: &str,
) -> Option<&'static str> {
    match (model, dest, spec, rev) {
        // gamemdx `Timing Init` (FUN_18002bcd0): strncmp(soft_id, tbl[i], 9)
        // against these three 9-char prefixes selects timing preset 9
        // (sound 105 / input 28 / render 30) instead of the machine-type
        // default (preset 2: sound 124 / render 36). Moot when the
        // timing_offsets mod has values configured (its setter hook wins).
        ("MDX", "A", "F", "B") | ("MDX", "Y", "F", "B") | ("MDX", "E", "K", "B") => Some(
            "gamemdx Timing Init selects stock timing preset 9 only for MDX:A:F:B / MDX:Y:F:B / \
             MDX:E:K:B; with the rev overridden the machine-type default preset applies \
             (irrelevant when timing_offsets values are configured)",
        ),
        // arkmdxbio2 FUN_1800172d0: J-cabinet premium/galaxy pricing
        // eligibility allowlist `MDX:J:I:{A,B,C}` (strncmp 9), gating the
        // coin-option item01/item02 price limits and the boot-time
        // need_premium_setting / need_galaxy_setting migrations.
        ("MDX", "J", "I", "A") | ("MDX", "J", "I", "B") | ("MDX", "J", "I", "C") => Some(
            "arkmdxbio2 limits J gold-cabinet premium/galaxy pricing eligibility to MDX:J:I:{A,B,C}; \
             with the rev overridden those coin-option paths treat the cabinet as ineligible",
        ),
        // arkmdxbio2 FUN_180004810: `TDX:U:J:C` (strncmp 9) selects
        // /prop/ark-config.xml; any other U:J ident selects ark-config_nopp.
        ("TDX", "U", "J", "C") => Some(
            "arkmdxbio2 loads /prop/ark-config.xml only for TDX:U:J:C; with the rev overridden it \
             loads /prop/ark-config_nopp.xml",
        ),
        _ => None,
    }
}

/// Whether `rev` is a value libavs-ea3's `check_soft_id_code` accepts: one
/// uppercase ASCII letter.
pub fn is_valid_rev(rev: &str) -> bool {
    matches!(rev.as_bytes(), [b] if b.is_ascii_uppercase())
}

// ── Windows side ────────────────────────────────────────────────────

/// `void ea3_boot(property_node* ea3)` — spice2x passes the `/ea3` node of
/// the ea3-config property it built from `prop/ea3-config.xml` +
/// `prop/ea3-ident.xml`.
#[cfg(windows)]
type Ea3BootFn = unsafe extern "C" fn(*mut u8);

/// libavs-win64 2.16.[3-8] property API (export names from spice2x's
/// `IMPORT_AVS21630` table; semantics verified in Ghidra on 2.16.3):
/// `property_search(property, node, path) -> node` — either of the first
/// two may be NULL.
#[cfg(windows)]
type FnPropertySearch = unsafe extern "C" fn(*mut u8, *mut u8, *const i8) -> *mut u8;
/// `property_node_create(property, parent, type, path, ...)` — str view:
/// for kbin type 11 the variadic slot carries the NUL-terminated string
/// POINTER (x64 MSVC variadic ints/pointers land exactly where a fixed 5th
/// parameter would).
#[cfg(windows)]
type FnPropertyNodeCreateStr =
    unsafe extern "C" fn(*mut u8, *mut u8, i32, *const i8, *const i8) -> *mut u8;
/// `property_node_remove(node)` — unlinks and releases the node.
#[cfg(windows)]
type FnPropertyNodeRemove = unsafe extern "C" fn(*mut u8) -> i32;
/// `property_node_get_desc(node) -> property` — the owning property object
/// (the same value `property_search` derives internally from a node).
#[cfg(windows)]
type FnPropertyNodeGetDesc = unsafe extern "C" fn(*mut u8) -> *mut u8;
/// `property_node_refer(property, node, path, type, data, size) -> status`
/// (< 0 on error). For `str` it copies the NUL-terminated value into `data`.
#[cfg(windows)]
type FnPropertyNodeRefer =
    unsafe extern "C" fn(*mut u8, *mut u8, *const i8, i32, *mut u8, u32) -> i32;

#[cfg(windows)]
#[derive(Clone, Copy)]
struct PropertyApi {
    search: FnPropertySearch,
    node_create_str: FnPropertyNodeCreateStr,
    node_remove: FnPropertyNodeRemove,
    node_get_desc: FnPropertyNodeGetDesc,
    node_refer: FnPropertyNodeRefer,
}

#[cfg(windows)]
static mut PROPERTY_API: Option<PropertyApi> = None;
#[cfg(windows)]
static mut EA3_BOOT_HOOK: Option<GenericDetour<Ea3BootFn>> = None;

/// Export present in libavs-win64 2.16.[3-8] but not 2.16.1 — the same
/// disambiguator `avs_layeredfs::avs_resolver` keys its version table on.
/// 2.16.1 shares the `XCnbrep7` prefix with DIFFERENT numbering, so the
/// names below would silently resolve to unrelated functions there.
#[cfg(windows)]
const LIBAVS_2163_MARKER: &str = "XCnbrep700013c";

/// spice2x `avs/ea3.cpp` boot export names, most likely first. NOTE
/// `XEyy2igh000007` is `ea3_boot` on 2.16.3+ but `ea3_shutdown` on 2.16.1
/// (whose boot is `XEyy2igh000006`) — which is why every candidate is
/// validated by body content before it is hooked.
#[cfg(windows)]
const EA3_BOOT_EXPORT_NAMES: &[&str] = &[
    "XEyy2igh000007", // 2.16.3.0 / 2.16.5.1 / 2.16.7.1 / 2.16.8.1
    "XEmdwapa000024", // 2.17.0.0 / 2.17.3.0
    "XEyy2igh000006", // 2.16.1.0
    "XE592acd00008c", // 2.15.8.0
    "XE7aee11000070", // 2.14.3.0
    "XEb552d500005d", // 2.13.6.0
    "ea3_boot",       // legacy (unmangled)
];

/// libavs-win64-ea3 2.16.3 `ea3_boot` prologue (AOB fallback when no export
/// name validates): `PUSH RDI; PUSH R12; SUB RSP,0x258; MOV RDI,RCX; CALL
/// is_booted; MOVZX EAX,AL; TEST EAX,EAX; JZ +0x0B; ADD RSP,0x258; POP R12;
/// POP RDI; RET; LEA RCX,["ea3-boot"]; LEA RDX,["startup"]; CALL log`.
/// Byte-identical on both 2.16.3 r7106 ea3 DLLs in the reference set.
#[cfg(windows)]
const EA3_BOOT_AOB: &str = "57 41 54 48 81 EC 58 02 00 00 48 89 CF E8 ?? ?? ?? ?? 0F B6 C0 85 C0 74 0B \
                            48 81 C4 58 02 00 00 41 5C 5F C3 48 8D 0D ?? ?? ?? ?? 48 8D 15 ?? ?? ?? ?? E8";

/// How far into a candidate function the `"ea3-boot"` / `"startup"` log
/// call must appear (it is the first thing after the is-booted early-out).
#[cfg(windows)]
const BOOT_MARKER_WINDOW: usize = 0x80;
/// Max distance from the `"ea3-boot"` LEA to its `"startup"` partner.
#[cfg(windows)]
const BOOT_MARKER_PAIR_WINDOW: usize = 0x20;

/// Bound on the module wait when neither libavs DLL nor gamemdx ever shows
/// up (a foreign launcher) — never let this service wedge the rest of init.
#[cfg(windows)]
const MODULE_WAIT_MAX_MS: u64 = 120_000;
/// The launcher reaches `ea3_boot` within ~1–2 s of loading hook DLLs; if
/// the detour has not fired by then something else is going on.
#[cfg(windows)]
const BOOT_WATCHDOG_SECS: u64 = 30;

/// Resolve everything and install the `ea3_boot` detour. Always-on:
/// nothing here consults `mod-config.json`. Idempotent, fail-open (a miss
/// logs one WARN and leaves the stock identity alone). Call as the FIRST
/// step of `lib::init` — under spice2x `-z` the DLL loads before libavs, so
/// this waits (bounded) for the two libavs modules, which the launcher
/// loads long before it reaches `ea3_boot`.
#[cfg(windows)]
pub fn init() -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let Some((avs, ea3)) = wait_for_libavs_modules() else {
        log_warn!(
            "ident_override: libavs-win64 / libavs-win64-ea3 never appeared -- rev override unavailable (stock identity)"
        );
        return false;
    };
    let Some(api) = resolve_property_api(avs.handle) else {
        return false;
    };
    let Some(target) = resolve_ea3_boot(&ea3) else {
        log_warn!(
            "ident_override: could not locate ea3_boot in {} -- rev override unavailable (stock identity)",
            ea3.name
        );
        return false;
    };
    unsafe {
        // Store-before-enable (see `hooks::install_enabled`): the callback
        // reads both statics, so both must be populated before the patch.
        PROPERTY_API = Some(api);
        if let Err(error) =
            hooks::install_enabled(addr_of_mut!(EA3_BOOT_HOOK), target, ea3_boot_hook)
        {
            PROPERTY_API = None;
            log_warn!(
                "ident_override: ea3_boot detour installation failed: {} -- rev override unavailable",
                error
            );
            return false;
        }
    }
    INSTALLED.store(true, Ordering::Release);
    log_info!(
        "ident_override: ea3_boot detour installed @ {:p} -- soft-id rev will report as '{}'",
        target as *const (),
        OVERRIDE_REV as char
    );
    spawn_boot_watchdog();
    true
}

#[cfg(not(windows))]
pub fn init() -> bool {
    false
}

/// Wait until both libavs DLLs are resident. Bounded by gamemdx's presence
/// (the launcher loads it AFTER libavs core + ea3, so once it exists a
/// missing libavs is a real miss, not a race) and a hard timeout.
#[cfg(windows)]
fn wait_for_libavs_modules() -> Option<(GameModule, GameModule)> {
    let started = std::time::Instant::now();
    loop {
        let avs = module_resolver::resolve_libavs_module();
        let ea3 = module_resolver::resolve_libavs_ea3_module();
        if let (Some(avs), Some(ea3)) = (avs, ea3) {
            return Some((avs, ea3));
        }
        if module_resolver::get_game_module().is_some()
            || started.elapsed().as_millis() as u64 > MODULE_WAIT_MAX_MS
        {
            // One last look, then give up.
            return module_resolver::resolve_libavs_module()
                .zip(module_resolver::resolve_libavs_ea3_module());
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[cfg(windows)]
unsafe fn get_proc(handle: HMODULE, name: &str) -> Option<*const ()> {
    let cname = CString::new(name).ok()?;
    GetProcAddress(handle, PCSTR(cname.as_ptr() as *const u8)).map(|f| f as *const ())
}

/// Resolve the five property functions by their 2.16.[3-8] export names,
/// refusing on any other AVS version (marker export absent).
#[cfg(windows)]
fn resolve_property_api(avs: HMODULE) -> Option<PropertyApi> {
    unsafe {
        if get_proc(avs, LIBAVS_2163_MARKER).is_none() {
            log_warn!(
                "ident_override: libavs-win64 is not 2.16.[3-8] (marker export {} absent) -- rev override unavailable (stock identity)",
                LIBAVS_2163_MARKER
            );
            return None;
        }
        macro_rules! resolve {
            ($name:literal, $ty:ty) => {{
                let Some(p) = get_proc(avs, $name) else {
                    log_warn!(
                        "ident_override: libavs-win64 export {} missing -- rev override unavailable",
                        $name
                    );
                    return None;
                };
                #[allow(clippy::missing_transmute_annotations)]
                let typed: $ty = std::mem::transmute(p);
                typed
            }};
        }
        Some(PropertyApi {
            search: resolve!("XCnbrep70000a1", FnPropertySearch),
            node_create_str: resolve!("XCnbrep70000a2", FnPropertyNodeCreateStr),
            node_remove: resolve!("XCnbrep70000a3", FnPropertyNodeRemove),
            node_get_desc: resolve!("XCnbrep70000ae", FnPropertyNodeGetDesc),
            node_refer: resolve!("XCnbrep70000af", FnPropertyNodeRefer),
        })
    }
}

/// Find `ea3_boot`: try the known export names, validating each candidate
/// by body content; fall back to the 2.16.3 prologue AOB (also validated).
#[cfg(windows)]
fn resolve_ea3_boot(ea3: &GameModule) -> Option<Ea3BootFn> {
    unsafe {
        for name in EA3_BOOT_EXPORT_NAMES {
            let Some(addr) = get_proc(ea3.handle, name) else {
                continue;
            };
            let addr = addr as *const u8;
            if validate_ea3_boot_body(addr, ea3) {
                log_info!(
                    "ident_override: ea3_boot = export {} (body validated)",
                    name
                );
                return Some(std::mem::transmute::<*const u8, Ea3BootFn>(addr));
            }
            log_warn!(
                "ident_override: export {} did not validate as ea3_boot (no \"ea3-boot\"/\"startup\" log prologue) -- skipped",
                name
            );
        }
        let hit = scanner::scan_pattern(ea3.base, ea3.size, EA3_BOOT_AOB)?;
        if !validate_ea3_boot_body(hit.address, ea3) {
            return None;
        }
        log_info!(
            "ident_override: ea3_boot = AOB fallback @ {}+{:#x} (body validated)",
            ea3.name,
            hit.offset
        );
        Some(std::mem::transmute::<*const u8, Ea3BootFn>(hit.address))
    }
}

/// True iff `addr` lies inside `module` and its first `BOOT_MARKER_WINDOW`
/// bytes contain `LEA RCX,[rip+"ea3-boot"]` followed within
/// `BOOT_MARKER_PAIR_WINDOW` bytes by `LEA RDX,[rip+"startup"]` — the boot
/// function's opening `log_misc("ea3-boot", "startup")`. `ea3_shutdown`
/// and every other export lack the pair.
#[cfg(windows)]
unsafe fn validate_ea3_boot_body(addr: *const u8, module: &GameModule) -> bool {
    let start = addr as usize;
    let base = module.base as usize;
    let end = base.saturating_add(module.size);
    if start < base || start.saturating_add(BOOT_MARKER_WINDOW + 8) > end {
        return false;
    }
    if !memory::is_readable(addr, BOOT_MARKER_WINDOW + 8) {
        return false;
    }
    let body = std::slice::from_raw_parts(addr, BOOT_MARKER_WINDOW);
    for i in 0..body.len().saturating_sub(7) {
        // 48 8D 0D disp32 = LEA RCX,[rip+disp32]
        if body[i] != 0x48 || body[i + 1] != 0x8D || body[i + 2] != 0x0D {
            continue;
        }
        if !rip_target_is_cstr(addr.add(i + 3), module, b"ea3-boot\0") {
            continue;
        }
        let pair_end = (i + 7 + BOOT_MARKER_PAIR_WINDOW).min(body.len().saturating_sub(7));
        for j in (i + 7)..pair_end {
            // 48 8D 15 disp32 = LEA RDX,[rip+disp32]
            if body[j] == 0x48
                && body[j + 1] == 0x8D
                && body[j + 2] == 0x15
                && rip_target_is_cstr(addr.add(j + 3), module, b"startup\0")
            {
                return true;
            }
        }
    }
    false
}

/// Decode the RIP-relative disp32 at `disp` and compare the pointed bytes
/// (which must lie inside `module`) with `expected` (NUL included).
#[cfg(windows)]
unsafe fn rip_target_is_cstr(disp: *const u8, module: &GameModule, expected: &[u8]) -> bool {
    let target = scanner::decode_rip_relative(disp);
    let t = target as usize;
    let base = module.base as usize;
    if t < base || t.saturating_add(expected.len()) > base.saturating_add(module.size) {
        return false;
    }
    if !memory::is_readable(target, expected.len()) {
        return false;
    }
    std::slice::from_raw_parts(target, expected.len()) == expected
}

/// Outcome of one override attempt, for the boot log.
#[cfg(windows)]
enum Applied {
    /// `soft/rev` rewritten; carries the stock letter it replaced.
    Rewritten { old: String },
    /// The file already says [`OVERRIDE_REV`]; nothing to do.
    AlreadySet,
}

#[cfg(windows)]
unsafe extern "C" fn ea3_boot_hook(ea3: *mut u8) {
    let Some(hook) = (&*addr_of!(EA3_BOOT_HOOK)).as_ref() else {
        return; // unreachable: store precedes enable
    };
    BOOT_OBSERVED.store(true, Ordering::Release);
    // Everything before the original is best-effort and panic-contained:
    // the game must boot with its stock identity rather than not at all.
    match catch_unwind(AssertUnwindSafe(|| apply_override(ea3))) {
        Ok(Ok((Applied::Rewritten { old }, ident))) => {
            log_info!(
                "ident_override: soft/rev '{}' -> '{}' -- the game now identifies as {} (network model=, title screen, ea3-share)",
                old,
                OVERRIDE_REV as char,
                ident
            );
        }
        Ok(Ok((Applied::AlreadySet, ident))) => {
            log_info!(
                "ident_override: ea3-ident already carries rev '{}' -- nothing to rewrite ({})",
                OVERRIDE_REV as char,
                ident
            );
        }
        Ok(Err(reason)) => {
            log_warn!(
                "ident_override: could not rewrite soft/rev ({}) -- booting with the stock identity",
                reason
            );
        }
        Err(_) => {
            log_warn!(
                "ident_override: panic while rewriting soft/rev -- booting with the stock identity"
            );
        }
    }
    hook.call(ea3);
}

/// Read a `str` child of `node` into an owned String ("?" when unreadable).
#[cfg(windows)]
unsafe fn read_str_child(api: &PropertyApi, node: *mut u8, name: &[u8]) -> String {
    debug_assert!(name.ends_with(b"\0"));
    let mut buf = [0u8; 32];
    let status = (api.node_refer)(
        std::ptr::null_mut(),
        node,
        name.as_ptr() as *const i8,
        KBIN_TYPE_STR,
        buf.as_mut_ptr(),
        buf.len() as u32,
    );
    if status < 0 {
        return "?".to_string();
    }
    let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..len]).into_owned()
}

/// Rewrite `soft/rev` under the `/ea3` node to [`OVERRIDE_REV`]. Returns the
/// outcome plus the soft-id code the game will present. Order is
/// create-new → remove-old → verify (`property_search("rev")` must now yield
/// the new node) so a failure at any leg leaves exactly one, stock-valued
/// `rev` node behind.
#[cfg(windows)]
unsafe fn apply_override(ea3: *mut u8) -> Result<(Applied, String), String> {
    let Some(api) = *addr_of!(PROPERTY_API) else {
        return Err("property API not resolved".into());
    };
    if ea3.is_null() || !memory::is_readable(ea3, 8) {
        return Err("ea3 node pointer is null/unreadable".into());
    }
    let soft = (api.search)(std::ptr::null_mut(), ea3, b"soft\0".as_ptr() as *const i8);
    if soft.is_null() {
        return Err("no /ea3/soft node in the ea3 config".into());
    }
    let model = read_str_child(&api, soft, b"model\0");
    let dest = read_str_child(&api, soft, b"dest\0");
    let spec = read_str_child(&api, soft, b"spec\0");
    let ext = read_str_child(&api, soft, b"ext\0");
    let old_rev_node = (api.search)(std::ptr::null_mut(), soft, b"rev\0".as_ptr() as *const i8);
    if old_rev_node.is_null() {
        return Err("no /ea3/soft/rev node in the ea3 config".into());
    }
    let old_rev = read_str_child(&api, soft, b"rev\0");
    let new_rev = (OVERRIDE_REV as char).to_string();
    if old_rev == new_rev {
        return Ok((
            Applied::AlreadySet,
            compose_soft_id(&model, &dest, &spec, &new_rev, &ext),
        ));
    }
    if let Some(reason) = rev_sensitive_reason(&model, &dest, &spec, &old_rev) {
        log_warn!(
            "ident_override: stock ident {}:{}:{}:{} is rev-sensitive -- {}",
            model,
            dest,
            spec,
            old_rev,
            reason
        );
    }

    let property = (api.node_get_desc)(ea3);
    if property.is_null() {
        return Err("property_node_get_desc(/ea3) returned null".into());
    }
    let value = [OVERRIDE_REV, 0u8];
    let new_rev_node = (api.node_create_str)(
        property,
        soft,
        KBIN_TYPE_STR,
        b"rev\0".as_ptr() as *const i8,
        value.as_ptr() as *const i8,
    );
    if new_rev_node.is_null() {
        return Err("property_node_create(soft/rev) failed (stock node untouched)".into());
    }
    // The property now has two `rev` children; `property_search` (and the
    // ea3 lib's psmap import) return the FIRST — the stock one — until it is
    // removed. The remove's return value does not distinguish success from
    // "not removable" on this build, so verify by re-searching instead.
    let _ = (api.node_remove)(old_rev_node);
    let now = (api.search)(std::ptr::null_mut(), soft, b"rev\0".as_ptr() as *const i8);
    if now != new_rev_node {
        // Old node still first: undo our addition so exactly the stock
        // node remains (best effort — a duplicate would be harmless anyway,
        // psmap import reads the first match).
        let _ = (api.node_remove)(new_rev_node);
        return Err(format!(
            "property_node_remove(old soft/rev) did not take effect (stock rev '{}' kept)",
            old_rev
        ));
    }
    let readback = read_str_child(&api, soft, b"rev\0");
    if readback != new_rev {
        return Err(format!(
            "soft/rev read-back is '{}' after rewrite (expected '{}')",
            readback, new_rev
        ));
    }
    Ok((
        Applied::Rewritten { old: old_rev },
        compose_soft_id(&model, &dest, &spec, &new_rev, &ext),
    ))
}

/// One-shot WARN if the launcher never called `ea3_boot` through our detour
/// — the DLL was loaded after ea3 boot (unexpected under `-k`/`-z`), so the
/// game runs with its stock identity.
#[cfg(windows)]
fn spawn_boot_watchdog() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(BOOT_WATCHDOG_SECS));
        if !boot_observed() {
            log_warn!(
                "ident_override: ea3_boot was not observed within {} s of install -- the hook DLL may have loaded after ea3 boot; the game is running with its stock identity",
                BOOT_WATCHDOG_SECS
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_rev_is_a_valid_letter() {
        assert!(is_valid_rev(&(OVERRIDE_REV as char).to_string()));
        assert!(is_valid_rev("A"));
        assert!(!is_valid_rev("a"));
        assert!(!is_valid_rev(""));
        assert!(!is_valid_rev("XX"));
        assert!(!is_valid_rev("1"));
    }

    #[test]
    fn composes_the_stock_shape() {
        let rev = (OVERRIDE_REV as char).to_string();
        assert_eq!(
            compose_soft_id("MDX", "J", "F", &rev, "2026082500"),
            "MDX:J:F:M:2026082500"
        );
    }

    #[test]
    fn override_rev_is_not_the_omnimix_letter() {
        // bemaniutils keys omnimix on rev `X`; the modpack must not collide.
        assert_ne!(OVERRIDE_REV, b'X');
    }

    #[test]
    fn stock_japan_hd_ident_is_not_rev_sensitive() {
        assert!(rev_sensitive_reason("MDX", "J", "F", "A").is_none());
        assert!(rev_sensitive_reason("MDX", "J", "F", "B").is_none());
        assert!(rev_sensitive_reason("MDX", "A", "F", "A").is_none());
        assert!(rev_sensitive_reason("MDX", "J", "I", "D").is_none());
    }

    #[test]
    fn known_rev_sensitive_idents_are_flagged() {
        assert!(rev_sensitive_reason("MDX", "A", "F", "B").is_some());
        assert!(rev_sensitive_reason("MDX", "Y", "F", "B").is_some());
        assert!(rev_sensitive_reason("MDX", "E", "K", "B").is_some());
        for rev in ["A", "B", "C"] {
            assert!(rev_sensitive_reason("MDX", "J", "I", rev).is_some());
        }
        assert!(rev_sensitive_reason("TDX", "U", "J", "C").is_some());
    }
}
