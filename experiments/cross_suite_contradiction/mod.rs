use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod evidence_graph;
mod frontier_membership_only;
mod promotion_kind_only;
mod risk_signal_gate;

use evidence_graph::EvidenceGraphVariant;
use frontier_membership_only::FrontierMembershipOnlyVariant;
use promotion_kind_only::PromotionKindOnlyVariant;
use risk_signal_gate::RiskSignalGateVariant;

const CASES_PATH: &str = "experiments/cross_suite_contradiction/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContradictionCase {
    pub id: String,
    pub suite: String,
    pub local_frontier: Vec<String>,
    pub provisional_reference: Option<String>,
    pub promotion_decision_kind: String,
    pub promotion_reference: Option<String>,
    pub replay_signal: String,
    pub support_gap: i32,
    pub environment_specific: bool,
    pub runtime_risk: u32,
    pub expected_detection_kind: String,
    pub expected_conflicting_reference: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContradictionDecision {
    pub detection_kind: String,
    pub conflicting_reference: Option<String>,
    pub rationale: String,
    pub consumed_signals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContradictionVariantCaseResult {
    pub case_id: String,
    pub correct: bool,
    pub expected_detection_kind: String,
    pub selected_detection_kind: String,
    pub expected_conflicting_reference: Option<String>,
    pub selected_conflicting_reference: Option<String>,
    pub consumed_signals: usize,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContradictionSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub evidence_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContradictionVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: ContradictionSourceMetrics,
    pub cases: Vec<ContradictionVariantCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_consumed_signals: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContradictionExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<ContradictionCase>,
    pub variants: Vec<ContradictionVariantReport>,
}

pub trait ContradictionVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn detect(&self, case: &ContradictionCase) -> Result<ContradictionDecision>;
}

pub fn run_suite(root: &Path) -> Result<ContradictionExperimentReport> {
    let cases = load_cases(root)?;
    let decision_refs = [
        "no_contradiction",
        "reference_conflict",
        "premature_promotion",
        "risk_conflict",
    ];
    let variants: Vec<Box<dyn ContradictionVariant>> = vec![
        Box::new(FrontierMembershipOnlyVariant::default()),
        Box::new(PromotionKindOnlyVariant::default()),
        Box::new(RiskSignalGateVariant::default()),
        Box::new(EvidenceGraphVariant::default()),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &decision_refs)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;
        let mut total_consumed_signals = 0usize;

        for case in &cases {
            let decision = variant.detect(case).with_context(|| {
                format!(
                    "variant '{}' failed to detect contradiction for case '{}'",
                    variant.name(),
                    case.id
                )
            })?;
            let bench_average_us = benchmark_variant(variant.as_ref(), case)?;
            let is_correct = decision_matches(case, &decision);
            if is_correct {
                correct += 1;
            }
            total_average_us += bench_average_us;
            total_consumed_signals += decision.consumed_signals;

            case_results.push(ContradictionVariantCaseResult {
                case_id: case.id.clone(),
                correct: is_correct,
                expected_detection_kind: case.expected_detection_kind.clone(),
                selected_detection_kind: decision.detection_kind,
                expected_conflicting_reference: case.expected_conflicting_reference.clone(),
                selected_conflicting_reference: decision.conflicting_reference,
                consumed_signals: decision.consumed_signals,
                average_decision_us: bench_average_us,
            });
        }

        reports.push(ContradictionVariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            cases: case_results,
            accuracy_pct: correct as f64 * 100.0 / cases.len() as f64,
            average_decision_us: total_average_us / cases.len() as f64,
            average_consumed_signals: total_consumed_signals as f64 / cases.len() as f64,
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
        });
    }

    Ok(ContradictionExperimentReport {
        problem: "Local suite frontiers and stable-runtime promotion policy should be compared explicitly so contradictory signals become new experiment work instead of hidden architecture debt.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &ContradictionExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} accuracy_pct={:.1} avg_decision_us={:.2} avg_consumed_signals={:.2} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_consumed_signals,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &ContradictionExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Cross-Suite Contradiction Detection\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("| case | suite | expected detection | conflicting reference | why |\n");
    markdown.push_str("| --- | --- | --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | {} |\n",
            case.id,
            case.suite,
            case.expected_detection_kind,
            case.expected_conflicting_reference
                .as_deref()
                .unwrap_or("none"),
            case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str(
        "| variant | style | accuracy | avg us | avg consumed signals | readability | extensibility |\n",
    );
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | {} | {:.1}% | {:.2} | {:.2} | {} | {} |\n",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_consumed_signals,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &ContradictionExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Cross-Suite Contradiction Detection\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while using the richest contradiction evidence ({:.2} signals).\n",
            reference.name, reference.average_consumed_signals
        ));
    }
    markdown.push_str("- Keep contradiction detection experimental until more promotion and rollback cycles create real cross-suite mismatches.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Cross-Suite Contradiction Detection\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct ContradictionCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub suite: String,\n");
    markdown.push_str("    pub local_frontier: Vec<String>,\n");
    markdown.push_str("    pub provisional_reference: Option<String>,\n");
    markdown.push_str("    pub promotion_decision_kind: String,\n");
    markdown.push_str("    pub promotion_reference: Option<String>,\n");
    markdown.push_str("    pub replay_signal: String,\n");
    markdown.push_str("    pub support_gap: i32,\n");
    markdown.push_str("    pub environment_specific: bool,\n");
    markdown.push_str("    pub runtime_risk: u32,\n");
    markdown.push_str("    pub expected_detection_kind: String,\n");
    markdown.push_str("    pub expected_conflicting_reference: Option<String>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ContradictionDecision {\n");
    markdown.push_str("    pub detection_kind: String,\n");
    markdown.push_str("    pub conflicting_reference: Option<String>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("    pub consumed_signals: usize,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait ContradictionVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn detect(&self, case: &ContradictionCase) -> anyhow::Result<ContradictionDecision>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str("- Shared input: same local-frontier and promotion-policy evidence tuple for every contradiction detector.\n");
    markdown.push_str("- Shared metrics: detection accuracy, average decision time, average consumed signals, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    detection_kind: impl Into<String>,
    conflicting_reference: Option<String>,
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> ContradictionDecision {
    ContradictionDecision {
        detection_kind: detection_kind.into(),
        conflicting_reference,
        rationale: rationale.into(),
        consumed_signals,
    }
}

pub(crate) fn frontier_contains(case: &ContradictionCase, reference: &str) -> bool {
    case.local_frontier.iter().any(|entry| entry == reference)
}

pub(crate) fn has_single_frontier(case: &ContradictionCase) -> bool {
    case.local_frontier.len() == 1
}

fn decision_matches(case: &ContradictionCase, decision: &ContradictionDecision) -> bool {
    decision.detection_kind == case.expected_detection_kind
        && decision.conflicting_reference == case.expected_conflicting_reference
}

fn load_cases(root: &Path) -> Result<Vec<ContradictionCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(variant: &dyn ContradictionVariant, case: &ContradictionCase) -> Result<f64> {
    let _ = variant.detect(case)?;
    let start = Instant::now();
    for _ in 0..BENCH_ITERATIONS {
        let _ = variant.detect(case)?;
    }
    Ok(start.elapsed().as_secs_f64() * 1_000_000.0 / BENCH_ITERATIONS as f64)
}

fn collect_source_metrics(
    root: &Path,
    source_path: &str,
    decision_refs: &[&str],
) -> Result<ContradictionSourceMetrics> {
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
    let branch_tokens = ["if ", "match ", " for ", "while ", "&&", "||", ".iter("]
        .into_iter()
        .map(|token| source.matches(token).count())
        .sum();
    let hardcoded_decision_refs = decision_refs
        .iter()
        .map(|decision| source.matches(&format!("\"{decision}\"")).count())
        .sum();
    let evidence_refs = [
        "frontier_contains(",
        "has_single_frontier(",
        "promotion_decision_kind",
        "promotion_reference",
        "replay_signal",
        "support_gap",
        "environment_specific",
        "runtime_risk",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(ContradictionSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        evidence_refs,
    })
}

fn readability_score(source: &ContradictionSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &ContradictionSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_decision_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.evidence_refs as i32 * 4)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &ContradictionExperimentReport) -> Vec<&ContradictionVariantReport> {
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
    report: &ContradictionExperimentReport,
) -> Option<&ContradictionVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_consumed_signals
            .partial_cmp(&right.average_consumed_signals)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
