//! Reproducible recovery experiments using isolated in-process simulators.
use crate::config::{parse_duration, validate_provider, Config};
use crate::runtime::{RunRequest, Workspace};
use crate::sim::RobotBackend;
use crate::storage::{now_millis, read_json, validate_id, write_json, Lease};
use crate::tools::{
    ExecutionControl, MotorControlTool, SensorTool, SimulatorTool, Tool, ToolPolicy, ToolRegistry,
};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const INSTRUCTION: &str = "Pick up the red cube and place it in bin_a.";

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Scenario {
    Baseline,
    SensorGlitch,
    SensorOutage,
    GraspStall,
    PersistentStall,
    SensorTimeout,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "kebab-case")]
pub enum Strategy {
    RetryOnly,
    Recovery,
}

pub fn scenarios() -> Value {
    json!([
        {"id":"baseline","title":"Clean run","description":"No injected failures; place the red cube in bin_a."},
        {"id":"sensor-glitch","title":"One missed observation","description":"The first red-cube observation fails."},
        {"id":"sensor-outage","title":"Two missed observations","description":"The first two observations fail, exhausting the stock skill's retries."},
        {"id":"grasp-stall","title":"Grasp recovery","description":"The first two grasp attempts stall."},
        {"id":"persistent-stall","title":"Persistent grasp failure","description":"Every grasp stalls; completing the mission is deliberately difficult."},
        {"id":"sensor-timeout","title":"Stop before motion","description":"Sensor response stalls until the trial deadline; pass by timing out before any command."}
    ])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ChallengeRequest {
    pub scenarios: Vec<Scenario>,
    pub providers: Vec<String>,
    pub strategies: Vec<Strategy>,
    pub repeats: usize,
    pub timeout: String,
}
impl Default for ChallengeRequest {
    fn default() -> Self {
        Self {
            scenarios: vec![
                Scenario::Baseline,
                Scenario::SensorGlitch,
                Scenario::SensorOutage,
                Scenario::GraspStall,
            ],
            providers: vec!["mock".into()],
            strategies: vec![Strategy::RetryOnly, Strategy::Recovery],
            repeats: 3,
            timeout: "5s".into(),
        }
    }
}
impl ChallengeRequest {
    fn validate(&self) -> Result<()> {
        fn unique<T: Ord>(values: &[T]) -> bool {
            !values.is_empty() && values.iter().collect::<BTreeSet<_>>().len() == values.len()
        }
        if !unique(&self.scenarios) || !unique(&self.providers) || !unique(&self.strategies) {
            bail!("challenge scenarios, providers and strategies must be nonempty and unique");
        }
        if self.repeats == 0
            || self.repeats > 10
            || self.providers.len() > 4
            || self.scenarios.len() * self.providers.len() * self.strategies.len() * self.repeats
                > 100
        {
            bail!("challenge supports 1–10 repeats, up to four providers and at most 100 trials");
        }
        for provider in &self.providers {
            validate_provider(provider)?;
            if provider == "auto" {
                bail!("challenge providers must be explicit; auto is not supported");
            }
        }
        if parse_duration(&self.timeout)? > Duration::from_secs(60) {
            bail!("challenge trial timeout must not exceed 60s");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trial {
    pub run_id: String,
    pub session: String,
    pub scenario: Scenario,
    pub provider: String,
    pub strategy: Strategy,
    pub repetition: usize,
    pub status: String,
    pub passed: bool,
    pub elapsed_ms: Option<u64>,
    pub recovery_ms: Option<u64>,
    pub tool_attempts: usize,
    pub retries: usize,
    pub replans: usize,
    pub faults_injected: usize,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    pub id: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub status: String,
    pub cancel_requested: bool,
    pub request: ChallengeRequest,
    pub execution_config: Config,
    pub trials: Vec<Trial>,
}
#[derive(Debug, Serialize)]
pub struct Ranking {
    pub provider: String,
    pub strategy: Strategy,
    pub planned: usize,
    pub finished: usize,
    pub passed: usize,
    pub pass_rate: f64,
    pub mean_elapsed_ms: Option<f64>,
    pub retries: usize,
    pub replans: usize,
}
impl Challenge {
    pub fn report(&self) -> Value {
        json!({"challenge": self, "rankings": self.rankings()})
    }
    pub fn rankings(&self) -> Vec<Ranking> {
        let mut groups: BTreeMap<(String, Strategy), Vec<&Trial>> = BTreeMap::new();
        for trial in &self.trials {
            groups
                .entry((trial.provider.clone(), trial.strategy))
                .or_default()
                .push(trial);
        }
        let mut rankings: Vec<_> = groups
            .into_iter()
            .map(|((provider, strategy), trials)| {
                let durations: Vec<_> =
                    trials.iter().filter_map(|trial| trial.elapsed_ms).collect();
                let passed = trials.iter().filter(|trial| trial.passed).count();
                Ranking {
                    provider,
                    strategy,
                    planned: trials.len(),
                    finished: trials
                        .iter()
                        .filter(|trial| !["queued", "running"].contains(&trial.status.as_str()))
                        .count(),
                    passed,
                    pass_rate: passed as f64 / trials.len() as f64,
                    mean_elapsed_ms: (!durations.is_empty()).then(|| {
                        durations.iter().map(|ms| *ms as f64).sum::<f64>() / durations.len() as f64
                    }),
                    retries: trials.iter().map(|trial| trial.retries).sum(),
                    replans: trials.iter().map(|trial| trial.replans).sum(),
                }
            })
            .collect();
        rankings.sort_by(|a, b| {
            b.pass_rate
                .total_cmp(&a.pass_rate)
                .then(a.retries.cmp(&b.retries))
                .then(a.provider.cmp(&b.provider))
                .then(a.strategy.cmp(&b.strategy))
        });
        rankings
    }
}

#[derive(Debug, Clone)]
pub struct Challenges {
    pub workspace: Workspace,
}
impl Challenges {
    fn directory(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.workspace.store.root.join("challenges").join(id))
    }
    fn lock(&self) -> Result<Lease> {
        Lease::acquire_wait(
            &self.workspace.store.root.join(".challenges.lock"),
            Duration::from_secs(2),
        )
    }
    fn save(&self, challenge: &Challenge) -> Result<()> {
        write_json(
            &self.directory(&challenge.id)?.join("challenge.json"),
            challenge,
        )
    }
    pub fn get(&self, id: &str) -> Result<Challenge> {
        read_json(&self.directory(id)?.join("challenge.json"))
    }
    pub fn list(&self) -> Result<Vec<Challenge>> {
        let directory = self.workspace.store.root.join("challenges");
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut challenges = Vec::new();
        for entry in fs::read_dir(directory)? {
            let path = entry?.path().join("challenge.json");
            if path.is_file() {
                challenges.push(read_json::<Challenge>(&path)?);
            }
        }
        challenges.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        Ok(challenges)
    }
    pub fn create(&self, request: ChallengeRequest) -> Result<Challenge> {
        request.validate()?;
        let catalog = self.workspace.validate_skills()?;
        let prompt = fs::read_to_string(self.workspace.project.join("prompts/planner_prompt.txt"))?;
        let context = self.workspace.context()?;
        let id = uuid::Uuid::new_v4().to_string();
        let mut config = self.workspace.config.clone();
        config.webhook = None;
        config.memory_recall = false;
        config.skill_dirs.clear();
        config.context_files.clear();
        let assets = self.directory(&id)?.join("assets");
        fs::create_dir_all(assets.join("skills"))?;
        fs::create_dir_all(assets.join("prompts"))?;
        for (index, skill) in catalog.values().enumerate() {
            crate::memory::atomic_write(
                &assets.join("skills").join(format!("{index}.yaml")),
                serde_yaml::to_string(skill)?.as_bytes(),
            )?;
        }
        crate::memory::atomic_write(
            &assets.join("prompts/planner_prompt.txt"),
            prompt.as_bytes(),
        )?;
        if !context.is_empty() {
            crate::memory::atomic_write(&assets.join("context.md"), context.as_bytes())?;
            config.context_files.push("context.md".into());
        }
        let mut snapshot = self.workspace.clone();
        snapshot.project = assets;
        snapshot.config = config.clone();
        snapshot.context()?;
        let mut trials = Vec::new();
        // Repetition first keeps comparison samples interleaved by scenario.
        for repetition in 1..=request.repeats {
            for scenario in &request.scenarios {
                for provider in &request.providers {
                    for strategy in &request.strategies {
                        let run_id = uuid::Uuid::new_v4().to_string();
                        trials.push(Trial {
                            session: format!("challenge_{}", uuid::Uuid::new_v4().simple()),
                            run_id,
                            scenario: *scenario,
                            provider: provider.clone(),
                            strategy: *strategy,
                            repetition,
                            status: "queued".into(),
                            passed: false,
                            elapsed_ms: None,
                            recovery_ms: None,
                            tool_attempts: 0,
                            retries: 0,
                            replans: 0,
                            faults_injected: 0,
                            error: None,
                        });
                    }
                }
            }
        }
        let timestamp = now_millis();
        let challenge = Challenge {
            id,
            created_at: timestamp,
            updated_at: timestamp,
            status: "queued".into(),
            cancel_requested: false,
            request,
            execution_config: config,
            trials,
        };
        let _lease = self.lock()?;
        self.save(&challenge)?;
        Ok(challenge)
    }
    pub fn cancel(&self, id: &str) -> Result<Challenge> {
        let _lease = self.lock()?;
        let mut challenge = self.get(id)?;
        if ["queued", "running"].contains(&challenge.status.as_str()) {
            challenge.cancel_requested = true;
            if challenge.status == "queued" {
                challenge.status = "cancelled".into();
                for trial in &mut challenge.trials {
                    trial.status = "cancelled".into();
                }
            }
            challenge.updated_at = now_millis();
            self.save(&challenge)?;
        }
        Ok(challenge)
    }
    /// Caller holds the shared job runner lease. Interrupted challenges never replay.
    pub fn recover_interrupted(&self) -> Result<()> {
        let _lease = self.lock()?;
        for mut challenge in self.list()? {
            if challenge.status == "running" {
                challenge.status = "interrupted".into();
                challenge.updated_at = now_millis();
                for trial in &mut challenge.trials {
                    if trial.status == "running" {
                        trial.status = "interrupted".into();
                        trial.error = Some("runner stopped; trial was not replayed".into());
                    }
                }
                self.save(&challenge)?;
            }
        }
        Ok(())
    }
    /// Caller holds the shared job runner lease.
    pub fn claim(&self, id: &str) -> Result<Challenge> {
        let _lease = self.lock()?;
        let mut challenge = self.get(id)?;
        if challenge.status != "queued" {
            bail!("challenge requires a queued claim");
        }
        challenge.status = "running".into();
        challenge.updated_at = now_millis();
        self.save(&challenge)?;
        Ok(challenge)
    }
    pub fn claim_next(&self) -> Result<Option<Challenge>> {
        match self
            .list()?
            .into_iter()
            .find(|challenge| challenge.status == "queued")
        {
            Some(challenge) => Ok(Some(self.claim(&challenge.id)?)),
            None => Ok(None),
        }
    }
    /// Each trial receives a fresh simulator, fault counters, memory and budget.
    pub fn execute(&self, claimed: Challenge, shutdown: &ExecutionControl) -> Result<Challenge> {
        let current = self.get(&claimed.id)?;
        if claimed.status != "running"
            || current.status != "running"
            || claimed
                .trials
                .iter()
                .map(|trial| &trial.run_id)
                .ne(current.trials.iter().map(|trial| &trial.run_id))
        {
            bail!("challenge requires its running claim");
        }
        let claimed = current;
        for index in 0..claimed.trials.len() {
            let control =
                ExecutionControl::with_timeout(parse_duration(&claimed.request.timeout)?)?;
            let trial = {
                let _lease = self.lock()?;
                let mut current = self.get(&claimed.id)?;
                if current.cancel_requested || shutdown.stop_reason().is_some() {
                    break;
                }
                if current.trials[index].status != "queued" {
                    bail!("challenge trial cannot be replayed");
                }
                current.trials[index].status = "running".into();
                current.updated_at = now_millis();
                self.save(&current)?;
                current.trials[index].clone()
            };
            let mut workspace = self.workspace.clone();
            workspace.project = self.directory(&claimed.id)?.join("assets");
            workspace.config = claimed.execution_config.clone();
            workspace.config.max_replans = if trial.strategy == Strategy::Recovery {
                1
            } else {
                0
            };
            let request = RunRequest {
                instruction: INSTRUCTION.into(),
                session: trial.session.clone(),
                provider: Some(trial.provider.clone()),
                fallbacks: Some(Vec::new()),
                timeout: Some(claimed.request.timeout.clone()),
            };
            let faults = FaultInjection::new(trial.scenario);
            let started = Instant::now();
            let done = AtomicBool::new(false);
            struct Completion<'a>(&'a AtomicBool);
            impl Drop for Completion<'_> {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Release);
                }
            }
            let (result, finished) = std::thread::scope(|scope| {
                scope.spawn(|| {
                    while !done.load(Ordering::Acquire) {
                        if shutdown.stop_reason().is_some()
                            || self
                                .get(&claimed.id)
                                .map(|challenge| challenge.cancel_requested)
                                .unwrap_or(true)
                        {
                            control.cancel();
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(20));
                    }
                });
                let _completion = Completion(&done);
                let result = workspace.run_trial(request, &control, &trial.run_id, &faults);
                (result, Instant::now())
            });
            let elapsed_ms = finished.duration_since(started).as_millis().try_into()?;
            let stats = faults
                .stats
                .lock()
                .map_err(|_| anyhow::anyhow!("challenge fault metrics poisoned"))?;
            let _lease = self.lock()?;
            let mut current = self.get(&claimed.id)?;
            let saved = &mut current.trials[index];
            saved.elapsed_ms = Some(elapsed_ms);
            saved.tool_attempts = stats.attempts;
            saved.faults_injected = stats.injected;
            match result {
                Ok(result) => {
                    saved.status = result.execution.status.as_str().into();
                    saved.replans = result.execution.replans;
                    if saved.status == "failed" {
                        saved.error = result
                            .execution
                            .report
                            .as_ref()
                            .and_then(|report| report.steps.last())
                            .map(|step| step.observation.clone());
                    }
                    saved.retries = result
                        .execution
                        .reports
                        .iter()
                        .flat_map(|report| &report.steps)
                        .map(|step| step.attempts.saturating_sub(1))
                        .sum();
                    saved.passed = if trial.scenario == Scenario::SensorTimeout {
                        saved.status == "timed_out" && stats.injected > 0 && stats.commands == 0
                    } else {
                        saved.status == "completed"
                            && stats.placements > 0
                            && result.execution.backend_state.last_pose == "bin_a"
                            && result.execution.backend_state.held_object.is_none()
                            && (trial.scenario == Scenario::Baseline || stats.injected > 0)
                    };
                    if saved.passed && trial.scenario != Scenario::SensorTimeout {
                        saved.recovery_ms = stats
                            .first_fault
                            .map(|time| finished.duration_since(time).as_millis())
                            .map(u64::try_from)
                            .transpose()?;
                    }
                }
                Err(error) => {
                    saved.status = "failed".into();
                    saved.error = Some(format!("{error:#}"));
                }
            }
            current.updated_at = now_millis();
            self.save(&current)?;
        }
        let _lease = self.lock()?;
        let mut current = self.get(&claimed.id)?;
        current.status = if current.cancel_requested || shutdown.stop_reason().is_some() {
            "cancelled"
        } else {
            "completed"
        }
        .into();
        for trial in &mut current.trials {
            if trial.status == "queued" {
                trial.status = "cancelled".into();
            }
        }
        current.updated_at = now_millis();
        self.save(&current)?;
        Ok(current)
    }
}

#[derive(Default)]
struct FaultStats {
    attempts: usize,
    commands: usize,
    placements: usize,
    injected: usize,
    sensor_calls: usize,
    grasp_calls: usize,
    first_fault: Option<Instant>,
}
pub(crate) struct FaultInjection {
    scenario: Scenario,
    stats: Arc<Mutex<FaultStats>>,
}
impl FaultInjection {
    fn new(scenario: Scenario) -> Self {
        Self {
            scenario,
            stats: Arc::new(Mutex::new(FaultStats::default())),
        }
    }
    pub(crate) fn registry(
        &self,
        backend: Arc<dyn RobotBackend>,
        policy: ToolPolicy,
    ) -> ToolRegistry {
        let mut registry = ToolRegistry::new().with_policy(policy);
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(SensorTool::without_transient_failures()),
            Box::new(MotorControlTool::without_transient_failures(
                backend.clone(),
            )),
            Box::new(SimulatorTool::new(backend)),
        ];
        for inner in tools {
            registry.register_tool(FaultTool {
                inner,
                scenario: self.scenario,
                stats: self.stats.clone(),
            });
        }
        registry
    }
}
struct FaultTool {
    inner: Box<dyn Tool>,
    scenario: Scenario,
    stats: Arc<Mutex<FaultStats>>,
}
impl Tool for FaultTool {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn validate_input(&self, input: &Value) -> Result<()> {
        self.inner.validate_input(input)
    }
    fn execute(&self, input: Value) -> Result<Value> {
        self.execute_with_control(input, &ExecutionControl::default())
    }
    fn execute_with_control(&self, input: Value, control: &ExecutionControl) -> Result<Value> {
        control.check()?;
        self.validate_input(&input)?;
        let sensor = self.name() == "sensor" && input["target"] == "red_cube";
        let grasp = self.name() == "motor_control" && input["action"] == "grasp";
        let fail = {
            let mut stats = self
                .stats
                .lock()
                .map_err(|_| anyhow::anyhow!("challenge fault metrics poisoned"))?;
            stats.attempts += 1;
            if self.name() != "sensor" {
                stats.commands += 1;
            }
            if sensor {
                stats.sensor_calls += 1;
            }
            if grasp {
                stats.grasp_calls += 1;
            }
            let fail = match self.scenario {
                Scenario::SensorGlitch => sensor && stats.sensor_calls <= 1,
                Scenario::SensorOutage => sensor && stats.sensor_calls <= 2,
                Scenario::GraspStall => grasp && stats.grasp_calls <= 2,
                Scenario::PersistentStall => grasp,
                Scenario::SensorTimeout => sensor,
                Scenario::Baseline => false,
            };
            if fail {
                stats.injected += 1;
                stats.first_fault.get_or_insert_with(Instant::now);
            }
            fail
        };
        if fail {
            if self.scenario == Scenario::SensorTimeout {
                control.wait(Duration::from_secs(61))?;
            }
            return Ok(
                json!({"tool":self.name(),"detected":false,"accepted":false,"target":input["target"],"detail":"injected recovery challenge failure"}),
            );
        }
        let output = self.inner.execute_with_control(input.clone(), control)?;
        if input["action"] == "place"
            && input["target"] == "red_cube"
            && input["location"] == "bin_a"
            && output["accepted"] == true
        {
            self.stats
                .lock()
                .map_err(|_| anyhow::anyhow!("challenge fault metrics poisoned"))?
                .placements += 1;
        }
        Ok(output)
    }
}
