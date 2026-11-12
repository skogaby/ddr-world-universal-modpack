//! The pack chooser (optional content packs design §4.11): a line-based
//! checklist of the catalog's packs. No cursor control, so it behaves the
//! same on a Windows 7 console, a modern terminal and under Wine.
//!
//! Everything but the input loop is pure: [`Screen`] holds the ticks,
//! [`Screen::render`] draws it, [`Screen::apply_line`] edits it and
//! [`Screen::changes`] says what applying it would do. The loop gives up
//! after [`IDLE_CAP`] without input and applies nothing — a cabinet must
//! never be left waiting in it.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

use super::catalog::Catalog;
use super::megabytes;
use super::state::{Marker, Selection, SelectionFile};
use super::{installed_state, selected_ids, Installed};
use crate::console::{Input, LineReader};

/// How long one question waits before the chooser closes unchanged.
pub const IDLE_CAP: Duration = Duration::from_secs(5 * 60);
const RULE_WIDTH: usize = 74;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub group: String,
    pub title: String,
    pub items: u32,
    pub zip_size: u64,
    pub installed_size: u64,
    pub state: Installed,
    /// Added by a release since the user last saw the list.
    pub is_new: bool,
    pub ticked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub rows: Vec<Row>,
    pub install_new: bool,
}

/// What applying a screen would do, as row indexes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Changes {
    /// Ticked and not installed at the catalog's revision.
    pub download: Vec<usize>,
    /// Installed and no longer ticked.
    pub remove: Vec<usize>,
    /// The "future releases" tick differs from the saved one.
    pub install_new_changed: bool,
    /// The set of ticked packs differs from the saved selection.
    pub selection_changed: bool,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.download.is_empty()
            && self.remove.is_empty()
            && !self.install_new_changed
            && !self.selection_changed
    }
}

/// What one input line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// The screen changed (or tokens were ignored): draw it again.
    Redraw { ignored: Vec<String> },
    /// An empty line: apply.
    Apply,
    /// `q`: leave without changing anything.
    Quit,
}

impl Screen {
    /// The screen as it opens: a pack is ticked when it is selected or
    /// installed.
    pub fn open(
        catalog: &Catalog,
        selection: &SelectionFile,
        markers: &BTreeMap<String, Marker>,
    ) -> Self {
        let selected = selected_ids(selection, markers);
        let saved = match selection {
            SelectionFile::Loaded(s) => Some(s),
            _ => None,
        };
        let rows = catalog
            .packs
            .iter()
            .map(|pack| {
                let state = installed_state(pack, markers);
                Row {
                    id: pack.id.clone(),
                    group: pack.group.clone(),
                    title: pack.title.clone(),
                    items: pack.items,
                    zip_size: pack.zip_size,
                    installed_size: pack.installed_size,
                    state,
                    is_new: saved.is_some_and(|s| !s.known.is_empty() && !s.is_known(&pack.id))
                        && state == Installed::No,
                    ticked: selected.contains(&pack.id) || state != Installed::No,
                }
            })
            .collect();
        Screen {
            rows,
            install_new: saved.is_some_and(|s| s.install_new),
        }
    }

    /// The letter that toggles each group: its first letter, unless that is
    /// taken by a command or by an earlier group.
    fn group_keys(&self) -> Vec<(char, &str)> {
        let mut keys: Vec<(char, &str)> = Vec::new();
        for row in &self.rows {
            if keys.iter().any(|(_, g)| *g == row.group) {
                continue;
            }
            let Some(key) = row.group.chars().next().map(|c| c.to_ascii_lowercase()) else {
                continue;
            };
            if !key.is_ascii_lowercase()
                || "anfq".contains(key)
                || keys.iter().any(|(k, _)| *k == key)
            {
                continue;
            }
            keys.push((key, row.group.as_str()));
        }
        keys
    }

    pub fn render(&self) -> String {
        let ticked = self.rows.iter().filter(|r| r.ticked).count();
        let on_disk: u64 = self
            .rows
            .iter()
            .filter(|r| r.ticked)
            .map(|r| r.installed_size)
            .sum();
        let title_width = self
            .rows
            .iter()
            .map(|r| r.title.chars().count())
            .max()
            .unwrap_or(0);
        let rule = "=".repeat(RULE_WIDTH);
        let mut out = format!(
            "\n{rule}\n OPTIONAL PACKS            {ticked} of {} selected, {} to keep on disk\n{rule}\n",
            self.rows.len(),
            megabytes(on_disk)
        );
        let mut group: Option<&str> = None;
        for (i, row) in self.rows.iter().enumerate() {
            if group != Some(row.group.as_str()) {
                out.push_str(&format!("\n {}\n", row.group.to_uppercase()));
                group = Some(row.group.as_str());
            }
            let note = match row.state {
                Installed::Current => "installed",
                Installed::OtherRevision => "installed, update available",
                Installed::No if row.is_new => "NEW",
                Installed::No => "",
            };
            let line = format!(
                "  {:>2} [{}] {:<title_width$} {:>4} {:<8} {:>9}   {note}",
                i + 1,
                if row.ticked { 'x' } else { ' ' },
                row.title,
                row.items,
                row.group.to_lowercase(),
                megabytes(row.zip_size),
            );
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out.push_str(&format!(
            "\n   f [{}] Also install packs added by future releases\n\n",
            if self.install_new { 'x' } else { ' ' }
        ));
        out.push_str(" Type numbers to switch packs on or off, e.g. \"1 2 3\" or \"4-6\".\n ");
        for (key, name) in self.group_keys() {
            out.push_str(&format!("{key} = all {}   ", name.to_lowercase()));
        }
        out.push_str("a = everything   n = nothing\n");
        out.push_str(" Enter = apply     q = leave without changing anything\n");
        out
    }

    /// Apply one input line to the ticks.
    pub fn apply_line(&mut self, line: &str) -> Edit {
        let line = line.trim();
        if line.is_empty() {
            return Edit::Apply;
        }
        if line.eq_ignore_ascii_case("q") {
            return Edit::Quit;
        }
        let group_keys: Vec<(char, String)> = self
            .group_keys()
            .into_iter()
            .map(|(k, g)| (k, g.to_string()))
            .collect();
        let mut ignored = Vec::new();
        for token in line
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|t| !t.is_empty())
        {
            let lower = token.to_ascii_lowercase();
            match lower.as_str() {
                "a" => self.rows.iter_mut().for_each(|r| r.ticked = true),
                "n" => {
                    self.rows.iter_mut().for_each(|r| r.ticked = false);
                    self.install_new = false;
                }
                "f" => self.install_new = !self.install_new,
                _ => {
                    let key = lower.chars().next().filter(|_| lower.chars().count() == 1);
                    if let Some((_, group)) =
                        key.and_then(|k| group_keys.iter().find(|(g, _)| *g == k))
                    {
                        // All on, unless they already all are: then all off.
                        let all = self
                            .rows
                            .iter()
                            .filter(|r| &r.group == group)
                            .all(|r| r.ticked);
                        self.rows
                            .iter_mut()
                            .filter(|r| &r.group == group)
                            .for_each(|r| r.ticked = !all);
                    } else if let Some((lo, hi)) = parse_range(&lower, self.rows.len()) {
                        for row in &mut self.rows[lo - 1..hi] {
                            row.ticked = !row.ticked;
                        }
                    } else {
                        ignored.push(token.to_string());
                    }
                }
            }
        }
        Edit::Redraw { ignored }
    }

    /// What applying this screen would change, given the saved selection.
    pub fn changes(&self, selection: &SelectionFile) -> Changes {
        let saved = match selection {
            SelectionFile::Loaded(s) => Some(s),
            _ => None,
        };
        let ticked: Vec<&str> = self.ticked_ids();
        let saved_ids: Vec<&str> = saved
            .map(|s| s.packs.iter().map(String::as_str).collect())
            .unwrap_or_default();
        Changes {
            download: (0..self.rows.len())
                .filter(|&i| self.rows[i].ticked && self.rows[i].state != Installed::Current)
                .collect(),
            remove: (0..self.rows.len())
                .filter(|&i| !self.rows[i].ticked && self.rows[i].state != Installed::No)
                .collect(),
            install_new_changed: self.install_new != saved.is_some_and(|s| s.install_new),
            selection_changed: saved.is_none()
                || ticked.len() != saved_ids.len()
                || ticked.iter().any(|id| !saved_ids.contains(id)),
        }
    }

    fn ticked_ids(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|r| r.ticked)
            .map(|r| r.id.as_str())
            .collect()
    }

    /// The selection this screen stands for: the ticked packs, the "future
    /// releases" tick, and every listed pack as known.
    pub fn selection(&self, previous: &SelectionFile) -> Selection {
        let mut selection = match previous {
            SelectionFile::Loaded(s) => s.clone(),
            _ => Selection::new(),
        };
        selection.packs = self.ticked_ids().into_iter().map(str::to_string).collect();
        selection.install_new = self.install_new;
        for row in &self.rows {
            if !selection.is_known(&row.id) {
                selection.known.push(row.id.clone());
            }
        }
        selection
    }

    /// The confirmation text for `changes`.
    pub fn summary(&self, changes: &Changes) -> String {
        let mut out = String::from("\n");
        if !changes.download.is_empty() {
            let bytes: u64 = changes
                .download
                .iter()
                .map(|&i| self.rows[i].zip_size)
                .sum();
            out.push_str(&format!(
                " Download {} pack(s), {}\n",
                changes.download.len(),
                megabytes(bytes)
            ));
        }
        if !changes.remove.is_empty() {
            let names: Vec<&str> = changes
                .remove
                .iter()
                .map(|&i| self.rows[i].title.as_str())
                .collect();
            out.push_str(&format!(
                " Remove   {} pack(s): {}\n",
                changes.remove.len(),
                names.join(", ")
            ));
        }
        if changes.install_new_changed {
            out.push_str(if self.install_new {
                " Future packs: install automatically\n"
            } else {
                " Future packs: ask first\n"
            });
        }
        if changes.download.is_empty() && changes.remove.is_empty() && !changes.install_new_changed
        {
            out.push_str(" Save this selection (nothing to download or remove)\n");
        }
        out
    }
}

/// `7` or `3-9`, 1-based and within `1..=count`.
fn parse_range(token: &str, count: usize) -> Option<(usize, usize)> {
    let (lo, hi) = match token.split_once('-') {
        Some((a, b)) => (a.parse::<usize>().ok()?, b.parse::<usize>().ok()?),
        None => {
            let n = token.parse::<usize>().ok()?;
            (n, n)
        }
    };
    (lo >= 1 && lo <= hi && hi <= count).then_some((lo, hi))
}

/// How the chooser ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Apply(Screen),
    /// `q`, end of input, a declined confirmation at end of input, or idle.
    Unchanged,
}

/// Run the checklist against `input`, drawing on `out`.
pub fn run(
    mut screen: Screen,
    selection: &SelectionFile,
    input: &LineReader,
    out: &mut dyn Write,
    idle: Duration,
) -> Decision {
    let ask = |out: &mut dyn Write, prompt: &str| -> Option<String> {
        let _ = write!(out, "{prompt}");
        let _ = out.flush();
        match input.read(idle) {
            Input::Line(line) => Some(line),
            Input::Closed => {
                let _ = writeln!(out);
                None
            }
            Input::TimedOut => {
                let _ = writeln!(
                    out,
                    "\n No input for {} minutes; nothing was changed.",
                    idle.as_secs() / 60
                );
                None
            }
        }
    };
    loop {
        let _ = write!(out, "{}", screen.render());
        let Some(line) = ask(out, " > ") else {
            return Decision::Unchanged;
        };
        match screen.apply_line(&line) {
            Edit::Quit => {
                let _ = writeln!(out, " Nothing changed.");
                return Decision::Unchanged;
            }
            Edit::Redraw { ignored } => {
                for token in ignored {
                    let _ = writeln!(out, " (ignored {token:?})");
                }
            }
            Edit::Apply => {
                let changes = screen.changes(selection);
                if changes.is_empty() {
                    let _ = writeln!(out, " Nothing to do.");
                    return Decision::Unchanged;
                }
                let _ = write!(out, "{}", screen.summary(&changes));
                let Some(answer) = ask(out, " Go ahead? [Y/n] ") else {
                    return Decision::Unchanged;
                };
                if matches!(
                    answer.trim().to_ascii_lowercase().as_str(),
                    "" | "y" | "yes"
                ) {
                    return Decision::Apply(screen);
                }
                // Anything else: back to the list.
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::console::LIVE_COLUMNS;
    use crate::packs::catalog::tests::test_pack;
    use crate::packs::catalog::Pack;
    use crate::packs::selection_of;
    use crate::packs::state::{parse_marker, tests::marker_json};
    use std::io::Cursor;

    /// Dancers `a b c`, stages `x y` — five rows.
    fn catalog() -> Catalog {
        let mut packs: Vec<Pack> = ["a", "b", "c"]
            .iter()
            .map(|id| test_pack(id, &format!("Mix {}", id.to_uppercase()), &id.repeat(64)))
            .collect();
        for id in ["x", "y"] {
            let mut p = test_pack(id, &format!("Hall {}", id.to_uppercase()), &id.repeat(64));
            p.group = "Stages".to_string();
            packs.push(p);
        }
        for (i, p) in packs.iter_mut().enumerate() {
            p.zip_size = (i as u64 + 1) * 10 * 1024 * 1024;
            p.installed_size = (i as u64 + 1) * 30 * 1024 * 1024;
            p.items = 10 + i as u32;
        }
        Catalog { packs }
    }

    fn markers(rows: &[(&str, &str)]) -> BTreeMap<String, Marker> {
        rows.iter()
            .map(|(id, cid)| {
                (
                    id.to_string(),
                    parse_marker(&marker_json(id, cid, &[]), id).unwrap(),
                )
            })
            .collect()
    }

    fn ticks(screen: &Screen) -> String {
        screen
            .rows
            .iter()
            .map(|r| if r.ticked { 'x' } else { '.' })
            .collect()
    }

    fn blank() -> Screen {
        Screen::open(&catalog(), &SelectionFile::Absent, &BTreeMap::new())
    }

    #[test]
    fn opens_with_selected_and_installed_packs_ticked() {
        let mut saved = selection_of(&["a"]);
        saved.known = vec!["a".into(), "b".into(), "x".into()];
        saved.install_new = true;
        // `b` installed by hand at an old revision; `x` installed and current.
        let m = markers(&[("b", "old"), ("x", &"x".repeat(64))]);
        let screen = Screen::open(&catalog(), &SelectionFile::Loaded(saved), &m);
        assert_eq!(ticks(&screen), "xx.x.");
        assert!(screen.install_new);
        let states: Vec<Installed> = screen.rows.iter().map(|r| r.state).collect();
        assert_eq!(
            states,
            [
                Installed::No,
                Installed::OtherRevision,
                Installed::No,
                Installed::Current,
                Installed::No
            ]
        );
        let new: Vec<bool> = screen.rows.iter().map(|r| r.is_new).collect();
        assert_eq!(
            new,
            [false, false, true, false, true],
            "c and y were never shown"
        );

        // Before anything was ever chosen nothing is "new", and the installed
        // packs are the ticks.
        let first = Screen::open(&catalog(), &SelectionFile::Absent, &m);
        assert_eq!(ticks(&first), ".x.x.");
        assert!(first.rows.iter().all(|r| !r.is_new) && !first.install_new);
    }

    #[test]
    fn input_lines_edit_the_ticks() {
        let mut s = blank();
        assert_eq!(s.apply_line("1 3"), Edit::Redraw { ignored: vec![] });
        assert_eq!(ticks(&s), "x.x..");
        s.apply_line("2-4");
        assert_eq!(ticks(&s), "xx.x.", "a range toggles each row in it");
        s.apply_line("5,5 5");
        assert_eq!(ticks(&s), "xx.xx", "commas and repeats");
        s.apply_line("d");
        assert_eq!(ticks(&s), "xxxxx", "a group letter fills its group");
        s.apply_line("D");
        assert_eq!(ticks(&s), "...xx", "and empties it when it was full");
        s.apply_line("s");
        assert_eq!(ticks(&s), ".....");
        s.apply_line("a f");
        assert_eq!(ticks(&s), "xxxxx");
        assert!(s.install_new);
        s.apply_line("n");
        assert_eq!(ticks(&s), ".....");
        assert!(
            !s.install_new,
            "nothing means nothing, future packs included"
        );
    }

    #[test]
    fn junk_is_reported_and_changes_nothing_else() {
        let mut s = blank();
        assert_eq!(
            s.apply_line("1 0 6 3-2 2-9 x1 -- 2"),
            Edit::Redraw {
                ignored: ["0", "6", "3-2", "2-9", "x1", "--"]
                    .iter()
                    .map(|t| t.to_string())
                    .collect()
            }
        );
        assert_eq!(ticks(&s), "xx...");
        assert_eq!(s.apply_line("   "), Edit::Apply);
        assert_eq!(s.apply_line(""), Edit::Apply);
        assert_eq!(s.apply_line(" Q "), Edit::Quit);
        assert_eq!(ticks(&s), "xx...");
    }

    #[test]
    fn the_rendered_screen() {
        let mut saved = selection_of(&["a"]);
        saved.known = vec!["a".into(), "b".into(), "x".into(), "y".into()];
        let m = markers(&[("a", &"a".repeat(64)), ("x", "old")]);
        let screen = Screen::open(&catalog(), &SelectionFile::Loaded(saved), &m);
        let text = screen.render();
        let rule = "=".repeat(74);
        let expected = format!(
            "\n{rule}\n OPTIONAL PACKS            2 of 5 selected, 150.0 MB to keep on disk\n{rule}\n\
\n DANCERS\n\
\x20  1 [x] Mix A    10 dancers    10.0 MB   installed\n\
\x20  2 [ ] Mix B    11 dancers    20.0 MB\n\
\x20  3 [ ] Mix C    12 dancers    30.0 MB   NEW\n\
\n STAGES\n\
\x20  4 [x] Hall X   13 stages     40.0 MB   installed, update available\n\
\x20  5 [ ] Hall Y   14 stages     50.0 MB\n\
\n   f [ ] Also install packs added by future releases\n\
\n Type numbers to switch packs on or off, e.g. \"1 2 3\" or \"4-6\".\n\
\x20d = all dancers   s = all stages   a = everything   n = nothing\n\
\x20Enter = apply     q = leave without changing anything\n"
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn the_real_catalogs_longest_rows_fit_an_80_column_console() {
        let mut c = catalog();
        c.packs[0].title = "HOTTSTPARTY 1-3".to_string();
        c.packs[0].items = 137;
        c.packs[0].zip_size = 1900 * 1024 * 1024;
        let m = markers(&[("a", "old")]);
        let screen = Screen::open(&c, &SelectionFile::Absent, &m);
        for line in screen.render().lines() {
            assert!(line.chars().count() <= LIVE_COLUMNS, "{line:?}");
        }
    }

    #[test]
    fn group_letters_avoid_commands_and_each_other() {
        let mut c = catalog();
        c.packs[0].group = "Announcers".to_string(); // `a` is "everything"
        c.packs[1].group = "Skins".to_string(); // takes `s` before Stages
        let screen = Screen::open(&c, &SelectionFile::Absent, &BTreeMap::new());
        assert_eq!(screen.group_keys(), [('s', "Skins"), ('d', "Dancers")]);
        assert!(screen
            .render()
            .contains(" s = all skins   d = all dancers   a = everything"));
    }

    #[test]
    fn changes_compare_against_what_is_installed_and_saved() {
        let saved = SelectionFile::Loaded(selection_of(&["a", "x"]));
        let m = markers(&[("a", &"a".repeat(64)), ("x", "old"), ("y", &"y".repeat(64))]);
        let mut s = Screen::open(&catalog(), &saved, &m);
        assert_eq!(ticks(&s), "x..xx");
        // As opened: `x` wants its update, and the hand-installed `y` would be
        // adopted into the selection.
        let opened = s.changes(&saved);
        assert_eq!(opened.download, [3]);
        assert!(opened.remove.is_empty() && opened.selection_changed);

        s.apply_line("1 2 5 f");
        let c = s.changes(&saved);
        assert_eq!(c.download, [1, 3], "b is new, x has another revision");
        assert_eq!(c.remove, [0, 4], "a and y are installed and unticked");
        assert!(c.install_new_changed && c.selection_changed);
        assert_eq!(
            s.summary(&c),
            "\n Download 2 pack(s), 60.0 MB\n Remove   2 pack(s): Mix A, Hall Y\n Future packs: install automatically\n"
        );
        let sel = s.selection(&saved);
        assert_eq!(sel.packs, ["b", "x"]);
        assert!(sel.install_new);
        assert_eq!(sel.known, ["a", "b", "c", "x", "y"]);

        // Everything current and saved: nothing to do.
        let all_saved = SelectionFile::Loaded(selection_of(&["a"]));
        let settled = Screen::open(&catalog(), &all_saved, &markers(&[("a", &"a".repeat(64))]));
        assert!(settled.changes(&all_saved).is_empty());
        // The same screen with no selection file yet is worth saving.
        let unsaved = settled.changes(&SelectionFile::Absent);
        assert!(unsaved.selection_changed && !unsaved.is_empty());
        assert!(settled.summary(&unsaved).contains("Save this selection"));
    }

    fn drive(script: &str, selection: &SelectionFile) -> (Decision, String) {
        let input = LineReader::spawn(Cursor::new(script.to_string().into_bytes()));
        let mut out = Vec::new();
        let screen = Screen::open(&catalog(), selection, &BTreeMap::new());
        let decision = run(screen, selection, &input, &mut out, Duration::from_secs(5));
        (decision, String::from_utf8(out).unwrap())
    }

    #[test]
    fn select_then_confirm_applies() {
        let (decision, text) = drive("1-2 4\n\ny\n", &SelectionFile::Absent);
        let Decision::Apply(screen) = decision else {
            panic!("expected Apply:\n{text}");
        };
        assert_eq!(ticks(&screen), "xx.x.");
        assert!(
            text.contains(" Download 3 pack(s), 70.0 MB\n Go ahead? [Y/n] "),
            "{text}"
        );
        assert_eq!(
            text.matches("OPTIONAL PACKS").count(),
            2,
            "drawn, then redrawn after the edit"
        );
        // A bare Enter at the confirmation means yes.
        assert!(matches!(
            drive("5\n\n\n", &SelectionFile::Absent).0,
            Decision::Apply(_)
        ));
    }

    #[test]
    fn declining_returns_to_the_list_and_quitting_changes_nothing() {
        let (decision, text) = drive("1\n\nn\nq\n", &SelectionFile::Absent);
        assert_eq!(decision, Decision::Unchanged);
        assert_eq!(text.matches("OPTIONAL PACKS").count(), 3, "{text}");
        assert!(text.ends_with(" Nothing changed.\n"), "{text}");

        let (decision, text) = drive("zzz\nq\n", &SelectionFile::Absent);
        assert_eq!(decision, Decision::Unchanged);
        assert!(text.contains(" (ignored \"zzz\")"), "{text}");
    }

    #[test]
    fn applying_an_unchanged_screen_does_nothing() {
        let saved = SelectionFile::Loaded(selection_of(&[]));
        let (decision, text) = drive("\n", &saved);
        assert_eq!(decision, Decision::Unchanged);
        assert!(text.ends_with(" Nothing to do.\n"), "{text}");
    }

    #[test]
    fn end_of_input_and_silence_change_nothing() {
        // Input ends before the confirmation is answered.
        assert_eq!(
            drive("1\n\n", &SelectionFile::Absent).0,
            Decision::Unchanged
        );
        assert_eq!(drive("", &SelectionFile::Absent).0, Decision::Unchanged);

        struct Silent;
        impl std::io::Read for Silent {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                std::thread::sleep(Duration::from_secs(3600));
                Ok(0)
            }
        }
        let input = LineReader::spawn(std::io::BufReader::new(Silent));
        let mut out = Vec::new();
        let decision = run(
            blank(),
            &SelectionFile::Absent,
            &input,
            &mut out,
            Duration::from_millis(30),
        );
        assert_eq!(decision, Decision::Unchanged);
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("nothing was changed"), "{text}");
    }
}
