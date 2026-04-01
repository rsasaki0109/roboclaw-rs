use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod latest_train_only;
mod per_train_majority;
mod static_source_priority;
mod trust_decay_budget;

use latest_train_only::LatestTrainOnlyVariant;
use per_train_majority::PerTrainMajorityVariant;
use static_source_priority::StaticSourcePriorityVariant;
use trust_decay_budget::TrustDecayBudgetVariant;

const CASES_PATH: &str = "experiments/artifact_trust_decay/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainArtifact {
    pub source: String,
    pub decision_kind: String,
    pub reference: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseTrain {
    pub train: String,
    pub artifacts: Vec<TrainArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactTrustCase {
    pub id: String,
    pub suite: String,
    pub current_reference: String,
    pub release_trains: Vec<ReleaseTrain>,
    pub expected_decision_kind: String,
    pub expected_reference: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactTrustDecision {
    pub decision_kind: String,
    pub selected_reference: Option<String>,
    pub rationale: String,
    pub consumed_signals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactTrustCaseResult {
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
pub struct ArtifactTrustSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub evidence_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactTrustVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: ArtifactTrustSourceMetrics,
    pub cases: Vec<ArtifactTrustCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_consumed_signals: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactTrustExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<ArtifactTrustCase>,
    pub variants: Vec<ArtifactTrustVariantReport>,
}

pub trait ArtifactTrustVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn decide(&self, case: &ArtifactTrustCase) -> Result<ArtifactTrustDecision>;
}

pub fn run_suite(root: &Path) -> Result<ArtifactTrustExperimentReport> {
    let cases = load_cases(root)?;
    let decision_refs = [
        "trust_confirmed",
        "trust_superseded",
        "trust_decay",
        "trust_rejected",
    ];
    let variants: Vec<Box<dyn ArtifactTrustVariant>> = vec![
        Box::new(LatestTrainOnlyVariant::default()),
        Box::new(StaticSourcePriorityVariant::default()),
        Box::new(PerTrainMajorityVariant::default()),
        Box::new(TrustDecayBudgetVariant::default()),
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
                    "variant '{}' failed to decide artifact trust for case '{}'",
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

            case_results.push(ArtifactTrustCaseResult {
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

        reports.push(ArtifactTrustVariantReport {
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

    Ok(ArtifactTrustExperimentReport {
        problem: "Artifact trust should decay across release trains when changelog, release notes, and rollback notes disagree, instead of treating every artifact mention as equally stable forever.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &ArtifactTrustExperimentReport) -> String {
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

pub fn render_experiments_section(report: &ArtifactTrustExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Artifact Trust Decay\n\n");
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

pub fn render_decisions_section(report: &ArtifactTrustExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Artifact Trust Decay\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while using the richest cross-train evidence window ({:.2} signals).\n",
            reference.name, reference.average_consumed_signals
        ));
    }
    markdown.push_str("- Keep artifact trust policy experimental until the repo has real release-train disagreements and rollback-note history.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Artifact Trust Decay\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct TrainArtifact {\n");
    markdown.push_str("    pub source: String,\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub reference: Option<String>,\n");
    markdown.push_str("    pub confidence: f64,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ReleaseTrain {\n");
    markdown.push_str("    pub train: String,\n");
    markdown.push_str("    pub artifacts: Vec<TrainArtifact>,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ArtifactTrustCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub suite: String,\n");
    markdown.push_str("    pub current_reference: String,\n");
    markdown.push_str("    pub release_trains: Vec<ReleaseTrain>,\n");
    markdown.push_str("    pub expected_decision_kind: String,\n");
    markdown.push_str("    pub expected_reference: Option<String>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ArtifactTrustDecision {\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub selected_reference: Option<String>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("    pub consumed_signals: usize,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait ArtifactTrustVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn decide(&self, case: &ArtifactTrustCase) -> anyhow::Result<ArtifactTrustDecision>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown
        .push_str("- Shared input: same per-train artifact set for every trust-decay policy.\n");
    markdown.push_str("- Shared metrics: decision accuracy, average decision time, average consumed signals, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    decision_kind: impl Into<String>,
    selected_reference: Option<String>,
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> ArtifactTrustDecision {
    ArtifactTrustDecision {
        decision_kind: decision_kind.into(),
        selected_reference,
        rationale: rationale.into(),
        consumed_signals,
    }
}

pub(crate) fn latest_train(case: &ArtifactTrustCase) -> Option<&ReleaseTrain> {
    case.release_trains.last()
}

pub(crate) fn source_weight(source: &str) -> f64 {
    match source {
        "rollback_notes" => 6.0,
        "release_notes" => 5.0,
        "changelog" => 4.0,
        _ => 3.0,
    }
}

pub(crate) fn artifact_score(artifact: &TrainArtifact) -> f64 {
    source_weight(&artifact.source) * artifact.confidence.max(0.0)
}

pub(crate) fn current_support(train: &ReleaseTrain, current_reference: &str) -> f64 {
    train
        .artifacts
        .iter()
        .filter(|artifact| artifact.decision_kind != "rollback_reference")
        .filter(|artifact| artifact.reference.as_deref() == Some(current_reference))
        .map(artifact_score)
        .sum()
}

pub(crate) fn best_challenger_support(
    train: &ReleaseTrain,
    current_reference: &str,
) -> Option<(String, f64)> {
    let mut scores = BTreeMap::<String, f64>::new();
    for artifact in &train.artifacts {
        if artifact.decision_kind == "rollback_reference" {
            continue;
        }
        let Some(reference) = &artifact.reference else {
            continue;
        };
        if reference == current_reference {
            continue;
        }
        *scores.entry(reference.clone()).or_default() += artifact_score(artifact);
    }
    scores.into_iter().max_by(|left, right| {
        left.1
            .partial_cmp(&right.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

pub(crate) fn rollback_support(train: &ReleaseTrain) -> f64 {
    train
        .artifacts
        .iter()
        .filter(|artifact| artifact.decision_kind == "rollback_reference")
        .map(artifact_score)
        .sum()
}

pub(crate) fn strong_signal_count(train: &ReleaseTrain, min_confidence: f64) -> usize {
    train
        .artifacts
        .iter()
        .filter(|artifact| artifact.confidence >= min_confidence)
        .count()
}

pub(crate) fn train_has_conflict(
    train: &ReleaseTrain,
    current_reference: &str,
    min_confidence: f64,
) -> bool {
    let mut buckets = BTreeSet::new();
    for artifact in &train.artifacts {
        if artifact.confidence < min_confidence {
            continue;
        }
        if artifact.decision_kind == "rollback_reference" {
            buckets.insert("rollback".to_string());
        } else if artifact.reference.as_deref() == Some(current_reference) {
            buckets.insert("current".to_string());
        } else if let Some(reference) = &artifact.reference {
            buckets.insert(format!("challenger:{reference}"));
        }
    }
    buckets.len() > 1
}

pub(crate) fn train_consensus(
    train: &ReleaseTrain,
    current_reference: &str,
    min_confidence: f64,
) -> Option<(String, Option<String>)> {
    if train_has_conflict(train, current_reference, min_confidence) {
        return None;
    }

    let current = current_support(train, current_reference);
    let challenger = best_challenger_support(train, current_reference);
    let challenger_score = challenger.as_ref().map(|(_, score)| *score).unwrap_or(0.0);
    let rollback = rollback_support(train);

    if rollback >= 5.5 && rollback > current + 1.0 && rollback > challenger_score + 1.0 {
        Some(("rollback_reference".to_string(), None))
    } else if challenger_score >= 6.5
        && challenger_score > current + 1.0
        && challenger_score > rollback + 1.0
    {
        Some((
            "replace_reference".to_string(),
            challenger.map(|(reference, _)| reference),
        ))
    } else if current >= 6.5 && current > challenger_score + 1.0 && current > rollback + 1.0 {
        Some((
            "keep_promoted".to_string(),
            Some(current_reference.to_string()),
        ))
    } else {
        None
    }
}

pub(crate) fn current_consensus_train_count(
    case: &ArtifactTrustCase,
    min_confidence: f64,
) -> usize {
    case.release_trains
        .iter()
        .filter(|train| {
            matches!(
                train_consensus(train, &case.current_reference, min_confidence),
                Some((ref kind, Some(ref reference)))
                    if kind == "keep_promoted" && reference == &case.current_reference
            )
        })
        .count()
}

pub(crate) fn dominant_challenger_trains(
    case: &ArtifactTrustCase,
    min_confidence: f64,
) -> Option<(String, usize)> {
    let mut counts = BTreeMap::<String, usize>::new();
    for train in &case.release_trains {
        if let Some((kind, Some(reference))) =
            train_consensus(train, &case.current_reference, min_confidence)
        {
            if kind == "replace_reference" {
                *counts.entry(reference).or_default() += 1;
            }
        }
    }
    counts.into_iter().max_by_key(|(_, count)| *count)
}

pub(crate) fn rollback_consensus_train_count(
    case: &ArtifactTrustCase,
    min_confidence: f64,
) -> usize {
    case.release_trains
        .iter()
        .filter(|train| {
            matches!(
                train_consensus(train, &case.current_reference, min_confidence),
                Some((ref kind, None)) if kind == "rollback_reference"
            )
        })
        .count()
}

pub(crate) fn conflict_train_count(case: &ArtifactTrustCase, min_confidence: f64) -> usize {
    case.release_trains
        .iter()
        .filter(|train| train_has_conflict(train, &case.current_reference, min_confidence))
        .count()
}

pub(crate) fn latest_source_artifact<'a>(
    case: &'a ArtifactTrustCase,
    source: &str,
) -> Option<&'a TrainArtifact> {
    for train in case.release_trains.iter().rev() {
        let candidate = train
            .artifacts
            .iter()
            .filter(|artifact| artifact.source == source)
            .max_by(|left, right| {
                left.confidence
                    .partial_cmp(&right.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        if candidate.is_some() {
            return candidate;
        }
    }
    None
}

pub(crate) fn any_high_confidence_rollback(case: &ArtifactTrustCase, min_confidence: f64) -> bool {
    case.release_trains.iter().any(|train| {
        train.artifacts.iter().any(|artifact| {
            artifact.source == "rollback_notes"
                && artifact.decision_kind == "rollback_reference"
                && artifact.confidence >= min_confidence
        })
    })
}

fn decision_matches(case: &ArtifactTrustCase, decision: &ArtifactTrustDecision) -> bool {
    decision.decision_kind == case.expected_decision_kind
        && decision.selected_reference == case.expected_reference
}

fn load_cases(root: &Path) -> Result<Vec<ArtifactTrustCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(variant: &dyn ArtifactTrustVariant, case: &ArtifactTrustCase) -> Result<f64> {
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
) -> Result<ArtifactTrustSourceMetrics> {
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
        "latest_train(",
        "current_support(",
        "best_challenger_support(",
        "rollback_support(",
        "strong_signal_count(",
        "train_has_conflict(",
        "current_consensus_train_count(",
        "dominant_challenger_trains(",
        "rollback_consensus_train_count(",
        "latest_source_artifact(",
        "any_high_confidence_rollback(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(ArtifactTrustSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        evidence_refs,
    })
}

fn readability_score(source: &ArtifactTrustSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &ArtifactTrustSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_decision_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.evidence_refs as i32 * 4)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &ArtifactTrustExperimentReport) -> Vec<&ArtifactTrustVariantReport> {
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
    report: &ArtifactTrustExperimentReport,
) -> Option<&ArtifactTrustVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_consumed_signals
            .partial_cmp(&right.average_consumed_signals)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
