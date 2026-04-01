use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod adjacent_pending_reuse;
mod carryover_budget;
mod latest_train_only;
mod surface_chain_vote;

use adjacent_pending_reuse::AdjacentPendingReuseVariant;
use carryover_budget::CarryoverBudgetVariant;
use latest_train_only::LatestTrainOnlyVariant;
use surface_chain_vote::SurfaceChainVoteVariant;

const CASES_PATH: &str = "experiments/cross_train_lag_carryover/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarryoverSurfaceSignal {
    pub surface: String,
    pub decision_kind: String,
    pub reference: Option<String>,
    pub confidence: f64,
    pub lag_hours: u32,
    pub allowed_lag_hours: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CarryoverReleaseTrain {
    pub train: String,
    pub signals: Vec<CarryoverSurfaceSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossTrainLagCase {
    pub id: String,
    pub suite: String,
    pub current_reference: String,
    pub release_trains: Vec<CarryoverReleaseTrain>,
    pub expected_decision_kind: String,
    pub expected_reference: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossTrainLagDecision {
    pub decision_kind: String,
    pub selected_reference: Option<String>,
    pub rationale: String,
    pub consumed_signals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossTrainLagCaseResult {
    pub case_id: String,
    pub correct: bool,
    pub expected_decision_kind: String,
    pub selected_decision_kind: String,
    pub expected_reference: Option<String>,
    pub selected_reference: Option<String>,
    pub consumed_signals: usize,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossTrainLagSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub evidence_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossTrainLagVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: CrossTrainLagSourceMetrics,
    pub cases: Vec<CrossTrainLagCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_consumed_signals: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossTrainLagExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<CrossTrainLagCase>,
    pub variants: Vec<CrossTrainLagVariantReport>,
}

#[derive(Debug, Clone)]
pub(crate) struct TrainCarryoverState {
    pub kind: String,
    pub reference: Option<String>,
    pub current_within: f64,
    pub current_overdue: f64,
    pub challenger_score: f64,
    pub challenger_surfaces: usize,
    pub challenger_critical: usize,
    pub rollback_score: f64,
    pub rollback_critical: usize,
}

pub trait CrossTrainLagVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn decide(&self, case: &CrossTrainLagCase) -> Result<CrossTrainLagDecision>;
}

pub fn run_suite(root: &Path) -> Result<CrossTrainLagExperimentReport> {
    let cases = load_cases(root)?;
    let decision_refs = [
        "carryover_confirmed",
        "carryover_superseded",
        "carryover_pending",
        "carryover_blocked",
    ];
    let variants: Vec<Box<dyn CrossTrainLagVariant>> = vec![
        Box::new(LatestTrainOnlyVariant::default()),
        Box::new(AdjacentPendingReuseVariant::default()),
        Box::new(SurfaceChainVoteVariant::default()),
        Box::new(CarryoverBudgetVariant::default()),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &decision_refs)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;
        let mut total_consumed_signals = 0usize;

        for case in &cases {
            let decision = variant.decide(case).with_context(|| {
                format!(
                    "variant '{}' failed to decide cross-train lag carryover for case '{}'",
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

            case_results.push(CrossTrainLagCaseResult {
                case_id: case.id.clone(),
                correct: is_correct,
                expected_decision_kind: case.expected_decision_kind.clone(),
                selected_decision_kind: decision.decision_kind,
                expected_reference: case.expected_reference.clone(),
                selected_reference: decision.selected_reference,
                consumed_signals: decision.consumed_signals,
                average_decision_us: bench_average_us,
            });
        }

        reports.push(CrossTrainLagVariantReport {
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

    Ok(CrossTrainLagExperimentReport {
        problem: "Unresolved publication lag should carry across release cuts only through explicit budgets, instead of treating every new train as either a full reset or a full continuation.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &CrossTrainLagExperimentReport) -> String {
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

pub fn render_experiments_section(report: &CrossTrainLagExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Cross-Train Lag Carryover\n\n");
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

pub fn render_decisions_section(report: &CrossTrainLagExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Cross-Train Lag Carryover\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while using the richest carryover-evidence window ({:.2} signals).\n",
            reference.name, reference.average_consumed_signals
        ));
    }
    markdown.push_str("- Keep cross-train carryover policy experimental until the repo has real multi-cut publication traces with unresolved lag debt.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Cross-Train Lag Carryover\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct CarryoverSurfaceSignal {\n");
    markdown.push_str("    pub surface: String,\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub reference: Option<String>,\n");
    markdown.push_str("    pub confidence: f64,\n");
    markdown.push_str("    pub lag_hours: u32,\n");
    markdown.push_str("    pub allowed_lag_hours: u32,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct CarryoverReleaseTrain {\n");
    markdown.push_str("    pub train: String,\n");
    markdown.push_str("    pub signals: Vec<CarryoverSurfaceSignal>,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct CrossTrainLagCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub suite: String,\n");
    markdown.push_str("    pub current_reference: String,\n");
    markdown.push_str("    pub release_trains: Vec<CarryoverReleaseTrain>,\n");
    markdown.push_str("    pub expected_decision_kind: String,\n");
    markdown.push_str("    pub expected_reference: Option<String>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct CrossTrainLagDecision {\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub selected_reference: Option<String>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("    pub consumed_signals: usize,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait CrossTrainLagVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn decide(&self, case: &CrossTrainLagCase) -> anyhow::Result<CrossTrainLagDecision>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown
        .push_str("- Shared input: same multi-train lag snapshots for every carryover policy.\n");
    markdown.push_str("- Shared metrics: decision accuracy, average decision time, average consumed signals, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    decision_kind: impl Into<String>,
    selected_reference: Option<String>,
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> CrossTrainLagDecision {
    CrossTrainLagDecision {
        decision_kind: decision_kind.into(),
        selected_reference,
        rationale: rationale.into(),
        consumed_signals,
    }
}

pub(crate) fn latest_train(case: &CrossTrainLagCase) -> Option<&CarryoverReleaseTrain> {
    case.release_trains.last()
}

pub(crate) fn is_within_budget(signal: &CarryoverSurfaceSignal) -> bool {
    signal.lag_hours <= signal.allowed_lag_hours
}

pub(crate) fn is_critical_surface(surface: &str) -> bool {
    matches!(surface, "package_registry" | "release_feed")
}

pub(crate) fn surface_weight(surface: &str) -> f64 {
    match surface {
        "package_registry" => 6.0,
        "release_feed" => 5.0,
        "docs_portal" => 4.0,
        "api_docs" => 4.0,
        _ => 3.0,
    }
}

pub(crate) fn signal_score(signal: &CarryoverSurfaceSignal) -> f64 {
    surface_weight(&signal.surface) * signal.confidence.max(0.0)
}

pub(crate) fn current_within_budget_support(
    train: &CarryoverReleaseTrain,
    current_reference: &str,
    min_confidence: f64,
) -> f64 {
    train
        .signals
        .iter()
        .filter(|signal| signal.confidence >= min_confidence)
        .filter(|signal| signal.decision_kind != "rollback_reference")
        .filter(|signal| signal.reference.as_deref() == Some(current_reference))
        .filter(|signal| is_within_budget(signal))
        .map(signal_score)
        .sum()
}

pub(crate) fn current_overdue_support(
    train: &CarryoverReleaseTrain,
    current_reference: &str,
    min_confidence: f64,
) -> f64 {
    train
        .signals
        .iter()
        .filter(|signal| signal.confidence >= min_confidence)
        .filter(|signal| signal.decision_kind != "rollback_reference")
        .filter(|signal| signal.reference.as_deref() == Some(current_reference))
        .filter(|signal| !is_within_budget(signal))
        .map(signal_score)
        .sum()
}

pub(crate) fn best_challenger_support(
    train: &CarryoverReleaseTrain,
    current_reference: &str,
    min_confidence: f64,
) -> Option<(String, f64, usize, usize)> {
    let mut scores = BTreeMap::<String, (f64, BTreeSet<String>, BTreeSet<String>)>::new();
    for signal in &train.signals {
        if signal.confidence < min_confidence || signal.decision_kind == "rollback_reference" {
            continue;
        }
        let Some(reference) = &signal.reference else {
            continue;
        };
        if reference == current_reference {
            continue;
        }
        let entry = scores
            .entry(reference.clone())
            .or_insert_with(|| (0.0, BTreeSet::new(), BTreeSet::new()));
        entry.0 += signal_score(signal);
        entry.1.insert(signal.surface.clone());
        if is_critical_surface(&signal.surface) {
            entry.2.insert(signal.surface.clone());
        }
    }
    scores
        .into_iter()
        .max_by(|left, right| {
            left.1
                 .0
                .partial_cmp(&right.1 .0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.1 .2.len().cmp(&right.1 .2.len()))
                .then_with(|| left.1 .1.len().cmp(&right.1 .1.len()))
        })
        .map(|(reference, (score, surfaces, critical))| {
            (reference, score, surfaces.len(), critical.len())
        })
}

pub(crate) fn rollback_support(train: &CarryoverReleaseTrain, min_confidence: f64) -> (f64, usize) {
    let mut score = 0.0;
    let mut critical = BTreeSet::new();
    for signal in &train.signals {
        if signal.confidence < min_confidence || signal.decision_kind != "rollback_reference" {
            continue;
        }
        score += signal_score(signal);
        if is_critical_surface(&signal.surface) {
            critical.insert(signal.surface.clone());
        }
    }
    (score, critical.len())
}

pub(crate) fn train_state(
    train: &CarryoverReleaseTrain,
    current_reference: &str,
    min_confidence: f64,
    current_confirm_threshold: f64,
    challenger_supersede_threshold: f64,
    rollback_block_threshold: f64,
    low_challenger_threshold: f64,
) -> TrainCarryoverState {
    let current_within = current_within_budget_support(train, current_reference, min_confidence);
    let current_overdue = current_overdue_support(train, current_reference, min_confidence);
    let challenger = best_challenger_support(train, current_reference, min_confidence);
    let challenger_score = challenger
        .as_ref()
        .map(|(_, score, _, _)| *score)
        .unwrap_or(0.0);
    let challenger_surfaces = challenger
        .as_ref()
        .map(|(_, _, surfaces, _)| *surfaces)
        .unwrap_or(0);
    let challenger_critical = challenger
        .as_ref()
        .map(|(_, _, _, critical)| *critical)
        .unwrap_or(0);
    let (rollback_score, rollback_critical) = rollback_support(train, min_confidence);

    let (kind, reference) = if rollback_score >= rollback_block_threshold && rollback_critical >= 1
    {
        ("blocked".to_string(), None)
    } else if challenger_score >= challenger_supersede_threshold
        && challenger_surfaces >= 2
        && (challenger_critical >= 2 || current_overdue >= 4.5)
    {
        (
            "superseded".to_string(),
            challenger.map(|(reference, _, _, _)| reference),
        )
    } else if current_within >= current_confirm_threshold
        && challenger_score <= low_challenger_threshold
        && rollback_score < 1.0
    {
        ("confirmed".to_string(), Some(current_reference.to_string()))
    } else {
        (
            "pending".to_string(),
            challenger.map(|(reference, _, _, _)| reference),
        )
    };

    TrainCarryoverState {
        kind,
        reference,
        current_within,
        current_overdue,
        challenger_score,
        challenger_surfaces,
        challenger_critical,
        rollback_score,
        rollback_critical,
    }
}

pub(crate) fn collect_train_states(
    case: &CrossTrainLagCase,
    min_confidence: f64,
    current_confirm_threshold: f64,
    challenger_supersede_threshold: f64,
    rollback_block_threshold: f64,
    low_challenger_threshold: f64,
) -> Vec<TrainCarryoverState> {
    case.release_trains
        .iter()
        .map(|train| {
            train_state(
                train,
                &case.current_reference,
                min_confidence,
                current_confirm_threshold,
                challenger_supersede_threshold,
                rollback_block_threshold,
                low_challenger_threshold,
            )
        })
        .collect()
}

pub(crate) fn consecutive_pending_chain(
    states: &[TrainCarryoverState],
) -> Vec<TrainCarryoverState> {
    let mut chain = Vec::new();
    for state in states.iter().rev() {
        if state.kind == "pending" {
            chain.push(state.clone());
        } else {
            break;
        }
    }
    chain.reverse();
    chain
}

fn decision_matches(case: &CrossTrainLagCase, decision: &CrossTrainLagDecision) -> bool {
    decision.decision_kind == case.expected_decision_kind
        && decision.selected_reference == case.expected_reference
}

fn load_cases(root: &Path) -> Result<Vec<CrossTrainLagCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(variant: &dyn CrossTrainLagVariant, case: &CrossTrainLagCase) -> Result<f64> {
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
) -> Result<CrossTrainLagSourceMetrics> {
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
        "current_within_budget_support(",
        "current_overdue_support(",
        "best_challenger_support(",
        "rollback_support(",
        "train_state(",
        "collect_train_states(",
        "consecutive_pending_chain(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(CrossTrainLagSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        evidence_refs,
    })
}

fn readability_score(source: &CrossTrainLagSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &CrossTrainLagSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_decision_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.evidence_refs as i32 * 4)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &CrossTrainLagExperimentReport) -> Vec<&CrossTrainLagVariantReport> {
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
    report: &CrossTrainLagExperimentReport,
) -> Option<&CrossTrainLagVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_consumed_signals
            .partial_cmp(&right.average_consumed_signals)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
