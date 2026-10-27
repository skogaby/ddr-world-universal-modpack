//! The injection ceiling — how many indexed songs one boot may put into the
//! game, decided deterministically from the index and the game's limits.
//! Pure: `std` + `super::model` only; mounted by
//! `scripts/validate_stepmania.sh`.
//!
//! There is no operator knob. The index may hold any number of songs (the
//! `index.bin` / artefact caches are unbounded); what reaches `musicdb.xml`,
//! the thumbnail arc and the boot pass is the subset this module selects,
//! and the rest is reported in one WARN. Two limits apply, in order:
//!
//! 1. **Count** — [`MAX_INJECTED_SONGS`]. The texture table (limit 5) is
//!    the hard cliff: its patched size is the handle format's maximum, so
//!    30,208 injected thumbnails is where the game stops, forever. The
//!    count ceiling sits just under it, round, and also bounds what no
//!    patch widens — the series thumbnail arc the game loads whole at boot
//!    (~18.5 KiB of VRAM per song), the one-time capture pass after indexing
//!    (`fast_bootup`; ~48 s per 14k songs under Wine, then replayed from
//!    cache), the wheel. Set from the 14,138-song synthetic cabinet run
//!    (2026-10-26); never per cabinet.
//! 2. **Step-data record pool** — the game's SSQ loader has a fixed record
//!    pool with no overflow guard (`STOCK_RECORD_POOL` 4,096; every song
//!    registers one file per boot, split-timing songs one per level).
//!    Overflowing it corrupts the heap and the boot never leaves "NOW
//!    LOADING 0%" (cabinet run 2026-10-26). `Budget::record_pool` is the
//!    LIVE pool — expanded only when `song_limit_expansion`'s patch landed
//!    before the loader was built — and the selection keeps songs while
//!    their file count fits `record_pool − STOCK_FILE_RESERVE`.
//! 3. **AVS property node cap** — a property tree created without
//!    `PROPERTY_MODE_EXTEND` holds at most 65,535 nodes
//!    (`AVS_ERROR_PROP_NODE_OVER_65535`, 0x80092143): the parse fails, the
//!    music DB comes back EMPTY and the boot crashes in the step-data
//!    actor (cabinet run 2026-10-26, 14,138 songs → `W:prop: read error by
//!    mem[…](80092143)`). The stock document is ~16,200 nodes and every
//!    injected entry is [`NODES_PER_SONG`], so the stock mode admits ~3,800
//!    songs. `song_limit_expansion` ORs the extend flag into the parser's
//!    mode; `Budget::property_extended` says whether that landed, and when
//!    it did not the selection stops at [`node_song_budget`] songs.
//! 4. **Arc loader chunk** — the game's `ArchiveFileCallback` streams every
//!    arc in fixed chunks (`STOCK_ARC_CHUNK_BYTES`, 512 KiB) and parses the
//!    header — cue table + name table — out of the FIRST chunk with no
//!    bounds check; a header past the chunk reads member names out of the
//!    buffer and crashes the boot at the first thumbnail load (cabinet run
//!    2026-10-26: 14,138-member arc, 750 KB header, AV in the name copy at
//!    the HARDWARE_CHECK → PRE_SPLASH transition). `Budget::arc_chunk_bytes`
//!    is the LIVE chunk (4 MiB when `song_limit_expansion`'s patch landed
//!    before `onBoot`), and the selection keeps songs while the thumbnail
//!    arc's header ([`thumbnail_header_cost`] per song) fits it.
//! 5. **GS texture table** — every wheel thumbnail is one resident texture
//!    and the graphics system's table holds `STOCK_TEXTURE_SLOTS` (4,096)
//!    of them, stock thumbnails and UI included; a full table hands the DDS
//!    callback handle 0 and the boot dies on the I/O worker (cabinet run
//!    2026-10-26). `Budget::texture_slots` is the LIVE table (32,768 — the
//!    handle format's limit — when `song_limit_expansion`'s patch landed
//!    before `Renderer::initGs`), and the selection keeps at most
//!    `texture_slots − STOCK_TEXTURE_RESERVE` songs.
//! 6. **Arena text budget** — the AVS property arena the game parses the
//!    whole document into (`mods::song_limit_expansion`). The tree costs
//!    roughly the text; the stock 716 KB file fits the stock 1 MiB arena, so
//!    the tree-over-text factor is ≤ 1.5 and [`text_budget`] admits
//!    `arena × 2/3` bytes of document. With the stock arena (patch failed or
//!    the mod is off) nothing is injected: the stock document already uses
//!    most of it and the headroom is unknown. With the expanded arena this
//!    budget never binds below the count ceiling; it is the safety net for a
//!    large operator omnimix overlay or a future count raise.
//!
//! Selection order is the sorted `pack/folder` key, so the same subset is
//! kept on every boot regardless of index completion order, and the
//! operator can predict it: the first packs alphabetically stay.

use std::cmp::Ordering;

use super::model::SongEntry;

/// Count ceiling (see the module doc): just under the texture table's hard
/// limit (`EXPANDED_TEXTURE_SLOTS − STOCK_TEXTURE_RESERVE` = 30,208). The
/// other patched limits admit more (arena ≈ 79k, arc header ≈ 80k, record
/// pool 129k files); tested to 14,138 (2026-10-26). The thumbnail arc at
/// this count is ~555 MB on disk and in VRAM.
pub const MAX_INJECTED_SONGS: usize = 30_000;
/// The stock property arena (the imm32 at every parser site); at or below
/// this nothing is injected.
pub const STOCK_ARENA_BYTES: usize = 0x0010_0000;
/// The arena `mods::song_limit_expansion` patches in (64 MiB; AGCS heap,
/// freed after the parse). Defined here, in a mountable file, so the prep
/// tool's ceiling report and the DLL's patch share one number.
pub const EXPANDED_ARENA_BYTES: usize = 0x0400_0000;
/// The game's step-data record pool: one record per SSQ file registered at
/// boot, 4,096 stock, with NO overflow guard (an exhausted pool corrupts
/// the heap and the boot never leaves "NOW LOADING 0%").
pub const STOCK_RECORD_POOL: usize = 0x1000;
/// The pool `mods::song_limit_expansion` patches in (0x40 + 0xA0 bytes per
/// record → ~29 MB).
pub const EXPANDED_RECORD_POOL: usize = 0x2_0000;
/// Records the stock music DB and the game's own loads need: ~1,500 stock
/// SSQs plus the hardcoded split-file specials, with headroom.
pub const STOCK_FILE_RESERVE: usize = 2_048;
/// Nodes an AVS property may hold without `PROPERTY_MODE_EXTEND`
/// (element and attribute nodes together; `__type`/`__count` are node
/// metadata, not nodes).
pub const STOCK_PROPERTY_NODE_CAP: usize = 65_535;
/// Element nodes one injected `<music>` entry costs: the entry itself plus
/// its 11 children (`musicdb::entry_xml`; asserted by its tests).
pub const NODES_PER_SONG: usize = 12;
/// Nodes the stock document uses (16,214 on 20260915) plus headroom for an
/// operator omnimix overlay.
pub const STOCK_NODE_RESERVE: usize = 20_000;
/// The arc loader's stock chunk: the most header one arc may carry.
pub const STOCK_ARC_CHUNK_BYTES: usize = 0x8_0000;
/// The chunk `mods::song_limit_expansion` patches in (4 MiB; the loader's
/// 4-slot pool grows to 16 MiB with it).
pub const EXPANDED_ARC_CHUNK_BYTES: usize = 0x40_0000;
/// Fixed arc header bytes (magic, version, count, compression).
pub const ARC_HEADER_FIXED: usize = 16;
/// Per-member header bytes besides the basename: a 16-byte cue plus the
/// `data/jacket/thumbnail/` + `_tn.dds` wrapping and the NUL
/// (`core::art::arcpack::thumbnail_member`; asserted by its tests).
pub const THUMBNAIL_MEMBER_OVERHEAD: usize = 16 + 22 + 7 + 1;
/// The gs texture table (and TextureData pool) `Renderer::initGs` builds.
pub const STOCK_TEXTURE_SLOTS: usize = 0x1000;
/// The table `mods::song_limit_expansion` patches in: the most the
/// 15-bit slot field of a texture handle can address.
pub const EXPANDED_TEXTURE_SLOTS: usize = 0x8000;
/// Textures the stock game needs resident at the wheel: ~1,500 stock
/// thumbnails plus UI/sprite sheets, with headroom.
pub const STOCK_TEXTURE_RESERVE: usize = 2_560;
/// Tree-over-text safety factor, as `text ≤ arena × NUM / DEN`.
const ARENA_TEXT_NUM: usize = 2;
const ARENA_TEXT_DEN: usize = 3;

/// Document bytes the arena may safely hold.
#[must_use]
pub const fn text_budget(arena_bytes: usize) -> usize {
    arena_bytes / ARENA_TEXT_DEN * ARENA_TEXT_NUM
}

/// Songs a stock-mode (non-extended) property tree admits after the stock
/// document: `(65,535 − reserve) / nodes per song` = 3,794.
#[must_use]
pub const fn node_song_budget() -> usize {
    (STOCK_PROPERTY_NODE_CAP - STOCK_NODE_RESERVE) / NODES_PER_SONG
}

/// What limits the selection worked under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// The arena the game's parser will use this boot.
    pub arena_bytes: usize,
    /// The count ceiling (normally [`MAX_INJECTED_SONGS`]).
    pub max_songs: usize,
    /// The step-data record pool the game's loader has this boot.
    pub record_pool: usize,
    /// Whether the music-DB parser creates its property tree with
    /// `PROPERTY_MODE_EXTEND` this boot (no 65,535-node cap).
    pub property_extended: bool,
    /// The arc loader's chunk this boot: the thumbnail arc's header must
    /// fit it.
    pub arc_chunk_bytes: usize,
    /// GS texture slots this boot: one per injected thumbnail.
    pub texture_slots: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            arena_bytes: STOCK_ARENA_BYTES,
            max_songs: MAX_INJECTED_SONGS,
            record_pool: STOCK_RECORD_POOL,
            property_extended: false,
            arc_chunk_bytes: STOCK_ARC_CHUNK_BYTES,
            texture_slots: STOCK_TEXTURE_SLOTS,
        }
    }
}

impl Budget {
    /// Every patch landed (the tool's assumption when reporting).
    #[must_use]
    pub fn expanded() -> Self {
        Self {
            arena_bytes: EXPANDED_ARENA_BYTES,
            max_songs: MAX_INJECTED_SONGS,
            record_pool: EXPANDED_RECORD_POOL,
            property_extended: true,
            arc_chunk_bytes: EXPANDED_ARC_CHUNK_BYTES,
            texture_slots: EXPANDED_TEXTURE_SLOTS,
        }
    }

    /// Songs whose thumbnails fit the texture table after the stock set.
    #[must_use]
    pub fn texture_budget(&self) -> usize {
        self.texture_slots.saturating_sub(STOCK_TEXTURE_RESERVE)
    }

    /// Thumbnail-arc header bytes the member table may use.
    #[must_use]
    pub fn arc_header_budget(&self) -> usize {
        self.arc_chunk_bytes.saturating_sub(ARC_HEADER_FIXED)
    }

    /// SSQ files the injected set may register: the pool minus the stock
    /// reserve.
    #[must_use]
    pub fn file_budget(&self) -> usize {
        self.record_pool.saturating_sub(STOCK_FILE_RESERVE)
    }

    /// Songs the property tree admits: unbounded in extended mode, else
    /// [`node_song_budget`].
    #[must_use]
    pub fn node_budget(&self) -> usize {
        if self.property_extended {
            usize::MAX
        } else {
            node_song_budget()
        }
    }
}

/// Thumbnail-arc header bytes one song costs: its cue plus its member name.
#[must_use]
pub fn thumbnail_header_cost(entry: &SongEntry) -> usize {
    THUMBNAIL_MEMBER_OVERHEAD + entry.basename.len()
}

/// SSQ files a song registers at boot: one, or one per split level.
#[must_use]
pub fn files_of(entry: &SongEntry) -> usize {
    if entry.has_split_files() {
        entry
            .split_levels
            .iter()
            .filter(|&&m| m != 0)
            .count()
            .max(1)
    } else {
        1
    }
}

/// The selection outcome.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Selection {
    /// Indices into the input slice of the songs to inject, in selection
    /// (sorted-key) order.
    pub kept: Vec<usize>,
    /// Songs left out by the count ceiling.
    pub over_count: usize,
    /// Songs left out because their SSQ files would overflow the step-data
    /// record pool (`Budget::file_budget`).
    pub over_pool: usize,
    /// Songs left out because the property tree is in stock (non-extended)
    /// mode and would pass 65,535 nodes (`Budget::node_budget`).
    pub over_nodes: usize,
    /// Songs left out because the thumbnail arc's header would outgrow the
    /// arc loader's chunk (`Budget::arc_header_budget`).
    pub over_arc: usize,
    /// Songs left out because their thumbnails would overflow the gs
    /// texture table (`Budget::texture_budget`).
    pub over_textures: usize,
    /// The arena is stock-sized: everything was left out.
    pub arena_unpatched: bool,
}

impl Selection {
    #[must_use]
    pub fn left_out(&self) -> usize {
        self.over_count + self.over_pool + self.over_nodes + self.over_arc + self.over_textures
    }
}

fn key_cmp(a: &SongEntry, b: &SongEntry) -> Ordering {
    a.pack
        .cmp(&b.pack)
        .then_with(|| a.folder.cmp(&b.folder))
        .then_with(|| a.basename.cmp(&b.basename))
}

/// [`select`] over the records the DLL loads: `audio_failed` ones are not
/// loaded (the tool rejects them), so they take no ceiling slot. The tool
/// uses this so its kept set — and the thumbnail arc digest — equals the
/// DLL's.
#[must_use]
pub fn select_loadable<T: std::borrow::Borrow<SongEntry>>(
    entries: &[T],
    budget: &Budget,
) -> Selection {
    let mut sel = Selection::default();
    if budget.arena_bytes <= STOCK_ARENA_BYTES {
        sel.arena_unpatched = true;
        return sel;
    }
    let mut order: Vec<usize> = (0..entries.len())
        .filter(|&i| !entries[i].borrow().audio_failed)
        .collect();
    order.sort_by(|&a, &b| key_cmp(entries[a].borrow(), entries[b].borrow()));
    take(entries, order, budget, &mut sel);
    sel
}

/// Walk `order`, keeping songs while the count ceiling, the node budget,
/// the texture budget, the arc-header budget and the file budget all hold;
/// the rest are counted by which limit stopped them (in that order).
fn take<T: std::borrow::Borrow<SongEntry>>(
    entries: &[T],
    order: Vec<usize>,
    budget: &Budget,
    sel: &mut Selection,
) {
    let file_budget = budget.file_budget();
    let node_budget = budget.node_budget();
    let header_budget = budget.arc_header_budget();
    let texture_budget = budget.texture_budget();
    let mut files = 0usize;
    let mut header = 0usize;
    for i in order {
        let e = entries[i].borrow();
        if sel.kept.len() >= budget.max_songs {
            sel.over_count += 1;
            continue;
        }
        if sel.kept.len() >= node_budget {
            sel.over_nodes += 1;
            continue;
        }
        if sel.kept.len() >= texture_budget {
            sel.over_textures += 1;
            continue;
        }
        let h = thumbnail_header_cost(e);
        if header + h > header_budget {
            sel.over_arc += 1;
            continue;
        }
        let f = files_of(e);
        if files + f > file_budget {
            sel.over_pool += 1;
            continue;
        }
        files += f;
        header += h;
        sel.kept.push(i);
    }
}

/// Choose the songs to inject under `budget`. Deterministic for a given
/// set of entries (sorted by `pack/folder`, input order irrelevant).
#[must_use]
pub fn select<T: std::borrow::Borrow<SongEntry>>(entries: &[T], budget: &Budget) -> Selection {
    let mut sel = Selection::default();
    if budget.arena_bytes <= STOCK_ARENA_BYTES {
        sel.arena_unpatched = true;
        return sel;
    }
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|&a, &b| key_cmp(entries[a].borrow(), entries[b].borrow()));
    take(entries, order, budget, &mut sel);
    sel
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pack: &str, folder: &str, n: u32) -> SongEntry {
        SongEntry {
            pack: pack.into(),
            folder: folder.into(),
            basename: format!("z{n:05}"),
            mcode: 9_000_000 + n,
            levels: [0, 0, 0, 0, 5, 0, 0, 0, 0, 0],
            ..SongEntry::default()
        }
    }

    const EXPANDED: usize = EXPANDED_ARENA_BYTES;

    #[test]
    fn text_budget_is_two_thirds() {
        assert_eq!(text_budget(0x0400_0000), 0x0400_0000 / 3 * 2);
        assert_eq!(text_budget(3), 2);
        // The stock arena would admit less than the stock document.
        assert!(text_budget(STOCK_ARENA_BYTES) < 716_552);
    }

    #[test]
    fn stock_arena_injects_nothing() {
        let e = [entry("A", "1", 1), entry("A", "2", 2)];
        let sel = select(
            &e,
            &Budget {
                arena_bytes: STOCK_ARENA_BYTES,
                max_songs: 10,
                record_pool: EXPANDED_RECORD_POOL,
                property_extended: true,
                arc_chunk_bytes: EXPANDED_ARC_CHUNK_BYTES,
                texture_slots: EXPANDED_TEXTURE_SLOTS,
            },
        );
        assert!(sel.arena_unpatched);
        assert!(sel.kept.is_empty());
        assert_eq!(sel.over_count, 0);
        assert!(select(&e, &Budget::default()).arena_unpatched);
    }

    #[test]
    fn selection_is_sorted_by_key_and_order_independent() {
        let a = entry("Pack B", "song", 1);
        let b = entry("Pack A", "zeta", 2);
        let c = entry("Pack A", "alpha", 3);
        let budget = Budget {
            arena_bytes: EXPANDED,
            max_songs: 10,
            record_pool: EXPANDED_RECORD_POOL,
            property_extended: true,
            arc_chunk_bytes: EXPANDED_ARC_CHUNK_BYTES,
            texture_slots: EXPANDED_TEXTURE_SLOTS,
        };
        let s1 = select(&[a.clone(), b.clone(), c.clone()], &budget);
        assert_eq!(s1.kept, vec![2, 1, 0]);
        let s2 = select(&[c, b, a], &budget);
        assert_eq!(s2.kept, vec![0, 1, 2]);
        assert_eq!(s1.over_count, 0);
        assert!(!s1.arena_unpatched);
    }

    #[test]
    fn count_ceiling_keeps_the_first_keys() {
        let entries: Vec<SongEntry> = (0..10u32)
            .rev()
            .map(|i| entry("P", &format!("{i:02}"), i))
            .collect();
        let sel = select(
            &entries,
            &Budget {
                arena_bytes: EXPANDED,
                max_songs: 4,
                record_pool: EXPANDED_RECORD_POOL,
                property_extended: true,
                arc_chunk_bytes: EXPANDED_ARC_CHUNK_BYTES,
                texture_slots: EXPANDED_TEXTURE_SLOTS,
            },
        );
        assert_eq!(sel.over_count, 6);
        let kept: Vec<&str> = sel
            .kept
            .iter()
            .map(|&i| entries[i].folder.as_str())
            .collect();
        assert_eq!(kept, ["00", "01", "02", "03"]);
        // Removing a kept pack lets the next keys in — predictable for the
        // operator.
        let fewer: Vec<SongEntry> = entries[..8].to_vec(); // folders 09..02
        let sel2 = select(
            &fewer,
            &Budget {
                arena_bytes: EXPANDED,
                max_songs: 4,
                record_pool: EXPANDED_RECORD_POOL,
                property_extended: true,
                arc_chunk_bytes: EXPANDED_ARC_CHUNK_BYTES,
                texture_slots: EXPANDED_TEXTURE_SLOTS,
            },
        );
        let kept2: Vec<&str> = sel2
            .kept
            .iter()
            .map(|&i| fewer[i].folder.as_str())
            .collect();
        assert_eq!(kept2, ["02", "03", "04", "05"]);
    }

    #[test]
    fn record_pool_caps_by_files_not_songs() {
        // Pool 2,048 + 3 → a file budget of 3; a split song costs its levels.
        let mut split = entry("A", "split", 1);
        split.split_levels = [0, 2, 0, 4, 0]; // two files
        let e = [split, entry("B", "one", 2), entry("C", "two", 3)];
        let sel = select(
            &e,
            &Budget {
                arena_bytes: EXPANDED,
                max_songs: 10,
                record_pool: STOCK_FILE_RESERVE + 3,
                property_extended: true,
                arc_chunk_bytes: EXPANDED_ARC_CHUNK_BYTES,
                texture_slots: EXPANDED_TEXTURE_SLOTS,
            },
        );
        assert_eq!(sel.kept, vec![0, 1]); // 2 + 1 files; "C" would be the 4th
        assert_eq!(sel.over_pool, 1);
        assert_eq!(sel.over_count, 0);
        assert_eq!(sel.left_out(), 1);
        // The stock pool admits ~2,000 single-file songs, never more.
        let many: Vec<SongEntry> = (0..3000u32)
            .map(|i| entry("P", &format!("{i:05}"), i))
            .collect();
        let sel = select(&many, &Budget::default());
        assert!(sel.arena_unpatched); // stock arena → nothing at all
                                      // (expanded texture table so the pool is the binding limit)
        let sel = select(
            &many,
            &Budget {
                arena_bytes: EXPANDED,
                texture_slots: EXPANDED_TEXTURE_SLOTS,
                ..Budget::default()
            },
        );
        assert_eq!(sel.kept.len(), STOCK_RECORD_POOL - STOCK_FILE_RESERVE);
        assert_eq!(
            sel.over_pool,
            3000 - (STOCK_RECORD_POOL - STOCK_FILE_RESERVE)
        );
        assert_eq!(files_of(&many[0]), 1);
    }

    #[test]
    fn stock_property_mode_caps_by_nodes() {
        let budget = Budget {
            arena_bytes: EXPANDED,
            record_pool: EXPANDED_RECORD_POOL,
            property_extended: false,
            arc_chunk_bytes: EXPANDED_ARC_CHUNK_BYTES,
            texture_slots: EXPANDED_TEXTURE_SLOTS,
            ..Budget::default()
        };
        assert_eq!(budget.node_budget(), node_song_budget());
        assert_eq!(node_song_budget(), (65_535 - 20_000) / 12);
        let n = node_song_budget() + 5;
        let many: Vec<SongEntry> = (0..n as u32)
            .map(|i| entry("P", &format!("{i:05}"), i))
            .collect();
        let sel = select(&many, &budget);
        assert_eq!(sel.kept.len(), node_song_budget());
        assert_eq!(sel.over_nodes, 5);
        assert_eq!(sel.over_pool, 0);
        assert_eq!(sel.left_out(), 5);
        // Extended mode lifts it entirely.
        let sel = select(&many, &Budget::expanded());
        assert_eq!(sel.kept.len(), n);
        assert_eq!(sel.over_nodes, 0);
        // The stock document plus the budget stays under the cap.
        assert!(
            STOCK_NODE_RESERVE + node_song_budget() * NODES_PER_SONG <= STOCK_PROPERTY_NODE_CAP
        );
    }

    #[test]
    fn stock_arc_chunk_caps_by_header_bytes() {
        // 6-char basenames: 46 + 6 = 52 header bytes each.
        let e = entry("A", "x", 1);
        assert_eq!(thumbnail_header_cost(&e), THUMBNAIL_MEMBER_OVERHEAD + 6);
        let many: Vec<SongEntry> = (0..12_000u32)
            .map(|i| entry("P", &format!("{i:05}"), i))
            .collect();
        let budget = Budget {
            arc_chunk_bytes: STOCK_ARC_CHUNK_BYTES,
            ..Budget::expanded()
        };
        let sel = select(&many, &budget);
        let per = THUMBNAIL_MEMBER_OVERHEAD + 6;
        let fit = (STOCK_ARC_CHUNK_BYTES - ARC_HEADER_FIXED) / per;
        assert_eq!(sel.kept.len(), fit);
        assert_eq!(sel.over_arc, 12_000 - fit);
        assert!(fit > 10_000 && fit < 10_100, "{fit}");
        // The expanded chunk takes them all.
        let sel = select(&many, &Budget::expanded());
        assert_eq!(sel.kept.len(), 12_000);
        assert_eq!(sel.over_arc, 0);
    }

    #[test]
    fn stock_texture_table_caps_by_thumbnails() {
        let many: Vec<SongEntry> = (0..3000u32)
            .map(|i| entry("P", &format!("{i:05}"), i))
            .collect();
        let budget = Budget {
            texture_slots: STOCK_TEXTURE_SLOTS,
            ..Budget::expanded()
        };
        let sel = select(&many, &budget);
        assert_eq!(sel.kept.len(), STOCK_TEXTURE_SLOTS - STOCK_TEXTURE_RESERVE);
        assert_eq!(
            sel.over_textures,
            3000 - (STOCK_TEXTURE_SLOTS - STOCK_TEXTURE_RESERVE)
        );
        let sel = select(&many, &Budget::expanded());
        assert_eq!(sel.over_textures, 0);
        assert_eq!(sel.kept.len(), 3000);
    }

    #[test]
    fn default_ceiling_is_sane() {
        // The document at the ceiling stays inside the expanded arena's
        // text budget even with a 2 MiB operator overlay.
        assert!(MAX_INJECTED_SONGS * 600 + 2 * 1024 * 1024 < text_budget(EXPANDED));
        // And the mcode window can number every song.
        assert!(MAX_INJECTED_SONGS < 1_000_000);
        // The expanded pool holds the ceiling even if every song had three
        // split files, plus the stock reserve.
        assert!(MAX_INJECTED_SONGS * 3 + STOCK_FILE_RESERVE <= EXPANDED_RECORD_POOL);
        // And the expanded texture table holds the ceiling after the stock set.
        assert!(MAX_INJECTED_SONGS <= EXPANDED_TEXTURE_SLOTS - STOCK_TEXTURE_RESERVE);
        // And the thumbnail arc header at the ceiling (16-char basenames)
        // fits the expanded chunk.
        assert!(
            ARC_HEADER_FIXED + MAX_INJECTED_SONGS * (THUMBNAIL_MEMBER_OVERHEAD + 16)
                <= EXPANDED_ARC_CHUNK_BYTES
        );
    }
}
