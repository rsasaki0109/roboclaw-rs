use crate::tools::ToolPolicy;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const BUILTIN_TOOLS: [&str; 3] = ["sensor", "simulator", "motor_control"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlannerConfig {
    pub provider: String,
    pub fallbacks: Vec<String>,
}

impl Default for PlannerConfig {
    fn default() -> Self {
        Self {
            provider: "mock".into(),
            fallbacks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub planner: PlannerConfig,
    pub skill_dirs: Vec<PathBuf>,
    pub context_files: Vec<PathBuf>,
    pub memory_recall: bool,
    pub state_dir: PathBuf,
    pub timeout: Option<String>,
    pub max_replans: usize,
    pub tools: ToolPolicy,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            planner: PlannerConfig::default(),
            skill_dirs: Vec::new(),
            context_files: Vec::new(),
            memory_recall: true,
            state_dir: "target/roboclaw".into(),
            timeout: None,
            max_replans: 1,
            tools: ToolPolicy::default(),
        }
    }
}

impl Config {
    pub fn load(project: &Path, explicit: Option<&Path>) -> Result<Self> {
        let path = explicit
            .map(Path::to_path_buf)
            .unwrap_or_else(|| project.join("roboclaw.yaml"));
        let config = match fs::read_to_string(&path) {
            Ok(content) => serde_yaml::from_str(&content)
                .with_context(|| format!("invalid config {:?}", path))?,
            Err(error) if explicit.is_none() && error.kind() == std::io::ErrorKind::NotFound => {
                Self::default()
            }
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read config {:?}", path))
            }
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("unsupported config version {}", self.version);
        }
        validate_provider(&self.planner.provider)?;
        if self.planner.fallbacks.len() > 4 {
            bail!("at most four fallback providers are supported");
        }
        for provider in &self.planner.fallbacks {
            validate_provider(provider)?;
            if provider == "auto" {
                bail!("auto is only allowed as the primary provider");
            }
        }
        if let Some(timeout) = &self.timeout {
            parse_duration(timeout)?;
        }
        if self.max_replans > 10 {
            bail!("max_replans must not exceed 10");
        }
        if self.state_dir.as_os_str().is_empty() {
            bail!("state_dir must not be empty");
        }
        for name in self
            .tools
            .deny
            .iter()
            .chain(self.tools.allow.iter().flatten())
        {
            if !BUILTIN_TOOLS.contains(&name.as_str()) {
                bail!("unknown configured tool '{name}'");
            }
        }
        Ok(())
    }
}

pub fn validate_provider(provider: &str) -> Result<()> {
    if !["mock", "auto", "local", "openai", "claude"].contains(&provider) {
        bail!("unknown planner provider '{provider}'");
    }
    Ok(())
}

pub fn parse_duration(value: &str) -> Result<Duration> {
    let duration = humantime::parse_duration(value)?;
    if duration.is_zero() {
        bail!("timeout must be greater than zero");
    }
    if std::time::Instant::now().checked_add(duration).is_none() {
        bail!("timeout is too large");
    }
    Ok(duration)
}
