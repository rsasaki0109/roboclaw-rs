mod common;
use common::Project;
use roboclaw_rs::runtime::Workspace;
use roboclaw_rs::storage::{read_json, write_json, Lease};
use roboclaw_rs::tools::ExecutionControl;
use roboclaw_rs::webhooks::{Delivery, Webhooks};
use serde_json::{json, Value};
use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tiny_http::{Header, Response, Server, StatusCode};

fn configure(project: &Project, server: &Server, extra: &str) {
    project.config(&format!(
        "webhook:\n  url: http://{}/notify?receiver=local\n  retry_delay: 1s\n{extra}",
        server.server_addr()
    ));
}
fn dispatcher(project: &Project) -> Webhooks {
    Webhooks {
        workspace: Workspace::load(&project.root, None).unwrap(),
    }
}
fn due(project: &Project, mut delivery: Delivery) {
    delivery.next_attempt_at = 0;
    write_json(
        &project
            .root
            .join("target/roboclaw/webhooks")
            .join(format!("{}.json", delivery.id)),
        &delivery,
    )
    .unwrap();
}
fn receiver(
    server: Server,
    statuses: Vec<u16>,
) -> (
    thread::JoinHandle<()>,
    mpsc::Receiver<(Value, String, Option<String>)>,
) {
    let (sender, received) = mpsc::channel();
    let worker = thread::spawn(move || {
        for status in statuses {
            let mut request = server
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .expect("webhook did not arrive");
            let key = request
                .headers()
                .iter()
                .find(|header| header.field.equiv("Idempotency-Key"))
                .unwrap()
                .value
                .to_string();
            let auth = request
                .headers()
                .iter()
                .find(|header| header.field.equiv("Authorization"))
                .map(|header| header.value.to_string());
            let payload = serde_json::from_reader(request.as_reader()).unwrap();
            sender.send((payload, key, auth)).unwrap();
            request
                .respond(Response::empty(StatusCode(status)))
                .unwrap();
        }
    });
    (worker, received)
}

#[test]
fn successful_cli_delivery_is_minimal_authenticated_persistent_and_not_resent() {
    let project = Project::new();
    let historical = project.json(&["run", "wave_arm"]);
    let server = Server::http("127.0.0.1:0").unwrap();
    configure(
        &project,
        &server,
        "  token_env: ROBOCLAW_TEST_WEBHOOK_TOKEN\n",
    );
    let run = project.json(&["run", "wave_arm", "--session", "lab"]);
    let webhook = dispatcher(&project);
    let pending = webhook.list().unwrap();
    assert_eq!(pending.len(), 1);
    assert_ne!(pending[0].run_id, historical["run_id"].as_str().unwrap());
    assert_eq!(pending[0].run_id, run["run_id"].as_str().unwrap());
    assert!(!project.root.join("target/roboclaw/webhooks").exists());
    let (worker, received) = receiver(server, vec![204]);
    let output = project
        .command()
        .env("ROBOCLAW_TEST_WEBHOOK_TOKEN", "local-secret-for-test")
        .args(["webhooks", "dispatch", "--json"])
        .output()
        .unwrap();
    let delivered = common::json(output);
    assert_eq!(delivered[0]["status"], "delivered");
    let (payload, key, authorization) = received.recv_timeout(Duration::from_secs(5)).unwrap();
    worker.join().unwrap();
    assert_eq!(key, run["run_id"].as_str().unwrap());
    assert_eq!(
        authorization.as_deref(),
        Some("Bearer local-secret-for-test")
    );
    assert_eq!(payload["delivery_id"], run["run_id"]);
    assert_eq!(payload["event"], "run.finished");
    assert_eq!(payload["status"], "completed");
    assert_eq!(payload["session"], "lab");
    assert!(payload["finished_at"].is_number());
    for private in ["instruction", "error", "memory", "reports"] {
        assert!(payload.get(private).is_none());
    }
    let saved = fs::read_to_string(
        project
            .root
            .join("target/roboclaw/webhooks")
            .join(format!("{key}.json")),
    )
    .unwrap();
    assert!(!saved.contains("local-secret-for-test"));
    assert!(!saved.contains("receiver=local"));
    assert_eq!(project.json(&["webhooks", "show", &key])["attempts"], 1);
    assert_eq!(
        common::json(
            project
                .command()
                .env("ROBOCLAW_TEST_WEBHOOK_TOKEN", "local-secret-for-test")
                .args(["webhooks", "dispatch", "--json"])
                .output()
                .unwrap()
        ),
        json!([])
    );
    assert_eq!(project.json(&["runs", "list"]).as_array().unwrap().len(), 2);
}

#[test]
fn retryable_http_failure_preserves_payload_and_never_reexecutes_run() {
    let project = Project::new();
    let server = Server::http("127.0.0.1:0").unwrap();
    configure(&project, &server, "");
    project.json(&["run", "wave_arm"]);
    let (worker, received) = receiver(server, vec![503, 200]);
    let webhook = dispatcher(&project);
    let output = project
        .command()
        .args(["webhooks", "dispatch", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let first: Vec<Delivery> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(first[0].status, "pending");
    assert_eq!(first[0].http_status, Some(503));
    assert_eq!(first[0].attempts, 1);
    assert!(first[0].next_attempt_at > first[0].last_attempt_at.unwrap());
    assert!(webhook
        .dispatch(100, &ExecutionControl::default())
        .unwrap()
        .is_empty());
    due(&project, first[0].clone());
    let second = dispatcher(&project)
        .dispatch(100, &ExecutionControl::default())
        .unwrap();
    assert_eq!(second[0].status, "delivered");
    assert_eq!(second[0].attempts, 2);
    let first_request = received.recv_timeout(Duration::from_secs(5)).unwrap();
    let second_request = received.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first_request, second_request);
    worker.join().unwrap();
    assert_eq!(webhook.workspace.store.runs().unwrap().len(), 1);
    assert_eq!(webhook.workspace.store.sessions().unwrap()[0].run_count, 1);
}

#[test]
fn unacknowledged_claim_retries_same_id_and_exhausted_claim_is_terminal() {
    let project = Project::new();
    let server = Server::http("127.0.0.1:0").unwrap();
    configure(&project, &server, "  max_attempts: 2\n");
    project.json(&["run", "wave_arm"]);
    let mut delivery = dispatcher(&project).list().unwrap().remove(0);
    delivery.status = "sending".into();
    delivery.attempts = 1;
    due(&project, delivery.clone());
    let (worker, received) = receiver(server, vec![503]);
    let result = dispatcher(&project)
        .dispatch(100, &ExecutionControl::default())
        .unwrap();
    assert_eq!(result[0].status, "failed");
    assert_eq!(result[0].attempts, 2);
    assert_eq!(
        received.recv_timeout(Duration::from_secs(5)).unwrap().1,
        delivery.id
    );
    worker.join().unwrap();
    assert!(dispatcher(&project)
        .dispatch(100, &ExecutionControl::default())
        .unwrap()
        .is_empty());
    delivery.attempts = 2;
    due(&project, delivery);
    dispatcher(&project)
        .dispatch(100, &ExecutionControl::default())
        .unwrap();
    assert_eq!(dispatcher(&project).list().unwrap()[0].status, "failed");
    assert_eq!(project.json(&["runs", "list"]).as_array().unwrap().len(), 1);
}

#[test]
fn permanent_errors_and_redirects_stop_without_following_location() {
    for status in [400, 401, 302] {
        let project = Project::new();
        let server = Server::http("127.0.0.1:0").unwrap();
        let target = Server::http("127.0.0.1:0").unwrap();
        configure(&project, &server, "");
        project.json(&["run", "wave_arm"]);
        let location = format!("http://{}/redirect", target.server_addr());
        let worker = thread::spawn(move || {
            let request = server
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
            request
                .respond(
                    Response::from_string("private receiver error body")
                        .with_status_code(status)
                        .with_header(Header::from_bytes("Location", location).unwrap()),
                )
                .unwrap();
        });
        let result = dispatcher(&project)
            .dispatch(100, &ExecutionControl::default())
            .unwrap();
        assert_eq!(result[0].status, "failed");
        assert_eq!(result[0].http_status, Some(status));
        assert!(!result[0]
            .error
            .as_deref()
            .unwrap()
            .contains("private receiver"));
        assert!(dispatcher(&project)
            .dispatch(100, &ExecutionControl::default())
            .unwrap()
            .is_empty());
        assert!(target
            .recv_timeout(Duration::from_millis(30))
            .unwrap()
            .is_none());
        worker.join().unwrap();
    }
}

#[test]
fn retry_after_and_timeout_are_bounded_and_errors_do_not_leak_url() {
    let project = Project::new();
    let server = Server::http("127.0.0.1:0").unwrap();
    configure(&project, &server, "  timeout: 100ms\n");
    project.json(&["run", "wave_arm"]);
    let worker = thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        request
            .respond(
                Response::empty(429)
                    .with_header(Header::from_bytes("Retry-After", "999999").unwrap()),
            )
            .unwrap();
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        thread::sleep(Duration::from_millis(200));
        drop(request);
    });
    let first = dispatcher(&project)
        .dispatch(100, &ExecutionControl::default())
        .unwrap();
    assert_eq!(first[0].status, "pending");
    assert!(first[0].next_attempt_at >= first[0].last_attempt_at.unwrap() + 3_600_000);
    due(&project, first[0].clone());
    let second = dispatcher(&project)
        .dispatch(100, &ExecutionControl::default())
        .unwrap();
    assert_eq!(second[0].error.as_deref(), Some("request timed out"));
    assert_eq!(second[0].http_status, None);
    assert!(!serde_json::to_string(&second)
        .unwrap()
        .contains("receiver=local"));
    worker.join().unwrap();
}

#[test]
fn missing_or_invalid_credentials_preserve_pending_delivery_and_doctor_is_readonly() {
    let project = Project::new();
    project.config(
        "webhook:\n  url: https://example.com/notify\n  token_env: ROBOCLAW_MISSING_TEST_TOKEN\n",
    );
    project.json(&["run", "wave_arm"]);
    for token in [None, Some(""), Some("sensitive\ncredential")] {
        let mut command = project.command();
        if let Some(token) = token {
            command.env("ROBOCLAW_MISSING_TEST_TOKEN", token);
        } else {
            command.env_remove("ROBOCLAW_MISSING_TEST_TOKEN");
        }
        let output = command.args(["webhooks", "dispatch"]).output().unwrap();
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("sensitive"));
    }
    let output = project
        .command()
        .env_remove("ROBOCLAW_MISSING_TEST_TOKEN")
        .args(["doctor", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!project.root.join("target/roboclaw/webhooks").exists());
    assert_eq!(dispatcher(&project).list().unwrap()[0].attempts, 0);
}

#[test]
fn config_rejects_unsafe_endpoints_and_invalid_bounds_without_side_effects() {
    for fragment in [
        "url: http://example.com/notify",
        "url: file:///tmp/notify",
        "url: https://user:secret@example.com/notify",
        "url: https://example.com/notify#fragment",
        "url: http://not-localhost.example/notify",
        "url: https://example.com\n  timeout: 31s",
        "url: https://example.com\n  retry_delay: 1ms",
        "url: https://example.com\n  max_attempts: 0",
        "url: https://example.com\n  max_attempts: 21",
        "url: https://example.com\n  token_env: 1INVALID",
        "url: https://example.com\n  token: secret",
    ] {
        let project = Project::new();
        project.config(&format!("webhook:\n  {fragment}\n"));
        let output = project
            .command()
            .args(["config", "check"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{fragment}");
        assert!(!project.root.join("target").exists());
    }
    for url in [
        "http://127.0.0.1:8080/notify",
        "http://[::1]:8080/notify",
        "http://localhost:8080/notify",
        "https://example.com/notify",
    ] {
        let project = Project::new();
        project.config(&format!("webhook:\n  url: {url}\n"));
        project.json(&["config", "check"]);
        assert!(!project.root.join("target").exists());
    }
}

#[test]
fn cancelled_dispatch_and_exclusive_lock_do_not_make_http_attempts() {
    let project = Project::new();
    let server = Server::http("127.0.0.1:0").unwrap();
    configure(&project, &server, "");
    project.json(&["run", "wave_arm"]);
    let webhook = dispatcher(&project);
    let control = ExecutionControl::default();
    control.cancel();
    assert!(webhook.dispatch(100, &control).unwrap().is_empty());
    assert!(!webhook.workspace.store.root.join(".webhooks.lock").exists());
    let _lease = Lease::acquire(&webhook.workspace.store.root.join(".webhooks.lock")).unwrap();
    assert!(webhook.dispatch(100, &ExecutionControl::default()).is_err());
    assert_eq!(webhook.list().unwrap()[0].attempts, 0);
    assert!(server
        .recv_timeout(Duration::from_millis(30))
        .unwrap()
        .is_none());
}

#[test]
fn terminal_failure_is_delivered_but_running_and_legacy_records_are_excluded() {
    let project = Project::new();
    let server = Server::http("127.0.0.1:0").unwrap();
    configure(&project, &server, "");
    let output = project
        .command()
        .args(["run", "wave_arm", "--provider", "openai"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let webhook = dispatcher(&project);
    let delivery = webhook.list().unwrap().remove(0);
    assert_eq!(delivery.payload["status"], "failed");
    assert!(delivery.payload["backend_state"].is_null());
    assert!(delivery.payload.get("error").is_none());
    let path = webhook
        .workspace
        .store
        .run_dir(&delivery.id)
        .unwrap()
        .join("run.json");
    let mut run: Value = read_json(&path).unwrap();
    run["status"] = json!("running");
    write_json(&path, &run).unwrap();
    assert!(webhook.list().unwrap().is_empty());
    run["status"] = json!("failed");
    run.as_object_mut().unwrap().remove("webhook");
    write_json(&path, &run).unwrap();
    assert!(webhook.list().unwrap().is_empty());
    run["webhook"] = json!(true);
    write_json(&path, &run).unwrap();
    let (worker, received) = receiver(server, vec![200]);
    assert_eq!(
        webhook.dispatch(100, &ExecutionControl::default()).unwrap()[0].status,
        "delivered"
    );
    assert_eq!(
        received.recv_timeout(Duration::from_secs(5)).unwrap().0["status"],
        "failed"
    );
    worker.join().unwrap();
}

#[test]
fn manual_retry_keeps_identity_payload_counts_and_failure_history_without_sending() {
    let project = Project::new();
    let server = Server::http("127.0.0.1:0").unwrap();
    configure(&project, &server, "  max_attempts: 1\n");
    project.json(&["run", "wave_arm"]);
    let (worker, received) = receiver(server, vec![401, 200]);
    let webhook = dispatcher(&project);
    let failed = webhook
        .dispatch(100, &ExecutionControl::default())
        .unwrap()
        .remove(0);
    assert_eq!(failed.status, "failed");
    let retried = project.json(&["webhooks", "retry", &failed.id]);
    assert_eq!(retried["status"], "pending");
    assert_eq!(retried["id"], failed.id);
    assert_eq!(retried["payload"], failed.payload);
    assert_eq!(retried["attempts"], 1);
    assert_eq!(retried["attempts_at_retry"], 1);
    assert_eq!(retried["retries"][0]["http_status"], 401);
    assert_eq!(retried["retries"][0]["error"], "receiver returned HTTP 401");
    assert_eq!(
        retried["retries"][0]["last_attempt_at"],
        failed.last_attempt_at.unwrap()
    );
    assert!(retried["retries"][0]["requested_at"].is_number());
    assert!(retried["error"].is_null());
    let first = received.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        received.try_recv().is_err(),
        "retry must queue without HTTP"
    );
    assert!(!project
        .command()
        .args(["webhooks", "retry", &failed.id])
        .output()
        .unwrap()
        .status
        .success());
    let delivered = dispatcher(&project)
        .dispatch(100, &ExecutionControl::default())
        .unwrap()
        .remove(0);
    assert_eq!(delivered.status, "delivered");
    assert_eq!(delivered.attempts, 2);
    assert_eq!(delivered.retries.len(), 1);
    assert_eq!(
        first,
        received.recv_timeout(Duration::from_secs(5)).unwrap()
    );
    worker.join().unwrap();
    assert!(webhook.retry(&failed.id).is_err());
    assert_eq!(webhook.workspace.store.runs().unwrap().len(), 1);
    assert_eq!(webhook.workspace.store.sessions().unwrap()[0].run_count, 1);
}

#[test]
fn manual_retry_restores_bounded_budget_backoff_and_supports_repeated_retries() {
    let project = Project::new();
    let server = Server::http("127.0.0.1:0").unwrap();
    configure(&project, &server, "  max_attempts: 2\n");
    project.json(&["run", "wave_arm"]);
    let mut failed = dispatcher(&project).list().unwrap().remove(0);
    // A legacy record stopped after exhausting its original two attempts.
    failed.status = "failed".into();
    failed.attempts = 2;
    failed.error = Some("receiver returned HTTP 503".into());
    failed.http_status = Some(503);
    let mut legacy = serde_json::to_value(&failed).unwrap();
    legacy.as_object_mut().unwrap().remove("attempts_at_retry");
    legacy.as_object_mut().unwrap().remove("retries");
    write_json(
        &project
            .root
            .join("target/roboclaw/webhooks")
            .join(format!("{}.json", failed.id)),
        &legacy,
    )
    .unwrap();
    let (worker, received) = receiver(server, vec![503, 503, 200]);
    let webhook = dispatcher(&project);
    webhook.retry(&failed.id).unwrap();
    let first = webhook
        .dispatch(100, &ExecutionControl::default())
        .unwrap()
        .remove(0);
    assert_eq!(first.status, "pending");
    assert_eq!(first.attempts, 3);
    let delay = first.next_attempt_at - roboclaw_rs::storage::now_millis();
    assert!(
        delay <= 1000 && delay > 500,
        "backoff must restart at 1s: {delay}"
    );
    due(&project, first);
    let exhausted = webhook
        .dispatch(100, &ExecutionControl::default())
        .unwrap()
        .remove(0);
    assert_eq!(exhausted.status, "failed");
    assert_eq!(exhausted.attempts, 4);
    assert!(webhook
        .dispatch(100, &ExecutionControl::default())
        .unwrap()
        .is_empty());
    let retried = webhook.retry(&failed.id).unwrap();
    assert_eq!(retried.retries.len(), 2);
    assert_eq!(retried.attempts_at_retry, 4);
    assert_eq!(retried.retries[0].attempts, 2);
    assert_eq!(retried.retries[1].attempts, 4);
    let delivered = webhook
        .dispatch(100, &ExecutionControl::default())
        .unwrap()
        .remove(0);
    assert_eq!(delivered.status, "delivered");
    assert_eq!(delivered.attempts, 5);
    assert_eq!(delivered.retries.len(), 2);
    for _ in 0..3 {
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap().1,
            failed.id
        );
    }
    worker.join().unwrap();
}

#[test]
fn manual_retry_rejects_disabled_credentials_busy_sender_and_other_states_without_mutation() {
    let project = Project::new();
    let server = Server::http("127.0.0.1:0").unwrap();
    configure(&project, &server, "");
    project.json(&["run", "wave_arm"]);
    let webhook = dispatcher(&project);
    let mut delivery = webhook.list().unwrap().remove(0);
    for status in ["pending", "sending", "delivered"] {
        delivery.status = status.into();
        due(&project, delivery.clone());
        assert!(webhook
            .retry(&delivery.id)
            .unwrap_err()
            .downcast_ref::<roboclaw_rs::webhooks::RetryConflict>()
            .is_some());
        assert!(webhook.get(&delivery.id).unwrap().retries.is_empty());
    }
    delivery.status = "failed".into();
    due(&project, delivery.clone());
    let path = project
        .root
        .join("target/roboclaw/webhooks")
        .join(format!("{}.json", delivery.id));
    let before = fs::read(&path).unwrap();
    {
        let _lease = Lease::acquire(&webhook.workspace.store.root.join(".webhooks.lock")).unwrap();
        assert!(webhook.retry(&delivery.id).is_err());
    }
    project.config(
        "webhook:\n  url: https://example.com/notify\n  token_env: ROBOCLAW_MISSING_TEST_TOKEN\n",
    );
    let output = project
        .command()
        .env_remove("ROBOCLAW_MISSING_TEST_TOKEN")
        .args(["webhooks", "retry", &delivery.id])
        .output()
        .unwrap();
    assert!(!output.status.success());
    project.config("webhook: null\n");
    assert!(dispatcher(&project).retry(&delivery.id).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(server
        .recv_timeout(Duration::from_millis(30))
        .unwrap()
        .is_none());
}
