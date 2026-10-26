//! Console + file logging, and the `log_*!` macros the mounted DLL modules
//! expect at the crate root.
//!
//! Every line goes to the console (INFO → stdout, WARN/ERROR → stderr, DEBUG
//! only with `--verbose`) and, once [`attach_file`] has been called, to
//! `ddr_world_hook_prep.log` in the game folder (DEBUG always reaches the
//! file). Console and file lines are byte-identical so console text pasted
//! into a bug report matches the log. The prefix is elapsed time since
//! process start (`[+12.34s]`); the header carries the wall-clock unix time.
//!
//! Logging never aborts a run: a file that cannot be written produces one
//! console WARN and is then ignored.

use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Level::Debug => "DEBUG",
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

/// Console sink: receives finished lines by level.
pub type ConsoleSink = Box<dyn Fn(Level, &str) + Send>;

fn default_console_sink() -> ConsoleSink {
    Box::new(|level, line| match level {
        Level::Debug | Level::Info => println!("{line}"),
        Level::Warn | Level::Error => eprintln!("{line}"),
    })
}

pub struct Logger {
    start: Instant,
    file: Option<File>,
    file_failed: bool,
    verbose: bool,
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
            verbose: false,
            console,
        }
    }

    pub fn set_verbose(&mut self, on: bool) {
        self.verbose = on;
    }

    /// Truncate/create `path` and mirror every subsequent line into it.
    pub fn attach_file(&mut self, path: &Path) -> io::Result<()> {
        let file = File::create(path)?;
        self.file = Some(file);
        self.file_failed = false;
        Ok(())
    }

    pub fn emit(&mut self, level: Level, msg: &str) {
        let line = format_line(self.start.elapsed(), level, msg);
        if level != Level::Debug || self.verbose {
            (self.console)(level, &line);
        }
        if let Some(file) = self.file.as_mut() {
            if writeln!(file, "{line}").is_err() && !self.file_failed {
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

    pub fn header(&mut self, version: &str, argv: &[String], game_root: &Path) {
        let unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.emit(Level::Info, &format!("ddr_world_hook_prep {version}"));
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

fn with_global<R>(f: impl FnOnce(&mut Logger) -> R) -> R {
    let mut guard = match global().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    f(&mut guard)
}

pub fn attach_file(path: &Path) -> io::Result<()> {
    with_global(|l| l.attach_file(path))
}

pub fn set_verbose(on: bool) {
    with_global(|l| l.set_verbose(on));
}

pub fn header(version: &str, argv: &[String], game_root: &Path) {
    with_global(|l| l.header(version, argv, game_root));
}

pub fn emit(level: Level, msg: &str) {
    with_global(|l| l.emit(level, msg));
}

#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => { $crate::log::emit($crate::log::Level::Debug, &format!($($arg)*)) };
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
    use std::sync::{Arc, Mutex as StdMutex};

    fn capturing() -> (Logger, Arc<StdMutex<Vec<(Level, String)>>>) {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let sink = seen.clone();
        let logger = Logger::with_console(Box::new(move |level, line| {
            sink.lock().unwrap().push((level, line.to_string()));
        }));
        (logger, seen)
    }

    #[test]
    fn line_format_is_stable() {
        assert_eq!(
            format_line(Duration::from_millis(12_340), Level::Warn, "hello"),
            "[+12.34s] WARN  hello"
        );
        assert_eq!(
            format_line(Duration::ZERO, Level::Debug, "x"),
            "[+0.00s] DEBUG x"
        );
    }

    #[test]
    fn debug_reaches_the_console_only_when_verbose() {
        let (mut l, seen) = capturing();
        l.emit(Level::Debug, "hidden");
        l.emit(Level::Info, "shown");
        l.set_verbose(true);
        l.emit(Level::Debug, "now shown");
        let levels: Vec<Level> = seen.lock().unwrap().iter().map(|(lv, _)| *lv).collect();
        assert_eq!(levels, vec![Level::Info, Level::Debug]);
    }

    #[test]
    fn debug_always_reaches_the_file() {
        let dir = std::env::temp_dir().join(format!("ddr_prep_log_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.log");
        let (mut l, _) = capturing();
        l.attach_file(&path).unwrap();
        l.emit(Level::Debug, "to file");
        l.emit(Level::Info, "and this");
        drop(l);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("DEBUG to file"));
        assert!(text.contains("INFO  and this"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
