//! AFP Patcher — Intercepts AFP stream creation to apply binary patches in-flight.
//!
//! Hooks `afp_stream_do_create` (resolved by named export from libafp-win64.dll) to modify AFP template data
//! before the game's AFP runtime processes it. Mods register patch callbacks for
//! specific template names; when a matching template is loaded, the callback
//! receives the raw AFP data and returns a modified version.
//!
//! Key insight: libafputils descrambles AFP data (BSI byte-swaps AND string table
//! cipher) BEFORE calling afp_stream_do_create. The data arriving in our hook is
//! fully descrambled plaintext — we can read the template name directly from the
//! string table without any BSI or cipher operations.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Mutex;

use crate::{log_info, log_warn};

/// Callback type for AFP patches. Receives (afp_data, bsi_data).
/// Returns Some((new_afp, new_bsi)) to replace, or None to pass through unchanged.
pub type AfpPatchFn = Box<dyn Fn(&[u8], &[u8]) -> Option<(Vec<u8>, Vec<u8>)> + Send + Sync>;

/// Raw data interceptor — called for ALL data passing through afp_stream_do_create
/// (AFP, GEO, etc). Returns Some(new_data) to replace, None to pass through.
pub type RawInterceptorFn = Box<dyn Fn(&[u8]) -> Option<Vec<u8>> + Send + Sync>;

struct PatcherState {
    patches: HashMap<String, AfpPatchFn>,
    raw_interceptors: Vec<RawInterceptorFn>,
    /// Patched data buffers kept alive for the duration of the game.
    /// The AFP runtime reads from these pointers, so they must not be freed.
    kept_alive: Vec<(Vec<u8>, Vec<u8>)>,
    kept_alive_raw: Vec<Vec<u8>>,
}

unsafe impl Send for PatcherState {}

static STATE: Lazy<Mutex<PatcherState>> = Lazy::new(|| {
    Mutex::new(PatcherState {
        patches: HashMap::new(),
        raw_interceptors: Vec::new(),
        kept_alive: Vec::new(),
        kept_alive_raw: Vec::new(),
    })
});

// afp_stream_do_create signature (confirmed):
//   int afp_stream_do_create(void* data, int size, int flags)
type AfpStreamDoCreateFn = unsafe extern "C" fn(*const u8, i32, i32) -> i32;

static mut DETOUR: Option<GenericDetour<AfpStreamDoCreateFn>> = None;

/// Read the template name from descrambled AFP data.
/// The string table is already plaintext — just read raw ASCII bytes.
fn read_afp_name(data: &[u8]) -> Option<String> {
    if data.len() < 56 {
        return None;
    }

    // Verify AP2 magic (bytes 1-3 with high bit masked)
    if data[1] & 0x7F != b'2' || data[2] & 0x7F != b'P' || data[3] & 0x7F != b'A' {
        return None;
    }

    let name_offset = u16::from_le_bytes(data[10..12].try_into().ok()?) as usize;
    if name_offset == 0 {
        return None;
    }

    let st_offset = u32::from_le_bytes(data[48..52].try_into().ok()?) as usize;
    let st_size = u32::from_le_bytes(data[52..56].try_into().ok()?) as usize;

    if st_offset + st_size > data.len() || name_offset >= st_size {
        return None;
    }

    // String table is already descrambled plaintext — just read until null
    let start = st_offset + name_offset;
    let end = st_offset + st_size;
    let name_bytes: Vec<u8> = data[start..end]
        .iter()
        .take_while(|&&b| b != 0)
        .copied()
        .collect();

    String::from_utf8(name_bytes).ok()
}

/// The hook function that intercepts AFP stream creation.
unsafe extern "C" fn hooked_stream_do_create(data: *const u8, size: i32, flags: i32) -> i32 {
    let original = unsafe { &*std::ptr::addr_of!(DETOUR) }.as_ref().unwrap();

    if data.is_null() || size <= 4 {
        return original.call(data, size, flags);
    }

    let data_slice = std::slice::from_raw_parts(data, size as usize);

    // Try raw interceptors first (handles GEO data, etc.)
    {
        let mut state = STATE.lock().unwrap();
        for interceptor in &state.raw_interceptors {
            if let Some(new_data) = interceptor(data_slice) {
                let new_size = new_data.len() as i32;
                let new_ptr = new_data.as_ptr();
                state.kept_alive_raw.push(new_data);
                return original.call(new_ptr, new_size, flags);
            }
        }
    }

    // Then try named AFP template patches
    let has_patches = !STATE.lock().unwrap().patches.is_empty();
    if !has_patches || size <= 60 {
        return original.call(data, size, flags);
    }

    if let Some(ref name) = read_afp_name(data_slice) {
        let state = STATE.lock().unwrap();
        if let Some(patch_fn) = state.patches.get(name.as_str()) {
            // Data is already descrambled — pass empty BSI
            let result = patch_fn(data_slice, &[]);
            drop(state);

            if let Some((new_afp, new_bsi)) = result {
                log_info!(
                    "AfpPatcher: patched \"{}\" ({} -> {} bytes)",
                    name,
                    size,
                    new_afp.len()
                );

                let new_size = new_afp.len() as i32;
                let new_ptr = new_afp.as_ptr();
                STATE.lock().unwrap().kept_alive.push((new_afp, new_bsi));
                return original.call(new_ptr, new_size, flags);
            } else {
                log_warn!("AfpPatcher: patch function returned None for \"{}\"", name);
            }
        }
    }

    original.call(data, size, flags)
}

/// Register a patch callback for a specific AFP template name.
pub fn register_patch(name: &str, patch_fn: AfpPatchFn) {
    let mut state = STATE.lock().unwrap();
    log_info!("AfpPatcher: registered patch for \"{}\"", name);
    state.patches.insert(name.to_string(), patch_fn);
}

/// Register a raw data interceptor called for ALL data through afp_stream_do_create.
pub fn register_raw_interceptor(interceptor: RawInterceptorFn) {
    let mut state = STATE.lock().unwrap();
    log_info!("AfpPatcher: registered raw interceptor");
    state.raw_interceptors.push(interceptor);
}

/// Initialize the AFP patcher by hooking afp_stream_do_create.
pub fn init() -> bool {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    unsafe {
        let dll_name = CString::new("libafp-win64.dll").unwrap();
        let afp_handle = match GetModuleHandleA(PCSTR(dll_name.as_ptr() as *const u8)) {
            Ok(h) if !h.is_invalid() => h,
            _ => {
                log_warn!("AfpPatcher: libafp-win64.dll not loaded");
                return false;
            }
        };

        // afp_stream_do_create — resolve by name
        let func_name = CString::new("afp_stream_do_create").unwrap();
        let func_addr = match GetProcAddress(afp_handle, PCSTR(func_name.as_ptr() as *const u8)) {
            Some(f) => f as *const u8,
            None => {
                log_warn!("AfpPatcher: afp_stream_do_create not found");
                return false;
            }
        };

        let target: AfpStreamDoCreateFn = std::mem::transmute(func_addr);
        match crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(DETOUR),
            target,
            hooked_stream_do_create,
        ) {
            Ok(()) => {
                log_info!("AfpPatcher: hooked afp_stream_do_create @ {:p}", func_addr);
                true
            }
            Err(e) => {
                log_warn!("AfpPatcher: failed to install hook: {:?}", e);
                false
            }
        }
    }
}

/// Call afp_stream_do_create directly to register a new AFP template.
/// `data` must be fully descrambled (BSI applied + string table decoded).
/// The data is kept alive permanently (AFP runtime holds a pointer to it).
pub fn register_afp(data: Vec<u8>) -> bool {
    let size = data.len() as i32;
    let ptr = data.as_ptr();
    // Keep the data alive — AFP runtime reads from this pointer
    STATE.lock().unwrap().kept_alive_raw.push(data);
    unsafe {
        if let Some(ref detour) = *std::ptr::addr_of!(DETOUR) {
            let result = detour.call(ptr, size, 0);
            result >= 0
        } else {
            false
        }
    }
}
