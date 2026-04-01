use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod always_reset_rescan;
mod checkpoint_aware_specialist;
mod single_step_retry;
mod verification_first;

use always_reset_rescan::AlwaysResetRescanVariant;
use checkpoint_aware_specialist::CheckpointAwareSpecialistVariant;
use single_step_retry::SingleStepRetryVariant;
use verification_first::VerificationFirstVariant;

const CASES_PATH: &str = "experiments/recovery_skill_patterns/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPatternCase {
    pub id: String,
    pub failed_step: String,
    pub failed_tool: String,
    pub backend_pose: String,
    pub held_object: Option<String>,
    pub target_visible: Option<bool>,
    pub previous_replans: usize,
    pub max_replans: usize,
    pub expected_pattern: String,
    pub expected_resume_original_instruction: bool,
    pub expected_resume_step: Option<String>,
    pub expected_manual_handoff: bool,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPatternDecision {
    pub pattern_name: String,
    pub recovery_steps: Vec<String>,
    pub resume_original_instruction: bool,
    pub resume_step: Option<String>,
    pub manual_handoff: bool,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPatternCaseResult {
    pub case_id: String,
    pub correct: bool,
    pub expected_pattern: String,
    pub selected_pattern: String,
    pub expected_resume_step: Option<String>,
    pub selected_resume_step: Option<String>,
    pub expected_manual_handoff: bool,
    pub selected_manual_handoff: bool,
    pub recovery_step_count: usize,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPatternSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_pattern_refs: usize,
    pub context_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPatternVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: RecoveryPatternSourceMetrics,
    pub cases: Vec<RecoveryPatternCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_recovery_steps: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryPatternExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<RecoveryPatternCase>,
    pub variants: Vec<RecoveryPatternVariantReport>,
}

pub trait RecoveryPatternVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn design(&self, case: &RecoveryPatternCase) -> Result<RecoveryPatternDecision>;
}

pub fn run_suite(root: &Path) -> Result<RecoveryPatternExperimentReport> {
    let cases = load_cases(root)?;
    let patterns = vec![
        "rescan_only".to_string(),
        "retry_grasp".to_string(),
        "retry_place".to_string(),
        "reset_rescan".to_string(),
        "pregrasp_verify".to_string(),
        "verify_hold".to_string(),
        "reconfirm_place".to_string(),
        "direct_resume".to_string(),
        "manual_handoff".to_string(),
    ];
    let variants: Vec<Box<dyn RecoveryPatternVariant>> = vec![
        Box::new(SingleStepRetryVariant::default()),
        Box::new(AlwaysResetRescanVariant::default()),
        Box::new(VerificationFirstVariant::default()),
        Box::new(CheckpointAwareSpecialistVariant::default()),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &patterns)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;
        let mut total_recovery_steps = 0usize;

        for case in &cases {
            let decision = variant.design(case).with_context(|| {
                format!(
                    "variant '{}' failed to design case '{}'",
                    variant.name(),
                    case.id
                )
            })?;
            let bench_average_us = benchmark_variant(variant.as_ref(), case)?;
            let recovery_step_count = decision.recovery_steps.len();
            let is_correct = decision_matches(case, &decision);
            if is_correct {
                correct += 1;
            }
            total_average_us += bench_average_us;
            total_recovery_steps += recovery_step_count;

            case_results.push(RecoveryPatternCaseResult {
                case_id: case.id.clone(),
                correct: is_correct,
                expected_pattern: case.expected_pattern.clone(),
                selected_pattern: decision.pattern_name,
                expected_resume_step: case.expected_resume_step.clone(),
                selected_resume_step: decision.resume_step,
                expected_manual_handoff: case.expected_manual_handoff,
                selected_manual_handoff: decision.manual_handoff,
                recovery_step_count,
                average_decision_us: bench_average_us,
            });
        }

        reports.push(RecoveryPatternVariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            cases: case_results,
            accuracy_pct: correct as f64 * 100.0 / cases.len() as f64,
            average_decision_us: total_average_us / cases.len() as f64,
            average_recovery_steps: total_recovery_steps as f64 / cases.len() as f64,
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
        });
    }

    Ok(RecoveryPatternExperimentReport {
        problem: "Recovery skill design should evolve through competing patterns instead of freezing the first specialized skill set into the runtime.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &RecoveryPatternExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} accuracy_pct={:.1} avg_decision_us={:.2} avg_recovery_steps={:.2} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_recovery_steps,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &RecoveryPatternExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Recovery Skill Patterns\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("| case | failed step | expected pattern | expected resume | why |\n");
    markdown.push_str("| --- | --- | --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | {} |\n",
            case.id,
            case.failed_step,
            case.expected_pattern,
            case.expected_resume_step.as_deref().unwrap_or("none"),
            case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str(
        "| variant | style | accuracy | avg us | avg recovery steps | readability | extensibility |\n",
    );
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | {} | {:.1}% | {:.2} | {:.2} | {} | {} |\n",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_recovery_steps,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &RecoveryPatternExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Recovery Skill Patterns\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while keeping recovery sequences short ({:.2} average steps).\n",
            reference.name, reference.average_recovery_steps
        ));
    }
    markdown.push_str("- Keep recovery patterns experimental until real failure traces force convergence on which recoveries deserve first-class YAML skills.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Recovery Skill Patterns\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct RecoveryPatternCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub failed_step: String,\n");
    markdown.push_str("    pub failed_tool: String,\n");
    markdown.push_str("    pub backend_pose: String,\n");
    markdown.push_str("    pub held_object: Option<String>,\n");
    markdown.push_str("    pub target_visible: Option<bool>,\n");
    markdown.push_str("    pub previous_replans: usize,\n");
    markdown.push_str("    pub max_replans: usize,\n");
    markdown.push_str("    pub expected_pattern: String,\n");
    markdown.push_str("    pub expected_resume_original_instruction: bool,\n");
    markdown.push_str("    pub expected_resume_step: Option<String>,\n");
    markdown.push_str("    pub expected_manual_handoff: bool,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct RecoveryPatternDecision {\n");
    markdown.push_str("    pub pattern_name: String,\n");
    markdown.push_str("    pub recovery_steps: Vec<String>,\n");
    markdown.push_str("    pub resume_original_instruction: bool,\n");
    markdown.push_str("    pub resume_step: Option<String>,\n");
    markdown.push_str("    pub manual_handoff: bool,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait RecoveryPatternVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str("    fn design(&self, case: &RecoveryPatternCase) -> anyhow::Result<RecoveryPatternDecision>;\n");
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str("- Shared input: same failure context and retry budget for all recovery-pattern variants.\n");
    markdown.push_str("- Shared metrics: accuracy, average decision time, average recovery steps, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    pattern_name: impl Into<String>,
    recovery_steps: Vec<&str>,
    resume_original_instruction: bool,
    resume_step: Option<&str>,
    manual_handoff: bool,
    rationale: impl Into<String>,
) -> RecoveryPatternDecision {
    RecoveryPatternDecision {
        pattern_name: pattern_name.into(),
        recovery_steps: recovery_steps.into_iter().map(str::to_string).collect(),
        resume_original_instruction,
        resume_step: resume_step.map(str::to_string),
        manual_handoff,
        rationale: rationale.into(),
    }
}

pub(crate) fn budget_exhausted(case: &RecoveryPatternCase) -> bool {
    case.previous_replans >= case.max_replans
}

fn decision_matches(case: &RecoveryPatternCase, decision: &RecoveryPatternDecision) -> bool {
    decision.pattern_name == case.expected_pattern
        && decision.resume_original_instruction == case.expected_resume_original_instruction
        && decision.resume_step == case.expected_resume_step
        && decision.manual_handoff == case.expected_manual_handoff
}

fn load_cases(root: &Path) -> Result<Vec<RecoveryPatternCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn RecoveryPatternVariant,
    case: &RecoveryPatternCase,
) -> Result<f64> {
    let _ = variant.design(case)?;
    let start = Instant::now();
    for _ in 0..BENCH_ITERATIONS {
        let _ = variant.design(case)?;
    }
    Ok(start.elapsed().as_secs_f64() * 1_000_000.0 / BENCH_ITERATIONS as f64)
}

fn collect_source_metrics(
    root: &Path,
    source_path: &str,
    patterns: &[String],
) -> Result<RecoveryPatternSourceMetrics> {
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
    let branch_tokens = ["if ", "match ", " for ", "while ", "&&", "||", ".is_some()"]
        .into_iter()
        .map(|token| source.matches(token).count())
        .sum();
    let hardcoded_pattern_refs = patterns
        .iter()
        .map(|pattern| source.matches(&format!("\"{pattern}\"")).count())
        .sum();
    let context_refs = [
        "budget_exhausted(",
        "failed_step",
        "failed_tool",
        "backend_pose",
        "held_object",
        "target_visible",
        "previous_replans",
        "max_replans",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(RecoveryPatternSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_pattern_refs,
        context_refs,
    })
}

fn readability_score(source: &RecoveryPatternSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &RecoveryPatternSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_pattern_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.context_refs as i32 * 6)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(
    report: &RecoveryPatternExperimentReport,
) -> Vec<&RecoveryPatternVariantReport> {
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

fn provisional_frontier(
    report: &RecoveryPatternExperimentReport,
) -> Option<&RecoveryPatternVariantReport> {
    frontier_variants(report).into_iter().min_by(|left, right| {
        left.average_recovery_steps
            .partial_cmp(&right.average_recovery_steps)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                (left.readability_score + left.extensibility_score)
                    .cmp(&(right.readability_score + right.extensibility_score))
            })
    })
}
