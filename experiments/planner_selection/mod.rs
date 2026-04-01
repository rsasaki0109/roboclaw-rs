use anyhow::{Context, Result};
use roboclaw_rs::skills::{RecoveryContext, SkillCatalog};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod catalog_score;
mod current_heuristic_clone;
mod keyword_rules;
mod pipeline_router;

use catalog_score::CatalogScoreVariant;
use current_heuristic_clone::CurrentHeuristicCloneVariant;
use keyword_rules::KeywordRulesVariant;
use pipeline_router::PipelineRouterVariant;

const CASES_PATH: &str = "experiments/planner_selection/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningCase {
    pub id: String,
    pub instruction: String,
    pub expected_skill: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantDecision {
    pub selected_skill: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantCaseResult {
    pub case_id: String,
    pub expected_skill: String,
    pub selected_skill: String,
    pub correct: bool,
    pub rationale: String,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_skill_refs: usize,
    pub metadata_refs: usize,
    pub stage_functions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: VariantSourceMetrics,
    pub cases: Vec<VariantCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<PlanningCase>,
    pub variants: Vec<VariantReport>,
}

pub trait PlannerVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn plan(&self, case: &PlanningCase, catalog: &SkillCatalog) -> Result<VariantDecision>;
}

pub fn run_suite(root: &Path) -> Result<ExperimentReport> {
    let skill_dir = root.join("skills");
    let catalog = SkillCatalog::from_dir(&skill_dir)
        .with_context(|| format!("failed to load skill catalog from {:?}", skill_dir))?;
    let cases = load_cases(root)?;
    let skill_names = catalog.names();

    let variants: Vec<Box<dyn PlannerVariant>> = vec![
        Box::new(CurrentHeuristicCloneVariant::default()),
        Box::new(KeywordRulesVariant::default()),
        Box::new(CatalogScoreVariant::default()),
        Box::new(PipelineRouterVariant::default()),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &skill_names)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;

        for case in &cases {
            let decision = variant.plan(case, &catalog).with_context(|| {
                format!(
                    "variant '{}' failed to plan case '{}'",
                    variant.name(),
                    case.id
                )
            })?;
            let bench_average_us = benchmark_variant(variant.as_ref(), case, &catalog)?;
            let is_correct = decision.selected_skill == case.expected_skill;
            if is_correct {
                correct += 1;
            }
            total_average_us += bench_average_us;
            case_results.push(VariantCaseResult {
                case_id: case.id.clone(),
                expected_skill: case.expected_skill.clone(),
                selected_skill: decision.selected_skill,
                correct: is_correct,
                rationale: decision.rationale,
                average_decision_us: bench_average_us,
            });
        }

        let average_decision_us = total_average_us / cases.len() as f64;
        reports.push(VariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            cases: case_results,
            accuracy_pct: correct as f64 * 100.0 / cases.len() as f64,
            average_decision_us,
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
        });
    }

    Ok(ExperimentReport {
        problem: "Planner skill selection should evolve through comparable experiments instead of one-off abstraction.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &ExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} accuracy_pct={:.1} avg_decision_us={:.2} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &ExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Planner Selection\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("The stable runtime stays in `crates/`; planner variants live in `experiments/planner_selection/` and are judged on the same instruction set.\n\n");
    markdown.push_str("| case | expected | why |\n");
    markdown.push_str("| --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | {} |\n",
            case.id, case.expected_skill, case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str(
        "| variant | style | accuracy | avg us | readability | extensibility | source |\n",
    );
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | {} | {:.1}% | {:.2} | {} | {} | `{}` |\n",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.readability_score,
            variant.extensibility_score,
            variant.source.source_path
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &ExperimentReport) -> String {
    let frontier_names = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Planner Selection\n\n");
    markdown.push_str(&format!(
        "- Frontier set: {}.\n",
        if frontier_names.is_empty() {
            "none".to_string()
        } else {
            frontier_names
        }
    ));
    if let Some(variant) = provisional_frontier(report) {
        markdown.push_str(&format!(
            "- Provisional reference: `{}` because it stayed on the frontier and had the best balance between readability ({}) and extensibility ({}).\n",
            variant.name, variant.readability_score, variant.extensibility_score
        ));
    }
    markdown.push_str("- Keep all planner variants experimental until additional cases force convergence on a smaller minimum interface.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Planner Selection\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct PlanningCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub instruction: String,\n");
    markdown.push_str("    pub expected_skill: String,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct VariantDecision {\n");
    markdown.push_str("    pub selected_skill: String,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait PlannerVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str("    fn plan(&self, case: &PlanningCase, catalog: &SkillCatalog) -> anyhow::Result<VariantDecision>;\n");
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str("- Shared input: same `PlanningCase` list and same `SkillCatalog`.\n");
    markdown.push_str("- Shared metrics: accuracy, average decision time, readability proxy, extensibility proxy.\n");
    markdown
}

fn load_cases(root: &Path) -> Result<Vec<PlanningCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn PlannerVariant,
    case: &PlanningCase,
    catalog: &SkillCatalog,
) -> Result<f64> {
    let _ = variant.plan(case, catalog)?;
    let start = Instant::now();
    for _ in 0..BENCH_ITERATIONS {
        let _ = variant.plan(case, catalog)?;
    }
    let average_us = start.elapsed().as_secs_f64() * 1_000_000.0 / BENCH_ITERATIONS as f64;
    Ok(average_us)
}

fn collect_source_metrics(
    root: &Path,
    source_path: &str,
    skill_names: &[String],
) -> Result<VariantSourceMetrics> {
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
    let stage_functions = source
        .lines()
        .filter(|line| line.trim_start().starts_with("fn stage_"))
        .count();

    let branch_tokens = [
        "if ", "match ", " for ", "while ", "&&", "||", ".find(", ".filter(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    let hardcoded_skill_refs = skill_names
        .iter()
        .map(|skill| source.matches(&format!("\"{skill}\"")).count())
        .sum();

    let metadata_refs = [
        "recovery_candidates(",
        "recovery_candidate_names",
        "recovery_for",
        "matches_recovery_context",
        "recovery_context_from_instruction",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(VariantSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_skill_refs,
        metadata_refs,
        stage_functions,
    })
}

fn readability_score(source: &VariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &VariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_skill_refs as i32 * 10) - (source.branch_tokens as i32 * 2)
            + (source.metadata_refs as i32 * 10)
            + (source.stage_functions as i32 * 6)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

pub(crate) fn normalize(text: &str) -> String {
    text.to_lowercase()
}

pub(crate) fn tokenize_keywords(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| token.len() > 2)
        .map(|token| token.to_lowercase())
        .collect()
}

pub(crate) fn is_replan_instruction(text: &str) -> bool {
    text.contains("previous execution failed")
        || text.contains("replan_after_")
        || text.contains("failed step:")
}

pub(crate) fn recovery_context_from_instruction(text: &str) -> RecoveryContext {
    RecoveryContext {
        failed_step: extract_instruction_field(text, "failed step:"),
        tool: extract_instruction_field(text, "failed tool:"),
        observation: extract_instruction_field(text, "observation:"),
    }
}

pub(crate) fn extract_instruction_field(text: &str, label: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(label)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

pub(crate) fn choose_named_skill(
    catalog: &SkillCatalog,
    skill_name: &str,
    rationale: impl Into<String>,
) -> VariantDecision {
    let selected_skill = if catalog.get(skill_name).is_some() {
        skill_name.to_string()
    } else {
        catalog
            .first()
            .map(|skill| skill.name.clone())
            .unwrap_or_else(|| "unknown".to_string())
    };

    VariantDecision {
        selected_skill,
        rationale: rationale.into(),
    }
}

pub(crate) fn fallback_to_first(
    catalog: &SkillCatalog,
    rationale: impl Into<String>,
) -> VariantDecision {
    VariantDecision {
        selected_skill: catalog
            .first()
            .map(|skill| skill.name.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        rationale: rationale.into(),
    }
}

pub(crate) fn recovery_candidates(catalog: &SkillCatalog, instruction: &str) -> Vec<String> {
    let normalized = normalize(instruction);
    if !is_replan_instruction(&normalized) {
        return Vec::new();
    }

    catalog.recovery_candidate_names(&recovery_context_from_instruction(&normalized))
}

pub(crate) fn skill_token_overlap_score(
    instruction_tokens: &[String],
    skill_name: &str,
    skill_description: &str,
) -> usize {
    let skill_tokens = tokenize_keywords(&format!("{skill_name} {skill_description}"));
    instruction_tokens
        .iter()
        .filter(|token| skill_tokens.contains(token))
        .count()
}

fn frontier_variants(report: &ExperimentReport) -> Vec<&VariantReport> {
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

fn provisional_frontier(report: &ExperimentReport) -> Option<&VariantReport> {
    frontier_variants(report)
        .into_iter()
        .max_by_key(|variant| variant.readability_score + variant.extensibility_score)
}
