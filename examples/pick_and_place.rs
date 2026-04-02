use anyhow::Result;
use roboclaw_rs::agent::{planner_from_env, Agent, Executor};
use roboclaw_rs::gateway::RoboclawGateway;
use roboclaw_rs::memory::Memory;
use roboclaw_rs::ros2::Ros2Bridge;
use roboclaw_rs::sim::{GazeboBackend, RobotBackend};
use roboclaw_rs::skills::SkillCatalog;
use roboclaw_rs::tools::{MotorControlTool, SensorTool, SimulatorTool, ToolRegistry};
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let skill_dir = root.join("skills");
    let prompt_path = root.join("prompts/planner_prompt.txt");
    let memory_dir = root.join("target/demo-memory");
    let instruction = input_instruction();

    let catalog = SkillCatalog::from_dir(&skill_dir)?;
    let memory = Memory::new(&memory_dir)?;
    let planner = planner_from_env(&prompt_path)?;
    let ros2 = Ros2Bridge::from_env("roboclaw_gateway")?;

    let backend: Arc<dyn RobotBackend> = Arc::new(GazeboBackend::with_ros2(ros2.clone()));
    let mut registry = ToolRegistry::new();
    registry.register_tool(SensorTool::default());
    registry.register_tool(SimulatorTool::new(backend.clone()));
    registry.register_tool(MotorControlTool::new(backend.clone()));

    let executor = Executor::new(registry);
    let agent = Agent::new(memory, planner, executor);
    let mut gateway = RoboclawGateway::new(agent, catalog, ros2, backend.clone());

    let result = gateway.handle_instruction(&instruction)?;
    if gateway.ros2_bridge().transport_name() == roboclaw_rs::ros2::RCLRS_TRANSPORT_NAME {
        std::thread::sleep(Duration::from_millis(250));
    }

    println!("instruction={}", instruction);
    println!("gateway_node={}", gateway.ros2_bridge().node_name());
    println!("ros2_transport={}", gateway.ros2_bridge().transport_name());
    println!("planner_provider={}", result.report.planner_provider);
    println!(
        "planner_reason={}",
        result.report.planner_reason.as_deref().unwrap_or("none")
    );
    println!("selected_skill={}", result.report.skill.name);
    println!("execution_attempts={}", result.reports.len());
    println!("replans={}", result.replans);
    println!("completed={}", result.report.completed);
    println!(
        "failed_step={}",
        result.report.failed_step.as_deref().unwrap_or("none")
    );
    println!("next_action={}", result.report.next_action);
    for (index, report) in result.reports.iter().enumerate() {
        println!(
            "execution_attempt={} skill={} resumed_from_step={} completed={} failed_step={} next_action={}",
            index + 1,
            report.skill.name,
            report.resumed_from_step.as_deref().unwrap_or("none"),
            report.completed,
            report.failed_step.as_deref().unwrap_or("none"),
            report.next_action
        );
    }
    println!("steps_executed={}", result.report.steps.len());
    for step in &result.report.steps {
        println!(
            "step={} tool={} status={} attempts={} observation={} output={}",
            step.step_name,
            step.tool_name,
            step.status.as_str(),
            step.attempts,
            step.observation,
            step.output
        );
    }

    println!("published_topics={:?}", gateway.topics());
    for message in gateway.ros2_bridge().published_messages() {
        println!("ros2 {} {}", message.topic, message.payload);
    }
    for message in gateway.ros2_bridge().received_messages() {
        println!("ros2_received {} {}", message.topic, message.payload);
    }

    println!("sim_state={:?}", result.backend_state);
    Ok(())
}

fn input_instruction() -> String {
    let cli_args = env::args().skip(1).collect::<Vec<_>>();
    if !cli_args.is_empty() {
        return cli_args.join(" ");
    }

    env::var("ROBOCLAW_INSTRUCTION").unwrap_or_else(|_| {
        "Use the simulator to pick up the red cube and place it in bin_a.".to_string()
    })
}
