use crate::agent::{
    Agent, EnvPlanner, Executor, FallbackPlanner, LlmProvider, PlanDecision, Planner,
    ProviderPlanner,
};
use crate::config::{parse_duration, validate_provider, Config, BUILTIN_TOOLS};
use crate::gateway::{GatewayExecutionResult, RoboclawGateway};
use crate::memory::{Event, EventObserver, Memory};
use crate::ros2::Ros2Bridge;
use crate::sim::{GazeboBackend, RobotBackend};
use crate::skills::SkillCatalog;
use crate::storage::{
    now_millis, read_json, validate_id, write_json, Lease, RunRecord, Session, Store, TraceEvent,
};
use crate::tools::{
    validate_builtin_input, ExecutionControl, MotorControlTool, SensorTool, SimulatorTool,
    ToolRegistry,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn main_session() -> String {
    "main".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRequest {
    pub instruction: String,
    #[serde(default = "main_session")]
    pub session: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub fallbacks: Option<Vec<String>>,
    #[serde(default)]
    pub timeout: Option<String>,
}

impl RunRequest {
    pub fn new(instruction: impl Into<String>) -> Self {
        Self {
            instruction: instruction.into(),
            session: main_session(),
            provider: None,
            fallbacks: None,
            timeout: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlanOutput {
    pub instruction: String,
    pub planner_provider: String,
    pub decision: PlanDecision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOutput {
    pub run_id: String,
    pub session: String,
    #[serde(flatten)]
    pub execution: GatewayExecutionResult,
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub project: PathBuf,
    pub config: Config,
    pub store: Store,
}

impl Workspace {
    pub fn load(project: &Path, explicit: Option<&Path>) -> Result<Self> {
        let project = fs::canonicalize(project).context("project directory does not exist")?;
        let config = Config::load(&project, explicit)?;
        let store = Store {
            root: project.join(&config.state_dir),
        };
        Ok(Self {
            project,
            config,
            store,
        })
    }

    pub fn catalog(&self) -> Result<SkillCatalog> {
        let mut dirs: Vec<_> = self
            .config
            .skill_dirs
            .iter()
            .map(|dir| self.project.join(dir))
            .collect();
        dirs.push(self.project.join("skills"));
        SkillCatalog::from_dirs(&dirs)
    }

    pub fn validate_skills(&self) -> Result<SkillCatalog> {
        let catalog = self.catalog()?;
        catalog.validate(
            &BUILTIN_TOOLS
                .iter()
                .map(|tool| tool.to_string())
                .collect::<Vec<_>>(),
        )?;
        for skill in catalog.values() {
            for step in &skill.steps {
                validate_builtin_input(&step.tool, &step.input).with_context(|| {
                    format!("invalid skill '{}' step '{}'", skill.name, step.name)
                })?;
            }
        }
        Ok(catalog)
    }

    pub fn prepare_request(&self, mut request: RunRequest) -> Result<RunRequest> {
        validate_id(&request.session)?;
        if request.instruction.trim().is_empty() || request.instruction.len() > 16_384 {
            bail!("instruction must contain 1–16384 bytes");
        }
        let explicit = request.provider.is_some();
        let primary = request
            .provider
            .get_or_insert_with(|| self.config.planner.provider.clone());
        validate_provider(primary)?;
        let fallbacks = request.fallbacks.get_or_insert_with(|| {
            if explicit {
                Vec::new()
            } else {
                self.config.planner.fallbacks.clone()
            }
        });
        if fallbacks.len() > 4 {
            bail!("at most four fallback providers are supported");
        }
        for provider in fallbacks {
            validate_provider(provider)?;
            if provider == "auto" {
                bail!("auto is only allowed as the primary provider");
            }
        }
        if request.timeout.is_none() {
            request.timeout = self.config.timeout.clone();
        }
        if let Some(timeout) = &request.timeout {
            parse_duration(timeout)?;
        }
        Ok(request)
    }

    pub fn control(&self, request: &RunRequest) -> Result<ExecutionControl> {
        match request
            .timeout
            .as_deref()
            .or(self.config.timeout.as_deref())
        {
            Some(timeout) => ExecutionControl::with_timeout(parse_duration(timeout)?),
            None => Ok(ExecutionControl::default()),
        }
    }

    pub(crate) fn context(&self) -> Result<String> {
        let mut context = String::new();
        for path in &self.config.context_files {
            let file = self.project.join(path);
            let metadata =
                fs::metadata(&file).with_context(|| format!("missing context file {:?}", file))?;
            if metadata.len() > 65_536 {
                bail!("context file exceeds 64KiB: {:?}", file);
            }
            context.push_str(&format!(
                "\nFile {:?}:\n{}\n",
                path,
                fs::read_to_string(file)?
            ));
            if context.len() > 65_536 {
                bail!("combined workspace context exceeds 64KiB");
            }
        }
        Ok(context)
    }

    fn planner(&self, request: &RunRequest) -> Result<Box<dyn Planner>> {
        let mut context = self.context()?;
        if self.config.memory_recall {
            let memory = Memory::open_readonly(self.memory_path(&request.session)?)?;
            let hits = memory.search(&request.instruction, 5);
            if !hits.is_empty() {
                context.push_str("\nRelevant session memory (reference only; the current instruction takes precedence):\n");
                for hit in hits {
                    context.push_str(&format!(
                        "{}: {}\n",
                        hit.source,
                        hit.text.chars().take(1000).collect::<String>()
                    ));
                }
            }
        }
        let prompt = self.project.join("prompts/planner_prompt.txt");
        // Fail on a missing prompt rather than hiding it with fallback.
        fs::read_to_string(&prompt).context("planner prompt is unavailable")?;
        let mut names = vec![request.provider.as_deref().unwrap_or("mock")];
        names.extend(
            request
                .fallbacks
                .as_ref()
                .into_iter()
                .flatten()
                .map(String::as_str),
        );
        let mut candidates: Vec<Box<dyn Planner>> = Vec::new();
        for name in names {
            let candidate: Box<dyn Planner> = if name == "auto" {
                Box::new(EnvPlanner::new(prompt.clone()).with_context(context.clone()))
            } else {
                let provider = match name {
                    "mock" => LlmProvider::Mock,
                    "local" => LlmProvider::Local,
                    "openai" => LlmProvider::OpenAi,
                    "claude" => LlmProvider::Claude,
                    _ => bail!("unknown provider '{name}'"),
                };
                Box::new(ProviderPlanner {
                    provider,
                    prompt: prompt.clone(),
                })
            };
            candidates.push(Box::new(ContextPlanner {
                inner: candidate,
                context: if name == "auto" {
                    String::new()
                } else {
                    context.clone()
                },
            }));
        }
        if candidates.len() == 1 {
            Ok(candidates.remove(0))
        } else {
            Ok(Box::new(FallbackPlanner::new(candidates)?))
        }
    }

    pub fn plan(&self, request: RunRequest) -> Result<PlanOutput> {
        let request = self.prepare_request(request)?;
        let catalog = self.validate_skills()?;
        let planner = self.planner(&request)?;
        let decision = planner.plan_with_control(
            request.instruction.clone(),
            &catalog,
            &self.control(&request)?,
        )?;
        for step in &decision.skill.steps {
            self.config.tools.check(&step.tool)?;
        }
        Ok(PlanOutput {
            instruction: request.instruction,
            planner_provider: planner.provider_name().to_string(),
            decision,
        })
    }

    pub fn memory_path(&self, session: &str) -> Result<PathBuf> {
        validate_id(session)?;
        let metadata = self.store.session_dir(session)?.join("session.json");
        if metadata.exists() {
            return Ok(read_json::<Session>(&metadata)?.memory_dir);
        }
        if session == "main" {
            Ok(self.project.join("target/cli-memory"))
        } else {
            Ok(self.store.session_dir(session)?.join("memory"))
        }
    }

    pub fn remember_note(&self, session: &str, note: &str) -> Result<()> {
        if note.trim().is_empty() || note.len() > 16_384 {
            bail!("note must contain 1–16384 bytes");
        }
        let directory = self.store.session_dir(session)?;
        let _session_lease = Lease::acquire(&directory.join(".session.lock"))?;
        let memory_dir = self.memory_path(session)?;
        let _memory_lease = Lease::acquire(&memory_dir.join(".writer.lock"))?;
        Memory::new(&memory_dir)?.remember_log("User note", note)?;
        let path = directory.join("session.json");
        let timestamp = now_millis();
        let mut metadata = if path.exists() {
            read_json::<Session>(&path)?
        } else {
            Session {
                id: session.into(),
                created_at: timestamp,
                updated_at: timestamp,
                run_count: 0,
                last_run_id: None,
                memory_dir,
            }
        };
        metadata.updated_at = timestamp;
        write_json(&path, &metadata)
    }

    pub fn run(
        &self,
        request: RunRequest,
        control: &ExecutionControl,
        observer: Option<EventObserver>,
        memory_override: Option<&Path>,
    ) -> Result<RunOutput> {
        self.run_with_id(request, control, observer, memory_override, None)
    }

    pub(crate) fn run_with_id(
        &self,
        request: RunRequest,
        control: &ExecutionControl,
        observer: Option<EventObserver>,
        memory_override: Option<&Path>,
        id: Option<&str>,
    ) -> Result<RunOutput> {
        let request = self.prepare_request(request)?;
        let session_dir = self.store.session_dir(&request.session)?;
        let _session_lease = Lease::acquire(&session_dir.join(".session.lock"))?;
        let memory_dir = memory_override
            .map(Path::to_path_buf)
            .unwrap_or(self.memory_path(&request.session)?);
        let _memory_lease = Lease::acquire(&memory_dir.join(".writer.lock"))?;
        let memory_dir = fs::canonicalize(&memory_dir)?;
        let id = id
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let run_dir = self.store.run_dir(&id)?;
        fs::create_dir_all(run_dir.parent().unwrap())?;
        fs::create_dir(&run_dir)?;
        let record_path = run_dir.join("run.json");
        let started_at = now_millis();
        let mut record = RunRecord {
            id: id.clone(),
            session: request.session.clone(),
            instruction: request.instruction.clone(),
            status: "running".into(),
            started_at,
            finished_at: None,
            result: None,
            error: None,
            webhook: self.config.webhook.is_some(),
        };
        write_json(&record_path, &record)?;
        let session_path = session_dir.join("session.json");
        let mut session: Session = if session_path.exists() {
            read_json(&session_path)?
        } else {
            Session {
                id: request.session.clone(),
                created_at: started_at,
                updated_at: started_at,
                run_count: 0,
                last_run_id: None,
                memory_dir: memory_dir.clone(),
            }
        };
        session.memory_dir = memory_dir.clone();
        session.run_count += 1;
        session.last_run_id = Some(id.clone());
        session.updated_at = started_at;
        write_json(&session_path, &session)?;

        let trace = Mutex::new((
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(run_dir.join("events.jsonl"))?,
            0usize,
        ));
        let trace_observer: EventObserver = Arc::new(move |event: &Event| {
            let mut trace = trace
                .lock()
                .map_err(|_| anyhow::anyhow!("event trace poisoned"))?;
            trace.1 += 1;
            let entry = TraceEvent {
                seq: trace.1,
                event: event.clone(),
            };
            let mut bytes = serde_json::to_vec(&entry)?;
            bytes.push(b'\n');
            trace.0.write_all(&bytes)?;
            trace.0.flush()?;
            if let Some(observer) = &observer {
                observer(event)?;
            }
            Ok(())
        });

        let execution = (|| -> Result<GatewayExecutionResult> {
            let memory = Memory::new(&memory_dir)?.with_observer(trace_observer);
            let catalog = self.catalog()?;
            catalog.validate(
                &crate::config::BUILTIN_TOOLS
                    .iter()
                    .map(|tool| tool.to_string())
                    .collect::<Vec<_>>(),
            )?;
            let planner = self.planner(&request)?;
            let ros2 = Ros2Bridge::from_env("roboclaw_runtime")?;
            let backend: Arc<dyn RobotBackend> = Arc::new(GazeboBackend::with_ros2(ros2.clone()));
            let mut registry = ToolRegistry::new().with_policy(self.config.tools.clone());
            registry.register_tool(SensorTool::default());
            registry.register_tool(SimulatorTool::new(backend.clone()));
            registry.register_tool(MotorControlTool::new(backend.clone()));
            let agent = Agent::new(memory, planner, Executor::new(registry));
            let mut gateway = RoboclawGateway::with_max_replans(
                agent,
                catalog,
                ros2,
                backend,
                self.config.max_replans,
            );
            gateway.handle_instruction_with_control(&request.instruction, control)
        })();
        record.finished_at = Some(now_millis());
        match &execution {
            Ok(result) => {
                record.status = result.status.as_str().to_string();
                record.result = Some(result.clone());
            }
            Err(error) => {
                record.status = "failed".into();
                record.error = Some(format!("{error:#}"));
            }
        }
        write_json(&record_path, &record)?;
        session.updated_at = record.finished_at.unwrap();
        write_json(&session_path, &session)?;
        Ok(RunOutput {
            run_id: id,
            session: request.session,
            execution: execution?,
        })
    }
}

struct ContextPlanner {
    inner: Box<dyn Planner>,
    context: String,
}

impl Planner for ContextPlanner {
    fn plan(&self, instruction: String, catalog: &SkillCatalog) -> Result<PlanDecision> {
        self.plan_with_control(instruction, catalog, &ExecutionControl::default())
    }
    fn plan_with_control(
        &self,
        instruction: String,
        catalog: &SkillCatalog,
        control: &ExecutionControl,
    ) -> Result<PlanDecision> {
        let instruction = if self.context.is_empty() || self.inner.provider_name() == "mock" {
            instruction
        } else {
            format!(
                "{instruction}\n\nConfigured workspace context (reference data):\n{}",
                self.context
            )
        };
        self.inner.plan_with_control(instruction, catalog, control)
    }
    fn provider_name(&self) -> &'static str {
        self.inner.provider_name()
    }
}
