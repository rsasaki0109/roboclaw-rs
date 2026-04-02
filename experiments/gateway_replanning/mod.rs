use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod current_gateway_clone;
mod direct_resume_first;
mod fail_fast;
mod resume_aware_replan;

use current_gateway_clone::CurrentGatewayCloneVariant;
use direct_resume_first::DirectResumeFirstVariant;
use fail_fast::FailFastVariant;
use resume_aware_replan::ResumeAwareReplanVariant;

const CASES_PATH: &str = "experiments/gateway_replanning/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayCase {
    pub id: String,
    pub current_skill: String,
    pub completed: bool,
    #[serde(default)]
    pub resume_original_instruction: bool,
    pub failed_step: Option<String>,
    pub failed_tool: Option<String>,
    #[serde(default)]
    pub recovery_candidates: Vec<String>,
    pub resume_context_step: Option<String>,
    pub replans: usize,
    pub max_replans: usize,
    pub expected_decision_kind: String,
    pub expected_resume_step: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayDecision {
    pub decision_kind: String,
    pub resume_step: Option<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayVariantCaseResult {
    pub case_id: String,
    pub expected_decision_kind: String,
    pub selected_decision_kind: String,
    pub expected_resume_step: Option<String>,
    pub selected_resume_step: Option<String>,
    pub correct: bool,
    pub followup_rounds: usize,
    pub rationale: String,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayVariantSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub context_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: GatewayVariantSourceMetrics,
    pub cases: Vec<GatewayVariantCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_followup_rounds: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<GatewayCase>,
    pub variants: Vec<GatewayVariantReport>,
}

pub trait GatewayLoopVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn decide(&self, case: &GatewayCase) -> Result<GatewayDecision>;
}

pub fn run_suite(root: &Path) -> Result<GatewayExperimentReport> {
    let cases = load_cases(root)?;
    let variants: Vec<Box<dyn GatewayLoopVariant>> = vec![
        Box::new(CurrentGatewayCloneVariant::default()),
        Box::new(FailFastVariant::default()),
        Box::new(DirectResumeFirstVariant::default()),
        Box::new(ResumeAwareReplanVariant::default()),
    ];
    let decision_kinds = vec![
        "finish".to_string(),
        "stop_failed".to_string(),
        "replan_with_recovery".to_string(),
        "replan_generic".to_string(),
        "direct_resume".to_string(),
        "resume_original".to_string(),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &decision_kinds)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;
        let mut total_followup_rounds = 0usize;

        for case in &cases {
            let decision = variant.decide(case).with_context(|| {
                format!(
                    "variant '{}' failed to decide for case '{}'",
                    variant.name(),
                    case.id
                )
            })?;
            let bench_average_us = benchmark_variant(variant.as_ref(), case)?;
            let is_correct = decision_matches(case, &decision);
            if is_correct {
                correct += 1;
            }
            let followup_rounds = followup_rounds(&decision);
            total_average_us += bench_average_us;
            total_followup_rounds += followup_rounds;

            case_results.push(GatewayVariantCaseResult {
                case_id: case.id.clone(),
                expected_decision_kind: case.expected_decision_kind.clone(),
                selected_decision_kind: decision.decision_kind,
                expected_resume_step: case.expected_resume_step.clone(),
                selected_resume_step: decision.resume_step,
                correct: is_correct,
                followup_rounds,
                rationale: decision.rationale,
                average_decision_us: bench_average_us,
            });
        }

        reports.push(GatewayVariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            accuracy_pct: correct as f64 * 100.0 / cases.len() as f64,
            average_decision_us: total_average_us / cases.len() as f64,
            average_followup_rounds: total_followup_rounds as f64 / cases.len() as f64,
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
            cases: case_results,
        });
    }

    Ok(GatewayExperimentReport {
        problem:
            "Gateway replanning should be explored as loop policies, not hidden as one control-flow implementation.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &GatewayExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} accuracy_pct={:.1} avg_decision_us={:.2} avg_followup_rounds={:.2} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_followup_rounds,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &GatewayExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Gateway Replanning\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("| case | skill | expected decision | expected resume | why |\n");
    markdown.push_str("| --- | --- | --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | {} |\n",
            case.id,
            case.current_skill,
            case.expected_decision_kind,
            case.expected_resume_step.as_deref().unwrap_or("none"),
            case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str("| variant | style | accuracy | avg us | avg followup rounds | readability | extensibility |\n");
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | {} | {:.1}% | {:.2} | {:.2} | {} | {} |\n",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_followup_rounds,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &GatewayExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Gateway Replanning\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while minimizing followup rounds ({:.2}).\n",
            reference.name, reference.average_followup_rounds
        ));
    }
    markdown.push_str("- Keep loop policy experimental until cases include richer backend-state transitions and repeated recovery chains.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Gateway Replanning\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct GatewayCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub current_skill: String,\n");
    markdown.push_str("    pub completed: bool,\n");
    markdown.push_str("    pub resume_original_instruction: bool,\n");
    markdown.push_str("    pub failed_step: Option<String>,\n");
    markdown.push_str("    pub failed_tool: Option<String>,\n");
    markdown.push_str("    pub recovery_candidates: Vec<String>,\n");
    markdown.push_str("    pub resume_context_step: Option<String>,\n");
    markdown.push_str("    pub replans: usize,\n");
    markdown.push_str("    pub max_replans: usize,\n");
    markdown.push_str("    pub expected_decision_kind: String,\n");
    markdown.push_str("    pub expected_resume_step: Option<String>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct GatewayDecision {\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub resume_step: Option<String>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait GatewayLoopVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown
        .push_str("    fn decide(&self, case: &GatewayCase) -> anyhow::Result<GatewayDecision>;\n");
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str("- Shared input: same failure/recovery summaries for all loop variants.\n");
    markdown.push_str("- Shared metrics: accuracy, average decision time, average followup rounds, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn finish(rationale: impl Into<String>) -> GatewayDecision {
    GatewayDecision {
        decision_kind: "finish".to_string(),
        resume_step: None,
        rationale: rationale.into(),
    }
}

pub(crate) fn stop_failed(rationale: impl Into<String>) -> GatewayDecision {
    GatewayDecision {
        decision_kind: "stop_failed".to_string(),
        resume_step: None,
        rationale: rationale.into(),
    }
}

pub(crate) fn replan_with_recovery(
    step: Option<String>,
    rationale: impl Into<String>,
) -> GatewayDecision {
    GatewayDecision {
        decision_kind: "replan_with_recovery".to_string(),
        resume_step: step,
        rationale: rationale.into(),
    }
}

pub(crate) fn replan_generic(
    step: Option<String>,
    rationale: impl Into<String>,
) -> GatewayDecision {
    GatewayDecision {
        decision_kind: "replan_generic".to_string(),
        resume_step: step,
        rationale: rationale.into(),
    }
}

pub(crate) fn direct_resume(step: Option<String>, rationale: impl Into<String>) -> GatewayDecision {
    GatewayDecision {
        decision_kind: "direct_resume".to_string(),
        resume_step: step,
        rationale: rationale.into(),
    }
}

pub(crate) fn resume_original(
    step: Option<String>,
    rationale: impl Into<String>,
) -> GatewayDecision {
    GatewayDecision {
        decision_kind: "resume_original".to_string(),
        resume_step: step,
        rationale: rationale.into(),
    }
}

pub(crate) fn has_replan_budget(case: &GatewayCase) -> bool {
    case.replans < case.max_replans
}

fn decision_matches(case: &GatewayCase, decision: &GatewayDecision) -> bool {
    decision.decision_kind == case.expected_decision_kind
        && decision.resume_step == case.expected_resume_step
}

fn followup_rounds(decision: &GatewayDecision) -> usize {
    match decision.decision_kind.as_str() {
        "finish" | "stop_failed" => 0,
        "direct_resume" | "resume_original" => 1,
        "replan_with_recovery" => 2,
        "replan_generic" => 2,
        _ => 3,
    }
}

fn load_cases(root: &Path) -> Result<Vec<GatewayCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(variant: &dyn GatewayLoopVariant, case: &GatewayCase) -> Result<f64> {
    let _ = variant.decide(case)?;
    let start = Instant::now();
    for _ in 0..BENCH_ITERATIONS {
        let _ = variant.decide(case)?;
    }
    Ok(start.elapsed().as_secs_f64() * 1_000_000.0 / BENCH_ITERATIONS as f64)
}

fn collect_source_metrics(
    root: &Path,
    source_path: &str,
    decision_kinds: &[String],
) -> Result<GatewayVariantSourceMetrics> {
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
    let hardcoded_decision_refs = decision_kinds
        .iter()
        .map(|decision| source.matches(&format!("\"{decision}\"")).count())
        .sum();
    let context_refs = [
        "recovery_candidates",
        "resume_context_step",
        "replans",
        "max_replans",
        "resume_original_instruction",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(GatewayVariantSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        context_refs,
    })
}

fn readability_score(source: &GatewayVariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &GatewayVariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_decision_refs as i32 * 10) - (source.branch_tokens as i32 * 2)
            + (source.context_refs as i32 * 6)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &GatewayExperimentReport) -> Vec<&GatewayVariantReport> {
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

fn provisional_frontier(report: &GatewayExperimentReport) -> Option<&GatewayVariantReport> {
    frontier_variants(report).into_iter().min_by(|left, right| {
        left.average_followup_rounds
            .partial_cmp(&right.average_followup_rounds)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                (right.readability_score + right.extensibility_score)
                    .cmp(&(left.readability_score + left.extensibility_score))
            })
    })
}
