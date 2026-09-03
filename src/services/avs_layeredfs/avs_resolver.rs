//! AVS DLL export resolution — finds libavs-win64.dll and resolves function pointers
//! across 6 AVS version export tables.

use std::ffi::CString;
use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

// ── FFI types matching AVS C signatures ──────────────────────────────

/// AVS file handle (int32_t in C).
pub type AvsFile = i32;

/// AVS file reader callback — same signature as avs_fs_read.
pub type AvsReaderFn = unsafe extern "C" fn(AvsFile, *mut u8, usize) -> usize;

/// AVS stat structure (packed, matches C layout).
#[repr(C, packed)]
#[derive(Default, Clone, Copy)]
pub struct AvsStat {
    pub st_ctime: i64,
    pub st_mtime: i64,
    pub st_atime: i64,
    pub link_count: i32,
    pub filesize: u32,
    pub hi_filesize: u32,
    pub mode: u16,
    pub perm: u16,
}

/// AVS compression stream (packed, matches C layout).
#[repr(C, packed)]
pub struct CStream {
    pub output_buffer: *mut u8,
    pub input_buffer: *mut u8,
    pub output_size: u32,
    pub input_size: u32,
}

/// Opaque property handle.
pub type PropertyT = *mut u8;
/// Opaque node handle.
pub type NodeT = *mut u8;

// ── Function pointer types ───────────────────────────────────────────

pub type FnAvsFsOpen = unsafe extern "C" fn(*const i8, u16, i32) -> AvsFile;
pub type FnAvsFsClose = unsafe extern "C" fn(AvsFile);
pub type FnAvsFsRead = unsafe extern "C" fn(AvsFile, *mut u8, usize) -> usize;
pub type FnAvsFsLstat = unsafe extern "C" fn(*const i8, *mut AvsStat) -> i32;
pub type FnAvsFsFstat = unsafe extern "C" fn(AvsFile, *mut AvsStat) -> i32;
pub type FnAvsFsLseek = unsafe extern "C" fn(AvsFile, i32, i32) -> i32;
pub type FnAvsFsMount = unsafe extern "C" fn(*const i8, *const i8, *const i8, *const i8) -> i32;
pub type FnAvsFsConvertPath = unsafe extern "C" fn(*mut i8, *const i8) -> i32;

pub type FnPropertyReadQueryMemsize =
    unsafe extern "C" fn(AvsReaderFn, AvsFile, *mut i32, *mut i32) -> i32;
pub type FnPropertyReadQueryMemsizeLong =
    unsafe extern "C" fn(AvsReaderFn, AvsFile, *mut i32, *mut i32, *mut i32) -> i32;
pub type FnPropertyCreate = unsafe extern "C" fn(i32, *mut u8, u32) -> PropertyT;
pub type FnPropertyInsertRead = unsafe extern "C" fn(PropertyT, NodeT, AvsReaderFn, AvsFile) -> i32;
pub type FnPropertyMemWrite = unsafe extern "C" fn(PropertyT, *mut i8, i32) -> i32;
pub type FnPropertyQuerySize = unsafe extern "C" fn(PropertyT) -> i32;
pub type FnPropertyDestroy = unsafe extern "C" fn(PropertyT);

pub type FnCstreamCreate = unsafe extern "C" fn(i32) -> *mut CStream;
pub type FnCstreamOperate = unsafe extern "C" fn(*mut CStream) -> i32;
pub type FnCstreamFinish = unsafe extern "C" fn(*mut CStream) -> i32;
pub type FnCstreamDestroy = unsafe extern "C" fn(*mut CStream) -> i32;

// ── Resolved function pointer collection ─────────────────────────────

pub struct AvsFunctions {
    // Filesystem
    pub avs_fs_open: FnAvsFsOpen,
    pub avs_fs_close: FnAvsFsClose,
    pub avs_fs_read: FnAvsFsRead,
    pub avs_fs_lstat: FnAvsFsLstat,
    pub avs_fs_fstat: FnAvsFsFstat,
    pub avs_fs_lseek: FnAvsFsLseek,
    pub avs_fs_mount: FnAvsFsMount,
    pub avs_fs_convert_path: FnAvsFsConvertPath,
    // Property
    pub property_read_query_memsize: FnPropertyReadQueryMemsize,
    pub property_read_query_memsize_long: Option<FnPropertyReadQueryMemsizeLong>,
    pub property_create: FnPropertyCreate,
    pub property_insert_read: FnPropertyInsertRead,
    pub property_mem_write: FnPropertyMemWrite,
    pub property_query_size: FnPropertyQuerySize,
    pub property_destroy: FnPropertyDestroy,
    // Compression
    pub cstream_create: FnCstreamCreate,
    pub cstream_operate: FnCstreamOperate,
    pub cstream_finish: FnCstreamFinish,
    pub cstream_destroy: FnCstreamDestroy,
}

unsafe impl Send for AvsFunctions {}
unsafe impl Sync for AvsFunctions {}

/// Result of successful AVS resolution.
pub struct AvsResolution {
    pub functions: AvsFunctions,
    pub version: u16,
    pub version_name: &'static str,
}

// ── Export table definitions (6 AVS versions) ────────────────────────

struct AvsExportTable {
    version_name: &'static str,
    version: u16,
    unique_check: Option<&'static str>,
    // Filesystem
    avs_fs_open: &'static str,
    avs_fs_close: &'static str,
    avs_fs_convert_path: &'static str,
    avs_fs_read: &'static str,
    avs_fs_lseek: &'static str,
    avs_fs_fstat: &'static str,
    avs_fs_lstat: &'static str,
    avs_fs_mount: &'static str,
    // Property
    property_read_query_memsize: &'static str,
    property_read_query_memsize_long: &'static str,
    property_create: &'static str,
    property_insert_read: &'static str,
    property_mem_write: &'static str,
    property_destroy: &'static str,
    property_query_size: &'static str,
    // Compression
    cstream_create: &'static str,
    cstream_operate: &'static str,
    cstream_finish: &'static str,
    cstream_destroy: &'static str,
}

static EXPORT_TABLES: &[AvsExportTable] = &[
    AvsExportTable {
        version_name: "plain (2.12.x and older)",
        version: 1200,
        unique_check: None,
        avs_fs_open: "avs_fs_open",
        avs_fs_close: "avs_fs_close",
        avs_fs_convert_path: "avs_fs_convert_path",
        avs_fs_read: "avs_fs_read",
        avs_fs_lseek: "avs_fs_lseek",
        avs_fs_fstat: "avs_fs_fstat",
        avs_fs_lstat: "avs_fs_lstat",
        avs_fs_mount: "avs_fs_mount",
        property_read_query_memsize: "property_read_query_memsize",
        property_read_query_memsize_long: "property_read_query_memsize_long",
        property_create: "property_create",
        property_insert_read: "property_insert_read",
        property_mem_write: "property_mem_write",
        property_destroy: "property_destroy",
        property_query_size: "property_query_size",
        cstream_create: "cstream_create",
        cstream_operate: "cstream_operate",
        cstream_finish: "cstream_finish",
        cstream_destroy: "cstream_destroy",
    },
    AvsExportTable {
        version_name: "2.13.x (XC058ba5)",
        version: 1300,
        unique_check: None,
        avs_fs_open: "XC058ba50000b6",
        avs_fs_close: "XC058ba500011b",
        avs_fs_convert_path: "XC058ba50000d5",
        avs_fs_read: "XC058ba5000139",
        avs_fs_lseek: "XC058ba500000f",
        avs_fs_fstat: "XC058ba50000d0",
        avs_fs_lstat: "XC058ba5000063",
        avs_fs_mount: "XC058ba500009c",
        property_read_query_memsize: "XC058ba5000066",
        property_read_query_memsize_long: "XC058ba5000091",
        property_create: "XC058ba5000107",
        property_insert_read: "XC058ba5000016",
        property_mem_write: "XC058ba5000162",
        property_destroy: "XC058ba500010f",
        property_query_size: "XC058ba5000101",
        cstream_create: "XC058ba5000118",
        cstream_operate: "XC058ba5000078",
        cstream_finish: "XC058ba5000130",
        cstream_destroy: "XC058ba500012b",
    },
    AvsExportTable {
        version_name: "2.15.x (XCd229cc)",
        version: 1500,
        unique_check: None,
        avs_fs_open: "XCd229cc000090",
        avs_fs_close: "XCd229cc00011f",
        avs_fs_convert_path: "XCd229cc00001e",
        avs_fs_read: "XCd229cc00010d",
        avs_fs_lseek: "XCd229cc00004d",
        avs_fs_fstat: "XCd229cc0000c3",
        avs_fs_lstat: "XCd229cc0000c0",
        avs_fs_mount: "XCd229cc0000ce",
        property_read_query_memsize: "XCd229cc0000ff",
        property_read_query_memsize_long: "XCd229cc00002b",
        property_create: "XCd229cc000126",
        property_insert_read: "XCd229cc00009a",
        property_mem_write: "XCd229cc000033",
        property_destroy: "XCd229cc00013c",
        property_query_size: "XCd229cc000032",
        cstream_create: "XCd229cc000141",
        cstream_operate: "XCd229cc00008c",
        cstream_finish: "XCd229cc000025",
        cstream_destroy: "XCd229cc0000e3",
    },
    AvsExportTable {
        version_name: "2.16.[3-7] (XCnbrep7 SDVX cloud)",
        version: 1630,
        unique_check: Some("XCnbrep700013c"),
        avs_fs_open: "XCnbrep700004e",
        avs_fs_close: "XCnbrep7000055",
        avs_fs_convert_path: "XCnbrep7000046",
        avs_fs_read: "XCnbrep7000051",
        avs_fs_lseek: "XCnbrep700004f",
        avs_fs_fstat: "XCnbrep7000062",
        avs_fs_lstat: "XCnbrep7000063",
        avs_fs_mount: "XCnbrep700004b",
        property_read_query_memsize: "XCnbrep70000b0",
        property_read_query_memsize_long: "XCnbrep70000b1",
        property_create: "XCnbrep7000090",
        property_insert_read: "XCnbrep7000094",
        property_mem_write: "XCnbrep70000b8",
        property_destroy: "XCnbrep7000091",
        property_query_size: "XCnbrep700009f",
        cstream_create: "XCnbrep7000130",
        cstream_operate: "XCnbrep7000132",
        cstream_finish: "XCnbrep7000133",
        cstream_destroy: "XCnbrep7000134",
    },
    AvsExportTable {
        version_name: "2.16.1 (XCnbrep7 IIDX)",
        version: 1610,
        unique_check: None,
        avs_fs_open: "XCnbrep7000039",
        avs_fs_close: "XCnbrep7000040",
        avs_fs_convert_path: "XCnbrep7000031",
        avs_fs_read: "XCnbrep700003c",
        avs_fs_lseek: "XCnbrep700003a",
        avs_fs_fstat: "XCnbrep700004d",
        avs_fs_lstat: "XCnbrep700004e",
        avs_fs_mount: "XCnbrep7000036",
        property_read_query_memsize: "XCnbrep700009b",
        property_read_query_memsize_long: "XCnbrep700009c",
        property_create: "XCnbrep700007b",
        property_insert_read: "XCnbrep700007f",
        property_mem_write: "XCnbrep70000a3",
        property_destroy: "XCnbrep700007c",
        property_query_size: "XCnbrep700008a",
        cstream_create: "XCnbrep7000124",
        cstream_operate: "XCnbrep7000126",
        cstream_finish: "XCnbrep7000127",
        cstream_destroy: "XCnbrep7000128",
    },
    AvsExportTable {
        version_name: "2.17.x (XCgsqzn0)",
        version: 1700,
        unique_check: None,
        avs_fs_open: "XCgsqzn000004e",
        avs_fs_close: "XCgsqzn0000055",
        avs_fs_convert_path: "XCgsqzn0000046",
        avs_fs_read: "XCgsqzn0000051",
        avs_fs_lseek: "XCgsqzn000004f",
        avs_fs_fstat: "XCgsqzn0000062",
        avs_fs_lstat: "XCgsqzn0000063",
        avs_fs_mount: "XCgsqzn000004b",
        property_read_query_memsize: "XCgsqzn00000b0",
        property_read_query_memsize_long: "XCgsqzn00000b1",
        property_create: "XCgsqzn0000090",
        property_insert_read: "XCgsqzn0000094",
        property_mem_write: "XCgsqzn00000b8",
        property_destroy: "XCgsqzn0000091",
        property_query_size: "XCgsqzn000009f",
        cstream_create: "XCgsqzn0000130",
        cstream_operate: "XCgsqzn0000132",
        cstream_finish: "XCgsqzn0000133",
        cstream_destroy: "XCgsqzn0000134",
    },
];

// ── Resolution logic ─────────────────────────────────────────────────

/// Resolve a single export by name from a module handle. Returns None if not found.
unsafe fn get_proc(handle: HMODULE, name: &str) -> Option<*const ()> {
    let cname = CString::new(name).ok()?;
    let addr = GetProcAddress(handle, PCSTR(cname.as_ptr() as *const u8));
    addr.map(|f| f as *const ())
}

/// Try to resolve all required AVS functions from a given export table.
/// Returns None if any required function is missing.
unsafe fn try_resolve_table(handle: HMODULE, table: &AvsExportTable) -> Option<AvsFunctions> {
    // Check unique_check export if present (disambiguates tables with same prefix)
    if let Some(check) = table.unique_check {
        get_proc(handle, check)?;
    }

    macro_rules! resolve {
        ($field:ident) => {{
            #[allow(clippy::missing_transmute_annotations)]
            std::mem::transmute(get_proc(handle, table.$field)?)
        }};
    }

    let memsize_long: Option<FnPropertyReadQueryMemsizeLong> =
        get_proc(handle, table.property_read_query_memsize_long).map(|p| {
            #[allow(clippy::missing_transmute_annotations)]
            std::mem::transmute(p)
        });

    Some(AvsFunctions {
        avs_fs_open: resolve!(avs_fs_open),
        avs_fs_close: resolve!(avs_fs_close),
        avs_fs_read: resolve!(avs_fs_read),
        avs_fs_lstat: resolve!(avs_fs_lstat),
        avs_fs_fstat: resolve!(avs_fs_fstat),
        avs_fs_lseek: resolve!(avs_fs_lseek),
        avs_fs_mount: resolve!(avs_fs_mount),
        avs_fs_convert_path: resolve!(avs_fs_convert_path),
        property_read_query_memsize: resolve!(property_read_query_memsize),
        property_read_query_memsize_long: memsize_long,
        property_create: resolve!(property_create),
        property_insert_read: resolve!(property_insert_read),
        property_mem_write: resolve!(property_mem_write),
        property_query_size: resolve!(property_query_size),
        property_destroy: resolve!(property_destroy),
        cstream_create: resolve!(cstream_create),
        cstream_operate: resolve!(cstream_operate),
        cstream_finish: resolve!(cstream_finish),
        cstream_destroy: resolve!(cstream_destroy),
    })
}

/// Find the AVS DLL in process memory and resolve all function exports.
pub fn resolve_avs() -> Option<AvsResolution> {
    // Find which AVS DLL is loaded
    let handle = find_avs_dll()?;

    // Try each export table until one matches
    for table in EXPORT_TABLES {
        if let Some(functions) = unsafe { try_resolve_table(handle, table) } {
            return Some(AvsResolution {
                functions,
                version: table.version,
                version_name: table.version_name,
            });
        }
    }

    None
}

/// Search for a loaded AVS DLL by known names.
fn find_avs_dll() -> Option<HMODULE> {
    let dll_names: &[&str] = &["libavs-win64.dll", "libavs-win32.dll", "avs2-core.dll"];
    for name in dll_names {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let handle = unsafe { GetModuleHandleW(PCWSTR(wide.as_ptr())) };
        if let Ok(h) = handle {
            if !h.is_invalid() {
                return Some(h);
            }
        }
    }
    None
}

/// Block until a libavs module is loaded. LayeredFS now initializes BEFORE
/// the gamemdx wait (the game's `Application::onBoot` drains shader.arc /
/// musicdb.xml within a few hundred ms of gamemdx loading, so the fs hooks
/// must already be in place), and spice2x loads `-k` DLLs after avs-core —
/// so this normally returns immediately. Bounded: once gamemdx itself is
/// loaded, libavs necessarily is too (ea3 loaded it), so we stop waiting
/// and let `resolve_avs` report the real outcome.
pub fn wait_for_avs_dll() {
    loop {
        if find_avs_dll().is_some() || crate::core::module_resolver::get_game_module().is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Returns the read-mode flag for avs_fs_open based on AVS version.
/// New AVS (≥2.14) uses bitflags (R=1), old AVS uses enum (R=0).
pub fn avs_open_mode_read(version: u16) -> u16 {
    if version >= 1400 {
        1
    } else {
        0
    }
}
