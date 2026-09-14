//! Console + file logging (design §4.13).
//!
//! Every line goes to the console (INFO → stdout, WARN/ERROR → stderr) and,
//! once [`attach_file`] has been called, to `ddr_world_hook_updater.log` in the
//! game folder. Console and file lines are byte-identical — operators paste
//! console text into bug reports, so one format keeps that unambiguous. The
//! prefix is elapsed time since process start (`[+12.34s]`), which needs no
//! date/time crate; the header line carries the wall-clock unix time instead.
//!
//! Logging must never abort a run: a file that cannot be written produces one
//! console WARN and is then ignored.

use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `pad` (not `write_str`) so `{:<5}` width formatting applies.
        f.pad(match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        })
    }
}

/// One log line: `[+12.34s] LEVEL  message` (level padded to five columns).
pub fn format_line(elapsed: Duration, level: Level, msg: &str) -> String {
    format!("[+{:.2}s] {:<5} {}", elapsed.as_secs_f64(), level, msg)
}

/// Console sink: receives finished lines by level. Injectable so tests can
/// capture console output; the default writes to stdout / stderr.
pub type ConsoleSink = Box<dyn Fn(Level, &str) + Send>;

fn default_console_sink() -> ConsoleSink {
    Box::new(|level, line| match level {
        Level::Info => println!("{line}"),
        Level::Warn | Level::Error => eprintln!("{line}"),
    })
}

/// A logger instance. The process uses one global instance behind the
/// `log_*!` macros; tests build their own.
pub struct Logger {
    start: Instant,
    file: Option<File>,
    file_failed: bool,
    console: ConsoleSink,
}

impl Logger {
    pub fn new() -> Self {
        Self::with_console(default_console_sink())
    }

    pub fn with_console(console: ConsoleSink) -> Self {
        Self {
            start: Instant::now(),
            file: None,
            file_failed: false,
            console,
        }
    }

    /// Truncate/create `path` and mirror every subsequent line into it.
    pub fn attach_file(&mut self, path: &Path) -> io::Result<()> {
        let file = File::create(path)?;
        self.file = Some(file);
        self.file_failed = false;
        Ok(())
    }

    /// Emit one line to the console and, if attached, the file.
    pub fn emit(&mut self, level: Level, msg: &str) {
        let line = format_line(self.start.elapsed(), level, msg);
        (self.console)(level, &line);
        if let Some(file) = self.file.as_mut() {
            if writeln!(file, "{line}").is_err() && !self.file_failed {
                // Report once, then keep going: logging never aborts the run.
                self.file_failed = true;
                self.file = None;
                let warn = format_line(
                    self.start.elapsed(),
                    Level::Warn,
                    "could not write to the log file; continuing with console output only",
                );
                (self.console)(Level::Warn, &warn);
            }
        }
    }

    /// The run header: updater version, wall-clock unix time, game folder and
    /// the argument list — the first thing a field log shows.
    pub fn header(&mut self, version: &str, argv: &[String], game_root: &Path) {
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.emit(Level::Info, &format!("ddr_world_hook_updater {version}"));
        self.emit(Level::Info, &format!("unix time: {unix}"));
        self.emit(
            Level::Info,
            &format!("game folder: {}", game_root.display()),
        );
        self.emit(Level::Info, &format!("arguments: {}", argv.join(" ")));
    }
}

impl Default for Logger {
    fn default() -> Self {
        Self::new()
    }
}

static LOGGER: OnceLock<Mutex<Logger>> = OnceLock::new();

fn global() -> &'static Mutex<Logger> {
    LOGGER.get_or_init(|| Mutex::new(Logger::new()))
}

/// Run `f` against the global logger, tolerating a poisoned mutex (a panic
/// while holding the lock must not silence all later output).
fn with_global<R>(f: impl FnOnce(&mut Logger) -> R) -> R {
    let mut guard = match global().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(&mut guard)
}

/// Attach the global logger to a file (truncating it).
pub fn attach_file(path: &Path) -> io::Result<()> {
    with_global(|l| l.attach_file(path))
}

/// Write the run header through the global logger.
pub fn header(version: &str, argv: &[String], game_root: &Path) {
    with_global(|l| l.header(version, argv, game_root));
}

/// Emit through the global logger (used by the macros).
pub fn emit(level: Level, msg: &str) {
    with_global(|l| l.emit(level, msg));
}

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::log::emit($crate::log::Level::Info, &format!($($arg)*)) };
}

#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::log::emit($crate::log::Level::Warn, &format!($($arg)*)) };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::log::emit($crate::log::Level::Error, &format!($($arg)*)) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct Temp(PathBuf);

    impl Temp {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("ddr_updater_log_test_{}_{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Temp(dir)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    type Captured = Arc<StdMutex<Vec<(Level, String)>>>;

    fn capturing_logger() -> (Logger, Captured) {
        let captured: Captured = Arc::new(StdMutex::new(Vec::new()));
        let sink = captured.clone();
        let logger = Logger::with_console(Box::new(move |level, line| {
            sink.lock().unwrap().push((level, line.to_string()));
        }));
        (logger, captured)
    }

    fn line_matches_prefix(line: &str) -> bool {
        // `[+D.DDs] LEVEL ` where LEVEL is INFO/WARN/ERROR padded to 5.
        let Some(rest) = line.strip_prefix("[+") else {
            return false;
        };
        let Some((num, rest)) = rest.split_once("s] ") else {
            return false;
        };
        let Some((int, frac)) = num.split_once('.') else {
            return false;
        };
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        digits(int)
            && frac.len() == 2
            && digits(frac)
            && (rest.starts_with("INFO  ")
                || rest.starts_with("WARN  ")
                || rest.starts_with("ERROR "))
    }

    #[test]
    fn l1_format_line_is_pinned() {
        let line = format_line(Duration::from_millis(12_345), Level::Warn, "x");
        assert_eq!(line, "[+12.35s] WARN  x");
        assert_eq!(
            format_line(Duration::ZERO, Level::Error, "e"),
            "[+0.00s] ERROR e"
        );
        assert_eq!(
            format_line(Duration::from_secs(1), Level::Info, "i"),
            "[+1.00s] INFO  i"
        );
        assert!(line_matches_prefix(&line));
    }

    #[test]
    fn l2_attach_truncates_and_mirrors_lines_in_order() {
        let t = Temp::new();
        let path = t.0.join("run.log");
        fs::write(&path, "OLD CONTENT\n").unwrap();
        let (mut logger, _) = capturing_logger();
        logger.attach_file(&path).unwrap();
        logger.emit(Level::Info, "first");
        logger.emit(Level::Warn, "second");
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("OLD"), "{text}");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert!(lines[0].ends_with("INFO  first") && line_matches_prefix(lines[0]));
        assert!(lines[1].ends_with("WARN  second") && line_matches_prefix(lines[1]));
    }

    #[test]
    fn l3_header_lines() {
        let (mut logger, captured) = capturing_logger();
        let argv = vec!["--check".to_string(), "--force".to_string()];
        logger.header("0.1.0", &argv, Path::new("/game"));
        let lines = captured.lock().unwrap();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].1.ends_with("ddr_world_hook_updater 0.1.0"));
        let unix: u64 = lines[1]
            .1
            .rsplit("unix time: ")
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert!(unix > 1_700_000_000, "{unix}");
        assert!(lines[2].1.ends_with("game folder: /game"));
        assert!(lines[3].1.ends_with("arguments: --check --force"));
    }

    #[test]
    fn l4_unwritable_log_file_never_aborts() {
        let t = Temp::new();
        let (mut logger, captured) = capturing_logger();
        // A directory cannot be created as a file.
        assert!(logger.attach_file(&t.0).is_err());
        logger.emit(Level::Info, "still running");
        assert_eq!(captured.lock().unwrap().len(), 1);

        // Attach successfully, then make writes fail by removing the directory.
        let path = t.0.join("gone").join("run.log");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        logger.attach_file(&path).unwrap();
        drop(fs::remove_dir_all(t.0.join("gone")));
        // On most platforms writes to the unlinked handle still succeed; the
        // contract under test is only that emit never panics or errors out.
        logger.emit(Level::Info, "after removal");
        assert!(captured.lock().unwrap().len() >= 2);
    }

    #[test]
    fn l5_console_only_before_attach_and_level_routing() {
        let (mut logger, captured) = capturing_logger();
        logger.emit(Level::Info, "a");
        logger.emit(Level::Error, "b");
        let lines = captured.lock().unwrap();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].0, Level::Info);
        assert!(lines[0].1.ends_with("INFO  a"));
        assert_eq!(lines[1].0, Level::Error);
        assert!(lines[1].1.ends_with("ERROR b"));
    }

    #[test]
    fn global_facade_and_macros_work() {
        let t = Temp::new();
        let path = t.0.join("global.log");
        attach_file(&path).unwrap();
        crate::log_info!("hello {}", 1);
        crate::log_warn!("careful");
        crate::log_error!("bad");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("INFO  hello 1"), "{text}");
        assert!(text.contains("WARN  careful"), "{text}");
        assert!(text.contains("ERROR bad"), "{text}");
        for line in text.lines() {
            assert!(line_matches_prefix(line), "{line}");
        }
    }
}
