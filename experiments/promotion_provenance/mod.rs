use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

mod documented_chain;
mod latest_cut_only;
mod lineage_budget;
mod release_majority;

use documented_chain::DocumentedChainVariant;
use latest_cut_only::LatestCutOnlyVariant;
use lineage_budget::LineageBudgetVariant;
use release_majority::ReleaseMajorityVariant;

const CASES_PATH: &str = "experiments/promotion_provenance/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseCut {
    pub tag: String,
    pub decision_kind: String,
    pub reference: Option<String>,
    pub documented: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionProvenanceCase {
    pub id: String,
    pub suite: String,
    pub current_reference: String,
    pub release_cuts: Vec<ReleaseCut>,
    pub expected_decision_kind: String,
    pub expected_reference: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionProvenanceDecision {
    pub decision_kind: String,
    pub selected_reference: Option<String>,
    pub rationale: String,
    pub consumed_signals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionProvenanceCaseResult {
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
pub struct PromotionProvenanceSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub evidence_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionProvenanceVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: PromotionProvenanceSourceMetrics,
    pub cases: Vec<PromotionProvenanceCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_consumed_signals: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionProvenanceExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<PromotionProvenanceCase>,
    pub variants: Vec<PromotionProvenanceVariantReport>,
}

pub trait PromotionProvenanceVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn decide(&self, case: &PromotionProvenanceCase) -> Result<PromotionProvenanceDecision>;
}

pub fn run_suite(root: &Path) -> Result<PromotionProvenanceExperimentReport> {
    let cases = load_cases(root)?;
    let decision_refs = [
        "provenance_confirmed",
        "provenance_superseded",
        "provenance_gap",
        "provenance_broken",
    ];
    let variants: Vec<Box<dyn PromotionProvenanceVariant>> = vec![
        Box::new(LatestCutOnlyVariant::default()),
        Box::new(DocumentedChainVariant::default()),
        Box::new(ReleaseMajorityVariant::default()),
        Box::new(LineageBudgetVariant::default()),
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
                    "variant '{}' failed to decide provenance for case '{}'",
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

            case_results.push(PromotionProvenanceCaseResult {
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

        reports.push(PromotionProvenanceVariantReport {
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

    Ok(PromotionProvenanceExperimentReport {
        problem: "Promotion history should stay inspectable across release cuts so stable-runtime trust has a documented lineage instead of a single latest-state claim.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &PromotionProvenanceExperimentReport) -> String {
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

pub fn render_experiments_section(report: &PromotionProvenanceExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Promotion Provenance\n\n");
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

pub fn render_decisions_section(report: &PromotionProvenanceExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Promotion Provenance\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while using the richest release-history evidence ({:.2} signals).\n",
            reference.name, reference.average_consumed_signals
        ));
    }
    markdown.push_str("- Keep provenance policy experimental until multiple real release cuts and changelog practices exist in the repo.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Promotion Provenance\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct ReleaseCut {\n");
    markdown.push_str("    pub tag: String,\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub reference: Option<String>,\n");
    markdown.push_str("    pub documented: bool,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct PromotionProvenanceCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub suite: String,\n");
    markdown.push_str("    pub current_reference: String,\n");
    markdown.push_str("    pub release_cuts: Vec<ReleaseCut>,\n");
    markdown.push_str("    pub expected_decision_kind: String,\n");
    markdown.push_str("    pub expected_reference: Option<String>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct PromotionProvenanceDecision {\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub selected_reference: Option<String>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("    pub consumed_signals: usize,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait PromotionProvenanceVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn decide(&self, case: &PromotionProvenanceCase) -> anyhow::Result<PromotionProvenanceDecision>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str("- Shared input: same release-cut lineage for every provenance policy.\n");
    markdown.push_str("- Shared metrics: decision accuracy, average decision time, average consumed signals, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    decision_kind: impl Into<String>,
    selected_reference: Option<String>,
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> PromotionProvenanceDecision {
    PromotionProvenanceDecision {
        decision_kind: decision_kind.into(),
        selected_reference,
        rationale: rationale.into(),
        consumed_signals,
    }
}

pub(crate) fn latest_cut(case: &PromotionProvenanceCase) -> Option<&ReleaseCut> {
    case.release_cuts.last()
}

pub(crate) fn has_rollback(case: &PromotionProvenanceCase) -> bool {
    case.release_cuts
        .iter()
        .any(|cut| cut.decision_kind == "rollback_reference")
}

pub(crate) fn has_replace(case: &PromotionProvenanceCase) -> bool {
    case.release_cuts
        .iter()
        .any(|cut| cut.decision_kind == "replace_reference")
}

pub(crate) fn all_documented(case: &PromotionProvenanceCase) -> bool {
    case.release_cuts.iter().all(|cut| cut.documented)
}

pub(crate) fn missing_documentation_count(case: &PromotionProvenanceCase) -> usize {
    case.release_cuts
        .iter()
        .filter(|cut| !cut.documented)
        .count()
}

pub(crate) fn origin_reference(case: &PromotionProvenanceCase) -> Option<String> {
    case.release_cuts
        .iter()
        .find_map(|cut| cut.reference.clone())
}

pub(crate) fn documented_same_reference_chain(case: &PromotionProvenanceCase) -> bool {
    all_documented(case)
        && !has_rollback(case)
        && case.release_cuts.iter().all(|cut| {
            cut.reference.as_deref() == Some(case.current_reference.as_str())
                && cut.decision_kind != "replace_reference"
        })
}

pub(crate) fn documented_replace_chain_to_current(case: &PromotionProvenanceCase) -> bool {
    all_documented(case)
        && !has_rollback(case)
        && has_replace(case)
        && latest_cut(case).and_then(|cut| cut.reference.as_deref())
            == Some(case.current_reference.as_str())
}

pub(crate) fn majority_reference(case: &PromotionProvenanceCase) -> Option<String> {
    let mut counts = BTreeMap::<String, usize>::new();
    for cut in &case.release_cuts {
        if let Some(reference) = &cut.reference {
            *counts.entry(reference.clone()).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(reference, _)| reference)
}

pub(crate) fn reference_count(case: &PromotionProvenanceCase, reference: &str) -> usize {
    case.release_cuts
        .iter()
        .filter(|cut| cut.reference.as_deref() == Some(reference))
        .count()
}

fn decision_matches(
    case: &PromotionProvenanceCase,
    decision: &PromotionProvenanceDecision,
) -> bool {
    decision.decision_kind == case.expected_decision_kind
        && decision.selected_reference == case.expected_reference
}

fn load_cases(root: &Path) -> Result<Vec<PromotionProvenanceCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn PromotionProvenanceVariant,
    case: &PromotionProvenanceCase,
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
) -> Result<PromotionProvenanceSourceMetrics> {
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
        "latest_cut(",
        "has_rollback(",
        "has_replace(",
        "all_documented(",
        "missing_documentation_count(",
        "origin_reference(",
        "documented_replace_chain_to_current(",
        "majority_reference(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(PromotionProvenanceSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        evidence_refs,
    })
}

fn readability_score(source: &PromotionProvenanceSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &PromotionProvenanceSourceMetrics) -> u32 {
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
    report: &PromotionProvenanceExperimentReport,
) -> Vec<&PromotionProvenanceVariantReport> {
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
    report: &PromotionProvenanceExperimentReport,
) -> Option<&PromotionProvenanceVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_consumed_signals
            .partial_cmp(&right.average_consumed_signals)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
