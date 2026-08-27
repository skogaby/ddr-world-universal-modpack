//! Non-Native OS Support — Wine/CrossOver-only workaround(s).
//!
//! DDR World assumes a real Windows cabinet. Under CrossOver/Wine (macOS/Linux)
//! some of those assumptions break; this mod bundles the workarounds that must live
//! in-process. It currently contains a single sub-fix (the background-movie crash
//! stub); the scaffolding (best-effort resolve in `init`, independent self-disable,
//! `is_active()` = union of installed hooks) is kept so further OS workarounds can be
//! added the same way.
//!
//! ## Background-movie DirectShow graph stub (movie-crash fix)
//!
//! Background movies / music videos are `.wmv` files played through a Windows
//! **DirectShow** filter graph (`quartz.dll`). Under Wine two independent
//! failure modes exist, and the mod handles both:
//!
//! - **Crash** (spice2x audio hooks enabled): spice2x IAT-patches
//!   `CoCreateInstance` process-wide and wraps `MMDeviceEnumerator` /
//!   `IAudioClient`; Wine's builtin `winmm` consumes those wrappers internally
//!   while `devenum` enumerates audio renderers during
//!   `IGraphBuilder::RenderFile`'s intelligent-connect, and faults
//!   (`quartz` → `devenum` → `winmm` access-violation) the moment a
//!   movie-backed song starts — including autonomously in the attract-mode
//!   demo loop. Running spice2x with `-audiohookdisable` removes the crash at
//!   the source (verified live 2026-08-19; the game's own audio is WASAPI and
//!   unaffected).
//! - **Decode failure → soft-lock** (no crash, e.g. with `-audiohookdisable`):
//!   CrossOver's GStreamer stack has no VC-1 decoder (VideoToolbox doesn't do
//!   VC-1), so `RenderFile` on stock movies fails with `VFW_E_CANNOT_RENDER`
//!   (0x80040218); `BuildGraph`'s error path never writes player state 3 and
//!   the song waits forever on the movie-ready gate. Movies transcoded to
//!   H.264 (any container; VideoToolbox decodes them) render fine.
//!
//! The shared `services::movie_policy` service owns the sole detour on gamemdx
//! `DShowPlayer::BuildGraph` (AOB signature `movie_build_graph` — the **only**
//! user of `CLSID_FilterGraph` in the binary;
//! Ghidra 0x18023AE40 on 20260616 / 0x180256EB0 on 20260324). Two modes,
//! selected by `non_native_os_support.movie_mode` in mod-config.json (read at
//! enable):
//!
//! - `"suppress"` (default): the detour **never calls the original** —
//!   `CoCreateInstance`/`RenderFile` never run — and instead fakes the success
//!   epilogue's one observable side effect: it writes player state (+0x8) = 3
//!   ("opened") and returns 0. Crash-safe under every spice2x configuration;
//!   all movies absent.
//! - `"fallback"`: the detour calls the original first; a SUCCEEDED build
//!   plays normally, a FAILED one gets the same faked epilogue (no movie, no
//!   stall). This lets converted H.264 movies play while unconverted VC-1
//!   files degrade gracefully — conversion can proceed incrementally.
//!   Requires `-audiohookdisable` under Wine (the crash path above is the
//!   original stub's raison d'être and still exists with audio hooks on).
//!   Fallback mode also installs `services::mfplat_vih_fix` (Wine-gated):
//!   with the native Windows Media runtime installed in the bottle
//!   (qasf/wmvcore/wmasf/wmvdecod/wmadmod — see
//!   `docs/native_wm_runtime_bottle_setup.md`), stock VC-1 movies decode
//!   natively once Wine mfplat's `MFInitMediaTypeFromVideoInfoHeader`
//!   FOURCC-subtype bug is worked around; without the runtime the fix is
//!   inert and VC-1 keeps degrading to no-movie.
//!
//! In both modes the state-3 write is load-bearing: the `Dx9Movie::update`
//! status machine only advances past "opening" when `getState()` reads 3, and
//! the demo/gameplay sequences poll that status before starting the song (a
//! plain error-returning stub soft-locks the attract demo). The `opened` byte
//! (+0x14) stays 0, so the per-frame get-frame path early-returns before
//! touching any (null) COM pointer — the movie "plays" silently delivering no
//! frames. RE record: `.agents/planning/20260721-non-native-os-support/`.
//!
//! ## Removed: network-status / EACoin(PASELI) online fixes
//!
//! Earlier revisions also carried two **networking** sub-fixes for CrossOver — an
//! `arkGetNetworkStatus` CHECKING→ONLINE promotion (boot online) and a libavs
//! `ea3_get_status` DOWN→ONLINE promotion (PASELI availability). Both worked around
//! the same root cause: Wine can't create the raw ICMP socket AVS keepalive needs.
//! spice2x's **`-icmphook`** flag now fakes that keepalive game-agnostically at the
//! socket layer, so DDR World boots fully online — PASELI included — with no hook
//! DLL injected. The in-process promotions are therefore redundant and have been
//! removed; use `-icmphook` instead. Their RE records are retained (marked
//! superseded) at `.agents/planning/20260721-raw-socket-network-fix/` (network) and
//! `.agents/planning/20260722-eacoin-paseli-online-cascade/` (PASELI).
//!
//! ## How it degrades
//!
//! The shared service resolves/installs best-effort at boot. If unavailable, this
//! mod logs a warning and stays inert; `is_active()` reports true only while its
//! own Non-Native OS contributor is set. Config-gated
//! like any other mod (default ON via the pack's omitted-key-enables convention;
//! disable via `"non-native-operating-system-support": false` or the mod menu). The
//! movie stub is the one behavioral trade-off — suppress mode makes backgrounds
//! static — so operators on real hardware should leave the mod off to keep their
//! music videos.

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
