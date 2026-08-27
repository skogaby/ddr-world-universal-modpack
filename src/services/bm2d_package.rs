//! BM2D package service — on-demand load / lookup / release of BM2D data
//! packages through the game's own `bm2d::data::Manager` (gamemdx).
//!
//! The game registers BM2D packages (IFS-inside-ARC, e.g.
//! `data/arc/custom/background/background_0001.arc`) in a name-keyed registry
//! and creates them asynchronously: `request_load(dir, name)` queues the arc,
//! the engine's main loop pumps `Manager::Update` every frame, and
//! `is_ready(name)` flips true once the entry exists AND its package was
//! created. `release(name)` destroys the package + erases the entry
//! synchronously. Arc name variants (`%s_v3`, `%s_lite`, ...) are resolved by
//! the game's own loader — always request the base name. Cabinet-validated
//! 2026-07-09; see `.agents/planning/20260708-background-preview-overlay/
//! progress.md` ("STEP-1 VALIDATED FACTS" + Ghidra findings).
//!
//! On top of the raw wrappers this service adds two safety layers the game
//! does NOT provide:
//!
//! - **Refcounting.** The game's `request_load` dedups by name and its
//!   `release` destroys immediately — so two independent holders (e.g. both
//!   player sides previewing the same background) would tread on each other.
//!   [`request_load`] hands out a [`LoadTicket`] per holder; the game-side
//!   release is only issued when the LAST ticket for a name is released.
//! - **Borrowed-entry guard.** If an entry for the name already exists at the
//!   0→1 request (e.g. the game itself has the applied background resident as
//!   the song-select backdrop), the in-game request would dedup to a no-op —
//!   we never own such an entry, and [`release`] must never destroy it under
//!   the game. Those residencies are marked *borrowed* and their game-side
//!   release is skipped.
//!
//! Callers must destroy any AFP layer created from a package BEFORE releasing
//! that package's ticket. All calls are render/game-thread only. Addresses
//! come from AOB signatures (`bm2d_data_request_load` / `bm2d_data_is_ready`
//! / `bm2d_data_release`) and derivation (`bm2d_package_registry` /
//! `bm2d_package_lookup`) in `core/signatures.rs`.

use once_cell::sync::{Lazy, OnceCell};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::core::signatures::SignatureStore;
use crate::{log_debug, log_info, log_warn};

/// One-shot latch for the early-boot "registry not created yet" warn.
static REGISTRY_MISSING_LOGGED: AtomicBool = AtomicBool::new(false);

// ── Game function types (Ghidra-confirmed, both 2026 builds) ─────────

/// `bool bm2d_data_request_load(const char* dir, const char* name, u32 flag)`
type RequestLoadFn = unsafe extern "C" fn(*const i8, *const i8, u32) -> u8;
/// `bool bm2d_data_is_ready(const char* name)`
type IsReadyFn = unsafe extern "C" fn(*const i8) -> u8;
/// `void bm2d_data_release(const char* name)`
type ReleaseFn = unsafe extern "C" fn(*const i8);
/// `Entry* lookup(Entry* begin, Entry* end, const char* name)` — returns `end`
/// when not found. Entry stride 0x40; package ptr at entry+0x30.
type LookupFn = unsafe extern "C" fn(*const u8, *const u8, *const i8) -> *const u8;

/// Byte offset (within the matched `bm2d_data_is_ready` pattern) of the
/// disp8 in its final instruction `MOV RCX,[RAX+disp8]` — the package
/// pointer's offset within a registry entry (`entry[6]`, 0x30 on both 2026
/// builds). Read from the matched bytes at init so the service can never
/// desync from the scanned reality (the disp8 is wildcarded in the pattern).
const IS_READY_ENTRY_PKG_DISP8_OFFSET: usize = 39;
/// Byte offset of the afpu package id (u32) within the package object.
/// Not embedded in any scanned pattern — Ghidra-verified on both 2026
/// builds (create path `FUN_18003e060` / `FUN_18003e760`; see
/// `.agents/planning/20260708-background-preview-overlay/progress.md`).
/// Failure mode if a future build shifts it is graceful: the garbage id
/// makes `afpu_get_afp_info_at_package` return non-zero → layer create
/// fails with a warn → previews degrade to chrome-only.
const PACKAGE_AFPU_ID_OFFSET: usize = 0x314;

struct Api {
    request_load: RequestLoadFn,
    is_ready: IsReadyFn,
    release: ReleaseFn,
    lookup: LookupFn,
    /// Address of the global *pointer* to the registry object ([0]=begin,
    /// [8]=end). The object is created lazily during boot — deref at use time.
    registry_global: *const u8,
    /// Package-pointer offset within a registry entry, derived from the
    /// matched `bm2d_data_is_ready` bytes (see
    /// [`IS_READY_ENTRY_PKG_DISP8_OFFSET`]).
    entry_package_offset: usize,
}

// Raw pointers are fixed game addresses, valid for the process lifetime and
// only used from the render thread.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

/// Write-once at init, read-only after — no lock (and no `unwrap` on a
/// mutex from render-thread callbacks).
static API: OnceCell<Api> = OnceCell::new();

/// Per-name residency bookkeeping backing the refcount + borrowed guard.
struct Residency {
    /// Live [`LoadTicket`]s for this name.
    count: u32,
    /// True when the registry entry pre-existed our 0→1 request (owned by
    /// the game or another system) — game-side release must be skipped.
    borrowed: bool,
}

static RESIDENCY: Lazy<Mutex<HashMap<String, Residency>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static RESIDENCY_POISON_LOGGED: AtomicBool = AtomicBool::new(false);

fn lock_residency() -> std::sync::MutexGuard<'static, HashMap<String, Residency>> {
    match RESIDENCY.lock() {
        Ok(g) => g,
        Err(poisoned) => {
            if !RESIDENCY_POISON_LOGGED.swap(true, Ordering::AcqRel) {
                log_warn!("BM2D_PKG: residency mutex poisoned — recovered (one-shot notice)");
            }
            poisoned.into_inner()
        }
    }
}

/// One holder's claim on a loaded package. Not `Copy`/`Clone` — [`release`]
/// consumes it, making double-release a compile error. Dropping one without
/// releasing (realistically: a panic unwind) warns and heals the bookkeeping
/// WITHOUT issuing the game-side release — see [`LoadTicket::drop`].
pub struct LoadTicket {
    name: CString,
}

impl LoadTicket {
    /// The asset name this ticket keeps resident (for logging).
    pub fn name(&self) -> &str {
        self.name.to_str().unwrap_or("")
    }

    /// Move the name out without firing the Drop path (release path).
    fn into_name(self) -> CString {
        // SAFETY: reads the field then forgets `self`, so Drop never runs
        // and the CString is owned exactly once.
        let name = unsafe { std::ptr::read(&self.name) };
        std::mem::forget(self);
        name
    }
}

impl Drop for LoadTicket {
    fn drop(&mut self) {
        log_warn!(
            "BM2D_PKG: LoadTicket {:?} dropped without release — releasing bookkeeping (game-side release skipped)",
            self.name()
        );
        // Heal the refcount so this name's residency doesn't wedge above zero
        // forever (which would silently disable game-side release for every
        // future holder). The game-side release is deliberately NOT issued
        // from Drop: a drop means unwind/bug, and a layer might still be
        // bound to this package — leaking one engine entry is safer than
        // destroying a stream under it. The orphaned registry entry then
        // pre-exists the next 0→1 request and is re-adopted as `borrowed`.
        let name = self.name().to_string();
        let mut residency = lock_residency();
        if let Some(entry) = residency.get_mut(&name) {
            entry.count = entry.count.saturating_sub(1);
            if entry.count == 0 {
                residency.remove(&name);
            }
        }
    }
}

/// A registered package (registry `entry[6]`). A borrowed view tied to the
/// [`LoadTicket`] it was looked up through — the lifetime parameter makes a
/// use-after-release a compile error (the package pointer dangles once the
/// last ticket for its name is released and the game's deferred destroy
/// runs). It does not own anything.
///
/// **Do not create AFP layers from a package whose residency is borrowed**
/// (the entry pre-existed our request — see [`request_load`]): the game can
/// release such an entry at any time, destroying the stream under any layer
/// bound to it (the 2026-07-09 crash class). Consumers that bind layers must
/// load privately-named packages they wholly own.
#[derive(Clone, Copy)]
pub struct PackageHandle<'t> {
    ptr: *const u8,
    _ticket: std::marker::PhantomData<&'t LoadTicket>,
}

impl PackageHandle<'_> {
    /// The afpu package id (u32 at package+0x314) —
    /// `bm2d_api::create_layer_from_package` takes this.
    pub fn afpu_package_id(&self) -> u32 {
        unsafe { *(self.ptr.add(PACKAGE_AFPU_ID_OFFSET) as *const u32) }
    }
}

/// Resolve the five gamemdx addresses. Call once from `lib.rs` (after
/// `bm2d_api::init`). Non-fatal: a miss leaves `is_available()` false and
/// consumers degrade (background previews stay chrome-only).
pub fn init(signatures: &SignatureStore) -> bool {
    if API.get().is_some() {
        return true;
    }

    macro_rules! sig {
        ($name:expr) => {
            match signatures.get_address($name) {
                Some(a) => a,
                None => {
                    log_warn!(
                        "BM2D_PKG: signature {} unresolved — service unavailable",
                        $name
                    );
                    return false;
                }
            }
        };
    }
    let request_load = sig!("bm2d_data_request_load");
    let is_ready = sig!("bm2d_data_is_ready");
    let release = sig!("bm2d_data_release");
    let lookup = sig!("bm2d_package_lookup");
    let registry_global = sig!("bm2d_package_registry");

    // Read the entry-layout disp8 from the matched is_ready bytes. Assert the
    // surrounding opcode really is `MOV RCX,[RAX+disp8]` (48 8B 48) so a
    // future edit to the pattern string that shifts positions can't silently
    // desync this hand-counted offset; bound the disp8 to the entry stride
    // (0x40) — disp8 is signed in the encoding, so this also rejects the
    // 0x80+ range that would misread as a large positive offset.
    let opcode_ok = unsafe {
        *is_ready.add(IS_READY_ENTRY_PKG_DISP8_OFFSET - 3) == 0x48
            && *is_ready.add(IS_READY_ENTRY_PKG_DISP8_OFFSET - 2) == 0x8B
            && *is_ready.add(IS_READY_ENTRY_PKG_DISP8_OFFSET - 1) == 0x48
    };
    let entry_package_offset = unsafe { *is_ready.add(IS_READY_ENTRY_PKG_DISP8_OFFSET) } as usize;
    if !opcode_ok
        || entry_package_offset == 0
        || entry_package_offset % 8 != 0
        || entry_package_offset >= 0x40
    {
        log_warn!(
            "BM2D_PKG: derived entry package offset 0x{:X} looks wrong (opcode_ok={}) — service unavailable",
            entry_package_offset,
            opcode_ok
        );
        return false;
    }

    let api = Api {
        request_load: unsafe { std::mem::transmute::<*const u8, RequestLoadFn>(request_load) },
        is_ready: unsafe { std::mem::transmute::<*const u8, IsReadyFn>(is_ready) },
        release: unsafe { std::mem::transmute::<*const u8, ReleaseFn>(release) },
        lookup: unsafe { std::mem::transmute::<*const u8, LookupFn>(lookup) },
        registry_global,
        entry_package_offset,
    };
    let ok = API.set(api).is_ok();
    if ok {
        log_info!(
            "BM2D_PKG: initialized (request_load/is_ready/release/lookup + registry, entry pkg offset 0x{:X})",
            entry_package_offset
        );
    }
    ok
}

pub fn is_available() -> bool {
    API.get().is_some()
}

/// Deref the registry global → registry object, or None if the game hasn't
/// created it yet (early boot). The game's own fns deref it unguarded, so
/// every wrapper below checks this first.
fn registry_obj(api: &Api) -> Option<*const u8> {
    let obj = unsafe { *(api.registry_global as *const *const u8) };
    if obj.is_null() {
        None
    } else {
        Some(obj)
    }
}

/// Raw registry scan: does an entry for `name` exist (package ready or not)?
/// Mirrors the game's `request_load` dedup criterion.
fn entry_exists(api: &Api, name: &CStr) -> bool {
    let Some(obj) = registry_obj(api) else {
        return false;
    };
    unsafe {
        let begin = *(obj as *const *const u8);
        let end = *(obj.add(8) as *const *const u8);
        let entry = (api.lookup)(begin, end, name.as_ptr());
        entry != end && !entry.is_null()
    }
}

/// Non-blocking, idempotent: ensure the package for `dir`/`name` (e.g.
/// `"custom/background"`, `"background_0001"`) is loaded & registered, and
/// claim a residency ticket for it. Poll [`is_ready`] per render tick until
/// true (the engine's main loop creates the package; ~0.3-0.5 s for a
/// background), then [`lookup`] the handle. Returns None if the service is
/// unavailable, the registry doesn't exist yet, or the game rejects the load.
///
/// If the entry pre-exists our 0→1 request, the residency is **borrowed**
/// (see [`PackageHandle`]'s warning — never bind AFP layers to a borrowed
/// package; the game can destroy it at any time). Render/game thread only —
/// the residency lock is held across the in-game call below, which is safe
/// only because all callers (and the engine's own registry mutation) share
/// that single thread.
pub fn request_load(dir: &str, name: &str) -> Option<LoadTicket> {
    let api = API.get()?;
    let c_dir = CString::new(dir).ok()?;
    let c_name = CString::new(name).ok()?;
    if registry_obj(api).is_none() {
        // One-shot: a caller syncing a whole prefetch window during early
        // boot would otherwise burst this warn per entry.
        if !REGISTRY_MISSING_LOGGED.swap(true, Ordering::AcqRel) {
            log_warn!(
                "BM2D_PKG: request_load({:?}) — registry not created yet (one-shot notice)",
                name
            );
        }
        return None;
    }

    let mut residency = lock_residency();
    let entry = residency.entry(name.to_string()).or_insert(Residency {
        count: 0,
        borrowed: false,
    });
    entry.count += 1;
    if entry.count == 1 {
        // 0→1: decide ownership. If an entry for this name already exists
        // (the game's own residency, e.g. the applied background), the
        // in-game request would dedup to a no-op — mark borrowed so release
        // never destroys what we didn't load.
        entry.borrowed = entry_exists(api, &c_name);
        if entry.borrowed {
            log_debug!(
                "BM2D_PKG: request_load({:?}) — entry pre-exists (borrowed; game-side release will be skipped)",
                name
            );
        } else {
            let ret = unsafe { (api.request_load)(c_dir.as_ptr(), c_name.as_ptr(), 0) };
            log_debug!("BM2D_PKG: request_load({:?}, {:?}, 0) = {}", dir, name, ret);
            if ret == 0 {
                residency.remove(name);
                log_warn!("BM2D_PKG: request_load({:?}) rejected by game", name);
                return None;
            }
        }
    } else {
        log_debug!(
            "BM2D_PKG: request_load({:?}) — already resident (refcount now {})",
            name,
            entry.count
        );
    }
    Some(LoadTicket { name: c_name })
}

/// True once the ticket's entry exists AND its package was created (the
/// game's own readiness predicate).
pub fn is_ready(ticket: &LoadTicket) -> bool {
    is_ready_c(&ticket.name)
}

/// Readiness by raw name (diagnostics — e.g. checking post-release state or
/// pre-request residency).
pub fn is_name_ready(name: &str) -> bool {
    match CString::new(name) {
        Ok(c) => is_ready_c(&c),
        Err(_) => false,
    }
}

fn is_ready_c(name: &CStr) -> bool {
    let Some(api) = API.get() else {
        return false;
    };
    if registry_obj(api).is_none() {
        return false;
    }
    unsafe { (api.is_ready)(name.as_ptr()) != 0 }
}

/// Registry lookup for the ticket's package. Some once the package was
/// created (i.e. after [`is_ready`] flips true). The handle borrows the
/// ticket — it cannot outlive the residency that keeps the package alive.
pub fn lookup<'t>(ticket: &'t LoadTicket) -> Option<PackageHandle<'t>> {
    let api = API.get()?;
    let obj = registry_obj(api)?;
    unsafe {
        let begin = *(obj as *const *const u8);
        let end = *(obj.add(8) as *const *const u8);
        let entry = (api.lookup)(begin, end, ticket.name.as_ptr());
        if entry == end || entry.is_null() {
            return None;
        }
        let pkg = *(entry.add(api.entry_package_offset) as *const *const u8);
        if pkg.is_null() {
            return None;
        }
        Some(PackageHandle {
            ptr: pkg,
            _ticket: std::marker::PhantomData,
        })
    }
}

/// Drop one holder's claim. When the last ticket for the name goes, the
/// game-side release is issued (destroys the package + erases the entry
/// synchronously; the actual afpu destroy is deferred to the engine's
/// Update) — unless the residency was borrowed. Destroy any AFP layer
/// created from this package BEFORE calling this.
pub fn release(ticket: LoadTicket) {
    let c_name = ticket.into_name();
    let name = c_name.to_str().unwrap_or("").to_string();

    // Bookkeeping first, unconditionally — the refcount must stay balanced
    // even if the API were somehow unavailable (a consumed ticket that
    // doesn't decrement would wedge this name's count above zero forever).
    let issue_game_release = {
        let mut residency = lock_residency();
        let Some(entry) = residency.get_mut(&name) else {
            log_warn!(
                "BM2D_PKG: release({:?}) — no residency entry (double release?)",
                name
            );
            return;
        };
        entry.count = entry.count.saturating_sub(1);
        if entry.count > 0 {
            log_debug!(
                "BM2D_PKG: release({:?}) — refcount now {}",
                name,
                entry.count
            );
            return;
        }
        let borrowed = entry.borrowed;
        residency.remove(&name);
        !borrowed
    };

    if !issue_game_release {
        log_debug!(
            "BM2D_PKG: release({:?}) — borrowed residency, game-side release skipped",
            name
        );
        return;
    }
    match API.get() {
        Some(api) => {
            unsafe { (api.release)(c_name.as_ptr()) };
            log_debug!("BM2D_PKG: release({:?}) — game-side release issued", name);
        }
        None => {
            // Unreachable in practice (tickets only exist once init succeeded
            // and the OnceCell is never cleared) — but never fail silently.
            log_warn!(
                "BM2D_PKG: release({:?}) — API unavailable, game-side release skipped",
                name
            );
        }
    }
}
