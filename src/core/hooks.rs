//! Hook Manager — Managed function hooking via the retour crate.
//!
//! Wraps retour::GenericDetour with a name-based registry for clean
//! enable/disable lifecycle.

use retour::GenericDetour;
use std::any::Any;
use std::collections::HashMap;

use crate::log_debug;

/// Create a detour, store its handle into `storage`, and only then enable it.
///
/// The order is load-bearing: the moment `enable()` patches the target's
/// prologue, ANY thread — game render loop, AVS boot workers, Win32 callers —
/// can land in our callback, and every callback reads its handle back out of
/// its `static mut` slot to reach the trampoline. Enable-then-store leaves a
/// window where the callback runs while the slot is still `None`; that race
/// was a real non-deterministic boot abort (see learnings.md). Storing first
/// closes it: on x86_64 the store to `storage` precedes the patch write in
/// program order (TSO), so a thread that can see the patched prologue can
/// also see the populated slot.
///
/// On enable failure the slot is cleared again and the error returned, so a
/// `false`/degraded init path never leaves a half-installed hook behind.
///
/// # Safety
/// `storage` must be valid for the process lifetime (a `static mut`), and
/// nothing else may be mutating it concurrently during install.
pub unsafe fn install_enabled<F: retour::Function>(
    storage: *mut Option<GenericDetour<F>>,
    target: F,
    callback: F,
) -> Result<(), retour::Error> {
    *storage = Some(GenericDetour::new(target, callback)?);
    let enable_result = (*storage).as_ref().map(|h| h.enable());
    if let Some(Err(e)) = enable_result {
        *storage = None;
        return Err(e);
    }
    Ok(())
}

pub struct HookManager {
    hooks: HashMap<String, Box<dyn Any + Send>>,
}

impl HookManager {
    pub fn new() -> Self {
        Self {
            hooks: HashMap::new(),
        }
    }

    /// Store a detour handle by name. The detour must already be enabled.
    pub fn store<T: retour::Function + 'static>(&mut self, name: &str, detour: GenericDetour<T>) {
        log_debug!("Hook stored: {}", name);
        self.hooks.insert(name.to_string(), Box::new(detour));
    }

    /// Remove and drop a detour by name (disables the hook).
    pub fn remove(&mut self, name: &str) {
        if self.hooks.remove(name).is_some() {
            log_debug!("Hook removed: {}", name);
        }
    }

    /// Remove all hooks.
    pub fn remove_all(&mut self) {
        for name in self.hooks.keys().cloned().collect::<Vec<_>>() {
            log_debug!("Hook removed: {}", name);
        }
        self.hooks.clear();
    }

    pub fn count(&self) -> usize {
        self.hooks.len()
    }
}
