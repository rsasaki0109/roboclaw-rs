mod common;
use common::Project;
use roboclaw_rs::jobs::Jobs;
use roboclaw_rs::runtime::{RunRequest, Workspace};
use roboclaw_rs::storage::{now_millis, Lease};
use roboclaw_rs::tools::ExecutionControl;
use serde_json::{json, Value};
use std::fs;
use std::time::Duration;

#[test]
fn strict_configuration_defaults_overrides_and_doctor_are_read_only() {
    let project = Project::new();
    assert_eq!(project.json(&["doctor"])["ok"], true);
    assert!(!project.root.join("target").exists());
    project.config("planner:\n  provider: mock\nmax_replans: 0\ntools:\n  deny: [motor_control]\n");
    assert_eq!(project.json(&["config", "show"])["max_replans"], 0);
    assert_eq!(project.json(&["tools", "list"])[2]["allowed"], false);
    project.config("timeuot: 20s\n");
    let output = project
        .command()
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert!(report["checks"][0]["detail"]
        .as_str()
        .unwrap()
        .contains("timeuot"));
    assert!(!project.root.join("target").exists());
}

#[test]
fn config_init_preserves_existing_files_and_rejects_invalid_bounds() {
    let project = Project::new();
    project.json(&["config", "init"]);
    project.json(&["config", "init", "--config", "custom.yaml"]);
    assert!(project.root.join("custom.yaml").exists());
    let before = fs::read(project.root.join("roboclaw.yaml")).unwrap();
    assert_eq!(
        project
            .command()
            .args(["config", "init"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        before,
        fs::read(project.root.join("roboclaw.yaml")).unwrap()
    );
    for yaml in [
        "version: 2",
        "timeout: 0s",
        "max_replans: 11",
        "tools:\n  allow: [shell]",
        "planner:\n  fallbacks: [auto]",
    ] {
        project.config(yaml);
        assert_eq!(
            project
                .command()
                .args(["config", "check"])
                .output()
                .unwrap()
                .status
                .code(),
            Some(1)
        );
    }
}

#[test]
fn explicit_provider_is_strict_and_fallback_must_be_requested() {
    let project = Project::new();
    project.config("planner:\n  provider: openai\n  fallbacks: [mock]\n");
    let plan = project.json(&["plan", "wave_arm"]);
    assert_eq!(plan["planner_provider"], "mock");
    assert!(plan["decision"]["reason"]
        .as_str()
        .unwrap()
        .contains("openai -> mock"));
    let output = project
        .command()
        .args(["plan", "wave_arm", "--provider", "openai", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        project.json(&[
            "plan",
            "wave_arm",
            "--provider",
            "openai",
            "--fallback",
            "mock"
        ])["planner_provider"],
        "mock"
    );
    let output = project
        .command()
        .args(["plan", "wave_arm", "--no-fallbacks"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!project.root.join("target").exists());
}

#[test]
fn workspace_skills_override_extensions_and_duplicate_names_fail() {
    let project = Project::new();
    fs::create_dir(project.root.join("extensions")).unwrap();
    fs::write(
        project.root.join("extensions/wave.yaml"),
        "name: wave_arm\ndescription: extension override\nsteps: []",
    )
    .unwrap();
    fs::write(
        project.root.join("extensions/inspect.yaml"),
        "name: inspect\ndescription: inspect robot\nsteps: []",
    )
    .unwrap();
    project.config("skill_dirs: [extensions]\n");
    assert_eq!(
        project.json(&["skills", "list"]).as_array().unwrap().len(),
        5
    );
    assert_ne!(
        project.json(&["skills", "show", "wave_arm"])["description"],
        "extension override"
    );
    fs::write(
        project.root.join("extensions/duplicate.yaml"),
        "name: inspect\ndescription: duplicate\nsteps: []",
    )
    .unwrap();
    let output = project.command().args(["skills", "list"]).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("duplicate skill 'inspect'"));
}

#[test]
fn skill_validator_checks_unknown_tools_and_checkpoint_references() {
    let project = Project::new();
    project.json(&["skills", "validate"]);
    fs::write(
        project.root.join("skills/bad.yaml"),
        "name: bad\ndescription: bad\nsteps:\n  - name: one\n    tool: shell",
    )
    .unwrap();
    assert_eq!(
        project
            .command()
            .args(["skills", "validate"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    fs::write(project.root.join("skills/bad.yaml"), "name: bad\ndescription: bad\nsteps:\n  - name: one\n    tool: sensor\n    input: {target: red_cube}\n    resume_from_step: missing").unwrap();
    let output = project
        .command()
        .args(["skills", "validate"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing checkpoint"));
}

#[test]
fn policy_preflight_prevents_earlier_operations_and_records_failure() {
    let project = Project::new();
    project.config("tools:\n  deny: [motor_control]\n");
    let output = project
        .command()
        .args(["run", "pick and place", "--session", "policy", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("denied by execution policy"));
    let runs = project.json(&["runs", "list"]);
    assert_eq!(runs[0]["status"], "failed");
    assert_eq!(runs[0]["result"], Value::Null);
    let events = project.json(&["runs", "events", runs[0]["id"].as_str().unwrap()]);
    assert!(!events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "tool_invoked"));
}

#[test]
fn invalid_later_input_is_rejected_before_the_first_tool() {
    let project = Project::new();
    fs::write(project.root.join("skills/wave_arm.yaml"), "name: wave_arm\ndescription: wave\nsteps:\n  - name: move\n    tool: simulator\n    input: {action: move_to, pose: home}\n  - name: invalid_grasp\n    tool: motor_control\n    input: {action: grasp}\n").unwrap();
    assert_eq!(
        project
            .command()
            .args(["run", "wave_arm"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(1)
    );
    let runs = project.json(&["runs", "list"]);
    let events = project.json(&["runs", "events", runs[0]["id"].as_str().unwrap()]);
    assert!(!events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "tool_invoked"));
}

#[test]
fn sessions_isolate_notes_runs_and_memory_search() {
    let project = Project::new();
    project.json(&[
        "memory",
        "remember",
        "Calibration marker alpha42",
        "--session",
        "alpha",
    ]);
    project.json(&[
        "memory",
        "remember",
        "Calibration marker beta73",
        "--session",
        "beta",
    ]);
    assert_eq!(
        project.json(&["memory", "search", "alpha42", "--session", "beta"]),
        json!([])
    );
    assert!(!project
        .json(&["memory", "search", "alpha42", "--session", "alpha"])
        .as_array()
        .unwrap()
        .is_empty());
    let run = project.json(&["run", "wave_arm", "--session", "alpha"]);
    assert_eq!(run["session"], "alpha");
    let session = project.json(&["sessions", "show", "alpha"]);
    assert_eq!(session["session"]["run_count"], 1);
    assert_eq!(session["runs"][0]["id"], run["run_id"]);
    let events = project.json(&["runs", "events", run["run_id"].as_str().unwrap()]);
    assert_eq!(events[0]["seq"], 1);
    let last = events.as_array().unwrap().last().unwrap()["seq"]
        .as_u64()
        .unwrap()
        .to_string();
    assert_eq!(
        project.json(&[
            "runs",
            "events",
            run["run_id"].as_str().unwrap(),
            "--after",
            &last
        ]),
        json!([])
    );
    for id in ["../escape", "", "a/b", "."] {
        assert_eq!(
            project
                .command()
                .args(["run", "wave_arm", "--session", id])
                .output()
                .unwrap()
                .status
                .code(),
            Some(1)
        );
    }
}

#[test]
fn a_busy_session_rejects_execution_without_new_run_records() {
    let project = Project::new();
    let workspace = Workspace::load(&project.root, None).unwrap();
    let directory = workspace.store.session_dir("busy").unwrap();
    let _lease = Lease::acquire(&directory.join(".session.lock")).unwrap();
    let output = project
        .command()
        .args(["run", "wave_arm", "--session", "busy"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("resource is busy"));
    assert!(workspace.store.runs().unwrap().is_empty());
}

#[test]
fn streamed_events_are_json_lines_and_end_with_a_result() {
    let project = Project::new();
    let output = project
        .command()
        .args(["run", "wave_arm", "--stream"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(entries[0]["type"], "event");
    assert!(entries
        .iter()
        .any(|entry| entry["event"]["kind"] == "tool_invoked"));
    assert_eq!(entries.last().unwrap()["type"], "result");
    assert_eq!(entries.last().unwrap()["result"]["status"], "completed");
}

#[test]
fn jobs_persist_cancel_and_schedule_without_immediate_robot_actions() {
    let project = Project::new();
    let job = project.json(&["jobs", "add", "wave_arm", "--after", "1h"]);
    assert_eq!(job["status"], "queued");
    assert!(job["request"]["session"]
        .as_str()
        .unwrap()
        .starts_with("job_"));
    assert_eq!(project.json(&["runs", "list"]), json!([]));
    assert_eq!(project.json(&["jobs", "run"]), json!([]));
    assert_eq!(
        project.json(&["jobs", "cancel", job["id"].as_str().unwrap()])["status"],
        "cancelled"
    );
    assert_eq!(project.json(&["jobs", "run"]), json!([]));
    let immediate = project.json(&["jobs", "add", "wave_arm"]);
    let results = project.json(&["jobs", "run"]);
    assert_eq!(results[0]["status"], "completed");
    assert_eq!(results[0]["id"], immediate["id"]);
    assert_eq!(project.json(&["jobs", "run"]), json!([]));
}

#[test]
fn durable_claim_is_not_replayed_after_process_loss_and_runner_is_exclusive() {
    let project = Project::new();
    let workspace = Workspace::load(&project.root, None).unwrap();
    let jobs = Jobs {
        workspace: workspace.clone(),
    };
    let _runner = jobs.runner_lease().unwrap();
    assert!(jobs.runner_lease().is_err());
    let job = jobs
        .add(RunRequest::new("wave_arm"), now_millis(), None, true)
        .unwrap();
    let claimed = jobs.claim_due(now_millis()).unwrap().unwrap();
    assert_eq!(claimed.id, job.id);
    assert_eq!(jobs.recover_interrupted().unwrap(), 1);
    assert_eq!(jobs.get(&job.id).unwrap().status, "interrupted");
    assert!(jobs.claim_due(now_millis()).unwrap().is_none());
    assert!(workspace.store.runs().unwrap().is_empty());
}

#[test]
fn interval_jobs_repeat_only_after_success_and_cancel_stops_future_runs() {
    let project = Project::new();
    let workspace = Workspace::load(&project.root, None).unwrap();
    let jobs = Jobs { workspace };
    let _runner = jobs.runner_lease().unwrap();
    let job = jobs
        .add(
            RunRequest::new("wave_arm"),
            now_millis(),
            Some(Duration::from_secs(60)),
            true,
        )
        .unwrap();
    let claim = jobs.claim_due(now_millis()).unwrap().unwrap();
    let result = jobs
        .execute(claim, &ExecutionControl::default(), None)
        .unwrap();
    assert_eq!(result.status, "queued");
    assert_eq!(result.last_run_status.as_deref(), Some("completed"));
    assert!(result.due_at > now_millis());
    assert!(jobs.claim_due(now_millis()).unwrap().is_none());
    assert_eq!(jobs.cancel(&job.id).unwrap().status, "cancelled");
    assert!(jobs.claim_due(u64::MAX).unwrap().is_none());
}

#[test]
fn cancelled_claim_and_failed_interval_do_not_execute_or_repeat() {
    let project = Project::new();
    let workspace = Workspace::load(&project.root, None).unwrap();
    let jobs = Jobs { workspace };
    let _runner = jobs.runner_lease().unwrap();
    let job = jobs
        .add(RunRequest::new("wave_arm"), now_millis(), None, true)
        .unwrap();
    let claim = jobs.claim_due(now_millis()).unwrap().unwrap();
    jobs.cancel(&job.id).unwrap();
    let result = jobs
        .execute(claim, &ExecutionControl::default(), None)
        .unwrap();
    assert_eq!(result.status, "cancelled");
    assert!(result.result.unwrap().execution.reports.is_empty());
    let mut request = RunRequest::new("wave_arm");
    request.timeout = Some("1ns".into());
    jobs.add(request, now_millis(), Some(Duration::from_secs(60)), true)
        .unwrap();
    let claim = jobs.claim_due(now_millis()).unwrap().unwrap();
    let result = jobs
        .execute(claim, &ExecutionControl::default(), None)
        .unwrap();
    assert_eq!(result.status, "timed_out");
    assert!(jobs.claim_due(u64::MAX).unwrap().is_none());
}

#[test]
fn auto_initialization_can_fall_back_and_an_unclaimed_job_cannot_execute() {
    let project = Project::new();
    let output = project
        .command()
        .env("ROBOCLAW_LLM_PROVIDER", "openai")
        .args([
            "plan",
            "wave_arm",
            "--provider",
            "auto",
            "--fallback",
            "mock",
            "--json",
        ])
        .output()
        .unwrap();
    let plan = common::json(output);
    assert_eq!(plan["planner_provider"], "mock");
    let workspace = Workspace::load(&project.root, None).unwrap();
    let jobs = Jobs { workspace };
    let job = jobs
        .add(RunRequest::new("wave_arm"), now_millis(), None, true)
        .unwrap();
    assert!(jobs
        .execute(job, &ExecutionControl::default(), None)
        .is_err());
    assert!(jobs.workspace.store.runs().unwrap().is_empty());
}

#[test]
fn auto_mock_planning_uses_the_current_instruction_instead_of_reference_context() {
    let project = Project::new();
    fs::write(
        project.root.join("context.md"),
        "Pick up the red cube and place it in bin_a.",
    )
    .unwrap();
    project.config("context_files: [context.md]\n");
    let output = project
        .command()
        .env("ROBOCLAW_LLM_PROVIDER", "mock")
        .args(["plan", "wave_arm", "--provider", "auto", "--json"])
        .output()
        .unwrap();
    assert_eq!(
        common::json(output)["decision"]["skill"]["name"],
        "wave_arm"
    );
}
