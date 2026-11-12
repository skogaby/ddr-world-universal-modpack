//! The console as an interactive device (design R19; optional content packs
//! design §4.10): who else is attached to it, timed line input, and lines
//! that redraw in place.
//!
//! **Ownership.** When the updater is the ONLY process attached to its
//! console it was double-clicked from Explorer, and the window would vanish
//! with the output the moment we exit — so we wait for Enter. Launched from
//! `gamestart.bat`, `cmd.exe` shares the console (count ≥ 2) and we never
//! wait; on non-Windows hosts we never wait either.
//!
//! **Timed input.** Standard input has no timed read, so ONE background
//! thread reads it line by line into a channel for the whole run
//! ([`LineReader`]) and every wait is a bounded receive on that channel. All
//! waits are bounded: an unattended machine can never be held, because the
//! game must start regardless (design R20). Only a complete line counts as
//! an answer; end of input (a closed or redirected stdin), a read error and
//! the timeout all mean "no".
//!
//! **Live lines.** A countdown or a download's progress is one console line
//! rewritten in place with a carriage return ([`LiveLine`]). It never goes
//! to the log file, is never wider than 79 columns (a redraw that wraps on
//! an 80-column console leaves debris), and is not written at all when
//! standard output is not a terminal.

use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Upper bound on the Enter-wait.
pub const WAIT_CAP: Duration = Duration::from_secs(60);
/// Widest a live line may be.
pub const LIVE_COLUMNS: usize = 79;

/// Pure decision: wait iff this process is alone on the console.
pub fn should_wait(process_count: u32) -> bool {
    process_count == 1
}

#[cfg(windows)]
fn console_process_count() -> u32 {
    use windows_sys::Win32::System::Console::GetConsoleProcessList;
    // A buffer of 2 is enough: we only care whether the count is 1.
    let mut ids = [0u32; 2];
    // SAFETY: `ids` is a valid, writable buffer of the length passed.
    let n = unsafe { GetConsoleProcessList(ids.as_mut_ptr(), ids.len() as u32) };
    // 0 = failure (no console at all, e.g. redirected/detached): don't wait.
    n
}

#[cfg(not(windows))]
fn console_process_count() -> u32 {
    0
}

/// True when the updater owns its console alone (double-click launch).
pub fn owns_console_alone() -> bool {
    should_wait(console_process_count())
}

/// Drop whatever was typed before a question was asked, so a key pressed
/// during the update cannot answer it. Console input only; a pipe is left
/// alone.
#[cfg(windows)]
fn discard_typeahead() {
    use windows_sys::Win32::System::Console::{
        FlushConsoleInputBuffer, GetStdHandle, STD_INPUT_HANDLE,
    };
    // SAFETY: both calls take plain values; a failing or non-console handle
    // makes FlushConsoleInputBuffer return an error we deliberately ignore.
    unsafe {
        FlushConsoleInputBuffer(GetStdHandle(STD_INPUT_HANDLE));
    }
}

#[cfg(not(windows))]
fn discard_typeahead() {}

/// What a timed read produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A complete line, without its line ending.
    Line(String),
    /// End of input or a read error: nobody will ever answer.
    Closed,
    TimedOut,
}

/// Line input with a timeout, fed by one background thread.
pub struct LineReader {
    rx: mpsc::Receiver<String>,
}

impl LineReader {
    /// Read lines from `source` on a background thread. The thread ends at
    /// end of input or on a read error (and dies with the process if the
    /// source never yields).
    pub fn spawn<R: BufRead + Send + 'static>(mut source: R) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || loop {
            let mut line = String::new();
            match source.read_line(&mut line) {
                // A final line without a newline was not confirmed with
                // Enter: it does not count.
                Ok(n) if n > 0 && line.ends_with('\n') => {
                    let text = line.trim_end_matches(['\n', '\r']).to_string();
                    if tx.send(text).is_err() {
                        return;
                    }
                }
                _ => return, // dropping `tx` reports Closed
            }
        });
        LineReader { rx }
    }

    pub fn read(&self, timeout: Duration) -> Input {
        match self.rx.recv_timeout(timeout) {
            Ok(line) => Input::Line(line),
            Err(RecvTimeoutError::Timeout) => Input::TimedOut,
            Err(RecvTimeoutError::Disconnected) => Input::Closed,
        }
    }
}

static STDIN_LINES: OnceLock<Mutex<LineReader>> = OnceLock::new();

/// Run `f` with the process-wide reader of standard input, starting it on
/// first use (after discarding anything typed before that moment).
pub fn with_stdin<T>(f: impl FnOnce(&LineReader) -> T) -> T {
    let reader = STDIN_LINES.get_or_init(|| {
        discard_typeahead();
        Mutex::new(LineReader::spawn(io::BufReader::new(io::stdin())))
    });
    let guard = reader
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&guard)
}

/// `text` cut to at most `max` characters.
pub fn fit(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// One console line rewritten in place.
pub struct LiveLine {
    enabled: bool,
    /// Characters of the previous draw (to blank a longer predecessor).
    last_len: usize,
    drawn: bool,
}

impl LiveLine {
    /// A live line on standard output; inert when that is not a terminal.
    pub fn stdout() -> Self {
        Self::new(io::stdout().is_terminal())
    }

    pub fn new(enabled: bool) -> Self {
        LiveLine {
            enabled,
            last_len: 0,
            drawn: false,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The bytes one draw writes: carriage return, the fitted text, and
    /// blanks over whatever the previous draw left beyond it.
    fn frame(&mut self, text: &str) -> String {
        let text = fit(text, LIVE_COLUMNS);
        let len = text.chars().count();
        let pad = self.last_len.saturating_sub(len);
        self.last_len = len;
        format!("\r{text}{}", " ".repeat(pad))
    }

    pub fn draw(&mut self, text: &str) {
        if !self.enabled {
            return;
        }
        let frame = self.frame(text);
        self.drawn = true;
        let mut out = io::stdout();
        let _ = out.write_all(frame.as_bytes());
        let _ = out.flush();
    }

    /// End the line so ordinary output continues below it.
    pub fn finish(&mut self) {
        if self.drawn {
            println!();
            self.drawn = false;
            self.last_len = 0;
        }
    }
}

/// `  [########............]  42%    77.3 / 184.1 MB`
pub fn progress_text(done: u64, total: u64) -> String {
    const CELLS: u64 = 20;
    let mb = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    if total == 0 {
        return format!("  {:.1} MB", mb(done));
    }
    let done = done.min(total);
    let filled = (done * CELLS / total) as usize;
    format!(
        "  [{}{}] {:>3}%  {:>7.1} / {:.1} MB",
        "#".repeat(filled),
        ".".repeat(CELLS as usize - filled),
        done * 100 / total,
        mb(done),
        mb(total)
    )
}

/// A download's progress as a live line, redrawn when the percentage moves
/// or a quarter of a second has passed.
pub struct Progress {
    live: LiveLine,
    last_pct: Option<u64>,
    last_draw: Option<Instant>,
}

impl Progress {
    pub fn stdout() -> Self {
        Progress {
            live: LiveLine::stdout(),
            last_pct: None,
            last_draw: None,
        }
    }

    pub fn update(&mut self, done: u64, total: u64) {
        if !self.live.enabled() {
            return;
        }
        let pct = if total == 0 {
            0
        } else {
            done.min(total) * 100 / total
        };
        let due = self
            .last_draw
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(250));
        if self.last_pct != Some(pct) || due {
            self.live.draw(&progress_text(done, total));
            self.last_pct = Some(pct);
            self.last_draw = Some(Instant::now());
        }
    }

    pub fn finish(&mut self) {
        self.live.finish();
    }
}

/// Count down from `seconds`, showing `render(seconds_left)` on a live line
/// (or once, as a plain line, when the console cannot redraw). True only
/// when a line arrives in time.
pub fn countdown(
    reader: &LineReader,
    seconds: u64,
    live: &mut LiveLine,
    render: &dyn Fn(u64) -> String,
) -> bool {
    if !live.enabled() {
        println!("{}", render(seconds));
    }
    let mut answered = false;
    for left in (1..=seconds).rev() {
        live.draw(&render(left));
        match reader.read(Duration::from_secs(1)) {
            Input::Line(_) => {
                answered = true;
                break;
            }
            Input::Closed => break,
            Input::TimedOut => {}
        }
    }
    live.finish();
    answered
}

/// Print the prompt and block until Enter, end of input, or [`WAIT_CAP`].
pub fn wait_for_enter() {
    print!(
        "Press Enter to close (closes by itself in {} s)...",
        WAIT_CAP.as_secs()
    );
    let _ = io::stdout().flush();
    with_stdin(|reader| reader.read(WAIT_CAP));
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SHORT: Duration = Duration::from_millis(40);
    const LONG: Duration = Duration::from_secs(5);

    /// A source that never yields and never ends (an idle keyboard).
    struct Silent;

    impl io::Read for Silent {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            std::thread::sleep(Duration::from_secs(3600));
            Ok(0)
        }
    }

    fn silent() -> LineReader {
        LineReader::spawn(io::BufReader::new(Silent))
    }

    fn scripted(text: &str) -> LineReader {
        LineReader::spawn(Cursor::new(text.to_string().into_bytes()))
    }

    #[test]
    fn wait_only_when_alone() {
        assert!(!should_wait(0), "no console / query failed");
        assert!(should_wait(1));
        assert!(!should_wait(2), "cmd.exe shares the console");
        assert!(!should_wait(7));
    }

    #[test]
    fn several_timed_reads_come_from_one_stream_in_order() {
        let reader = scripted("first\r\nsecond\n\nlast\n");
        assert_eq!(reader.read(LONG), Input::Line("first".into()));
        assert_eq!(reader.read(LONG), Input::Line("second".into()));
        assert_eq!(
            reader.read(LONG),
            Input::Line(String::new()),
            "a bare Enter"
        );
        assert_eq!(reader.read(LONG), Input::Line("last".into()));
        assert_eq!(reader.read(LONG), Input::Closed);
        assert_eq!(reader.read(LONG), Input::Closed, "and stays closed");
    }

    #[test]
    fn end_of_input_and_unconfirmed_text_are_not_answers() {
        assert_eq!(scripted("").read(LONG), Input::Closed);
        assert_eq!(scripted("typed but no Enter").read(LONG), Input::Closed);
    }

    #[test]
    fn silence_times_out_and_can_be_asked_again() {
        let reader = silent();
        assert_eq!(reader.read(SHORT), Input::TimedOut);
        assert_eq!(reader.read(SHORT), Input::TimedOut);
    }

    #[test]
    fn countdown_is_true_only_for_a_line() {
        let text = |n: u64| format!("continuing in {n}");
        let mut live = LiveLine::new(false);
        assert!(countdown(&scripted("\n"), 5, &mut live, &text));
        assert!(countdown(&scripted("anything\n"), 5, &mut live, &text));
        // Closed input answers "no" at once instead of waiting it out.
        let started = Instant::now();
        assert!(!countdown(&scripted(""), 5, &mut live, &text));
        assert!(started.elapsed() < Duration::from_secs(1));
        // Silence answers "no" when the time is up.
        let started = Instant::now();
        assert!(!countdown(&silent(), 1, &mut live, &text));
        assert!(started.elapsed() >= Duration::from_secs(1));
    }

    #[test]
    fn live_frames_overwrite_their_predecessor_and_never_exceed_the_width() {
        let mut live = LiveLine::new(true);
        assert_eq!(
            live.frame("starting in 10 seconds"),
            "\rstarting in 10 seconds"
        );
        assert_eq!(
            live.frame("starting in 9 seconds"),
            "\rstarting in 9 seconds "
        );
        assert_eq!(live.frame("x"), format!("\rx{}", " ".repeat(20)));
        let long = "é".repeat(200);
        let frame = live.frame(&long);
        assert_eq!(frame.chars().count(), 1 + LIVE_COLUMNS);
        assert_eq!(fit("abc", 2), "ab");
        // A disabled line draws nothing and has nothing to finish.
        let mut off = LiveLine::new(false);
        off.draw("ignored");
        assert!(!off.drawn);
    }

    #[test]
    fn progress_text_fits_and_reads_well() {
        assert_eq!(
            progress_text(0, 100 << 20),
            "  [....................]   0%      0.0 / 100.0 MB"
        );
        assert_eq!(
            progress_text(81 << 20, 193 << 20),
            "  [########............]  41%     81.0 / 193.0 MB"
        );
        assert_eq!(
            progress_text(u64::MAX, 2047 << 20),
            "  [####################] 100%   2047.0 / 2047.0 MB"
        );
        assert_eq!(progress_text(3 << 20, 0), "  3.0 MB", "unknown total");
        for (done, total) in [(0, 1), (5, 7), (1u64 << 40, 1u64 << 41)] {
            assert!(progress_text(done, total).chars().count() <= LIVE_COLUMNS);
        }
    }
}
