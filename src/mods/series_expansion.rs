//! Series Expansion Mod — Extends the series filter system for custom songs.
//!
//! Patches:
//! 1. Series mapper default: xor eax,eax → mov eax,esi (pass through raw value)
//! 2. Predicate LEA R8: redirect to extended table (filtering)
//! 3. UI loop LEA RBX: point to last custom entry's key field (FilterButton creation)
//! 4. UI loop MOV ESI,8: change to 8+N (create FilterButtons for custom entries)
//! 5. Label builder LEA+count: redirect table and count for song select filter label
//! 6. Per-song version label LEA: redirect to a 256-entry custom table so the
//!    `"Version / %s"` builder doesn't OOB-read on raw series values >= 22.
//! 7. Flare-skill classification walk in `ddr::player::Record::CalcFlareSkill`:
//!    redirect both classification-table disp32s at a 4-entry extended table
//!    and widen the walk bound, adding a highest-priority rule
//!    `series >= 22 → category 0`. Category 0's bucket is never summed into
//!    the flare skill totals, so custom-series songs are excluded from flare
//!    ranking instead of counting toward GOLD. See docs/flare_ranking_research.md.

use crate::core::afp;
use crate::core::memory;
use crate::core::scanner::decode_rip_relative;
use crate::mods::mod_trait::{Mod, ModContext};
use crate::services::afp_patcher;
use crate::services::avs_layeredfs::mod_paths;
use crate::services::series_filter_scroll;
use crate::{log_info, log_warn};
use retour::GenericDetour;
use serde::Deserialize;
use std::ffi::CString;

const ENTRY_STRIDE: usize = 0x88;
const VANILLA_ENTRY_COUNT: usize = 9;

// ── Flare-skill exclusion (patch 7) ─────────────────────────────────
// Offsets within the `flare_skill_classifier` match — see the signature
// comment in core/signatures.rs and docs/flare_ranking_research.md.
/// MOV disp32 — category-table walk base RVA (module-base-relative).
const FLARE_CAT_DISP32: usize = 20;
/// CMP disp32 — threshold-table walk base RVA (module-base-relative).
const FLARE_THR_DISP32: usize = 28;
/// imm8 of `CMP idx,-8` — the walk's entry-count bound (0xF8 → 0xF4).
const FLARE_LOOP_BOUND: usize = 41;
/// First series value excluded from flare skill. 21 (WORLD) is the highest
/// stock series value; everything above is custom content.
const FLARE_EXCLUDE_MIN_SERIES: u32 = 22;

/// Filtersort category index for the VERSION filter. The game keys filter
/// selection state by category in a per-category entry-count table
/// (`filter_entry_count_table`); VERSION is category 1. Verified at runtime:
/// category 1 carries the WORLD bit (0x100) in the saved `version` bitfield.
const VERSION_FILTER_CATEGORY: u32 = 1;

/// Signature: `fn(category: u32) -> u32`. Returns the entry count for a
/// filtersort category. The save/load paths bound their per-entry bit loops by
/// this count, so the VERSION category's hardcoded 9 must be widened to include
/// custom entries (see `filter_count_hook`).
type FilterCountFn = unsafe extern "C" fn(u32) -> u32;

/// The single detour on the filter entry-count leaf function. Accessed only
/// from `enable()` (write) and `filter_count_hook` (read) on the same thread.
static mut FILTER_COUNT_HOOK: Option<GenericDetour<FilterCountFn>> = None;

/// Total VERSION entry count to report from the hook: `VANILLA_ENTRY_COUNT +
/// n_custom`. Set in `init()` before the detour is installed.
static mut VERSION_TOTAL_COUNT: u32 = VANILLA_ENTRY_COUNT as u32;

/// Detour body for the per-category entry-count function. For the VERSION
/// category, report the extended count (stock + custom) so the filtersort
/// save mask-builder and load apply-loop cover custom series entries. All other
/// categories (including category 12, which shares VERSION's switch body in the
/// stock binary) delegate to the original — leaving their behavior untouched.
///
/// Panic-free: no `unwrap`/indexing; the original is called through a guarded
/// `Option` and a missing hook degrades to 0 (the function's own default-case
/// return) rather than crashing across the FFI boundary.
unsafe extern "C" fn filter_count_hook(category: u32) -> u32 {
    if category == VERSION_FILTER_CATEGORY {
        return *std::ptr::addr_of!(VERSION_TOTAL_COUNT);
    }
    match &*std::ptr::addr_of!(FILTER_COUNT_HOOK) {
        Some(hook) => hook.call(category),
        None => 0,
    }
}

// ── LayeredFS label-texture injection ───────────────────────────────
//
// Custom filter labels (`sefi_version_{key}`) live in the stock
// `select_music_option_v3.ifs`. We inject them the same way
// `custom_options` and `folder_expansion` do: clone a donor `sefi_version_*`
// atlas slot for each custom label and emit a `texturelist.merged.xml` that
// LayeredFS merges into the IFS at mount time. Raw PNGs alone don't work —
// the `filter_item` MovieClip applies the texture by name and expects it at
// a real atlas UV position, not the full-coverage UV the auto-inject path
// produces.

/// Mod folder holding the source label PNGs (and where the generated
/// `texturelist.merged.xml` is written).
const MOD_ROOT: &str = "./data_mods/custom_series";
/// Shared LayeredFS atlas/texturelist cache root.
const CACHE_ROOT: &str = "./data_mods/_cache";
/// Stock ARC + IFS containing the `sefi_version_*` filter labels.
const OPTION_ARC: &str = "data/arc/bm2d/select_music_option_v3.arc";
const OPTION_IFS: &str = "select_music_option_v3.ifs";
const OPTION_IFS_MOD_PATH: &str = "select_music_option_v3_ifs";
/// Atlas-name prefix for our cloned atlases. MUST be unique across all mods
/// that inject into this IFS (`custom_options` uses `copt_mods`) — the cache
/// filename is `md5(prefix_NNN)`, so a collision would overwrite the other
/// mod's atlas blob.
const ATLAS_PREFIX: &str = "cser_version";
/// Donor label whose atlas slot every custom label clones. `world` is a
/// plain GOLD-group version label with the right slot shape.
const LABEL_DONOR: &str = "sefi_version_world";

const FIELD_GROUP: usize = 0x00;
const FIELD_KEY: usize = 0x08;
const FIELD_SERIES_START: usize = 0x30;
const FIELD_CODE: usize = 0x38;
const FIELD_DISPLAY: usize = 0x60;
const STR_LENGTH: usize = 0x10;
const STR_CAPACITY: usize = 0x18;
const SSO_CAPACITY: u64 = 0x0F;

const VANILLA_ENTRIES: &[(&str, u32, u32, &str, &str)] = &[
    ("1th5th", 0, 1, "1st", "5th"),
    ("maxex", 1, 6, "MAX", "EXTREME"),
    ("novanova2", 2, 9, "SuperNOVA", "SuperNOVA2"),
    ("x", 3, 11, "X", "X3 VS 2ndMIX"),
    ("1314", 4, 14, "2013", "2014"),
    ("a", 5, 17, "A", "A"),
    ("a20plus", 6, 18, "A20", "A20 PLUS"),
    ("a3", 7, 20, "A3", "A3"),
    ("world", 8, 21, "WORLD", "WORLD"),
];

static mut SERIES_CONFIG: Option<SeriesConfig> = None;

#[derive(Deserialize, Clone)]
pub struct SeriesConfig {
    pub custom_series: Vec<CustomSeriesEntry>,
}

#[derive(Deserialize, Clone)]
pub struct CustomSeriesEntry {
    pub series_value: u8,
    pub label: String,
    pub texture_name: String,
}

pub fn get_config() -> Option<&'static SeriesConfig> {
    unsafe { (*std::ptr::addr_of!(SERIES_CONFIG)).as_ref() }
}

/// Build cloned-atlas label textures for each custom series and emit a
/// `texturelist.merged.xml` LayeredFS merges into the stock options IFS.
///
/// Each custom entry's `sefi_version_{texture_name}` label is composited at a
/// donor slot (cloned from `sefi_version_world`) so the `filter_item` MovieClip
/// resolves it by name with the correct atlas UVs. Source PNGs are read from
/// `data_mods/custom_series/select_music_option_v3_ifs/tex/`.
///
/// Graceful degradation: a missing stock texturelist or absent PNGs log at
/// WARN and skip — the filter entries still appear, just without custom labels.
fn generate_label_atlases(config: &SeriesConfig) {
    use crate::services::avs_layeredfs::atlas_cloner::{
        generate_cloned_atlases, load_stock_texturelist, NewTextureSpec,
    };

    if config.custom_series.is_empty() {
        return;
    }

    let texlist_xml = match load_stock_texturelist(OPTION_ARC, OPTION_IFS) {
        Some(x) => x,
        None => {
            log_warn!(
                "SeriesExpansion: could not load stock texturelist from {} — custom labels disabled",
                OPTION_ARC
            );
            return;
        }
    };

    // Texture name + PNG path per custom entry. Held in a Vec so the
    // NewTextureSpec borrows stay valid for the generate call.
    let owned: Vec<(String, String)> = config
        .custom_series
        .iter()
        .map(|entry| {
            let new_name = format!("sefi_version_{}", entry.texture_name);
            let png_path = format!("{}/{}/tex/{}.png", MOD_ROOT, OPTION_IFS_MOD_PATH, new_name);
            (new_name, png_path)
        })
        .collect();

    let specs: Vec<NewTextureSpec<'_>> = owned
        .iter()
        .map(|(new_name, png_path)| NewTextureSpec {
            new_name,
            donor_name: LABEL_DONOR,
            png_path,
        })
        .collect();

    if generate_cloned_atlases(
        &texlist_xml,
        OPTION_IFS_MOD_PATH,
        CACHE_ROOT,
        MOD_ROOT,
        ATLAS_PREFIX,
        &specs,
    ) {
        log_info!(
            "SeriesExpansion: generated {} custom label texture(s) into {}",
            specs.len(),
            OPTION_IFS
        );
        // Rescan mod folders so LayeredFS picks up the generated merged xml +
        // cached atlas before the game mounts the options IFS.
        mod_paths::init_mod_paths();
    } else {
        log_warn!("SeriesExpansion: label atlas generation produced no output");
    }
}

struct SavedPatch {
    addr: *mut u8,
    original: Vec<u8>,
    patched: Vec<u8>,
}

impl SavedPatch {
    fn save(addr: *mut u8, size: usize, patched: Vec<u8>) -> Self {
        let original = unsafe { std::slice::from_raw_parts(addr, size).to_vec() };
        Self {
            addr,
            original,
            patched,
        }
    }
    fn apply(&self) {
        unsafe {
            let old = memory::make_writable(self.addr as *const u8, self.patched.len());
            for (i, &b) in self.patched.iter().enumerate() {
                *self.addr.add(i) = b;
            }
            memory::restore_protection(self.addr as *const u8, self.patched.len(), old);
        }
    }
    fn restore(&self) {
        unsafe {
            let old = memory::make_writable(self.addr as *const u8, self.original.len());
            for (i, &b) in self.original.iter().enumerate() {
                *self.addr.add(i) = b;
            }
            memory::restore_protection(self.addr as *const u8, self.original.len(), old);
        }
    }
}

fn rip_disp(instr_addr: *const u8, instr_size: usize, target: *const u8) -> i32 {
    let next = instr_addr as isize + instr_size as isize;
    (target as isize - next) as i32
}

/// Bytes the VERSION filter-label builder seeds its result string with — the
/// literal `"DDR "` passed to the string ctor. This is what distinguishes the
/// VERSION builder from look-alike sibling builders (e.g. Clear Type) that share
/// the `filter_label_builder_count` shape but seed with the empty string.
const VERSION_SEED: &[u8] = b"DDR ";

/// How far back from a `filter_label_builder_count` match to scan for the seed
/// `LEA RDX, [rip→"DDR "]`. The seed sits ~0x78 bytes before the match in both
/// known builds (match is ~0xC6 into the builder, seed ~0x4E in); 0xC0 covers it
/// with margin while staying inside the enclosing function.
const SEED_SCAN_WINDOW: usize = 0xC0;

/// Returns true if the `filter_label_builder_count` match at `match_addr` belongs
/// to the VERSION builder — i.e. the enclosing function seeds its result string
/// with `"DDR "`. Scans backward for a `LEA RDX, [rip+disp32]` (opcode `48 8D 15`)
/// whose target holds [`VERSION_SEED`].
///
/// SAFETY: reads up to `SEED_SCAN_WINDOW` bytes before `match_addr` (always within
/// the same loaded `.text` builder) and dereferences each decoded LEA target only
/// after confirming the opcode, reading a fixed 4 bytes.
unsafe fn builder_seeds_with_ddr(match_addr: *const u8) -> bool {
    // Walk the window from oldest to the match, looking for the LEA RDX, rip.
    for off in (3..=SEED_SCAN_WINDOW).rev() {
        let p = match_addr.sub(off);
        if *p == 0x48 && *p.add(1) == 0x8D && *p.add(2) == 0x15 {
            let target = decode_rip_relative(p.add(3)) as *const u8;
            let seed = std::slice::from_raw_parts(target, VERSION_SEED.len());
            if seed == VERSION_SEED {
                return true;
            }
        }
    }
    false
}

unsafe fn write_sso_string(base: *mut u8, s: &str) {
    let bytes = s.as_bytes();
    assert!(bytes.len() <= 15, "SSO string too long: {}", s);
    for (i, &b) in bytes.iter().enumerate() {
        *base.add(i) = b;
    }
    for i in bytes.len()..16 {
        *base.add(i) = 0;
    }
    *(base.add(STR_LENGTH) as *mut u64) = bytes.len() as u64;
    *(base.add(STR_CAPACITY) as *mut u64) = SSO_CAPACITY;
}

unsafe fn write_entry(
    base: *mut u8,
    group: u32,
    key: &str,
    series_start: u32,
    code: &str,
    display: &str,
) {
    std::ptr::write_bytes(base, 0, ENTRY_STRIDE);
    *(base.add(FIELD_GROUP) as *mut u32) = group;
    write_sso_string(base.add(FIELD_KEY), key);
    *(base.add(FIELD_SERIES_START) as *mut u32) = series_start;
    write_sso_string(base.add(FIELD_CODE), code);
    write_sso_string(base.add(FIELD_DISPLAY), display);
}

pub struct SeriesExpansionMod {
    table: *mut u8,
    patches: Vec<SavedPatch>,
    // Resolved addresses
    mapper_default: *mut u8,
    pred_lea_addr: *const u8,
    ui_loop_count: *mut u8,
    ui_loop_lea: *const u8,
    thumb_loop_bound: *mut u8,
    label_builder_sites: Vec<*const u8>, // AOB matches for filter_label_builder_count
    // Per-song version label LEA — points at the LEA instruction itself
    // (not the disp32 operand). Form determines the disp32 offset:
    // standalone → +3, inlined → +3 (LEA encoding is identical, only the
    // location of the LEA within the match differs).
    label_lookup_lea: *const u8,
    label_lookup_table: *mut u8, // 256-entry custom string-pointer table
    label_strings: Vec<CString>, // owns the C-string memory referenced by table entries
    // Per-category filtersort entry-count function. Detoured to report the
    // extended VERSION count so custom-series filter selections persist in the
    // saved `version` bitfield.
    filter_count_addr: *const u8,
    // Flare-skill exclusion: the `flare_skill_classifier` match (null when the
    // signature is missing or table validation failed — patch skipped), plus
    // the two replacement module-base-relative disp32s pointing into the
    // mod-owned 4-entry classification tables built in init().
    flare_site: *const u8,
    flare_cat_disp: i32,
    flare_thr_disp: i32,
    n_custom: usize,
}

unsafe impl Send for SeriesExpansionMod {}

impl SeriesExpansionMod {
    pub fn new() -> Self {
        Self {
            table: std::ptr::null_mut(),
            patches: Vec::new(),
            mapper_default: std::ptr::null_mut(),
            pred_lea_addr: std::ptr::null(),
            ui_loop_count: std::ptr::null_mut(),
            ui_loop_lea: std::ptr::null(),
            thumb_loop_bound: std::ptr::null_mut(),
            label_builder_sites: Vec::new(),
            label_lookup_lea: std::ptr::null(),
            label_lookup_table: std::ptr::null_mut(),
            label_strings: Vec::new(),
            filter_count_addr: std::ptr::null(),
            flare_site: std::ptr::null(),
            flare_cat_disp: 0,
            flare_thr_disp: 0,
            n_custom: 0,
        }
    }

    fn load_config() -> Option<SeriesConfig> {
        match super::config::get() {
            Some(cfg) => match cfg.series_expansion.clone() {
                Some(config) => {
                    log_info!(
                        "SeriesExpansion: loaded {} custom series from config",
                        config.custom_series.len()
                    );
                    Some(config)
                }
                None => {
                    log_warn!("SeriesExpansion: no series_expansion config — mod disabled");
                    None
                }
            },
            None => {
                log_warn!("SeriesExpansion: config store not available — mod disabled");
                None
            }
        }
    }

    /// Build the extended flare-skill classification tables and remember the
    /// patch site (patch 7). The stock walk classifies a song's RAW series
    /// byte into flare-skill version categories (>=18 GOLD, >=14 WHITE,
    /// >=1 CLASSIC) with no upper bound, so custom series would count toward
    /// GOLD. We copy the stock 3-entry tables (inheriting any future
    /// threshold rebalance) and prepend a highest-priority rule
    /// `series >= 22 → category 0` — category 0's bucket is never summed
    /// into the flare skill totals (the game's own dead path for raw
    /// series 0). Fail-closed: any validation failure leaves `flare_site`
    /// null and the stock behavior untouched.
    ///
    /// SAFETY: reads the two disp32 operands inside the matched instruction
    /// bytes and the six stock table dwords they reference (read-only
    /// .rdata, valid for process lifetime).
    unsafe fn init_flare_exclusion(&mut self, site: *const u8, module_base: *const u8) {
        // Decode the stock walk-base RVAs (module-base-relative, NOT
        // rip-relative) and copy the stock entries. The walk base points at
        // the LAST entry; walk offsets 0/-4/-8 are entries [2]/[1]/[0].
        let cat_rva = std::ptr::read_unaligned(site.add(FLARE_CAT_DISP32) as *const i32);
        let thr_rva = std::ptr::read_unaligned(site.add(FLARE_THR_DISP32) as *const i32);
        let stock_cat = module_base.offset(cat_rva as isize) as *const u32;
        let stock_thr = module_base.offset(thr_rva as isize) as *const u32;
        let cats = [*stock_cat.sub(2), *stock_cat.sub(1), *stock_cat];
        let thrs = [*stock_thr.sub(2), *stock_thr.sub(1), *stock_thr];

        // Sanity: ascending thresholds strictly below the exclusion bound
        // (or the new rule would shadow a stock one), categories in 1..=3
        // (the summed set; also indexed into 4-element UI name arrays).
        let sane = thrs[0] >= 1
            && thrs[0] < thrs[1]
            && thrs[1] < thrs[2]
            && thrs[2] < FLARE_EXCLUDE_MIN_SERIES
            && cats.iter().all(|&c| (1..=3).contains(&c));
        if !sane {
            log_warn!(
                "SeriesExpansion: unexpected flare classification tables (cats={:?} thrs={:?}) — flare exclusion disabled",
                cats,
                thrs
            );
            return;
        }

        // Extended block: cats [1,2,3,0] then thrs [1,14,18,22]. The walk
        // bases (last entries) sit at +12 and +28; the patched walk reads
        // offsets 0/-4/-8/-12 from them.
        let block = memory::alloc_near(module_base, 32) as *mut u32;
        if block.is_null() {
            log_warn!(
                "SeriesExpansion: failed to allocate flare classification tables — flare exclusion disabled"
            );
            return;
        }
        for i in 0..3 {
            *block.add(i) = cats[i];
            *block.add(4 + i) = thrs[i];
        }
        *block.add(3) = 0; // category 0 = excluded from flare skill totals
        *block.add(7) = FLARE_EXCLUDE_MIN_SERIES;

        // The new walk bases must encode as module-base-relative disp32s.
        // alloc_near stays within ±2GB, but verify rather than assume.
        let cat_disp = block.add(3) as isize - module_base as isize;
        let thr_disp = block.add(7) as isize - module_base as isize;
        match (i32::try_from(cat_disp), i32::try_from(thr_disp)) {
            (Ok(c), Ok(t)) => {
                self.flare_site = site;
                self.flare_cat_disp = c;
                self.flare_thr_disp = t;
                log_info!(
                    "SeriesExpansion: flare exclusion tables built at {:p} (walk site {:p})",
                    block,
                    site
                );
            }
            _ => {
                log_warn!(
                    "SeriesExpansion: flare table block at {:p} out of disp32 range — flare exclusion disabled",
                    block
                );
            }
        }
    }
}

impl Mod for SeriesExpansionMod {
    fn id(&self) -> &str {
        "series-expansion"
    }
    fn name(&self) -> &str {
        "Series Expansion"
    }
    fn description(&self) -> &str {
        "Custom series filters for modded songs"
    }
    fn required_signatures(&self) -> &[&str] {
        &[
            "series_mapper_bounds",
            "version_predicate_lea",
            "ui_entry_loop",
            "thumbnail_arc_loop",
            "filter_entry_count_table",
        ]
    }

    fn init(&mut self, ctx: &ModContext) -> bool {
        let config = match Self::load_config() {
            Some(c) => c,
            None => return false,
        };
        if config.custom_series.is_empty() {
            log_warn!("SeriesExpansion: no custom series defined — mod disabled");
            return false;
        }

        self.n_custom = config.custom_series.len();

        // Resolve the per-category filtersort entry-count function and stash the
        // extended VERSION count for the detour to report. This widens the
        // filtersort save/load bit loops so custom-series selections persist in
        // the saved `version` bitfield.
        self.filter_count_addr = ctx.signatures.require_address("filter_entry_count_table");
        unsafe {
            VERSION_TOTAL_COUNT = (VANILLA_ENTRY_COUNT + self.n_custom) as u32;
        }

        let total_entries = VANILLA_ENTRY_COUNT + self.n_custom + 1; // +1 sentinel
        let table_size = total_entries * ENTRY_STRIDE;

        // Allocate near game module for RIP-relative disp32 range
        self.table = unsafe { memory::alloc_near(ctx.game_module.base, table_size) };
        if self.table.is_null() {
            log_warn!("SeriesExpansion: failed to allocate table near game module");
            return false;
        }

        // Write vanilla entries
        for (i, &(key, group, series_start, code, display)) in VANILLA_ENTRIES.iter().enumerate() {
            unsafe {
                write_entry(
                    self.table.add(i * ENTRY_STRIDE),
                    group,
                    key,
                    series_start,
                    code,
                    display,
                );
            }
        }

        // Write custom entries (sorted by series_value)
        let mut sorted: Vec<&CustomSeriesEntry> = config.custom_series.iter().collect();
        sorted.sort_by_key(|e| e.series_value);
        for (i, entry) in sorted.iter().enumerate() {
            let idx = VANILLA_ENTRY_COUNT + i;
            unsafe {
                write_entry(
                    self.table.add(idx * ENTRY_STRIDE),
                    8,
                    &entry.texture_name,
                    entry.series_value as u32,
                    &entry.label,
                    &entry.label,
                );
            }
            log_info!(
                "SeriesExpansion: entry [{}] series={} key={}",
                idx,
                entry.series_value,
                entry.texture_name
            );
        }

        // Write sentinel
        let sentinel_idx = VANILLA_ENTRY_COUNT + self.n_custom;
        let max_series = sorted
            .last()
            .map(|e| e.series_value as u32 + 1)
            .unwrap_or(22);
        unsafe {
            write_entry(
                self.table.add(sentinel_idx * ENTRY_STRIDE),
                sentinel_idx as u32,
                "",
                max_series,
                "",
                "",
            );
        }

        // Resolve mapper default case
        let bounds_match = ctx.signatures.require_address("series_mapper_bounds");
        self.mapper_default = unsafe { decode_rip_relative(bounds_match.add(11)) as *mut u8 };
        unsafe {
            let b0 = *self.mapper_default;
            let b1 = *self.mapper_default.add(1);
            if !((b0 == 0x31 || b0 == 0x33) && b1 == 0xC0) {
                log_warn!(
                    "SeriesExpansion: unexpected bytes at mapper default: {:02X} {:02X}",
                    b0,
                    b1
                );
                return false;
            }
        }

        // Resolve predicate LEA (at match+12)
        let pred_match = ctx.signatures.require_address("version_predicate_lea");
        self.pred_lea_addr = unsafe { pred_match.add(12) };

        // Resolve UI entry loop: MOV ESI,8 at offset 0, LEA RBX at offset 5
        let ui_match = ctx.signatures.require_address("ui_entry_loop");
        self.ui_loop_count = unsafe { ui_match.add(1) as *mut u8 }; // imm32 at offset 1
        self.ui_loop_lea = unsafe { ui_match.add(5) }; // LEA RBX at offset 5

        // Resolve thumbnail ARC loop bound: CMP RSI,0x15 at offset 6
        let thumb_match = ctx.signatures.require_address("thumbnail_arc_loop");
        self.thumb_loop_bound = unsafe { thumb_match.add(6) as *mut u8 };

        // Resolve label builder sites (song select filter label display).
        // Pattern matches at MOV [RSP+0x20]. Count byte (0x09) at offset 13.
        // LEA RCX [table_base] is 0x64 bytes before the count byte.
        //
        // The pattern keys on `MOV EDX,9`, which matches EVERY filter category
        // whose label builder has 9 entries — not just VERSION. The VERSION
        // builder and at least one other (Clear Type) both qualify, and patching
        // a non-VERSION builder repoints its label table at our version-entry
        // table, corrupting that category's label lookup and crashing the game
        // when that filter is selected (e.g. Clear Type → FC). Disambiguate: the
        // VERSION builder is the one that seeds its result string with the
        // literal "DDR " (`LEA RDX, [rip→"DDR "]` shortly before the match). Keep
        // only that site; skip the look-alike sibling builders.
        for addr in ctx.signatures.get_all_matches("filter_label_builder_count") {
            if unsafe { builder_seeds_with_ddr(addr) } {
                self.label_builder_sites.push(addr);
            } else {
                log_info!(
                    "SeriesExpansion: skipping non-VERSION label builder @ {:p} (no \"DDR \" seed)",
                    addr
                );
            }
        }
        log_info!(
            "SeriesExpansion: found {} VERSION label builder site(s)",
            self.label_builder_sites.len()
        );

        // Resolve per-song version label lookup LEA. Two structurally distinct
        // forms across builds; whichever matches wins. The standalone form
        // (newer builds) places the LEA 7 bytes into the match; the inlined
        // form (older builds) places it 8 bytes into the match. We patch the
        // disp32 either way to redirect the lookup at a 256-entry custom
        // table (built below).
        if let Some(m) = ctx.signatures.get_address("series_label_lookup_standalone") {
            self.label_lookup_lea = unsafe { m.add(7) };
        } else if let Some(m) = ctx.signatures.get_address("series_label_lookup_inlined") {
            self.label_lookup_lea = unsafe { m.add(8) };
        } else {
            log_warn!(
                "SeriesExpansion: no series_label_lookup signature matched — \
                 custom-series filter selection will crash sprintf_s if any \
                 songs are assigned to series >= 22"
            );
        }

        // Build the 256-entry replacement string-pointer table. Entries 0..21
        // mirror the original table (read via the LEA's existing disp32);
        // entries 22..255 default to the WORLD entry (idx 21) as a safe
        // fallback, then are overridden with config-supplied custom labels
        // for declared series values.
        if !self.label_lookup_lea.is_null() {
            let table_size = 256 * std::mem::size_of::<*const u8>();
            self.label_lookup_table =
                unsafe { memory::alloc_near(ctx.game_module.base, table_size) };
            if self.label_lookup_table.is_null() {
                log_warn!(
                    "SeriesExpansion: failed to allocate label-lookup table — \
                     skipping label fix"
                );
                self.label_lookup_lea = std::ptr::null();
            } else {
                unsafe {
                    let original_table =
                        decode_rip_relative(self.label_lookup_lea.add(3)) as *const *const u8;
                    let new_table = self.label_lookup_table as *mut *const u8;

                    // Copy vanilla entries 0..=21.
                    let world_ptr = *original_table.add(21);
                    for i in 0..22 {
                        *new_table.add(i) = *original_table.add(i);
                    }
                    // Default 22..=255 to the WORLD pointer so any unexpected
                    // raw value renders as "Version / DanceDanceRevolution
                    // WORLD" instead of crashing.
                    for i in 22..256 {
                        *new_table.add(i) = world_ptr;
                    }

                    // Override declared custom series with their config labels.
                    // CString backing storage lives in `self.label_strings` for
                    // the mod's lifetime; the raw pointers we install remain
                    // valid as long as the mod is loaded.
                    for entry in &config.custom_series {
                        let cstr = match CString::new(entry.label.as_str()) {
                            Ok(c) => c,
                            Err(_) => {
                                log_warn!(
                                    "SeriesExpansion: custom series {} label contains a NUL — using WORLD fallback",
                                    entry.series_value
                                );
                                continue;
                            }
                        };
                        let ptr = cstr.as_ptr() as *const u8;
                        self.label_strings.push(cstr);
                        *new_table.add(entry.series_value as usize) = ptr;
                    }
                }
                log_info!(
                    "SeriesExpansion: label-lookup table built at {:p} (LEA @ {:p})",
                    self.label_lookup_table,
                    self.label_lookup_lea
                );
            }
        }

        // Resolve the CalcFlareSkill classification walk and build the
        // extended tables (patch 7). Optional — a missing signature degrades
        // to stock behavior (custom series count toward GOLD flare skill)
        // without disabling the filter patches.
        match ctx.signatures.get_address("flare_skill_classifier") {
            Some(site) => unsafe {
                self.init_flare_exclusion(site, ctx.game_module.base);
            },
            None => {
                log_warn!(
                    "SeriesExpansion: flare_skill_classifier signature not found — \
                     custom series will count toward GOLD flare skill"
                );
            }
        }

        unsafe {
            SERIES_CONFIG = Some(config);
        }
        log_info!(
            "SeriesExpansion: initialized — table at {:p}, {} custom entries",
            self.table,
            self.n_custom
        );
        true
    }

    fn enable(&mut self) {
        // Generate the custom filter-label atlases + texturelist.merged.xml so
        // LayeredFS can serve `sefi_version_{key}` from the stock options IFS.
        if let Some(config) = get_config() {
            generate_label_atlases(config);
        }

        let last_entry_idx = VANILLA_ENTRY_COUNT + self.n_custom - 1;
        let last_entry_key = unsafe { self.table.add(last_entry_idx * ENTRY_STRIDE + FIELD_KEY) };

        // 1. Mapper default: xor eax,eax → mov eax,esi
        self.patches
            .push(SavedPatch::save(self.mapper_default, 2, vec![0x89, 0xF0]));

        // 2. Predicate LEA R8 → new table
        let disp = rip_disp(self.pred_lea_addr, 7, self.table as *const u8);
        self.patches.push(SavedPatch::save(
            unsafe { self.pred_lea_addr.add(3) as *mut u8 },
            4,
            disp.to_le_bytes().to_vec(),
        ));

        // 3. UI loop LEA RBX → last custom entry's key field
        let disp = rip_disp(self.ui_loop_lea, 7, last_entry_key as *const u8);
        self.patches.push(SavedPatch::save(
            unsafe { self.ui_loop_lea.add(3) as *mut u8 },
            4,
            disp.to_le_bytes().to_vec(),
        ));

        // 4. UI loop count: MOV ESI,8 → MOV ESI,8+N
        let new_count = (VANILLA_ENTRY_COUNT - 1 + self.n_custom) as u32; // 8 + N (loop counts from this down to 0)
        self.patches.push(SavedPatch::save(
            self.ui_loop_count,
            4,
            new_count.to_le_bytes().to_vec(),
        ));

        // 5. Thumbnail ARC loop: CMP RSI,0x15 → CMP RSI,max_series
        //    Only extend to the highest custom series value (not 0xFF — too many
        //    file open attempts crashes the AVS filesystem).
        if let Some(config) = get_config() {
            let max_series = config
                .custom_series
                .iter()
                .map(|e| e.series_value)
                .max()
                .unwrap_or(21);
            if max_series > 0x15 {
                self.patches
                    .push(SavedPatch::save(self.thumb_loop_bound, 1, vec![max_series]));
            }
        }

        for p in &self.patches {
            p.apply();
        }

        // 6. Label builder: redirect table LEA and update count at each site.
        //    MOV EDX opcode (BA) at pattern offset 13, imm32 at offset 14.
        //    LEA RCX [table] is 0x64 bytes before the BA opcode.
        let total_count = (VANILLA_ENTRY_COUNT + self.n_custom) as u32;
        for site in &self.label_builder_sites {
            unsafe {
                let count_addr = site.add(14) as *mut u8;
                let patch = SavedPatch::save(count_addr, 4, total_count.to_le_bytes().to_vec());
                patch.apply();
                self.patches.push(patch);

                let lea_addr = site.add(13).sub(0x64);
                let disp = rip_disp(lea_addr, 7, self.table as *const u8);
                let patch =
                    SavedPatch::save(lea_addr.add(3) as *mut u8, 4, disp.to_le_bytes().to_vec());
                patch.apply();
                self.patches.push(patch);
            }
        }

        // 6b. Per-song version label LEA: redirect at the 256-entry custom
        //     table built in init(). This is the fix for the OOB read in the
        //     "Version / %s" filter chip builder when songs are assigned to
        //     custom series values >= 22.
        if !self.label_lookup_lea.is_null() && !self.label_lookup_table.is_null() {
            let disp = rip_disp(
                self.label_lookup_lea,
                7,
                self.label_lookup_table as *const u8,
            );
            let patch = SavedPatch::save(
                unsafe { self.label_lookup_lea.add(3) as *mut u8 },
                4,
                disp.to_le_bytes().to_vec(),
            );
            patch.apply();
            self.patches.push(patch);
        }

        // 6c. Flare-skill exclusion: redirect the CalcFlareSkill
        //     classification walk at the extended 4-entry tables and widen
        //     the loop bound from 3 entries (-8) to 4 (-12). The disp32s are
        //     module-BASE-relative RVAs (the walk adds a LEA-materialized
        //     base register), so the values were precomputed in init() as
        //     `table - module_base` — NOT via rip_disp.
        if !self.flare_site.is_null() {
            unsafe {
                let patch = SavedPatch::save(
                    self.flare_site.add(FLARE_CAT_DISP32) as *mut u8,
                    4,
                    self.flare_cat_disp.to_le_bytes().to_vec(),
                );
                patch.apply();
                self.patches.push(patch);

                let patch = SavedPatch::save(
                    self.flare_site.add(FLARE_THR_DISP32) as *mut u8,
                    4,
                    self.flare_thr_disp.to_le_bytes().to_vec(),
                );
                patch.apply();
                self.patches.push(patch);

                // CMP idx,-8 → CMP idx,-12 (imm8 0xF8 → 0xF4).
                let patch = SavedPatch::save(
                    self.flare_site.add(FLARE_LOOP_BOUND) as *mut u8,
                    1,
                    vec![0xF4],
                );
                patch.apply();
                self.patches.push(patch);
            }
            log_info!(
                "SeriesExpansion: flare-skill exclusion active — series >= {} no longer count toward flare ranking",
                FLARE_EXCLUDE_MIN_SERIES
            );
        }

        // 7. Register AFP patches to inject scroll children into filter panel templates.
        let scroll_children: &[afp::ChildDef] = &[
            afp::ChildDef {
                name: "scroll_usr",
                depth: 11,
            },
            afp::ChildDef {
                name: "move_usr",
                depth: 12,
            },
            afp::ChildDef {
                name: "tri_l_usr",
                depth: 13,
            },
            afp::ChildDef {
                name: "tri_r_usr",
                depth: 14,
            },
        ];
        for i in 1..=5 {
            let template_name = format!("filter_switch_base{:02}", i);
            afp_patcher::register_patch(
                &template_name,
                Box::new(move |afp_data, bsi_data| {
                    afp::patch_inject_children(afp_data, bsi_data, scroll_children)
                }),
            );
        }

        log_info!(
            "SeriesExpansion: enabled — {} patches applied + AFP scroll children registered",
            self.patches.len()
        );

        // 7. Configure scroll driver with filter panel layout.
        let total_entries = VANILLA_ENTRY_COUNT + self.n_custom;
        if series_filter_scroll::is_available() {
            series_filter_scroll::configure(series_filter_scroll::ScrollConfig {
                columns: 2,
                row_height: 26.0,
                visible_rows: 9,
                total_entries,
            });
        }

        // 8. Detour the per-category filtersort entry-count function so the
        //    VERSION category reports stock + custom entries. This is what makes
        //    a selected custom-series filter persist in the saved `version`
        //    bitfield (both the save mask-builder and the load apply-loop bound
        //    their per-entry bit loops by this count). Other categories pass
        //    through to the original.
        if !self.filter_count_addr.is_null() {
            unsafe {
                let target: FilterCountFn = std::mem::transmute(self.filter_count_addr);
                match crate::core::hooks::install_enabled(
                    std::ptr::addr_of_mut!(FILTER_COUNT_HOOK),
                    target,
                    filter_count_hook,
                ) {
                    Ok(()) => {
                        log_info!(
                            "SeriesExpansion: filter entry-count detour installed (VERSION count {})",
                            *std::ptr::addr_of!(VERSION_TOTAL_COUNT)
                        );
                    }
                    Err(e) => {
                        log_warn!(
                            "SeriesExpansion: filter entry-count detour install failed: {:?} — custom filter selections will not persist",
                            e
                        );
                    }
                }
            }
        }
    }

    fn disable(&mut self) {
        for p in &self.patches {
            p.restore();
        }
        self.patches.clear();
        unsafe {
            FILTER_COUNT_HOOK = None;
        }
        log_info!("SeriesExpansion: disabled");
    }
}
