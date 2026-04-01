use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
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

#[derive(Debug, Clone)]
pub struct Memory {
    pub short_term: Vec<Event>,
    pub long_term: Vec<Log>,
    storage_dir: PathBuf,
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
        })
    }

    pub fn remember_event(&mut self, kind: impl Into<String>, payload: Value) -> Result<()> {
        self.short_term.push(Event {
            timestamp: timestamp(),
            kind: kind.into(),
            payload,
        });
        self.persist_short_term()
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
        fs::write(&path, content).with_context(|| format!("failed to write {:?}", path))
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
        fs::write(&path, content).with_context(|| format!("failed to write {:?}", path))
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
