mod common;
use common::Project;
use roboclaw_rs::challenges::{ChallengeRequest, Challenges, Scenario, Strategy};
use roboclaw_rs::jobs::Jobs;
use roboclaw_rs::runtime::Workspace;
use roboclaw_rs::storage::write_json;
use roboclaw_rs::tools::ExecutionControl;
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;

fn service(project: &Project) -> Challenges {
    Challenges {
        workspace: Workspace::load(&project.root, None).unwrap(),
    }
}
fn request(scenarios: Vec<Scenario>) -> ChallengeRequest {
    ChallengeRequest {
        scenarios,
        repeats: 1,
        timeout: "2s".into(),
        ..Default::default()
    }
}
fn run(service: &Challenges, request: ChallengeRequest) -> roboclaw_rs::challenges::Challenge {
    let _lease = Jobs {
        workspace: service.workspace.clone(),
    }
    .runner_lease()
    .unwrap();
    let queued = service.create(request).unwrap();
    service
        .execute(
            service.claim(&queued.id).unwrap(),
            &ExecutionControl::default(),
        )
        .unwrap()
}
#[test]
fn recovery_comparison_scores_actual_placements_and_records_isolated_traces() {
    let project = Project::new();
    project.config("webhook:\n  url: http://127.0.0.1:1/notify\n");
    let service = service(&project);
    let result = run(
        &service,
        ChallengeRequest {
            repeats: 1,
            ..Default::default()
        },
    );
    assert_eq!(result.status, "completed");
    assert_eq!(result.trials.len(), 8);
    assert!(result.execution_config.webhook.is_none());
    assert!(!result.execution_config.memory_recall);
    assert!(roboclaw_rs::webhooks::Webhooks {
        workspace: service.workspace.clone()
    }
    .list()
    .unwrap()
    .is_empty());
    let ranks = result.rankings();
    assert_eq!(ranks[0].strategy, Strategy::Recovery);
    assert_eq!((ranks[0].passed, ranks[0].planned), (4, 4));
    assert_eq!(ranks[0].replans, 2);
    assert_eq!(ranks[1].strategy, Strategy::RetryOnly);
    assert_eq!((ranks[1].passed, ranks[1].planned), (2, 4));
    assert_eq!(ranks[1].replans, 0);
    assert_eq!(ranks[0].retries, 3);
    assert_eq!(ranks[1].retries, 3);
    assert_eq!(
        result
            .trials
            .iter()
            .map(|t| &t.session)
            .collect::<BTreeSet<_>>()
            .len(),
        8
    );
    assert!(!service.workspace.memory_path("main").unwrap().exists());
    for trial in &result.trials {
        let run = service.workspace.store.run(&trial.run_id).unwrap();
        assert!(!run.webhook);
        assert_eq!(run.session, trial.session);
        assert_eq!(run.status, trial.status);
        assert!(!service
            .workspace
            .store
            .events(&trial.run_id, 0, 1000)
            .unwrap()
            .is_empty());
        assert_eq!(
            trial.faults_injected,
            match trial.scenario {
                Scenario::Baseline => 0,
                Scenario::SensorGlitch => 1,
                _ => 2,
            }
        );
        assert_eq!(
            trial.recovery_ms.is_some(),
            trial.passed && trial.scenario != Scenario::Baseline
        );
    }
}
#[test]
fn faults_reset_between_repetitions_and_persistent_failure_never_passes() {
    let project = Project::new();
    let service = service(&project);
    let result = run(
        &service,
        ChallengeRequest {
            scenarios: vec![Scenario::SensorGlitch, Scenario::PersistentStall],
            repeats: 2,
            ..Default::default()
        },
    );
    for trial in &result.trials {
        if trial.scenario == Scenario::SensorGlitch {
            assert!(trial.passed);
            assert_eq!(trial.faults_injected, 1);
            assert_eq!(trial.retries, 1);
        } else {
            assert!(!trial.passed);
            assert_eq!(trial.status, "failed");
            assert!(trial.faults_injected >= 2);
        }
    }
}
#[test]
fn deadline_scenario_passes_only_after_an_injected_stall_with_no_motion() {
    let project = Project::new();
    let service = service(&project);
    let result = run(
        &service,
        ChallengeRequest {
            timeout: "250ms".into(),
            ..request(vec![Scenario::SensorTimeout])
        },
    );
    for trial in &result.trials {
        assert!(trial.passed);
        assert_eq!(trial.status, "timed_out");
        assert_eq!(trial.faults_injected, 1);
        assert_eq!(trial.tool_attempts, 1);
        let run = service.workspace.store.run(&trial.run_id).unwrap();
        assert_eq!(run.result.unwrap().backend_state.last_pose, "home");
    }
}
#[test]
fn queued_comparison_freezes_skills_prompt_policy_and_context() {
    let project = Project::new();
    fs::write(project.root.join("reference.md"), "Initial context").unwrap();
    project.config("context_files: [reference.md]\n");
    let service = service(&project);
    let queued = service.create(request(vec![Scenario::Baseline])).unwrap();
    fs::remove_dir_all(project.root.join("skills")).unwrap();
    fs::remove_dir_all(project.root.join("prompts")).unwrap();
    fs::write(project.root.join("reference.md"), "Modified context").unwrap();
    project.config("tools:\n  deny: [simulator]\n");
    let _lease = Jobs {
        workspace: service.workspace.clone(),
    }
    .runner_lease()
    .unwrap();
    let result = service
        .execute(
            service.claim(&queued.id).unwrap(),
            &ExecutionControl::default(),
        )
        .unwrap();
    assert!(result.trials.iter().all(|trial| trial.passed));
    let context = fs::read_to_string(
        service
            .workspace
            .store
            .root
            .join("challenges")
            .join(&result.id)
            .join("assets/context.md"),
    )
    .unwrap();
    assert!(context.contains("Initial context"));
    assert!(!context.contains("Modified context"));
}
#[test]
fn forbidden_tools_and_a_skill_that_only_moves_cannot_win() {
    let project = Project::new();
    project.config("tools:\n  deny: [simulator]\n");
    let denied = run(&service(&project), request(vec![Scenario::Baseline]));
    assert!(denied
        .trials
        .iter()
        .all(|trial| !trial.passed && trial.tool_attempts == 0));
    project.config("{}");
    fs::write(project.root.join("skills/pick_and_place.yaml"), "name: pick_and_place\ndescription: incomplete mission\nsteps:\n  - name: just_move\n    tool: simulator\n    input: {action: move_to, pose: bin_a}\n    expect: {accepted: true}\n").unwrap();
    let incomplete = run(&service(&project), request(vec![Scenario::Baseline]));
    assert!(incomplete
        .trials
        .iter()
        .all(|trial| trial.status == "completed" && !trial.passed));
}
#[test]
fn cancelling_queued_trials_and_recovering_crashes_never_replays_them() {
    let project = Project::new();
    let service = service(&project);
    let queued = service.create(request(vec![Scenario::Baseline])).unwrap();
    assert!(service
        .execute(queued.clone(), &ExecutionControl::default())
        .is_err());
    let cancelled = service.cancel(&queued.id).unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert_eq!(cancelled.rankings()[0].pass_rate, 0.0);
    assert!(cancelled
        .trials
        .iter()
        .all(|trial| trial.status == "cancelled" && trial.elapsed_ms.is_none()));
    let _lease = Jobs {
        workspace: service.workspace.clone(),
    }
    .runner_lease()
    .unwrap();
    assert!(service.claim_next().unwrap().is_none());
    let queued = service.create(request(vec![Scenario::Baseline])).unwrap();
    let mut claimed = service.claim(&queued.id).unwrap();
    claimed.trials[0].status = "running".into();
    write_json(
        &service
            .workspace
            .store
            .root
            .join("challenges")
            .join(&queued.id)
            .join("challenge.json"),
        &claimed,
    )
    .unwrap();
    service.recover_interrupted().unwrap();
    let recovered = service.get(&queued.id).unwrap();
    assert_eq!(recovered.status, "interrupted");
    assert_eq!(recovered.trials[0].status, "interrupted");
    assert!(service
        .execute(claimed, &ExecutionControl::default())
        .is_err());
    assert!(service.claim_next().unwrap().is_none());
    assert!(service.workspace.store.runs().unwrap().is_empty());
}
#[test]
fn invalid_comparisons_create_no_durable_trials() {
    let project = Project::new();
    let service = service(&project);
    for value in [
        json!({"repeats":0}),
        json!({"repeats":11}),
        json!({"scenarios":[]}),
        json!({"scenarios":["baseline","baseline"]}),
        json!({"strategies":[]}),
        json!({"providers":["auto"]}),
        json!({"providers":["bogus"]}),
        json!({"providers":["mock","mock"]}),
        json!({"timeout":"0s"}),
        json!({"timeout":"61s"}),
        json!({"providers":["mock","local","openai","claude"],"repeats":10}),
    ] {
        let request: ChallengeRequest = serde_json::from_value(value).unwrap();
        assert!(service.create(request).is_err());
    }
    assert!(!service.workspace.store.root.join("challenges").exists());
    assert!(serde_json::from_value::<ChallengeRequest>(json!({"unexpected":true})).is_err());
    assert!(serde_json::from_value::<ChallengeRequest>(json!({"scenarios":["bogus"]})).is_err());
}
#[test]
fn cli_comparisons_ignore_global_faults_real_ros_and_configured_planner_fallbacks() {
    let project = Project::new();
    project.config("planner:\n  provider: claude\n  fallbacks: [mock]\n");
    let result = common::json(
        project
            .command()
            .env("ROBOCLAW_ROS2_BRIDGE", "rclrs")
            .env("ROBOCLAW_SENSOR_FAIL_COUNT", "100")
            .env("ROBOCLAW_MOTOR_FAIL_COUNT", "100")
            .args([
                "challenges",
                "run",
                "--scenario",
                "baseline",
                "--repeats",
                "1",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    assert!(result["challenge"]["trials"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["passed"] == true));
    let failed = project.json(&[
        "challenges",
        "run",
        "--scenario",
        "baseline",
        "--provider",
        "claude,mock",
        "--repeats",
        "1",
    ]);
    assert!(failed["challenge"]["trials"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["provider"] == "claude")
        .all(|t| t["status"] == "failed" && t["passed"] == false));
    assert_eq!(failed["rankings"][0]["provider"], "mock");
    let queued = project.json(&[
        "challenges",
        "submit",
        "--scenario",
        "baseline",
        "--repeats",
        "1",
    ]);
    assert_eq!(queued["status"], "queued");
    let id = queued["id"].as_str().unwrap();
    assert_eq!(
        project.json(&["challenges", "show", id])["challenge"]["id"],
        id
    );
    assert_eq!(
        project.json(&["challenges", "cancel", id])["status"],
        "cancelled"
    );
    assert_eq!(
        project
            .json(&["challenges", "list"])
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        project
            .json(&["challenges", "scenarios"])
            .as_array()
            .unwrap()
            .len(),
        6
    );
}
