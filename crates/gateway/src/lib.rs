use anyhow::Result;
use roboclaw_agent::{Agent, AgentReport, StepStatus};
use roboclaw_ros2::{
    RoboclawActionMessage, Ros2Bridge, CMD_VEL_TOPIC, JOINT_STATES_TOPIC, ROBOCLAW_ACTION_TOPIC,
    ROBOCLAW_STATE_TOPIC,
};
use roboclaw_sim::{state_to_ros2_message, RobotBackend, RobotState};
use roboclaw_skills::{RecoveryContext, SkillCatalog};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRunResult {
    pub report: AgentReport,
    pub reports: Vec<AgentReport>,
    pub backend_state: RobotState,
    pub replans: usize,
    pub topics: Vec<String>,
}

pub struct RoboclawGateway {
    agent: Agent,
    catalog: SkillCatalog,
    ros2: Ros2Bridge,
    backend: Arc<dyn RobotBackend>,
    max_replans: usize,
}

#[derive(Debug, Clone)]
struct ResumeContext {
    skill_name: String,
    step_name: String,
}

impl RoboclawGateway {
    pub fn new(
        agent: Agent,
        catalog: SkillCatalog,
        ros2: Ros2Bridge,
        backend: Arc<dyn RobotBackend>,
    ) -> Self {
        Self::with_max_replans(agent, catalog, ros2, backend, 1)
    }

    pub fn with_max_replans(
        agent: Agent,
        catalog: SkillCatalog,
        ros2: Ros2Bridge,
        backend: Arc<dyn RobotBackend>,
        max_replans: usize,
    ) -> Self {
        Self {
            agent,
            catalog,
            ros2,
            backend,
            max_replans,
        }
    }

    pub fn handle_instruction(&mut self, instruction: &str) -> Result<GatewayRunResult> {
        self.ros2.publish_action(&RoboclawActionMessage {
            event: "instruction_received".to_string(),
            instruction: Some(instruction.to_string()),
            skill: None,
            step: None,
            tool: None,
            backend: Some(self.backend.name().to_string()),
            action: None,
            detail: None,
            data: None,
        })?;

        let mut reports = Vec::new();
        let mut replans = 0usize;
        let mut planning_instruction = instruction.to_string();
        let mut resume_context: Option<ResumeContext> = None;

        loop {
            let execution_attempt = reports.len() + 1;
            let decision = self
                .agent
                .plan_only(planning_instruction.clone(), &self.catalog)?;
            let recovery_candidates =
                report_recovery_candidates(&self.catalog, &reports, &planning_instruction);
            let resumed_from_step = resume_context
                .as_ref()
                .filter(|context| {
                    planning_instruction == instruction
                        && decision.skill.name == context.skill_name
                        && decision.skill.supports_checkpoint_resume
                })
                .map(|context| context.step_name.clone());

            self.ros2.publish_action(&RoboclawActionMessage {
                event: "skill_selected".to_string(),
                instruction: None,
                skill: Some(decision.skill.name.clone()),
                step: None,
                tool: None,
                backend: Some(self.backend.name().to_string()),
                action: None,
                detail: decision.reason.clone(),
                data: Some(json!({
                    "execution_attempt": execution_attempt,
                    "replan_count": replans,
                    "resumed_from_step": resumed_from_step,
                    "suggested_recovery_skills": recovery_candidates,
                })),
            })?;

            let report = if let Some(step_name) = resumed_from_step.clone() {
                self.agent.run_with_decision_from_step(
                    instruction.to_string(),
                    decision,
                    step_name,
                )?
            } else {
                self.agent
                    .run_with_decision(instruction.to_string(), decision)?
            };

            self.publish_report(&report, execution_attempt, replans)?;

            let backend_state = self.backend.current_state();
            reports.push(report.clone());

            if report.completed && report.skill.resume_original_instruction {
                self.ros2.publish_action(&RoboclawActionMessage {
                    event: "recovery_completed".to_string(),
                    instruction: Some(instruction.to_string()),
                    skill: Some(report.skill.name.clone()),
                    step: None,
                    tool: None,
                    backend: Some(self.backend.name().to_string()),
                    action: None,
                    detail: Some("resuming original instruction after recovery skill".to_string()),
                    data: Some(json!({
                        "execution_attempt": execution_attempt,
                        "replan_count": replans,
                        "next_instruction": instruction,
                        "resume_from_step": resume_context.as_ref().map(|context| context.step_name.clone()),
                    })),
                })?;
                planning_instruction = instruction.to_string();
                continue;
            }

            if report.completed || replans >= self.max_replans {
                return Ok(GatewayRunResult {
                    report,
                    reports,
                    backend_state,
                    replans,
                    topics: vec![
                        CMD_VEL_TOPIC.to_string(),
                        JOINT_STATES_TOPIC.to_string(),
                        ROBOCLAW_ACTION_TOPIC.to_string(),
                        ROBOCLAW_STATE_TOPIC.to_string(),
                    ],
                });
            }

            replans += 1;
            let recovery_candidates = self.recovery_candidates_for_report(&report);
            planning_instruction = self.build_replan_instruction(
                instruction,
                &report,
                &backend_state,
                &recovery_candidates,
            );
            resume_context = checkpoint_resume_context(&report);

            self.ros2.publish_action(&RoboclawActionMessage {
                event: "execution_replan_requested".to_string(),
                instruction: None,
                skill: Some(report.skill.name.clone()),
                step: report.failed_step.clone(),
                tool: None,
                backend: Some(self.backend.name().to_string()),
                action: None,
                detail: Some(report.next_action.clone()),
                data: Some(json!({
                    "execution_attempt": execution_attempt,
                    "replan_count": replans,
                    "failed_step": report.failed_step.clone(),
                    "resume_from_step": resume_context.as_ref().map(|context| context.step_name.clone()),
                    "suggested_recovery_skills": recovery_candidates,
                    "backend_state": backend_state,
                })),
            })?;

            self.ros2.publish_action(&RoboclawActionMessage {
                event: "replan_started".to_string(),
                instruction: Some(instruction.to_string()),
                skill: None,
                step: report.failed_step.clone(),
                tool: None,
                backend: Some(self.backend.name().to_string()),
                action: None,
                detail: Some("planning follow-up skill after failed execution".to_string()),
                data: Some(json!({
                    "next_execution_attempt": execution_attempt + 1,
                    "replan_count": replans,
                    "resume_from_step": resume_context.as_ref().map(|context| context.step_name.clone()),
                    "suggested_recovery_skills": self.recovery_candidates_for_report(&report),
                })),
            })?;
        }
    }

    fn publish_report(
        &self,
        report: &AgentReport,
        execution_attempt: usize,
        replans: usize,
    ) -> Result<()> {
        for step in &report.steps {
            self.ros2.publish_action(&RoboclawActionMessage {
                event: match step.status {
                    StepStatus::Succeeded => "step_completed".to_string(),
                    StepStatus::Failed => "step_failed".to_string(),
                },
                instruction: None,
                skill: Some(report.skill.name.clone()),
                step: Some(step.step_name.clone()),
                tool: Some(step.tool_name.clone()),
                backend: Some(self.backend.name().to_string()),
                action: None,
                detail: Some(step.observation.clone()),
                data: Some(json!({
                    "execution_attempt": execution_attempt,
                    "replan_count": replans,
                    "resumed_from_step": report.resumed_from_step,
                    "attempts": step.attempts,
                    "status": step.status.as_str(),
                    "output": step.output.clone(),
                })),
            })?;
        }

        let mut state_message = state_to_ros2_message(&self.backend.current_state());
        state_message.active_skill = Some(report.skill.name.clone());
        state_message.next_action = Some(report.next_action.clone());
        state_message.steps_executed = Some(report.steps.len());
        state_message.completed = Some(report.completed);
        state_message.failed_step = report.failed_step.clone();
        self.ros2.publish_state(&state_message)?;
        Ok(())
    }

    fn build_replan_instruction(
        &self,
        instruction: &str,
        report: &AgentReport,
        backend_state: &RobotState,
        recovery_candidates: &[String],
    ) -> String {
        let failed_tool = failed_tool_name(report).unwrap_or("unknown");
        let suggested_recovery_skills = if recovery_candidates.is_empty() {
            "none".to_string()
        } else {
            recovery_candidates.join(", ")
        };
        format!(
            "Original instruction:\n{instruction}\n\nPrevious execution failed.\nFailed step: {}\nFailed tool: {}\nObservation: {}\nBackend state: {}\nRequested next action: {}\nSuggested recovery skills: {}\nChoose the best next skill from the catalog.",
            report.failed_step.as_deref().unwrap_or("unknown"),
            failed_tool,
            report
                .steps
                .last()
                .map(|step| step.observation.as_str())
                .unwrap_or("no step observation"),
            serde_json::to_string(backend_state)
                .unwrap_or_else(|_| "{\"backend\":\"unknown\"}".to_string()),
            report.next_action,
            suggested_recovery_skills,
        )
    }

    fn recovery_candidates_for_report(&self, report: &AgentReport) -> Vec<String> {
        let context = recovery_context_from_report(report);
        self.catalog.recovery_candidate_names(&context)
    }

    pub fn ros2_bridge(&self) -> &Ros2Bridge {
        &self.ros2
    }

    pub fn topics(&self) -> [&'static str; 4] {
        [
            CMD_VEL_TOPIC,
            JOINT_STATES_TOPIC,
            ROBOCLAW_ACTION_TOPIC,
            ROBOCLAW_STATE_TOPIC,
        ]
    }
}

fn checkpoint_resume_context(report: &AgentReport) -> Option<ResumeContext> {
    let failed_step = report.failed_step.as_ref()?;
    if !report.skill.supports_checkpoint_resume {
        return None;
    }

    let resume_from_step = report
        .skill
        .steps
        .iter()
        .find(|step| step.name == *failed_step)
        .and_then(|step| step.resume_from_step.clone())
        .unwrap_or_else(|| failed_step.clone());

    Some(ResumeContext {
        skill_name: report.skill.name.clone(),
        step_name: resume_from_step,
    })
}

fn recovery_context_from_report(report: &AgentReport) -> RecoveryContext {
    let failed_step_execution = failed_step_execution(report);
    RecoveryContext {
        failed_step: report.failed_step.clone(),
        tool: failed_step_execution.map(|step| step.tool_name.clone()),
        observation: failed_step_execution.map(|step| step.observation.clone()),
    }
}

fn failed_step_execution(report: &AgentReport) -> Option<&roboclaw_agent::StepExecution> {
    report.steps.iter().rev().find(|step| {
        report
            .failed_step
            .as_ref()
            .map(|failed_step| step.step_name == *failed_step)
            .unwrap_or(false)
    })
}

fn failed_tool_name(report: &AgentReport) -> Option<&str> {
    failed_step_execution(report).map(|step| step.tool_name.as_str())
}

fn report_recovery_candidates(
    catalog: &SkillCatalog,
    reports: &[AgentReport],
    planning_instruction: &str,
) -> Vec<String> {
    if planning_instruction
        .to_lowercase()
        .contains("previous execution failed")
    {
        return reports
            .last()
            .map(|report| catalog.recovery_candidate_names(&recovery_context_from_report(report)))
            .unwrap_or_default();
    }

    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use roboclaw_agent::{planner_for_provider, Agent, Executor, LlmProvider};
    use roboclaw_memory::Memory;
    use roboclaw_sim::GazeboBackend;
    use roboclaw_tools::{MotorControlTool, SensorTool, SimulatorTool, ToolRegistry};
    use serde_json::Value;
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn gateway_replans_after_failed_observation_and_recovers() {
        let root = unique_test_dir("roboclaw-gateway-replan");
        let skills_dir = root.join("skills");
        let memory_dir = root.join("memory");
        let prompt_path = root.join("planner_prompt.txt");

        fs::create_dir_all(&skills_dir).expect("skills dir should be creatable");
        fs::write(
            &prompt_path,
            "Choose the best skill from the provided catalog.",
        )
        .expect("prompt file should be writable");
        write_recovery_skills(&skills_dir);

        let catalog = SkillCatalog::from_dir(&skills_dir).expect("catalog should load");
        let memory = Memory::new(&memory_dir).expect("memory should initialize");
        let planner =
            planner_for_provider(&prompt_path, LlmProvider::Mock).expect("planner should build");
        let ros2 = Ros2Bridge::mock("gateway-test");
        let backend: Arc<dyn RobotBackend> = Arc::new(GazeboBackend::with_ros2(ros2.clone()));

        let mut registry = ToolRegistry::new();
        registry.register_tool(SensorTool::with_transient_failures("red_cube", 2));
        registry.register_tool(SimulatorTool::new(backend.clone()));
        registry.register_tool(MotorControlTool::new(backend.clone()));

        let agent = Agent::new(memory, planner, Executor::new(registry));
        let mut gateway =
            RoboclawGateway::with_max_replans(agent, catalog, ros2.clone(), backend, 1);

        let result = gateway
            .handle_instruction("Use the simulator to pick up the red cube and place it in bin_a.")
            .expect("gateway should recover after replanning");

        assert!(result.report.completed);
        assert_eq!(result.replans, 1);
        assert_eq!(result.reports.len(), 3);
        assert!(!result.reports[0].completed);
        assert_eq!(
            result.reports[0].failed_step.as_deref(),
            Some("detect_object")
        );
        assert_eq!(result.reports[1].skill.name, "recover_observation");
        assert!(result.reports[1].completed);
        assert!(result.reports[1].skill.resume_original_instruction);
        assert_eq!(result.reports[2].skill.name, "pick_and_place");
        assert!(result.reports[2].completed);
        assert_eq!(
            result.reports[2].resumed_from_step.as_deref(),
            Some("detect_object")
        );

        let replan_event_seen =
            gateway
                .ros2_bridge()
                .published_messages()
                .into_iter()
                .any(|message| {
                    message.topic == ROBOCLAW_ACTION_TOPIC
                        && message.payload["event"]
                            == Value::String("execution_replan_requested".to_string())
                });
        assert!(replan_event_seen);

        let suggested_recovery_seen =
            gateway
                .ros2_bridge()
                .published_messages()
                .into_iter()
                .any(|message| {
                    message.topic == ROBOCLAW_ACTION_TOPIC
                        && message.payload["data"]["suggested_recovery_skills"]
                            .as_array()
                            .map(|skills| {
                                skills.iter().any(|skill| {
                                    skill == &Value::String("recover_observation".to_string())
                                })
                            })
                            .unwrap_or(false)
                });
        assert!(suggested_recovery_seen);

        let recovery_completed_seen =
            gateway
                .ros2_bridge()
                .published_messages()
                .into_iter()
                .any(|message| {
                    message.topic == ROBOCLAW_ACTION_TOPIC
                        && message.payload["event"]
                            == Value::String("recovery_completed".to_string())
                });
        assert!(recovery_completed_seen);
    }

    #[test]
    fn gateway_resumes_from_checkpoint_step_after_recovery() {
        let root = unique_test_dir("roboclaw-gateway-checkpoint");
        let skills_dir = root.join("skills");
        let memory_dir = root.join("memory");
        let prompt_path = root.join("planner_prompt.txt");

        fs::create_dir_all(&skills_dir).expect("skills dir should be creatable");
        fs::write(
            &prompt_path,
            "Choose the best skill from the provided catalog.",
        )
        .expect("prompt file should be writable");
        write_recovery_skills(&skills_dir);

        let catalog = SkillCatalog::from_dir(&skills_dir).expect("catalog should load");
        let memory = Memory::new(&memory_dir).expect("memory should initialize");
        let planner =
            planner_for_provider(&prompt_path, LlmProvider::Mock).expect("planner should build");
        let ros2 = Ros2Bridge::mock("gateway-checkpoint-test");
        let backend: Arc<dyn RobotBackend> = Arc::new(GazeboBackend::with_ros2(ros2.clone()));

        let mut registry = ToolRegistry::new();
        registry.register_tool(SensorTool::default());
        registry.register_tool(SimulatorTool::new(backend.clone()));
        registry.register_tool(MotorControlTool::with_transient_failures(
            backend.clone(),
            "grasp",
            2,
        ));

        let agent = Agent::new(memory, planner, Executor::new(registry));
        let mut gateway =
            RoboclawGateway::with_max_replans(agent, catalog, ros2.clone(), backend, 1);

        let result = gateway
            .handle_instruction("Use the simulator to pick up the red cube and place it in bin_a.")
            .expect("gateway should resume from checkpoint after recovery");

        assert!(result.report.completed);
        assert_eq!(result.reports.len(), 3);
        assert_eq!(result.reports[0].failed_step.as_deref(), Some("grasp"));
        assert_eq!(result.reports[1].skill.name, "recover_grasp");
        assert_eq!(result.reports[2].skill.name, "pick_and_place");
        assert_eq!(
            result.reports[2].resumed_from_step.as_deref(),
            Some("move_to_object")
        );
        assert_eq!(result.reports[2].steps.len(), 3);
        assert_eq!(result.reports[2].steps[0].step_name, "move_to_object");

        let suggested_grasp_recovery_seen = gateway
            .ros2_bridge()
            .published_messages()
            .into_iter()
            .any(|message| {
                message.topic == ROBOCLAW_ACTION_TOPIC
                    && message.payload["data"]["suggested_recovery_skills"]
                        .as_array()
                        .map(|skills| {
                            skills
                                .iter()
                                .any(|skill| skill == &Value::String("recover_grasp".to_string()))
                        })
                        .unwrap_or(false)
            });
        assert!(suggested_grasp_recovery_seen);

        let resume_marker_seen =
            gateway
                .ros2_bridge()
                .published_messages()
                .into_iter()
                .any(|message| {
                    message.topic == ROBOCLAW_ACTION_TOPIC
                        && message.payload["data"]["resumed_from_step"]
                            == Value::String("move_to_object".to_string())
                });
        assert!(resume_marker_seen);
    }

    fn write_recovery_skills(skills_dir: &Path) {
        fs::write(
            skills_dir.join("pick_and_place.yaml"),
            r#"name: pick_and_place
description: pick up an object in simulation and place it into a bin
supports_checkpoint_resume: true
steps:
  - name: detect_object
    tool: sensor
    max_retries: 1
    resume_from_step: detect_object
    input:
      target: red_cube
    expect:
      detected: true
  - name: move_to_object
    tool: simulator
    resume_from_step: move_to_object
    input:
      action: move_to
      pose: table/front_left/pre_grasp
    expect:
      accepted: true
  - name: grasp
    tool: motor_control
    max_retries: 1
    resume_from_step: move_to_object
    input:
      action: grasp
      target: red_cube
    expect:
      accepted: true
  - name: place_object
    tool: simulator
    resume_from_step: place_object
    input:
      action: place
      target: red_cube
      location: bin_a
    expect:
      accepted: true
"#,
        )
        .expect("skill file should be writable");
        fs::write(
            skills_dir.join("recover_observation.yaml"),
            r#"name: recover_observation
description: recover from a failed observation by resetting pose and rescanning the target
resume_original_instruction: true
recovery_for:
  - failed_steps:
      - detect_object
    tools:
      - sensor
steps:
  - name: move_to_reobserve
    tool: simulator
    input:
      action: move_to
      pose: home
    expect:
      accepted: true
  - name: rescan_object
    tool: sensor
    input:
      target: red_cube
    expect:
      detected: true
"#,
        )
        .expect("recovery skill file should be writable");
        fs::write(
            skills_dir.join("recover_grasp.yaml"),
            r#"name: recover_grasp
description: recover from a failed grasp by returning to pre-grasp pose and verifying the target
resume_original_instruction: true
recovery_for:
  - failed_steps:
      - grasp
    tools:
      - motor_control
steps:
  - name: move_to_pre_grasp
    tool: simulator
    input:
      action: move_to
      pose: table/front_left/pre_grasp
    expect:
      accepted: true
  - name: verify_target_after_grasp_failure
    tool: sensor
    input:
      target: red_cube
    expect:
      detected: true
"#,
        )
        .expect("grasp recovery skill file should be writable");
    }

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be valid")
            .as_nanos();
        env::temp_dir().join(format!("{prefix}-{stamp}"))
    }
}
