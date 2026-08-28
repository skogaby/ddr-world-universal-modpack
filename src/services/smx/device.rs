//! SMX HID device discovery + connection (the impure edge of the SMX
//! transport). Port of `SMXDeviceSearch.cpp`'s enumeration: walk the HID
//! interface class via SetupAPI, open each path, and keep only devices with
//! VID `0x2341` / PID `0x8037` whose product string is `"StepManiaX"`
//! (stage controller) or `"SMXArcade"` (cabinet lights controller).
//!
//! All handles are opened `FILE_FLAG_OVERLAPPED`; the transport thread owns
//! the async read/write pumps. Every function here is best-effort: failures
//! return `None`/empty rather than propagating (unrelated HID devices failing
//! to open is completely normal).

use windows::core::PCWSTR;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces, SetupDiGetClassDevsW,
    SetupDiGetDeviceInterfaceDetailW, DIGCF_DEVICEINTERFACE, DIGCF_PRESENT,
    SP_DEVICE_INTERFACE_DATA, SP_DEVICE_INTERFACE_DETAIL_DATA_W,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_GetAttributes, HidD_GetHidGuid, HidD_GetProductString, HidD_SetNumInputBuffers,
    HIDD_ATTRIBUTES,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES, FILE_FLAG_OVERLAPPED,
    FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};

pub const SMX_VID: u16 = 0x2341;
pub const SMX_PID: u16 = 0x8037;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    /// A stage (pad) controller — product string "StepManiaX".
    Stage,
    /// The dedicated-cabinet lights controller — product string "SMXArcade".
    Cabinet,
}

/// Enumerate every present HID interface path (wide strings, NUL-terminated).
pub fn enumerate_hid_paths() -> Vec<Vec<u16>> {
    let mut paths = Vec::new();
    unsafe {
        let hid_guid = HidD_GetHidGuid();

        let Ok(dev_info) = SetupDiGetClassDevsW(
            Some(&hid_guid),
            PCWSTR::null(),
            None,
            DIGCF_DEVICEINTERFACE | DIGCF_PRESENT,
        ) else {
            return paths;
        };

        let mut index = 0u32;
        loop {
            let mut iface = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInterfaces(dev_info, None, &hid_guid, index, &mut iface).is_err() {
                break; // ERROR_NO_MORE_ITEMS (or a real failure — either way stop)
            }
            index += 1;

            // First call gets the required size (fails with INSUFFICIENT_BUFFER).
            let mut required = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(
                dev_info,
                &iface,
                None,
                0,
                Some(&mut required),
                None,
            );
            if required == 0 || required > 4096 {
                continue;
            }

            // Variable-size struct: allocate a raw buffer, set the STRUCT's
            // cbSize (not the buffer size), then read DevicePath (offset 4).
            let mut buf = vec![0u8; required as usize];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(
                dev_info,
                &iface,
                Some(detail),
                required,
                None,
                None,
            )
            .is_err()
            {
                continue;
            }

            // DevicePath is a NUL-terminated wide string at struct offset 4.
            let path_ptr = std::ptr::addr_of!((*detail).DevicePath) as *const u16;
            let max_chars = (required as usize - 4) / 2;
            let mut path = Vec::new();
            for i in 0..max_chars {
                let ch = *path_ptr.add(i);
                path.push(ch);
                if ch == 0 {
                    break;
                }
            }
            if path.last() == Some(&0) {
                paths.push(path);
            }
        }

        let _ = SetupDiDestroyDeviceInfoList(dev_info);
    }
    paths
}

/// Try to open `path` as an SMX device. Returns the overlapped-mode handle
/// and the device kind, or `None` if the path isn't an SMX device (or won't
/// open — normal for unrelated devices).
pub fn try_open_smx(path: &[u16]) -> Option<(HANDLE, DeviceKind)> {
    unsafe {
        let handle = CreateFileW(
            PCWSTR(path.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(FILE_ATTRIBUTE_NORMAL.0 | FILE_FLAG_OVERLAPPED.0),
            None,
        )
        .ok()?;

        // VID/PID gate.
        let mut attrs = HIDD_ATTRIBUTES {
            Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32,
            ..Default::default()
        };
        if !HidD_GetAttributes(handle, &mut attrs).as_bool()
            || attrs.VendorID != SMX_VID
            || attrs.ProductID != SMX_PID
        {
            let _ = CloseHandle(handle);
            return None;
        }

        // The VID/PID are stock Arduino ids — confirm via the product string.
        // SUBSTRING match, not the SDK's exact match: Wine/CrossOver's
        // hidclass composes the product string with manufacturer fragments
        // (cabinet-caught 2026-08-27: "Revolution StepManiaX" /
        // "Step Re SMXArcade" instead of Windows' exact "StepManiaX" /
        // "SMXArcade"). The VID/PID gate plus substring keeps the original
        // intent (reject non-SMX Arduinos) on both platforms.
        let mut product = [0u16; 255];
        if !HidD_GetProductString(
            handle,
            product.as_mut_ptr() as *mut _,
            (product.len() * 2) as u32,
        )
        .as_bool()
        {
            let _ = CloseHandle(handle);
            return None;
        }
        let len = product.iter().position(|&c| c == 0).unwrap_or(0);
        let name = String::from_utf16_lossy(&product[..len]);
        let kind = if name.contains("SMXArcade") {
            DeviceKind::Cabinet
        } else if name.contains("StepManiaX") {
            DeviceKind::Stage
        } else {
            crate::log_warn!(
                "SMX: VID/PID match with unrecognized product string {:?} -- ignoring device",
                name
            );
            let _ = CloseHandle(handle);
            return None;
        };

        // Deepen the input-report ring so bursts aren't dropped (SDK uses 512).
        let _ = HidD_SetNumInputBuffers(handle, 512);

        Some((handle, kind))
    }
}

/// Close a device handle (best-effort).
pub fn close_device(handle: HANDLE) {
    unsafe {
        let _ = CloseHandle(handle);
    }
}
