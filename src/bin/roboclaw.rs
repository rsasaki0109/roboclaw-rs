use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use roboclaw_rs::agent::{
    planner_for_provider, planner_from_env, Agent, Executor, LlmProvider, PlanDecision, Planner,
};
use roboclaw_rs::gateway::RoboclawGateway;
use roboclaw_rs::memory::Memory;
use roboclaw_rs::ros2::Ros2Bridge;
use roboclaw_rs::sim::{GazeboBackend, RobotBackend};
use roboclaw_rs::skills::SkillCatalog;
use roboclaw_rs::tools::{MotorControlTool, SensorTool, SimulatorTool, ToolRegistry};
use serde::Serialize;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "roboclaw", version, about = "Inspect and run robot skills")]
struct Cli {
    /// Directory containing skills/ and prompts/.
    #[arg(long, global = true, default_value = ".")]
    project_dir: PathBuf,
    /// Print structured JSON instead of a human-readable summary.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Inspect the available YAML skills.
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
    /// Select a skill and show its steps without executing robot commands.
    Plan(InstructionArgs),
    /// Execute an instruction with the in-process simulator.
    Run {
        #[command(flatten)]
        args: InstructionArgs,
        /// Memory storage directory; defaults to PROJECT_DIR/target/cli-memory.
        #[arg(long)]
        memory_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum SkillsCommand {
    /// List skill names, descriptions, and step counts.
    List,
}

#[derive(Args)]
struct InstructionArgs {
    /// Instruction to plan or execute (quote instructions containing spaces).
    #[arg(value_parser = parse_instruction)]
    instruction: String,
    /// Planner provider; auto uses ROBOCLAW_LLM_PROVIDER and provider discovery.
    #[arg(long, value_enum, default_value = "mock")]
    provider: Provider,
}

#[derive(Clone, Copy, ValueEnum)]
enum Provider {
    Mock,
    Auto,
    Local,
    #[value(name = "openai")]
    OpenAi,
    Claude,
}

impl Provider {
    fn planner(self, prompt: &Path) -> Result<Box<dyn Planner>> {
        let provider = match self {
            Self::Auto => return planner_from_env(prompt),
            Self::Mock => LlmProvider::Mock,
            Self::Local => LlmProvider::Local,
            Self::OpenAi => LlmProvider::OpenAi,
            Self::Claude => LlmProvider::Claude,
        };
        planner_for_provider(prompt, provider)
    }
}

#[derive(Serialize)]
struct PlanOutput {
    instruction: String,
    planner_provider: &'static str,
    decision: PlanDecision,
}

fn parse_instruction(value: &str) -> std::result::Result<String, String> {
    if value.trim().is_empty() {
        Err("instruction must not be empty".to_string())
    } else {
        Ok(value.to_string())
    }
}

fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(status) => status,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn execute(cli: Cli) -> Result<ExitCode> {
    let catalog = SkillCatalog::from_dir(cli.project_dir.join("skills"))?;
    match cli.command {
        CliCommand::Skills {
            command: SkillsCommand::List,
        } => {
            let skills = catalog.values().collect::<Vec<_>>();
            if cli.json {
                print_json(&skills)?;
            } else {
                for skill in skills {
                    println!(
                        "{}\t{} steps\t{}",
                        skill.name,
                        skill.steps.len(),
                        skill.description
                    );
                }
            }
        }
        CliCommand::Plan(args) => {
            let planner = args
                .provider
                .planner(&cli.project_dir.join("prompts/planner_prompt.txt"))?;
            let output = PlanOutput {
                decision: planner.plan(args.instruction.clone(), &catalog)?,
                planner_provider: planner.provider_name(),
                instruction: args.instruction,
            };
            if cli.json {
                print_json(&output)?;
            } else {
                println!("planner_provider={}", output.planner_provider);
                println!("selected_skill={}", output.decision.skill.name);
                println!(
                    "planner_reason={}",
                    output.decision.reason.as_deref().unwrap_or("none")
                );
                for (index, step) in output.decision.skill.steps.iter().enumerate() {
                    println!(
                        "{}. {} ({}) input={}",
                        index + 1,
                        step.name,
                        step.tool,
                        step.input
                    );
                }
            }
        }
        CliCommand::Run { args, memory_dir } => {
            let planner = args
                .provider
                .planner(&cli.project_dir.join("prompts/planner_prompt.txt"))?;
            let ros2 = Ros2Bridge::from_env("roboclaw_cli")?;
            let memory = Memory::new(
                memory_dir.unwrap_or_else(|| cli.project_dir.join("target/cli-memory")),
            )?;
            let backend: Arc<dyn RobotBackend> = Arc::new(GazeboBackend::with_ros2(ros2.clone()));
            let mut registry = ToolRegistry::new();
            registry.register_tool(SensorTool::default());
            registry.register_tool(SimulatorTool::new(backend.clone()));
            registry.register_tool(MotorControlTool::new(backend.clone()));
            let agent = Agent::new(memory, planner, Executor::new(registry));
            let mut gateway = RoboclawGateway::new(agent, catalog, ros2, backend);
            let result = gateway.handle_instruction(&args.instruction)?;
            if cli.json {
                print_json(&result)?;
            } else {
                println!("planner_provider={}", result.report.planner_provider);
                println!("selected_skill={}", result.report.skill.name);
                println!("completed={}", result.report.completed);
                println!("execution_attempts={}", result.reports.len());
                println!("replans={}", result.replans);
                println!(
                    "failed_step={}",
                    result.report.failed_step.as_deref().unwrap_or("none")
                );
                println!("next_action={}", result.report.next_action);
                println!("last_pose={}", result.backend_state.last_pose);
                println!(
                    "held_object={}",
                    result
                        .backend_state
                        .held_object
                        .as_deref()
                        .unwrap_or("none")
                );
            }
            if !result.report.completed {
                return Ok(ExitCode::FAILURE);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn print_json(value: &impl Serialize) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}
