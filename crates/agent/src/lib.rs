use anyhow::{anyhow, Context, Result};
use reqwest::blocking::Client;
use roboclaw_memory::Memory;
use roboclaw_skills::{RecoveryContext, Skill, SkillCatalog, SkillStep};
use roboclaw_tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

pub trait Planner: Send + Sync {
    fn plan(&self, instruction: String, catalog: &SkillCatalog) -> Result<PlanDecision>;

    fn provider_name(&self) -> &'static str {
        "planner"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDecision {
    pub skill: Skill,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerTurnDebug {
    pub is_replan: bool,
    pub allowed_skills: Vec<String>,
    pub matching_recovery_skills: Vec<String>,
    pub prompt: String,
    pub schema: Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum LlmProvider {
    OpenAi,
    Claude,
    Local,
    Mock,
}

impl LlmProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            LlmProvider::OpenAi => "openai",
            LlmProvider::Claude => "claude",
            LlmProvider::Local => "local",
            LlmProvider::Mock => "mock",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FilePromptPlanner {
    provider: LlmProvider,
    prompt_template: String,
}

impl FilePromptPlanner {
    pub fn from_file(provider: LlmProvider, path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self {
            provider,
            prompt_template: load_prompt_template(path)?,
        })
    }

    pub fn provider(&self) -> LlmProvider {
        self.provider
    }

    pub fn prompt_template(&self) -> &str {
        &self.prompt_template
    }
}

#[derive(Debug, Clone)]
pub struct OllamaPlannerConfig {
    pub host: String,
    pub model: String,
    pub timeout: Duration,
}

impl OllamaPlannerConfig {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            host: "http://127.0.0.1:11434".to_string(),
            model: model.into(),
            timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OllamaPlanner {
    config: OllamaPlannerConfig,
    prompt_template: String,
    client: Client,
}

impl OllamaPlanner {
    pub fn from_file(path: impl AsRef<Path>, config: OllamaPlannerConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .context("failed to build ollama http client")?;

        Ok(Self {
            config,
            prompt_template: load_prompt_template(path)?,
            client,
        })
    }

    pub fn discover_generation_model(host: &str) -> Result<Option<String>> {
        let host = host.trim_end_matches('/');
        let url = format!("{host}/api/tags");
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .context("failed to build ollama discovery client")?;
        let response = client
            .get(url)
            .send()
            .context("failed to query ollama model list")?
            .error_for_status()
            .context("ollama model discovery returned an error status")?;
        let tags: OllamaTagsResponse = response
            .json()
            .context("failed to parse ollama model list response")?;

        Ok(tags
            .models
            .into_iter()
            .find(|model| model.is_generation_capable())
            .map(|model| model.name))
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiPlannerConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub timeout: Duration,
}

impl OpenAiPlannerConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "gpt-5-mini".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenAiPlanner {
    config: OpenAiPlannerConfig,
    prompt_template: String,
    client: Client,
}

impl OpenAiPlanner {
    pub fn from_file(path: impl AsRef<Path>, config: OpenAiPlannerConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .context("failed to build openai http client")?;

        Ok(Self {
            config,
            prompt_template: load_prompt_template(path)?,
            client,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ClaudePlannerConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub api_version: String,
    pub timeout: Duration,
}

impl ClaudePlannerConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "claude-sonnet-4-6".to_string(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            api_version: "2023-06-01".to_string(),
            timeout: Duration::from_secs(60),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClaudePlanner {
    config: ClaudePlannerConfig,
    prompt_template: String,
    client: Client,
}

impl ClaudePlanner {
    pub fn from_file(path: impl AsRef<Path>, config: ClaudePlannerConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .context("failed to build claude http client")?;

        Ok(Self {
            config,
            prompt_template: load_prompt_template(path)?,
            client,
        })
    }
}

impl Planner for FilePromptPlanner {
    fn plan(&self, instruction: String, catalog: &SkillCatalog) -> Result<PlanDecision> {
        let normalized_instruction = instruction.to_lowercase();

        if is_replan_instruction(&normalized_instruction) {
            let recovery_context = replan_context_from_instruction(&normalized_instruction);
            if let Some(skill) = catalog.recovery_skill_for_context(&recovery_context) {
                return Ok(PlanDecision {
                    skill: skill.clone(),
                    reason: Some(format!(
                        "recovery rule matched failed_step={:?} tool={:?}",
                        recovery_context.failed_step, recovery_context.tool
                    )),
                });
            }

            if let Some(skill) = catalog
                .values()
                .find(|skill| skill.resume_original_instruction)
            {
                return Ok(PlanDecision {
                    skill: skill.clone(),
                    reason: Some(
                        "replanning context fell back to first recovery skill in catalog"
                            .to_string(),
                    ),
                });
            }
        }

        if normalized_instruction.contains("pick") && normalized_instruction.contains("place") {
            if let Some(skill) = catalog.get("pick_and_place") {
                return Ok(PlanDecision {
                    skill: skill.clone(),
                    reason: Some("heuristic matched pick and place keywords".to_string()),
                });
            }
        }

        for skill in catalog.values() {
            let skill_name = skill.name.to_lowercase();
            let skill_description = skill.description.to_lowercase();

            if normalized_instruction.contains(&skill_name)
                || normalized_instruction.contains(&skill_description)
            {
                return Ok(PlanDecision {
                    skill: skill.clone(),
                    reason: Some(format!("heuristic matched '{}'", skill.name)),
                });
            }
        }

        let instruction_tokens = tokenize_keywords(&normalized_instruction);
        let best_match = catalog
            .values()
            .filter_map(|skill| {
                let skill_tokens = tokenize_keywords(&format!(
                    "{} {}",
                    skill.name.to_lowercase(),
                    skill.description.to_lowercase()
                ));
                let overlap = instruction_tokens
                    .iter()
                    .filter(|token| skill_tokens.contains(*token))
                    .count();

                if overlap > 0 {
                    Some((overlap, skill))
                } else {
                    None
                }
            })
            .max_by_key(|(overlap, _)| *overlap);

        if let Some((overlap, skill)) = best_match {
            return Ok(PlanDecision {
                skill: skill.clone(),
                reason: Some(format!("heuristic token overlap score={overlap}")),
            });
        }

        catalog
            .first()
            .cloned()
            .map(|skill| PlanDecision {
                skill,
                reason: Some("heuristic fallback to first loaded skill".to_string()),
            })
            .ok_or_else(|| anyhow!("planner could not find any loaded skills"))
    }

    fn provider_name(&self) -> &'static str {
        self.provider.as_str()
    }
}

impl Planner for OllamaPlanner {
    fn plan(&self, instruction: String, catalog: &SkillCatalog) -> Result<PlanDecision> {
        let selection_schema = planner_selection_schema_for_instruction(&instruction, catalog);
        let request = OllamaGenerateRequest {
            model: self.config.model.clone(),
            system: self.prompt_template.clone(),
            prompt: build_planner_user_prompt(&instruction, catalog),
            stream: false,
            format: selection_schema,
            options: json!({
                "temperature": 0
            }),
        };

        let response = self
            .client
            .post(format!(
                "{}/api/generate",
                self.config.host.trim_end_matches('/')
            ))
            .json(&request)
            .send()
            .context("failed to call local ollama planner")?
            .error_for_status()
            .context("local ollama planner returned an error status")?;

        let payload: OllamaGenerateResponse = response
            .json()
            .context("failed to parse local ollama planner response")?;
        let selection = parse_selection_text(&payload.response)?;
        resolve_skill_selection(catalog, selection)
    }

    fn provider_name(&self) -> &'static str {
        LlmProvider::Local.as_str()
    }
}

impl Planner for OpenAiPlanner {
    fn plan(&self, instruction: String, catalog: &SkillCatalog) -> Result<PlanDecision> {
        let selection_schema = planner_selection_schema_for_instruction(&instruction, catalog);
        let request = OpenAiResponsesRequest {
            model: self.config.model.clone(),
            input: vec![
                OpenAiInputMessage {
                    role: "system".to_string(),
                    content: self.prompt_template.clone(),
                },
                OpenAiInputMessage {
                    role: "user".to_string(),
                    content: build_planner_user_prompt(&instruction, catalog),
                },
            ],
            text: OpenAiTextSettings {
                format: OpenAiTextFormat {
                    kind: "json_schema".to_string(),
                    name: "skill_selection".to_string(),
                    description: "Select exactly one skill from the catalog.".to_string(),
                    schema: selection_schema,
                    strict: true,
                },
            },
        };

        let payload: Value = self
            .client
            .post(format!(
                "{}/responses",
                self.config.base_url.trim_end_matches('/')
            ))
            .bearer_auth(&self.config.api_key)
            .json(&request)
            .send()
            .context("failed to call openai planner")?
            .error_for_status()
            .context("openai planner returned an error status")?
            .json()
            .context("failed to parse openai planner response")?;

        let selection = parse_selection_text(&extract_openai_output_text(&payload)?)?;
        resolve_skill_selection(catalog, selection)
    }

    fn provider_name(&self) -> &'static str {
        LlmProvider::OpenAi.as_str()
    }
}

impl Planner for ClaudePlanner {
    fn plan(&self, instruction: String, catalog: &SkillCatalog) -> Result<PlanDecision> {
        let selection_schema = planner_selection_schema_for_instruction(&instruction, catalog);
        let request = ClaudeMessagesRequest {
            model: self.config.model.clone(),
            max_tokens: 256,
            system: self.prompt_template.clone(),
            tools: vec![ClaudeToolDefinition {
                name: "select_skill".to_string(),
                description: "Select exactly one skill from the provided skill catalog."
                    .to_string(),
                input_schema: selection_schema,
                strict: true,
            }],
            tool_choice: ClaudeToolChoice {
                kind: "tool".to_string(),
                name: "select_skill".to_string(),
            },
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: build_planner_user_prompt(&instruction, catalog),
            }],
        };

        let payload: ClaudeMessagesResponse = self
            .client
            .post(format!(
                "{}/messages",
                self.config.base_url.trim_end_matches('/')
            ))
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", &self.config.api_version)
            .json(&request)
            .send()
            .context("failed to call claude planner")?
            .error_for_status()
            .context("claude planner returned an error status")?
            .json()
            .context("failed to parse claude planner response")?;

        let selection = extract_claude_tool_selection(&payload)?;
        resolve_skill_selection(catalog, selection)
    }

    fn provider_name(&self) -> &'static str {
        LlmProvider::Claude.as_str()
    }
}

#[derive(Clone)]
pub struct Executor {
    registry: ToolRegistry,
}

impl Executor {
    pub fn new(registry: ToolRegistry) -> Self {
        Self { registry }
    }

    pub fn execute_step(&self, step: &SkillStep) -> Result<Value> {
        self.registry.execute(&step.tool, step.input.clone())
    }

    pub fn available_tools(&self) -> Vec<String> {
        self.registry.tool_names()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepExecution {
    pub step_name: String,
    pub tool_name: String,
    pub output: Value,
    pub attempts: usize,
    pub status: StepStatus,
    pub observation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Succeeded,
    Failed,
}

impl StepStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            StepStatus::Succeeded => "succeeded",
            StepStatus::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentReport {
    pub instruction: String,
    pub planner_provider: String,
    pub planner_reason: Option<String>,
    pub skill: Skill,
    pub steps: Vec<StepExecution>,
    pub resumed_from_step: Option<String>,
    pub completed: bool,
    pub failed_step: Option<String>,
    pub next_action: String,
}

pub struct Agent {
    pub memory: Memory,
    pub planner: Box<dyn Planner>,
    pub executor: Executor,
}

#[derive(Debug, Serialize)]
struct OllamaGenerateRequest {
    model: String,
    system: String,
    prompt: String,
    stream: bool,
    format: Value,
    options: Value,
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    response: String,
}

#[derive(Debug, Serialize)]
struct OpenAiResponsesRequest {
    model: String,
    input: Vec<OpenAiInputMessage>,
    text: OpenAiTextSettings,
}

#[derive(Debug, Serialize)]
struct OpenAiInputMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct OpenAiTextSettings {
    format: OpenAiTextFormat,
}

#[derive(Debug, Serialize)]
struct OpenAiTextFormat {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    description: String,
    schema: Value,
    strict: bool,
}

#[derive(Debug, Serialize)]
struct ClaudeMessagesRequest {
    model: String,
    max_tokens: u32,
    system: String,
    tools: Vec<ClaudeToolDefinition>,
    tool_choice: ClaudeToolChoice,
    messages: Vec<ClaudeMessage>,
}

#[derive(Debug, Serialize)]
struct ClaudeToolDefinition {
    name: String,
    description: String,
    input_schema: Value,
    strict: bool,
}

#[derive(Debug, Serialize)]
struct ClaudeToolChoice {
    #[serde(rename = "type")]
    kind: String,
    name: String,
}

#[derive(Debug, Serialize)]
struct ClaudeMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeMessagesResponse {
    content: Vec<ClaudeContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ClaudeContentBlock {
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
    input: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct PlannerSelection {
    skill: String,
    #[allow(dead_code)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaTagModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagModel {
    name: String,
    details: OllamaTagDetails,
}

#[derive(Debug, Deserialize)]
struct OllamaTagDetails {
    family: Option<String>,
    families: Option<Vec<String>>,
}

impl Agent {
    pub fn new(memory: Memory, planner: Box<dyn Planner>, executor: Executor) -> Self {
        Self {
            memory,
            planner,
            executor,
        }
    }

    pub fn plan_only(
        &self,
        instruction: impl Into<String>,
        catalog: &SkillCatalog,
    ) -> Result<PlanDecision> {
        self.planner.plan(instruction.into(), catalog)
    }

    pub fn run_loop(
        &mut self,
        instruction: impl Into<String>,
        catalog: &SkillCatalog,
    ) -> Result<AgentReport> {
        let instruction = instruction.into();
        let decision = self
            .planner
            .plan(instruction.clone(), catalog)
            .context("planner failed")?;

        self.run_with_decision(instruction, decision)
    }

    pub fn run_with_decision(
        &mut self,
        instruction: impl Into<String>,
        decision: PlanDecision,
    ) -> Result<AgentReport> {
        self.run_with_decision_internal(instruction.into(), decision, None)
    }

    pub fn run_with_decision_from_step(
        &mut self,
        instruction: impl Into<String>,
        decision: PlanDecision,
        step_name: impl Into<String>,
    ) -> Result<AgentReport> {
        self.run_with_decision_internal(instruction.into(), decision, Some(step_name.into()))
    }

    fn run_with_decision_internal(
        &mut self,
        instruction: String,
        decision: PlanDecision,
        resumed_from_step: Option<String>,
    ) -> Result<AgentReport> {
        self.memory.remember_event(
            "instruction_received",
            json!({ "instruction": instruction.clone() }),
        )?;

        let skill = decision.skill;
        let planner_reason = decision.reason.clone();
        let start_step_index = resumed_from_step
            .as_deref()
            .map(|step_name| step_index_for_skill(&skill, step_name))
            .transpose()?
            .unwrap_or(0);

        self.memory.remember_event(
            "skill_selected",
            json!({
                "skill": skill.name.clone(),
                "resumed_from_step": resumed_from_step.clone(),
            }),
        )?;

        let mut steps = Vec::new();
        let mut completed = true;
        let mut failed_step = None;
        for step in skill.steps.iter().skip(start_step_index) {
            let mut final_output = Value::Null;
            let mut final_observation = "step not executed".to_string();
            let mut final_status = StepStatus::Failed;
            let mut attempts = 0usize;

            for attempt in 0..=step.max_retries {
                attempts = attempt + 1;
                self.memory.remember_event(
                    "tool_invoked",
                    json!({
                        "step": step.name.clone(),
                        "tool": step.tool.clone(),
                        "input": step.input.clone(),
                        "attempt": attempts,
                    }),
                )?;

                let output = self
                    .executor
                    .execute_step(step)
                    .with_context(|| format!("step '{}' failed", step.name))?;

                let evaluation = evaluate_step_output(step, &output)
                    .with_context(|| format!("failed to validate step '{}'", step.name))?;

                self.memory.remember_event(
                    "step_observed",
                    json!({
                        "step": step.name.clone(),
                        "tool": step.tool.clone(),
                        "attempt": attempts,
                        "status": evaluation.status.as_str(),
                        "observation": evaluation.detail,
                        "output": output.clone(),
                    }),
                )?;

                self.memory.remember_event(
                    "tool_completed",
                    json!({
                        "step": step.name.clone(),
                        "tool": step.tool.clone(),
                        "attempt": attempts,
                        "status": evaluation.status.as_str(),
                        "output": output.clone(),
                    }),
                )?;

                final_output = output;
                final_observation = evaluation.detail;
                final_status = evaluation.status;

                if final_status == StepStatus::Succeeded {
                    break;
                }

                if attempt < step.max_retries {
                    self.memory.remember_event(
                        "step_retry_scheduled",
                        json!({
                            "step": step.name.clone(),
                            "tool": step.tool.clone(),
                            "attempt": attempts,
                            "next_attempt": attempts + 1,
                            "reason": final_observation.clone(),
                        }),
                    )?;
                }
            }

            let step_execution = StepExecution {
                step_name: step.name.clone(),
                tool_name: step.tool.clone(),
                output: final_output,
                attempts,
                status: final_status,
                observation: final_observation,
            };

            let step_failed = step_execution.status == StepStatus::Failed;
            steps.push(step_execution);

            if step_failed {
                completed = false;
                failed_step = Some(step.name.clone());
                break;
            }
        }

        let next_action = if let Some(step_name) = failed_step.as_deref() {
            format!("replan_after_{step_name}")
        } else {
            steps
                .last()
                .map(|step| format!("monitor_after_{}", step.step_name))
                .unwrap_or_else(|| "idle".to_string())
        };

        self.memory.remember_log(
            format!("Executed {}", skill.name),
            format!(
                "instruction: {}\nprovider: {}\nreason: {}\nresumed_from_step: {}\ntools: {}\nsteps_completed: {}\ncompleted: {}\nfailed_step: {}\nnext_action: {}",
                instruction,
                self.planner.provider_name(),
                planner_reason.as_deref().unwrap_or("none"),
                resumed_from_step.as_deref().unwrap_or("none"),
                self.executor.available_tools().join(", "),
                steps.len(),
                completed,
                failed_step.as_deref().unwrap_or("none"),
                next_action
            ),
        )?;

        Ok(AgentReport {
            instruction,
            planner_provider: self.planner.provider_name().to_string(),
            planner_reason,
            skill,
            steps,
            resumed_from_step,
            completed,
            failed_step,
            next_action,
        })
    }
}

impl OllamaTagModel {
    fn is_generation_capable(&self) -> bool {
        let mut labels = Vec::new();
        labels.push(self.name.to_lowercase());
        if let Some(family) = &self.details.family {
            labels.push(family.to_lowercase());
        }
        if let Some(families) = &self.details.families {
            for family in families {
                labels.push(family.to_lowercase());
            }
        }

        !labels.iter().any(|label| {
            label.contains("embed")
                || label.contains("embedding")
                || label.contains("nomic-bert")
                || label.contains("bge")
        })
    }
}

pub fn planner_from_env(path: impl AsRef<Path>) -> Result<Box<dyn Planner>> {
    let path = path.as_ref();
    match requested_provider_from_env()? {
        RequestedPlanner::Provider(provider) => planner_for_provider(path, provider),
        RequestedPlanner::Auto => {
            if let Ok(planner) = build_local_planner(path) {
                return Ok(planner);
            }
            if let Ok(planner) = build_openai_planner(path) {
                return Ok(planner);
            }
            if let Ok(planner) = build_claude_planner(path) {
                return Ok(planner);
            }
            Ok(Box::new(FilePromptPlanner::from_file(
                LlmProvider::Mock,
                path,
            )?))
        }
    }
}

pub fn planner_for_provider(
    path: impl AsRef<Path>,
    provider: LlmProvider,
) -> Result<Box<dyn Planner>> {
    let path = path.as_ref();
    match provider {
        LlmProvider::Mock => Ok(Box::new(FilePromptPlanner::from_file(
            LlmProvider::Mock,
            path,
        )?)),
        LlmProvider::Local => build_local_planner(path),
        LlmProvider::OpenAi => build_openai_planner(path),
        LlmProvider::Claude => build_claude_planner(path),
    }
}

pub fn planner_turn_debug(
    instruction: impl AsRef<str>,
    catalog: &SkillCatalog,
) -> PlannerTurnDebug {
    let instruction = instruction.as_ref();
    let normalized_instruction = instruction.to_lowercase();
    let is_replan = is_replan_instruction(&normalized_instruction);
    let context = replan_context_from_instruction(&normalized_instruction);
    let matching_recovery_skills = if is_replan {
        catalog.recovery_candidate_names(&context)
    } else {
        Vec::new()
    };

    PlannerTurnDebug {
        is_replan,
        allowed_skills: allowed_skill_names_for_instruction(instruction, catalog),
        matching_recovery_skills,
        prompt: build_planner_user_prompt(instruction, catalog),
        schema: planner_selection_schema_for_instruction(instruction, catalog),
    }
}

fn build_local_planner(path: &Path) -> Result<Box<dyn Planner>> {
    let host =
        env::var("ROBOCLAW_OLLAMA_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
    let model = env::var("ROBOCLAW_OLLAMA_MODEL")
        .ok()
        .or_else(|| env::var("OLLAMA_MODEL").ok())
        .or_else(|| {
            OllamaPlanner::discover_generation_model(&host)
                .ok()
                .flatten()
        })
        .ok_or_else(|| anyhow!("no local generative ollama model found"))?;

    let mut config = OllamaPlannerConfig::new(model);
    config.host = host;
    Ok(Box::new(OllamaPlanner::from_file(path, config)?))
}

fn build_openai_planner(path: &Path) -> Result<Box<dyn Planner>> {
    let api_key = env::var("ROBOCLAW_OPENAI_API_KEY")
        .ok()
        .or_else(|| env::var("OPENAI_API_KEY").ok())
        .ok_or_else(|| anyhow!("OPENAI_API_KEY is not set"))?;

    let mut config = OpenAiPlannerConfig::new(api_key);
    if let Ok(model) = env::var("ROBOCLAW_OPENAI_MODEL") {
        config.model = model;
    }
    if let Ok(base_url) = env::var("ROBOCLAW_OPENAI_BASE_URL") {
        config.base_url = base_url;
    }

    Ok(Box::new(OpenAiPlanner::from_file(path, config)?))
}

fn build_claude_planner(path: &Path) -> Result<Box<dyn Planner>> {
    let api_key = env::var("ROBOCLAW_CLAUDE_API_KEY")
        .ok()
        .or_else(|| env::var("ANTHROPIC_API_KEY").ok())
        .ok_or_else(|| anyhow!("ANTHROPIC_API_KEY is not set"))?;

    let mut config = ClaudePlannerConfig::new(api_key);
    if let Ok(model) = env::var("ROBOCLAW_CLAUDE_MODEL") {
        config.model = model;
    }
    if let Ok(base_url) = env::var("ROBOCLAW_CLAUDE_BASE_URL") {
        config.base_url = base_url;
    }
    if let Ok(api_version) = env::var("ROBOCLAW_CLAUDE_API_VERSION") {
        config.api_version = api_version;
    }

    Ok(Box::new(ClaudePlanner::from_file(path, config)?))
}

fn load_prompt_template(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    fs::read_to_string(path).with_context(|| format!("failed to read {:?}", path))
}

fn build_planner_user_prompt(instruction: &str, catalog: &SkillCatalog) -> String {
    let skills = build_skill_catalog_summary(catalog);
    let allowed_skills = allowed_skill_names_for_instruction(instruction, catalog);
    let allowed_skills_guidance = if allowed_skills.len() < catalog.names().len() {
        format!(
            "\nAllowed skills for this turn: {}",
            allowed_skills.join(", ")
        )
    } else {
        String::new()
    };
    let recovery_guidance = build_recovery_guidance(instruction, catalog)
        .map(|guidance| format!("\n\nRecovery guidance:\n{guidance}"))
        .unwrap_or_default();

    format!(
        "Available skills:\n{}{}\n\nInstruction:\n{}{}\n\nChoose exactly one skill from the catalog.",
        skills, allowed_skills_guidance, instruction, recovery_guidance
    )
}

fn planner_selection_schema_for_instruction(instruction: &str, catalog: &SkillCatalog) -> Value {
    let allowed_skills = allowed_skill_names_for_instruction(instruction, catalog);
    let skill_description = format!(
        "Select exactly one loaded skill name. Allowed skills for this turn: {}. Catalog summary:\n{}",
        allowed_skills.join(", "),
        build_skill_catalog_summary(catalog)
    );
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "skill": {
                "type": "string",
                "enum": allowed_skills,
                "description": skill_description,
            },
            "reason": {
                "type": "string",
                "description": "Short rationale referencing the instruction, skill fit, or recovery match."
            }
        },
        "required": ["skill"]
    })
}

fn build_skill_catalog_summary(catalog: &SkillCatalog) -> String {
    catalog
        .values()
        .map(|skill| {
            let recovery = skill
                .recovery_summary()
                .map(|summary| format!(" | recovery_for={summary}"))
                .unwrap_or_default();
            format!(
                "- {}: {} | steps={}{}",
                skill.name,
                skill.description,
                skill
                    .steps
                    .iter()
                    .map(|step| step.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                recovery,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_recovery_guidance(instruction: &str, catalog: &SkillCatalog) -> Option<String> {
    let normalized_instruction = instruction.to_lowercase();
    if !is_replan_instruction(&normalized_instruction) {
        return None;
    }

    let context = replan_context_from_instruction(&normalized_instruction);
    let matching_recovery_skills = catalog.recovery_candidate_names(&context);
    let suggested = if matching_recovery_skills.is_empty() {
        "none".to_string()
    } else {
        matching_recovery_skills.join(", ")
    };

    Some(format!(
        "- failed_step: {}\n- failed_tool: {}\n- observation: {}\n- matching_recovery_skills: {}\nPrefer one of matching_recovery_skills when it fits the failure.",
        context.failed_step.as_deref().unwrap_or("unknown"),
        context.tool.as_deref().unwrap_or("unknown"),
        context.observation.as_deref().unwrap_or("unknown"),
        suggested,
    ))
}

fn allowed_skill_names_for_instruction(instruction: &str, catalog: &SkillCatalog) -> Vec<String> {
    let normalized_instruction = instruction.to_lowercase();
    if !is_replan_instruction(&normalized_instruction) {
        return catalog.names();
    }

    let context = replan_context_from_instruction(&normalized_instruction);
    let matching_recovery_skills = catalog.recovery_candidate_names(&context);
    if matching_recovery_skills.is_empty() {
        catalog.names()
    } else {
        matching_recovery_skills
    }
}

fn parse_selection_text(raw: &str) -> Result<PlannerSelection> {
    let trimmed = raw.trim();
    let without_fence = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    serde_json::from_str(without_fence).with_context(|| {
        format!(
            "failed to parse planner selection JSON from llm output: {}",
            without_fence
        )
    })
}

fn parse_selection_value(value: Value) -> Result<PlannerSelection> {
    serde_json::from_value(value).context("failed to parse planner selection value")
}

fn resolve_skill_selection(
    catalog: &SkillCatalog,
    selection: PlannerSelection,
) -> Result<PlanDecision> {
    let PlannerSelection { skill, reason } = selection;
    resolve_skill(catalog, &skill)
        .map(|resolved_skill| PlanDecision {
            skill: resolved_skill,
            reason,
        })
        .ok_or_else(|| {
            anyhow!(
                "planner returned unknown skill '{}' , available skills: {}",
                skill,
                catalog.names().join(", ")
            )
        })
}

fn extract_openai_output_text(payload: &Value) -> Result<String> {
    if let Some(text) = payload.get("output_text").and_then(Value::as_str) {
        return Ok(text.to_string());
    }

    if let Some(items) = payload.get("output").and_then(Value::as_array) {
        for item in items {
            if let Some(contents) = item.get("content").and_then(Value::as_array) {
                for content in contents {
                    if let Some(text) = content.get("text").and_then(Value::as_str) {
                        return Ok(text.to_string());
                    }
                }
            }
        }
    }

    Err(anyhow!(
        "openai response did not include output_text or output.content.text"
    ))
}

fn extract_claude_tool_selection(payload: &ClaudeMessagesResponse) -> Result<PlannerSelection> {
    let input = payload
        .content
        .iter()
        .find(|block| block.kind == "tool_use" && block.name.as_deref() == Some("select_skill"))
        .and_then(|block| block.input.clone())
        .ok_or_else(|| anyhow!("claude response did not contain select_skill tool_use"))?;

    parse_selection_value(input)
}

fn resolve_skill(catalog: &SkillCatalog, selected: &str) -> Option<Skill> {
    if let Some(skill) = catalog.get(selected) {
        return Some(skill.clone());
    }

    let normalized = normalize_skill_name(selected);
    for skill in catalog.values() {
        if normalize_skill_name(&skill.name) == normalized {
            return Some(skill.clone());
        }
    }

    None
}

fn normalize_skill_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

fn tokenize_keywords(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| token.len() > 2)
        .filter(|token| {
            !matches!(
                *token,
                "the"
                    | "and"
                    | "for"
                    | "with"
                    | "from"
                    | "into"
                    | "that"
                    | "this"
                    | "use"
                    | "using"
                    | "robot"
            )
        })
        .map(|token| token.to_string())
        .collect()
}

fn is_replan_instruction(text: &str) -> bool {
    text.contains("previous execution failed")
        || text.contains("replan_after_")
        || text.contains("replan_started")
        || text.contains("failed step:")
}

fn replan_context_from_instruction(text: &str) -> RecoveryContext {
    RecoveryContext {
        failed_step: extract_instruction_field(text, "failed step:"),
        tool: extract_instruction_field(text, "failed tool:"),
        observation: extract_instruction_field(text, "observation:"),
    }
}

fn extract_instruction_field(text: &str, label: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix(label)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn requested_provider_from_env() -> Result<RequestedPlanner> {
    let requested = env::var("ROBOCLAW_LLM_PROVIDER").unwrap_or_else(|_| "auto".to_string());
    match requested.to_lowercase().as_str() {
        "auto" => Ok(RequestedPlanner::Auto),
        "mock" => Ok(RequestedPlanner::Provider(LlmProvider::Mock)),
        "local" | "ollama" => Ok(RequestedPlanner::Provider(LlmProvider::Local)),
        "openai" => Ok(RequestedPlanner::Provider(LlmProvider::OpenAi)),
        "claude" | "anthropic" => Ok(RequestedPlanner::Provider(LlmProvider::Claude)),
        other => Err(anyhow!(
            "unsupported ROBOCLAW_LLM_PROVIDER '{}' expected one of: auto, mock, local, openai, claude",
            other
        )),
    }
}

enum RequestedPlanner {
    Auto,
    Provider(LlmProvider),
}

#[derive(Debug, Clone)]
struct StepEvaluation {
    status: StepStatus,
    detail: String,
}

fn evaluate_step_output(step: &SkillStep, output: &Value) -> Result<StepEvaluation> {
    if step.expect.is_null() {
        return Ok(StepEvaluation {
            status: StepStatus::Succeeded,
            detail: "no expectation defined".to_string(),
        });
    }

    let resolved_expectation = resolve_expectation_templates(&step.expect, &step.input)
        .with_context(|| format!("invalid expectation template for step '{}'", step.name))?;

    if value_contains(output, &resolved_expectation) {
        Ok(StepEvaluation {
            status: StepStatus::Succeeded,
            detail: format!("matched expectation {}", resolved_expectation),
        })
    } else {
        Ok(StepEvaluation {
            status: StepStatus::Failed,
            detail: format!(
                "expected subset {} was not present in output {}",
                resolved_expectation, output
            ),
        })
    }
}

fn resolve_expectation_templates(expectation: &Value, input: &Value) -> Result<Value> {
    match expectation {
        Value::String(template) => {
            if let Some(path) = template.strip_prefix("$input.") {
                lookup_value_path(input, path)
                    .cloned()
                    .ok_or_else(|| anyhow!("missing input path '{}'", path))
            } else {
                Ok(Value::String(template.clone()))
            }
        }
        Value::Array(values) => values
            .iter()
            .map(|value| resolve_expectation_templates(value, input))
            .collect::<Result<Vec<_>>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut resolved = serde_json::Map::new();
            for (key, value) in map {
                resolved.insert(key.clone(), resolve_expectation_templates(value, input)?);
            }
            Ok(Value::Object(resolved))
        }
        other => Ok(other.clone()),
    }
}

fn lookup_value_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

fn value_contains(actual: &Value, expected: &Value) -> bool {
    match expected {
        Value::Object(expected_map) => {
            let Some(actual_map) = actual.as_object() else {
                return false;
            };

            expected_map.iter().all(|(key, expected_value)| {
                actual_map
                    .get(key)
                    .map(|actual_value| value_contains(actual_value, expected_value))
                    .unwrap_or(false)
            })
        }
        Value::Array(expected_values) => {
            let Some(actual_values) = actual.as_array() else {
                return false;
            };

            actual_values.len() == expected_values.len()
                && actual_values.iter().zip(expected_values.iter()).all(
                    |(actual_value, expected_value)| value_contains(actual_value, expected_value),
                )
        }
        _ => actual == expected,
    }
}

fn step_index_for_skill(skill: &Skill, step_name: &str) -> Result<usize> {
    skill
        .steps
        .iter()
        .position(|step| step.name == step_name)
        .ok_or_else(|| {
            anyhow!(
                "skill '{}' does not contain step '{}'",
                skill.name,
                step_name
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use roboclaw_tools::{Tool, ToolRegistry};
    use std::env;
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn schema_contains_catalog_skill_enum() {
        let catalog = test_catalog();
        let schema = planner_selection_schema_for_instruction("", &catalog);
        let skill_enum = schema["properties"]["skill"]["enum"]
            .as_array()
            .expect("enum should be present")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();

        assert_eq!(
            skill_enum,
            vec![
                "pick_and_place",
                "recover_grasp",
                "recover_observation",
                "wave_arm"
            ]
        );
    }

    #[test]
    fn schema_describes_recovery_metadata() {
        let catalog = test_catalog();
        let schema = planner_selection_schema_for_instruction("", &catalog);
        let description = schema["properties"]["skill"]["description"]
            .as_str()
            .expect("skill description should be present");

        assert!(description.contains("recover_grasp"));
        assert!(description.contains("recovery_for=failed_steps=grasp; tools=motor_control"));
        assert!(description.contains("recover_observation"));
    }

    #[test]
    fn schema_narrows_to_matching_recovery_skills_for_replan_instruction() {
        let catalog = test_catalog();
        let schema = planner_selection_schema_for_instruction(
            "Original instruction:\nPick and place.\n\nPrevious execution failed.\nFailed step: grasp\nFailed tool: motor_control\nObservation: transient motor stall",
            &catalog,
        );
        let skill_enum = schema["properties"]["skill"]["enum"]
            .as_array()
            .expect("enum should be present")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();

        assert_eq!(skill_enum, vec!["recover_grasp"]);
    }

    #[test]
    fn parses_json_selection_from_text() {
        let selection = parse_selection_text("```json\n{\"skill\":\"pick_and_place\"}\n```")
            .expect("selection should parse");
        assert_eq!(selection.skill, "pick_and_place");
    }

    #[test]
    fn mock_planner_uses_token_overlap_for_wave_instruction() {
        let catalog = test_catalog();
        let planner = FilePromptPlanner {
            provider: LlmProvider::Mock,
            prompt_template: "planner prompt".to_string(),
        };

        let decision = planner
            .plan("Wave to acknowledge the operator.".to_string(), &catalog)
            .expect("mock planner should select a skill");

        assert_eq!(decision.skill.name, "wave_arm");
    }

    #[test]
    fn mock_planner_prefers_recovery_skill_for_replan_instruction() {
        let catalog = test_catalog();
        let planner = FilePromptPlanner {
            provider: LlmProvider::Mock,
            prompt_template: "planner prompt".to_string(),
        };

        let decision = planner
            .plan(
                "Original instruction:\nPick and place.\n\nPrevious execution failed.\nFailed step: detect_object\nFailed tool: sensor".to_string(),
                &catalog,
            )
            .expect("mock planner should select a recovery skill");

        assert_eq!(decision.skill.name, "recover_observation");
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("recovery rule matched"));
    }

    #[test]
    fn mock_planner_prefers_grasp_recovery_skill_for_grasp_failure() {
        let catalog = test_catalog();
        let planner = FilePromptPlanner {
            provider: LlmProvider::Mock,
            prompt_template: "planner prompt".to_string(),
        };

        let decision = planner
            .plan(
                "Original instruction:\nPick and place.\n\nPrevious execution failed.\nFailed step: grasp\nFailed tool: motor_control\nObservation: transient motor stall".to_string(),
                &catalog,
            )
            .expect("mock planner should select a grasp recovery skill");

        assert_eq!(decision.skill.name, "recover_grasp");
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("tool=Some(\"motor_control\")"));
    }

    #[test]
    fn planner_prompt_includes_recovery_metadata() {
        let catalog = test_catalog();
        let prompt = build_planner_user_prompt("Recover from a failed grasp.", &catalog);

        assert!(prompt.contains("recovery_for=failed_steps=grasp; tools=motor_control"));
        assert!(prompt.contains("recovery_for=failed_steps=detect_object; tools=sensor"));
    }

    #[test]
    fn planner_prompt_includes_recovery_guidance_for_replan_instruction() {
        let catalog = test_catalog();
        let prompt = build_planner_user_prompt(
            "Original instruction:\nPick and place.\n\nPrevious execution failed.\nFailed step: grasp\nFailed tool: motor_control\nObservation: transient motor stall",
            &catalog,
        );

        assert!(prompt.contains("Recovery guidance:"));
        assert!(prompt.contains("Allowed skills for this turn: recover_grasp"));
        assert!(prompt.contains("matching_recovery_skills: recover_grasp"));
        assert!(prompt.contains("Prefer one of matching_recovery_skills"));
    }

    #[test]
    fn planner_turn_debug_narrows_allowed_skills_for_recovery_turn() {
        let catalog = test_catalog();
        let debug = planner_turn_debug(
            "Original instruction:\nPick and place.\n\nPrevious execution failed.\nFailed step: grasp\nFailed tool: motor_control\nObservation: transient motor stall",
            &catalog,
        );

        assert!(debug.is_replan);
        assert_eq!(debug.allowed_skills, vec!["recover_grasp"]);
        assert_eq!(debug.matching_recovery_skills, vec!["recover_grasp"]);
        assert!(debug
            .prompt
            .contains("Allowed skills for this turn: recover_grasp"));
        let skill_enum = debug.schema["properties"]["skill"]["enum"]
            .as_array()
            .expect("enum should be present")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(skill_enum, vec!["recover_grasp"]);
    }

    #[test]
    fn resolves_input_templates_inside_expectation() {
        let resolved = resolve_expectation_templates(
            &json!({
                "state": {
                    "last_pose": "$input.pose",
                    "held_object": "$input.target",
                }
            }),
            &json!({
                "pose": "bin_a",
                "target": "red_cube",
            }),
        )
        .expect("expectation template should resolve");

        assert_eq!(
            resolved,
            json!({
                "state": {
                    "last_pose": "bin_a",
                    "held_object": "red_cube",
                }
            })
        );
    }

    #[test]
    fn retries_failed_step_and_recovers() {
        let mut agent = test_agent_with_sequence_tool(vec![
            json!({"detected": false}),
            json!({"detected": true}),
        ]);

        let report = agent
            .run_with_decision(
                "Detect the target".to_string(),
                PlanDecision {
                    skill: Skill {
                        name: "detect_then_retry".to_string(),
                        description: "detect with retry".to_string(),
                        resume_original_instruction: false,
                        supports_checkpoint_resume: false,
                        recovery_for: vec![],
                        steps: vec![SkillStep {
                            name: "detect_object".to_string(),
                            tool: "sequence_sensor".to_string(),
                            input: json!({"target": "red_cube"}),
                            expect: json!({"detected": true}),
                            max_retries: 1,
                            resume_from_step: None,
                        }],
                    },
                    reason: Some("test".to_string()),
                },
            )
            .expect("agent should recover after retry");

        assert!(report.completed);
        assert_eq!(report.failed_step, None);
        assert_eq!(report.next_action, "monitor_after_detect_object");
        assert_eq!(report.steps.len(), 1);
        assert_eq!(report.steps[0].attempts, 2);
        assert_eq!(report.steps[0].status, StepStatus::Succeeded);
    }

    #[test]
    fn requests_replan_after_exhausting_retries() {
        let mut agent = test_agent_with_sequence_tool(vec![
            json!({"detected": false}),
            json!({"detected": false}),
        ]);

        let report = agent
            .run_with_decision(
                "Detect the target".to_string(),
                PlanDecision {
                    skill: Skill {
                        name: "detect_then_fail".to_string(),
                        description: "detect and fail".to_string(),
                        resume_original_instruction: false,
                        supports_checkpoint_resume: false,
                        recovery_for: vec![],
                        steps: vec![SkillStep {
                            name: "detect_object".to_string(),
                            tool: "sequence_sensor".to_string(),
                            input: json!({"target": "red_cube"}),
                            expect: json!({"detected": true}),
                            max_retries: 1,
                            resume_from_step: None,
                        }],
                    },
                    reason: Some("test".to_string()),
                },
            )
            .expect("agent should return a report even when observation fails");

        assert!(!report.completed);
        assert_eq!(report.failed_step.as_deref(), Some("detect_object"));
        assert_eq!(report.next_action, "replan_after_detect_object");
        assert_eq!(report.steps.len(), 1);
        assert_eq!(report.steps[0].attempts, 2);
        assert_eq!(report.steps[0].status, StepStatus::Failed);
    }

    #[test]
    fn can_resume_execution_from_named_step() {
        let mut agent = test_agent_with_sequence_tool(vec![
            json!({"detected": true}),
            json!({"detected": true}),
        ]);

        let report = agent
            .run_with_decision_from_step(
                "Resume the skill".to_string(),
                PlanDecision {
                    skill: Skill {
                        name: "checkpoint_skill".to_string(),
                        description: "resume from a later step".to_string(),
                        resume_original_instruction: false,
                        supports_checkpoint_resume: true,
                        recovery_for: vec![],
                        steps: vec![
                            SkillStep {
                                name: "detect_object".to_string(),
                                tool: "sequence_sensor".to_string(),
                                input: json!({"target": "red_cube"}),
                                expect: json!({"detected": true}),
                                max_retries: 0,
                                resume_from_step: None,
                            },
                            SkillStep {
                                name: "confirm_detection".to_string(),
                                tool: "sequence_sensor".to_string(),
                                input: json!({"target": "red_cube"}),
                                expect: json!({"detected": true}),
                                max_retries: 0,
                                resume_from_step: None,
                            },
                        ],
                    },
                    reason: Some("test".to_string()),
                },
                "confirm_detection",
            )
            .expect("agent should resume from the requested step");

        assert!(report.completed);
        assert_eq!(
            report.resumed_from_step.as_deref(),
            Some("confirm_detection")
        );
        assert_eq!(report.steps.len(), 1);
        assert_eq!(report.steps[0].step_name, "confirm_detection");
    }

    fn test_agent_with_sequence_tool(outputs: Vec<Value>) -> Agent {
        let memory_dir = env::temp_dir().join(unique_test_name("roboclaw-agent-memory"));
        let memory = Memory::new(&memory_dir).expect("test memory should be creatable");
        let planner: Box<dyn Planner> = Box::new(FilePromptPlanner {
            provider: LlmProvider::Mock,
            prompt_template: "planner prompt".to_string(),
        });

        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(SequenceTool::new("sequence_sensor", outputs)));

        Agent::new(memory, planner, Executor::new(registry))
    }

    fn test_catalog() -> SkillCatalog {
        let dir = env::temp_dir().join(unique_test_name("roboclaw-agent-test"));
        fs::create_dir_all(&dir).expect("test dir should be creatable");

        fs::write(
            dir.join("pick_and_place.yaml"),
            "name: pick_and_place\ndescription: pick\nsteps:\n  - name: detect\n    tool: sensor\n",
        )
        .expect("skill file should be writable");
        fs::write(
            dir.join("wave.yaml"),
            "name: wave_arm\ndescription: wave the robot arm to acknowledge or greet\nsteps: []\n",
        )
        .expect("skill file should be writable");
        fs::write(
            dir.join("recover_observation.yaml"),
            "name: recover_observation\ndescription: recover from failed observation\nresume_original_instruction: true\nrecovery_for:\n  - failed_steps:\n      - detect_object\n    tools:\n      - sensor\nsteps: []\n",
        )
        .expect("skill file should be writable");
        fs::write(
            dir.join("recover_grasp.yaml"),
            "name: recover_grasp\ndescription: recover from failed grasp\nresume_original_instruction: true\nrecovery_for:\n  - failed_steps:\n      - grasp\n    tools:\n      - motor_control\nsteps: []\n",
        )
        .expect("skill file should be writable");

        SkillCatalog::from_dir(&dir).expect("catalog should load from temp dir")
    }

    fn unique_test_name(prefix: &str) -> String {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        format!("{prefix}-{stamp}")
    }

    struct SequenceTool {
        name: String,
        outputs: Vec<Value>,
        cursor: Mutex<usize>,
    }

    impl SequenceTool {
        fn new(name: &str, outputs: Vec<Value>) -> Self {
            Self {
                name: name.to_string(),
                outputs,
                cursor: Mutex::new(0),
            }
        }
    }

    impl Tool for SequenceTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn execute(&self, _input: Value) -> Result<Value> {
            let mut cursor = self.cursor.lock().expect("sequence tool mutex poisoned");
            let index = (*cursor).min(self.outputs.len().saturating_sub(1));
            *cursor += 1;
            Ok(self.outputs[index].clone())
        }
    }
}
