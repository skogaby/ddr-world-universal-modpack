//! Non-Native OS Support — the CrossOver/Wine background-movie workaround.
//!
//! Background movies / music videos are `.wmv` files played through a
//! DirectShow filter graph (`quartz.dll`). Under Wine two independent
//! failure modes exist:
//!
//! - **Crash** (spice2x audio hooks on): spice2x IAT-patches
//!   `CoCreateInstance` process-wide and wraps `MMDeviceEnumerator` /
//!   `IAudioClient`; Wine's builtin `winmm` consumes those wrappers while
//!   `devenum` enumerates audio renderers during `RenderFile`, and faults the
//!   moment a movie-backed song starts (attract demo included). spice2x
//!   `-audiohookdisable` removes it at the source (the game's own audio is
//!   WASAPI and unaffected).
//! - **Decode failure → soft-lock**: Wine's GStreamer stack has no VC-1
//!   decoder, so `RenderFile` on stock movies fails; `BuildGraph`'s error
//!   path never writes player state 3 and the song waits forever on the
//!   movie-ready gate. H.264 transcodes (`scripts/convert_movies.sh`) render.
//!
//! This mod is a contributor to `services::movie_policy`, which owns the sole
//! detour on gamemdx `DShowPlayer::BuildGraph` (AOB `movie_build_graph`).
//! `enable()` sets `MovieSuppressor::NonNativeOs` with the mode read from
//! `non_native_os_support.movie_mode` (operator-only config, never written;
//! read at enable, unknown values WARN and mean `suppress`):
//!
//! - `"suppress"` (default): the original never runs; the hook fakes the
//!   success epilogue's one load-bearing side effect (player state `+0x8` =
//!   3, "opened") and returns 0. Crash-safe under every spice2x
//!   configuration; all movies absent.
//! - `"fallback"`: the original runs first (on a path absolutized for Wine's
//!   source-filter probe) and only a FAILED build gets the faked epilogue —
//!   playable movies play, unplayable ones degrade to no-movie. Requires
//!   spice2x `-audiohookdisable` under Wine. Fallback also installs
//!   `services::mfplat_vih_fix` (itself Wine-gated, fail-open), which works
//!   around Wine mfplat's `MFInitMediaTypeFromVideoInfoHeader` FOURCC-subtype
//!   bug so stock VC-1 decodes natively once Microsoft's WM runtime is set up
//!   in the bottle — recipe and root-cause trail in
//!   `docs/native_wm_runtime_bottle_setup.md`. Without the runtime VC-1 keeps
//!   degrading to no-movie.
//!
//! Full suppression by another contributor (`SongRate`, `BackgroundDancers`)
//! always wins over fallback. The state-3 write is load-bearing (an
//! error-returning stub soft-locks the attract demo); the `opened` byte
//! (`+0x14`) stays 0 so per-frame code takes its guarded early return.
//!
//! **Not Wine-gated.** The mod defaults ON (not in `DEFAULT_OFF_MODS`) and
//! does not consult `platform::running_under_wine` — only `mfplat_vih_fix`
//! does. On native Windows the default `suppress` mode therefore removes
//! every background movie; `fallback` is near-stock there (real builds
//! succeed). Real-hardware operators who want movies disable the mod
//! (`"non-native-operating-system-support": false` or the mod menu).
//!
//! Degradation: without the shared movie hook the mod logs one WARN and stays
//! inert; `is_active()` = hook available ∧ this contributor set. The former
//! networking sub-fixes (online / PASELI status promotions) are gone —
//! spice2x `-icmphook` fakes the AVS keepalive Wine could not send.

use crate::mods::config;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::mfplat_vih_fix;
use crate::services::movie_policy::{self, MovieSuppressor};
use crate::{log_info, log_warn};

pub struct NonNativeOsSupportMod;

impl NonNativeOsSupportMod {
    pub fn new() -> Self {
        Self
    }

    /// Reads `non_native_os_support.movie_mode` — `true` = fallback mode.
    /// Absent section/key or `"suppress"` = suppress; unknown values warn
    /// once and fall back to suppress (the crash-safe default).
    fn fallback_mode_configured() -> bool {
        let mode = config::get()
            .and_then(|c| c.non_native_os_support.as_ref())
            .and_then(|c| c.movie_mode.as_deref());
        match mode {
            None | Some("suppress") => false,
            Some("fallback") => true,
            Some(other) => {
                log_warn!(
                    "NonNativeOsSupport: unknown movie_mode {:?} -- using \"suppress\"",
                    other
                );
                false
            }
        }
    }
}

impl Mod for NonNativeOsSupportMod {
    fn id(&self) -> &str {
        "non-native-operating-system-support"
    }

    fn name(&self) -> &str {
        "Non-Native OS Support"
    }

    fn description(&self) -> &str {
        "Wine/CrossOver workaround: prevents background-movie DirectShow crashes and stalls (movie_mode: suppress | fallback)"
    }

    fn required_signatures(&self) -> &[&str] {
        // Deliberately empty (lenient): the movie target resolves best-effort in
        // `init` and the mod self-disables in `enable` if it's missing, so a
        // signature drift degrades gracefully instead of skipping registration.
        &[]
    }

    fn init(&mut self, _ctx: &ModContext) -> bool {
        true
    }

    fn enable(&mut self) {
        if movie_policy::is_available() {
            let fallback = Self::fallback_mode_configured();
            movie_policy::set_non_native_fallback(fallback);
            movie_policy::set_suppressed(MovieSuppressor::NonNativeOs, true);
            log_info!(
                "NonNativeOsSupport: movie contributor enabled (mode: {})",
                if fallback {
                    "fallback -- real graph build first, fake success on failure"
                } else {
                    "suppress -- graph build never runs"
                }
            );
            // Fallback mode runs the real DirectShow graph build; when the
            // bottle carries the native Windows Media runtime, VC-1 decode
            // additionally needs the Wine mfplat FOURCC-subtype fix (see
            // services::mfplat_vih_fix). Wine-gated + idempotent + fail-open;
            // without it, unconverted VC-1 movies keep degrading to no-movie.
            //
            // NOTE: `services::ntdll_state_shim` (the quartz IAT patch) is
            // deliberately NOT installed — the native-quartz bottle
            // experiment it supported was abandoned 2026-08-21 (native
            // quartz hard-locks in its VMR x wined3d path; see
            // docs/native_wm_runtime_bottle_setup.md §2.9). The module is
            // retained uncalled as the proven LdrRegisterDllNotification
            // IAT-patch pattern.
            if fallback {
                mfplat_vih_fix::install();
            }
        } else {
            log_warn!("NonNativeOsSupport: shared movie policy unavailable -- mod self-disabled");
        }
    }

    fn disable(&mut self) {
        movie_policy::set_suppressed(MovieSuppressor::NonNativeOs, false);
        movie_policy::set_non_native_fallback(false);
        log_info!("NonNativeOsSupport: disabled (background-movie playback restored)");
    }

    /// Active iff the detour is installed. `enable()` self-disables (installs
    /// nothing) when the target didn't resolve, so reporting this keeps the
    /// registry/mod-menu from showing a false `[ON]` over an inert mod.
    fn is_active(&self) -> bool {
        movie_policy::is_available() && movie_policy::is_suppressed(MovieSuppressor::NonNativeOs)
    }
}
