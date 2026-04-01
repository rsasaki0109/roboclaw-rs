use anyhow::{Context, Result};
use roboclaw_rs::skills::{RecoveryContext, SkillCatalog};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod catalog_freeform;
mod mirrored_constraints;
mod prompt_constrained;
mod schema_constrained;

use catalog_freeform::CatalogFreeformVariant;
use mirrored_constraints::MirroredConstraintsVariant;
use prompt_constrained::PromptConstrainedVariant;
use schema_constrained::SchemaConstrainedVariant;

const CASES_PATH: &str = "experiments/planner_prompt_shaping/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptShapeCase {
    pub id: String,
    pub user_goal: String,
    pub failed_step: Option<String>,
    pub failed_tool: Option<String>,
    pub observation: Option<String>,
    pub expected_skill: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArtifact {
    pub prompt: String,
    pub schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptShapeDecision {
    pub selected_skill: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptShapeCaseResult {
    pub case_id: String,
    pub correct: bool,
    pub expected_skill: String,
    pub selected_skill: String,
    pub prompt_chars: usize,
    pub constraint_signals: usize,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptShapeSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_phrase_refs: usize,
    pub prompt_builder_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptShapeVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: PromptShapeSourceMetrics,
    pub cases: Vec<PromptShapeCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_prompt_chars: f64,
    pub average_constraint_signals: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptShapeExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<PromptShapeCase>,
    pub variants: Vec<PromptShapeVariantReport>,
}

pub trait PromptShapeVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn build(&self, case: &PromptShapeCase, catalog: &SkillCatalog) -> Result<PromptArtifact>;
}

pub fn run_suite(root: &Path) -> Result<PromptShapeExperimentReport> {
    let cases = load_cases(root)?;
    let catalog = SkillCatalog::from_dir(root.join("skills"))?;
    let prompt_phrases = vec![
        "Allowed skills for this turn".to_string(),
        "matching_recovery_skills".to_string(),
        "Decision rules".to_string(),
        "failed_step".to_string(),
        "failed_tool".to_string(),
        "Instruction".to_string(),
    ];
    let variants: Vec<Box<dyn PromptShapeVariant>> = vec![
        Box::new(CatalogFreeformVariant::default()),
        Box::new(PromptConstrainedVariant::default()),
        Box::new(SchemaConstrainedVariant::default()),
        Box::new(MirroredConstraintsVariant::default()),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &prompt_phrases)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;
        let mut total_prompt_chars = 0usize;
        let mut total_constraint_signals = 0usize;

        for case in &cases {
            let artifact = variant.build(case, &catalog).with_context(|| {
                format!("variant '{}' failed on case '{}'", variant.name(), case.id)
            })?;
            let prompt_chars = artifact.prompt.len() + artifact.schema.to_string().len();
            let constraint_signals = constraint_signal_count(&artifact);
            let decision = simulate_selection(&artifact, &catalog)?;
            let bench_average_us = benchmark_variant(variant.as_ref(), case, &catalog)?;
            let is_correct = decision.selected_skill == case.expected_skill;
            if is_correct {
                correct += 1;
            }
            total_average_us += bench_average_us;
            total_prompt_chars += prompt_chars;
            total_constraint_signals += constraint_signals;

            case_results.push(PromptShapeCaseResult {
                case_id: case.id.clone(),
                correct: is_correct,
                expected_skill: case.expected_skill.clone(),
                selected_skill: decision.selected_skill,
                prompt_chars,
                constraint_signals,
                average_decision_us: bench_average_us,
            });
        }

        reports.push(PromptShapeVariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            cases: case_results,
            accuracy_pct: correct as f64 * 100.0 / cases.len() as f64,
            average_decision_us: total_average_us / cases.len() as f64,
            average_prompt_chars: total_prompt_chars as f64 / cases.len() as f64,
            average_constraint_signals: total_constraint_signals as f64 / cases.len() as f64,
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
        });
    }

    Ok(PromptShapeExperimentReport {
        problem: "Planner prompt shaping should evolve through comparable prompt/schema variants instead of freezing one instruction wrapper before provider behavior is understood.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &PromptShapeExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} accuracy_pct={:.1} avg_decision_us={:.2} avg_prompt_chars={:.2} avg_constraint_signals={:.2} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_prompt_chars,
            variant.average_constraint_signals,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &PromptShapeExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Planner Prompt Shaping\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("| case | expected skill | recovery turn | why |\n");
    markdown.push_str("| --- | --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} |\n",
            case.id,
            case.expected_skill,
            case.failed_step.is_some(),
            case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str(
        "| variant | style | accuracy | avg us | avg prompt chars | avg constraint signals | readability | extensibility |\n",
    );
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | {} | {:.1}% | {:.2} | {:.2} | {:.2} | {} | {} |\n",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_prompt_chars,
            variant.average_constraint_signals,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &PromptShapeExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Planner Prompt Shaping\n\n");
    markdown.push_str(&format!(
        "- Frontier set: {}.\n",
        if frontier.is_empty() {
            "none".to_string()
        } else {
            frontier
        }
    ));
    if let Some(reference) = provisional_frontier(report) {
        markdown.push_str(&format!(
            "- Provisional reference: `{}` because it stayed on the frontier while keeping prompt size bounded ({:.2} chars).\n",
            reference.name, reference.average_prompt_chars
        ));
    }
    markdown.push_str("- Keep prompt shaping outside the stable planner path until provider comparisons confirm which constraints need to live in prompt text versus schema.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Planner Prompt Shaping\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct PromptShapeCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub user_goal: String,\n");
    markdown.push_str("    pub failed_step: Option<String>,\n");
    markdown.push_str("    pub failed_tool: Option<String>,\n");
    markdown.push_str("    pub observation: Option<String>,\n");
    markdown.push_str("    pub expected_skill: String,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct PromptArtifact {\n");
    markdown.push_str("    pub prompt: String,\n");
    markdown.push_str("    pub schema: serde_json::Value,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait PromptShapeVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str("    fn build(&self, case: &PromptShapeCase, catalog: &SkillCatalog) -> anyhow::Result<PromptArtifact>;\n");
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str(
        "- Shared input: same user goal and failure context for all prompt-shaping variants.\n",
    );
    markdown.push_str("- Shared metrics: accuracy under a constrained surrogate selector, average prompt size, average constraint signals, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn recovery_context(case: &PromptShapeCase) -> RecoveryContext {
    RecoveryContext {
        failed_step: case.failed_step.clone(),
        tool: case.failed_tool.clone(),
        observation: case.observation.clone(),
    }
}

pub(crate) fn allowed_skills(case: &PromptShapeCase, catalog: &SkillCatalog) -> Vec<String> {
    let matching = catalog.recovery_candidate_names(&recovery_context(case));
    if matching.is_empty() {
        catalog.names()
    } else {
        matching
    }
}

pub(crate) fn catalog_summary(catalog: &SkillCatalog) -> String {
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
                recovery
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn base_instruction(case: &PromptShapeCase) -> String {
    if case.failed_step.is_none() {
        case.user_goal.clone()
    } else {
        format!("Original instruction:\n{}", case.user_goal)
    }
}

pub(crate) fn structured_recovery_guidance(
    case: &PromptShapeCase,
    catalog: &SkillCatalog,
) -> Option<String> {
    let matching = catalog.recovery_candidate_names(&recovery_context(case));
    if case.failed_step.is_none() && case.failed_tool.is_none() && case.observation.is_none() {
        return None;
    }

    Some(format!(
        "failed_step: {}\nfailed_tool: {}\nobservation: {}\nmatching_recovery_skills: {}",
        case.failed_step.as_deref().unwrap_or("unknown"),
        case.failed_tool.as_deref().unwrap_or("unknown"),
        case.observation.as_deref().unwrap_or("unknown"),
        if matching.is_empty() {
            "none".to_string()
        } else {
            matching.join(", ")
        }
    ))
}

pub(crate) fn full_schema(
    allowed: &[String],
    catalog: &SkillCatalog,
    include_allowed_description: bool,
) -> Value {
    let description = if include_allowed_description {
        format!(
            "Select exactly one skill. Allowed skills for this turn: {}. Catalog summary:\n{}",
            allowed.join(", "),
            catalog_summary(catalog)
        )
    } else {
        "Select exactly one skill.".to_string()
    };

    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "skill": {
                "type": "string",
                "enum": allowed,
                "description": description
            }
        },
        "required": ["skill"]
    })
}

fn simulate_selection(
    artifact: &PromptArtifact,
    catalog: &SkillCatalog,
) -> Result<PromptShapeDecision> {
    let recovery_text = parse_recovery_text(&artifact.prompt).to_lowercase();
    let schema_text = artifact.schema.to_string().to_lowercase();
    let schema_candidates = artifact.schema["properties"]["skill"]["enum"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|items| !items.is_empty())
        .unwrap_or_else(|| catalog.names());
    let prompt_allowed = parse_csv_field(&artifact.prompt, "Allowed skills for this turn:");
    let matching_recovery = parse_csv_field(&artifact.prompt, "matching_recovery_skills:");
    let failed_step = parse_scalar_field(&artifact.prompt, "failed_step:");
    let failed_tool = parse_scalar_field(&artifact.prompt, "failed_tool:");
    let instruction_text = parse_instruction_text(&artifact.prompt).to_lowercase();

    let mut best = None::<(String, i32)>;
    for candidate in schema_candidates {
        let mut score = 0i32;
        if schema_text.contains(&format!("\"{}\"", candidate.to_lowercase()))
            && artifact.schema["properties"]["skill"]["enum"]
                .as_array()
                .map(|items| items.len())
                == Some(1)
        {
            score += 10;
        }
        if prompt_allowed.iter().any(|item| item == &candidate) {
            score += 8;
        }
        if matching_recovery.iter().any(|item| item == &candidate) {
            score += 10;
        }
        if schema_text.contains(&format!(
            "allowed skills for this turn: {}",
            candidate.to_lowercase()
        )) {
            score += 5;
        }
        if artifact
            .prompt
            .to_lowercase()
            .contains(&format!("choose {}", candidate))
        {
            score += 6;
        }

        match candidate.as_str() {
            "pick_and_place" => {
                if contains_any(
                    &instruction_text,
                    &["pick", "place", "transport", "cube", "bin"],
                ) {
                    score += 4;
                }
            }
            "wave_arm" => {
                if contains_any(
                    &instruction_text,
                    &["wave", "acknowledge", "greet", "gesture"],
                ) {
                    score += 4;
                }
            }
            "recover_grasp" => {
                if failed_step.as_deref() == Some("grasp")
                    || failed_tool.as_deref() == Some("motor_control")
                {
                    score += 6;
                }
                if contains_any(
                    &recovery_text,
                    &["motor stall", "grasp failure", "pre-grasp"],
                ) {
                    score += 3;
                }
            }
            "recover_observation" => {
                if failed_step.as_deref() == Some("detect_object")
                    || failed_tool.as_deref() == Some("sensor")
                {
                    score += 6;
                }
                if contains_any(
                    &recovery_text,
                    &["target not detected", "rescan", "observation failure"],
                ) {
                    score += 3;
                }
            }
            _ => {}
        }

        if !prompt_allowed.is_empty() && !prompt_allowed.iter().any(|item| item == &candidate) {
            score -= 4;
        }

        if best
            .as_ref()
            .map(|(_, best_score)| score > *best_score)
            .unwrap_or(true)
        {
            best = Some((candidate, score));
        }
    }

    let (selected_skill, score) =
        best.ok_or_else(|| anyhow::anyhow!("no prompt-shaping candidates available"))?;
    Ok(PromptShapeDecision {
        rationale: format!("surrogate score={score}"),
        selected_skill,
    })
}

fn parse_recovery_text(prompt: &str) -> String {
    if let Some(start) = prompt.find("Recovery guidance:\n") {
        return prompt[start + "Recovery guidance:\n".len()..]
            .split("\n\n")
            .next()
            .unwrap_or_default()
            .to_string();
    }
    if let Some(start) = prompt.find("Recovery context:\n") {
        return prompt[start + "Recovery context:\n".len()..]
            .split("\n\n")
            .next()
            .unwrap_or_default()
            .to_string();
    }
    String::new()
}

fn parse_instruction_text(prompt: &str) -> String {
    if let Some(start) = prompt.find("Instruction:\n") {
        return prompt[start + "Instruction:\n".len()..]
            .split("\n\n")
            .next()
            .unwrap_or_default()
            .to_string();
    }
    if let Some(start) = prompt.find("Task:\n") {
        return prompt[start + "Task:\n".len()..]
            .split("\n\n")
            .next()
            .unwrap_or_default()
            .to_string();
    }
    prompt.to_string()
}

fn parse_csv_field(prompt: &str, label: &str) -> Vec<String> {
    parse_scalar_field(prompt, label)
        .map(|value| {
            value
                .split(',')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty() && item != "none")
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_scalar_field(prompt: &str, label: &str) -> Option<String> {
    prompt
        .lines()
        .find_map(|line| line.trim().strip_prefix(label).map(str::trim))
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn constraint_signal_count(artifact: &PromptArtifact) -> usize {
    let prompt = artifact.prompt.to_lowercase();
    let schema = artifact.schema.to_string().to_lowercase();

    [
        prompt.contains("allowed skills for this turn"),
        prompt.contains("matching_recovery_skills"),
        prompt.contains("decision rules"),
        artifact.schema["properties"]["skill"]["enum"]
            .as_array()
            .map(|items| items.len() < 4)
            .unwrap_or(false),
        schema.contains("allowed skills for this turn"),
    ]
    .into_iter()
    .filter(|present| *present)
    .count()
}

fn load_cases(root: &Path) -> Result<Vec<PromptShapeCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn PromptShapeVariant,
    case: &PromptShapeCase,
    catalog: &SkillCatalog,
) -> Result<f64> {
    let _ = variant.build(case, catalog)?;
    let start = Instant::now();
    for _ in 0..BENCH_ITERATIONS {
        let artifact = variant.build(case, catalog)?;
        let _ = simulate_selection(&artifact, catalog)?;
    }
    Ok(start.elapsed().as_secs_f64() * 1_000_000.0 / BENCH_ITERATIONS as f64)
}

fn collect_source_metrics(
    root: &Path,
    source_path: &str,
    prompt_phrases: &[String],
) -> Result<PromptShapeSourceMetrics> {
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
    let branch_tokens = ["if ", "match ", " for ", "while ", "&&", "||", "format!"]
        .into_iter()
        .map(|token| source.matches(token).count())
        .sum();
    let hardcoded_phrase_refs = prompt_phrases
        .iter()
        .map(|phrase| source.matches(&format!("\"{phrase}\"")).count())
        .sum();
    let prompt_builder_refs = [
        "allowed_skills(",
        "catalog_summary(",
        "base_instruction(",
        "structured_recovery_guidance(",
        "full_schema(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(PromptShapeSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_phrase_refs,
        prompt_builder_refs,
    })
}

fn readability_score(source: &PromptShapeSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &PromptShapeSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_phrase_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.prompt_builder_refs as i32 * 6)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &PromptShapeExperimentReport) -> Vec<&PromptShapeVariantReport> {
    let best_accuracy = report
        .variants
        .iter()
        .map(|variant| variant.accuracy_pct)
        .fold(0.0f64, f64::max);
    report
        .variants
        .iter()
        .filter(|variant| (variant.accuracy_pct - best_accuracy).abs() < f64::EPSILON)
        .collect()
}

fn provisional_frontier(report: &PromptShapeExperimentReport) -> Option<&PromptShapeVariantReport> {
    frontier_variants(report).into_iter().min_by(|left, right| {
        left.average_prompt_chars
            .partial_cmp(&right.average_prompt_chars)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
