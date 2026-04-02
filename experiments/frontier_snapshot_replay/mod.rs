use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod latest_snapshot_only;
mod provider_majority;
mod strict_consensus_gate;
mod weighted_stability;

use latest_snapshot_only::LatestSnapshotOnlyVariant;
use provider_majority::ProviderMajorityVariant;
use strict_consensus_gate::StrictConsensusGateVariant;
use weighted_stability::WeightedStabilityVariant;

const CASES_PATH: &str = "experiments/frontier_snapshot_replay/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEvidence {
    pub provider: String,
    pub model: String,
    pub days_ago: u32,
    pub frontier_accuracy: f64,
    pub rival_accuracy: f64,
    pub comparable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierReplayCase {
    pub id: String,
    pub suite: String,
    pub frontier_candidate: String,
    pub rival_candidate: String,
    pub snapshots: Vec<SnapshotEvidence>,
    pub expected_decision_kind: String,
    pub expected_reference: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierReplayDecision {
    pub decision_kind: String,
    pub selected_reference: Option<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierReplayCaseResult {
    pub case_id: String,
    pub correct: bool,
    pub expected_decision_kind: String,
    pub selected_decision_kind: String,
    pub expected_reference: Option<String>,
    pub selected_reference: Option<String>,
    pub evidence_count: usize,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierReplaySourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub snapshot_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierReplayVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: FrontierReplaySourceMetrics,
    pub cases: Vec<FrontierReplayCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_evidence_count: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontierReplayExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<FrontierReplayCase>,
    pub variants: Vec<FrontierReplayVariantReport>,
}

pub trait FrontierReplayVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn decide(&self, case: &FrontierReplayCase) -> Result<FrontierReplayDecision>;
}

pub fn run_suite(root: &Path) -> Result<FrontierReplayExperimentReport> {
    let cases = load_cases(root)?;
    let decision_refs = ["promote_reference", "hold_experimental", "switch_reference"];
    let variants: Vec<Box<dyn FrontierReplayVariant>> = vec![
        Box::new(LatestSnapshotOnlyVariant::default()),
        Box::new(ProviderMajorityVariant::default()),
        Box::new(StrictConsensusGateVariant::default()),
        Box::new(WeightedStabilityVariant::default()),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &decision_refs)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;
        let mut total_evidence_count = 0usize;

        for case in &cases {
            let decision = variant.decide(case).with_context(|| {
                format!(
                    "variant '{}' failed to decide for case '{}'",
                    variant.name(),
                    case.id
                )
            })?;
            let bench_average_us = benchmark_variant(variant.as_ref(), case)?;
            let evidence_count = comparable_snapshots(case).len();
            let is_correct = decision_matches(case, &decision);
            if is_correct {
                correct += 1;
            }
            total_average_us += bench_average_us;
            total_evidence_count += evidence_count;

            case_results.push(FrontierReplayCaseResult {
                case_id: case.id.clone(),
                correct: is_correct,
                expected_decision_kind: case.expected_decision_kind.clone(),
                selected_decision_kind: decision.decision_kind,
                expected_reference: case.expected_reference.clone(),
                selected_reference: decision.selected_reference,
                evidence_count,
                average_decision_us: bench_average_us,
            });
        }

        reports.push(FrontierReplayVariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            cases: case_results,
            accuracy_pct: correct as f64 * 100.0 / cases.len() as f64,
            average_decision_us: total_average_us / cases.len() as f64,
            average_evidence_count: total_evidence_count as f64 / cases.len() as f64,
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
        });
    }

    Ok(FrontierReplayExperimentReport {
        problem: "Frontier promotion should replay versioned provider/model snapshots instead of depending on a single current validation run.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &FrontierReplayExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} accuracy_pct={:.1} avg_decision_us={:.2} avg_evidence_count={:.2} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_evidence_count,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &FrontierReplayExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Frontier Snapshot Replay\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("| case | suite | expected decision | expected reference | why |\n");
    markdown.push_str("| --- | --- | --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | {} |\n",
            case.id,
            case.suite,
            case.expected_decision_kind,
            case.expected_reference.as_deref().unwrap_or("none"),
            case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str(
        "| variant | style | accuracy | avg us | avg evidence count | readability | extensibility |\n",
    );
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | {} | {:.1}% | {:.2} | {:.2} | {} | {} |\n",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_evidence_count,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &FrontierReplayExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Frontier Snapshot Replay\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while using the broadest evidence window ({:.2} snapshots).\n",
            reference.name, reference.average_evidence_count
        ));
    }
    markdown.push_str("- Keep promotion rules experimental until live provider validation produces real snapshot histories for more than one environment.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Frontier Snapshot Replay\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct SnapshotEvidence {\n");
    markdown.push_str("    pub provider: String,\n");
    markdown.push_str("    pub model: String,\n");
    markdown.push_str("    pub days_ago: u32,\n");
    markdown.push_str("    pub frontier_accuracy: f64,\n");
    markdown.push_str("    pub rival_accuracy: f64,\n");
    markdown.push_str("    pub comparable: bool,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct FrontierReplayCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub suite: String,\n");
    markdown.push_str("    pub frontier_candidate: String,\n");
    markdown.push_str("    pub rival_candidate: String,\n");
    markdown.push_str("    pub snapshots: Vec<SnapshotEvidence>,\n");
    markdown.push_str("    pub expected_decision_kind: String,\n");
    markdown.push_str("    pub expected_reference: Option<String>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct FrontierReplayDecision {\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub selected_reference: Option<String>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait FrontierReplayVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str("    fn decide(&self, case: &FrontierReplayCase) -> anyhow::Result<FrontierReplayDecision>;\n");
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str(
        "- Shared input: same versioned provider/model snapshot list for all replay variants.\n",
    );
    markdown.push_str("- Shared metrics: decision accuracy, average decision time, average evidence count, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    decision_kind: impl Into<String>,
    selected_reference: Option<String>,
    rationale: impl Into<String>,
) -> FrontierReplayDecision {
    FrontierReplayDecision {
        decision_kind: decision_kind.into(),
        selected_reference,
        rationale: rationale.into(),
    }
}

pub(crate) fn comparable_snapshots(case: &FrontierReplayCase) -> Vec<&SnapshotEvidence> {
    case.snapshots
        .iter()
        .filter(|snapshot| snapshot.comparable)
        .collect()
}

pub(crate) fn latest_snapshot(case: &FrontierReplayCase) -> Option<&SnapshotEvidence> {
    comparable_snapshots(case)
        .into_iter()
        .min_by_key(|snapshot| snapshot.days_ago)
}

pub(crate) fn latest_per_provider(case: &FrontierReplayCase) -> Vec<&SnapshotEvidence> {
    let mut snapshots = comparable_snapshots(case);
    snapshots.sort_by_key(|snapshot| snapshot.days_ago);
    let mut latest = Vec::new();
    for snapshot in snapshots {
        if latest
            .iter()
            .all(|existing: &&SnapshotEvidence| existing.provider != snapshot.provider)
        {
            latest.push(snapshot);
        }
    }
    latest
}

pub(crate) fn winner(snapshot: &SnapshotEvidence, margin: f64) -> &'static str {
    let delta = snapshot.frontier_accuracy - snapshot.rival_accuracy;
    if delta >= margin {
        "frontier"
    } else if delta <= -margin {
        "rival"
    } else {
        "tie"
    }
}

fn decision_matches(case: &FrontierReplayCase, decision: &FrontierReplayDecision) -> bool {
    decision.decision_kind == case.expected_decision_kind
        && decision.selected_reference == case.expected_reference
}

fn load_cases(root: &Path) -> Result<Vec<FrontierReplayCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn FrontierReplayVariant,
    case: &FrontierReplayCase,
) -> Result<f64> {
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
    decision_refs: &[&str],
) -> Result<FrontierReplaySourceMetrics> {
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
    let snapshot_refs = [
        "comparable_snapshots(",
        "latest_snapshot(",
        "latest_per_provider(",
        "winner(",
        "days_ago",
        "frontier_accuracy",
        "rival_accuracy",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(FrontierReplaySourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        snapshot_refs,
    })
}

fn readability_score(source: &FrontierReplaySourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &FrontierReplaySourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_decision_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.snapshot_refs as i32 * 6)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &FrontierReplayExperimentReport) -> Vec<&FrontierReplayVariantReport> {
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
    report: &FrontierReplayExperimentReport,
) -> Option<&FrontierReplayVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_evidence_count
            .partial_cmp(&right.average_evidence_count)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
