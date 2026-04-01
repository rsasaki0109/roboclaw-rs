use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod backend_authoritative;
mod confidence_weighted;
mod failure_aware_merge;
mod sensor_first;

use backend_authoritative::BackendAuthoritativeVariant;
use confidence_weighted::ConfidenceWeightedVariant;
use failure_aware_merge::FailureAwareMergeVariant;
use sensor_first::SensorFirstVariant;

const CASES_PATH: &str = "experiments/backend_observation_fusion/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionCase {
    pub id: String,
    pub failed_step: Option<String>,
    pub failed_tool: Option<String>,
    pub backend_pose: String,
    pub backend_held_object: Option<String>,
    pub sensor_detected: Option<bool>,
    pub sensor_pose: Option<String>,
    pub sensor_confidence: Option<f64>,
    pub expected_fused_pose: Option<String>,
    pub expected_target_visible: Option<bool>,
    pub expected_held_object: Option<String>,
    pub expected_target_pose: Option<String>,
    pub expected_replan_hint: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionDecision {
    pub fused_pose: Option<String>,
    pub target_visible: Option<bool>,
    pub held_object: Option<String>,
    pub target_pose: Option<String>,
    pub trust_label: String,
    pub replan_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionVariantCaseResult {
    pub case_id: String,
    pub correct: bool,
    pub expected_fused_pose: Option<String>,
    pub selected_fused_pose: Option<String>,
    pub expected_target_visible: Option<bool>,
    pub selected_target_visible: Option<bool>,
    pub expected_held_object: Option<String>,
    pub selected_held_object: Option<String>,
    pub expected_target_pose: Option<String>,
    pub selected_target_pose: Option<String>,
    pub expected_replan_hint: String,
    pub selected_replan_hint: String,
    pub signal_count: usize,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionVariantSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_hint_refs: usize,
    pub context_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: FusionVariantSourceMetrics,
    pub cases: Vec<FusionVariantCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_signal_count: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FusionExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<FusionCase>,
    pub variants: Vec<FusionVariantReport>,
}

pub trait FusionVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn fuse(&self, case: &FusionCase) -> Result<FusionDecision>;
}

pub fn run_suite(root: &Path) -> Result<FusionExperimentReport> {
    let cases = load_cases(root)?;
    let variants: Vec<Box<dyn FusionVariant>> = vec![
        Box::new(BackendAuthoritativeVariant::default()),
        Box::new(SensorFirstVariant::default()),
        Box::new(ConfidenceWeightedVariant::default()),
        Box::new(FailureAwareMergeVariant::default()),
    ];
    let replan_hints = vec![
        "reobserve_target".to_string(),
        "recover_grasp".to_string(),
        "verify_grasp_state".to_string(),
        "retry_place".to_string(),
        "resume_motion".to_string(),
        "placement_verified".to_string(),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &replan_hints)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;
        let mut total_signal_count = 0usize;

        for case in &cases {
            let decision = variant.fuse(case).with_context(|| {
                format!(
                    "variant '{}' failed to fuse case '{}'",
                    variant.name(),
                    case.id
                )
            })?;
            let bench_average_us = benchmark_variant(variant.as_ref(), case)?;
            let signal_count = signal_count(&decision);
            let is_correct = decision_matches(case, &decision);
            if is_correct {
                correct += 1;
            }
            total_average_us += bench_average_us;
            total_signal_count += signal_count;

            case_results.push(FusionVariantCaseResult {
                case_id: case.id.clone(),
                correct: is_correct,
                expected_fused_pose: case.expected_fused_pose.clone(),
                selected_fused_pose: decision.fused_pose.clone(),
                expected_target_visible: case.expected_target_visible,
                selected_target_visible: decision.target_visible,
                expected_held_object: case.expected_held_object.clone(),
                selected_held_object: decision.held_object.clone(),
                expected_target_pose: case.expected_target_pose.clone(),
                selected_target_pose: decision.target_pose.clone(),
                expected_replan_hint: case.expected_replan_hint.clone(),
                selected_replan_hint: decision.replan_hint.clone(),
                signal_count,
                average_decision_us: bench_average_us,
            });
        }

        reports.push(FusionVariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            accuracy_pct: correct as f64 * 100.0 / cases.len() as f64,
            average_decision_us: total_average_us / cases.len() as f64,
            average_signal_count: total_signal_count as f64 / cases.len() as f64,
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
            cases: case_results,
        });
    }

    Ok(FusionExperimentReport {
        problem: "Backend observation fusion should evolve through competing summaries of sensor and backend state before replanning.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &FusionExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} accuracy_pct={:.1} avg_decision_us={:.2} avg_signal_count={:.2} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_signal_count,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &FusionExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Backend Observation Fusion\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("| case | failed tool | expected hint | why |\n");
    markdown.push_str("| --- | --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}` | {} |\n",
            case.id,
            case.failed_tool.as_deref().unwrap_or("none"),
            case.expected_replan_hint,
            case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str(
        "| variant | style | accuracy | avg us | avg signals | readability | extensibility |\n",
    );
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | {} | {:.1}% | {:.2} | {:.2} | {} | {} |\n",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_signal_count,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &FusionExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Backend Observation Fusion\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while keeping richer fused context ({:.2} signals).\n",
            reference.name, reference.average_signal_count
        ));
    }
    markdown.push_str("- Keep fusion policy outside core until ROS2/Gazebo observations add richer conflict cases.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Backend Observation Fusion\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct FusionCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub failed_step: Option<String>,\n");
    markdown.push_str("    pub failed_tool: Option<String>,\n");
    markdown.push_str("    pub backend_pose: String,\n");
    markdown.push_str("    pub backend_held_object: Option<String>,\n");
    markdown.push_str("    pub sensor_detected: Option<bool>,\n");
    markdown.push_str("    pub sensor_pose: Option<String>,\n");
    markdown.push_str("    pub sensor_confidence: Option<f64>,\n");
    markdown.push_str("    pub expected_fused_pose: Option<String>,\n");
    markdown.push_str("    pub expected_target_visible: Option<bool>,\n");
    markdown.push_str("    pub expected_held_object: Option<String>,\n");
    markdown.push_str("    pub expected_target_pose: Option<String>,\n");
    markdown.push_str("    pub expected_replan_hint: String,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct FusionDecision {\n");
    markdown.push_str("    pub fused_pose: Option<String>,\n");
    markdown.push_str("    pub target_visible: Option<bool>,\n");
    markdown.push_str("    pub held_object: Option<String>,\n");
    markdown.push_str("    pub target_pose: Option<String>,\n");
    markdown.push_str("    pub trust_label: String,\n");
    markdown.push_str("    pub replan_hint: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait FusionVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str("    fn fuse(&self, case: &FusionCase) -> anyhow::Result<FusionDecision>;\n");
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str("- Shared input: same backend and sensor summary per case.\n");
    markdown.push_str("- Shared metrics: accuracy, average decision time, average signal count, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn backend_pose(case: &FusionCase) -> Option<String> {
    Some(case.backend_pose.clone())
}

pub(crate) fn backend_held_object(case: &FusionCase) -> Option<String> {
    case.backend_held_object.clone()
}

pub(crate) fn sensor_visible(case: &FusionCase) -> Option<bool> {
    case.sensor_detected
}

pub(crate) fn sensor_pose(case: &FusionCase) -> Option<String> {
    case.sensor_pose.clone()
}

pub(crate) fn sensor_confidence(case: &FusionCase) -> f64 {
    case.sensor_confidence.unwrap_or(0.0)
}

pub(crate) fn decision(
    fused_pose: Option<String>,
    target_visible: Option<bool>,
    held_object: Option<String>,
    target_pose: Option<String>,
    trust_label: impl Into<String>,
    replan_hint: impl Into<String>,
) -> FusionDecision {
    FusionDecision {
        fused_pose,
        target_visible,
        held_object,
        target_pose,
        trust_label: trust_label.into(),
        replan_hint: replan_hint.into(),
    }
}

fn decision_matches(case: &FusionCase, decision: &FusionDecision) -> bool {
    decision.fused_pose == case.expected_fused_pose
        && decision.target_visible == case.expected_target_visible
        && decision.held_object == case.expected_held_object
        && decision.target_pose == case.expected_target_pose
        && decision.replan_hint == case.expected_replan_hint
}

fn signal_count(decision: &FusionDecision) -> usize {
    [
        decision.fused_pose.is_some(),
        decision.target_visible.is_some(),
        decision.held_object.is_some(),
        decision.target_pose.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count()
}

fn load_cases(root: &Path) -> Result<Vec<FusionCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(variant: &dyn FusionVariant, case: &FusionCase) -> Result<f64> {
    let _ = variant.fuse(case)?;
    let start = Instant::now();
    for _ in 0..BENCH_ITERATIONS {
        let _ = variant.fuse(case)?;
    }
    Ok(start.elapsed().as_secs_f64() * 1_000_000.0 / BENCH_ITERATIONS as f64)
}

fn collect_source_metrics(
    root: &Path,
    source_path: &str,
    replan_hints: &[String],
) -> Result<FusionVariantSourceMetrics> {
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
    let hardcoded_hint_refs = replan_hints
        .iter()
        .map(|hint| source.matches(&format!("\"{hint}\"")).count())
        .sum();
    let context_refs = [
        "backend_pose(",
        "backend_held_object(",
        "sensor_visible(",
        "sensor_pose(",
        "sensor_confidence(",
        "failed_tool",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(FusionVariantSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_hint_refs,
        context_refs,
    })
}

fn readability_score(source: &FusionVariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &FusionVariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_hint_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.context_refs as i32 * 6)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &FusionExperimentReport) -> Vec<&FusionVariantReport> {
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

fn provisional_frontier(report: &FusionExperimentReport) -> Option<&FusionVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_signal_count
            .partial_cmp(&right.average_signal_count)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                (left.readability_score + left.extensibility_score)
                    .cmp(&(right.readability_score + right.extensibility_score))
            })
    })
}
