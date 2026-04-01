use anyhow::Result;
use roboclaw_rs::agent::{planner_for_provider, planner_turn_debug, LlmProvider};
use roboclaw_rs::skills::SkillCatalog;
use serde_json::to_string_pretty;
use std::env;
use std::path::PathBuf;

fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let skill_dir = root.join("skills");
    let prompt_path = root.join("prompts/planner_prompt.txt");
    let options = input_options();
    let scenario = options.scenario.clone();
    let instruction = input_instruction(&scenario);
    let providers = options.providers.clone();
    let catalog = SkillCatalog::from_dir(&skill_dir)?;
    let debug = planner_turn_debug(&instruction, &catalog);

    println!("scenario={}", scenario);
    println!("instruction={}", instruction);
    println!("is_replan={}", debug.is_replan);
    println!("allowed_skills={}", debug.allowed_skills.join(","));
    println!(
        "matching_recovery_skills={}",
        if debug.matching_recovery_skills.is_empty() {
            "none".to_string()
        } else {
            debug.matching_recovery_skills.join(",")
        }
    );
    println!(
        "providers={}",
        providers
            .iter()
            .map(|provider| provider.as_str())
            .collect::<Vec<_>>()
            .join(",")
    );
    if options.debug || options.show_prompt {
        println!("planner_prompt_begin");
        println!("{}", debug.prompt);
        println!("planner_prompt_end");
    }
    if options.debug || options.show_schema {
        println!("planner_schema_begin");
        println!("{}", to_string_pretty(&debug.schema)?);
        println!("planner_schema_end");
    }

    for provider in providers {
        match run_provider(&prompt_path, &catalog, &instruction, provider) {
            Ok((selected_skill, reason)) => {
                println!(
                    "provider={} status=ok selected_skill={} reason={}",
                    provider.as_str(),
                    selected_skill,
                    sanitize_error(reason.as_deref().unwrap_or("none"))
                );
            }
            Err(error) => {
                println!(
                    "provider={} status=error detail={}",
                    provider.as_str(),
                    sanitize_error(&error.to_string())
                );
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct CompareOptions {
    scenario: String,
    providers: Vec<LlmProvider>,
    debug: bool,
    show_prompt: bool,
    show_schema: bool,
}

fn run_provider(
    prompt_path: &PathBuf,
    catalog: &SkillCatalog,
    instruction: &str,
    provider: LlmProvider,
) -> Result<(String, Option<String>)> {
    let planner = planner_for_provider(prompt_path, provider)?;
    let decision = planner.plan(instruction.to_string(), catalog)?;
    Ok((decision.skill.name, decision.reason))
}

fn input_instruction(scenario: &str) -> String {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut instruction_tokens = Vec::new();
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }

        if arg == "--providers" || arg == "--scenario" {
            skip_next = true;
            continue;
        }

        if arg == "--debug" || arg == "--show-prompt" || arg == "--show-schema" {
            continue;
        }

        if arg.starts_with("--providers=") || arg.starts_with("--scenario=") {
            continue;
        }

        instruction_tokens.push(arg);
    }

    if !instruction_tokens.is_empty() {
        return instruction_tokens.join(" ");
    }

    env::var("ROBOCLAW_INSTRUCTION")
        .ok()
        .unwrap_or_else(|| default_instruction_for_scenario(scenario))
}

fn input_options() -> CompareOptions {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut providers = None;
    let mut scenario = None;
    let mut debug = false;
    let mut show_prompt = false;
    let mut show_schema = false;

    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--providers" => {
                if let Some(value) = args.get(index + 1) {
                    providers = Some(parse_provider_list(value));
                    index += 1;
                }
            }
            "--scenario" => {
                if let Some(value) = args.get(index + 1) {
                    scenario = Some(value.clone());
                    index += 1;
                }
            }
            "--debug" => debug = true,
            "--show-prompt" => show_prompt = true,
            "--show-schema" => show_schema = true,
            other => {
                if let Some(value) = other.strip_prefix("--providers=") {
                    providers = Some(parse_provider_list(value));
                } else if let Some(value) = other.strip_prefix("--scenario=") {
                    scenario = Some(value.to_string());
                }
            }
        }
        index += 1;
    }

    if env_flag("ROBOCLAW_COMPARE_DEBUG") {
        debug = true;
    }
    if env_flag("ROBOCLAW_COMPARE_SHOW_PROMPT") {
        show_prompt = true;
    }
    if env_flag("ROBOCLAW_COMPARE_SHOW_SCHEMA") {
        show_schema = true;
    }

    CompareOptions {
        scenario: scenario
            .or_else(|| env::var("ROBOCLAW_COMPARE_SCENARIO").ok())
            .unwrap_or_else(|| "default".to_string()),
        providers: providers
            .or_else(|| {
                env::var("ROBOCLAW_COMPARE_PROVIDERS")
                    .ok()
                    .map(|value| parse_provider_list(&value))
            })
            .unwrap_or_else(|| vec![LlmProvider::Mock, LlmProvider::Local]),
        debug,
        show_prompt,
        show_schema,
    }
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

fn default_instruction_for_scenario(scenario: &str) -> String {
    match scenario.to_lowercase().as_str() {
        "recover-grasp" | "grasp-recovery" => "Original instruction:\nUse the simulator to pick up the red cube and place it in bin_a.\n\nPrevious execution failed.\nFailed step: grasp\nFailed tool: motor_control\nObservation: transient motor stall\nSuggested recovery skills: recover_grasp".to_string(),
        "recover-observation" | "observation-recovery" => "Original instruction:\nUse the simulator to pick up the red cube and place it in bin_a.\n\nPrevious execution failed.\nFailed step: detect_object\nFailed tool: sensor\nObservation: target not detected\nSuggested recovery skills: recover_observation".to_string(),
        "wave" => "Wave to acknowledge the operator.".to_string(),
        _ => "Use the simulator to pick up the red cube and place it in bin_a.".to_string(),
    }
}

fn parse_provider_list(raw: &str) -> Vec<LlmProvider> {
    raw.split(',')
        .filter_map(|item| parse_provider(item.trim()))
        .collect::<Vec<_>>()
}

fn parse_provider(raw: &str) -> Option<LlmProvider> {
    match raw.to_lowercase().as_str() {
        "mock" => Some(LlmProvider::Mock),
        "local" | "ollama" => Some(LlmProvider::Local),
        "openai" => Some(LlmProvider::OpenAi),
        "claude" | "anthropic" => Some(LlmProvider::Claude),
        _ => None,
    }
}

fn sanitize_error(error: &str) -> String {
    error.replace('\n', " | ")
}
