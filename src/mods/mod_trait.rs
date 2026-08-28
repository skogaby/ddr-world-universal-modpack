//! Mod trait, ModContext, and ModRegistry — the core mod system.
//!
//! Every mod implements the `Mod` trait, which defines its identity, lifecycle,
//! and required game signatures. The `ModRegistry` manages registration, enable/disable
//! state, and config integration.
//!
//! ## Mod lifecycle
//!
//! 1. `new()` — Construct the mod (no game state available yet)
//! 2. `init(ctx)` — Called once at registration. Access signatures and game module.
//!    Return `false` to abort registration.
//! 3. `enable()` — Activate the mod. Create widgets, install hooks, subscribe to events.
//! 4. `disable()` — Deactivate. Clean up widgets and state. Hooks are auto-removed
//!    by the registry.
//!
//! ## Example
//!
//! ```rust
//! impl Mod for MyMod {
//!     fn id(&self) -> &str { "my-mod" }
//!     fn name(&self) -> &str { "My Mod" }
//!     fn description(&self) -> &str { "Does something cool" }
//!     fn required_signatures(&self) -> &[&str] { &["some_function"] }
//!     fn init(&mut self, ctx: &ModContext) -> bool { true }
//!     fn enable(&mut self) { /* create widgets, hooks */ }
//!     fn disable(&mut self) { /* cleanup */ }
//! }
//! ```

use crate::core::hooks::HookManager;
use crate::core::module_resolver::GameModule;
use crate::core::profiling;
use crate::core::signatures::SignatureStore;
use crate::{log_info, log_warn};
use std::collections::HashMap;

/// Mods whose `enable()` does substantial late-binding-tolerant work
/// (filesystem I/O, asset generation) that doesn't need to complete before
/// the game's first frame. `enable_with_config` enables every other mod
/// first so faster hooks land sooner; these enable last.
const LATE_BINDING_MODS: &[&str] = &["folder-expansion", "webui-options"];

/// Mods that default OFF when absent from the config `mods` map (every
/// other mod defaults ON). Reserved for hardware-specific mods that are
/// meaningless — or actively wrong — on cabinets without that hardware.
const DEFAULT_OFF_MODS: &[&str] = &["smx-hardware"];

/// Context passed to mods during initialization. Provides read-only access
/// to the game module (base address, size) and resolved function signatures.
pub struct ModContext<'a> {
    pub game_module: &'a GameModule,
    pub signatures: &'a SignatureStore,
}

/// Context passed to `Mod::early_apply`. A subset of `ModContext` — derived
/// signatures are not yet resolved at this phase, so `signatures` here only
/// contains the linear AOB hits from `resolve_all`. Mods invoked via
/// `early_apply` should rely on top-level signatures or perform their own
/// ad-hoc scans.
pub struct EarlyContext<'a> {
    pub game_module: &'a GameModule,
    pub signatures: &'a SignatureStore,
}

/// The trait that all mods must implement. Defines identity, lifecycle hooks,
/// and signature requirements.
pub trait Mod: Send {
    /// Unique identifier for this mod (e.g., "fast-bootup"). Used in config files.
    fn id(&self) -> &str;
    /// Human-readable display name (e.g., "Fast Bootup"). Shown in the mod menu.
    fn name(&self) -> &str;
    /// Short description of what the mod does. Shown in the mod menu.
    fn description(&self) -> &str;
    /// List of signature names this mod requires. If any are missing, the mod
    /// won't be registered. Return `&[]` if no signatures are needed.
    fn required_signatures(&self) -> &[&str];
    /// Called once during registration. Use `ctx` to read game addresses.
    /// Return `false` to abort registration (e.g., if a required address is wrong).
    fn init(&mut self, ctx: &ModContext) -> bool;
    /// Activate the mod. Create widgets, install hooks, subscribe to scene/input events.
    fn enable(&mut self);
    /// Deactivate the mod. Destroy widgets, clean up state. Hooks registered via
    /// the mod's HookManager are automatically removed by the registry.
    fn disable(&mut self);

    /// Reports whether the mod actually activated after `enable()` ran. The
    /// registry calls this right after `enable()` and records the result as the
    /// mod's enabled state (instead of assuming `enable()` always succeeded).
    ///
    /// A mod that **self-disables internally** — e.g. a load-bearing signature
    /// didn't resolve, so `enable()` logged a warning and returned without
    /// installing anything — should override this to return `false`. Then the
    /// registry (and the mod-menu, which renders `entry.enabled`) shows the mod
    /// as OFF rather than falsely ON over an inert mod. This matters most for a
    /// mod that contributes child overlay rows gated on its master toggle: a
    /// false `[ON]` master would otherwise imply config that never applies.
    ///
    /// Default: `true` (the mod is active whenever it was enabled) — so mods
    /// that don't self-disable need not implement this.
    fn is_active(&self) -> bool {
        true
    }

    /// Optional. Called once at init-time, after `resolve_all` but before
    /// `resolve_derived` and service init. Use for setup that must land
    /// before the game touches a particular code path (e.g. patching a
    /// buffer size before the game's first XML parse).
    ///
    /// Mods that implement `early_apply` are still expected to implement
    /// `init` and `enable` normally; those will run later in init after
    /// services come up. The mod is responsible for tracking what
    /// `early_apply` already did and making `init`/`enable` no-op on the
    /// duplicated work, so that the mod-menu's runtime toggle path still
    /// works.
    ///
    /// Default: returns `true` (no-op success). Returning `false` logs a
    /// warning but does not abort init.
    fn early_apply(&mut self, _ctx: &EarlyContext) -> bool {
        true
    }
}

struct ModEntry {
    mod_impl: Box<dyn Mod>,
    enabled: bool,
    hooks: HookManager,
}

pub struct ModRegistry {
    mods: Vec<ModEntry>,
}

impl ModRegistry {
    pub fn new() -> Self {
        Self { mods: Vec::new() }
    }

    pub fn register(&mut self, mut mod_impl: Box<dyn Mod>, ctx: &ModContext) {
        let missing: Vec<&str> = mod_impl
            .required_signatures()
            .iter()
            .filter(|name| ctx.signatures.get_address(name).is_none())
            .copied()
            .collect();

        if !missing.is_empty() {
            log_warn!(
                "Mod '{}' skipped -- missing signatures: {}",
                mod_impl.name(),
                missing.join(", ")
            );
            return;
        }

        let ok = mod_impl.init(ctx);
        if !ok {
            log_warn!("Mod '{}' failed to initialize", mod_impl.name());
            return;
        }

        log_info!("Mod registered: {} ({})", mod_impl.name(), mod_impl.id());
        self.mods.push(ModEntry {
            mod_impl,
            enabled: false,
            hooks: HookManager::new(),
        });
    }

    pub fn enable(&mut self, id: &str) {
        if let Some(entry) = self.mods.iter_mut().find(|e| e.mod_impl.id() == id) {
            if entry.enabled {
                return;
            }
            entry.mod_impl.enable();
            // Honor an internal self-disable: a mod whose load-bearing setup
            // failed reports `is_active() == false`, so we record it as NOT
            // enabled rather than showing a false ON over an inert mod (the
            // mod-menu renders `enabled`). Default `is_active()` is `true`, so
            // mods that don't self-disable are unaffected.
            entry.enabled = entry.mod_impl.is_active();
            if entry.enabled {
                log_info!("Mod enabled: {}", entry.mod_impl.name());
            } else {
                log_info!(
                    "Mod '{}' enabled but self-disabled (inactive) -- recorded as off",
                    entry.mod_impl.name()
                );
            }
            profiling::tick(&format!("enable/{}", id));
        }
    }

    pub fn disable(&mut self, id: &str) {
        if let Some(entry) = self.mods.iter_mut().find(|e| e.mod_impl.id() == id) {
            if !entry.enabled {
                return;
            }
            entry.mod_impl.disable();
            entry.hooks.remove_all();
            entry.enabled = false;
            log_info!("Mod disabled: {}", entry.mod_impl.name());
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        self.mods
            .iter()
            .find(|e| e.mod_impl.id() == id)
            .is_some_and(|e| e.enabled)
    }

    pub fn get_entries(&self) -> Vec<ModInfo> {
        self.mods
            .iter()
            .map(|e| ModInfo {
                id: e.mod_impl.id().to_string(),
                name: e.mod_impl.name().to_string(),
                description: e.mod_impl.description().to_string(),
                enabled: e.enabled,
            })
            .collect()
    }

    pub fn enable_with_config(&mut self, config: &HashMap<String, bool>) {
        let ids: Vec<String> = self
            .mods
            .iter()
            .map(|e| e.mod_impl.id().to_string())
            .collect();

        // Partition the ids: late-binding-tolerant mods do substantial
        // disk I/O / asset generation work in their enable(), but their
        // hooks fire on player navigation (song-select / options menu),
        // not boot. Defer them to after fast mods so quick hooks land
        // sooner.
        let (fast, late): (Vec<String>, Vec<String>) = ids
            .into_iter()
            .partition(|id| !LATE_BINDING_MODS.contains(&id.as_str()));

        for id in fast.into_iter().chain(late.into_iter()) {
            if id == "mod-menu" {
                continue;
            }
            let default_on = !DEFAULT_OFF_MODS.contains(&id.as_str());
            let should_enable = config.get(&id).copied().unwrap_or(default_on);
            if should_enable {
                self.enable(&id);
            }
        }
    }

    pub fn enabled_count(&self) -> usize {
        self.mods.iter().filter(|e| e.enabled).count()
    }
}

#[derive(Clone)]
pub struct ModInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
}
