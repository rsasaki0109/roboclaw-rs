use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::Instant;

mod action_state_join;
mod latest_topic_join;
mod state_only_snapshot;
mod stream_reducer;

use action_state_join::ActionStateJoinVariant;
use latest_topic_join::LatestTopicJoinVariant;
use state_only_snapshot::StateOnlySnapshotVariant;
use stream_reducer::StreamReducerVariant;

const CASES_PATH: &str = "experiments/ros2_observation_ingestion/cases.json";
const BENCH_ITERATIONS: usize = 400;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationEvent {
    pub topic: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionCase {
    pub id: String,
    pub events: Vec<ObservationEvent>,
    pub expected_active_skill: Option<String>,
    pub expected_last_pose: Option<String>,
    pub expected_held_object: Option<String>,
    pub expected_failed_step: Option<String>,
    pub expected_resume_step: Option<String>,
    pub expected_motion_state: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationSummary {
    pub active_skill: Option<String>,
    pub last_pose: Option<String>,
    pub held_object: Option<String>,
    pub failed_step: Option<String>,
    pub resume_step: Option<String>,
    pub motion_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionCaseResult {
    pub case_id: String,
    pub correct: bool,
    pub expected_active_skill: Option<String>,
    pub selected_active_skill: Option<String>,
    pub expected_last_pose: Option<String>,
    pub selected_last_pose: Option<String>,
    pub expected_failed_step: Option<String>,
    pub selected_failed_step: Option<String>,
    pub expected_resume_step: Option<String>,
    pub selected_resume_step: Option<String>,
    pub expected_motion_state: String,
    pub selected_motion_state: String,
    pub context_fields: usize,
    pub average_decision_us: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionVariantSourceMetrics {
    pub source_path: String,
    pub loc_non_empty: usize,
    pub branch_tokens: usize,
    pub helper_functions: usize,
    pub hardcoded_event_refs: usize,
    pub topic_refs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionVariantReport {
    pub name: String,
    pub style: String,
    pub philosophy: String,
    pub source: IngestionVariantSourceMetrics,
    pub cases: Vec<IngestionCaseResult>,
    pub accuracy_pct: f64,
    pub average_decision_us: f64,
    pub average_context_fields: f64,
    pub readability_score: u32,
    pub extensibility_score: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestionExperimentReport {
    pub problem: String,
    pub generated_by: String,
    pub cases: Vec<IngestionCase>,
    pub variants: Vec<IngestionVariantReport>,
}

pub trait ObservationIngestionVariant {
    fn name(&self) -> &'static str;
    fn style(&self) -> &'static str;
    fn philosophy(&self) -> &'static str;
    fn source_path(&self) -> &'static str;
    fn ingest(&self, case: &IngestionCase) -> Result<ObservationSummary>;
}

pub fn run_suite(root: &Path) -> Result<IngestionExperimentReport> {
    let cases = load_cases(root)?;
    let event_refs = vec![
        "skill_selected".to_string(),
        "step_failed".to_string(),
        "execution_replan_requested".to_string(),
        "recovery_completed".to_string(),
        "/roboclaw/action".to_string(),
        "/roboclaw/state".to_string(),
        "/joint_states".to_string(),
        "/cmd_vel".to_string(),
    ];
    let variants: Vec<Box<dyn ObservationIngestionVariant>> = vec![
        Box::new(StateOnlySnapshotVariant::default()),
        Box::new(LatestTopicJoinVariant::default()),
        Box::new(ActionStateJoinVariant::default()),
        Box::new(StreamReducerVariant::default()),
    ];

    let mut reports = Vec::new();
    for variant in variants {
        let source = collect_source_metrics(root, variant.source_path(), &event_refs)?;
        let mut case_results = Vec::new();
        let mut correct = 0usize;
        let mut total_average_us = 0.0f64;
        let mut total_context_fields = 0usize;

        for case in &cases {
            let summary = variant.ingest(case).with_context(|| {
                format!(
                    "variant '{}' failed to ingest case '{}'",
                    variant.name(),
                    case.id
                )
            })?;
            let bench_average_us = benchmark_variant(variant.as_ref(), case)?;
            let context_fields = context_fields(&summary);
            let is_correct = summary_matches(case, &summary);
            if is_correct {
                correct += 1;
            }
            total_average_us += bench_average_us;
            total_context_fields += context_fields;

            case_results.push(IngestionCaseResult {
                case_id: case.id.clone(),
                correct: is_correct,
                expected_active_skill: case.expected_active_skill.clone(),
                selected_active_skill: summary.active_skill.clone(),
                expected_last_pose: case.expected_last_pose.clone(),
                selected_last_pose: summary.last_pose.clone(),
                expected_failed_step: case.expected_failed_step.clone(),
                selected_failed_step: summary.failed_step.clone(),
                expected_resume_step: case.expected_resume_step.clone(),
                selected_resume_step: summary.resume_step.clone(),
                expected_motion_state: case.expected_motion_state.clone(),
                selected_motion_state: summary.motion_state.clone(),
                context_fields,
                average_decision_us: bench_average_us,
            });
        }

        reports.push(IngestionVariantReport {
            name: variant.name().to_string(),
            style: variant.style().to_string(),
            philosophy: variant.philosophy().to_string(),
            source: source.clone(),
            cases: case_results,
            accuracy_pct: correct as f64 * 100.0 / cases.len() as f64,
            average_decision_us: total_average_us / cases.len() as f64,
            average_context_fields: total_context_fields as f64 / cases.len() as f64,
            readability_score: readability_score(&source),
            extensibility_score: extensibility_score(&source),
        });
    }

    Ok(IngestionExperimentReport {
        problem: "ROS2 and Gazebo observations should be ingested through comparable event-reduction shapes instead of freezing one subscriber topology into the runtime.".to_string(),
        generated_by: "cargo run --example planner_experiments -- --write-docs".to_string(),
        cases,
        variants: reports,
    })
}

pub fn render_summary(report: &IngestionExperimentReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("problem={}", report.problem));
    lines.push(format!("cases={}", report.cases.len()));
    for variant in &report.variants {
        lines.push(format!(
            "variant={} style={} accuracy_pct={:.1} avg_decision_us={:.2} avg_context_fields={:.2} readability_score={} extensibility_score={}",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_context_fields,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    lines.join("\n")
}

pub fn render_experiments_section(report: &IngestionExperimentReport) -> String {
    let mut markdown = String::new();
    markdown.push_str("## ROS2 Observation Ingestion\n\n");
    markdown.push_str(&format!("{}\n\n", report.problem));
    markdown.push_str("| case | events | expected skill | expected pose | why |\n");
    markdown.push_str("| --- | --- | --- | --- | --- |\n");
    for case in &report.cases {
        markdown.push_str(&format!(
            "| `{}` | {} | `{}` | `{}` | {} |\n",
            case.id,
            case.events.len(),
            case.expected_active_skill.as_deref().unwrap_or("none"),
            case.expected_last_pose.as_deref().unwrap_or("none"),
            case.why
        ));
    }
    markdown.push('\n');

    markdown.push_str(
        "| variant | style | accuracy | avg us | avg context fields | readability | extensibility |\n",
    );
    markdown.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for variant in &report.variants {
        markdown.push_str(&format!(
            "| `{}` | {} | {:.1}% | {:.2} | {:.2} | {} | {} |\n",
            variant.name,
            variant.style,
            variant.accuracy_pct,
            variant.average_decision_us,
            variant.average_context_fields,
            variant.readability_score,
            variant.extensibility_score
        ));
    }
    markdown.push('\n');
    markdown
}

pub fn render_decisions_section(report: &IngestionExperimentReport) -> String {
    let frontier = frontier_variants(report)
        .into_iter()
        .map(|variant| format!("`{}`", variant.name))
        .collect::<Vec<_>>()
        .join(", ");

    let mut markdown = String::new();
    markdown.push_str("## ROS2 Observation Ingestion\n\n");
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
            "- Provisional reference: `{}` because it stayed on the frontier while retaining the richest context ({:.2} fields).\n",
            reference.name, reference.average_context_fields
        ));
    }
    markdown.push_str("- Keep ROS2 ingestion policy outside the stable runtime until real delayed and out-of-order telemetry traces expand the case set.\n");
    markdown
}

pub fn render_interfaces_section() -> String {
    let mut markdown = String::new();
    markdown.push_str("## ROS2 Observation Ingestion\n\n");
    markdown.push_str("```rust\n");
    markdown.push_str("pub struct ObservationEvent {\n");
    markdown.push_str("    pub topic: String,\n");
    markdown.push_str("    pub payload: serde_json::Value,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct IngestionCase {\n");
    markdown.push_str("    pub id: String,\n");
    markdown.push_str("    pub events: Vec<ObservationEvent>,\n");
    markdown.push_str("    pub expected_active_skill: Option<String>,\n");
    markdown.push_str("    pub expected_last_pose: Option<String>,\n");
    markdown.push_str("    pub expected_held_object: Option<String>,\n");
    markdown.push_str("    pub expected_failed_step: Option<String>,\n");
    markdown.push_str("    pub expected_resume_step: Option<String>,\n");
    markdown.push_str("    pub expected_motion_state: String,\n");
    markdown.push_str("    pub why: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub struct ObservationSummary {\n");
    markdown.push_str("    pub active_skill: Option<String>,\n");
    markdown.push_str("    pub last_pose: Option<String>,\n");
    markdown.push_str("    pub held_object: Option<String>,\n");
    markdown.push_str("    pub failed_step: Option<String>,\n");
    markdown.push_str("    pub resume_step: Option<String>,\n");
    markdown.push_str("    pub motion_state: String,\n");
    markdown.push_str("}\n\n");
    markdown.push_str("pub trait ObservationIngestionVariant {\n");
    markdown.push_str("    fn name(&self) -> &'static str;\n");
    markdown.push_str("    fn style(&self) -> &'static str;\n");
    markdown.push_str("    fn philosophy(&self) -> &'static str;\n");
    markdown.push_str("    fn source_path(&self) -> &'static str;\n");
    markdown.push_str(
        "    fn ingest(&self, case: &IngestionCase) -> anyhow::Result<ObservationSummary>;\n",
    );
    markdown.push_str("}\n");
    markdown.push_str("```\n\n");
    markdown.push_str(
        "- Shared input: same ordered ROS2 topic event list for all ingestion variants.\n",
    );
    markdown.push_str("- Shared metrics: accuracy, average decision time, average context fields, readability proxy, extensibility proxy.\n");
    markdown
}

pub(crate) fn summary(
    active_skill: Option<String>,
    last_pose: Option<String>,
    held_object: Option<String>,
    failed_step: Option<String>,
    resume_step: Option<String>,
    motion_state: impl Into<String>,
) -> ObservationSummary {
    ObservationSummary {
        active_skill,
        last_pose,
        held_object,
        failed_step,
        resume_step,
        motion_state: motion_state.into(),
    }
}

pub(crate) fn action_event<'a>(event: &'a ObservationEvent) -> Option<&'a Value> {
    (event.topic == "/roboclaw/action").then_some(&event.payload)
}

pub(crate) fn state_event<'a>(event: &'a ObservationEvent) -> Option<&'a Value> {
    (event.topic == "/roboclaw/state").then_some(&event.payload)
}

pub(crate) fn joint_event<'a>(event: &'a ObservationEvent) -> Option<&'a Value> {
    (event.topic == "/joint_states").then_some(&event.payload)
}

pub(crate) fn cmd_vel_event<'a>(event: &'a ObservationEvent) -> Option<&'a Value> {
    (event.topic == "/cmd_vel").then_some(&event.payload)
}

pub(crate) fn motion_state_from_cmd(payload: &Value) -> Option<String> {
    let linear = payload.get("linear").and_then(Value::as_f64).unwrap_or(0.0);
    let angular = payload
        .get("angular")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    Some(
        if linear.abs() > f64::EPSILON || angular.abs() > f64::EPSILON {
            "moving".to_string()
        } else {
            "idle".to_string()
        },
    )
}

pub(crate) fn pose_from_state(payload: &Value) -> Option<String> {
    payload
        .get("last_pose")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn held_object_from_state(payload: &Value) -> Option<String> {
    payload
        .get("held_object")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn active_skill_from_state(payload: &Value) -> Option<String> {
    payload
        .get("active_skill")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn failed_step_from_state(payload: &Value) -> Option<String> {
    payload
        .get("failed_step")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn skill_from_action(payload: &Value) -> Option<String> {
    payload
        .get("skill")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn failed_step_from_action(payload: &Value) -> Option<String> {
    payload
        .get("step")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn resume_step_from_action(payload: &Value) -> Option<String> {
    payload
        .get("data")
        .and_then(|data| {
            data.get("resume_from_step")
                .or_else(|| data.get("resumed_from_step"))
        })
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(crate) fn event_name(payload: &Value) -> Option<&str> {
    payload.get("event").and_then(Value::as_str)
}

pub(crate) fn pose_from_joint_state(payload: &Value) -> Option<String> {
    let positions = payload.get("positions")?.as_array()?;
    let values = positions
        .iter()
        .map(Value::as_f64)
        .collect::<Option<Vec<_>>>()?;

    if matches_pose(&values, &[0.45, 0.1, 1.0]) {
        Some("table/front_left/pre_grasp".to_string())
    } else if matches_pose(&values, &[0.7, 0.2, 0.2]) {
        Some("bin_a".to_string())
    } else if matches_pose(&values, &[0.8, -0.4, 1.0]) {
        Some("gesture/wave_start".to_string())
    } else if matches_pose(&values, &[0.9, 0.45, 1.0]) {
        Some("gesture/wave_peak".to_string())
    } else if matches_pose(&values, &[0.0, 0.0, 1.0]) {
        Some("home".to_string())
    } else {
        None
    }
}

pub(crate) fn latest_topic<'a>(case: &'a IngestionCase, topic: &str) -> Option<&'a Value> {
    case.events
        .iter()
        .rev()
        .find(|event| event.topic == topic)
        .map(|event| &event.payload)
}

fn matches_pose(actual: &[f64], expected: &[f64]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected.iter())
            .all(|(actual, expected)| (actual - expected).abs() < 0.001)
}

fn summary_matches(case: &IngestionCase, summary: &ObservationSummary) -> bool {
    summary.active_skill == case.expected_active_skill
        && summary.last_pose == case.expected_last_pose
        && summary.held_object == case.expected_held_object
        && summary.failed_step == case.expected_failed_step
        && summary.resume_step == case.expected_resume_step
        && summary.motion_state == case.expected_motion_state
}

fn context_fields(summary: &ObservationSummary) -> usize {
    [
        summary.active_skill.is_some(),
        summary.last_pose.is_some(),
        summary.held_object.is_some(),
        summary.failed_step.is_some(),
        summary.resume_step.is_some(),
        summary.motion_state != "unknown",
    ]
    .into_iter()
    .filter(|present| *present)
    .count()
}

fn load_cases(root: &Path) -> Result<Vec<IngestionCase>> {
    let path = root.join(CASES_PATH);
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {:?}", path))?;
    serde_json::from_str(&content).with_context(|| format!("failed to parse {:?}", path))
}

fn benchmark_variant(
    variant: &dyn ObservationIngestionVariant,
    case: &IngestionCase,
) -> Result<f64> {
    let _ = variant.ingest(case)?;
    let start = Instant::now();
    for _ in 0..BENCH_ITERATIONS {
        let _ = variant.ingest(case)?;
    }
    Ok(start.elapsed().as_secs_f64() * 1_000_000.0 / BENCH_ITERATIONS as f64)
}

fn collect_source_metrics(
    root: &Path,
    source_path: &str,
    event_refs: &[String],
) -> Result<IngestionVariantSourceMetrics> {
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
        "if ", "match ", " for ", "while ", "&&", "||", ".find(", ".rev(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();
    let hardcoded_event_refs = event_refs
        .iter()
        .map(|reference| source.matches(&format!("\"{reference}\"")).count())
        .sum();
    let topic_refs = [
        "action_event(",
        "state_event(",
        "joint_event(",
        "cmd_vel_event(",
        "motion_state_from_cmd(",
        "pose_from_state(",
        "pose_from_joint_state(",
        "resume_step_from_action(",
    ]
    .into_iter()
    .map(|token| source.matches(token).count())
    .sum();

    Ok(IngestionVariantSourceMetrics {
        source_path: source_path.to_string(),
        loc_non_empty,
        branch_tokens,
        helper_functions,
        hardcoded_event_refs,
        topic_refs,
    })
}

fn readability_score(source: &IngestionVariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.loc_non_empty as i32 / 2) - (source.branch_tokens as i32 * 3)
            + (source.helper_functions as i32 * 4),
    )
}

fn extensibility_score(source: &IngestionVariantSourceMetrics) -> u32 {
    clamp_score(
        100 - (source.hardcoded_event_refs as i32 * 8) - (source.branch_tokens as i32 * 2)
            + (source.topic_refs as i32 * 6)
            + (source.helper_functions as i32 * 2),
    )
}

fn clamp_score(value: i32) -> u32 {
    value.clamp(0, 100) as u32
}

fn frontier_variants(report: &IngestionExperimentReport) -> Vec<&IngestionVariantReport> {
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

fn provisional_frontier(report: &IngestionExperimentReport) -> Option<&IngestionVariantReport> {
    frontier_variants(report).into_iter().max_by(|left, right| {
        left.average_context_fields
            .partial_cmp(&right.average_context_fields)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                (left.readability_score + left.extensibility_score)
                    .cmp(&(right.readability_score + right.extensibility_score))
            })
    })
}
