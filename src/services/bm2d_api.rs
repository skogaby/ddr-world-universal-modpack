//! BM2D API — Typed wrappers around libafp named exports for MovieClip manipulation.
//!
//! Provides safe(r) access to libafp's runtime API for finding children,
//! setting clip masks, moving layers, controlling visibility, and driving scroll.
//!
//! Also hosts the **AFP-layer wrapper set** (`AfpLayer` + `create_layer_from_package`
//! / setters / `layer_play` / `destroy_layer`): raw-layer lifetime management for
//! mods that instantiate a BM2D package's clip themselves (animated background
//! previews). Resolved as an all-or-nothing group, non-fatally — a miss disables
//! only `afp_layers_available()`, not the rest of BM2D.
//!
//! All functions resolved by named export — no ordinals, no hardcoded offsets.
//! Pool base derived from AOB signature at init time.

use once_cell::sync::{Lazy, OnceCell};
use std::ffi::CString;
use std::sync::Mutex;

use crate::core::scanner::decode_rip_relative;
use crate::core::signatures::SignatureStore;
use crate::{log_debug, log_info, log_warn};

// Function pointer types — signatures verified via Ghidra disassembly of libafp-win64.dll
type AfpMcReferFn = unsafe extern "C" fn(u32, *const i8) -> i32;
type AfpLayerMcReferFn = unsafe extern "C" fn(u32, *const i8) -> i32;
type AfpMcSearchFn = unsafe extern "C" fn(u32, *const i8) -> i32;
type AfpLayerSetMaskFn = unsafe extern "C" fn(u32, i32, i32, i32, i32) -> i32;
type AfpLayerSetPositionFn = unsafe extern "C" fn(u32, *const [f32; 2]) -> i32; // pointer to {x,y} float pair
type AfpMcOpFn = unsafe extern "C" fn(u32, i32, u64, u64) -> i32; // variadic: mc_id, op, arg1, arg2
type AfpMcSetParamFn = unsafe extern "C" fn(u32, i32, u64, u64) -> i32; // mc_id, param, value, unused
type AfpMcGetParamFn = unsafe extern "C" fn(u32, i32, *mut f32) -> i32; // mc_id, param, out -> 0 on success
type AfpMcTraversalFn = unsafe extern "C" fn(i32, i32) -> i32; // mc_id, depth -> next-sibling layer id (or negative)
type AfpMcLoadBitmapFn = unsafe extern "C" fn(i32, *const i8) -> i32; // mc_id, name -> 0 on success

struct Api {
    mc_refer: AfpMcReferFn,
    layer_mc_refer: AfpLayerMcReferFn,
    mc_search: AfpMcSearchFn,
    layer_set_mask: AfpLayerSetMaskFn,
    layer_set_position: AfpLayerSetPositionFn,
    mc_op: AfpMcOpFn,
    mc_set_param: AfpMcSetParamFn,
    /// Resolved non-fatally: a name miss disables only the geometry read
    /// (`mc_get_param`), not the rest of BM2D. The export is confirmed present
    /// (`afp_mc_get_param` = Ordinal 115 @ libafp+0x3E370, resolved by NAME so
    /// it survives ordinal shifts); kept optional purely as defence against a
    /// future build dropping/renaming it. `None` when the export is absent.
    mc_get_param: Option<AfpMcGetParamFn>,
    mc_traversal: AfpMcTraversalFn,
    mc_load_bitmap: AfpMcLoadBitmapFn,
    pool_base: *const u8,
    pool_stride: usize,
    pool_max: usize,
}

unsafe impl Send for Api {}

static API: Lazy<Mutex<Option<Api>>> = Lazy::new(|| Mutex::new(None));

/// Initialize the BM2D API by resolving libafp named exports and deriving the
/// BM2D pool base from the `bm2d_pool_iter` AOB signature.
pub fn init(signatures: &SignatureStore) -> bool {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    // AFP-layer wrapper set first: it's independent of the pool derivation
    // below, and a failure here is non-fatal (only `afp_layers_available()`
    // reports false; the consumers degrade to chrome-only previews).
    init_layer_api();
    // Raw color op (afp_layer_set_color) — resolved independently and
    // non-fatally for the overlay-element-styling mod's opacity one-shots.
    init_raw_layer_ops();

    unsafe {
        let dll = CString::new("libafp-win64.dll").unwrap();
        let handle = match GetModuleHandleA(PCSTR(dll.as_ptr() as *const u8)) {
            Ok(h) if !h.is_invalid() => h,
            _ => {
                log_warn!("BM2D_API: libafp-win64.dll not loaded");
                return false;
            }
        };

        macro_rules! resolve {
            ($name:expr, $ty:ty) => {{
                let cname = CString::new($name).unwrap();
                match GetProcAddress(handle, PCSTR(cname.as_ptr() as *const u8)) {
                    Some(f) => {
                        log_info!("BM2D_API: resolved {} @ {:p}", $name, f);
                        #[allow(clippy::missing_transmute_annotations)]
                        std::mem::transmute::<_, $ty>(f)
                    }
                    None => {
                        log_warn!("BM2D_API: {} not found", $name);
                        return false;
                    }
                }
            }};
        }

        // Non-fatal variant: a missing export disables only the feature that
        // uses it, rather than aborting the whole BM2D init. Returns Option.
        macro_rules! resolve_opt {
            ($name:expr, $ty:ty) => {{
                let cname = CString::new($name).unwrap();
                match GetProcAddress(handle, PCSTR(cname.as_ptr() as *const u8)) {
                    Some(f) => {
                        log_info!("BM2D_API: resolved {} @ {:p} (optional)", $name, f);
                        #[allow(clippy::missing_transmute_annotations)]
                        Some(std::mem::transmute::<_, $ty>(f))
                    }
                    None => {
                        log_warn!(
                            "BM2D_API: {} not found (optional) — feature disabled",
                            $name
                        );
                        None
                    }
                }
            }};
        }

        // Derive pool base from AOB signature
        let pool_match = match signatures.get_address("bm2d_pool_iter") {
            Some(a) => a,
            None => {
                log_warn!("BM2D_API: bm2d_pool_iter signature not resolved");
                return false;
            }
        };
        log_info!(
            "BM2D_API: bm2d_pool_iter pattern matched @ {:p}",
            pool_match
        );

        // Pattern matches at INC; ADD RDI,0x240; CMP EBX,0x400.
        // Walk backwards from match to find LEA R13,[rip+disp32] (4C 8D 2D xx xx xx xx).
        let pool_base = derive_pool_base(pool_match);
        if pool_base.is_null() {
            log_warn!("BM2D_API: could not derive pool base — LEA R13 not found within 64 bytes before pattern");
            return false;
        }

        // Read stride and max from the matched pattern itself for validation
        // Pattern bytes: FF C3 48 81 C7 [stride_le32] 81 FB [max_le32]
        //                0  1  2  3  4  5..8           9  10 11..14
        let stride = (pool_match.add(5) as *const u32).read_unaligned() as usize;
        let max = (pool_match.add(11) as *const u32).read_unaligned() as usize;
        log_info!(
            "BM2D_API: pool @ {:p}, stride=0x{:X}, max={}",
            pool_base,
            stride,
            max
        );

        if stride == 0 || stride > 0x1000 || max == 0 || max > 0x10000 {
            log_warn!(
                "BM2D_API: pool params look wrong (stride=0x{:X}, max={}) — aborting",
                stride,
                max
            );
            return false;
        }

        let api = Api {
            mc_refer: resolve!("afp_mc_refer", AfpMcReferFn),
            layer_mc_refer: resolve!("afp_layer_mc_refer", AfpLayerMcReferFn),
            mc_search: resolve!("afp_mc_search", AfpMcSearchFn),
            layer_set_mask: resolve!("afp_layer_set_mask", AfpLayerSetMaskFn),
            layer_set_position: resolve!("afp_layer_set_position", AfpLayerSetPositionFn),
            mc_op: resolve!("afp_mc_op", AfpMcOpFn),
            mc_set_param: resolve!("afp_mc_set_param", AfpMcSetParamFn),
            mc_get_param: resolve_opt!("afp_mc_get_param", AfpMcGetParamFn),
            mc_traversal: resolve!("afp_mc_traversal", AfpMcTraversalFn),
            mc_load_bitmap: resolve!("afp_mc_load_bitmap", AfpMcLoadBitmapFn),
            pool_base,
            pool_stride: stride,
            pool_max: max,
        };

        *API.lock().unwrap() = Some(api);
        log_info!("BM2D_API: initialized — required exports resolved, pool derived from AOB");
        true
    }
}

/// Walk backwards from the pool iteration pattern to find a LEA to the pool base.
/// Looks for LEA with any register: 4C 8D 2D (R13), 4C 8D 35 (R14), 4C 8D 3D (R15),
/// 48 8D 2D (RBP), 48 8D 35 (RSI), etc.
unsafe fn derive_pool_base(pattern_match: *const u8) -> *const u8 {
    // RIP-relative LEA encodings: REX.W 8D modrm where modrm & 0xC7 == 0x05
    // REX prefix is 0x48 (W) or 0x4C (W+R)
    for back in 0..64usize {
        let candidate = pattern_match.sub(back);
        let rex = *candidate;
        if (rex == 0x48 || rex == 0x4C)
            && *candidate.add(1) == 0x8D
            && (*candidate.add(2) & 0xC7) == 0x05
        {
            let resolved = decode_rip_relative(candidate.add(3));
            log_info!("BM2D_API: found LEA [rip+disp] at offset -{} from pattern (rex=0x{:02X} modrm=0x{:02X}), pool={:p}",
                back, rex, *candidate.add(2), resolved);
            return resolved;
        }
    }
    std::ptr::null()
}

pub fn is_available() -> bool {
    API.lock().unwrap().is_some()
}

/// Find a named child of a parent MC (type-4 MC ID input).
pub fn find_child(parent_mc_id: u32, name: &str) -> Option<u32> {
    let api = API.lock().unwrap();
    let api = api.as_ref()?;
    let cname = CString::new(name).ok()?;
    let result = unsafe { (api.mc_refer)(parent_mc_id, cname.as_ptr()) };
    if result >= 0 {
        Some(result as u32)
    } else {
        None
    }
}

/// Find a named child of a parent layer (type-1 layer ID input, from BM2D pool).
pub fn layer_find_child(parent_layer_id: u32, name: &str) -> Option<u32> {
    let api = API.lock().unwrap();
    let api = api.as_ref()?;
    let cname = CString::new(name).ok()?;
    let result = unsafe { (api.layer_mc_refer)(parent_layer_id, cname.as_ptr()) };
    if result >= 0 {
        Some(result as u32)
    } else {
        None
    }
}

/// Search for a child by path (recursive through hierarchy).
pub fn search_child(parent_mc_id: u32, path: &str) -> Option<u32> {
    let api = API.lock().unwrap();
    let api = api.as_ref()?;
    let cpath = CString::new(path).ok()?;
    let result = unsafe { (api.mc_search)(parent_mc_id, cpath.as_ptr()) };
    if result >= 0 {
        Some(result as u32)
    } else {
        None
    }
}

/// Set a rectangular clip mask on a layer (type-1 layer ID).
pub fn set_mask(layer_id: u32, x: i32, y: i32, w: i32, h: i32) -> bool {
    let api = API.lock().unwrap();
    let api = match api.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let ret = unsafe { (api.layer_set_mask)(layer_id, x, y, w, h) };
    log_debug!(
        "BM2D_API: set_mask(0x{:08X}, {}, {}, {}, {}) = {}",
        layer_id,
        x,
        y,
        w,
        h,
        ret
    );
    ret == 0
}

/// Set a layer's position (type-1/2 layer ID). Takes x,y as floats.
/// AFP stores these directly as f32 in the layer struct at +0x130/+0x134.
pub fn set_position(layer_id: u32, x: f32, y: f32) -> bool {
    let api = API.lock().unwrap();
    let api = match api.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let xy = [x, y];
    let ret = unsafe { (api.layer_set_position)(layer_id, &xy) };
    log_debug!(
        "BM2D_API: set_position(0x{:08X}, {}, {}) = {}",
        layer_id,
        x,
        y,
        ret
    );
    ret == 0
}

// NOTE: a 2-arg `set_attribute(layer_id, attr)` wrapper used to live here.
// It was DELETED (2026-07-09): the `afp_layer_set_attribute` export is
// actually 3-arg `(id, mask, value)` (Ghidra-confirmed), so the old wrapper
// passed an undefined value register — any future caller would have written
// nondeterministic attribute bits into the engine. Use [`layer_set_attribute`]
// (the `AfpLayer` wrapper, correct 3-arg form) instead.

/// Execute a MovieClip operation (type-4 MC ID).
/// Op 0x0F04 = set scroll position in pixels.
pub fn mc_op(mc_id: u32, op: i32, value: i32) -> bool {
    let api = API.lock().unwrap();
    let api = match api.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let ret = unsafe { (api.mc_op)(mc_id, op, value as u64, 0) };
    log_debug!(
        "BM2D_API: mc_op(0x{:08X}, 0x{:04X}, {}) = {}",
        mc_id,
        op,
        value,
        ret
    );
    ret == 0
}

/// `afp_mc_op` with a STRING third argument (the export widens it to u64,
/// so a pointer fits). Op `0xF09` = goto-frame-label-by-string + play — the
/// stock FullcomboActor's label-jump call (docs/s_marvelous_judgement RE);
/// used by the s_marvelous flash re-drive. Game-thread-only like every
/// libafp call.
pub fn mc_op_str(mc_id: u32, op: i32, value: &std::ffi::CStr) -> bool {
    let api = match API.lock() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let api = match api.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let ret = unsafe { (api.mc_op)(mc_id, op, value.as_ptr() as u64, 0) };
    ret == 0
}

/// Set a MovieClip parameter (type-4 MC ID).
/// Param 0x1007 = visibility (0=hidden, 1=visible).
pub fn mc_set_param(mc_id: u32, param: i32, value: i32) -> bool {
    let api = API.lock().unwrap();
    let api = match api.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let ret = unsafe { (api.mc_set_param)(mc_id, param, value as u64, 0) };
    log_debug!(
        "BM2D_API: mc_set_param(0x{:08X}, 0x{:04X}, {}) = {}",
        mc_id,
        param,
        value,
        ret
    );
    ret == 0
}

/// Set a MovieClip's component scale — `afp_mc_set_param(id, 0x1003)` =
/// `pw_set_scale(sx, sy)` (RE'd from libafp's set-param jump table;
/// `docs/playfield_styling_research.md` §4b). COMPONENT-based: writes the
/// MC's scale fields (obj+0x124/+0x128) and rebuilds its local matrix from
/// components — position (obj+0xD0/+0xD4) is untouched, so unlike
/// `afp_layer_set_matrix` there is NO translation-preservation concern.
/// Scale is 1.0-normalized. The value is passed as a POINTER to two floats
/// (the dispatcher hands the handler `&value`, and the handler dereferences
/// it as `float*`).
pub fn mc_set_scale(mc_id: u32, sx: f32, sy: f32) -> bool {
    const MC_PARAM_SCALE: i32 = 0x1003;
    let api = match API.lock() {
        Ok(g) => g,
        Err(_) => return false,
    };
    let api = match api.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let vals: [f32; 2] = [sx, sy];
    let ret = unsafe { (api.mc_set_param)(mc_id, MC_PARAM_SCALE, vals.as_ptr() as u64, 0) };
    log_debug!(
        "BM2D_API: mc_set_scale(0x{:08X}, {}, {}) = {}",
        mc_id,
        sx,
        sy,
        ret
    );
    ret == 0
}

/// Read a 2-float MovieClip parameter (afp_mc_get_param). Param ids are
/// Flash-property table indices (RE'd set/get tables,
/// `docs/playfield_styling_research.md` §4b): `0x1000` = position (x,y),
/// `0x1003` = scale (sx,sy — 1.0-normalized).
pub fn mc_get_vec2(mc_id: u32, param: i32) -> Option<(f32, f32)> {
    let api = API.lock().ok()?;
    let api = api.as_ref()?;
    let get = api.mc_get_param?;
    let mut out: [f32; 4] = [0.0; 4];
    let ret = unsafe { get(mc_id, param, out.as_mut_ptr()) };
    if ret == 0 {
        Some((out[0], out[1]))
    } else {
        None
    }
}

/// Get a MovieClip parameter (afp_mc_get_param / Ordinal 115). The engine's
/// `createRowUI` reads the option preview box's geometry with this call using
/// param ids `0x1015` (X), `0x1016` (Y), `0x1008` (size), `0x101b` (size/extent).
/// The engine writes a float to the out buffer and returns 0 on success; we
/// truncate to `i32` (screen px), mirroring how the game consumes the values
/// (`(int)local_230`). Returns `None` if the export wasn't resolved, the clip
/// is invalid, or the call reports failure (non-zero).
///
/// The out buffer is oversized (some params write more than one float — e.g.
/// `0x101b` populates a second slot the game reads at `+4`), so a short buffer
/// is never written past.
pub fn mc_get_param(mc_id: u32, param: i32) -> Option<i32> {
    let api = API.lock().ok()?;
    let api = api.as_ref()?;
    let get = api.mc_get_param?;
    let mut out: [f32; 4] = [0.0; 4];
    let ret = unsafe { get(mc_id, param, out.as_mut_ptr()) };
    log_debug!(
        "BM2D_API: mc_get_param(0x{:08X}, 0x{:04X}) = {} (out={:?})",
        mc_id,
        param,
        ret,
        out
    );
    if ret == 0 {
        Some(out[0] as i32)
    } else {
        None
    }
}

/// Resolve a frame LABEL to its frame number on a MovieClip — the exact
/// call shape the game's NoteResultActor grade handler uses before its
/// SetFrame (`afp_mc_get_param(mc, 0x1012, label_cstr, &out_frame)`; the
/// out param is a u32 frame, not a float). Returns `None` when the export
/// is unresolved or the lookup fails (label absent / clip invalid) — the
/// failure IS observable here, unlike `mc_op(0xF09)` whose internal lookup
/// failure still returns success (libafp RE, s-marvelous deploy #6).
pub fn mc_frame_by_label(mc_id: u32, label: &std::ffi::CStr) -> Option<u32> {
    let api = API.lock().ok()?;
    let api = api.as_ref()?;
    let get = api.mc_get_param?;
    let mut out: [u32; 4] = [0; 4];
    // Same export as mc_get_param; param 0x1012 takes the label string as
    // the 3rd arg and writes the frame to the 4th. Cast through the
    // variadic-style op signature.
    let get: unsafe extern "C" fn(u32, i32, *const i8, *mut u32) -> i32 =
        unsafe { std::mem::transmute(get) };
    let ret = unsafe { get(mc_id, 0x1012, label.as_ptr(), out.as_mut_ptr()) };
    if ret == 0 {
        Some(out[0])
    } else {
        None
    }
}

/// Iterate active BM2D pool entries (non-zero layer_id). Calls `f(pool_index, layer_id)`.
pub fn for_each_active<F: FnMut(usize, u32) -> bool>(mut f: F) {
    let api = API.lock().unwrap();
    let api = match api.as_ref() {
        Some(a) => a,
        None => return,
    };
    for i in 0..api.pool_max {
        let entry = unsafe { api.pool_base.add(i * api.pool_stride) };
        let layer_id = unsafe { (entry.add(0x08) as *const u32).read_unaligned() };
        if layer_id != 0 && !f(i, layer_id) {
            break;
        }
    }
}

/// Find the CMovieClip pool wrapper OBJECT for the active clip that
/// contains ALL of the named child MCs. Clip names are NOT stored in the
/// pool slots (slot+0x114 holds the root-MC search path "/", cabinet-
/// diagnosed 2026-08-16), so discovery goes by content: a slot qualifies
/// when `afp_layer_mc_refer` resolves every listed child under its layer.
/// Returns the wrapper pointer (the pool slot itself — layer id at +0x08),
/// or `None`. Consumers that hold the pointer across frames MUST
/// re-validate it with [`wrapper_has_children`] before every use: pool
/// slots are recycled when scenes tear layers down.
pub fn find_wrapper_by_children(children: &[&str]) -> Option<*mut u8> {
    let api = API.lock().unwrap();
    let api = api.as_ref()?;
    for i in 0..api.pool_max {
        let entry = unsafe { api.pool_base.add(i * api.pool_stride) };
        if unsafe { wrapper_children_resolve(api, entry, children) } {
            return Some(entry as *mut u8);
        }
    }
    None
}

/// Re-validate a wrapper pointer previously returned by
/// [`find_wrapper_by_children`]: still an active slot whose layer resolves
/// every listed child. libafp validates the layer id, so a stale/recycled
/// slot simply fails the resolve.
pub fn wrapper_has_children(wrapper: *const u8, children: &[&str]) -> bool {
    if wrapper.is_null() {
        return false;
    }
    let api = API.lock().unwrap();
    let api = match api.as_ref() {
        Some(a) => a,
        None => return false,
    };
    unsafe { wrapper_children_resolve(api, wrapper, children) }
}

/// Active-slot + children check on a raw pool entry. Caller guarantees
/// `entry` points at a pool slot (static storage — reads are always safe).
unsafe fn wrapper_children_resolve(api: &Api, entry: *const u8, children: &[&str]) -> bool {
    let layer_id = (entry.add(0x08) as *const u32).read_unaligned();
    if layer_id == 0 {
        return false;
    }
    for child in children {
        let cname = match CString::new(*child) {
            Ok(c) => c,
            Err(_) => return false,
        };
        if (api.layer_mc_refer)(layer_id, cname.as_ptr()) < 0 {
            return false;
        }
    }
    true
}

/// Resolve a BM2D vtable method address by byte offset.
/// Reads from pool_base[0] (vtable is pre-initialized on all slots).
/// E.g., offset 0x30 = set_position wrapper.
pub fn get_vtable_method(byte_offset: usize) -> Option<*const u8> {
    let api = API.lock().unwrap();
    let api = api.as_ref()?;
    unsafe {
        let vtable = *(api.pool_base as *const *const u8);
        if vtable.is_null() {
            return None;
        }
        let func = *(vtable.add(byte_offset) as *const *const u8);
        if func.is_null() {
            return None;
        }
        Some(func)
    }
}

/// Advance to the next sibling layer at `depth`. Returns `None` when the
/// walk reaches the end of the sibling chain (libafp returns a negative
/// integer). The `mc_id` input is a type-1 layer id (from the BM2D pool or
/// from a prior `layer_find_child` call).
pub fn mc_traversal(mc_id: u32, depth: i32) -> Option<u32> {
    let api = API.lock().unwrap();
    let api = api.as_ref()?;
    let result = unsafe { (api.mc_traversal)(mc_id as i32, depth) };
    if result >= 0 {
        Some(result as u32)
    } else {
        None
    }
}

/// Bind the named atlas bitmap to the layer identified by `mc_id`. Returns
/// `true` on success (libafp returns 0). An empty `name` or an unknown
/// texture returns `false` (libafp returns a negative error code, which we
/// saw as the `afp_mc_load_bitmap no .` warning in prior crash logs).
pub fn mc_load_bitmap(mc_id: u32, name: &str) -> bool {
    let api = API.lock().unwrap();
    let api = match api.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let cname = match CString::new(name) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let ret = unsafe { (api.mc_load_bitmap)(mc_id as i32, cname.as_ptr()) };
    log_debug!(
        "BM2D_API: mc_load_bitmap(0x{:08X}, {:?}) = {}",
        mc_id,
        name,
        ret
    );
    ret == 0
}

/// Search all active BM2D pool entries for one that has a descendant with the given name.
/// Uses afp_layer_mc_refer which accepts layer IDs (type 1) from the BM2D pool.
/// Returns Vec<(parent_layer_id, child_mc_id)>.
pub fn scan_for_child(name: &str) -> Vec<(u32, u32)> {
    let api = API.lock().unwrap();
    let api = match api.as_ref() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let cname = match CString::new(name) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();
    for i in 0..api.pool_max {
        let entry = unsafe { api.pool_base.add(i * api.pool_stride) };
        let layer_id = unsafe { (entry.add(0x08) as *const u32).read_unaligned() };
        if layer_id != 0 {
            let child = unsafe { (api.layer_mc_refer)(layer_id, cname.as_ptr()) };
            if child >= 0 {
                results.push((layer_id, child as u32));
            }
        }
    }
    log_debug!(
        "BM2D_API: scan_for_child(\"{}\") found {} hits",
        name,
        results.len()
    );
    results
}

// ─────────────────────────────────────────────────────────────────────
// AFP-layer wrapper set — raw-layer create / setup / play / destroy.
//
// Cabinet-validated recipe (2026-07-09, see `.agents/planning/
// 20260708-background-preview-overlay/progress.md` "STEP-1 VALIDATED
// FACTS") for instantiating a BM2D package's clip as a layer we own:
//
//   create_layer_from_package(pkg_id, "bg_root")
//   layer_set_attribute(&l, 0x200, 0x200)   // standard post-create setup
//   layer_set_group(&l, g) + layer_set_priority(&l, p)  // z placement
//   layer_set_scale(&l, s, s) + layer_set_position(&l, x, y)  // compose
//   layer_set_mask(&l, x, y, w, h)          // screen-space hard crop
//   layer_play(&l, 1.0)                     // 0.0 = paused static frame
//   ... engine renders + loops it ...
//   destroy_layer(l)                        // consumes the handle
//
// The ENGINE renders owned layers from its per-group display jobs —
// there is deliberately NO self-drive (`afp_do_render`) wrapper here:
// calling it from hook context asserts in `afp_advance_play_data` and
// crashes the game (Step-1 probe v1, phase B).
//
// Resolution is all-or-nothing and NON-fatal to `init`: any missing
// export leaves `afp_layers_available()` false and consumers degrade.
// `afpu_get_afp_info_at_package` comes from libafputils-win64.dll; the
// rest from libafp-win64.dll. All calls are render/game-thread only.
// ─────────────────────────────────────────────────────────────────────

/// `u32 afpu_get_afp_info_at_package(AfpInfo* out, u32 package_id, const char* name)`
/// — 0 on success. Out struct: name ptr at +0x10, stream id (u32) at +0x18.
type AfpuGetAfpInfoAtPackageFn = unsafe extern "C" fn(*mut AfpInfoDesc, u32, *const i8) -> u32;
/// `u32 afp_layer_create_with_property(u32 stream_id, const char* name, u64, u64)`
type AfpLayerCreateFn = unsafe extern "C" fn(u32, *const i8, u64, u64) -> u32;
/// `i32 afp_id_is_valid(u32 kind /*5=AFP_LAYER*/, u32 id)` — negative = invalid.
type AfpIdIsValidFn = unsafe extern "C" fn(u32, u32) -> i32;
/// `i32 afp_layer_play(u32 id, f32 rate)` — rate 1.0 = play, 0.0 = pause.
type AfpLayerPlayFn = unsafe extern "C" fn(u32, f32) -> i32;
/// `i32 afp_layer_set_attribute(u32 id, u32 mask, u32 value)` — the export's
/// true 3-arg form (bit 1 = visible, 0x200 = standard display setup).
type AfpLayerSetAttribute3Fn = unsafe extern "C" fn(u32, u32, u32) -> i32;
/// `i32 afp_layer_set_priority(u32 id, u16 priority)` — display sorts ascending
/// within a group: HIGHER priority = drawn later = on top.
type AfpLayerSetPriorityFn = unsafe extern "C" fn(u32, u16) -> i32;
/// `i32 afp_layer_set_group(u32 id, u16 group)`
type AfpLayerSetGroupFn = unsafe extern "C" fn(u32, u16) -> i32;
/// `i32 afp_layer_set_matrix(u32 id, const f32 m[6] /*{a,b,c,d,tx,ty}*/)`
type AfpLayerSetMatrixFn = unsafe extern "C" fn(u32, *const [f32; 6]) -> i32;
/// `i32 afp_layer_do_destroy(i32 kind /*5*/, u32 id, u8 deferred)`
type AfpLayerDoDestroyFn = unsafe extern "C" fn(i32, u32, u8) -> i32;

/// AFP object-kind constant for layer ids (confirmed at every game call site).
const AFP_LAYER: u32 = 5;

/// Out struct for `afpu_get_afp_info_at_package` (0x28 bytes per the Ghidra
/// analysis). Oversized here (0x30) so a layout surprise can't corrupt our
/// stack.
#[repr(C)]
struct AfpInfoDesc {
    raw: [u32; 12],
}

impl AfpInfoDesc {
    fn zeroed() -> Self {
        Self { raw: [0; 12] }
    }
    /// Template-name pointer the create call takes as its 2nd arg (+0x10).
    fn name_ptr(&self) -> *const i8 {
        (((self.raw[5] as u64) << 32) | self.raw[4] as u64) as *const i8
    }
    /// AFP stream id the create call takes as its 1st arg (+0x18).
    fn stream_id(&self) -> u32 {
        self.raw[6]
    }
}

struct LayerApi {
    afpu_get_afp_info_at_package: AfpuGetAfpInfoAtPackageFn,
    layer_create: AfpLayerCreateFn,
    id_is_valid: AfpIdIsValidFn,
    layer_play: AfpLayerPlayFn,
    layer_set_attribute3: AfpLayerSetAttribute3Fn,
    layer_set_priority: AfpLayerSetPriorityFn,
    layer_set_group: AfpLayerSetGroupFn,
    layer_set_position: AfpLayerSetPositionFn,
    layer_set_matrix: AfpLayerSetMatrixFn,
    layer_set_mask: AfpLayerSetMaskFn,
    layer_do_destroy: AfpLayerDoDestroyFn,
}

// Function pointers into loaded game modules — valid for the process
// lifetime; only invoked from the render thread.
unsafe impl Send for LayerApi {}
unsafe impl Sync for LayerApi {}

/// Write-once at init, read-only after — no lock needed (and no `unwrap`
/// on a mutex from render-thread callbacks).
static LAYER_API: OnceCell<LayerApi> = OnceCell::new();

/// `i32 afp_layer_set_color(u32 id, f32 r, f32 g, f32 b, f32 a)` — the
/// multiplicative CXFORM color setter (0 on success). Resolved NON-fatally
/// and INDEPENDENTLY of the all-or-nothing `LAYER_API` set: a miss must not
/// disable the bg-preview wrapper set (which doesn't use it), it only leaves
/// [`layer_color_available`] false. `Some(None)` = resolved-but-absent,
/// `None` = not yet resolved.
type AfpLayerSetColorRawFn = unsafe extern "C" fn(u32, f32, f32, f32, f32) -> i32;
static LAYER_SET_COLOR: OnceCell<Option<AfpLayerSetColorRawFn>> = OnceCell::new();

/// `i32 afp_layer_get_matrix(u32 id, f32 m[6] /*{a,b,c,d,tx,ty}*/)` — the
/// layer 2×3 matrix reader (0 on success). Same independent-optional-cell
/// treatment as [`LAYER_SET_COLOR`].
type AfpLayerGetMatrixRawFn = unsafe extern "C" fn(u32, *mut f32) -> i32;
static LAYER_GET_MATRIX: OnceCell<Option<AfpLayerGetMatrixRawFn>> = OnceCell::new();

/// Resolve a named export from an already-loaded module, or None (logged).
unsafe fn resolve_named_export(module: &str, name: &str) -> Option<*const ()> {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    let c_module = CString::new(module).ok()?;
    let handle = match GetModuleHandleA(PCSTR(c_module.as_ptr() as *const u8)) {
        Ok(h) if !h.is_invalid() => h,
        _ => {
            log_warn!("BM2D_API: {} not loaded", module);
            return None;
        }
    };
    let c_name = CString::new(name).ok()?;
    match GetProcAddress(handle, PCSTR(c_name.as_ptr() as *const u8)) {
        Some(f) => Some(f as *const ()),
        None => {
            log_warn!("BM2D_API: {}!{} not found", module, name);
            None
        }
    }
}

/// Resolve the AFP-layer wrapper set (all-or-nothing). Non-fatal: a miss
/// only leaves `afp_layers_available()` false.
fn init_layer_api() -> bool {
    if LAYER_API.get().is_some() {
        return true;
    }

    macro_rules! export {
        ($module:expr, $name:expr, $ty:ty) => {
            match unsafe { resolve_named_export($module, $name) } {
                Some(f) => unsafe { std::mem::transmute::<*const (), $ty>(f) },
                None => {
                    log_warn!(
                        "BM2D_API: AFP-layer API unavailable ({}!{} missing)",
                        $module,
                        $name
                    );
                    return false;
                }
            }
        };
    }
    const AFP: &str = "libafp-win64.dll";
    const AFPU: &str = "libafputils-win64.dll";

    let api = LayerApi {
        afpu_get_afp_info_at_package: export!(
            AFPU,
            "afpu_get_afp_info_at_package",
            AfpuGetAfpInfoAtPackageFn
        ),
        layer_create: export!(AFP, "afp_layer_create_with_property", AfpLayerCreateFn),
        id_is_valid: export!(AFP, "afp_id_is_valid", AfpIdIsValidFn),
        layer_play: export!(AFP, "afp_layer_play", AfpLayerPlayFn),
        layer_set_attribute3: export!(AFP, "afp_layer_set_attribute", AfpLayerSetAttribute3Fn),
        layer_set_priority: export!(AFP, "afp_layer_set_priority", AfpLayerSetPriorityFn),
        layer_set_group: export!(AFP, "afp_layer_set_group", AfpLayerSetGroupFn),
        layer_set_position: export!(AFP, "afp_layer_set_position", AfpLayerSetPositionFn),
        layer_set_matrix: export!(AFP, "afp_layer_set_matrix", AfpLayerSetMatrixFn),
        layer_set_mask: export!(AFP, "afp_layer_set_mask", AfpLayerSetMaskFn),
        layer_do_destroy: export!(AFP, "afp_layer_do_destroy", AfpLayerDoDestroyFn),
    };
    let ok = LAYER_API.set(api).is_ok();
    if ok {
        log_info!("BM2D_API: AFP-layer API resolved (libafp x10 + libafputils x1)");
    }
    ok
}

/// True once every export of the AFP-layer wrapper set resolved.
pub fn afp_layers_available() -> bool {
    LAYER_API.get().is_some()
}

/// Resolve `afp_layer_set_color` (+ `afp_layer_get_matrix`) into their own
/// optional cells. Non-fatal and independent of [`init_layer_api`]: a miss
/// leaves only the corresponding `*_available()` false.
fn init_raw_layer_ops() {
    if LAYER_SET_COLOR.get().is_none() {
        let set_color = unsafe { resolve_named_export("libafp-win64.dll", "afp_layer_set_color") }
            .map(|f| unsafe { std::mem::transmute::<*const (), AfpLayerSetColorRawFn>(f) });
        if set_color.is_some() {
            log_info!("BM2D_API: resolved afp_layer_set_color (raw color op)");
        } else {
            log_warn!("BM2D_API: afp_layer_set_color not found — raw color ops disabled");
        }
        let _ = LAYER_SET_COLOR.set(set_color);
    }
    if LAYER_GET_MATRIX.get().is_none() {
        let get_matrix =
            unsafe { resolve_named_export("libafp-win64.dll", "afp_layer_get_matrix") }
                .map(|f| unsafe { std::mem::transmute::<*const (), AfpLayerGetMatrixRawFn>(f) });
        if get_matrix.is_some() {
            log_info!("BM2D_API: resolved afp_layer_get_matrix (raw matrix read)");
        } else {
            log_warn!("BM2D_API: afp_layer_get_matrix not found — raw matrix reads disabled");
        }
        let _ = LAYER_GET_MATRIX.set(get_matrix);
    }
}

/// Non-owning: read a **game-owned** layer's 2×3 affine matrix
/// `{a,b,c,d,tx,ty}`. Same raw-id / non-owning caveat as
/// [`layer_set_scale_raw`]. `None` if `afp_layer_get_matrix` is unavailable
/// or the engine call fails.
pub fn layer_get_matrix_raw(layer_id: u32) -> Option<[f32; 6]> {
    let get = LAYER_GET_MATRIX.get().and_then(|o| *o)?;
    let mut m: [f32; 6] = [0.0; 6];
    unsafe {
        if get(layer_id, m.as_mut_ptr()) == 0 {
            Some(m)
        } else {
            None
        }
    }
}

/// Non-owning: write a **game-owned** layer's full 2×3 affine matrix
/// `{a,b,c,d,tx,ty}`. Same raw-id / non-owning caveat as
/// [`layer_set_scale_raw`]. Use with [`layer_get_matrix_raw`] for
/// read-modify-write transforms that must preserve the game's own
/// scale/translation components.
pub fn layer_set_matrix_raw(layer_id: u32, m: &[f32; 6]) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    unsafe { (api.layer_set_matrix)(layer_id, m) == 0 }
}

/// Non-owning: set a **game-owned** layer's 2×3 affine matrix to a pure
/// uniform/anisotropic scale `{sx,0,0,sy,0,0}` about the layer origin. The
/// caller MUST know `layer_id` references a live engine layer. This
/// deliberately takes a raw id (NOT an [`AfpLayer`]): these layers belong to
/// the game, and wrapping one in `AfpLayer` would arm its destroy-on-drop and
/// free a layer the game still owns. Reuses the same `afp_layer_set_matrix`
/// export the bg-preview wrapper set resolves. Returns false if that export
/// is unavailable ([`afp_layers_available`]) or the engine call fails.
pub fn layer_set_scale_raw(layer_id: u32, sx: f32, sy: f32) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    let m: [f32; 6] = [sx, 0.0, 0.0, sy, 0.0, 0.0];
    unsafe { (api.layer_set_matrix)(layer_id, &m) == 0 }
}

/// Non-owning: set a **game-owned** layer's 2×3 affine to a pure scale WITH an
/// explicit translation: `{sx,0,0,sy,tx,ty}`. Needed because the engine stores
/// the layer translation in the SAME 4×4 matrix (`layerobj+0x130/0x134`) that
/// `afp_layer_set_matrix` overwrites wholesale — so a bare scale would zero the
/// translation and slam the element to the origin. Pass the layer's current
/// position as `(tx,ty)` (for these HUD clips, the `SetPosition` x/y). A
/// subsequent `afp_layer_set_position` only rewrites the translation dwords, so
/// the scale set here survives later game repositions. Same raw-id / non-owning
/// caveat as [`layer_set_scale_raw`].
pub fn layer_set_scale_translate_raw(layer_id: u32, sx: f32, sy: f32, tx: f32, ty: f32) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    let m: [f32; 6] = [sx, 0.0, 0.0, sy, tx, ty];
    unsafe { (api.layer_set_matrix)(layer_id, &m) == 0 }
}

/// Non-owning: set a **game-owned** layer's multiplicative color transform
/// (`r,g,b,a` each in `[0,1]`; standard Flash CXFORM mult channel). Same
/// raw-id / non-owning caveat as [`layer_set_scale_raw`] — never wrap these
/// game-owned ids in [`AfpLayer`]. Returns false if `afp_layer_set_color` is
/// unavailable ([`layer_color_available`]) or the engine call fails.
pub fn layer_set_color_raw(layer_id: u32, r: f32, g: f32, b: f32, a: f32) -> bool {
    let set_color = match LAYER_SET_COLOR.get().and_then(|o| *o) {
        Some(f) => f,
        None => return false,
    };
    unsafe { set_color(layer_id, r, g, b, a) == 0 }
}

/// True once `afp_layer_set_color` resolved (raw color ops usable).
pub fn layer_color_available() -> bool {
    LAYER_SET_COLOR.get().map(|o| o.is_some()).unwrap_or(false)
}

/// Non-owning: start/pause playback of a **game-owned** layer. `rate` is
/// the FLOAT playback rate (1.0 = play, 0.0 = paused static frame). Same
/// raw-id / non-owning caveat as [`layer_set_scale_raw`] — never wrap these
/// game-owned ids in [`AfpLayer`]. Used by the in-place song reset to
/// restore the pacemaker (`dance_score_compare`) clip to its song-start
/// paused state (the NoteResultActor's own onSetup plays it at rate 0).
pub fn layer_play_raw(layer_id: u32, rate: f32) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    unsafe { (api.layer_play)(layer_id, rate) == 0 }
}

/// Non-owning: set attribute bits on a **game-owned** layer
/// (`afp_layer_set_attribute(id, mask, value)` — mask 0x1 = visibility).
/// Same raw-id / non-owning caveat as [`layer_set_scale_raw`]. Used by the
/// pacemaker→ms-error swap to re-assert the `dance_score_compare` clip's
/// visibility on the dispatch that first forces `NoteResultActor+0xC0`
/// (the game's own set-visible ran earlier in the same handler with the
/// stale 0).
pub fn layer_set_attribute_raw(layer_id: u32, mask: u32, value: u32) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    unsafe { (api.layer_set_attribute3)(layer_id, mask, value) == 0 }
}

/// An AFP layer this mod owns. Deliberately not `Copy`/`Clone`:
/// [`destroy_layer`] consumes the handle, making double-destroy a compile
/// error. Dropping one without destroying leaks the engine-side layer
/// (logged as a warning so lifecycle bugs surface).
pub struct AfpLayer {
    id: u32,
}

impl AfpLayer {
    /// The raw engine layer id (for logging/diagnostics).
    pub fn id(&self) -> u32 {
        self.id
    }
}

impl Drop for AfpLayer {
    fn drop(&mut self) {
        log_warn!(
            "BM2D_API: AfpLayer 0x{:08X} dropped without destroy_layer — engine layer leaked",
            self.id
        );
    }
}

/// Resolve `template` inside a BM2D package (by its afpu package id, the
/// u32 the game stores at package+0x314) and create + validate a layer for
/// it. The new layer has engine defaults: group 0, priority 0, identity
/// transform (full-screen 1280x720), not yet playing — callers follow with
/// the standard setup (attribute 0x200, group/priority, scale/position/
/// mask, then [`layer_play`]). Looping is the engine default.
pub fn create_layer_from_package(afpu_package_id: u32, template: &str) -> Option<AfpLayer> {
    let api = LAYER_API.get()?;
    let c_template = CString::new(template).ok()?;
    unsafe {
        let mut desc = AfpInfoDesc::zeroed();
        let ret =
            (api.afpu_get_afp_info_at_package)(&mut desc, afpu_package_id, c_template.as_ptr());
        if ret != 0 {
            log_warn!(
                "BM2D_API: afpu_get_afp_info_at_package({:?}) = 0x{:08X} (pkg_id=0x{:08X})",
                template,
                ret,
                afpu_package_id
            );
            return None;
        }
        let id = (api.layer_create)(desc.stream_id(), desc.name_ptr(), 0, 0);
        let valid = (api.id_is_valid)(AFP_LAYER, id);
        if valid < 0 {
            // No destroy here, deliberately: an id that fails `afp_id_is_valid`
            // references no live engine object (the game's own create sites
            // bail the same way without a destroy). Do not "fix" this into a
            // destroy-of-invalid-id.
            log_warn!(
                "BM2D_API: layer create({:?}) = 0x{:08X} but is_valid = {} — no layer",
                template,
                id,
                valid
            );
            return None;
        }
        Some(AfpLayer { id })
    }
}

/// Set attribute bits: `value` masked by `mask` (the export's true 3-arg
/// form). Bit 1 = visible; 0x200 = the standard post-create display setup
/// the game applies to every layer it creates (NOT a one-shot flag —
/// looping clips keep it set).
pub fn layer_set_attribute(layer: &AfpLayer, mask: u32, value: u32) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    unsafe { (api.layer_set_attribute3)(layer.id, mask, value) == 0 }
}

/// Display priority within the layer's group — display sorts ascending, so
/// HIGHER priority = drawn later = on top.
pub fn layer_set_priority(layer: &AfpLayer, priority: u16) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    unsafe { (api.layer_set_priority)(layer.id, priority) == 0 }
}

/// BM2D render group (the engine renders per group each frame; groups 0-5
/// draw in job order 0,4,5,1,2,3 — the options modal lives in group 4).
pub fn layer_set_group(layer: &AfpLayer, group: u16) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    unsafe { (api.layer_set_group)(layer.id, group) == 0 }
}

/// Uniform/anisotropic scale via the layer's 2x3 affine matrix
/// `{sx,0,0,sy,0,0}`. COMPOSES with [`layer_set_position`] (top-left
/// anchor) — cabinet-validated.
pub fn layer_set_scale(layer: &AfpLayer, sx: f32, sy: f32) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    let m: [f32; 6] = [sx, 0.0, 0.0, sy, 0.0, 0.0];
    unsafe { (api.layer_set_matrix)(layer.id, &m) == 0 }
}

/// Screen position of the (scaled) clip's top-left corner.
pub fn layer_set_position(layer: &AfpLayer, x: f32, y: f32) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    let xy = [x, y];
    unsafe { (api.layer_set_position)(layer.id, &xy) == 0 }
}

/// Screen-space rectangular hard crop.
pub fn layer_set_mask(layer: &AfpLayer, x: i32, y: i32, w: i32, h: i32) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    unsafe { (api.layer_set_mask)(layer.id, x, y, w, h) == 0 }
}

/// Show/hide via attribute bit 1.
pub fn layer_set_visible(layer: &AfpLayer, visible: bool) -> bool {
    layer_set_attribute(layer, 0x1, if visible { 0x1 } else { 0x0 })
}

/// Start/stop playback. `rate` is a FLOAT playback rate: 1.0 = play
/// (looping is the engine default), 0.0 = paused static frame. Play also
/// clears the freshly-created layer's play-gate — a new layer doesn't
/// animate until played.
pub fn layer_play(layer: &AfpLayer, rate: f32) -> bool {
    let api = match LAYER_API.get() {
        Some(a) => a,
        None => return false,
    };
    unsafe { (api.layer_play)(layer.id, rate) == 0 }
}

/// Destroy the engine layer. Consumes the handle (exactly-once by
/// construction). Destroy layers BEFORE releasing their package. Returns
/// false (with the handle consumed but the engine call skipped/failed) if
/// the API is unavailable or the engine reports failure — callers should
/// surface a false return, since an undestroyed layer bound to a
/// soon-released package is the deferred-destroy crash class.
pub fn destroy_layer(layer: AfpLayer) -> bool {
    let id = layer.id;
    let Some(api) = LAYER_API.get() else {
        // Unreachable in practice (layers only exist once the API resolved).
        // Let the handle drop normally so the leak warning fires.
        return false;
    };
    std::mem::forget(layer); // consumed — suppress the leak warning
    let ret = unsafe { (api.layer_do_destroy)(AFP_LAYER as i32, id, 0) };
    ret == 0
}
