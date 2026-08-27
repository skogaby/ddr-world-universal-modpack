//! Widget Renderer — Native widget lifecycle via game render list integration.
//!
//! Instead of manually calling render_function from a hook, we create proper
//! agcs::BmpString wrappers and insert them into the game's own render list.
//! The game then renders our widgets naturally through its normal pipeline.
//!
//! See docs/widget_registration_system.md for the full architecture.

use once_cell::sync::Lazy;
use retour::GenericDetour;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::Mutex;

use crate::core::memory;
use crate::core::module_resolver::GameModule;
use crate::core::scanner::{decode_call_rel32, decode_rip_relative, scan_xrefs_to};
use crate::core::signatures::SignatureStore;
use crate::widgets::image_widget::{ImageWidget, ImageWidgetConfig};
use crate::widgets::text_widget::TextWidget;
use crate::{log_error, log_info, log_warn};

type RenderFn = unsafe extern "C" fn(*mut u8);
type WidgetFactoryFn = unsafe extern "C" fn(*const u8, i32, i32, *const u8) -> *mut u8;

pub(crate) struct RendererInner {
    game_base: *const u8,
    game_size: usize,
    font_ptr: *const u8,
    font_captured: bool,
    widget_factory_addr: *const u8,
    sprite_vtable_addr: *const u8,
    // Derived addresses
    bmpstring_vtable: *const u8,
    scene_manager_global: *const u8,
    wrapper_constructor_addr: *const u8,
    game_alloc_fn: *const u8,
    game_alloc_heap: *const u8,
    derived_resolved: bool,
    /// Closures to run on the game/render thread at the start of the next frame.
    pending_updates: Vec<Box<dyn FnOnce() + Send>>,
}

unsafe impl Send for RendererInner {}
unsafe impl Sync for RendererInner {}

pub(crate) static RENDERER: Lazy<Mutex<RendererInner>> = Lazy::new(|| {
    Mutex::new(RendererInner {
        game_base: std::ptr::null(),
        game_size: 0,
        font_ptr: std::ptr::null(),
        font_captured: false,
        widget_factory_addr: std::ptr::null(),
        sprite_vtable_addr: std::ptr::null(),
        bmpstring_vtable: std::ptr::null(),
        scene_manager_global: std::ptr::null(),
        wrapper_constructor_addr: std::ptr::null(),
        game_alloc_fn: std::ptr::null(),
        game_alloc_heap: std::ptr::null(),
        derived_resolved: false,
        pending_updates: Vec::new(),
    })
});

static mut RENDER_HOOK: Option<GenericDetour<RenderFn>> = None;

/// render_function hook — only used for one-time font pointer capture
/// and deferred arc loading (after BM2D is initialized).
unsafe extern "C" fn render_function_hook(widget: *mut u8) {
    {
        let mut r = RENDERER.lock().unwrap();
        if !r.font_captured {
            let render_state = *(widget.add(0x10) as *const *const u8);
            if !render_state.is_null() {
                let font = *(render_state.add(0x70) as *const *const u8);
                if !font.is_null() {
                    r.font_ptr = font;
                    r.font_captured = true;
                    log_info!("WidgetRenderer: font pointer captured @ {:p}", font);
                }
            }
        }
    }

    if let Some(ref hook) = RENDER_HOOK {
        hook.call(widget);
    }
}

static mut WRAPPER_HOOK: Option<GenericDetour<RenderFn>> = None;

/// Lock-free mirror of `scene_manager_global` (set once at init) for
/// hot-path consumers that must not take the RENDERER mutex.
static SCENE_MGR_GLOBAL_MIRROR: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

/// The render-list MANAGER the DLL's widgets register into
/// (`*(scene_manager) + 0xB0` — see `register_in_render_list`), or null.
/// Lock-free; the overlay-draw emitter identifies the widget layer's
/// layer-table entry by pointer identity with this.
pub fn render_list_manager() -> *mut u8 {
    let global = SCENE_MGR_GLOBAL_MIRROR.load(Ordering::Acquire);
    if global.is_null() {
        return std::ptr::null_mut();
    }
    unsafe {
        let scene_mgr = *(global as *const *const u8);
        if scene_mgr.is_null() {
            return std::ptr::null_mut();
        }
        *(scene_mgr.add(0xB0) as *const *mut u8)
    }
}

/// One-shot latch for the boot-time free-pool diagnostic (see
/// `log_free_pool_count_once`).
static POOL_DIAG_LOGGED: AtomicBool = AtomicBool::new(false);

/// Walk the render-list manager's free-node pool and count the available
/// widget nodes. Game thread only (reads live engine lists). `Err` carries
/// the unavailability reason for diagnostics.
fn walk_free_pool() -> Result<usize, &'static str> {
    // Poison-recover rather than unwrap: reachable from an extern "C" frame.
    let scene_mgr_global = {
        let r = match RENDERER.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        r.scene_manager_global
    };
    if scene_mgr_global.is_null() {
        return Err("scene_manager_global null");
    }
    // Manager layout mirrors `register_in_render_list`: +0x18 free head,
    // +0x20 sentinel; free node +0x08 = next. Pool empty when head == sentinel.
    unsafe {
        let scene_mgr = *(scene_mgr_global as *const *const u8);
        if scene_mgr.is_null() {
            return Err("scene manager null");
        }
        let render_list_mgr = *(scene_mgr.add(0xB0) as *const *const u8);
        if render_list_mgr.is_null() {
            return Err("render list manager null");
        }
        let sentinel = *(render_list_mgr.add(0x20) as *const *const u8);
        let mut node = *(render_list_mgr.add(0x18) as *const *const u8);
        if sentinel.is_null() || node.is_null() {
            return Err("pool head/sentinel null");
        }
        // Hard cap against a corrupted list — a healthy pool is far smaller.
        const WALK_CAP: usize = 4096;
        let mut count: usize = 0;
        while node != sentinel && !node.is_null() && count < WALK_CAP {
            count += 1;
            node = *(node.add(0x08) as *const *const u8);
        }
        if count >= WALK_CAP {
            return Err("walk exceeded cap (list corrupt?)");
        }
        Ok(count)
    }
}

/// Free widget-node count, for callers budgeting decorative widgets (e.g.
/// the mod menu's chrome headroom check). Game thread only. `None` when the
/// walk is unavailable — treat as "unknown", not "exhausted": per-widget
/// creation failure is already non-fatal.
pub fn free_node_count() -> Option<usize> {
    walk_free_pool().ok()
}

/// One-shot boot diagnostic: walk the render-list manager's free-node pool and
/// log how many widget nodes remain available. The pool is a game-side
/// pre-allocation of unknown size and nodes are permanently consumed
/// (`destroy()` only hides), so this count is the budget every widget-creating
/// feature draws from. Runs on the first wrapper-render pass — the manager is
/// guaranteed live there (the wrapper being rendered is a node in its list).
/// Fail-open: any null pointer or an implausibly long walk logs one
/// "unavailable" line instead; widget creation is never affected.
fn log_free_pool_count_once() {
    if POOL_DIAG_LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    match walk_free_pool() {
        Ok(count) => log_info!(
            "WidgetRenderer: render list free pool: {} node(s) available",
            count
        ),
        Err(reason) => log_info!("WidgetRenderer: free pool walk unavailable ({})", reason),
    }
}

/// wrapper_render hook — drains pending_updates on the game thread.
unsafe extern "C" fn wrapper_render_hook(this: *mut u8) {
    // Poll arcade input every render frame. Runs on the game's render thread
    // at native refresh rate — more responsive than a fixed-interval background
    // thread and matches the thread the game itself reads input on.
    crate::services::input_manager::poll();

    // One-shot pool diagnostic (cheap latched atomic after the first frame).
    log_free_pool_count_once();

    // Overlay-draw tick: per-scene command-list diagnostics (emission lives
    // in overlay_draw's layer-dispatcher detour). Panic-contained inside.
    crate::services::overlay_draw::on_wrapper_render();

    {
        // Poison-recover rather than unwrap: this is an extern "C" frame — a
        // panic here is UB (CLAUDE.md rule 1) — and the queue is plain data
        // (a poisoning panic mid-drain loses nothing the closures don't
        // re-establish themselves).
        let mut r = match RENDERER.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !r.pending_updates.is_empty() {
            let updates: Vec<Box<dyn FnOnce() + Send>> = r.pending_updates.drain(..).collect();
            drop(r);
            for f in updates {
                // Contain panics per-closure: consumers (overlay pumps, mod
                // callbacks) are written panic-free, but this boundary is the
                // architectural guarantee that a bug in one closure can't
                // unwind into game code or take down the rest of the batch.
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err() {
                    log_warn!("WidgetRenderer: panic in queued render-thread closure — contained");
                }
            }
        }
    }

    // Overlay-draw anchor: emit the animated background mid-walk when this
    // wrapper IS the menu's anchor (identity-gated, panic-contained inside).
    crate::services::overlay_draw::on_anchor_render(this);

    if let Some(ref hook) = WRAPPER_HOOK {
        hook.call(this);
    }

    // Post-original: re-arm the anchor's dirty flag (the game's render pass
    // clears it — a pre-original write would be clobbered). Keeps the walk
    // dispatching the anchor every frame while the background is active.
    crate::services::overlay_draw::on_anchor_rendered(this);
}

pub fn init(game_module: &GameModule, signatures: &SignatureStore) -> bool {
    {
        let mut r = RENDERER.lock().unwrap();
        r.game_base = game_module.base;
        r.game_size = game_module.size;

        if let Some(addr) = signatures.get_address("widget_factory") {
            r.widget_factory_addr = addr;
        } else {
            log_warn!("WidgetRenderer: widget_factory not resolved");
            return false;
        }

        if let Some(addr) = signatures.get_address("sprite_vtable") {
            r.sprite_vtable_addr = addr;
        }

        // Resolve BmpString vtable via RTTI
        r.bmpstring_vtable = find_bmpstring_vtable(signatures);
        if r.bmpstring_vtable.is_null() {
            log_warn!("WidgetRenderer: could not find agcs::BmpString vtable via RTTI");
            return false;
        }
        log_info!(
            "WidgetRenderer: BmpString vtable @ {:p}",
            r.bmpstring_vtable
        );

        // Find wrapper_constructor and game allocator
        resolve_wrapper_derived(&mut r, game_module, signatures);
        if r.wrapper_constructor_addr.is_null() {
            log_warn!("WidgetRenderer: could not find wrapper_constructor");
        }
        if r.game_alloc_fn.is_null() {
            log_warn!("WidgetRenderer: could not find game allocator");
        }
        if r.scene_manager_global.is_null() {
            log_warn!("WidgetRenderer: could not find scene_manager_global");
        } else {
            // Lock-free mirror for hot-path consumers (the overlay-draw
            // emitter identifies the widget layer by this manager).
            SCENE_MGR_GLOBAL_MIRROR.store(r.scene_manager_global as *mut u8, Ordering::Release);
        }
    }

    // Hook render_function (for font pointer capture only)
    let render_addr = match signatures.get_address("render_function") {
        Some(a) => a,
        None => {
            log_warn!("WidgetRenderer: render_function not resolved");
            return false;
        }
    };

    unsafe {
        let target: RenderFn = std::mem::transmute(render_addr);
        if let Err(e) = crate::core::hooks::install_enabled(
            std::ptr::addr_of_mut!(RENDER_HOOK),
            target,
            render_function_hook,
        ) {
            log_error!("WidgetRenderer: failed to install render hook: {}", e);
            return false;
        }
    }

    // Hook wrapper_render (for pending_updates drain only)
    if let Some(wrapper_addr) = signatures.get_address("wrapper_render") {
        unsafe {
            let target: RenderFn = std::mem::transmute(wrapper_addr);
            if let Err(e) = crate::core::hooks::install_enabled(
                std::ptr::addr_of_mut!(WRAPPER_HOOK),
                target,
                wrapper_render_hook,
            ) {
                log_warn!("WidgetRenderer: wrapper_render hook failed: {}", e);
            }
        }
    }

    log_info!("WidgetRenderer: initialized");
    true
}

type GameAllocFn = unsafe extern "C" fn(*const u8, usize, i32) -> *mut u8;
type WrapperConstructorFn = unsafe extern "C" fn(*mut u8, i32, *const u8) -> *mut u8;

/// Create a text widget and register it in the game's native render list.
pub fn create_text_widget() -> Option<TextWidget> {
    create_text_widget_with_wrapper().map(|(w, _)| w)
}

/// Like [`create_text_widget`], but also returns the WRAPPER address (the
/// `agcs::BmpString` the render walk dispatches `wrapper_render` on). The
/// overlay-draw animated background uses this as its emission-anchor
/// identity: emitting at the anchor's own render puts the quad mid-walk —
/// above everything the widget layer drew earlier (incl. full-screen
/// loading art) and below the menu widgets registered after it.
pub fn create_text_widget_with_wrapper() -> Option<(TextWidget, usize)> {
    let r = RENDERER.lock().unwrap();
    if !r.font_captured
        || r.wrapper_constructor_addr.is_null()
        || r.game_alloc_fn.is_null()
        || r.game_alloc_heap.is_null()
        || r.scene_manager_global.is_null()
    {
        return None;
    }

    let font_ptr = r.font_ptr;
    let ctor_addr = r.wrapper_constructor_addr;
    let alloc_fn_addr = r.game_alloc_fn;
    let alloc_heap_addr = r.game_alloc_heap;
    let scene_mgr_global = r.scene_manager_global;
    drop(r);

    unsafe {
        // Allocate 0x20 bytes for the wrapper using the game's allocator
        let game_alloc: GameAllocFn = std::mem::transmute(alloc_fn_addr);
        let heap = *(alloc_heap_addr as *const *const u8);
        let wrapper = game_alloc(heap, 0x20, 0);
        if wrapper.is_null() {
            log_warn!("WidgetRenderer: game allocator returned null for wrapper");
            return None;
        }

        // Call the wrapper_constructor: FUN_180201E90(buffer, group=0, font_ptr)
        // It allocates child_array, calls widget_factory, patches line_desc callbacks
        let ctor: WrapperConstructorFn = std::mem::transmute(ctor_addr);
        let wrapper = ctor(wrapper, 0, font_ptr);
        if wrapper.is_null() {
            log_warn!("WidgetRenderer: wrapper constructor returned null");
            return None;
        }

        // Get the inner widget from child_array[0]
        let child_array = *(wrapper.add(0x18) as *const *mut u8);
        if child_array.is_null() {
            return None;
        }
        let widget_ptr = *(child_array as *const *mut u8);
        if widget_ptr.is_null() {
            return None;
        }

        let widget = TextWidget::new(widget_ptr);
        widget.set_outline(0.0, 0.0, 0.0, 1.0, 1);

        // Register in the game's render list
        if !register_in_render_list(scene_mgr_global, wrapper) {
            log_warn!("WidgetRenderer: failed to register widget in render list");
        }

        Some((widget, wrapper as usize))
    }
}

/// Register a wrapper in the game's native render list.
unsafe fn register_in_render_list(scene_mgr_global: *const u8, wrapper: *mut u8) -> bool {
    // Step 4: Get render list manager
    let scene_mgr = *(scene_mgr_global as *const *const u8);
    if scene_mgr.is_null() {
        return false;
    }

    let render_list_mgr = *(scene_mgr.add(0xB0) as *const *mut u8);
    if render_list_mgr.is_null() {
        return false;
    }

    // Step 5: Pop a node from the free pool
    // manager layout: [3]=+0x18 free_head, [4]=+0x20 sentinel
    let free_head = *(render_list_mgr.add(0x18) as *const *mut u8);
    let sentinel = *(render_list_mgr.add(0x20) as *const *mut u8);

    if free_head.is_null() || free_head == sentinel {
        // Pool exhausted
        *(render_list_mgr.add(0x18) as *mut *mut u8) = std::ptr::null_mut();
        *(render_list_mgr.add(0x20) as *mut *mut u8) = std::ptr::null_mut();
        log_warn!("WidgetRenderer: render list node pool exhausted");
        return false;
    }

    // Pop: advance free head, detach node
    let next_free = *(free_head.add(0x08) as *const *mut u8);
    *(render_list_mgr.add(0x18) as *mut *mut u8) = next_free;
    *(free_head.add(0x08) as *mut *mut u8) = std::ptr::null_mut();

    // Data area is at node[0]
    let data_area = *(free_head as *const *mut u8);
    if data_area.is_null() {
        return false;
    }

    // Step 6: Initialize the data area
    memory::write_ptr(data_area.add(0x10), wrapper as *const u8); // wrapper copy 1
    memory::write_ptr(data_area.add(0x20), wrapper as *const u8); // wrapper copy 2
    memory::write_u8(data_area.add(0x28), 0); // visibility = visible

    // Increment wrapper ref_count
    let rc = memory::read_u32(wrapper.add(0x08) as *const u8);
    memory::write_u32(wrapper.add(0x08), rc + 1);

    // Step 7: Append to active list tail
    // manager: [5]=+0x28 head, [6]=+0x30 tail, +0x3C count
    let count_ptr = render_list_mgr.add(0x3C) as *mut i32;
    *count_ptr += 1;

    // New node's next = NULL
    memory::write_ptr(data_area.add(0x08), std::ptr::null());

    let tail = *(render_list_mgr.add(0x30) as *const *mut u8);
    if tail.is_null() {
        // List was empty — set head
        *(render_list_mgr.add(0x28) as *mut *mut u8) = data_area;
    } else {
        // Append after current tail
        memory::write_ptr(tail.add(0x08), data_area as *const u8);
    }
    // Update tail
    *(render_list_mgr.add(0x30) as *mut *mut u8) = data_area;

    true
}

/// Create a new image widget (agcs::Sprite) and register in the render list.
///
/// Takes an `ImageWidgetConfig` with all properties. If `texture_name` is set,
/// a background thread is spawned to resolve the texture asynchronously —
/// the sprite starts hidden and is shown once the texture resolves.
pub fn create_image_widget(config: &ImageWidgetConfig) -> Option<ImageWidget> {
    let r = RENDERER.lock().unwrap();
    if r.sprite_vtable_addr.is_null() || r.scene_manager_global.is_null() {
        return None;
    }

    let sprite_vtable = r.sprite_vtable_addr;
    let scene_mgr_global = r.scene_manager_global;
    drop(r);

    unsafe {
        let ptr = memory::alloc_zeroed(0x68);
        if ptr.is_null() {
            return None;
        }

        // Base class fields
        memory::write_ptr(ptr, sprite_vtable);
        memory::write_u32(ptr.add(0x08), 0x7FFFFFFF);
        memory::write_u32(ptr.add(0x10), 0x0100);
        // Start hidden — the resolver or the mod will show it when ready
        memory::write_u8(ptr.add(0x12), 0);

        // Sprite fields from config
        memory::write_i32(ptr.add(0x2C), config.blend_mode);
        memory::write_f32(ptr.add(0x30), config.x);
        memory::write_f32(ptr.add(0x34), config.y);
        memory::write_f32(ptr.add(0x38), config.width);
        memory::write_f32(ptr.add(0x3C), config.height);
        memory::write_f32(ptr.add(0x40), config.scale_x);
        memory::write_f32(ptr.add(0x44), config.scale_y);
        memory::write_f32(ptr.add(0x48), config.anchor_x);
        memory::write_f32(ptr.add(0x4C), config.anchor_y);
        memory::write_f32(ptr.add(0x50), config.rotation);
        memory::write_f32(ptr.add(0x5C), 1.0);
        memory::write_f32(ptr.add(0x60), 1.0);
        memory::write_u32(ptr.add(0x64), config.color);

        // Apply opacity to color's alpha channel (ABGR: alpha is high byte)
        if config.opacity < 1.0 {
            let alpha = (config.opacity.clamp(0.0, 1.0) * 255.0) as u32;
            let color = memory::read_u32(ptr.add(0x64) as *const u8);
            memory::write_u32(ptr.add(0x64), (color & 0x00FFFFFF) | (alpha << 24));
        }

        if !register_in_render_list(scene_mgr_global, ptr) {
            log_warn!("WidgetRenderer: failed to register image widget in render list");
        }

        let widget = ImageWidget::new(ptr);

        // Spawn background thread that schedules texture resolution on the game thread
        if let Some(ref name) = config.texture_name {
            let tex_name = name.clone();
            let native_ptr = ptr as usize; // Send-safe
            std::thread::spawn(move || {
                use crate::services::texture_resolver;
                // Wait until the texture system can resolve names. Stock and
                // LayeredFS-injected textures resolve through BM2D's
                // get_bitmap_info callback once the resolver is available.
                loop {
                    if texture_resolver::is_available() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                log_info!("WidgetRenderer: resolving texture '{}'...", tex_name);
                // Schedule resolve attempts on the game thread indefinitely
                let resolved = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                loop {
                    if resolved.load(std::sync::atomic::Ordering::Acquire) {
                        return;
                    }
                    let name = tex_name.clone();
                    let done = resolved.clone();
                    let np = native_ptr;
                    run_on_render_thread(move || {
                        if done.load(std::sync::atomic::Ordering::Acquire) {
                            return;
                        }
                        if let Some(tex) = texture_resolver::resolve(&name) {
                            let p = np as *mut u8;
                            memory::write_i32(p.add(0x28), tex.texture_id);
                            memory::write_f32(p.add(0x54), tex.uv_left);
                            memory::write_f32(p.add(0x58), tex.uv_top);
                            memory::write_f32(p.add(0x5C), tex.uv_right);
                            memory::write_f32(p.add(0x60), tex.uv_bottom);
                            // Don't force show — let the mod's scene logic control visibility
                            log_info!(
                                "WidgetRenderer: texture '{}' resolved (id={})",
                                name,
                                tex.texture_id
                            );
                            done.store(true, std::sync::atomic::Ordering::Release);
                        }
                    });
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            });
        }

        Some(widget)
    }
}

pub fn is_available() -> bool {
    // Poison-recovered: callable from render-thread paths that must not panic.
    match RENDERER.lock() {
        Ok(g) => g.font_captured,
        Err(poisoned) => poisoned.into_inner().font_captured,
    }
}

/// Queue a closure to run on the game/render thread at the start of the next frame.
pub fn run_on_render_thread(f: impl FnOnce() + Send + 'static) {
    // Poison-recovered: this is called from render-thread pumps (which
    // self-reschedule) and hook callbacks — an unwrap here would panic
    // inside an extern "C" frame if the mutex was ever poisoned.
    let mut r = match RENDERER.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    r.pending_updates.push(Box::new(f));
}

// ── Dynamic address resolution ──────────────────────────────────

/// Find agcs::BmpString vtable via RTTI string search.
fn find_bmpstring_vtable(signatures: &SignatureStore) -> *const u8 {
    signatures
        .find_vtable_by_rtti(".?AVBmpString@agcs@@", "BmpString")
        .unwrap_or(std::ptr::null())
}

/// Find the wrapper_constructor by scanning xrefs to widget_factory,
/// then extract game allocator and scene_manager_global from it.
fn resolve_wrapper_derived(
    r: &mut RendererInner,
    game_module: &GameModule,
    signatures: &SignatureStore,
) {
    let factory_addr = match signatures.get_address("widget_factory") {
        Some(a) => a,
        None => return,
    };
    let base = game_module.base;
    let size = game_module.size;

    unsafe {
        // Find E8 CALL instructions that target widget_factory
        for pc in scan_xrefs_to(base, size, factory_addr) {
            let i = pc.offset_from(base) as usize;

            // Check if the containing function writes the BmpString vtable
            let search_start = i.saturating_sub(256);
            let vtable_val = r.bmpstring_vtable as usize;
            let mut found_vtable_lea = false;
            for j in search_start..i {
                let b = base.add(j);
                if *b == 0x48 && *b.add(1) == 0x8D && *b.add(2) == 0x05 {
                    let lea_target = decode_rip_relative(b.add(3)) as usize;
                    if lea_target == vtable_val {
                        found_vtable_lea = true;
                        break;
                    }
                }
            }
            if !found_vtable_lea {
                continue;
            }

            // Found the wrapper_constructor. Find its start (MOV [RSP+8],RCX prologue).
            let mut ctor_start: *const u8 = std::ptr::null();
            for j in (search_start..i).rev() {
                let b = base.add(j);
                if *b == 0x48 && *b.add(1) == 0x89 && *b.add(2) == 0x4C && *b.add(3) == 0x24 {
                    ctor_start = b;
                    break;
                }
            }
            if ctor_start.is_null() {
                continue;
            }

            r.wrapper_constructor_addr = ctor_start;
            log_info!(
                "WidgetRenderer: wrapper_constructor @ +0x{:X}",
                ctor_start.offset_from(base) as usize
            );

            // Extract game allocator: first MOV RCX,[rip+disp] + CALL pattern
            let ctor_len = i - (ctor_start.offset_from(base) as usize);
            for j in 0..ctor_len.saturating_sub(12) {
                let b = ctor_start.add(j);
                if *b == 0x48 && *b.add(1) == 0x8B && *b.add(2) == 0x0D {
                    let heap_addr = decode_rip_relative(b.add(3));
                    for k in 7..17usize {
                        if j + k < ctor_len && *b.add(k) == 0xE8 {
                            let alloc_fn = decode_call_rel32(b.add(k));
                            if (alloc_fn.offset_from(base) as usize) < size {
                                r.game_alloc_fn = alloc_fn;
                                r.game_alloc_heap = heap_addr;
                                log_info!("WidgetRenderer: game allocator resolved");
                                break;
                            }
                        }
                    }
                    if !r.game_alloc_fn.is_null() {
                        break;
                    }
                }
            }

            resolve_scene_manager_global(r, game_module);
            r.derived_resolved = true;
            return;
        }
    }
}

/// Find scene_manager_global by scanning for MOV reg,[rip+disp] followed
/// by MOV reg,[reg+0xB0] — the pattern used by render list registration code.
fn resolve_scene_manager_global(r: &mut RendererInner, game_module: &GameModule) {
    let base = game_module.base;
    let size = game_module.size;

    unsafe {
        for i in 0..size.saturating_sub(20) {
            let b = base.add(i);
            if *b != 0x48 || *b.add(1) != 0x8B {
                continue;
            }
            let modrm = *b.add(2);
            if !matches!(modrm, 0x05 | 0x0D | 0x15) {
                continue;
            }

            let global_addr = decode_rip_relative(b.add(3));
            if (global_addr.offset_from(base) as usize) >= size {
                continue;
            }

            for j in 0..16usize {
                let c = b.add(7 + j);
                if *c == 0x48 && *c.add(1) == 0x8B && (*c.add(2) & 0xC0) == 0x80 {
                    let inner_disp = (c.add(3) as *const i32).read_unaligned();
                    if inner_disp == 0xB0 {
                        r.scene_manager_global = global_addr;
                        log_info!("WidgetRenderer: scene_manager_global @ {:p}", global_addr);
                        return;
                    }
                }
            }
        }
    }
    log_warn!("WidgetRenderer: could not resolve scene_manager_global");
}
