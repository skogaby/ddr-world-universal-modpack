//! Conversion glue shared by the runtime index and `prep/` (the StepMania prep tool) —
//! pure (std + `crate::core`).
//!
//! Folder discovery (`find_simfile`, `find_audio`), the chart-timing read
//! that feeds `choose_lead_in` (`chart_timings`), display-text and BPM
//! selection, and the art-source resolution order of design §4.6. Nothing
//! here logs or touches game state; the index wraps each call with its own
//! reject reasons.

use std::fs;
use std::path::{Path, PathBuf};

use crate::core::art::source::{classify_name, is_image_name, SourceKind};
use crate::core::sm::model::Song;
use crate::core::sm::scroll::ScrollGimmicks;
use crate::core::sm::title_glyphs;
use crate::core::ssq::ssq_chunk::find_chunk;
use crate::core::ssq::timing::TempoConverter;
use crate::core::ssq::writer::{slot_code, slot_index, SsqBundle, SsqFile};

use super::model::{SLOTS, SPLIT_FILES};

/// Audio containers the loader accepts, in StepMania's preference order.
pub const AUDIO_EXTS: [&str; 3] = ["ogg", "mp3", "wav"];

/// `.ssc` preferred over `.sm` (StepMania's own rule); case-insensitive.
#[must_use]
pub fn find_simfile(dir: &Path) -> Option<PathBuf> {
    let mut ssc = None;
    let mut sm = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("ssc") if ssc.is_none() => ssc = Some(p),
            Some("sm") if sm.is_none() => sm = Some(p),
            _ => {}
        }
    }
    ssc.or(sm)
}

/// Files directly inside `dir`.
#[must_use]
pub fn list_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = fs::read_dir(dir)
        .ok()
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn file_name_lower(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_ascii_lowercase())
        .unwrap_or_default()
}

/// A tag's bare file name, lowercased (`../foo/Bar.PNG` → `bar.png`); `None`
/// when empty.
fn tag_file_name(tag: &str) -> Option<String> {
    let normalised = tag.replace('\\', "/");
    let name = normalised.rsplit('/').next().unwrap_or("").trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_ascii_lowercase())
    }
}

/// `#MUSIC` resolved case-insensitively inside the folder, else the audio
/// file there (one, or the first by StepMania's ogg > mp3 > wav preference).
#[must_use]
pub fn find_audio(dir: &Path, music_tag: &str) -> Option<PathBuf> {
    let entries = list_files(dir);
    if let Some(wanted) = tag_file_name(music_tag) {
        if let Some(p) = entries.iter().find(|p| file_name_lower(p) == wanted) {
            return Some(p.clone());
        }
    }
    let audio: Vec<&PathBuf> = entries
        .iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        })
        .collect();
    if audio.len() == 1 {
        return Some(audio[0].clone());
    }
    for ext in AUDIO_EXTS {
        if let Some(p) = audio.iter().find(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        }) {
            return Some((*p).clone());
        }
    }
    None
}

/// `(min tempo_data[0], min first-note ms)` over every file and declared
/// slot of a bundle written with `lead_in_ms = 0` — the inputs of
/// `core::audio::lead_in::choose_lead_in`.
#[must_use]
pub fn chart_timings(bundle: &SsqBundle) -> (i32, i32) {
    let mut td0 = i32::MAX;
    let mut first = i32::MAX;
    for file in &bundle.files {
        let Some(conv) = TempoConverter::from_ssq(&file.bytes) else {
            continue;
        };
        if let Some((_, ms)) = conv.entries().next() {
            td0 = td0.min(ms);
        }
        for (slot, _) in &file.charts {
            let Some(code) = slot_code(*slot) else {
                continue;
            };
            let Some(chunk) = find_chunk(&file.bytes, 3, code) else {
                continue;
            };
            if chunk.param3 == 0 || chunk.body.len() < 4 {
                continue;
            }
            let tick =
                i32::from_le_bytes([chunk.body[0], chunk.body[1], chunk.body[2], chunk.body[3]]);
            first = first.min(conv.beat_to_music_count(tick));
        }
    }
    (
        if td0 == i32::MAX { 0 } else { td0 },
        if first == i32::MAX { 0 } else { first },
    )
}

/// What the index declares for a song from the SSQ files that passed their
/// self-check (design §4.2): the musicdb levels per slot, the split table
/// the `ssq_path` resolver answers from, and the committed slots in file
/// order (for the scroll-gimmick resolution).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Declaration {
    /// Level per musicdb slot (clamped 1..=19); 0 = not declared.
    pub levels: [u8; SLOTS],
    /// `split_levels[N − 1]` = bitmask of levels held by `<b>_N.ssq`.
    pub split_levels: [u8; SPLIT_FILES],
    /// Every declared slot, in file order.
    pub slots: Vec<u8>,
}

impl Declaration {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.levels.iter().all(|&l| l == 0)
    }
}

/// Fold the charts of `files` (the ones that passed `check_ssq`) into a
/// [`Declaration`]. A chart in `<b>_N.ssq` (`index` 1..=5) marks bit
/// `slot % 5` of `split_levels[N − 1]`; the base file (`index` 0) marks none.
#[must_use]
pub fn declare(files: &[&SsqFile]) -> Declaration {
    let mut d = Declaration::default();
    for file in files {
        for (slot, level) in &file.charts {
            if let Some(l) = d.levels.get_mut(usize::from(*slot)) {
                *l = (*level).clamp(1, 19);
            }
            d.slots.push(*slot);
            if (1..=SPLIT_FILES as u8).contains(&file.index) {
                d.split_levels[usize::from(file.index - 1)] |= 1 << (slot % 5);
            }
        }
    }
    d
}

/// `<b>.ssq` or `<b>_N.ssq`.
#[must_use]
pub fn ssq_file_name(basename: &str, index: u8) -> String {
    if index == 0 {
        format!("{basename}.ssq")
    } else {
        format!("{basename}_{index}.ssq")
    }
}

/// `tempo_data[0]` of a written file — the music count of beat 0 (the
/// renderer's `+0xAC` clock reads in this domain). `0` when unreadable.
#[must_use]
pub fn beat0_ms(file: &SsqFile) -> i32 {
    TempoConverter::from_ssq(&file.bytes)
        .map(|c| c.beat_to_music_count(0))
        .unwrap_or(0)
}

/// What `committed_scroll_gimmicks` decided for one song.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScrollResolution {
    /// Tables to publish at gameplay load; `None` when every committed chart
    /// is identity.
    pub gimmicks: Option<ScrollGimmicks>,
    /// Set when the committed charts carry *different* tables: the song-level
    /// tables were used instead (the renderer seam has no difficulty context).
    pub conflict: Option<String>,
}

/// Resolve the `#SCROLLS`/`#SPEEDS` tables of the charts declared in
/// `file` into the one set the renderer seam can apply per basename. See
/// [`committed_scroll_gimmicks_for_slots`].
#[must_use]
pub fn committed_scroll_gimmicks(song: &Song, file: &SsqFile) -> ScrollResolution {
    let slots: Vec<u8> = file.charts.iter().map(|c| c.0).collect();
    committed_scroll_gimmicks_for_slots(song, &slots)
}

/// Resolve the `#SCROLLS`/`#SPEEDS` tables of the charts committed to the
/// given musicdb slots (across every file of the song — one `<b>.ssq` or
/// several `<b>_N.ssq`) into the one set the renderer seam can apply per
/// basename. Every chart's tables are built through its own effective
/// timing. One distinct non-identity set → that set; all identity → `None`;
/// several distinct sets → the song-level tables with `conflict` naming the
/// slots (StepMania applies per-chart tables, but `get_offset_y` cannot
/// tell which difficulty is being drawn).
#[must_use]
pub fn committed_scroll_gimmicks_for_slots(song: &Song, slots: &[u8]) -> ScrollResolution {
    let mut sets: Vec<(u8, ScrollGimmicks)> = Vec::new();
    for slot in slots {
        let Some(chart) = song
            .charts
            .iter()
            .find(|c| slot_index(c.style, c.difficulty) == Some(*slot))
        else {
            continue;
        };
        let g = ScrollGimmicks::new(
            chart.effective_scrolls(song),
            chart.effective_speeds(song),
            chart.effective_timing(&song.timing),
        );
        sets.push((*slot, g));
    }
    let mut distinct: Vec<&ScrollGimmicks> = Vec::new();
    for (_, g) in &sets {
        if !distinct.contains(&g) {
            distinct.push(g);
        }
    }
    match distinct.as_slice() {
        [] => ScrollResolution::default(),
        [one] => ScrollResolution {
            gimmicks: (!one.is_identity()).then(|| (*one).clone()),
            conflict: None,
        },
        _ => {
            let song_level = ScrollGimmicks::new(&song.scrolls, &song.speeds, &song.timing);
            let slots: Vec<String> = sets.iter().map(|(s, _)| s.to_string()).collect();
            ScrollResolution {
                gimmicks: (!song_level.is_identity()).then_some(song_level),
                conflict: Some(format!(
                    "charts in slots [{}] carry different #SCROLLS/#SPEEDS; using the song-level tables",
                    slots.join(", ")
                )),
            }
        }
    }
}

/// Display title: the native `#TITLE` when the game's song-title fonts can
/// draw every character of it (`core::sm::title_glyphs`, extracted from the
/// KBF glyph tables), else `#TITLETRANSLIT`, else the native title anyway;
/// the subtitle (same rule) is appended after a space when present.
/// Whitespace-trimmed; the caller substitutes the folder name when this is
/// empty.
#[must_use]
pub fn display_title(song: &Song) -> String {
    let title = pick(&song.title_translit, &song.title);
    let subtitle = pick(&song.subtitle_translit, &song.subtitle);
    join_title(title, subtitle)
}

/// Sort/filter key source for the musicdb `title_yomi`: the transliterated
/// title (+ subtitle) when present, else the native one. Stock yomi is
/// ASCII, so a CJK display title still sorts by its romanisation.
#[must_use]
pub fn sort_title(song: &Song) -> String {
    let title = pick_translit(&song.title_translit, &song.title);
    let subtitle = pick_translit(&song.subtitle_translit, &song.subtitle);
    join_title(title, subtitle)
}

/// Display artist: native `#ARTIST` when drawable, else `#ARTISTTRANSLIT`,
/// else native; trimmed.
#[must_use]
pub fn display_artist(song: &Song) -> String {
    pick(&song.artist_translit, &song.artist).trim().to_string()
}

fn join_title(title: &str, subtitle: &str) -> String {
    let mut out = title.trim().to_string();
    let sub = subtitle.trim();
    if !sub.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(sub);
    }
    out
}

/// Native when every character has a title-font glyph, else the
/// transliteration when it has one, else native.
fn pick<'a>(translit: &'a str, native: &'a str) -> &'a str {
    if title_glyphs::renderable(native.trim()) || translit.trim().is_empty() {
        native
    } else {
        translit
    }
}

/// Transliteration when present, else native.
fn pick_translit<'a>(translit: &'a str, native: &'a str) -> &'a str {
    if translit.trim().is_empty() {
        native
    } else {
        translit
    }
}

/// `(bpm_min, bpm_max)` for the musicdb: `#DISPLAYBPM` when numeric and
/// positive, else the positive BPMs of the song tempo map; clamped to
/// 1..=999 (the wheel's digit art).
#[must_use]
pub fn bpm_range(song: &Song) -> (u16, u16) {
    let clamp = |v: f64| -> u16 {
        if v.is_finite() {
            v.round().clamp(1.0, 999.0) as u16
        } else {
            1
        }
    };
    if let Some((lo, hi)) = song.display_bpm {
        if lo.is_finite() && hi.is_finite() && lo > 0.0 && hi > 0.0 {
            let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
            return (clamp(lo), clamp(hi));
        }
    }
    let mut lo = f64::INFINITY;
    let mut hi = 0.0f64;
    for (_, bpm) in &song.timing.bpms {
        let v = bpm.as_rational().as_f64();
        if v > 0.0 && v.is_finite() {
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    if hi <= 0.0 {
        return (120, 120);
    }
    (clamp(lo), clamp(hi))
}

/// How a jacket source was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtPick {
    /// `None` = decide by aspect ratio when decoding.
    pub kind: Option<SourceKind>,
    /// Absolute/relative path as found on disk.
    pub path: PathBuf,
}

/// Resolve a tag like `../pack_bn.png` against `dir` lexically (no
/// filesystem canonicalisation), case-insensitively for the final component.
fn resolve_tag(dir: &Path, tag: &str) -> Option<PathBuf> {
    let tag = tag.trim();
    if tag.is_empty() {
        return None;
    }
    let mut p = dir.to_path_buf();
    for comp in tag.replace('\\', "/").split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                if !p.pop() {
                    return None;
                }
            }
            c => p.push(c),
        }
    }
    if p.is_file() {
        return Some(p);
    }
    // Case-insensitive retry on the file name.
    let parent = p.parent()?;
    let wanted = file_name_lower(&p);
    list_files(parent)
        .into_iter()
        .find(|f| file_name_lower(f) == wanted)
}

/// Source order (design §4.6, maintainer 2026-10-23): `#JACKET` →
/// `#BACKGROUND` → `#BANNER` → filename heuristics in the song folder →
/// pack-level art → `None` (placeholder). A 4:3 background is much closer to
/// the square DDR draws than a 418×164 banner, so it outranks the banner.
#[must_use]
pub fn resolve_art(song_dir: &Path, pack_dir: &Path, song: &Song) -> Option<ArtPick> {
    for (tag, kind) in [
        (&song.jacket, SourceKind::Jacket),
        (&song.background, SourceKind::Background),
        (&song.banner, SourceKind::Banner),
    ] {
        if let Some(path) =
            resolve_tag(song_dir, tag).filter(|p| is_image_name(&file_name_lower(p)))
        {
            return Some(ArtPick {
                kind: Some(kind),
                path,
            });
        }
    }
    if let Some(pick) = heuristic_pick(
        song_dir,
        &[
            SourceKind::Jacket,
            SourceKind::Background,
            SourceKind::Banner,
        ],
    ) {
        return Some(pick);
    }
    heuristic_pick(
        pack_dir,
        &[
            SourceKind::Background,
            SourceKind::Banner,
            SourceKind::Jacket,
        ],
    )
}

fn heuristic_pick(dir: &Path, order: &[SourceKind]) -> Option<ArtPick> {
    let images: Vec<PathBuf> = list_files(dir)
        .into_iter()
        .filter(|p| is_image_name(&file_name_lower(p)))
        .collect();
    if images.is_empty() {
        return None;
    }
    for want in order {
        if let Some(p) = images
            .iter()
            .find(|p| classify_name(&file_name_lower(p)) == Some(*want))
        {
            return Some(ArtPick {
                kind: Some(*want),
                path: p.clone(),
            });
        }
    }
    let unclassified: Vec<&PathBuf> = images
        .iter()
        .filter(|p| classify_name(&file_name_lower(p)).is_none())
        .collect();
    if unclassified.len() == 1 {
        return Some(ArtPick {
            kind: None,
            path: unclassified[0].clone(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sm::simfile;
    use crate::core::ssq::writer::{self, WriteOptions};

    #[test]
    fn chart_timings_reads_td0_and_first_note() {
        let text = "#OFFSET:-0.241;\n#BPMS:0=140;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Hard;\n#NOTES:\n0000\n0000\n0000\n0000\n,\n0000\n0000\n0000\n0000\n,\n1000\n0000\n0000\n0000\n;\n";
        let song = simfile::parse_ssc(text);
        let bundle = writer::write_song(&song, &WriteOptions::default()).expect("write");
        let (td0, first) = chart_timings(&bundle);
        assert_eq!(td0, 241);
        assert!((first - (241 + 3429)).abs() <= 1, "{first}");
        let file0 = bundle.files.iter().find(|f| f.index == 0).expect("file0");
        assert_eq!(beat0_ms(file0), 241);
        let shifted = writer::write_song(
            &song,
            &WriteOptions {
                lead_in_ms: 2000,
                ..WriteOptions::default()
            },
        )
        .expect("write");
        assert_eq!(beat0_ms(&shifted.files[0]), 2241);
    }

    fn chart_block(difficulty: &str, extra: &str) -> String {
        format!(
            "#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:{difficulty};\n#METER:5;\n{extra}#NOTES:\n1000\n0000\n0000\n0000\n;\n"
        )
    }

    #[test]
    fn declare_folds_base_and_split_files() {
        // One timing → one base file, no split table.
        let song = simfile::parse_ssc(&format!(
            "#BPMS:0=120;\n{}{}",
            chart_block("Easy", ""),
            chart_block("Hard", "")
        ));
        let b = writer::write_song(&song, &WriteOptions::default()).expect("write");
        let files: Vec<&SsqFile> = b.files.iter().collect();
        let d = declare(&files);
        assert_eq!(d.levels, [0, 5, 0, 5, 0, 0, 0, 0, 0, 0]);
        assert_eq!(d.split_levels, [0; SPLIT_FILES]);
        assert_eq!(d.slots, vec![1, 3]);
        assert!(!d.is_empty());
        // Expert on its own timing → `_N` per level, Rule A answers per slot.
        let song = simfile::parse_ssc(&format!(
            "#BPMS:0=120;\n{}{}{}",
            chart_block("Easy", ""),
            chart_block("Hard", "#BPMS:0=240;\n"),
            chart_block("Challenge", "")
        ));
        let b = writer::write_song(&song, &WriteOptions::default()).expect("write");
        let files: Vec<&SsqFile> = b.files.iter().collect();
        let d = declare(&files);
        assert_eq!(d.levels, [0, 5, 0, 5, 5, 0, 0, 0, 0, 0]);
        assert_eq!(d.split_levels, [0, 1 << 1, 0, 1 << 3, 1 << 4]);
        assert_eq!(d.slots, vec![1, 3, 4]);
        let entry = super::super::model::SongEntry {
            split_levels: d.split_levels,
            ..Default::default()
        };
        assert_eq!(
            (0..5).map(|s| entry.split_file_for(s)).collect::<Vec<_>>(),
            vec![None, Some(2), None, Some(4), Some(5)]
        );
        // A file that failed its check is simply left out: its slot vanishes.
        let only_passed: Vec<&SsqFile> = b.files.iter().filter(|f| f.index != 4).collect();
        let d = declare(&only_passed);
        assert_eq!(d.levels[3], 0);
        assert_eq!(d.split_levels[3], 0);
        assert_eq!(declare(&[]), Declaration::default());
        assert!(declare(&[]).is_empty());
        assert_eq!(ssq_file_name("zab12c", 0), "zab12c.ssq");
        assert_eq!(ssq_file_name("zab12c", 4), "zab12c_4.ssq");
    }

    #[test]
    fn committed_scroll_tables_resolve_per_song() {
        const SCROLLS: &str = "#SCROLLS:0=1,8=0,12=1;\n";
        const SPEEDS: &str = "#SPEEDS:0=1=0=0,16=0.5=4=0;\n";
        let write = |text: &str| {
            let song = simfile::parse_ssc(text);
            let bundle = writer::write_song(&song, &WriteOptions::default()).expect("write");
            let file0 = bundle
                .files
                .iter()
                .find(|f| f.index == 0)
                .cloned()
                .expect("file0");
            (song, file0)
        };
        // No tables anywhere → identity.
        let (song, f0) = write(&format!(
            "#BPMS:0=120;\n{}{}",
            chart_block("Easy", ""),
            chart_block("Hard", "")
        ));
        let r = committed_scroll_gimmicks(&song, &f0);
        assert_eq!(r, ScrollResolution::default());
        // Song-level tables shared by every chart → that set.
        let (song, f0) = write(&format!(
            "#BPMS:0=120;\n{SCROLLS}{SPEEDS}{}{}",
            chart_block("Easy", ""),
            chart_block("Hard", "")
        ));
        let r = committed_scroll_gimmicks(&song, &f0);
        let g = r.gimmicks.expect("tables");
        assert!(r.conflict.is_none());
        assert!((g.scroll.display_beat(10.0) - 8.0).abs() < 1e-9);
        assert!((g.speed.speed_at(18.0, 9.0) - 0.75).abs() < 1e-9);
        // Identity tables authored explicitly → still None.
        let (song, f0) = write(&format!(
            "#BPMS:0=120;\n#SCROLLS:0=1;\n#SPEEDS:0=1=0=0;\n{}",
            chart_block("Hard", "")
        ));
        assert!(committed_scroll_gimmicks(&song, &f0).gimmicks.is_none());
        // Charts disagree → song-level + conflict.
        let (song, f0) = write(&format!(
            "#BPMS:0=120;\n{SCROLLS}{}{}",
            chart_block("Easy", "#SCROLLS:0=1,4=2;\n"),
            chart_block("Hard", "")
        ));
        let r = committed_scroll_gimmicks(&song, &f0);
        assert!(
            r.conflict.as_deref().unwrap_or("").contains("slots [1, 3]"),
            "{r:?}"
        );
        let g = r.gimmicks.expect("song-level");
        assert!((g.scroll.display_beat(10.0) - 8.0).abs() < 1e-9);
        // Only an undeclared Edit chart carries tables → the committed set is identity.
        let (song, f0) = write(&format!(
            "#BPMS:0=120;\n{}{}",
            chart_block("Hard", ""),
            chart_block("Edit", SCROLLS)
        ));
        assert!(committed_scroll_gimmicks(&song, &f0).gimmicks.is_none());
    }

    #[test]
    fn display_text_and_bpm_rules() {
        let song = simfile::parse_ssc(
            "#TITLE:予;\n#TITLETRANSLIT:Yo;\n#SUBTITLE:(Hard);\n#ARTIST:A;\n#DISPLAYBPM:150:170;\n#BPMS:0=140;\n",
        );
        // 予 is in the title fonts → native wins; the yomi source stays romanised.
        assert_eq!(display_title(&song), "予 (Hard)");
        assert_eq!(sort_title(&song), "Yo (Hard)");
        assert_eq!(display_artist(&song), "A");
        let astral = simfile::parse_ssc(
            "#TITLE:Smile \u{1F600};\n#TITLETRANSLIT:Smile;\n#ARTIST:\u{1F600};\n#ARTISTTRANSLIT:Emoji;\n",
        );
        assert_eq!(display_title(&astral), "Smile");
        assert_eq!(display_artist(&astral), "Emoji");
        let no_translit = simfile::parse_ssc("#TITLE:Smile \u{1F600};\n");
        assert_eq!(display_title(&no_translit), "Smile \u{1F600}");
        assert_eq!(bpm_range(&song), (150, 170));
        let plain = simfile::parse_ssc("#TITLE:T;\n#BPMS:0=140,8=180.4;\n");
        assert_eq!(bpm_range(&plain), (140, 180));
        let none = simfile::parse_ssc("#TITLE:T;\n");
        assert_eq!(bpm_range(&none), (120, 120));
    }

    #[test]
    fn simfile_audio_and_art_discovery() {
        let tmp = std::env::temp_dir().join(format!(
            "sm-convert-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let pack = tmp.join("Pack");
        let song_dir = pack.join("Song");
        fs::create_dir_all(&song_dir).unwrap();
        fs::write(song_dir.join("a.sm"), b"#TITLE:x;").unwrap();
        fs::write(song_dir.join("a.SSC"), b"#TITLE:x;").unwrap();
        fs::write(song_dir.join("Track.OGG"), b"").unwrap();
        fs::write(song_dir.join("other.mp3"), b"").unwrap();
        fs::write(pack.join("pack-bn.png"), b"").unwrap();
        assert!(find_simfile(&song_dir).unwrap().ends_with("a.SSC"));
        assert!(find_audio(&song_dir, "track.ogg")
            .unwrap()
            .ends_with("Track.OGG"));
        assert!(find_audio(&song_dir, "").unwrap().ends_with("Track.OGG"));
        let song = simfile::parse_ssc("#BANNER:../pack-BN.png;");
        let pick = resolve_art(&song_dir, &pack, &song).unwrap();
        assert_eq!(pick.kind, Some(SourceKind::Banner));
        assert_eq!(file_name_lower(&pick.path), "pack-bn.png");
        let no_tags = simfile::parse_ssc("#TITLE:x;");
        let pick = resolve_art(&song_dir, &pack, &no_tags).unwrap();
        assert_eq!(pick.kind, Some(SourceKind::Banner));
        // Background outranks banner when both are tagged (ITG-style packs).
        fs::write(song_dir.join("Song-bg.png"), b"").unwrap();
        fs::write(song_dir.join("Song-banner.png"), b"").unwrap();
        let both = simfile::parse_ssc("#BANNER:Song-banner.png;\n#BACKGROUND:Song-bg.png;");
        let pick = resolve_art(&song_dir, &pack, &both).unwrap();
        assert_eq!(pick.kind, Some(SourceKind::Background));
        assert_eq!(file_name_lower(&pick.path), "song-bg.png");
        fs::remove_file(song_dir.join("Song-bg.png")).unwrap();
        fs::remove_file(song_dir.join("Song-banner.png")).unwrap();
        fs::write(song_dir.join("cover.jpg"), b"").unwrap();
        let pick = resolve_art(&song_dir, &pack, &no_tags).unwrap();
        assert_eq!(pick.kind, None);
        assert!(pick.path.ends_with("cover.jpg"));
        let _ = fs::remove_dir_all(&tmp);
    }
}
