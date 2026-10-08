use anyhow::Result;
use roboclaw_rs::agent::{Agent, ExecutionStatus, Executor, PlanDecision, Planner, StepStatus};
use roboclaw_rs::gateway::RoboclawGateway;
use roboclaw_rs::memory::Memory;
use roboclaw_rs::ros2::{Ros2Bridge, CMD_VEL_TOPIC, ROBOCLAW_ACTION_TOPIC};
use roboclaw_rs::sim::{GazeboBackend, RobotBackend};
use roboclaw_rs::skills::SkillCatalog;
use roboclaw_rs::tools::{ExecutionControl, Tool, ToolRegistry};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "roboclaw-control-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(path.join("skills")).unwrap();
        fs::write(path.join("skills/main.yaml"), "name: main\ndescription: main\nsteps:\n  - name: first\n    tool: controlled\n    max_retries: 2\n    expect:\n      accepted: true\n  - name: later\n    tool: controlled\n").unwrap();
        fs::write(path.join("skills/recovery.yaml"), "name: recovery\ndescription: recovery\nresume_original_instruction: true\nsteps:\n  - name: recover\n    tool: controlled\n").unwrap();
        Self(path)
    }

    fn gateway(&self, planner: Box<dyn Planner>, tool: impl Tool + 'static) -> RoboclawGateway {
        let catalog = SkillCatalog::from_dir(self.0.join("skills")).unwrap();
        let memory = Memory::new(self.0.join("memory")).unwrap();
        let ros2 = Ros2Bridge::mock("control-test");
        let backend: Arc<dyn RobotBackend> = Arc::new(GazeboBackend::with_ros2(ros2.clone()));
        let mut registry = ToolRegistry::new();
        registry.register_tool(tool);
        RoboclawGateway::new(
            Agent::new(memory, planner, Executor::new(registry)),
            catalog,
            ros2,
            backend,
        )
    }

    fn events(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.0.join("memory/short_term.json")).unwrap())
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct SelectPlanner {
    calls: Arc<AtomicUsize>,
    stop: Option<(usize, ExecutionControl)>,
    expected_control: Option<ExecutionControl>,
}

impl Planner for SelectPlanner {
    fn plan(&self, _instruction: String, catalog: &SkillCatalog) -> Result<PlanDecision> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some((stop_call, control)) = &self.stop {
            if call == *stop_call {
                control.cancel();
            }
        }
        Ok(PlanDecision {
            skill: catalog
                .get(if call == 0 { "main" } else { "recovery" })
                .unwrap()
                .clone(),
            reason: None,
        })
    }
    fn plan_with_control(
        &self,
        instruction: String,
        catalog: &SkillCatalog,
        control: &ExecutionControl,
    ) -> Result<PlanDecision> {
        control.check()?;
        if let Some(expected) = &self.expected_control {
            assert!(
                control
                    .remaining_time()
                    .unwrap()
                    .abs_diff(expected.remaining_time().unwrap())
                    < Duration::from_millis(20),
                "recovery must inherit the original deadline"
            );
        }
        let decision = self.plan(instruction, catalog);
        control.check()?;
        decision
    }
}

struct ControlledTool {
    calls: Arc<AtomicUsize>,
    effects: Arc<AtomicUsize>,
    cancel: bool,
    failures: usize,
}

impl Tool for ControlledTool {
    fn name(&self) -> &str {
        "controlled"
    }

    fn execute(&self, _input: Value) -> Result<Value> {
        unreachable!("controlled implementation must be called")
    }

    fn execute_with_control(&self, _input: Value, control: &ExecutionControl) -> Result<Value> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if self.cancel {
            control.cancel();
        }
        if call < self.failures {
            control.wait(Duration::from_millis(100))?;
            return Ok(json!({"accepted": false}));
        }
        control.wait(Duration::from_secs(10))?;
        self.effects.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"accepted": true}))
    }
}

fn tool(cancel: bool, failures: usize) -> (ControlledTool, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let effects = Arc::new(AtomicUsize::new(0));
    (
        ControlledTool {
            calls: calls.clone(),
            effects: effects.clone(),
            cancel,
            failures,
        },
        calls,
        effects,
    )
}

fn planner(stop: Option<ExecutionControl>) -> (Box<dyn Planner>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    (
        Box::new(SelectPlanner {
            calls: calls.clone(),
            stop: stop.map(|control| (0, control)),
            expected_control: None,
        }),
        calls,
    )
}

#[test]
fn precancelled_run_never_plans_or_executes_and_persists_reason() {
    let project = Project::new();
    let control = ExecutionControl::default();
    control.cancel();
    let (planner, plans) = planner(None);
    let (tool, calls, effects) = tool(false, 0);
    let mut gateway = project.gateway(planner, tool);
    let result = gateway
        .handle_instruction_with_control("main", &control)
        .unwrap();
    assert_eq!(result.status, ExecutionStatus::Cancelled);
    assert!(result.report.is_none());
    assert!(result.reports.is_empty());
    assert_eq!(plans.load(Ordering::SeqCst), 0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(project.events()[0]["payload"]["stage"], "planning");
    assert!(fs::read_to_string(project.0.join("memory/long_term.md"))
        .unwrap()
        .contains("cancelled"));
    let messages = gateway.ros2_bridge().published_messages();
    assert!(!messages
        .iter()
        .any(|message| message.topic == CMD_VEL_TOPIC));
    assert!(messages
        .iter()
        .any(|message| message.topic == ROBOCLAW_ACTION_TOPIC
            && message.payload["event"] == "execution_stopped"));
}

#[test]
fn cancellation_during_planning_discards_decision_without_starting_tools() {
    let project = Project::new();
    let control = ExecutionControl::default();
    let (planner, plans) = planner(Some(control.clone()));
    let (tool, calls, _) = tool(false, 0);
    let mut gateway = project.gateway(planner, tool);
    let result = gateway
        .handle_instruction_with_control("main", &control)
        .unwrap();
    assert_eq!(result.status, ExecutionStatus::Cancelled);
    assert!(result.report.is_none());
    assert_eq!(plans.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn cancelled_cooperative_tool_is_never_retried_or_replanned() {
    let project = Project::new();
    let (planner, plans) = planner(None);
    let (tool, calls, effects) = tool(true, 0);
    let mut gateway = project.gateway(planner, tool);
    let result = gateway
        .handle_instruction_with_control("main", &ExecutionControl::default())
        .unwrap();
    assert_eq!(result.status, ExecutionStatus::Cancelled);
    let report = result.report.unwrap();
    assert!(!report.completed);
    assert_eq!(report.steps.len(), 1);
    assert_eq!(report.steps[0].status, StepStatus::Cancelled);
    assert_eq!(report.steps[0].attempts, 1);
    assert_eq!(result.replans, 0);
    assert_eq!(plans.load(Ordering::SeqCst), 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert!(project
        .events()
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "execution_stopped"
            && event["payload"]["status"] == "cancelled"));
}

#[test]
fn synchronous_success_is_preserved_when_cancellation_arrives_in_flight() {
    let project = Project::new();
    let (planner, _) = planner(None);
    struct LegacyTool {
        control: ExecutionControl,
        calls: Arc<AtomicUsize>,
        effects: Arc<AtomicUsize>,
    }
    impl Tool for LegacyTool {
        fn name(&self) -> &str {
            "controlled"
        }
        fn execute(&self, _input: Value) -> Result<Value> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.control.cancel();
            self.effects.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"accepted": true}))
        }
    }
    let control = ExecutionControl::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let effects = Arc::new(AtomicUsize::new(0));
    let tool = LegacyTool {
        control: control.clone(),
        calls: calls.clone(),
        effects: effects.clone(),
    };
    let mut gateway = project.gateway(planner, tool);
    let result = gateway
        .handle_instruction_with_control("main", &control)
        .unwrap();
    assert_eq!(result.status, ExecutionStatus::Cancelled);
    let report = result.report.unwrap();
    assert_eq!(report.steps.len(), 1);
    assert_eq!(report.steps[0].status, StepStatus::Succeeded);
    assert_eq!(report.steps[0].output["accepted"], true);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    assert_eq!(result.replans, 0);
}

#[test]
fn deadline_interrupts_slow_tool_without_late_effects() {
    let project = Project::new();
    let (planner, _) = planner(None);
    let (tool, calls, effects) = tool(false, 0);
    let mut gateway = project.gateway(planner, tool);
    let control = ExecutionControl::with_timeout(Duration::from_millis(200)).unwrap();
    let started = Instant::now();
    let result = gateway
        .handle_instruction_with_control("main", &control)
        .unwrap();
    assert_eq!(result.status, ExecutionStatus::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(result.report.unwrap().steps[0].status, StepStatus::TimedOut);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(result.replans, 0);
}

#[test]
fn retries_and_recovery_share_the_original_deadline() {
    let project = Project::new();
    let control = ExecutionControl::with_timeout(Duration::from_secs(1)).unwrap();
    let plans = Arc::new(AtomicUsize::new(0));
    let planner = Box::new(SelectPlanner {
        calls: plans.clone(),
        stop: None,
        expected_control: Some(control.clone()),
    });
    // Exhaust three attempts; the recovery must inherit the remaining budget.
    let (tool, calls, effects) = tool(false, 3);
    let mut gateway = project.gateway(planner, tool);

    let result = gateway
        .handle_instruction_with_control("main", &control)
        .unwrap();
    assert_eq!(result.status, ExecutionStatus::TimedOut);
    assert_eq!(result.replans, 1);
    assert_eq!(result.reports.len(), 2);
    assert_eq!(result.reports[0].status, ExecutionStatus::Failed);
    assert_eq!(result.reports[0].steps[0].attempts, 3);
    assert_eq!(result.reports[1].status, ExecutionStatus::TimedOut);
    assert_eq!(plans.load(Ordering::SeqCst), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(effects.load(Ordering::SeqCst), 0);
}

#[test]
fn http_planners_cap_blocking_requests_to_the_remaining_budget() {
    use roboclaw_rs::agent::{
        ClaudePlanner, ClaudePlannerConfig, OllamaPlanner, OllamaPlannerConfig, OpenAiPlanner,
        OpenAiPlannerConfig,
    };
    use roboclaw_rs::tools::StopReason;
    use std::net::TcpListener;
    use std::sync::mpsc;

    let project = Project::new();
    let prompt = project.0.join("prompt.txt");
    fs::write(&prompt, "Choose a skill.").unwrap();
    let catalog = SkillCatalog::from_dir(project.0.join("skills")).unwrap();
    for provider in ["local", "openai", "claude"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let (release, finished) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let started = Instant::now();
            loop {
                match listener.accept() {
                    Ok((stream, _)) => {
                        // Hold the connection without responding until planning returns.
                        let _ = finished.recv_timeout(Duration::from_secs(5));
                        drop(stream);
                        return true;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if started.elapsed() > Duration::from_secs(5) {
                            return false;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("accept failed: {error}"),
                }
            }
        });
        let planner: Box<dyn Planner> = match provider {
            "local" => {
                let mut config = OllamaPlannerConfig::new("test-model");
                config.host = host;
                Box::new(OllamaPlanner::from_file(&prompt, config).unwrap())
            }
            "openai" => {
                let mut config = OpenAiPlannerConfig::new("test-key");
                config.base_url = host;
                Box::new(OpenAiPlanner::from_file(&prompt, config).unwrap())
            }
            _ => {
                let mut config = ClaudePlannerConfig::new("test-key");
                config.base_url = host;
                Box::new(ClaudePlanner::from_file(&prompt, config).unwrap())
            }
        };
        let control = ExecutionControl::with_timeout(Duration::from_millis(200)).unwrap();
        let started = Instant::now();
        let result = planner.plan_with_control("main".into(), &catalog, &control);
        let elapsed = started.elapsed();
        let _ = release.send(());
        assert!(server.join().unwrap(), "{provider} did not connect");
        let error = result.unwrap_err();
        assert_eq!(
            error.downcast_ref::<StopReason>(),
            Some(&StopReason::TimedOut),
            "{provider}: {error:#}"
        );
        assert!(
            elapsed < Duration::from_secs(3),
            "{provider} ignored deadline"
        );
    }
}

#[test]
fn cancelling_recovery_planning_preserves_the_previous_failed_report() {
    let project = Project::new();
    let control = ExecutionControl::default();
    let plans = Arc::new(AtomicUsize::new(0));
    let planner = Box::new(SelectPlanner {
        calls: plans.clone(),
        stop: Some((1, control.clone())),
        expected_control: None,
    });
    let (tool, calls, effects) = tool(false, 3);
    let mut gateway = project.gateway(planner, tool);
    let result = gateway
        .handle_instruction_with_control("main", &control)
        .unwrap();
    assert_eq!(result.status, ExecutionStatus::Cancelled);
    assert_eq!(result.reports.len(), 1);
    assert_eq!(result.report.unwrap().status, ExecutionStatus::Failed);
    assert_eq!(result.replans, 1);
    assert_eq!(plans.load(Ordering::SeqCst), 2);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    let events = project.events();
    let stop = events
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["kind"] == "execution_stopped")
        .unwrap();
    assert_eq!(stop["payload"]["status"], "cancelled");
    assert_eq!(stop["payload"]["stage"], "planning");
}
