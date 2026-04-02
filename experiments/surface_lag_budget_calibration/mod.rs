use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;

mod flat_defaults;
mod latest_observation;
mod max_observed;
mod trace_budget;

use flat_defaults::FlatDefaultsVariant;
use latest_observation::LatestObservationVariant;
use max_observed::MaxObservedVariant;
use trace_budget::TraceBudgetVariant;

const CASES_PATH: &str = "experiments/surface_lag_budget_calibration/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceLagCalibrationCase {
    pub id: String,
    pub suite: String,
    pub surface: String,
    pub current_budget_hours: u32,
    pub observed_lag_hours: Vec<u32>,
    pub expected_decision_kind: String,
    pub expected_budget_hours: Option<u32>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceLagCalibrationDecision {
    pub decision_kind: String,
    pub selected_budget_hours: Option<u32>,
    pub rationale: String,
    pub consumed_signals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceLagCalibrationCaseResult {
    pub case_id: String,
    pub correct: bool,
    pub expected_decision_kind: String,
    pub selected_decision_kind: String,
    pub expected_budget_hours: Option<u32>,
    pub selected_budget_hours: Option<u32>,
    pub consumed_signals: usize,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceLagCalibrationSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_decision_refs: usize,
    pub evidence_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceLagCalibrationVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: SurfaceLagCalibrationSourceMetrics,
    pub cases: Vec<SurfaceLagCalibrationCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_consumed_signals: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SurfaceLagCalibrationExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<SurfaceLagCalibrationCase>,
    pub variants: Vec<SurfaceLagCalibrationVariantReport>,
}

pub trait SurfaceLagCalibrationVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn decide(&self, case: &SurfaceLagCalibrationCase) -> Result<SurfaceLagCalibrationDecision>;
}

pub fn run_suite(root: &Path) -> Result<SurfaceLagCalibrationExperimentReport> {
    let cases = load_cases(root)?;
    let decision_refs = ["budget_keep", "budget_raise", "budget_lower", "budget_hold"];
    let variants: Vec<Box<dyn SurfaceLagCalibrationVariant>> = vec![
        Box::new(FlatDefaultsVariant::default()),
        Box::new(LatestObservationVariant::default()),
        Box::new(MaxObservedVariant::default()),
        Box::new(TraceBudgetVariant::default()),
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
                    "variant '{}' failed to calibrate lag budget for case '{}'",
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

            case_results.push(SurfaceLagCalibrationCaseResult {
                case_id: case.id.clone(),
                correct: is_correct,
                expected_decision_kind: case.expected_decision_kind.clone(),
                selected_decision_kind: decision.decision_kind,
                expected_budget_hours: case.expected_budget_hours,
                selected_budget_hours: decision.selected_budget_hours,
                consumed_signals: decision.consumed_signals,
                average_decision_us: bench_average_us,
            });
        }

        reports.push(SurfaceLagCalibrationVariantReport {
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

    Ok(SurfaceLagCalibrationExperimentReport {
        problem: "Surface-specific publication lag budgets should be calibrated from observed traces instead of hard-coding one global delay model across registries, release feeds, and docs portals.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &SurfaceLagCalibrationExperimentReport) -> String {
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

pub fn render_experiments_section(report: &SurfaceLagCalibrationExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Surface Lag Budget Calibration\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown
        .push_str("| case | suite | surface | expected decision | expected budget hours | why |\n");
    markdown.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | `{}` | `{}` | `{}` | {} |\n",
            case.id,
            case.suite,
            case.surface,
            case.expected_decision_kind,
            case.expected_budget_hours
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
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

pub fn render_decisions_section(report: &SurfaceLagCalibrationExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Surface Lag Budget Calibration\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while using the richest trace-evidence window ({:.2} signals).\n",
            reference.name, reference.average_consumed_signals
        ));
    }
    markdown.push_str("- Keep surface-specific lag calibration experimental until the repo has trace exports from real package, feed, and docs publication pipelines.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Surface Lag Budget Calibration\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct SurfaceLagCalibrationCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub suite: String,\n");
    markdown.push_str("    pub surface: String,\n");
    markdown.push_str("    pub current_budget_hours: u32,\n");
    markdown.push_str("    pub observed_lag_hours: Vec<u32>,\n");
    markdown.push_str("    pub expected_decision_kind: String,\n");
    markdown.push_str("    pub expected_budget_hours: Option<u32>,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct SurfaceLagCalibrationDecision {\n");
    markdown.push_str("    pub decision_kind: String,\n");
    markdown.push_str("    pub selected_budget_hours: Option<u32>,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("    pub consumed_signals: usize,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait SurfaceLagCalibrationVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn decide(&self, case: &SurfaceLagCalibrationCase) -> anyhow::Result<SurfaceLagCalibrationDecision>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown
        .push_str("- Shared input: same per-surface lag traces for every calibration policy.\n");
    markdown.push_str("- Shared metrics: decision accuracy, average decision time, average consumed signals, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn default_budget(surface: &str) -> u32 {
    match surface {
        "package_registry" => 24,
        "release_feed" => 6,
        "docs_portal" => 12,
        "api_docs" => 6,
        _ => 12,
    }
}

pub(crate) fn budget_step(surface: &str) -> u32 {
    match surface {
        "package_registry" => 6,
        "release_feed" => 2,
        "docs_portal" => 6,
        "api_docs" => 6,
        _ => 6,
    }
}

pub(crate) fn round_up_to_step(value: u32, step: u32) -> u32 {
    if step == 0 {
        return value;
    }
    let remainder = value % step;
    if remainder == 0 {
        value
    } else {
        value + (step - remainder)
    }
}

pub(crate) fn sorted_lags(case: &SurfaceLagCalibrationCase) -> Vec<u32> {
    let mut lags = case.observed_lag_hours.clone();
    lags.sort_unstable();
    lags
}

pub(crate) fn percentile_lag(
    case: &SurfaceLagCalibrationCase,
    numerator: usize,
    denominator: usize,
) -> u32 {
    let lags = sorted_lags(case);
    if lags.is_empty() {
        return 0;
    }
    let index = ((lags.len() - 1) * numerator) / denominator;
    lags[index]
}

pub(crate) fn latest_lag(case: &SurfaceLagCalibrationCase) -> u32 {
    case.observed_lag_hours.last().copied().unwrap_or(0)
}

pub(crate) fn max_lag(case: &SurfaceLagCalibrationCase) -> u32 {
    case.observed_lag_hours.iter().copied().max().unwrap_or(0)
}

pub(crate) fn outlier_count(case: &SurfaceLagCalibrationCase) -> usize {
    let p50 = percentile_lag(case, 1, 2);
    case.observed_lag_hours
        .iter()
        .filter(|lag| **lag >= p50.saturating_mul(3).max(p50 + 18))
        .count()
}

pub(crate) fn apply_budget_decision(
    current_budget_hours: u32,
    selected_budget_hours: u32,
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> SurfaceLagCalibrationDecision {
    let decision_kind = if selected_budget_hours > current_budget_hours {
        "budget_raise"
    } else if selected_budget_hours < current_budget_hours {
        "budget_lower"
    } else {
        "budget_keep"
    };
    SurfaceLagCalibrationDecision {
        decision_kind: decision_kind.to_string(),
        selected_budget_hours: Some(selected_budget_hours),
        rationale: rationale.into(),
        consumed_signals,
    }
}

pub(crate) fn hold_budget(
    current_budget_hours: u32,
    rationale: impl Into<String>,
    consumed_signals: usize,
) -> SurfaceLagCalibrationDecision {
    SurfaceLagCalibrationDecision {
        decision_kind: "budget_hold".to_string(),
        selected_budget_hours: Some(current_budget_hours),
        rationale: rationale.into(),
        consumed_signals,
    }
}

fn decision_matches(
    case: &SurfaceLagCalibrationCase,
    decision: &SurfaceLagCalibrationDecision,
) -> bool {
    decision.decision_kind == case.expected_decision_kind
        && decision.selected_budget_hours == case.expected_budget_hours
}

fn load_cases(root: &Path) -> Result<Vec<SurfaceLagCalibrationCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn SurfaceLagCalibrationVariant,
    case: &SurfaceLagCalibrationCase,
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
) -> Result<SurfaceLagCalibrationSourceMetrics> {
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
        "default_budget(",
        "budget_step(",
        "round_up_to_step(",
        "percentile_lag(",
        "latest_lag(",
        "max_lag(",
        "outlier_count(",
        "apply_budget_decision(",
        "hold_budget(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(SurfaceLagCalibrationSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_decision_refs,
        evidence_refs,
    })
}

fn readability_score(source: &SurfaceLagCalibrationSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &SurfaceLagCalibrationSourceMetrics) -> u32 {
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
    report: &SurfaceLagCalibrationExperimentReport,
) -> Vec<&SurfaceLagCalibrationVariantReport> {
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
    report: &SurfaceLagCalibrationExperimentReport,
) -> Option<&SurfaceLagCalibrationVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_consumed_signals
            .partial_cmp(&right.average_consumed_signals)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.extensibility_score.cmp(&right.extensibility_score))
    })
}
