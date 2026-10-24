//! `.ssc` / `.sm` → `Song`.
//!
//! Ported from `ddr-chart-tools` `src/ssc/mod.rs` and `src/sm/mod.rs`
//! (a733404). The tool's tag routing is kept: song-level tags fill the
//! `Song`; in SSC each `#NOTEDATA` opens a chart section that captures
//! every later tag; a 7-field `#NOTES` (the `.sm` form, also found at
//! song level in real `.ssc` files) is one chart by itself.
//!
//! Runtime differences (design §4.1), all of which leave the model for
//! an input that needed none of them identical to the tool's:
//! - nothing fails the song: an unsupported `#STEPSTYPE`, a bad note
//!   body or an incomplete section rejects that chart with a reason;
//! - tags and difficulty names are case-insensitive; an unknown
//!   difficulty becomes `Edit` with a warning;
//! - `#BPMS`/`#STOPS`/`#DELAYS`/`#WARPS`/`#FAKES` tolerate blank entries,
//!   trailing commas and scientific notation; a malformed entry is
//!   skipped with a warning; negative BPMs/stops and warps are *parsed*
//!   (the writer decides what they mean);
//! - per-chart `#OFFSET/#BPMS/#STOPS/#DELAYS/#WARPS/#FAKES` are kept as
//!   `Chart.timing` (song timing with each present tag replaced), or
//!   folded back to `None` when they equal the song timing;
//! - `#METER`, `#MUSIC`, the translit/subtitle/art/`#SELECTABLE`/
//!   `#DISPLAYBPM` tags are captured;
//! - `#SCROLLS` / `#SPEEDS` are kept (song level and per `#NOTEDATA`,
//!   `scroll.rs`) instead of dropped.

use super::model::{Beat, Bpm, Chart, Difficulty, Rational, RejectedChart, Song, Warning};
use super::msd::{self, MsdValue};
use super::notes;
use super::scroll::{self, ScrollEntry, SpeedEntry};

/// Parse a simfile, choosing the grammar by the path's extension:
/// `.sm` → [`parse_sm`], anything else → [`parse_ssc`].
#[must_use]
pub fn parse(path_hint: &str, text: &str) -> Song {
    let lower = path_hint.to_ascii_lowercase();
    if lower.ends_with(".sm") {
        parse_sm(text)
    } else {
        parse_ssc(text)
    }
}

/// Parse a complete SSC file.
#[must_use]
pub fn parse_ssc(text: &str) -> Song {
    let values = msd::tokenize(text);
    let mut song = Song {
        selectable: true,
        ..Song::default()
    };
    let mut current: Option<ChartDraft> = None;

    for value in &values {
        let tag = value.tag().trim().to_ascii_uppercase();

        if tag == "NOTEDATA" {
            if let Some(draft) = current.take() {
                draft.finalize(&mut song);
            }
            current = Some(ChartDraft::default());
            continue;
        }

        if let Some(draft) = current.as_mut() {
            draft.apply(&tag, value);
        } else if tag == "NOTES" && value.params.len() >= 7 {
            push_sm_notes(&mut song, value);
        } else {
            apply_song_tag(&mut song, &tag, value);
        }
    }

    if let Some(draft) = current.take() {
        draft.finalize(&mut song);
    }
    song
}

/// Parse a complete `.sm` file. Song-level tags are the SSC ones (plus
/// the pre-SM5 `#FREEZES` alias of `#STOPS`); each `#NOTES` is one chart:
/// `#NOTES:stepstype:description:difficulty:meter:radar:notedata;`.
#[must_use]
pub fn parse_sm(text: &str) -> Song {
    let values = msd::tokenize(text);
    let mut song = Song {
        selectable: true,
        ..Song::default()
    };
    for value in &values {
        let tag = value.tag().trim().to_ascii_uppercase();
        if tag == "NOTES" {
            if value.params.len() < 7 {
                song.rejected.push(RejectedChart {
                    stepstype: value.param(1).trim().to_string(),
                    difficulty: value.param(3).trim().to_string(),
                    reason: format!(
                        "#NOTES has {} fields, expected at least 7",
                        value.params.len()
                    ),
                });
                continue;
            }
            push_sm_notes(&mut song, value);
            continue;
        }
        apply_song_tag(&mut song, &tag, value);
    }
    song
}

/// One `.sm`-form `#NOTES` value (≥ 7 params) → chart or rejection.
fn push_sm_notes(song: &mut Song, value: &MsdValue) {
    let stepstype = value.param(1).trim().to_string();
    let difficulty = value.param(3).trim().to_string();
    let Some(style) = notes::parse_stepstype(&stepstype) else {
        song.rejected.push(RejectedChart {
            stepstype,
            difficulty,
            reason: "unsupported stepstype".to_string(),
        });
        return;
    };
    let mut warnings = Vec::new();
    let diff = resolve_difficulty(&difficulty, &mut warnings);
    let meter = parse_meter(value.param(4));
    match notes::parse_notes_body(value.param(6), style, &mut warnings) {
        Ok(notes) => song.charts.push(Chart {
            style,
            difficulty: diff,
            meter,
            description: value.param(2).trim().to_string(),
            timing: None,
            scrolls: None,
            speeds: None,
            notes,
            warnings,
        }),
        Err(e) => song.rejected.push(RejectedChart {
            stepstype,
            difficulty,
            reason: e.0,
        }),
    }
}

fn resolve_difficulty(raw: &str, warnings: &mut Vec<Warning>) -> Difficulty {
    match notes::parse_difficulty(raw) {
        Some(d) => d,
        None => {
            warnings.push(Warning(format!(
                "unknown difficulty {raw:?}; treated as Edit"
            )));
            Difficulty::Edit
        }
    }
}

fn parse_meter(s: &str) -> u32 {
    let t = s.trim();
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u32>().unwrap_or(0)
}

fn apply_song_tag(song: &mut Song, tag: &str, value: &MsdValue) {
    let v = value.param(1).trim();
    let mut warnings = Vec::new();
    match tag {
        "TITLE" => song.title = v.to_string(),
        "TITLETRANSLIT" => song.title_translit = v.to_string(),
        "SUBTITLE" => song.subtitle = v.to_string(),
        "SUBTITLETRANSLIT" => song.subtitle_translit = v.to_string(),
        "ARTIST" => song.artist = v.to_string(),
        "ARTISTTRANSLIT" => song.artist_translit = v.to_string(),
        "MUSIC" => song.music = v.to_string(),
        "BANNER" => song.banner = v.to_string(),
        "BACKGROUND" => song.background = v.to_string(),
        "JACKET" => song.jacket = v.to_string(),
        "SELECTABLE" => song.selectable = !v.eq_ignore_ascii_case("NO"),
        "DISPLAYBPM" => song.display_bpm = parse_display_bpm(value),
        "OFFSET" => {
            if let Some(r) = parse_offset(v, &mut warnings) {
                song.timing.offset_s = r;
            }
        }
        "BPMS" => song.timing.bpms = parse_bpms(v, &mut warnings),
        "STOPS" | "FREEZES" => song.timing.stops = parse_seconds_pairs("STOPS", v, &mut warnings),
        "DELAYS" => song.timing.delays = parse_seconds_pairs("DELAYS", v, &mut warnings),
        "WARPS" => song.timing.warps = parse_beat_pairs("WARPS", v, &mut warnings),
        "FAKES" => song.timing.fakes = parse_beat_pairs("FAKES", v, &mut warnings),
        "SCROLLS" => song.scrolls = scroll::parse_scrolls(v, &mut warnings),
        "SPEEDS" => song.speeds = scroll::parse_speeds(v, &mut warnings),
        "SAMPLESTART" => song.sample_start_s = parse_number(v).map(|r| r.as_f64()),
        "SAMPLELENGTH" => song.sample_length_s = parse_number(v).map(|r| r.as_f64()),
        _ => {}
    }
    song.warnings.append(&mut warnings);
}

/// `#DISPLAYBPM:min:max;`, `#DISPLAYBPM:bpm;` or `#DISPLAYBPM:*;`.
fn parse_display_bpm(value: &MsdValue) -> Option<(f64, f64)> {
    let a = value.param(1).trim();
    if a.is_empty() || a == "*" {
        return None;
    }
    let lo = parse_number(a)?.as_f64();
    let b = value.param(2).trim();
    let hi = if b.is_empty() || b == "*" {
        lo
    } else {
        parse_number(b)?.as_f64()
    };
    Some((lo.min(hi), lo.max(hi)))
}

fn parse_offset(v: &str, warnings: &mut Vec<Warning>) -> Option<Rational> {
    if v.is_empty() {
        return Some(Rational::zero());
    }
    let r = parse_number(v);
    if r.is_none() {
        warnings.push(Warning(format!("#OFFSET {v:?} is not a number; using 0")));
    }
    r
}

/// Parse a decimal with optional sign, fraction and exponent (`-0.25`,
/// `1.4e2`, `+3`) into an exact `Rational`. Returns `None` for anything
/// else, including exponents beyond ±18 and values that overflow.
#[must_use]
pub fn parse_number(s: &str) -> Option<Rational> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (sign, rest) = match trimmed.strip_prefix('-') {
        Some(r) => (-1i64, r),
        None => (1i64, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(i) => (&rest[..i], rest[i + 1..].parse::<i32>().ok()?),
        None => (rest, 0i32),
    };
    if !(-18..=18).contains(&exponent) {
        return None;
    }
    let (int_part, frac_part) = match mantissa.split_once('.') {
        Some((a, b)) => (a, b),
        None => (mantissa, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let int_val: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    let frac_digits = u32::try_from(frac_part.len()).ok()?;
    if frac_digits > 18 {
        return None;
    }
    let frac_val: i64 = if frac_part.is_empty() {
        0
    } else {
        frac_part.parse().ok()?
    };
    let pow = 10i64.checked_pow(frac_digits)?;
    let num = int_val
        .checked_mul(pow)?
        .checked_add(frac_val)?
        .checked_mul(sign)?;
    let base = Rational::new(num, pow).ok()?;
    if exponent == 0 {
        return Some(base);
    }
    let scale = 10i64.checked_pow(exponent.unsigned_abs())?;
    if exponent > 0 {
        base.mul(&Rational::from_integer(scale)).ok()
    } else {
        base.div(&Rational::from_integer(scale)).ok()
    }
}

/// Split a `beat=value,beat=value,…` list tolerantly: blank entries and
/// trailing commas are ignored, malformed entries warn and are skipped.
fn parse_pairs(tag: &str, s: &str, warnings: &mut Vec<Warning>) -> Vec<(Rational, Rational)> {
    let mut out = Vec::new();
    for (i, entry) in s.split(',').enumerate() {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let Some((a, b)) = entry.split_once('=') else {
            warnings.push(Warning(format!(
                "#{tag} entry {i} {entry:?} has no `=`; skipped"
            )));
            continue;
        };
        match (parse_number(a), parse_number(b)) {
            (Some(a), Some(b)) => out.push((a, b)),
            _ => warnings.push(Warning(format!(
                "#{tag} entry {i} {entry:?} is not `number=number`; skipped"
            ))),
        }
    }
    out
}

fn parse_bpms(s: &str, warnings: &mut Vec<Warning>) -> Vec<(Beat, Bpm)> {
    parse_pairs("BPMS", s, warnings)
        .into_iter()
        .map(|(b, v)| (Beat::from_rational(b), Bpm::from_rational(v)))
        .collect()
}

fn parse_seconds_pairs(tag: &str, s: &str, warnings: &mut Vec<Warning>) -> Vec<(Beat, Rational)> {
    parse_pairs(tag, s, warnings)
        .into_iter()
        .map(|(b, v)| (Beat::from_rational(b), v))
        .collect()
}

fn parse_beat_pairs(tag: &str, s: &str, warnings: &mut Vec<Warning>) -> Vec<(Beat, Beat)> {
    parse_pairs(tag, s, warnings)
        .into_iter()
        .map(|(b, v)| (Beat::from_rational(b), Beat::from_rational(v)))
        .collect()
}

/// Accumulates one `#NOTEDATA` section.
#[derive(Debug, Default)]
struct ChartDraft {
    stepstype_raw: String,
    difficulty_raw: String,
    meter: u32,
    description: String,
    chartname: String,
    credit: String,
    notes_body: Option<String>,
    offset: Option<Rational>,
    bpms: Option<Vec<(Beat, Bpm)>>,
    stops: Option<Vec<(Beat, Rational)>>,
    delays: Option<Vec<(Beat, Rational)>>,
    warps: Option<Vec<(Beat, Beat)>>,
    fakes: Option<Vec<(Beat, Beat)>>,
    scrolls: Option<Vec<ScrollEntry>>,
    speeds: Option<Vec<SpeedEntry>>,
    warnings: Vec<Warning>,
}

impl ChartDraft {
    fn apply(&mut self, tag: &str, value: &MsdValue) {
        let v = value.param(1).trim();
        match tag {
            "STEPSTYPE" => self.stepstype_raw = v.to_string(),
            "DIFFICULTY" => self.difficulty_raw = v.to_string(),
            "METER" => self.meter = parse_meter(v),
            "DESCRIPTION" => self.description = v.to_string(),
            "CHARTNAME" => self.chartname = v.to_string(),
            "CREDIT" => self.credit = v.to_string(),
            "NOTES" | "NOTES2" => self.notes_body = Some(value.param(1).to_string()),
            "OFFSET" => {
                if let Some(r) = parse_offset(v, &mut self.warnings) {
                    self.offset = Some(r);
                }
            }
            "BPMS" => self.bpms = Some(parse_bpms(v, &mut self.warnings)),
            "STOPS" => self.stops = Some(parse_seconds_pairs("STOPS", v, &mut self.warnings)),
            "DELAYS" => self.delays = Some(parse_seconds_pairs("DELAYS", v, &mut self.warnings)),
            "WARPS" => self.warps = Some(parse_beat_pairs("WARPS", v, &mut self.warnings)),
            "FAKES" => self.fakes = Some(parse_beat_pairs("FAKES", v, &mut self.warnings)),
            "SCROLLS" => self.scrolls = Some(scroll::parse_scrolls(v, &mut self.warnings)),
            "SPEEDS" => self.speeds = Some(scroll::parse_speeds(v, &mut self.warnings)),
            _ => {}
        }
    }

    fn has_timing_override(&self) -> bool {
        self.offset.is_some()
            || self.bpms.is_some()
            || self.stops.is_some()
            || self.delays.is_some()
            || self.warps.is_some()
            || self.fakes.is_some()
    }

    fn finalize(mut self, song: &mut Song) {
        let reject = |song: &mut Song, this: &ChartDraft, reason: String| {
            song.rejected.push(RejectedChart {
                stepstype: this.stepstype_raw.clone(),
                difficulty: this.difficulty_raw.clone(),
                reason,
            });
        };
        let Some(style) = notes::parse_stepstype(&self.stepstype_raw) else {
            let reason = if self.stepstype_raw.is_empty() {
                "#NOTEDATA has no #STEPSTYPE".to_string()
            } else {
                "unsupported stepstype".to_string()
            };
            reject(song, &self, reason);
            return;
        };
        let Some(body) = self.notes_body.take() else {
            reject(song, &self, "#NOTEDATA has no #NOTES".to_string());
            return;
        };
        let difficulty = resolve_difficulty(&self.difficulty_raw, &mut self.warnings);
        let notes = match notes::parse_notes_body(&body, style, &mut self.warnings) {
            Ok(n) => n,
            Err(e) => {
                reject(song, &self, e.0);
                return;
            }
        };
        let timing = if self.has_timing_override() {
            let mut t = song.timing.clone();
            if let Some(o) = self.offset {
                t.offset_s = o;
            }
            if let Some(b) = self.bpms.take() {
                t.bpms = b;
            }
            if let Some(s) = self.stops.take() {
                t.stops = s;
            }
            if let Some(d) = self.delays.take() {
                t.delays = d;
            }
            if let Some(w) = self.warps.take() {
                t.warps = w;
            }
            if let Some(f) = self.fakes.take() {
                t.fakes = f;
            }
            (t != song.timing).then_some(t)
        } else {
            None
        };
        let description = [&self.description, &self.chartname, &self.credit]
            .into_iter()
            .find(|s| !s.is_empty())
            .cloned()
            .unwrap_or_default();
        // Like the timing override: a per-chart table equal to the song's
        // folds back to `None`.
        let scrolls = self.scrolls.take().filter(|s| *s != song.scrolls);
        let speeds = self.speeds.take().filter(|s| *s != song.speeds);
        song.charts.push(Chart {
            style,
            difficulty,
            meter: self.meter,
            description,
            timing,
            scrolls,
            speeds,
            notes,
            warnings: std::mem::take(&mut self.warnings),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::sm::model::{NoteKind, Style};

    fn r(n: i64, d: i64) -> Rational {
        Rational::new(n, d).unwrap()
    }

    const BASIC_CHART: &str = "\
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Easy;
#METER:4;
#NOTES:
1000
0000
0100
0000
;
";

    #[test]
    fn song_header_tags() {
        let text = "\
\u{feff}#VERSION:0.83;
#TITLE:Song Title;
#TITLETRANSLIT:Romaji;
#SUBTITLE:(remix);
#ARTIST:Someone;
#MUSIC:song.ogg;
#JACKET:jk.png;
#BANNER:bn.png;
#BACKGROUND:bg.png;
#SELECTABLE:NO;
#DISPLAYBPM:120:180;
#OFFSET:-0.123;
#SAMPLESTART:30.0;
#SAMPLELENGTH:15.0;
#BPMS:0.000=120.000;
";
        let song = parse_ssc(text);
        assert_eq!(song.title, "Song Title");
        assert_eq!(song.title_translit, "Romaji");
        assert_eq!(song.subtitle, "(remix)");
        assert_eq!(song.artist, "Someone");
        assert_eq!(song.music, "song.ogg");
        assert_eq!(song.jacket, "jk.png");
        assert_eq!(song.banner, "bn.png");
        assert_eq!(song.background, "bg.png");
        assert!(!song.selectable);
        assert_eq!(song.display_bpm, Some((120.0, 180.0)));
        assert_eq!(song.timing.offset_s, r(-123, 1000));
        assert_eq!(song.sample_start_s, Some(30.0));
        assert_eq!(song.sample_length_s, Some(15.0));
        assert_eq!(song.timing.bpms.len(), 1);
        assert_eq!(song.timing.bpms[0].1, Bpm::from_rational(r(120, 1)));
        assert!(song.charts.is_empty());
        assert!(song.warnings.is_empty());
    }

    #[test]
    fn tolerant_timing_lists() {
        let song = parse_ssc(
            "#BPMS:0=1.4e2,,16.5=180.0,;\n#STOPS:8.000=0.500,\n;\n#DELAYS:4=1;\n#WARPS:8=2;\n#FAKES:12=4,junk;",
        );
        assert_eq!(song.timing.bpms.len(), 2);
        assert_eq!(song.timing.bpms[0].1, Bpm::from_rational(r(140, 1)));
        assert_eq!(song.timing.bpms[1].0, Beat::from_rational(r(33, 2)));
        assert_eq!(song.timing.stops, vec![(Beat::from_integer(8), r(1, 2))]);
        assert_eq!(song.timing.delays, vec![(Beat::from_integer(4), r(1, 1))]);
        assert_eq!(
            song.timing.warps,
            vec![(Beat::from_integer(8), Beat::from_integer(2))]
        );
        assert_eq!(
            song.timing.fakes,
            vec![(Beat::from_integer(12), Beat::from_integer(4))]
        );
        assert_eq!(song.warnings.len(), 1, "{:?}", song.warnings);
    }

    #[test]
    fn negative_bpm_and_stop_are_parsed_not_rejected() {
        let song = parse_ssc("#BPMS:0=120,4=-120;\n#STOPS:2=-0.5;");
        assert_eq!(song.timing.bpms.len(), 2);
        assert_eq!(song.timing.stops.len(), 1);
        assert!(song.timing.has_gimmicks());
    }

    #[test]
    fn number_parsing() {
        assert_eq!(parse_number("0.5"), Some(r(1, 2)));
        assert_eq!(parse_number("-0.250"), Some(r(-1, 4)));
        assert_eq!(parse_number("+3"), Some(r(3, 1)));
        assert_eq!(parse_number("1.4e2"), Some(r(140, 1)));
        assert_eq!(parse_number("25E-1"), Some(r(5, 2)));
        assert_eq!(parse_number(".5"), Some(r(1, 2)));
        assert_eq!(parse_number("5."), Some(r(5, 1)));
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("1e99"), None);
        assert_eq!(parse_number("1.2.3"), None);
        assert_eq!(parse_number("99999999999999999999"), None);
    }

    #[test]
    fn ssc_chart_section_parses_with_meter() {
        let song = parse_ssc(&format!("#BPMS:0=120;\n{BASIC_CHART}"));
        assert_eq!(song.charts.len(), 1);
        let c = &song.charts[0];
        assert_eq!(c.style, Style::Single);
        assert_eq!(c.difficulty, Difficulty::Basic);
        assert_eq!(c.meter, 4);
        assert!(c.timing.is_none());
        assert_eq!(c.notes.len(), 2);
        assert!(song.rejected.is_empty());
    }

    #[test]
    fn per_chart_isolation() {
        let text = "\
#BPMS:0=120;
#NOTEDATA:;
#STEPSTYPE:dance-solo;
#DIFFICULTY:Hard;
#NOTES:
100000
;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Crazy;
#NOTES:
1000
;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Hard;
#NOTES:
2000
0000
;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Challenge;
#NOTES:
1000
;
";
        let song = parse_ssc(text);
        assert_eq!(song.rejected.len(), 1, "{:?}", song.rejected);
        assert_eq!(song.rejected[0].stepstype, "dance-solo");
        assert_eq!(song.rejected[0].reason, "unsupported stepstype");
        assert_eq!(song.charts.len(), 3);
        assert_eq!(song.charts[0].difficulty, Difficulty::Edit);
        assert_eq!(song.charts[0].warnings.len(), 1);
        // The unclosed hold head is kept as a tap with a warning (StepMania).
        assert_eq!(song.charts[1].difficulty, Difficulty::Expert);
        assert_eq!(song.charts[1].notes[0].kind, NoteKind::Tap);
        assert!(song.charts[1].warnings[0].0.contains("never closed"));
        assert_eq!(song.charts[2].difficulty, Difficulty::Challenge);
    }

    #[test]
    fn per_chart_timing_is_captured_and_identical_override_folds_to_none() {
        let text = "\
#OFFSET:0;
#BPMS:0=120;
#STOPS:;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Hard;
#BPMS:0=200;
#DELAYS:4=0.5;
#NOTES:
1000
;
#NOTEDATA:;
#STEPSTYPE:dance-double;
#DIFFICULTY:Hard;
#OFFSET:0.000;
#BPMS:0.000=120.000;
#STOPS:;
#NOTES:
10000000
;
";
        let song = parse_ssc(text);
        assert_eq!(song.charts.len(), 2);
        let t = song.charts[0].timing.as_ref().expect("override");
        assert_eq!(t.bpms, vec![(Beat::zero(), Bpm::from_rational(r(200, 1)))]);
        assert_eq!(t.delays, vec![(Beat::from_integer(4), r(1, 2))]);
        assert_eq!(t.offset_s, Rational::zero());
        assert!(song.charts[1].timing.is_none());
    }

    #[test]
    fn sm_notes_six_field_form_and_aliases() {
        let text = "\
#TITLE:Test Song;
#OFFSET:-0.050;
#BPMS:0.000=150.000;
#FREEZES:4=0.25;
#NOTES:
     dance-single:
     author:
     Heavy:
     9:
     0.5,0.5,0.5,0.5,0.5:
1000
0000
0000
0000
;
#NOTES:
     dance-double:
     :
     Light:
     3:
     :
10000000
;
#NOTES:
     pump-single:
     :
     Hard:
     3:
     :
10000
;
#NOTES:bad:form;
";
        let song = parse_sm(text);
        assert_eq!(song.title, "Test Song");
        assert_eq!(song.timing.offset_s, r(-1, 20));
        assert_eq!(song.timing.stops, vec![(Beat::from_integer(4), r(1, 4))]);
        assert_eq!(song.charts.len(), 2);
        assert_eq!(song.charts[0].difficulty, Difficulty::Expert);
        assert_eq!(song.charts[0].meter, 9);
        assert_eq!(song.charts[0].description, "author");
        assert_eq!(song.charts[0].notes[0].kind, NoteKind::Tap);
        assert_eq!(song.charts[1].style, Style::Double);
        assert_eq!(song.charts[1].difficulty, Difficulty::Basic);
        assert_eq!(song.rejected.len(), 2);
        assert_eq!(song.rejected[0].stepstype, "pump-single");
        assert!(song.rejected[1].reason.contains("fields"));
    }

    #[test]
    fn sm_style_notes_inside_ssc_at_song_level() {
        let song =
            parse_ssc("#BPMS:0=120;\n#NOTES:dance-single::Medium:5:0,0,0,0,0:\n1000\n0000\n;\n");
        assert_eq!(song.charts.len(), 1);
        assert_eq!(song.charts[0].difficulty, Difficulty::Difficult);
        assert_eq!(song.charts[0].meter, 5);
    }

    #[test]
    fn case_insensitive_tags_and_crlf() {
        let song = parse_ssc("#title:Lower;\r\n#bpms:0=100;\r\n#offset:0.5;\r\n");
        assert_eq!(song.title, "Lower");
        assert_eq!(song.timing.bpms.len(), 1);
        assert_eq!(song.timing.offset_s, r(1, 2));
    }

    #[test]
    fn shift_jis_fixture_round_trips_through_decode() {
        let bytes = b"#TITLE:\x97\xf6\x82\xcc\x89\xcc;\n#BPMS:0=120;\n";
        let (text, enc) = crate::core::sm::text::decode(bytes);
        assert_eq!(enc, crate::core::sm::text::Encoding::ShiftJis);
        let song = parse_ssc(&text);
        assert_eq!(song.title, "恋の歌");
        assert_eq!(song.timing.bpms.len(), 1);
    }

    #[test]
    fn parse_picks_grammar_by_extension() {
        let sm = "#BPMS:0=120;\n#NOTES:dance-single::Easy:1::\n1000\n;\n";
        assert_eq!(parse("Song.SM", sm).charts.len(), 1);
        assert_eq!(parse("song.ssc", sm).charts.len(), 1);
        let ssc = format!("#BPMS:0=120;\n{BASIC_CHART}");
        assert_eq!(parse("song.ssc", &ssc).charts.len(), 1);
        // `.sm` grammar treats NOTEDATA tags as unknown and finds no charts.
        assert_eq!(parse("song.sm", &ssc).charts.len(), 0);
    }

    #[test]
    fn display_bpm_forms() {
        assert_eq!(parse_ssc("#DISPLAYBPM:*;").display_bpm, None);
        assert_eq!(
            parse_ssc("#DISPLAYBPM:150;").display_bpm,
            Some((150.0, 150.0))
        );
        assert_eq!(
            parse_ssc("#DISPLAYBPM:200:100;").display_bpm,
            Some((100.0, 200.0))
        );
    }

    #[test]
    fn scrolls_and_speeds_are_kept_at_song_and_chart_level() {
        let text = "\
#BPMS:0=120;
#SCROLLS:0.000=1.000,116.000=0.000,116.500=1.000;
#SPEEDS:0.000=1.000=0.000=0,160.000=0.000=0.000=0,160.021=1.000=6.000=0;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Hard;
#SCROLLS:0=1,8=2;
#NOTES:
1000
;
#NOTEDATA:;
#STEPSTYPE:dance-single;
#DIFFICULTY:Challenge;
#SCROLLS:0.000=1.000,116.000=0.000,116.500=1.000;
#SPEEDS:0.000=1.000=0.000=0,160.000=0.000=0.000=0,160.021=1.000=6.000=0;
#NOTES:
1000
;
";
        let song = parse_ssc(text);
        assert_eq!(song.scrolls.len(), 3);
        assert_eq!(song.scrolls[1].beat, Beat::from_integer(116));
        assert_eq!(song.scrolls[1].factor, Rational::zero());
        assert_eq!(song.speeds.len(), 3);
        assert_eq!(song.speeds[2].delay, r(6, 1));
        assert_eq!(song.charts.len(), 2);
        let hard = &song.charts[0];
        assert_eq!(hard.scrolls.as_ref().map(Vec::len), Some(2));
        assert!(hard.speeds.is_none());
        assert_eq!(hard.effective_speeds(&song).len(), 3);
        assert_eq!(hard.effective_scrolls(&song).len(), 2);
        // Identical per-chart tables fold back to the song's.
        let chal = &song.charts[1];
        assert!(chal.scrolls.is_none() && chal.speeds.is_none());
        assert!(
            chal.timing.is_none(),
            "scroll tags are not timing overrides"
        );
        assert!(song.warnings.is_empty(), "{:?}", song.warnings);
    }

    #[test]
    fn garbage_input_does_not_panic() {
        for text in [
            "",
            "#",
            "#:::;",
            "#NOTEDATA:;",
            "#NOTES:;",
            "#BPMS:=;",
            "#NOTEDATA:;#NOTES:1000;",
        ] {
            let _ = parse_ssc(text);
            let _ = parse_sm(text);
        }
    }
}
