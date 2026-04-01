use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

mod critical_environment_gate;
mod documented_majority;
mod environment_lineage_budget;
mod latest_environment_only;

use critical_environment_gate::CriticalEnvironmentGateVariant;
use documented_majority::DocumentedMajorityVariant;
use environment_lineage_budget::EnvironmentLineageBudgetVariant;
use latest_environment_only::LatestEnvironmentOnlyVariant;

const CASES_PATH: &str = "experiments/promotion_environment_provenance/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentSnapshot {
    pub environment: String,
    pub decision_kind: String,
    pub reference: Option<String>,
    pub documented: bool,
    pub critical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionEnvironmentCase {
    pub id: String,
    pub suite: String,
    pub current_reference: String,
    pub environment_snapshots: Vec<EnvironmentSnapshot>,
    pub expected_decision_kind: String,
    pub expected_reference: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionEnvironmentDecision {
    pub decision_kind: String,
    pub selected_reference: Option<String>,
    pub rationale: String,
    pub consumed_signals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionEnvironmentCaseResult {
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
pub struct PromotionEnvironmentSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub evidence_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionEnvironmentVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: PromotionEnvironmentSourceMetrics,
    pub cases: Vec<PromotionEnvironmentCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_consumed_signals: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionEnvironmentExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<PromotionEnvironmentCase>,
    pub variants: Vec<PromotionEnvironmentVariantReport>,
}

pub trait PromotionEnvironmentVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn decide(&self, case: &PromotionEnvironmentCase) -> Result<PromotionEnvironmentDecision>;
}

pub fn run_suite(root: &Path) -> Result<PromotionEnvironmentExperimentReport> {
    let cases = load_cases(root)?;
    let decision_refs = [
        "environment_confirmed",
        "environment_superseded",
        "environment_gap",
        "environment_blocked",
    ];
    let variants: Vec<Box<dyn PromotionEnvironmentVariant>> = vec![
        Box::new(LatestEnvironmentOnlyVariant::default()),
        Box::new(DocumentedMajorityVariant::default()),
        Box::new(CriticalEnvironmentGateVariant::default()),
        Box::new(EnvironmentLineageBudgetVariant::default()),
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
                    "variant '{}' failed to decide environment provenance for case '{}'",
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

            case_results.push(PromotionEnvironmentCaseResult {
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

        reports.push(PromotionEnvironmentVariantReport {
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

    Ok(PromotionEnvironmentExperimentReport {
        problem: "Promotion provenance should stay coherent across deployment environments, not just across release cuts, before stable-runtime trust is increased.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &PromotionEnvironmentExperimentReport) -> String {
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

pub fn render_experiments_section(report: &PromotionEnvironmentExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Promotion Environment Provenance\n\n");
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

pub fn render_decisions_section(report: &PromotionEnvironmentExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Promotion Environment Provenance\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while using the richest environment-evidence window ({:.2} signals).\n",
            reference.name, reference.average_consumed_signals
        ));
    }
    markdown.push_str("- Keep deployment-environment provenance experimental until the repo has real staged rollout records beyond synthetic cases.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Promotion Environment Provenance\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct EnvironmentSnapshot {\n");
    markdown.push_str("    pub environment: String,\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub reference: Option<String>,\n");
    markdown.push_str("    pub documented: bool,\n");
    markdown.push_str("    pub critical: bool,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct PromotionEnvironmentCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub suite: String,\n");
    markdown.push_str("    pub current_reference: String,\n");
    markdown.push_str("    pub environment_snapshots: Vec<EnvironmentSnapshot>,\n");
    markdown.push_str("    pub expected_decision_kind: String,\n");
    markdown.push_str("    pub expected_reference: Option<String>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct PromotionEnvironmentDecision {\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub selected_reference: Option<String>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("    pub consumed_signals: usize,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait PromotionEnvironmentVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn decide(&self, case: &PromotionEnvironmentCase) -> anyhow::Result<PromotionEnvironmentDecision>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str(
        "- Shared input: same environment snapshot set for every rollout-provenance policy.\n",
    );
    markdown.push_str("- Shared metrics: decision accuracy, average decision time, average consumed signals, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    decision_kind: impl Into<String>,
    selected_reference: Option<String>,
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> PromotionEnvironmentDecision {
    PromotionEnvironmentDecision {
        decision_kind: decision_kind.into(),
        selected_reference,
        rationale: rationale.into(),
        consumed_signals,
    }
}

pub(crate) fn latest_environment(case: &PromotionEnvironmentCase) -> Option<&EnvironmentSnapshot> {
    case.environment_snapshots.last()
}

pub(crate) fn documented_coverage(case: &PromotionEnvironmentCase) -> f64 {
    if case.environment_snapshots.is_empty() {
        return 0.0;
    }
    case.environment_snapshots
        .iter()
        .filter(|snapshot| snapshot.documented)
        .count() as f64
        / case.environment_snapshots.len() as f64
}

pub(crate) fn has_any_documentation_gap(case: &PromotionEnvironmentCase) -> bool {
    case.environment_snapshots
        .iter()
        .any(|snapshot| !snapshot.documented)
}

pub(crate) fn has_blocking_environment(case: &PromotionEnvironmentCase) -> bool {
    case.environment_snapshots
        .iter()
        .any(|snapshot| snapshot.decision_kind == "rollback_reference")
}

pub(crate) fn blocking_environment_count(case: &PromotionEnvironmentCase) -> usize {
    case.environment_snapshots
        .iter()
        .filter(|snapshot| snapshot.decision_kind == "rollback_reference")
        .count()
}

pub(crate) fn has_blocking_critical_environment(case: &PromotionEnvironmentCase) -> bool {
    case.environment_snapshots
        .iter()
        .any(|snapshot| snapshot.critical && snapshot.decision_kind == "rollback_reference")
}

pub(crate) fn all_critical_documented(case: &PromotionEnvironmentCase) -> bool {
    case.environment_snapshots
        .iter()
        .filter(|snapshot| snapshot.critical)
        .all(|snapshot| snapshot.documented)
}

pub(crate) fn documented_majority_reference(case: &PromotionEnvironmentCase) -> Option<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for snapshot in &case.environment_snapshots {
        if snapshot.documented && snapshot.decision_kind != "rollback_reference" {
            if let Some(reference) = &snapshot.reference {
                *counts.entry(reference.clone()).or_default() += 1;
            }
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(reference, _)| reference)
}

pub(crate) fn critical_reference_consensus(case: &PromotionEnvironmentCase) -> Option<String> {
    let references = case
        .environment_snapshots
        .iter()
        .filter(|snapshot| snapshot.critical && snapshot.documented)
        .filter_map(|snapshot| snapshot.reference.clone())
        .collect::<Vec<_>>();
    if references.is_empty() {
        return None;
    }
    let first = references.first()?.clone();
    if references.iter().all(|reference| reference == &first) {
        Some(first)
    } else {
        None
    }
}

pub(crate) fn challenger_dominance(case: &PromotionEnvironmentCase) -> Option<(String, i32)> {
    let mut counts = BTreeMap::<String, usize>::new();
    for snapshot in &case.environment_snapshots {
        if snapshot.decision_kind != "rollback_reference" {
            if let Some(reference) = &snapshot.reference {
                *counts.entry(reference.clone()).or_default() += 1;
            }
        }
    }
    let current_count = counts.get(&case.current_reference).copied().unwrap_or(0) as i32;
    let best = counts.into_iter().max_by_key(|(_, count)| *count)?;
    Some((best.0, best.1 as i32 - current_count))
}

fn decision_matches(
    case: &PromotionEnvironmentCase,
    decision: &PromotionEnvironmentDecision,
) -> bool {
    decision.decision_kind == case.expected_decision_kind
        && decision.selected_reference == case.expected_reference
}

fn load_cases(root: &Path) -> Result<Vec<PromotionEnvironmentCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn PromotionEnvironmentVariant,
    case: &PromotionEnvironmentCase,
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
) -> Result<PromotionEnvironmentSourceMetrics> {
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
        "latest_environment(",
        "documented_coverage(",
        "has_any_documentation_gap(",
        "has_blocking_environment(",
        "has_blocking_critical_environment(",
        "all_critical_documented(",
        "documented_majority_reference(",
        "critical_reference_consensus(",
        "challenger_dominance(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(PromotionEnvironmentSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        evidence_refs,
    })
}

fn readability_score(source: &PromotionEnvironmentSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &PromotionEnvironmentSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_decision_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.evidence_refs as i32 * 4)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(
    report: &PromotionEnvironmentExperimentReport,
) -> Vec<&PromotionEnvironmentVariantReport> {
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
    report: &PromotionEnvironmentExperimentReport,
) -> Option<&PromotionEnvironmentVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_consumed_signals
            .partial_cmp(&right.average_consumed_signals)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
