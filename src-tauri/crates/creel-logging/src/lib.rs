use log::{LevelFilter, Log, Metadata, Record};
use std::{
    backtrace::Backtrace,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const MAX_CRASH_BYTES: u64 = 2 * 1024 * 1024;
const RETAINED_LOGS: usize = 5;

static LOG_DIRECTORY: OnceLock<PathBuf> = OnceLock::new();
static PANIC_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

struct FileLogger {
    level: LevelFilter,
    writer: Mutex<RotatingWriter>,
}

struct RotatingWriter {
    path: PathBuf,
    writer: BufWriter<File>,
    bytes_written: u64,
}

impl RotatingWriter {
    fn open(path: PathBuf) -> io::Result<Self> {
        let file = open_append(&path)?;
        let bytes_written = file.metadata()?.len();
        Ok(Self {
            path,
            writer: BufWriter::new(file),
            bytes_written,
        })
    }

    fn write_line(&mut self, line: &[u8], flush: bool) -> io::Result<()> {
        if self.bytes_written.saturating_add(line.len() as u64) > MAX_LOG_BYTES {
            self.rotate()?;
        }
        self.writer.write_all(line)?;
        self.bytes_written = self.bytes_written.saturating_add(line.len() as u64);
        if flush {
            self.writer.flush()?;
        }
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        rotate_files(&self.path, RETAINED_LOGS)?;
        let file = open_append(&self.path)?;
        self.writer = BufWriter::new(file);
        self.bytes_written = 0;
        Ok(())
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("unnamed");
        let timestamp = timestamp();
        let line = format!(
            "{timestamp} {:<5} pid={} thread={thread_name} target={} {}\n",
            record.level(),
            std::process::id(),
            record.target(),
            record.args()
        );
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_line(line.as_bytes(), true);
        }
    }

    fn flush(&self) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.writer.flush();
        }
    }
}

pub fn init(log_directory: &Path, file_stem: &str, level: LevelFilter) -> io::Result<PathBuf> {
    fs::create_dir_all(log_directory)?;
    let path = log_directory.join(format!("{file_stem}.log"));
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES)
    {
        rotate_files(&path, RETAINED_LOGS)?;
    }
    let logger = FileLogger {
        level,
        writer: Mutex::new(RotatingWriter::open(path.clone())?),
    };
    let _ = LOG_DIRECTORY.set(log_directory.to_path_buf());
    log::set_boxed_logger(Box::new(logger))
        .map_err(|error| io::Error::other(format!("cannot install logger: {error}")))?;
    log::set_max_level(level);
    Ok(path)
}

pub fn install_panic_hook(component: &'static str) {
    if PANIC_HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "unknown location".into());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        let report = format!(
            "{} CRASH component={component} pid={} thread={:?} location={location}\npanic: {payload}\nbacktrace:\n{}\n",
            timestamp(),
            std::process::id(),
            std::thread::current().name(),
            Backtrace::force_capture()
        );
        log::error!(target: "panic", "{report}");
        log::logger().flush();
        let _ = append_crash_report(component, report.as_bytes());
        previous(info);
    }));
}

pub fn flush() {
    log::logger().flush();
}

fn append_crash_report(component: &str, report: &[u8]) -> io::Result<()> {
    let Some(directory) = LOG_DIRECTORY.get() else {
        return Ok(());
    };
    let path = directory.join(format!("{component}-crash.log"));
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= MAX_CRASH_BYTES)
    {
        rotate_files(&path, 2)?;
    }
    let mut file = open_append(&path)?;
    file.write_all(report)?;
    file.flush()
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn rotate_files(path: &Path, retained: usize) -> io::Result<()> {
    if retained == 0 {
        if path.exists() {
            fs::remove_file(path)?;
        }
        return Ok(());
    }
    for index in (1..retained).rev() {
        let source = rotated_path(path, index);
        let destination = rotated_path(path, index + 1);
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        if source.exists() {
            fs::rename(source, destination)?;
        }
    }
    let first = rotated_path(path, 1);
    if first.exists() {
        fs::remove_file(&first)?;
    }
    if path.exists() {
        fs::rename(path, first)?;
    }
    Ok(())
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("log");
    path.with_extension(format!("{extension}.{index}"))
}

fn timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown-time".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_paths_keep_the_log_extension_visible() {
        let path = Path::new("dcreel.log");
        assert_eq!(rotated_path(path, 1), PathBuf::from("dcreel.log.1"));
        assert_eq!(rotated_path(path, 5), PathBuf::from("dcreel.log.5"));
    }
}
