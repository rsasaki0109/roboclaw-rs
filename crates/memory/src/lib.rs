use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub timestamp: String,
    pub kind: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    pub timestamp: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone)]
pub struct Memory {
    pub short_term: Vec<Event>,
    pub long_term: Vec<Log>,
    storage_dir: PathBuf,
    observer: Option<EventObserver>,
}

pub type EventObserver = Arc<dyn Fn(&Event) -> Result<()> + Send + Sync>;

impl std::fmt::Debug for Memory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Memory")
            .field("storage_dir", &self.storage_dir)
            .field("short_term", &self.short_term)
            .field("long_term", &self.long_term)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryHit {
    pub source: String,
    pub timestamp: String,
    pub text: String,
    pub score: usize,
}

/// Publish a fully synced file through an atomic rename in the same directory.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".roboclaw-{}-{}-{}.tmp",
        std::process::id(),
        timestamp(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let write = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if write.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write.with_context(|| format!("failed to atomically write {:?}", path))
}

impl Memory {
    pub fn new(storage_dir: impl AsRef<Path>) -> Result<Self> {
        let storage_dir = storage_dir.as_ref().to_path_buf();
        fs::create_dir_all(&storage_dir)
            .with_context(|| format!("failed to create memory directory {:?}", storage_dir))?;

        let short_term = Self::load_short_term(&storage_dir)?;
        let long_term = Self::load_long_term(&storage_dir)?;

        Ok(Self {
            short_term,
            long_term,
            storage_dir,
            observer: None,
        })
    }

    pub fn open_readonly(storage_dir: impl AsRef<Path>) -> Result<Self> {
        let storage_dir = storage_dir.as_ref().to_path_buf();
        Ok(Self {
            short_term: Self::load_short_term(&storage_dir)?,
            long_term: Self::load_long_term(&storage_dir)?,
            storage_dir,
            observer: None,
        })
    }

    pub fn with_observer(mut self, observer: EventObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    /// Deterministic local keyword search; no model calls or embeddings required.
    pub fn search(&self, query: &str, limit: usize) -> Vec<MemoryHit> {
        let query = query.trim().to_lowercase();
        let tokens: Vec<_> = query.split_whitespace().collect();
        if tokens.is_empty() {
            return Vec::new();
        }
        let candidates = self
            .short_term
            .iter()
            .map(|event| MemoryHit {
                source: "event".to_string(),
                timestamp: event.timestamp.clone(),
                text: format!("{} {}", event.kind, event.payload),
                score: 0,
            })
            .chain(self.long_term.iter().map(|log| MemoryHit {
                source: "log".to_string(),
                timestamp: log.timestamp.clone(),
                text: format!("{}\n{}", log.title, log.body),
                score: 0,
            }));
        let mut hits: Vec<_> = candidates
            .enumerate()
            .filter_map(|(index, mut hit)| {
                let text = hit.text.to_lowercase();
                hit.score = tokens.iter().filter(|token| text.contains(**token)).count();
                if hit.score == 0 {
                    return None;
                }
                if text.contains(&query) {
                    hit.score += 2;
                }
                Some((index, hit))
            })
            .collect();
        hits.sort_by(|(a_index, a), (b_index, b)| {
            b.score
                .cmp(&a.score)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
                .then_with(|| b_index.cmp(a_index))
        });
        hits.into_iter().take(limit).map(|(_, hit)| hit).collect()
    }

    pub fn remember_event(&mut self, kind: impl Into<String>, payload: Value) -> Result<()> {
        self.short_term.push(Event {
            timestamp: timestamp(),
            kind: kind.into(),
            payload,
        });
        self.persist_short_term()?;
        if let Some(observer) = &self.observer {
            observer(self.short_term.last().unwrap())?;
        }
        Ok(())
    }

    pub fn remember_log(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Result<()> {
        self.long_term.push(Log {
            timestamp: timestamp(),
            title: title.into(),
            body: body.into(),
        });
        self.persist_long_term()
    }

    pub fn storage_dir(&self) -> &Path {
        &self.storage_dir
    }

    fn short_term_path(storage_dir: &Path) -> PathBuf {
        storage_dir.join("short_term.json")
    }

    fn long_term_path(storage_dir: &Path) -> PathBuf {
        storage_dir.join("long_term.md")
    }

    fn load_short_term(storage_dir: &Path) -> Result<Vec<Event>> {
        let path = Self::short_term_path(storage_dir);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content =
            fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
        serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
    }

    fn load_long_term(storage_dir: &Path) -> Result<Vec<Log>> {
        let path = Self::long_term_path(storage_dir);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content =
            fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
        Ok(parse_markdown_logs(&content))
    }

    fn persist_short_term(&self) -> Result<()> {
        let path = Self::short_term_path(&self.storage_dir);
        let content = serde_json::to_string_pretty(&self.short_term)?;
        atomic_write(&path, content.as_bytes())
    }

    fn persist_long_term(&self) -> Result<()> {
        let path = Self::long_term_path(&self.storage_dir);
        let mut content = String::new();
        for log in &self.long_term {
            content.push_str(&format!(
                "## {} | {}\n{}\n\n",
                log.timestamp, log.title, log.body
            ));
        }
        atomic_write(&path, content.as_bytes())
    }
}

fn parse_markdown_logs(content: &str) -> Vec<Log> {
    content
        .split("\n## ")
        .filter_map(|chunk| {
            let trimmed = chunk.trim();
            if trimmed.is_empty() {
                return None;
            }

            let normalized = trimmed.strip_prefix("## ").unwrap_or(trimmed);
            let mut lines = normalized.lines();
            let header = lines.next().unwrap_or_default();
            let body = lines.collect::<Vec<_>>().join("\n").trim().to_string();
            let (timestamp, title) = header
                .split_once(" | ")
                .map(|(ts, name)| (ts.to_string(), name.to_string()))
                .unwrap_or_else(|| ("unknown".to_string(), header.to_string()));

            Some(Log {
                timestamp,
                title,
                body,
            })
        })
        .collect()
}

fn timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
