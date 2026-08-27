//! Texture Resolver — Resolves texture names to IDs and UV coordinates
//! using the game's get_bitmap_info callback.
//!
//! The callback is stored in libafp-win64.dll's data section. We find candidate
//! global pointers by tracing afp_mc_load_bitmap's call chain for indirect calls,
//! then at resolve time validate each candidate (non-null, within libafp) to find
//! the correct one.
//!
//! IMPORTANT: resolve() must be called from the game thread (via run_on_render_thread).
//! The AFP callback accesses thread-local game state and crashes on background threads.
//!
//! Textures are identified by bare asset name (no path/extension).
//! Only resolvable after their IFS/ARC has been loaded by the game.

use once_cell::sync::Lazy;
use std::ffi::CString;
use std::sync::Mutex;

use crate::core::scanner::{decode_call_rel32, decode_rip_relative};
use crate::{log_info, log_warn};

type GetBitmapInfoFn = unsafe extern "C" fn(*mut u8, *const u8) -> i32;
type GetTextureBindIdFn = unsafe extern "C" fn(*const u8, *const u8) -> i32;

#[derive(Clone, Debug)]
pub struct TextureInfo {
    pub texture_id: i32,
    pub atlas_width: u16,
    pub atlas_height: u16,
    pub uv_left: f32,
    pub uv_top: f32,
    pub uv_right: f32,
    pub uv_bottom: f32,
}

struct TextureResolverInner {
    /// All candidate global pointer addresses found in libafp's call chain.
    candidates: Vec<*const *const u8>,
    /// Once validated, the resolved callback function pointer.
    resolved_cb: *const u8,
    afp_base: usize,
    afp_end: usize,
    get_texture_bind_id: *const u8,
    initialized: bool,
}

unsafe impl Send for TextureResolverInner {}

static TEXTURE_RESOLVER: Lazy<Mutex<TextureResolverInner>> = Lazy::new(|| {
    Mutex::new(TextureResolverInner {
        candidates: Vec::new(),
        resolved_cb: std::ptr::null(),
        afp_base: 0,
        afp_end: 0,
        get_texture_bind_id: std::ptr::null(),
        initialized: false,
    })
});

pub fn init() -> bool {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
    use windows::Win32::System::Threading::GetCurrentProcess;

    unsafe {
        let afp_dll = CString::new("libafp-win64.dll").unwrap();
        let afp_handle = match GetModuleHandleA(PCSTR(afp_dll.as_ptr() as *const u8)) {
            Ok(h) if !h.is_invalid() => h,
            _ => {
                log_warn!("TextureResolver: libafp-win64.dll not loaded");
                return false;
            }
        };
        let mut info = MODULEINFO::default();
        if GetModuleInformation(
            GetCurrentProcess(),
            afp_handle,
            &mut info,
            std::mem::size_of::<MODULEINFO>() as u32,
        )
        .is_err()
        {
            log_warn!("TextureResolver: failed to get libafp module info");
            return false;
        }
        let afp_base = info.lpBaseOfDll as usize;
        let afp_size = info.SizeOfImage as usize;
        let afp_end = afp_base + afp_size;

        let export_name = CString::new("afp_mc_load_bitmap").unwrap();
        let load_bitmap = match GetProcAddress(afp_handle, PCSTR(export_name.as_ptr() as *const u8))
        {
            Some(f) => f as *const u8,
            None => {
                log_warn!("TextureResolver: afp_mc_load_bitmap not found");
                return false;
            }
        };

        // Collect ALL candidate indirect call targets from the call chain
        let mut candidates: Vec<*const *const u8> = Vec::new();
        let callees = find_all_calls(load_bitmap, 256);
        for callee in &callees {
            let c = *callee as usize;
            if c < afp_base || c >= afp_end {
                continue;
            }
            let targets = find_all_indirect_call_targets(*callee, 512);
            for target in &targets {
                let t = *target as usize;
                if t >= afp_base && t + 8 <= afp_end {
                    candidates.push(*target as *const *const u8);
                }
            }
        }
        log_info!(
            "TextureResolver: found {} candidate globals in libafp call chain",
            candidates.len()
        );

        // Get afpu_get_texture_bind_id from libafputils
        let utils_dll = CString::new("libafputils-win64.dll").unwrap();
        let bind_id_fn = GetModuleHandleA(PCSTR(utils_dll.as_ptr() as *const u8))
            .ok()
            .and_then(|h| {
                let name = CString::new("afpu_get_texture_bind_id").unwrap();
                GetProcAddress(h, PCSTR(name.as_ptr() as *const u8)).map(|f| f as *const u8)
            })
            .unwrap_or(std::ptr::null());

        let mut resolver = TEXTURE_RESOLVER.lock().unwrap();
        resolver.candidates = candidates;
        resolver.afp_base = afp_base;
        resolver.afp_end = afp_end;
        resolver.get_texture_bind_id = bind_id_fn;
        resolver.initialized = true;

        log_info!("TextureResolver: initialized (deferred validation, game-thread only)");
        true
    }
}

/// Resolve a texture name to its atlas ID and UV coordinates.
/// MUST be called from the game thread (via run_on_render_thread).
pub fn resolve(name: &str) -> Option<TextureInfo> {
    let mut resolver = TEXTURE_RESOLVER.lock().unwrap();
    if !resolver.initialized {
        return None;
    }

    let afp_base = resolver.afp_base;
    let afp_end = resolver.afp_end;

    // Validate callback on first call (first valid match wins)
    if resolver.resolved_cb.is_null() {
        for i in 0..resolver.candidates.len() {
            let candidate = resolver.candidates[i];
            let cb = unsafe { *candidate };
            if cb.is_null() {
                continue;
            }
            let cb_addr = cb as usize;
            if cb_addr >= afp_base && cb_addr < afp_end {
                log_info!(
                    "TextureResolver: validated callback @ {:p} (global {:p})",
                    cb,
                    candidate
                );
                resolver.resolved_cb = cb;
                break;
            }
        }
        if resolver.resolved_cb.is_null() {
            return None;
        }
    }

    let cb_fn = resolver.resolved_cb;
    let bind_id_fn = resolver.get_texture_bind_id;
    drop(resolver);

    unsafe {
        let get_info: GetBitmapInfoFn = std::mem::transmute(cb_fn);
        let c_name = CString::new(name).ok()?;
        let mut buf = [0u8; 16];

        let ok = get_info(buf.as_mut_ptr(), c_name.as_ptr() as *const u8);
        if ok == 0 {
            return None;
        }

        let bm2d_texture_ptr = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let texture_id = if !bind_id_fn.is_null() && bm2d_texture_ptr != 0 {
            let convert: GetTextureBindIdFn = std::mem::transmute(bind_id_fn);
            convert(bm2d_texture_ptr as *const u8, std::ptr::null())
        } else {
            bm2d_texture_ptr as i32
        };
        let atlas_width = u16::from_le_bytes([buf[4], buf[5]]);
        let atlas_height = u16::from_le_bytes([buf[6], buf[7]]);
        let px_l = (u16::from_le_bytes([buf[8], buf[9]]) / 2) as f32;
        let px_r = (u16::from_le_bytes([buf[10], buf[11]]) / 2) as f32;
        let px_t = (u16::from_le_bytes([buf[12], buf[13]]) / 2) as f32;
        let px_b = (u16::from_le_bytes([buf[14], buf[15]]) / 2) as f32;

        let aw = atlas_width as f32;
        let ah = atlas_height as f32;
        if aw == 0.0 || ah == 0.0 {
            return None;
        }

        log_info!(
            "TextureResolver: resolved '{}' -> id={}, atlas={}x{}",
            name,
            texture_id,
            atlas_width,
            atlas_height
        );

        Some(TextureInfo {
            texture_id,
            atlas_width,
            atlas_height,
            uv_left: px_l / aw,
            uv_top: px_t / ah,
            uv_right: px_r / aw,
            uv_bottom: px_b / ah,
        })
    }
}

pub fn is_available() -> bool {
    TEXTURE_RESOLVER.lock().unwrap().initialized
}

/// Return the validated get_bitmap_info callback address, or None if not yet resolved.
/// The callback is validated lazily on first resolve() call from the game thread.
/// Callers can force validation by calling resolve() with any name first.
pub fn get_bitmap_info_callback() -> Option<*const u8> {
    let resolver = TEXTURE_RESOLVER.lock().unwrap();
    if !resolver.initialized {
        return None;
    }
    if !resolver.resolved_cb.is_null() {
        return Some(resolver.resolved_cb);
    }
    // Try to validate now
    drop(resolver);
    // Force validation by attempting a dummy resolve
    let _ = resolve("__dummy_validation__");
    let resolver = TEXTURE_RESOLVER.lock().unwrap();
    if !resolver.resolved_cb.is_null() {
        Some(resolver.resolved_cb)
    } else {
        None
    }
}

// ── Instruction scanning helpers ────────────────────────────────

unsafe fn find_all_calls(addr: *const u8, max_bytes: usize) -> Vec<*const u8> {
    let mut targets = Vec::new();
    for i in 0..max_bytes {
        if *addr.add(i) == 0xE8 {
            let target = decode_call_rel32(addr.add(i));
            let diff = (target as isize).wrapping_sub(addr as isize).unsigned_abs();
            if diff < 0x1000000 {
                targets.push(target);
            }
        }
    }
    targets
}

unsafe fn find_all_indirect_call_targets(addr: *const u8, max_bytes: usize) -> Vec<*const u8> {
    let mut targets = Vec::new();
    for i in 0..max_bytes.saturating_sub(6) {
        let b0 = *addr.add(i);
        let b1 = *addr.add(i + 1);

        if b0 == 0xFF && b1 == 0x15 {
            // CALL qword ptr [RIP+disp32] — 6-byte insn, disp at +2.
            targets.push(decode_rip_relative(addr.add(i + 2)));
            continue;
        }

        if b0 == 0x48 && b1 == 0x8B {
            let b2 = *addr.add(i + 2);
            if matches!(b2, 0x05 | 0x0D | 0x15 | 0x35 | 0x3D) {
                let data_addr = decode_rip_relative(addr.add(i + 3));
                for j in (i + 7)..std::cmp::min(i + 39, max_bytes.saturating_sub(1)) {
                    if *addr.add(j) == 0xFF
                        && (*addr.add(j + 1) >= 0xD0 && *addr.add(j + 1) <= 0xD7)
                    {
                        targets.push(data_addr);
                        break;
                    }
                }
            }
        }
    }
    targets
}
