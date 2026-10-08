use crate::gateway::GatewayExecutionResult;
use crate::memory::{atomic_write, Event};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        bail!("identifier must contain 1–64 ASCII letters, digits, '-' or '_'");
    }
    Ok(())
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(value)?)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&fs::read(path).with_context(|| format!("failed to read {:?}", path))?)
        .with_context(|| format!("invalid stored JSON {:?}", path))
}

/// OS file lock is released on process exit; a leftover lock file is harmless.
pub struct Lease(File);

impl Lease {
    pub fn acquire(path: &Path) -> Result<Self> {
        Self::acquire_wait(path, std::time::Duration::ZERO)
    }

    pub fn acquire_wait(path: &Path, timeout: std::time::Duration) -> Result<Self> {
        fs::create_dir_all(path.parent().context("lock has no parent")?)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        let started = std::time::Instant::now();
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => break,
                Err(error)
                    if error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
                        && started.elapsed() < timeout =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                Err(error) => {
                    return Err(error).with_context(|| format!("resource is busy: {:?}", path))
                }
            }
        }
        Ok(Self(file))
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub run_count: usize,
    pub last_run_id: Option<String>,
    pub memory_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub session: String,
    pub instruction: String,
    pub status: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub result: Option<GatewayExecutionResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub seq: usize,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, Clone)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn session_dir(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.root.join("sessions").join(id))
    }
    pub fn run_dir(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.root.join("runs").join(id))
    }
    pub fn session(&self, id: &str) -> Result<Session> {
        read_json(&self.session_dir(id)?.join("session.json"))
    }
    pub fn run(&self, id: &str) -> Result<RunRecord> {
        read_json(&self.run_dir(id)?.join("run.json"))
    }
    pub fn sessions(&self) -> Result<Vec<Session>> {
        self.list("sessions", "session.json")
    }
    pub fn runs(&self) -> Result<Vec<RunRecord>> {
        self.list("runs", "run.json")
    }
    fn list<T: DeserializeOwned>(&self, directory: &str, filename: &str) -> Result<Vec<T>> {
        let path = self.root.join(directory);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        entries
            .into_iter()
            .filter(|entry| entry.path().join(filename).is_file())
            .map(|entry| read_json(&entry.path().join(filename)))
            .collect()
    }
    pub fn events(&self, run: &str, after: usize, limit: usize) -> Result<Vec<TraceEvent>> {
        let path = self.run_dir(run)?.join("events.jsonl");
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        content
            .split_inclusive('\n')
            .filter(|line| line.ends_with('\n'))
            .map(|line| serde_json::from_str::<TraceEvent>(line).map_err(Into::into))
            .filter(|event| event.as_ref().map_or(true, |event| event.seq > after))
            .take(limit)
            .collect()
    }
}
