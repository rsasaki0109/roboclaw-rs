use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

mod artifact_lineage_budget;
mod changelog_first;
mod documented_only;
mod dual_artifact_match;

use artifact_lineage_budget::ArtifactLineageBudgetVariant;
use changelog_first::ChangelogFirstVariant;
use documented_only::DocumentedOnlyVariant;
use dual_artifact_match::DualArtifactMatchVariant;

const CASES_PATH: &str = "experiments/provenance_backfill/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackfillReleaseCut {
    pub tag: String,
    pub decision_kind: String,
    pub reference: Option<String>,
    pub documented: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub source: String,
    pub tag: String,
    pub decision_kind: String,
    pub reference: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceBackfillCase {
    pub id: String,
    pub suite: String,
    pub current_reference: String,
    pub release_cuts: Vec<BackfillReleaseCut>,
    pub artifacts: Vec<ArtifactRecord>,
    pub expected_decision_kind: String,
    pub expected_reference: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceBackfillDecision {
    pub decision_kind: String,
    pub selected_reference: Option<String>,
    pub rationale: String,
    pub consumed_signals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceBackfillCaseResult {
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
pub struct ProvenanceBackfillSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub evidence_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceBackfillVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: ProvenanceBackfillSourceMetrics,
    pub cases: Vec<ProvenanceBackfillCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_consumed_signals: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceBackfillExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<ProvenanceBackfillCase>,
    pub variants: Vec<ProvenanceBackfillVariantReport>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedCut {
    pub tag: String,
    pub decision_kind: String,
    pub reference: Option<String>,
}

pub trait ProvenanceBackfillVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn decide(&self, case: &ProvenanceBackfillCase) -> Result<ProvenanceBackfillDecision>;
}

pub fn run_suite(root: &Path) -> Result<ProvenanceBackfillExperimentReport> {
    let cases = load_cases(root)?;
    let decision_refs = [
        "backfill_confirmed",
        "backfill_superseded",
        "backfill_gap",
        "backfill_rejected",
    ];
    let variants: Vec<Box<dyn ProvenanceBackfillVariant>> = vec![
        Box::new(DocumentedOnlyVariant::default()),
        Box::new(ChangelogFirstVariant::default()),
        Box::new(DualArtifactMatchVariant::default()),
        Box::new(ArtifactLineageBudgetVariant::default()),
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
                    "variant '{}' failed to decide provenance backfill for case '{}'",
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

            case_results.push(ProvenanceBackfillCaseResult {
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

        reports.push(ProvenanceBackfillVariantReport {
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

    Ok(ProvenanceBackfillExperimentReport {
        problem: "Missing release provenance should be recoverable from changelog and release-note artifacts only through comparable backfill policies, not one-off manual edits.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &ProvenanceBackfillExperimentReport) -> String {
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

pub fn render_experiments_section(report: &ProvenanceBackfillExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Provenance Backfill\n\n");
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

pub fn render_decisions_section(report: &ProvenanceBackfillExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Provenance Backfill\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while using the richest artifact-evidence window ({:.2} signals).\n",
            reference.name, reference.average_consumed_signals
        ));
    }
    markdown.push_str("- Keep artifact backfill experimental until the repo has real changelog, release-note, and rollback-note history to mine.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Provenance Backfill\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct BackfillReleaseCut {\n");
    markdown.push_str("    pub tag: String,\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub reference: Option<String>,\n");
    markdown.push_str("    pub documented: bool,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ArtifactRecord {\n");
    markdown.push_str("    pub source: String,\n");
    markdown.push_str("    pub tag: String,\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub reference: Option<String>,\n");
    markdown.push_str("    pub confidence: f64,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ProvenanceBackfillCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub suite: String,\n");
    markdown.push_str("    pub current_reference: String,\n");
    markdown.push_str("    pub release_cuts: Vec<BackfillReleaseCut>,\n");
    markdown.push_str("    pub artifacts: Vec<ArtifactRecord>,\n");
    markdown.push_str("    pub expected_decision_kind: String,\n");
    markdown.push_str("    pub expected_reference: Option<String>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ProvenanceBackfillDecision {\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub selected_reference: Option<String>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("    pub consumed_signals: usize,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait ProvenanceBackfillVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn decide(&self, case: &ProvenanceBackfillCase) -> anyhow::Result<ProvenanceBackfillDecision>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str(
        "- Shared input: same release cuts and same artifact set for every backfill policy.\n",
    );
    markdown.push_str("- Shared metrics: decision accuracy, average decision time, average consumed signals, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    decision_kind: impl Into<String>,
    selected_reference: Option<String>,
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> ProvenanceBackfillDecision {
    ProvenanceBackfillDecision {
        decision_kind: decision_kind.into(),
        selected_reference,
        rationale: rationale.into(),
        consumed_signals,
    }
}

pub(crate) fn base_resolved_cuts(case: &ProvenanceBackfillCase) -> Vec<ResolvedCut> {
    case.release_cuts
        .iter()
        .map(|cut| {
            if cut.documented {
                ResolvedCut {
                    tag: cut.tag.clone(),
                    decision_kind: cut.decision_kind.clone(),
                    reference: cut.reference.clone(),
                }
            } else {
                ResolvedCut {
                    tag: cut.tag.clone(),
                    decision_kind: "missing_provenance".to_string(),
                    reference: None,
                }
            }
        })
        .collect()
}

pub(crate) fn all_documented(case: &ProvenanceBackfillCase) -> bool {
    case.release_cuts.iter().all(|cut| cut.documented)
}

pub(crate) fn missing_documentation_count(case: &ProvenanceBackfillCase) -> usize {
    case.release_cuts
        .iter()
        .filter(|cut| !cut.documented)
        .count()
}

pub(crate) fn missing_tags(case: &ProvenanceBackfillCase) -> Vec<String> {
    case.release_cuts
        .iter()
        .filter(|cut| !cut.documented)
        .map(|cut| cut.tag.clone())
        .collect()
}

pub(crate) fn documented_has_rollback(case: &ProvenanceBackfillCase) -> bool {
    case.release_cuts
        .iter()
        .any(|cut| cut.documented && cut.decision_kind == "rollback_reference")
}

pub(crate) fn source_artifact(
    case: &ProvenanceBackfillCase,
    tag: &str,
    source: &str,
    min_confidence: f64,
) -> Option<ArtifactRecord> {
    case.artifacts
        .iter()
        .filter(|artifact| {
            artifact.tag == tag
                && artifact.source == source
                && artifact.confidence >= min_confidence
        })
        .max_by(|left, right| {
            left.confidence
                .partial_cmp(&right.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
}

pub(crate) fn best_artifact(
    case: &ProvenanceBackfillCase,
    tag: &str,
    min_confidence: f64,
) -> Option<ArtifactRecord> {
    case.artifacts
        .iter()
        .filter(|artifact| artifact.tag == tag && artifact.confidence >= min_confidence)
        .max_by(|left, right| {
            left.confidence
                .partial_cmp(&right.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
}

pub(crate) fn matching_dual_artifact(
    case: &ProvenanceBackfillCase,
    tag: &str,
    min_confidence: f64,
) -> Option<ArtifactRecord> {
    let changelog = source_artifact(case, tag, "changelog", min_confidence)?;
    let release_notes = source_artifact(case, tag, "release_notes", min_confidence)?;
    if changelog.decision_kind == release_notes.decision_kind
        && changelog.reference == release_notes.reference
    {
        Some(changelog)
    } else {
        None
    }
}

pub(crate) fn artifact_conflict(
    case: &ProvenanceBackfillCase,
    tag: &str,
    min_confidence: f64,
) -> bool {
    let signatures = case
        .artifacts
        .iter()
        .filter(|artifact| artifact.tag == tag && artifact.confidence >= min_confidence)
        .map(|artifact| {
            (
                artifact.decision_kind.clone(),
                artifact
                    .reference
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
            )
        })
        .collect::<BTreeSet<_>>();
    signatures.len() > 1
}

pub(crate) fn trusted_rollback_artifact(
    case: &ProvenanceBackfillCase,
    tag: &str,
    min_confidence: f64,
) -> Option<ArtifactRecord> {
    case.artifacts
        .iter()
        .filter(|artifact| {
            artifact.tag == tag
                && artifact.decision_kind == "rollback_reference"
                && artifact.confidence >= min_confidence
        })
        .max_by(|left, right| {
            left.confidence
                .partial_cmp(&right.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
}

pub(crate) fn any_trusted_rollback_artifact(
    case: &ProvenanceBackfillCase,
    min_confidence: f64,
) -> bool {
    case.artifacts.iter().any(|artifact| {
        artifact.decision_kind == "rollback_reference" && artifact.confidence >= min_confidence
    })
}

pub(crate) fn apply_artifact(
    resolved_cuts: &mut [ResolvedCut],
    tag: &str,
    artifact: &ArtifactRecord,
) {
    if let Some(cut) = resolved_cuts.iter_mut().find(|cut| cut.tag == tag) {
        cut.decision_kind = artifact.decision_kind.clone();
        cut.reference = artifact.reference.clone();
    }
}

pub(crate) fn classify_resolved_chain(
    case: &ProvenanceBackfillCase,
    resolved_cuts: &[ResolvedCut],
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> ProvenanceBackfillDecision {
    if resolved_cuts
        .iter()
        .any(|cut| cut.decision_kind == "missing_provenance" || cut.reference.is_none())
    {
        return decision("backfill_gap", None, rationale, consumed_signals);
    }

    if resolved_cuts
        .iter()
        .any(|cut| cut.decision_kind == "rollback_reference")
    {
        return decision("backfill_rejected", None, rationale, consumed_signals);
    }

    let latest_reference = resolved_cuts
        .last()
        .and_then(|cut| cut.reference.as_deref());
    if latest_reference != Some(case.current_reference.as_str()) {
        return decision("backfill_gap", None, rationale, consumed_signals);
    }

    let origin_reference = resolved_cuts.iter().find_map(|cut| cut.reference.clone());
    let replaced_to_current = resolved_cuts.iter().any(|cut| {
        cut.decision_kind == "replace_reference"
            && cut.reference.as_deref() == Some(case.current_reference.as_str())
    });

    if replaced_to_current && origin_reference.as_deref() != Some(case.current_reference.as_str()) {
        return decision(
            "backfill_superseded",
            Some(case.current_reference.clone()),
            rationale,
            consumed_signals,
        );
    }

    if resolved_cuts.iter().all(|cut| {
        cut.reference.as_deref() == Some(case.current_reference.as_str())
            && cut.decision_kind != "replace_reference"
    }) {
        return decision(
            "backfill_confirmed",
            Some(case.current_reference.clone()),
            rationale,
            consumed_signals,
        );
    }

    decision("backfill_gap", None, rationale, consumed_signals)
}

fn decision_matches(case: &ProvenanceBackfillCase, decision: &ProvenanceBackfillDecision) -> bool {
    decision.decision_kind == case.expected_decision_kind
        && decision.selected_reference == case.expected_reference
}

fn load_cases(root: &Path) -> Result<Vec<ProvenanceBackfillCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn ProvenanceBackfillVariant,
    case: &ProvenanceBackfillCase,
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
) -> Result<ProvenanceBackfillSourceMetrics> {
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
        "all_documented(",
        "missing_documentation_count(",
        "documented_has_rollback(",
        "source_artifact(",
        "best_artifact(",
        "matching_dual_artifact(",
        "artifact_conflict(",
        "trusted_rollback_artifact(",
        "classify_resolved_chain(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(ProvenanceBackfillSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        evidence_refs,
    })
}

fn readability_score(source: &ProvenanceBackfillSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &ProvenanceBackfillSourceMetrics) -> u32 {
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
    report: &ProvenanceBackfillExperimentReport,
) -> Vec<&ProvenanceBackfillVariantReport> {
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
    report: &ProvenanceBackfillExperimentReport,
) -> Option<&ProvenanceBackfillVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_consumed_signals
            .partial_cmp(&right.average_consumed_signals)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
