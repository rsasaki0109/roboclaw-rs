use anyhow::{Context, Result};
use roboclaw_rs::agent::{planner_for_provider, LlmProvider};
use roboclaw_rs::skills::SkillCatalog;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;
use std::time::Instant;

mod claude_provider;
mod local_provider;
mod mock_provider;
mod openai_provider;

use claude_provider::ClaudeProviderVariant;
use local_provider::LocalProviderVariant;
use mock_provider::MockProviderVariant;
use openai_provider::OpenAiProviderVariant;

const CASES_PATH: &str = "experiments/cross_provider_validation/cases.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCase {
    pub id: String,
    pub instruction: String,
    pub expected_skill: String,
    pub why: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProviderAvailability {
    Available,
    Unavailable,
    Disabled,
}

impl ProviderAvailability {
    fn as_str(self) -> &'static str {
        match self {
            ProviderAvailability::Available => "available",
            ProviderAvailability::Unavailable => "unavailable",
            ProviderAvailability::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderValidationDecision {
    pub selected_skill: Option<String>,
    pub reason: Option<String>,
    pub availability: ProviderAvailability,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderValidationCaseResult {
    pub case_id: String,
    pub expected_skill: String,
    pub selected_skill: Option<String>,
    pub correct: bool,
    pub availability: ProviderAvailability,
    pub latency_ms: Option<f64>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderValidationSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub provider_refs: usize,
    pub env_gate_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderValidationVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: ProviderValidationSourceMetrics,
    pub availability: ProviderAvailability,
    pub attempted_cases: usize,
    pub cases: Vec<ProviderValidationCaseResult>,
    pub accuracy_pct: f64,
    pub average_latency_ms: Option<f64>,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderValidationExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<ProviderCase>,
    pub variants: Vec<ProviderValidationVariantReport>,
}

pub trait ProviderValidationVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn provider(&self) -> LlmProvider;
    fn availability(&self, prompt_path: &Path) -> ProviderAvailability;
    fn evaluate(
        &self,
        case: &ProviderCase,
        prompt_path: &Path,
        catalog: &SkillCatalog,
    ) -> Result<ProviderValidationDecision>;
}

pub fn run_suite(root: &Path) -> Result<ProviderValidationExperimentReport> {
    let cases = load_cases(root)?;
    let prompt_path = root.join("prompts/planner_prompt.txt");
    let catalog = SkillCatalog::from_dir(root.join("skills"))?;
    let provider_names = ["mock", "local", "openai", "claude"];
    let variants: Vec<Box<dyn ProviderValidationVariant>> = vec![
        Box::new(MockProviderVariant::default()),
        Box::new(LocalProviderVariant::default()),
        Box::new(OpenAiProviderVariant::default()),
        Box::new(ClaudeProviderVariant::default()),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &provider_names)?;
        let availability = variant.availability(&prompt_path);
        let mut attempted_cases = 0usize;
        let mut correct = 0usize;
        let mut total_latency_ms = 0.0f64;
        let mut cases_results = Vec::new();

        for case in &cases {
            if availability != ProviderAvailability::Available {
                cases_results.push(ProviderValidationCaseResult {
                    case_id: case.id.clone(),
                    expected_skill: case.expected_skill.clone(),
                    selected_skill: None,
                    correct: false,
                    availability,
                    latency_ms: None,
                    detail: Some(format!(
                        "provider {} is {}",
                        variant.name(),
                        availability.as_str()
                    )),
                });
                continue;
            }

            let start = Instant::now();
            let decision = variant
                .evaluate(case, &prompt_path, &catalog)
                .with_context(|| {
                    format!(
                        "variant '{}' failed to evaluate case '{}'",
                        variant.name(),
                        case.id
                    )
                })?;
            let latency_ms = start.elapsed().as_secs_f64() * 1_000.0;
            let is_correct =
                decision.selected_skill.as_deref() == Some(case.expected_skill.as_str());
            attempted_cases += 1;
            if is_correct {
                correct += 1;
            }
            total_latency_ms += latency_ms;

            cases_results.push(ProviderValidationCaseResult {
                case_id: case.id.clone(),
                expected_skill: case.expected_skill.clone(),
                selected_skill: decision.selected_skill,
                correct: is_correct,
                availability: decision.availability,
                latency_ms: Some(latency_ms),
                detail: decision
                    .reason
                    .or(decision.detail)
                    .or_else(|| Some("provider returned no detail".to_string())),
            });
        }

        reports.push(ProviderValidationVariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            availability,
            attempted_cases,
            cases: cases_results,
            accuracy_pct: if attempted_cases == 0 {
                0.0
            } else {
                correct as f64 * 100.0 / attempted_cases as f64
            },
            average_latency_ms: if attempted_cases == 0 {
                None
            } else {
                Some(total_latency_ms / attempted_cases as f64)
            },
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
        });
    }

    Ok(ProviderValidationExperimentReport {
        problem: "Planner frontiers should be validated across real provider adapters instead of assuming provider-neutral behavior from mock-only comparisons.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &ProviderValidationExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} availability={} attempted_cases={} accuracy_pct={:.1} avg_latency_ms={} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.availability.as_str(),
            variant.attempted_cases,
            variant.accuracy_pct,
            variant
                .average_latency_ms
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "n/a".to_string()),
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &ProviderValidationExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Cross-Provider Validation\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("| case | expected skill | why |\n");
    markdown.push_str("| --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | {} |\n",
            case.id, case.expected_skill, case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str("| provider | availability | attempted cases | accuracy | avg latency ms | readability | extensibility |\n");
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | `{}` | {} | {:.1}% | {} | {} | {} |\n",
            variant.name,
            variant.availability.as_str(),
            variant.attempted_cases,
            variant.accuracy_pct,
            variant
                .average_latency_ms
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "n/a".to_string()),
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &ProviderValidationExperimentReport) -> String {
    let available = report
        .variants
        .iter()
        .filter(|variant| variant.availability == ProviderAvailability::Available)
        .collect::<Vec<_>>();
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Cross-Provider Validation\n\n");
    markdown.push_str(&format!(
        "- Available providers in this environment: {}.\n",
        if available.is_empty() {
            "none".to_string()
        } else {
            available
                .iter()
                .map(|variant| format!("`{}`", variant.name))
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    markdown.push_str(&format!(
        "- Frontier set among available providers: {}.\n",
        if frontier.is_empty() {
            "none".to_string()
        } else {
            frontier
        }
    ));
    if let Some(reference) = provisional_frontier(report) {
        markdown.push_str(&format!(
            "- Provisional reference: `{}` because it stayed on the frontier with the lowest average latency ({} ms).\n",
            reference.name,
            reference
                .average_latency_ms
                .map(|value| format!("{value:.2}"))
                .unwrap_or_else(|| "n/a".to_string())
        ));
    }
    markdown.push_str("- Keep live provider validation outside the stable runtime because availability, API keys, and local model state are environment-specific.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Cross-Provider Validation\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct ProviderCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub instruction: String,\n");
    markdown.push_str("    pub expected_skill: String,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ProviderValidationDecision {\n");
    markdown.push_str("    pub selected_skill: Option<String>,\n");
    markdown.push_str("    pub reason: Option<String>,\n");
    markdown.push_str("    pub availability: ProviderAvailability,\n");
    markdown.push_str("    pub detail: Option<String>,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait ProviderValidationVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str("    fn provider(&self) -> LlmProvider;\n");
    markdown.push_str("    fn availability(&self, prompt_path: &Path) -> ProviderAvailability;\n");
    markdown.push_str("    fn evaluate(&self, case: &ProviderCase, prompt_path: &Path, catalog: &SkillCatalog) -> anyhow::Result<ProviderValidationDecision>;\n");
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str("- Shared input: same planning instructions for every provider adapter.\n");
    markdown.push_str("- Shared metrics: attempted-case accuracy, single-call latency, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn remote_validation_enabled() -> bool {
    env::var("ROBOCLAW_PROVIDER_VALIDATION_REMOTE")
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

pub(crate) fn evaluate_with_provider(
    provider: LlmProvider,
    prompt_path: &Path,
    catalog: &SkillCatalog,
    instruction: &str,
) -> Result<ProviderValidationDecision> {
    let planner = planner_for_provider(prompt_path, provider)?;
    let decision = planner.plan(instruction.to_string(), catalog)?;
    Ok(ProviderValidationDecision {
        selected_skill: Some(decision.skill.name),
        reason: decision.reason,
        availability: ProviderAvailability::Available,
        detail: None,
    })
}

fn load_cases(root: &Path) -> Result<Vec<ProviderCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn collect_source_metrics(
    root: &Path,
    source_path: &str,
    provider_names: &[&str],
) -> Result<ProviderValidationSourceMetrics> {
    let path = root.join(source_path);
    let source = fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    let loc_non_empty = source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty() && !trimmed.starts_with("//")
        })
        .count();
    let helper_functions = source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ")
        })
        .count();
    let branch_tokens = ["if ", "match ", " for ", "while ", "&&", "||", "Err("]
        .into_iter()
        .map(|token| source.matches(token).count())
        .sum();
    let provider_refs = provider_names
        .iter()
        .map(|provider| source.matches(provider).count())
        .sum();
    let env_gate_refs = [
        "remote_validation_enabled(",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(ProviderValidationSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        provider_refs,
        env_gate_refs,
    })
}

fn readability_score(source: &ProviderValidationSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &ProviderValidationSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.provider_refs as i32 * 4) - (source.branch_tokens as i32 * 2)
            + (source.env_gate_refs as i32 * 6)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(
    report: &ProviderValidationExperimentReport,
) -> Vec<&ProviderValidationVariantReport> {
    let available = report
        .variants
        .iter()
        .filter(|variant| variant.availability == ProviderAvailability::Available)
        .collect::<Vec<_>>();
    let best_accuracy = available
        .iter()
        .map(|variant| variant.accuracy_pct)
        .fold(0.0f64, f64::max);
    available
        .into_iter()
        .filter(|variant| (variant.accuracy_pct - best_accuracy).abs() < f64::EPSILON)
        .collect()
}

fn provisional_frontier(
    report: &ProviderValidationExperimentReport,
) -> Option<&ProviderValidationVariantReport> {
    frontier_variants(report).into_iter().min_by(|left, right| {
        left.average_latency_ms
            .unwrap_or(f64::MAX)
            .partial_cmp(&right.average_latency_ms.unwrap_or(f64::MAX))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.readability_score.cmp(&right.readability_score))
    })
}
