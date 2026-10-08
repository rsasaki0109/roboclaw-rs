use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use roboclaw_rs::agent::ExecutionStatus;
use roboclaw_rs::config::{parse_duration, Config, BUILTIN_TOOLS};
use roboclaw_rs::jobs::Jobs;
use roboclaw_rs::memory::{atomic_write, EventObserver, Memory};
use roboclaw_rs::runtime::{RunOutput, RunRequest, Workspace};
use roboclaw_rs::storage::now_millis;
use roboclaw_rs::tools::ExecutionControl;
use roboclaw_rs::webhooks::Webhooks;
use serde::Serialize;
use serde_json::json;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "roboclaw",
    version,
    about = "Plan, run and inspect robot workspaces"
)]
struct Cli {
    /// Directory containing skills/, prompts/ and optional roboclaw.yaml.
    #[arg(long, global = true, default_value = ".")]
    project_dir: PathBuf,
    /// Explicit YAML configuration path; defaults to PROJECT_DIR/roboclaw.yaml.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Print structured JSON instead of a human-readable summary.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Inspect and validate YAML skills.
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
    /// Select a skill without initializing a robot backend or memory.
    Plan(InstructionArgs),
    /// Execute an instruction with the in-process simulator.
    Run {
        #[command(flatten)]
        args: InstructionArgs,
        #[arg(long, default_value = "main")]
        session: String,
        /// Override memory storage (main defaults to PROJECT_DIR/target/cli-memory).
        #[arg(long)]
        memory_dir: Option<PathBuf>,
        /// Shared execution budget, e.g. 30s or 2m.
        #[arg(long, value_parser = parse_timeout)]
        timeout: Option<Duration>,
        /// Stream newline-delimited JSON events and a final result.
        #[arg(long, conflicts_with = "json")]
        stream: bool,
    },
    /// Inspect built-in tools and their effective policy.
    Tools {
        #[command(subcommand)]
        command: ToolsCommand,
    },
    /// Inspect or initialize workspace configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Check configuration, assets, credentials and stored state without robot actions.
    Doctor,
    /// Inspect persistent sessions and their run history.
    Sessions {
        #[command(subcommand)]
        command: SessionsCommand,
    },
    /// Inspect runs and their structured event traces.
    Runs {
        #[command(subcommand)]
        command: RunsCommand,
    },
    /// Search or inspect a session's local memory.
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    /// Submit and manage persistent one-time, interval or cron jobs.
    Jobs {
        #[command(subcommand)]
        command: JobsCommand,
    },
    /// Inspect and dispatch durable run notifications without robot actions.
    Webhooks {
        #[command(subcommand)]
        command: WebhooksCommand,
    },
    /// Start the local control API, dashboard and job runner.
    Gateway {
        #[command(subcommand)]
        command: GatewayCommand,
    },
}

#[derive(Subcommand)]
enum SkillsCommand {
    List,
    Show { name: String },
    Validate,
}
#[derive(Subcommand)]
enum ToolsCommand {
    List,
}
#[derive(Subcommand)]
enum ConfigCommand {
    Init,
    Show,
    Check,
}
#[derive(Subcommand)]
enum SessionsCommand {
    List,
    Show { id: String },
}
#[derive(Subcommand)]
enum RunsCommand {
    List,
    Show {
        id: String,
    },
    Events {
        id: String,
        #[arg(long, default_value_t = 0)]
        after: usize,
    },
}
#[derive(Subcommand)]
enum MemoryCommand {
    Remember {
        note: String,
        #[arg(long, default_value = "main")]
        session: String,
    },
    Search {
        query: String,
        #[arg(long, default_value = "main")]
        session: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Show {
        #[arg(long, default_value = "main")]
        session: String,
    },
}
#[derive(Subcommand)]
enum JobsCommand {
    Add {
        #[command(flatten)]
        args: InstructionArgs,
        /// Share an existing session; otherwise create an isolated job session.
        #[arg(long)]
        session: Option<String>,
        /// Delay before the first run, e.g. 10m.
        #[arg(long, value_parser = parse_timeout)]
        after: Option<Duration>,
        /// Repeat successful runs at this interval. Failed or cancelled jobs stop.
        #[arg(long, value_parser = parse_timeout)]
        every: Option<Duration>,
        /// Five-field calendar schedule, e.g. "0 9 * * MON-FRI".
        #[arg(long, conflicts_with_all = ["after", "every"])]
        cron: Option<String>,
        /// IANA timezone for --cron (default UTC), e.g. Asia/Tokyo.
        #[arg(long, requires = "cron")]
        timezone: Option<String>,
        #[arg(long, value_parser = parse_timeout)]
        timeout: Option<Duration>,
    },
    List,
    Show {
        id: String,
    },
    Cancel {
        id: String,
    },
    /// Run currently due jobs once; requires that no gateway runner is active.
    Run {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
}
#[derive(Subcommand)]
enum GatewayCommand {
    Serve {
        #[arg(long, default_value = "127.0.0.1:18790")]
        bind: SocketAddr,
    },
}

#[derive(Subcommand)]
enum WebhooksCommand {
    List,
    Show {
        id: String,
    },
    /// Requeue a failed notification; dispatch separately or keep a gateway active.
    Retry {
        id: String,
    },
    /// Send due notifications once; failed HTTP requests never re-run the robot.
    Dispatch {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
}

#[derive(Args)]
struct InstructionArgs {
    #[arg(value_parser = parse_instruction)]
    instruction: String,
    /// Explicit provider selections use no configured fallback unless requested.
    #[arg(long, value_enum)]
    provider: Option<Provider>,
    /// Opt in to fallback providers, in order (repeat or comma-separate).
    #[arg(
        long = "fallback",
        value_enum,
        value_delimiter = ',',
        conflicts_with = "no_fallbacks"
    )]
    fallbacks: Vec<Provider>,
    /// Disable configured provider fallback.
    #[arg(long)]
    no_fallbacks: bool,
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
    fn name(self) -> &'static str {
        match self {
            Self::Mock => "mock",
            Self::Auto => "auto",
            Self::Local => "local",
            Self::OpenAi => "openai",
            Self::Claude => "claude",
        }
    }
}
fn request(args: InstructionArgs, session: String) -> RunRequest {
    RunRequest {
        instruction: args.instruction,
        session,
        provider: args.provider.map(|provider| provider.name().into()),
        fallbacks: if args.no_fallbacks || !args.fallbacks.is_empty() {
            Some(
                args.fallbacks
                    .into_iter()
                    .map(|provider| provider.name().into())
                    .collect(),
            )
        } else {
            None
        },
        timeout: None,
    }
}
fn parse_instruction(value: &str) -> std::result::Result<String, String> {
    if value.trim().is_empty() || value.len() > 16_384 {
        Err("instruction must contain 1–16384 bytes".into())
    } else {
        Ok(value.into())
    }
}
fn parse_timeout(value: &str) -> std::result::Result<Duration, String> {
    parse_duration(value).map_err(|error| error.to_string())
}
fn signal_control(control: &ExecutionControl) -> Result<()> {
    let control = control.clone();
    ctrlc::set_handler(move || control.cancel())?;
    Ok(())
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
    if matches!(&cli.command, CliCommand::Doctor) {
        let report = roboclaw_rs::doctor::diagnose(&cli.project_dir, cli.config.as_deref());
        if cli.json {
            print_json(&report)?;
        } else {
            for check in &report.checks {
                println!(
                    "{} {}: {}",
                    if check.ok { "ok" } else { "error" },
                    check.name,
                    check.detail
                );
            }
        }
        return Ok(if report.ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    if matches!(
        &cli.command,
        CliCommand::Config {
            command: ConfigCommand::Init
        }
    ) {
        let path = cli
            .config
            .unwrap_or_else(|| cli.project_dir.join("roboclaw.yaml"));
        if path.exists() {
            bail!("config already exists: {:?}", path);
        }
        atomic_write(&path, serde_yaml::to_string(&Config::default())?.as_bytes())?;
        output(&json!({"created": path}), cli.json)?;
        return Ok(ExitCode::SUCCESS);
    }
    let workspace = Workspace::load(&cli.project_dir, cli.config.as_deref())?;
    match cli.command {
        CliCommand::Skills { command } => {
            let catalog = workspace.catalog()?;
            match command {
                SkillsCommand::List => {
                    let skills = catalog.values().collect::<Vec<_>>();
                    if cli.json { print_json(&skills)?; } else { for skill in skills { println!("{}\t{} steps\t{}", skill.name, skill.steps.len(), skill.description); } }
                }
                SkillsCommand::Show { name } => output(catalog.get(&name).with_context(|| format!("unknown skill '{name}'"))?, cli.json)?,
                SkillsCommand::Validate => { let catalog = workspace.validate_skills()?; output(&json!({"valid": true, "skills": catalog.names()}), cli.json)?; }
            }
        }
        CliCommand::Tools { command: ToolsCommand::List } => output(&BUILTIN_TOOLS.iter().map(|name| json!({"name": name, "allowed": workspace.config.tools.check(name).is_ok()})).collect::<Vec<_>>(), cli.json)?,
        CliCommand::Config { command } => match command {
            ConfigCommand::Show => output(&workspace.config, cli.json)?,
            ConfigCommand::Check => output(&json!({"valid": true}), cli.json)?,
            ConfigCommand::Init => unreachable!(),
        },
        CliCommand::Plan(args) => {
            let result = workspace.plan(request(args, "main".into()))?;
            if cli.json { print_json(&result)?; } else {
                println!("planner_provider={}", result.planner_provider);
                println!("selected_skill={}", result.decision.skill.name);
                println!("planner_reason={}", result.decision.reason.as_deref().unwrap_or("none"));
                for (index, step) in result.decision.skill.steps.iter().enumerate() { println!("{}. {} ({}) input={}", index + 1, step.name, step.tool, step.input); }
            }
        }
        CliCommand::Run { args, session, memory_dir, timeout, stream } => {
            let mut request = request(args, session);
            request.timeout = timeout.map(|timeout| humantime::format_duration(timeout).to_string());
            let control = workspace.control(&request)?;
            signal_control(&control)?;
            let observer: Option<EventObserver> = stream.then(|| Arc::new(|event: &roboclaw_rs::memory::Event| print_line(&json!({"type": "event", "event": event}))) as EventObserver);
            let execution = workspace.run(request, &control, observer, memory_dir.as_deref());
            if stream {
                match &execution { Ok(result) => print_line(&json!({"type": "result", "result": result}))?, Err(error) => print_line(&json!({"type": "error", "error": format!("{error:#}")}))? }
            }
            let result = execution?;
            if !stream { if cli.json { print_json(&result)?; } else { print_run(&result); } }
            return Ok(exit_status(result.execution.status));
        }
        CliCommand::Sessions { command } => match command {
            SessionsCommand::List => output(&workspace.store.sessions()?, cli.json)?,
            SessionsCommand::Show { id } => { let session = workspace.store.session(&id)?; let runs = workspace.store.runs()?.into_iter().filter(|run| run.session == id).collect::<Vec<_>>(); output(&json!({"session": session, "runs": runs}), cli.json)?; }
        },
        CliCommand::Runs { command } => match command {
            RunsCommand::List => output(&workspace.store.runs()?, cli.json)?,
            RunsCommand::Show { id } => output(&workspace.store.run(&id)?, cli.json)?,
            RunsCommand::Events { id, after } => output(&workspace.store.events(&id, after, 1000)?, cli.json)?,
        },
        CliCommand::Memory { command } => match command {
            MemoryCommand::Remember { note, session } => { workspace.remember_note(&session, &note)?; output(&json!({"remembered": true, "session": session}), cli.json)?; }
            MemoryCommand::Search { query, session, limit } => { let memory = Memory::open_readonly(workspace.memory_path(&session)?)?; output(&memory.search(&query, limit.min(100)), cli.json)?; }
            MemoryCommand::Show { session } => { let memory = Memory::open_readonly(workspace.memory_path(&session)?)?; output(&json!({"events": memory.short_term, "logs": memory.long_term}), cli.json)?; }
        },
        CliCommand::Jobs { command } => {
            let jobs = Jobs { workspace: workspace.clone() };
            match command {
                JobsCommand::Add { args, session, after, every, cron, timezone, timeout } => {
                    let isolated = session.is_none();
                    let mut request = request(args, session.unwrap_or_else(|| "main".into()));
                    request.timeout = timeout.map(|timeout| humantime::format_duration(timeout).to_string());
                    let job = if let Some(expression) = cron {
                        jobs.add_cron(request, &expression, timezone.as_deref().unwrap_or("UTC"), isolated)?
                    } else {
                        let delay = u64::try_from(after.unwrap_or_default().as_millis())?;
                        let due = now_millis().checked_add(delay).context("job deadline overflow")?;
                        jobs.add(request, due, every, isolated)?
                    };
                    output(&job, cli.json)?;
                }
                JobsCommand::List => output(&jobs.list()?, cli.json)?,
                JobsCommand::Show { id } => output(&jobs.get(&id)?, cli.json)?,
                JobsCommand::Cancel { id } => output(&jobs.cancel(&id)?, cli.json)?,
                JobsCommand::Run { limit } => {
                    if limit == 0 || limit > 1000 { bail!("job limit must be between 1 and 1000"); }
                    let shutdown = ExecutionControl::default(); signal_control(&shutdown)?;
                    let _lease = jobs.runner_lease()?;
                    jobs.recover_interrupted()?;
                    let mut outcomes = Vec::new();
                    let mut processed = Vec::new();
                    for _ in 0..limit {
                        if shutdown.stop_reason().is_some() { break; }
                        let Some(job) = jobs.claim_due_excluding(now_millis(), &processed)? else { break; };
                        processed.push(job.id.clone());
                        outcomes.push(jobs.execute(job, &shutdown, None)?);
                    }
                    let failed = outcomes.iter().any(|job| job.last_run_status.as_deref() != Some("completed"));
                    output(&outcomes, cli.json)?;
                    if shutdown.stop_reason().is_some() { return Ok(ExitCode::from(130)); }
                    if failed { return Ok(ExitCode::FAILURE); }
                }
            }
        }
        CliCommand::Webhooks { command } => {
            let webhooks = Webhooks { workspace: workspace.clone() };
            match command {
                WebhooksCommand::List => output(&webhooks.list()?, cli.json)?,
                WebhooksCommand::Show { id } => output(&webhooks.get(&id)?, cli.json)?,
                WebhooksCommand::Retry { id } => output(&webhooks.retry(&id)?, cli.json)?,
                WebhooksCommand::Dispatch { limit } => {
                    let shutdown = ExecutionControl::default(); signal_control(&shutdown)?;
                    let outcomes = webhooks.dispatch(limit, &shutdown)?;
                    let failed = outcomes.iter().any(|delivery| delivery.status != "delivered");
                    output(&outcomes, cli.json)?;
                    if shutdown.stop_reason().is_some() { return Ok(ExitCode::from(130)); }
                    if failed { return Ok(ExitCode::FAILURE); }
                }
            }
        },
        CliCommand::Gateway { command: GatewayCommand::Serve { bind } } => {
            let token = std::env::var("ROBOCLAW_GATEWAY_TOKEN").context("set ROBOCLAW_GATEWAY_TOKEN to start the gateway")?;
            let shutdown = ExecutionControl::default(); signal_control(&shutdown)?;
            roboclaw_rs::server::serve(workspace, bind, token, shutdown)?;
        }
        CliCommand::Doctor => unreachable!(),
    }
    Ok(ExitCode::SUCCESS)
}
fn exit_status(status: ExecutionStatus) -> ExitCode {
    match status {
        ExecutionStatus::Completed => ExitCode::SUCCESS,
        ExecutionStatus::Failed => ExitCode::FAILURE,
        ExecutionStatus::TimedOut => ExitCode::from(124),
        ExecutionStatus::Cancelled => ExitCode::from(130),
    }
}
fn print_run(result: &RunOutput) {
    let execution = &result.execution;
    println!("run_id={}", result.run_id);
    println!("session={}", result.session);
    println!("status={}", execution.status.as_str());
    println!(
        "completed={}",
        execution.status == ExecutionStatus::Completed
    );
    println!("execution_attempts={}", execution.reports.len());
    println!("replans={}", execution.replans);
    if let Some(report) = &execution.report {
        println!("planner_provider={}", report.planner_provider);
        println!("selected_skill={}", report.skill.name);
        println!(
            "failed_step={}",
            report.failed_step.as_deref().unwrap_or("none")
        );
    }
    println!(
        "next_action={}",
        if execution.status.is_stopped() {
            "stop_execution"
        } else {
            execution
                .report
                .as_ref()
                .map(|report| report.next_action.as_str())
                .unwrap_or("idle")
        }
    );
    println!("last_pose={}", execution.backend_state.last_pose);
    println!(
        "held_object={}",
        execution
            .backend_state
            .held_object
            .as_deref()
            .unwrap_or("none")
    );
}
fn output(value: &impl Serialize, json: bool) -> Result<()> {
    if json {
        print_json(value)
    } else {
        println!("{}", serde_json::to_string_pretty(value)?);
        Ok(())
    }
}
fn print_json(value: &impl Serialize) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(())
}
fn print_line(value: &impl Serialize) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, value)?;
    writeln!(stdout)?;
    stdout.flush()?;
    Ok(())
}
