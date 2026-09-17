//! Engine texture API (design §4.2.3) + the gs texture-registry lookup the
//! render-item builder uses to re-resolve material textures (RE §2.1/§2.5).
//!
//! All three engine entry points are reached through
//! [`Scene3dSites`](crate::core::signatures::Scene3dSites): `texture_create`
//! / `texture_release` (the ArrowPalette factory's create + the World twin of
//! the A3 item dtor's release, RE §1.4) and the OPTIONAL
//! `texture_lookup` trio (`lookup`, `default_texture`, `spin` — the model
//! converter's texture-table fill, RE §2.5). Bone textures are
//! `create_dynamic(4, bone_count, 0x74, 0x2001)` — WIDTH 4, one bone per
//! row (RE §2.8).
//!
//! Threading: `create_dynamic` and `lookup_gs_texture` take engine locks
//! (the registry's spin flags) and are GAME-THREAD ONLY. `release` takes only
//! the texture registry's own spin lock and is safe from the node dtor (a
//! job-graph worker) — that is the ONE engine call the dtor makes.

use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::System::Threading::SwitchToThread;

use crate::core::memory;

use super::sites;

/// Process-lifetime counters — `created == released` after every teardown is
/// the "no texture-registry growth" proof the Step 4 log line carries.
static CREATED: AtomicU32 = AtomicU32::new(0);
static RELEASED: AtomicU32 = AtomicU32::new(0);
/// Releases the engine answered `-1` to (stale handle — a double release or
/// a handle it never issued).
static RELEASE_STALE: AtomicU32 = AtomicU32::new(0);

/// Snapshot of the create/release counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextureBalance {
    pub created: u32,
    pub released: u32,
    pub stale: u32,
}

impl std::fmt::Display for TextureBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "created={} released={} stale={}",
            self.created, self.released, self.stale
        )
    }
}

/// The counters right now.
pub fn balance() -> TextureBalance {
    TextureBalance {
        created: CREATED.load(Ordering::Relaxed),
        released: RELEASED.load(Ordering::Relaxed),
        stale: RELEASE_STALE.load(Ordering::Relaxed),
    }
}

/// `u32 create(w, h, mips, fmt, usage)` — 0 on failure.
type TextureCreateFn = unsafe extern "C" fn(u32, u32, u32, u32, u32) -> u32;
/// `i32 release(u32 handle)` — refcount after, or -1 when the handle is stale.
type TextureReleaseFn = unsafe extern "C" fn(u32) -> i32;
/// `gs::TextureData* lookup(u32 gs_hash)` — null on miss.
type TextureLookupFn = unsafe extern "C" fn(u32) -> *const u8;

/// Create a 1-mip dynamic texture through the engine API. `None` on failure
/// or when the service is unavailable. GAME THREAD ONLY.
pub fn create_dynamic(width: u32, height: u32, format: u32, usage: u32) -> Option<u32> {
    let s = sites()?;
    if width == 0 || height == 0 {
        return None;
    }
    // SAFETY: `texture_create` was AOB-resolved to the engine's
    // `u32 create(w, h, mips, fmt, usage)` (RE §1.4).
    let create: TextureCreateFn = unsafe { std::mem::transmute(s.texture_create) };
    let handle = unsafe { create(width, height, 1, format, usage) };
    if handle == 0 {
        None
    } else {
        CREATED.fetch_add(1, Ordering::Relaxed);
        Some(handle)
    }
}

/// Release a handle obtained from [`create_dynamic`]. The engine's return
/// value (new refcount, or -1 for a stale handle) only feeds the
/// [`balance`] counters — nothing useful can be done about it from the dtor.
/// `0` is a no-op.
pub fn release(handle: u32) {
    if handle == 0 {
        return;
    }
    let Some(s) = sites() else { return };
    // SAFETY: `texture_release` was AOB-resolved to `i32 release(u32)`.
    let rel: TextureReleaseFn = unsafe { std::mem::transmute(s.texture_release) };
    let r = unsafe { rel(handle) };
    RELEASED.fetch_add(1, Ordering::Relaxed);
    if r < 0 {
        RELEASE_STALE.fetch_add(1, Ordering::Relaxed);
    }
}

/// Whether the optional lookup trio resolved on this build.
pub fn lookup_available() -> bool {
    sites().and_then(|s| s.texture_lookup).is_some()
}

/// The engine's default `TextureData*` (what the converter substitutes for a
/// texture it could not find). `None` when the trio is absent or the global
/// is unreadable/null.
pub fn default_texture() -> Option<*const u8> {
    let t = sites()?.texture_lookup?;
    if !memory::is_readable(t.default_texture, 8) {
        return None;
    }
    let p = unsafe { memory::read_ptr(t.default_texture) };
    if p.is_null() {
        None
    } else {
        Some(p)
    }
}

/// Look a gs texture up by its gs hash under the registry's spin flag,
/// replicating the converter's exact protocol (`while fetch_add(1) != 0 {
/// SwitchToThread }` … `store(0)`). `None` when the trio is absent or the
/// registry has no such texture. GAME THREAD ONLY (the lookup lazily
/// re-sorts the registry vector on its first call after a change).
pub fn lookup_gs_texture(hash: u32) -> Option<*const u8> {
    let t = sites()?.texture_lookup?;
    if !memory::is_readable(t.spin, 4) {
        return None;
    }
    // SAFETY: `spin` is the engine's u32 flag (probed above); the atomic view
    // is exactly how the engine's own LOCK XADD / XCHG treat it. `lookup`
    // was identity-gated on its prologue by the derivation.
    let found = unsafe {
        let spin = &*(t.spin as *const AtomicU32);
        let lookup: TextureLookupFn = std::mem::transmute(t.lookup);
        // Bounded so a wedged flag can never hang the game thread: the
        // engine holds this flag for microseconds (a binary search).
        let mut acquired = false;
        for _ in 0..100_000 {
            if spin.fetch_add(1, Ordering::AcqRel) == 0 {
                acquired = true;
                break;
            }
            let _ = SwitchToThread();
        }
        if !acquired {
            return None;
        }
        let p = lookup(hash);
        spin.store(0, Ordering::Release);
        p
    };
    if found.is_null() {
        None
    } else {
        Some(found)
    }
}
