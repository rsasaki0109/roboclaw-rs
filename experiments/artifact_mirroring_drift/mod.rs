use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod docs_portal_preferred;
mod latest_mirror_majority;
mod mirror_budget;
mod registry_only;

use docs_portal_preferred::DocsPortalPreferredVariant;
use latest_mirror_majority::LatestMirrorMajorityVariant;
use mirror_budget::MirrorBudgetVariant;
use registry_only::RegistryOnlyVariant;

const CASES_PATH: &str = "experiments/artifact_mirroring_drift/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorArtifact {
    pub mirror: String,
    pub decision_kind: String,
    pub reference: Option<String>,
    pub confidence: f64,
    pub freshness_hours: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirroredReleaseTrain {
    pub train: String,
    pub artifacts: Vec<MirrorArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirroringDriftCase {
    pub id: String,
    pub suite: String,
    pub current_reference: String,
    pub release_trains: Vec<MirroredReleaseTrain>,
    pub expected_decision_kind: String,
    pub expected_reference: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirroringDriftDecision {
    pub decision_kind: String,
    pub selected_reference: Option<String>,
    pub rationale: String,
    pub consumed_signals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirroringDriftCaseResult {
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
pub struct MirroringDriftSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub evidence_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirroringDriftVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: MirroringDriftSourceMetrics,
    pub cases: Vec<MirroringDriftCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_consumed_signals: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirroringDriftExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<MirroringDriftCase>,
    pub variants: Vec<MirroringDriftVariantReport>,
}

pub trait MirroringDriftVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn decide(&self, case: &MirroringDriftCase) -> Result<MirroringDriftDecision>;
}

pub fn run_suite(root: &Path) -> Result<MirroringDriftExperimentReport> {
    let cases = load_cases(root)?;
    let decision_refs = [
        "mirror_confirmed",
        "mirror_superseded",
        "mirror_drift",
        "mirror_rejected",
    ];
    let variants: Vec<Box<dyn MirroringDriftVariant>> = vec![
        Box::new(RegistryOnlyVariant::default()),
        Box::new(DocsPortalPreferredVariant::default()),
        Box::new(LatestMirrorMajorityVariant::default()),
        Box::new(MirrorBudgetVariant::default()),
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
                    "variant '{}' failed to decide artifact mirroring drift for case '{}'",
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

            case_results.push(MirroringDriftCaseResult {
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

        reports.push(MirroringDriftVariantReport {
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

    Ok(MirroringDriftExperimentReport {
        problem: "Mirrored release artifacts should be treated as drifting signals across package registries and documentation portals, not as one perfectly synchronized publication surface.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &MirroringDriftExperimentReport) -> String {
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

pub fn render_experiments_section(report: &MirroringDriftExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Artifact Mirroring Drift\n\n");
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

pub fn render_decisions_section(report: &MirroringDriftExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Artifact Mirroring Drift\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while using the richest mirror-evidence window ({:.2} signals).\n",
            reference.name, reference.average_consumed_signals
        ));
    }
    markdown.push_str("- Keep mirror-drift policy experimental until the repo has real publication lags across registries and documentation portals.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Artifact Mirroring Drift\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct MirrorArtifact {\n");
    markdown.push_str("    pub mirror: String,\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub reference: Option<String>,\n");
    markdown.push_str("    pub confidence: f64,\n");
    markdown.push_str("    pub freshness_hours: u32,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct MirroredReleaseTrain {\n");
    markdown.push_str("    pub train: String,\n");
    markdown.push_str("    pub artifacts: Vec<MirrorArtifact>,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct MirroringDriftCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub suite: String,\n");
    markdown.push_str("    pub current_reference: String,\n");
    markdown.push_str("    pub release_trains: Vec<MirroredReleaseTrain>,\n");
    markdown.push_str("    pub expected_decision_kind: String,\n");
    markdown.push_str("    pub expected_reference: Option<String>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct MirroringDriftDecision {\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub selected_reference: Option<String>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("    pub consumed_signals: usize,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait MirroringDriftVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn decide(&self, case: &MirroringDriftCase) -> anyhow::Result<MirroringDriftDecision>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str("- Shared input: same mirror snapshots for every drift policy.\n");
    markdown.push_str("- Shared metrics: decision accuracy, average decision time, average consumed signals, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    decision_kind: impl Into<String>,
    selected_reference: Option<String>,
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> MirroringDriftDecision {
    MirroringDriftDecision {
        decision_kind: decision_kind.into(),
        selected_reference,
        rationale: rationale.into(),
        consumed_signals,
    }
}

pub(crate) fn latest_train(case: &MirroringDriftCase) -> Option<&MirroredReleaseTrain> {
    case.release_trains.last()
}

pub(crate) fn mirror_weight(mirror: &str) -> f64 {
    match mirror {
        "package_registry" => 6.0,
        "docs_portal" => 5.0,
        "api_docs" => 4.0,
        "release_feed" => 4.0,
        _ => 3.0,
    }
}

pub(crate) fn mirror_score(artifact: &MirrorArtifact) -> f64 {
    let freshness_penalty = artifact.freshness_hours as f64 / 48.0;
    (mirror_weight(&artifact.mirror) * artifact.confidence.max(0.0) - freshness_penalty).max(0.0)
}

pub(crate) fn latest_mirror_artifact<'a>(
    case: &'a MirroringDriftCase,
    mirror: &str,
) -> Option<&'a MirrorArtifact> {
    let train = latest_train(case)?;
    train
        .artifacts
        .iter()
        .filter(|artifact| artifact.mirror == mirror)
        .max_by(|left, right| {
            left.confidence
                .partial_cmp(&right.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

pub(crate) fn current_support(
    train: &MirroredReleaseTrain,
    current_reference: &str,
    max_freshness_hours: u32,
    min_confidence: f64,
) -> f64 {
    train
        .artifacts
        .iter()
        .filter(|artifact| artifact.freshness_hours <= max_freshness_hours)
        .filter(|artifact| artifact.confidence >= min_confidence)
        .filter(|artifact| artifact.decision_kind != "rollback_reference")
        .filter(|artifact| artifact.reference.as_deref() == Some(current_reference))
        .map(mirror_score)
        .sum()
}

pub(crate) fn best_challenger_support(
    train: &MirroredReleaseTrain,
    current_reference: &str,
    max_freshness_hours: u32,
    min_confidence: f64,
) -> Option<(String, f64, usize)> {
    let mut scores = BTreeMap::<String, (f64, BTreeSet<String>)>::new();
    for artifact in &train.artifacts {
        if artifact.freshness_hours > max_freshness_hours
            || artifact.confidence < min_confidence
            || artifact.decision_kind == "rollback_reference"
        {
            continue;
        }
        let Some(reference) = &artifact.reference else {
            continue;
        };
        if reference == current_reference {
            continue;
        }
        let entry = scores
            .entry(reference.clone())
            .or_insert_with(|| (0.0, BTreeSet::new()));
        entry.0 += mirror_score(artifact);
        entry.1.insert(artifact.mirror.clone());
    }
    scores
        .into_iter()
        .max_by(|left, right| {
            left.1
                 .0
                .partial_cmp(&right.1 .0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.1 .1.len().cmp(&right.1 .1.len()))
        })
        .map(|(reference, (score, mirrors))| (reference, score, mirrors.len()))
}

pub(crate) fn rollback_support(
    train: &MirroredReleaseTrain,
    max_freshness_hours: u32,
    min_confidence: f64,
) -> f64 {
    train
        .artifacts
        .iter()
        .filter(|artifact| artifact.freshness_hours <= max_freshness_hours)
        .filter(|artifact| artifact.confidence >= min_confidence)
        .filter(|artifact| artifact.decision_kind == "rollback_reference")
        .map(mirror_score)
        .sum()
}

pub(crate) fn fresh_signal_count(
    train: &MirroredReleaseTrain,
    max_freshness_hours: u32,
    min_confidence: f64,
) -> usize {
    train
        .artifacts
        .iter()
        .filter(|artifact| artifact.freshness_hours <= max_freshness_hours)
        .filter(|artifact| artifact.confidence >= min_confidence)
        .count()
}

pub(crate) fn mirror_conflict(
    train: &MirroredReleaseTrain,
    current_reference: &str,
    max_freshness_hours: u32,
    min_confidence: f64,
) -> bool {
    let mut buckets = BTreeSet::new();
    for artifact in &train.artifacts {
        if artifact.freshness_hours > max_freshness_hours || artifact.confidence < min_confidence {
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
    train: &MirroredReleaseTrain,
    current_reference: &str,
    max_freshness_hours: u32,
    min_confidence: f64,
) -> Option<(String, Option<String>)> {
    if mirror_conflict(
        train,
        current_reference,
        max_freshness_hours,
        min_confidence,
    ) {
        return None;
    }

    let current = current_support(
        train,
        current_reference,
        max_freshness_hours,
        min_confidence,
    );
    let challenger = best_challenger_support(
        train,
        current_reference,
        max_freshness_hours,
        min_confidence,
    );
    let challenger_score = challenger
        .as_ref()
        .map(|(_, score, _)| *score)
        .unwrap_or(0.0);
    let rollback = rollback_support(train, max_freshness_hours, min_confidence);

    if rollback >= 5.5 && rollback > current + 0.5 && rollback > challenger_score + 0.5 {
        Some(("rollback_reference".to_string(), None))
    } else if challenger_score >= 6.0
        && challenger_score > current + 0.5
        && challenger_score > rollback + 0.5
    {
        Some((
            "replace_reference".to_string(),
            challenger.map(|(reference, _, _)| reference),
        ))
    } else if current >= 6.0 && current > challenger_score + 0.5 && current > rollback + 0.5 {
        Some((
            "keep_promoted".to_string(),
            Some(current_reference.to_string()),
        ))
    } else {
        None
    }
}

pub(crate) fn current_consensus_train_count(
    case: &MirroringDriftCase,
    max_freshness_hours: u32,
    min_confidence: f64,
) -> usize {
    case.release_trains
        .iter()
        .filter(|train| {
            matches!(
                train_consensus(train, &case.current_reference, max_freshness_hours, min_confidence),
                Some((ref kind, Some(ref reference)))
                    if kind == "keep_promoted" && reference == &case.current_reference
            )
        })
        .count()
}

pub(crate) fn dominant_challenger_trains(
    case: &MirroringDriftCase,
    max_freshness_hours: u32,
    min_confidence: f64,
) -> Option<(String, usize)> {
    let mut counts = BTreeMap::<String, usize>::new();
    for train in &case.release_trains {
        if let Some((kind, Some(reference))) = train_consensus(
            train,
            &case.current_reference,
            max_freshness_hours,
            min_confidence,
        ) {
            if kind == "replace_reference" {
                *counts.entry(reference).or_default() += 1;
            }
        }
    }
    counts.into_iter().max_by_key(|(_, count)| *count)
}

pub(crate) fn rollback_consensus_train_count(
    case: &MirroringDriftCase,
    max_freshness_hours: u32,
    min_confidence: f64,
) -> usize {
    case.release_trains
        .iter()
        .filter(|train| {
            matches!(
                train_consensus(train, &case.current_reference, max_freshness_hours, min_confidence),
                Some((ref kind, None)) if kind == "rollback_reference"
            )
        })
        .count()
}

fn decision_matches(case: &MirroringDriftCase, decision: &MirroringDriftDecision) -> bool {
    decision.decision_kind == case.expected_decision_kind
        && decision.selected_reference == case.expected_reference
}

fn load_cases(root: &Path) -> Result<Vec<MirroringDriftCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn MirroringDriftVariant,
    case: &MirroringDriftCase,
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
) -> Result<MirroringDriftSourceMetrics> {
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
        "latest_mirror_artifact(",
        "current_support(",
        "best_challenger_support(",
        "rollback_support(",
        "fresh_signal_count(",
        "mirror_conflict(",
        "current_consensus_train_count(",
        "dominant_challenger_trains(",
        "rollback_consensus_train_count(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(MirroringDriftSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        evidence_refs,
    })
}

fn readability_score(source: &MirroringDriftSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &MirroringDriftSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_decision_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.evidence_refs as i32 * 4)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &MirroringDriftExperimentReport) -> Vec<&MirroringDriftVariantReport> {
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
    report: &MirroringDriftExperimentReport,
) -> Option<&MirroringDriftVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_consumed_signals
            .partial_cmp(&right.average_consumed_signals)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
