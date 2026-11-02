//! Private, bounded JSONL logs. Never record RPC payloads, argv, or environments.
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

const LIMIT: u64 = 5 * 1024 * 1024;
const ROTATIONS: usize = 3;
static LOGGER: OnceLock<Logger> = OnceLock::new();

struct Logger {
    component: &'static str,
    file: Mutex<Option<RotatingFile>>,
    debug: bool,
    boot_id: String,
}
pub struct RotatingFile {
    path: PathBuf,
    file: File,
    size: u64,
    limit: u64,
}

pub fn directory() -> Result<PathBuf> {
    let state = env::var_os("XDG_STATE_HOME")
        .filter(|p| Path::new(p).is_absolute())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .context("HOME or an absolute XDG_STATE_HOME is required for file logging")?;
    Ok(state.join("igpu-man/logs"))
}

impl RotatingFile {
    fn open(path: &Path, limit: u64) -> Result<Self> {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let parent = path.parent().context("Log path has no parent")?;
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        let size = file.metadata()?.len();
        Ok(Self {
            path: path.into(),
            file,
            size,
            limit,
        })
    }
    fn write_line(&mut self, data: &[u8]) -> Result<()> {
        if data.len() as u64 > self.limit {
            anyhow::bail!("Log entry exceeds file size limit");
        }
        if self.size.saturating_add(data.len() as u64) > self.limit {
            self.file.flush()?;
            for n in (1..ROTATIONS).rev() {
                let from = self.path.with_extension(format!("jsonl.{n}"));
                let to = self.path.with_extension(format!("jsonl.{}", n + 1));
                match fs::rename(&from, &to) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            fs::rename(&self.path, self.path.with_extension("jsonl.1"))?;
            let replacement = Self::open(&self.path, self.limit)?;
            *self = replacement;
        }
        self.file.write_all(data)?;
        self.file.flush()?;
        self.size += data.len() as u64;
        Ok(())
    }
}

pub fn init(component: &'static str) {
    if LOGGER.get().is_some() {
        return;
    }
    let file = directory().and_then(|p| {
        RotatingFile::open(
            &p.join(format!("{component}-{}.jsonl", std::process::id())),
            LIMIT,
        )
    });
    let file = match file {
        Ok(file) => Some(file),
        Err(error) => {
            eprintln!(
                "igpu-man: private log file unavailable; structured logs use stderr ({})",
                error.root_cause()
            );
            None
        }
    };
    let logger = Logger {
        component,
        file: Mutex::new(file),
        debug: env::var("IGPU_MAN_LOG").as_deref() == Ok("debug"),
        boot_id: fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .map(|s| s.trim().to_owned())
            .unwrap_or_else(|_| "unknown".into()),
    };
    if LOGGER.set(logger).is_ok() {
        if let Some(logger) = LOGGER.get() {
            if log::set_logger(logger).is_ok() {
                log::set_max_level(log::LevelFilter::Debug);
            }
        }
        std::panic::set_hook(Box::new(|info| {
            let location = info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
            event(
                "error",
                "runtime.panic",
                json!({"location": location, "backtrace": std::backtrace::Backtrace::force_capture().to_string()}),
            );
        }));
        event(
            "info",
            "process.started",
            json!({"version": env!("CARGO_PKG_VERSION"),"source_sha256":crate::build::SOURCE_ID}),
        );
        if let Ok(path) = directory() {
            if let Err(error) = prune(&path, 32) {
                event("warn", "logging.prune_failed", error_fields(&error));
            }
        }
    }
}

/// Callers supply only allowlisted diagnostic fields. Do not pass arbitrary user data.
pub fn event(level: &str, name: &str, fields: Value) {
    let mut record = json!({"schema_version":1,"source_sha256":crate::build::SOURCE_ID,"unix_ms":crate::model::unix_ms(),"monotonic_ns":crate::model::monotonic_ns(),
        "component":LOGGER.get().map(|l| l.component).unwrap_or("uninitialized"),"pid":std::process::id(),"boot_id":LOGGER.get().map(|l|l.boot_id.as_str()).unwrap_or("unknown"),
        "level":level,"event":name,"fields":fields});
    let Ok(mut data) = serde_json::to_vec(&record) else {
        return;
    };
    if data.len() > 64 * 1024 {
        record["fields"] = json!({"omitted":true,"reason":"event exceeds 64 KiB"});
        let Ok(replacement) = serde_json::to_vec(&record) else {
            return;
        };
        data = replacement;
    }
    data.push(b'\n');
    let Some(logger) = LOGGER.get() else {
        return;
    };
    if level == "debug" && !logger.debug {
        return;
    }
    if let Ok(mut file) = logger.file.lock() {
        if let Some(writer) = file.as_mut() {
            if writer.write_line(&data).is_ok() {
                return;
            }
            eprintln!("igpu-man: log file write failed; falling back to stderr");
            *file = None;
        }
    }
    let _ = std::io::stderr().lock().write_all(&data);
}

/// Redact URLs and bound messages before writing library diagnostics.
fn safe_message(message: &str) -> String {
    let mut result = message.chars().take(1024).collect::<String>();
    for prefix in ["https://", "http://", "file://"] {
        while let Some(start) = result.find(prefix) {
            let end = result[start..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                .map(|n| start + n)
                .unwrap_or(result.len());
            result.replace_range(start..end, "[URL redacted]");
        }
    }
    result
}

pub fn error_fields(error: &anyhow::Error) -> Value {
    let chain: Vec<_> = error
        .chain()
        .take(8)
        .map(|cause| {
            if let Some(error) = cause.downcast_ref::<toml::de::Error>() {
                format!(
                    "TOML parse error (source omitted){}",
                    error
                        .span()
                        .map(|s| format!(" [bytes {}..{}]", s.start, s.end))
                        .unwrap_or_default()
                )
            } else if let Some(e) = cause.downcast_ref::<serde_json::Error>() {
                format!(
                    "JSON {:?} error at {}:{} (source omitted)",
                    e.classify(),
                    e.line(),
                    e.column()
                )
            } else {
                safe_message(&cause.to_string())
            }
        })
        .collect();
    json!({"causes":chain})
}

/// Safe terminal/IPC error text, including after typed causes cross a process boundary.
pub fn diagnostic_message(error: &anyhow::Error) -> String {
    error_fields(error)["causes"]
        .as_array()
        .map(|causes| {
            causes
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(": ")
        })
        .unwrap_or_else(|| "Operation failed".into())
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
            || (metadata.target().starts_with("igpu")
                && (self.debug || metadata.level() <= log::Level::Info))
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            event(
                record.level().as_str().to_ascii_lowercase().as_str(),
                "library.message",
                json!({"target":record.target(),"message":safe_message(&record.args().to_string())}),
            );
        }
    }
    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            if let Some(writer) = file.as_mut() {
                let _ = writer.file.flush();
            }
        }
    }
}

/// Remove oldest per-process logs, bounding retention across repeated app starts.
fn log_identity(name: &str) -> Option<(&str, u32)> {
    let (base, suffix) = name.split_once(".jsonl")?;
    if !["", ".1", ".2", ".3"].contains(&suffix) {
        return None;
    }
    let (component, pid) = base.split_once('-')?;
    if !["cli", "daemon", "desktop"].contains(&component)
        || !pid.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let pid = pid.parse().ok()?;
    (pid > 0).then_some((component, pid))
}

pub fn prune(directory: &Path, keep: usize) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(directory)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some((_, pid)) = log_identity(&name) else {
                return false;
            };
            e.file_type().is_ok_and(|t| t.is_file()) && !Path::new(&format!("/proc/{pid}")).exists()
        })
        .filter_map(|e| e.metadata().ok().map(|m| (m.modified().ok(), e.path())))
        .collect();
    entries.sort();
    let remove = entries.len().saturating_sub(keep);
    for (_, path) in entries.into_iter().take(remove) {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn tail(component: &str, limit: usize) -> Result<Vec<Value>> {
    use std::io::{Read, Seek, SeekFrom};
    if !["all", "cli", "daemon", "desktop"].contains(&component) || !(1..=1000).contains(&limit) {
        anyhow::bail!("Invalid log component or line limit (1–1000)");
    }
    let directory = directory()?;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    let mut records = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !log_identity(&name).is_some_and(|(c, _)| component == "all" || component == c)
            || !entry.file_type()?.is_file()
        {
            continue;
        }
        let mut file = match File::open(entry.path()) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let size = file.metadata()?.len();
        let start = size.saturating_sub(128 * 1024);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(128 * 1024).read_to_end(&mut bytes)?;
        let text = String::from_utf8_lossy(&bytes);
        for (i, line) in text.lines().enumerate() {
            if start > 0 && i == 0 {
                continue;
            } // May begin in the middle of a record.
            if let Ok(record) = serde_json::from_str::<Value>(line) {
                records.push(record);
            }
        }
    }
    records.sort_by_key(|v| {
        (
            v["unix_ms"].as_u64().unwrap_or(0),
            v["monotonic_ns"].as_u64().unwrap_or(0),
        )
    });
    let keep_from = records.len().saturating_sub(limit);
    Ok(records.into_iter().skip(keep_from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_preserves_active_and_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        let active = dir
            .path()
            .join(format!("desktop-{}.jsonl", std::process::id()));
        let unrelated = dir.path().join("notes-4294967295.jsonl");
        let invalid = dir.path().join("cli-4294967295.jsonl.backup");
        for path in [&active, &unrelated, &invalid] {
            fs::write(path, b"keep").unwrap();
        }
        for suffix in ["", ".1", ".2", ".3"] {
            fs::write(
                dir.path().join(format!("cli-4294967295.jsonl{suffix}")),
                b"old",
            )
            .unwrap();
        }
        prune(dir.path(), 1).unwrap();
        assert!(active.exists() && unrelated.exists() && invalid.exists());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 4);
        assert!(log_identity("cli-0.jsonl").is_none());
        assert!(log_identity("cli-12.jsonl.4").is_none());
    }
    #[test]
    fn rotation_and_permissions_are_bounded() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs/test.jsonl");
        let mut writer = RotatingFile::open(&path, 20).unwrap();
        for _ in 0..10 {
            writer.write_line(b"{\"test\":true}\n").unwrap();
        }
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 4);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    #[test]
    fn parser_errors_and_urls_do_not_expose_source_data() {
        let e = toml::from_str::<crate::config::Config>("secret='private'\nbroken=[").unwrap_err();
        assert!(!error_fields(&anyhow::Error::from(e))
            .to_string()
            .contains("private"));
        assert_eq!(
            safe_message("failed https://example.invalid/secret?token=abc end"),
            "failed [URL redacted] end"
        );
    }
}
