use anyhow::{anyhow, Context, Result};
use roboclaw_rs::skills::{Skill, SkillCatalog};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod conservative_rewind;
mod declared_checkpoint;
mod restart_skill;
mod resume_failed_step;

use conservative_rewind::ConservativeRewindVariant;
use declared_checkpoint::DeclaredCheckpointVariant;
use restart_skill::RestartSkillVariant;
use resume_failed_step::ResumeFailedStepVariant;

const CASES_PATH: &str = "experiments/resume_policy/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeCase {
    pub id: String,
    pub skill: String,
    pub failed_step: String,
    pub expected_resume_step: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeDecision {
    pub resume_step: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeVariantCaseResult {
    pub case_id: String,
    pub skill: String,
    pub failed_step: String,
    pub expected_resume_step: String,
    pub selected_resume_step: String,
    pub correct: bool,
    pub remaining_steps: usize,
    pub rationale: String,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeVariantSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_step_refs: usize,
    pub metadata_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: ResumeVariantSourceMetrics,
    pub cases: Vec<ResumeVariantCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_remaining_steps: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<ResumeCase>,
    pub variants: Vec<ResumeVariantReport>,
}

pub trait ResumePolicyVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn select_resume_step(&self, case: &ResumeCase, skill: &Skill) -> Result<ResumeDecision>;
}

pub fn run_suite(root: &Path) -> Result<ResumeExperimentReport> {
    let skill_dir = root.join("skills");
    let catalog = SkillCatalog::from_dir(&skill_dir)
        .with_context(|| format!("failed to load skill catalog from {:?}", skill_dir))?;
    let cases = load_cases(root)?;
    let step_names = catalog
        .values()
        .flat_map(|skill| skill.steps.iter().map(|step| step.name.clone()))
        .collect::<Vec<_>>();

    let variants: Vec<Box<dyn ResumePolicyVariant>> = vec![
        Box::new(RestartSkillVariant::default()),
        Box::new(ResumeFailedStepVariant::default()),
        Box::new(DeclaredCheckpointVariant::default()),
        Box::new(ConservativeRewindVariant::default()),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &step_names)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;
        let mut total_remaining_steps = 0usize;

        for case in &cases {
            let skill = catalog
                .get(&case.skill)
                .cloned()
                .ok_or_else(|| anyhow!("unknown skill '{}'", case.skill))?;
            let decision = variant.select_resume_step(case, &skill).with_context(|| {
                format!(
                    "variant '{}' failed to select resume step for case '{}'",
                    variant.name(),
                    case.id
                )
            })?;
            let bench_average_us = benchmark_variant(variant.as_ref(), case, &skill)?;
            let remaining_steps = remaining_steps_from(&skill, &decision.resume_step)?;
            let is_correct = decision.resume_step == case.expected_resume_step;
            if is_correct {
                correct += 1;
            }
            total_average_us += bench_average_us;
            total_remaining_steps += remaining_steps;

            case_results.push(ResumeVariantCaseResult {
                case_id: case.id.clone(),
                skill: case.skill.clone(),
                failed_step: case.failed_step.clone(),
                expected_resume_step: case.expected_resume_step.clone(),
                selected_resume_step: decision.resume_step,
                correct: is_correct,
                remaining_steps,
                rationale: decision.rationale,
                average_decision_us: bench_average_us,
            });
        }

        reports.push(ResumeVariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            accuracy_pct: correct as f64 * 100.0 / cases.len() as f64,
            average_decision_us: total_average_us / cases.len() as f64,
            average_remaining_steps: total_remaining_steps as f64 / cases.len() as f64,
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
            cases: case_results,
        });
    }

    Ok(ResumeExperimentReport {
        problem: "Resume policy after recovery should be discovered through comparable strategies, not frozen as a hidden gateway rule.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &ResumeExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} accuracy_pct={:.1} avg_decision_us={:.2} avg_remaining_steps={:.2} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_remaining_steps,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &ResumeExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Resume Policy\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("| case | skill | failed step | expected resume | why |\n");
    markdown.push_str("| --- | --- | --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | {} |\n",
            case.id, case.skill, case.failed_step, case.expected_resume_step, case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str("| variant | style | accuracy | avg us | avg remaining steps | readability | extensibility |\n");
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | {} | {:.1}% | {:.2} | {:.2} | {} | {} |\n",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_remaining_steps,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &ResumeExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Resume Policy\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while minimizing remaining steps ({:.2}) without sacrificing accuracy.\n",
            reference.name, reference.average_remaining_steps
        ));
    }
    markdown.push_str("- Do not move resume policy into a larger abstraction yet. Keep it as an experiment surface until more recovery cases exist.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Resume Policy\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct ResumeCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub skill: String,\n");
    markdown.push_str("    pub failed_step: String,\n");
    markdown.push_str("    pub expected_resume_step: String,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ResumeDecision {\n");
    markdown.push_str("    pub resume_step: String,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait ResumePolicyVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str("    fn select_resume_step(&self, case: &ResumeCase, skill: &Skill) -> anyhow::Result<ResumeDecision>;\n");
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str("- Shared input: same `ResumeCase` set and same YAML skill definitions.\n");
    markdown.push_str("- Shared metrics: accuracy, average decision time, average remaining steps, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn failed_step<'a>(case: &'a ResumeCase) -> &'a str {
    &case.failed_step
}

pub(crate) fn first_step(skill: &Skill) -> Result<String> {
    skill
        .steps
        .first()
        .map(|step| step.name.clone())
        .ok_or_else(|| anyhow!("skill '{}' has no steps", skill.name))
}

pub(crate) fn declared_checkpoint(skill: &Skill, step_name: &str) -> Result<String> {
    skill
        .steps
        .iter()
        .find(|step| step.name == step_name)
        .map(|step| {
            step.resume_from_step
                .clone()
                .unwrap_or_else(|| step.name.clone())
        })
        .ok_or_else(|| {
            anyhow!(
                "skill '{}' does not contain step '{}'",
                skill.name,
                step_name
            )
        })
}

pub(crate) fn step_index(skill: &Skill, step_name: &str) -> Result<usize> {
    skill
        .steps
        .iter()
        .position(|step| step.name == step_name)
        .ok_or_else(|| {
            anyhow!(
                "skill '{}' does not contain step '{}'",
                skill.name,
                step_name
            )
        })
}

pub(crate) fn choose_resume_step(
    skill: &Skill,
    step_name: &str,
    rationale: impl Into<String>,
) -> Result<ResumeDecision> {
    let _ = step_index(skill, step_name)?;
    Ok(ResumeDecision {
        resume_step: step_name.to_string(),
        rationale: rationale.into(),
    })
}

fn load_cases(root: &Path) -> Result<Vec<ResumeCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn ResumePolicyVariant,
    case: &ResumeCase,
    skill: &Skill,
) -> Result<f64> {
    let _ = variant.select_resume_step(case, skill)?;
    let start = Instant::now();
    for _ in 0..BENCH_ITERATIONS {
        let _ = variant.select_resume_step(case, skill)?;
    }
    Ok(start.elapsed().as_secs_f64() * 1_000_000.0 / BENCH_ITERATIONS as f64)
}

fn remaining_steps_from(skill: &Skill, resume_step: &str) -> Result<usize> {
    let index = step_index(skill, resume_step)?;
    Ok(skill.steps.len().saturating_sub(index))
}

fn collect_source_metrics(
    root: &Path,
    source_path: &str,
    step_names: &[String],
) -> Result<ResumeVariantSourceMetrics> {
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
    let branch_tokens = [
        "if ", "match ", " for ", "while ", "&&", "||", ".find(", ".filter(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();
    let hardcoded_step_refs = step_names
        .iter()
        .map(|step| source.matches(&format!("\"{step}\"")).count())
        .sum();
    let metadata_refs = [
        "resume_from_step",
        "declared_checkpoint(",
        "step_index(",
        "tool ==",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(ResumeVariantSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_step_refs,
        metadata_refs,
    })
}

fn readability_score(source: &ResumeVariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &ResumeVariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_step_refs as i32 * 10) - (source.branch_tokens as i32 * 2)
            + (source.metadata_refs as i32 * 8)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &ResumeExperimentReport) -> Vec<&ResumeVariantReport> {
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

fn provisional_frontier(report: &ResumeExperimentReport) -> Option<&ResumeVariantReport> {
    frontier_variants(report).into_iter().min_by(|left, right| {
        left.average_remaining_steps
            .partial_cmp(&right.average_remaining_steps)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                (right.readability_score + right.extensibility_score)
                    .cmp(&(left.readability_score + left.extensibility_score))
            })
    })
}
