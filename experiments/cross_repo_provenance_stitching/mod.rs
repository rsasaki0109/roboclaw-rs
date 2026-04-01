use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod docs_repo_preferred;
mod runtime_repo_only;
mod stitching_budget;
mod tag_join;

use docs_repo_preferred::DocsRepoPreferredVariant;
use runtime_repo_only::RuntimeRepoOnlyVariant;
use stitching_budget::StitchingBudgetVariant;
use tag_join::TagJoinVariant;

const CASES_PATH: &str = "experiments/cross_repo_provenance_stitching/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoArtifact {
    pub repository: String,
    pub source: String,
    pub tag: String,
    pub decision_kind: String,
    pub reference: Option<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossRepoStitchCase {
    pub id: String,
    pub suite: String,
    pub current_reference: String,
    pub release_tags: Vec<String>,
    pub artifacts: Vec<RepoArtifact>,
    pub expected_decision_kind: String,
    pub expected_reference: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossRepoStitchDecision {
    pub decision_kind: String,
    pub selected_reference: Option<String>,
    pub rationale: String,
    pub consumed_signals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossRepoStitchCaseResult {
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
pub struct CrossRepoStitchSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub evidence_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossRepoStitchVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: CrossRepoStitchSourceMetrics,
    pub cases: Vec<CrossRepoStitchCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_consumed_signals: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossRepoStitchExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<CrossRepoStitchCase>,
    pub variants: Vec<CrossRepoStitchVariantReport>,
}

#[derive(Debug, Clone)]
pub(crate) struct StitchedTag {
    pub tag: String,
    pub decision_kind: String,
    pub reference: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SignatureScore {
    pub decision_kind: String,
    pub reference: Option<String>,
    pub score: f64,
    pub repositories: usize,
}

pub trait CrossRepoStitchVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn decide(&self, case: &CrossRepoStitchCase) -> Result<CrossRepoStitchDecision>;
}

pub fn run_suite(root: &Path) -> Result<CrossRepoStitchExperimentReport> {
    let cases = load_cases(root)?;
    let decision_refs = [
        "stitched_confirmed",
        "stitched_superseded",
        "stitched_gap",
        "stitched_rejected",
    ];
    let variants: Vec<Box<dyn CrossRepoStitchVariant>> = vec![
        Box::new(RuntimeRepoOnlyVariant::default()),
        Box::new(DocsRepoPreferredVariant::default()),
        Box::new(TagJoinVariant::default()),
        Box::new(StitchingBudgetVariant::default()),
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
                    "variant '{}' failed to decide cross-repo provenance for case '{}'",
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

            case_results.push(CrossRepoStitchCaseResult {
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

        reports.push(CrossRepoStitchVariantReport {
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

    Ok(CrossRepoStitchExperimentReport {
        problem: "Release provenance should be stitchable across repositories only through comparable join policies, instead of assuming all evidence lives in one repo.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &CrossRepoStitchExperimentReport) -> String {
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

pub fn render_experiments_section(report: &CrossRepoStitchExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Cross-Repo Provenance Stitching\n\n");
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

pub fn render_decisions_section(report: &CrossRepoStitchExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Cross-Repo Provenance Stitching\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while using the richest cross-repo evidence window ({:.2} signals).\n",
            reference.name, reference.average_consumed_signals
        ));
    }
    markdown.push_str("- Keep cross-repo stitching experimental until the repo has real release artifacts split across multiple repositories.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Cross-Repo Provenance Stitching\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct RepoArtifact {\n");
    markdown.push_str("    pub repository: String,\n");
    markdown.push_str("    pub source: String,\n");
    markdown.push_str("    pub tag: String,\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub reference: Option<String>,\n");
    markdown.push_str("    pub confidence: f64,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct CrossRepoStitchCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub suite: String,\n");
    markdown.push_str("    pub current_reference: String,\n");
    markdown.push_str("    pub release_tags: Vec<String>,\n");
    markdown.push_str("    pub artifacts: Vec<RepoArtifact>,\n");
    markdown.push_str("    pub expected_decision_kind: String,\n");
    markdown.push_str("    pub expected_reference: Option<String>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct CrossRepoStitchDecision {\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub selected_reference: Option<String>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("    pub consumed_signals: usize,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait CrossRepoStitchVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn decide(&self, case: &CrossRepoStitchCase) -> anyhow::Result<CrossRepoStitchDecision>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown
        .push_str("- Shared input: same repo-scoped artifact set for every stitching policy.\n");
    markdown.push_str("- Shared metrics: decision accuracy, average decision time, average consumed signals, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    decision_kind: impl Into<String>,
    selected_reference: Option<String>,
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> CrossRepoStitchDecision {
    CrossRepoStitchDecision {
        decision_kind: decision_kind.into(),
        selected_reference,
        rationale: rationale.into(),
        consumed_signals,
    }
}

pub(crate) fn empty_stitched_tags(case: &CrossRepoStitchCase) -> Vec<StitchedTag> {
    case.release_tags
        .iter()
        .map(|tag| StitchedTag {
            tag: tag.clone(),
            decision_kind: "missing_provenance".to_string(),
            reference: None,
        })
        .collect()
}

pub(crate) fn apply_artifact(stitched: &mut [StitchedTag], tag: &str, artifact: &RepoArtifact) {
    if let Some(entry) = stitched.iter_mut().find(|entry| entry.tag == tag) {
        entry.decision_kind = artifact.decision_kind.clone();
        entry.reference = artifact.reference.clone();
    }
}

pub(crate) fn repo_artifact(
    case: &CrossRepoStitchCase,
    repository: &str,
    tag: &str,
    min_confidence: f64,
) -> Option<RepoArtifact> {
    case.artifacts
        .iter()
        .filter(|artifact| {
            artifact.repository == repository
                && artifact.tag == tag
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
    case: &CrossRepoStitchCase,
    tag: &str,
    min_confidence: f64,
) -> Option<RepoArtifact> {
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

pub(crate) fn artifact_score(artifact: &RepoArtifact) -> f64 {
    let source_weight = match artifact.source.as_str() {
        "rollback_notes" => 6.0,
        "release_notes" => 5.0,
        "changelog" => 4.0,
        _ => 3.0,
    };
    source_weight * artifact.confidence.max(0.0)
}

pub(crate) fn tag_signature_scores(
    case: &CrossRepoStitchCase,
    tag: &str,
    min_confidence: f64,
) -> Vec<SignatureScore> {
    let mut grouped = BTreeMap::<(String, Option<String>), (f64, BTreeSet<String>)>::new();
    for artifact in &case.artifacts {
        if artifact.tag != tag || artifact.confidence < min_confidence {
            continue;
        }
        let entry = grouped
            .entry((artifact.decision_kind.clone(), artifact.reference.clone()))
            .or_insert_with(|| (0.0, BTreeSet::new()));
        entry.0 += artifact_score(artifact);
        entry.1.insert(artifact.repository.clone());
    }

    let mut scores = grouped
        .into_iter()
        .map(
            |((decision_kind, reference), (score, repositories))| SignatureScore {
                decision_kind,
                reference,
                score,
                repositories: repositories.len(),
            },
        )
        .collect::<Vec<_>>();
    scores.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.repositories.cmp(&left.repositories))
    });
    scores
}

pub(crate) fn tag_conflict(case: &CrossRepoStitchCase, tag: &str, min_confidence: f64) -> bool {
    tag_signature_scores(case, tag, min_confidence).len() > 1
}

pub(crate) fn matching_repo_pair(
    case: &CrossRepoStitchCase,
    tag: &str,
    min_confidence: f64,
) -> Option<RepoArtifact> {
    let best = tag_signature_scores(case, tag, min_confidence)
        .into_iter()
        .find(|signature| signature.repositories >= 2)?;
    case.artifacts
        .iter()
        .filter(|artifact| {
            artifact.tag == tag
                && artifact.confidence >= min_confidence
                && artifact.decision_kind == best.decision_kind
                && artifact.reference == best.reference
        })
        .max_by(|left, right| {
            left.confidence
                .partial_cmp(&right.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
}

pub(crate) fn classify_stitched_chain(
    case: &CrossRepoStitchCase,
    stitched: &[StitchedTag],
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> CrossRepoStitchDecision {
    if stitched
        .iter()
        .any(|tag| tag.decision_kind == "missing_provenance" || tag.reference.is_none())
    {
        return decision("stitched_gap", None, rationale, consumed_signals);
    }

    if stitched
        .iter()
        .any(|tag| tag.decision_kind == "rollback_reference")
    {
        return decision("stitched_rejected", None, rationale, consumed_signals);
    }

    let latest_reference = stitched.last().and_then(|tag| tag.reference.as_deref());
    let origin_reference = stitched.iter().find_map(|tag| tag.reference.clone());
    let replaced = stitched
        .iter()
        .any(|tag| tag.decision_kind == "replace_reference");

    if latest_reference != Some(case.current_reference.as_str()) {
        if replaced {
            return decision(
                "stitched_superseded",
                latest_reference.map(|reference| reference.to_string()),
                rationale,
                consumed_signals,
            );
        }
        return decision("stitched_gap", None, rationale, consumed_signals);
    }

    if replaced && origin_reference.as_deref() != Some(case.current_reference.as_str()) {
        return decision(
            "stitched_superseded",
            Some(case.current_reference.clone()),
            rationale,
            consumed_signals,
        );
    }

    if stitched.iter().all(|tag| {
        tag.reference.as_deref() == Some(case.current_reference.as_str())
            && tag.decision_kind != "replace_reference"
    }) {
        return decision(
            "stitched_confirmed",
            Some(case.current_reference.clone()),
            rationale,
            consumed_signals,
        );
    }

    decision("stitched_gap", None, rationale, consumed_signals)
}

fn decision_matches(case: &CrossRepoStitchCase, decision: &CrossRepoStitchDecision) -> bool {
    decision.decision_kind == case.expected_decision_kind
        && decision.selected_reference == case.expected_reference
}

fn load_cases(root: &Path) -> Result<Vec<CrossRepoStitchCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn CrossRepoStitchVariant,
    case: &CrossRepoStitchCase,
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
) -> Result<CrossRepoStitchSourceMetrics> {
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
        "repo_artifact(",
        "best_artifact(",
        "artifact_score(",
        "tag_signature_scores(",
        "tag_conflict(",
        "matching_repo_pair(",
        "classify_stitched_chain(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(CrossRepoStitchSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        evidence_refs,
    })
}

fn readability_score(source: &CrossRepoStitchSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &CrossRepoStitchSourceMetrics) -> u32 {
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
    report: &CrossRepoStitchExperimentReport,
) -> Vec<&CrossRepoStitchVariantReport> {
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
    report: &CrossRepoStitchExperimentReport,
) -> Option<&CrossRepoStitchVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_consumed_signals
            .partial_cmp(&right.average_consumed_signals)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
