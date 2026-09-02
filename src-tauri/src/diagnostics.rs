use std::{fs, path::PathBuf};

const LOG_DIRECTORY_ENV: &str = "DCREEL_LOG_DIR";

pub fn log_directory() -> PathBuf {
    std::env::var_os(LOG_DIRECTORY_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            dirs::data_local_dir().map(|directory| directory.join("com.creel.desktop").join("logs"))
        })
        .unwrap_or_else(|| std::env::temp_dir().join("DCreel").join("logs"))
}

pub fn initialize(startup_args: &[String]) {
    let directory = log_directory();
    match creel_logging::init(&directory, "dcreel", log_level()) {
        Ok(path) => {
            creel_logging::install_panic_hook("dcreel");
            log::info!(
                target: "startup",
                "session started version={} executable={} args={:?} log={}",
                env!("CARGO_PKG_VERSION"),
                std::env::current_exe()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|error| format!("<unavailable: {error}>")),
                redacted_arguments(startup_args),
                path.display()
            );
        }
        Err(error) => {
            eprintln!("DCreel could not initialize file logging: {error}");
        }
    }
}

#[tauri::command]
pub fn write_frontend_log(level: String, message: String, source: Option<String>) {
    let message = truncate(&message, 32_768);
    let source = source
        .as_deref()
        .map(|value| truncate(value, 2_048))
        .unwrap_or_else(|| "webview".into());
    match level.to_ascii_lowercase().as_str() {
        "error" => log::error!(target: "frontend", "source={source} {message}"),
        "warn" | "warning" => log::warn!(target: "frontend", "source={source} {message}"),
        "debug" => log::debug!(target: "frontend", "source={source} {message}"),
        _ => log::info!(target: "frontend", "source={source} {message}"),
    }
}

#[tauri::command]
pub fn open_log_directory() -> Result<(), String> {
    let directory = log_directory();
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    log::info!(target: "diagnostics", "opening log directory={}", directory.display());
    opener::open(&directory).map_err(|error| error.to_string())
}

fn redacted_arguments(arguments: &[String]) -> Vec<String> {
    let mut redacted = Vec::new();
    let mut hide_next = false;
    for argument in arguments.iter().skip(1) {
        if hide_next {
            redacted.push("<path>".into());
            hide_next = false;
        } else if argument == creel_ipc::ARG_MAP_FOLDER {
            redacted.push(argument.clone());
            hide_next = true;
        } else if argument.starts_with("--") {
            redacted.push(argument.clone());
        } else {
            redacted.push("<argument>".into());
        }
    }
    redacted
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

const fn log_level() -> log::LevelFilter {
    if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    }
}
