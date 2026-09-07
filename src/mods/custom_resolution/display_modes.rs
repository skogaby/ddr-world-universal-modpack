//! Display-mode fail-safe (design §4.10 / R10). A back-buffer size the
//! display cannot present means a black screen with no way into the menu to
//! change the setting, so before any patch lands the requested output is
//! checked against what the desktop can do:
//!
//! - accepted outright in WINDOWED mode (spice2x's `-w` on the process
//!   command line — a window may be larger than the desktop; the CrossOver
//!   dev loop runs a 1512×982 logical desktop and still wants 1080p windows)
//!   or when it fits INSIDE the current desktop mode;
//! - otherwise (fullscreen) it must be an enumerable display mode
//!   (`EnumDisplaySettingsW`), matching the refresh rate too when FPS Unlock
//!   asks for a non-60 target.
//!
//! Deliberately permissive: the goal is to catch the "typed 3840x2160 on a
//! 1080p panel in fullscreen" class of mistakes, not to second-guess the
//! driver.

use super::plan::Dims;

/// True when the process command line carries spice2x's windowed flag (`-w`).
#[cfg(windows)]
pub fn spice_windowed() -> bool {
    use windows::Win32::System::Environment::GetCommandLineW;
    let cmd = unsafe { GetCommandLineW() };
    let s = unsafe { cmd.to_string() }.unwrap_or_default();
    s.split_whitespace().any(|t| t.eq_ignore_ascii_case("-w"))
}
#[cfg(not(windows))]
pub fn spice_windowed() -> bool {
    false
}

/// Result of the check — `Err` carries the WARN text.
pub fn validate(output: Dims, fps_hint: Option<u32>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::Win32::Graphics::Gdi::{
            EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE,
        };

        if spice_windowed() {
            return Ok(());
        }

        let mut dm = DEVMODEW {
            dmSize: std::mem::size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        let have_current =
            unsafe { EnumDisplaySettingsW(None, ENUM_CURRENT_SETTINGS, &mut dm) }.as_bool();
        if have_current && output.w <= dm.dmPelsWidth && output.h <= dm.dmPelsHeight {
            return Ok(());
        }
        let desktop = if have_current {
            format!("{}x{}", dm.dmPelsWidth, dm.dmPelsHeight)
        } else {
            "unknown".to_string()
        };

        let mut i = 0u32;
        let mut seen_size = false;
        loop {
            let mut m = DEVMODEW {
                dmSize: std::mem::size_of::<DEVMODEW>() as u16,
                ..Default::default()
            };
            let ok = unsafe { EnumDisplaySettingsW(None, ENUM_DISPLAY_SETTINGS_MODE(i), &mut m) }
                .as_bool();
            if !ok {
                break;
            }
            i += 1;
            if m.dmPelsWidth == output.w && m.dmPelsHeight == output.h {
                seen_size = true;
                match fps_hint {
                    Some(hz) if hz != 60 && m.dmDisplayFrequency != hz => continue,
                    _ => return Ok(()),
                }
            }
        }
        if seen_size {
            Err(format!(
                "{}x{} is a display mode but not at {} Hz (FPS Unlock target); desktop is {}",
                output.w,
                output.h,
                fps_hint.unwrap_or(60),
                desktop
            ))
        } else {
            Err(format!(
                "{}x{} is larger than the desktop ({}) and not an enumerable display mode",
                output.w, output.h, desktop
            ))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (output, fps_hint);
        Ok(())
    }
}
