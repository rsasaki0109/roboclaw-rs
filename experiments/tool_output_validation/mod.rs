use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

mod array_membership;
mod normalized_scalars;
mod strict_subset_clone;
mod tolerant_contract;

use array_membership::ArrayMembershipVariant;
use normalized_scalars::NormalizedScalarsVariant;
use strict_subset_clone::StrictSubsetCloneVariant;
use tolerant_contract::TolerantContractVariant;

const CASES_PATH: &str = "experiments/tool_output_validation/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationCase {
    pub id: String,
    pub expectation: Value,
    pub output: Value,
    pub expected_match: bool,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationDecision {
    pub matched: bool,
    pub checked_fields: usize,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationVariantCaseResult {
    pub case_id: String,
    pub correct: bool,
    pub expected_match: bool,
    pub selected_match: bool,
    pub checked_fields: usize,
    pub average_decision_us: f64,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationVariantSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_field_refs: usize,
    pub matcher_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: ValidationVariantSourceMetrics,
    pub cases: Vec<ValidationVariantCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_checked_fields: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<ValidationCase>,
    pub variants: Vec<ValidationVariantReport>,
}

pub trait ValidationVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn validate(&self, case: &ValidationCase) -> Result<ValidationDecision>;
}

pub fn run_suite(root: &Path) -> Result<ValidationExperimentReport> {
    let cases = load_cases(root)?;
    let field_names = field_names(&cases);
    let variants: Vec<Box<dyn ValidationVariant>> = vec![
        Box::new(StrictSubsetCloneVariant::default()),
        Box::new(NormalizedScalarsVariant::default()),
        Box::new(ArrayMembershipVariant::default()),
        Box::new(TolerantContractVariant::default()),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &field_names)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;
        let mut total_checked_fields = 0usize;

        for case in &cases {
            let decision = variant.validate(case).with_context(|| {
                format!(
                    "variant '{}' failed to validate case '{}'",
                    variant.name(),
                    case.id
                )
            })?;
            let bench_average_us = benchmark_variant(variant.as_ref(), case)?;
            let is_correct = decision.matched == case.expected_match;
            if is_correct {
                correct += 1;
            }
            total_average_us += bench_average_us;
            total_checked_fields += decision.checked_fields;

            case_results.push(ValidationVariantCaseResult {
                case_id: case.id.clone(),
                correct: is_correct,
                expected_match: case.expected_match,
                selected_match: decision.matched,
                checked_fields: decision.checked_fields,
                average_decision_us: bench_average_us,
                rationale: decision.rationale,
            });
        }

        reports.push(ValidationVariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            cases: case_results,
            accuracy_pct: correct as f64 * 100.0 / cases.len() as f64,
            average_decision_us: total_average_us / cases.len() as f64,
            average_checked_fields: total_checked_fields as f64 / cases.len() as f64,
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
        });
    }

    Ok(ValidationExperimentReport {
        problem: "Tool output validation should evolve through comparable matching contracts instead of freezing one expectation matcher into the runtime.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &ValidationExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} accuracy_pct={:.1} avg_decision_us={:.2} avg_checked_fields={:.2} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_checked_fields,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &ValidationExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## Tool Output Validation\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("| case | expected match | why |\n");
    markdown.push_str("| --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | `{}` | {} |\n",
            case.id, case.expected_match, case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str(
        "| variant | style | accuracy | avg us | avg checked fields | readability | extensibility |\n",
    );
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | {} | {:.1}% | {:.2} | {:.2} | {} | {} |\n",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_checked_fields,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &ValidationExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## Tool Output Validation\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while keeping the broadest reusable matcher surface (extensibility {}).\n",
            reference.name, reference.extensibility_score
        ));
    }
    markdown.push_str("- Keep the runtime matcher narrow until real tool outputs add more nested arrays, numeric coercion, and ambiguity cases.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## Tool Output Validation\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct ValidationCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub expectation: serde_json::Value,\n");
    markdown.push_str("    pub output: serde_json::Value,\n");
    markdown.push_str("    pub expected_match: bool,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ValidationDecision {\n");
    markdown.push_str("    pub matched: bool,\n");
    markdown.push_str("    pub checked_fields: usize,\n");
    markdown.push_str("    pub rationale: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait ValidationVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn validate(&self, case: &ValidationCase) -> anyhow::Result<ValidationDecision>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str("- Shared input: same expectation/output JSON pairs for all variants.\n");
    markdown.push_str("- Shared metrics: accuracy, average decision time, average checked fields, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn decision(
    matched: bool,
    checked_fields: usize,
    rationale: impl Into<String>,
) -> ValidationDecision {
    ValidationDecision {
        matched,
        checked_fields,
        rationale: rationale.into(),
    }
}

pub(crate) fn leaf_checks(value: &Value) -> usize {
    match value {
        Value::Object(map) => map.values().map(leaf_checks).sum(),
        Value::Array(values) => values.iter().map(leaf_checks).sum(),
        _ => 1,
    }
}

pub(crate) fn normalize_string(value: &str) -> String {
    value
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
        .replace('-', "_")
        .to_lowercase()
}

pub(crate) fn parse_bool(value: &str) -> Option<bool> {
    match normalize_string(value).as_str() {
        "true" | "yes" | "on" => Some(true),
        "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub(crate) fn parse_number(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok()
}

pub(crate) fn strict_subset(actual: &Value, expected: &Value) -> bool {
    match expected {
        Value::Object(expected_map) => {
            let Some(actual_map) = actual.as_object() else {
                return false;
            };
            expected_map.iter().all(|(key, expected_value)| {
                actual_map
                    .get(key)
                    .map(|actual_value| strict_subset(actual_value, expected_value))
                    .unwrap_or(false)
            })
        }
        Value::Array(expected_values) => {
            let Some(actual_values) = actual.as_array() else {
                return false;
            };
            actual_values.len() == expected_values.len()
                && actual_values.iter().zip(expected_values.iter()).all(
                    |(actual_value, expected_value)| strict_subset(actual_value, expected_value),
                )
        }
        _ => actual == expected,
    }
}

pub(crate) fn normalized_subset(actual: &Value, expected: &Value) -> bool {
    match expected {
        Value::Object(expected_map) => {
            let Some(actual_map) = actual.as_object() else {
                return false;
            };
            expected_map.iter().all(|(key, expected_value)| {
                actual_map
                    .get(key)
                    .map(|actual_value| normalized_subset(actual_value, expected_value))
                    .unwrap_or(false)
            })
        }
        Value::Array(expected_values) => {
            let Some(actual_values) = actual.as_array() else {
                return false;
            };
            actual_values.len() == expected_values.len()
                && actual_values.iter().zip(expected_values.iter()).all(
                    |(actual_value, expected_value)| {
                        normalized_subset(actual_value, expected_value)
                    },
                )
        }
        _ => normalized_scalar_eq(actual, expected),
    }
}

pub(crate) fn array_membership_subset(actual: &Value, expected: &Value) -> bool {
    match expected {
        Value::Object(expected_map) => {
            let Some(actual_map) = actual.as_object() else {
                return false;
            };
            expected_map.iter().all(|(key, expected_value)| {
                actual_map
                    .get(key)
                    .map(|actual_value| array_membership_subset(actual_value, expected_value))
                    .unwrap_or(false)
            })
        }
        Value::Array(expected_values) => {
            let Some(actual_values) = actual.as_array() else {
                return false;
            };
            expected_values.iter().all(|expected_value| {
                actual_values
                    .iter()
                    .any(|actual_value| array_membership_subset(actual_value, expected_value))
            })
        }
        _ => actual == expected,
    }
}

pub(crate) fn tolerant_subset(actual: &Value, expected: &Value) -> bool {
    match expected {
        Value::Object(expected_map) => {
            let Some(actual_map) = actual.as_object() else {
                return false;
            };
            expected_map.iter().all(|(key, expected_value)| {
                actual_map
                    .get(key)
                    .map(|actual_value| tolerant_subset(actual_value, expected_value))
                    .unwrap_or(false)
            })
        }
        Value::Array(expected_values) => {
            let Some(actual_values) = actual.as_array() else {
                return false;
            };
            expected_values.iter().all(|expected_value| {
                actual_values
                    .iter()
                    .any(|actual_value| tolerant_subset(actual_value, expected_value))
            })
        }
        _ => normalized_scalar_eq(actual, expected),
    }
}

fn normalized_scalar_eq(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::String(actual_text), Value::String(expected_text)) => {
            normalize_string(actual_text) == normalize_string(expected_text)
        }
        (Value::String(actual_text), Value::Bool(expected_bool)) => {
            parse_bool(actual_text) == Some(*expected_bool)
        }
        (Value::Bool(actual_bool), Value::String(expected_text)) => {
            parse_bool(expected_text) == Some(*actual_bool)
        }
        (Value::String(actual_text), Value::Number(expected_number)) => parse_number(actual_text)
            .zip(expected_number.as_f64())
            .map(|(actual_number, expected_number)| {
                (actual_number - expected_number).abs() < f64::EPSILON
            })
            .unwrap_or(false),
        (Value::Number(actual_number), Value::String(expected_text)) => actual_number
            .as_f64()
            .zip(parse_number(expected_text))
            .map(|(actual_number, expected_number)| {
                (actual_number - expected_number).abs() < f64::EPSILON
            })
            .unwrap_or(false),
        _ => actual == expected,
    }
}

fn load_cases(root: &Path) -> Result<Vec<ValidationCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(variant: &dyn ValidationVariant, case: &ValidationCase) -> Result<f64> {
    let _ = variant.validate(case)?;
    let start = Instant::now();
    for _ in 0..BENCH_ITERATIONS {
        let _ = variant.validate(case)?;
    }
    Ok(start.elapsed().as_secs_f64() * 1_000_000.0 / BENCH_ITERATIONS as f64)
}

fn collect_source_metrics(
    root: &Path,
    source_path: &str,
    field_names: &[String],
) -> Result<ValidationVariantSourceMetrics> {
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
    let branch_tokens = [
        "if ", "match ", " for ", "while ", "&&", "||", ".all(", ".any(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();
    let hardcoded_field_refs = field_names
        .iter()
        .map(|field| source.matches(&format!("\"{field}\"")).count())
        .sum();
    let matcher_refs = [
        "leaf_checks(",
        "normalize_string(",
        "parse_bool(",
        "parse_number(",
        "strict_subset(",
        "normalized_subset(",
        "array_membership_subset(",
        "tolerant_subset(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(ValidationVariantSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_field_refs,
        matcher_refs,
    })
}

fn field_names(cases: &[ValidationCase]) -> Vec<String> {
    let mut field_names = BTreeSet::new();
    for case in cases {
        collect_field_names(&case.expectation, &mut field_names);
        collect_field_names(&case.output, &mut field_names);
    }
    field_names.into_iter().collect()
}

fn collect_field_names(value: &Value, names: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                names.insert(key.clone());
                collect_field_names(nested, names);
            }
        }
        Value::Array(values) => {
            for nested in values {
                collect_field_names(nested, names);
            }
        }
        _ => {}
    }
}

fn readability_score(source: &ValidationVariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &ValidationVariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_field_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.matcher_refs as i32 * 6)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &ValidationExperimentReport) -> Vec<&ValidationVariantReport> {
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

fn provisional_frontier(report: &ValidationExperimentReport) -> Option<&ValidationVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.extensibility_score
            .cmp(&right.extensibility_score)
            .then_with(|| left.readability_score.cmp(&right.readability_score))
    })
}
