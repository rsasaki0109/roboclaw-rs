mod common;
use common::Project;
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const TOKEN: &str = "gateway-integration-test-token";
struct Gateway {
    child: Option<Child>,
    base: String,
    client: Client,
}
impl Gateway {
    fn start(project: &Project, env: &[(&str, String)]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let mut command = project.command();
        for (key, value) in env {
            command.env(key, value);
        }
        let child = command
            .env("ROBOCLAW_GATEWAY_TOKEN", TOKEN)
            .args(["gateway", "serve", "--bind", &address.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let gateway = Self {
            child: Some(child),
            base: format!("http://{address}"),
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap(),
        };
        let started = Instant::now();
        loop {
            if gateway
                .client
                .get(format!("{}/api/health", gateway.base))
                .bearer_auth(TOKEN)
                .send()
                .is_ok_and(|response| response.status().is_success())
            {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "gateway did not start"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        gateway
    }
    fn get(&self, path: &str) -> Value {
        self.client
            .get(format!("{}/api/{path}", self.base))
            .bearer_auth(TOKEN)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    }
    fn post(&self, path: &str, body: Value) -> Value {
        self.client
            .post(format!("{}/api/{path}", self.base))
            .bearer_auth(TOKEN)
            .json(&body)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap()
    }
    fn wait_job(&self, id: &str, status: &str) -> Value {
        let started = Instant::now();
        loop {
            let job = self.get(&format!("jobs/{id}"));
            if job["status"] == status {
                return job;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "job did not reach {status}: {job}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    #[cfg(unix)]
    fn shutdown(mut self) {
        let pid = self.child.as_ref().unwrap().id().to_string();
        assert!(Command::new("kill")
            .args(["-INT", &pid])
            .status()
            .unwrap()
            .success());
        let started = Instant::now();
        while self.child.as_mut().unwrap().try_wait().unwrap().is_none() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "gateway failed to shut down"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(self.child.take().unwrap().wait().unwrap().success());
    }
}
impl Drop for Gateway {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn authenticated_gateway_plans_runs_and_exposes_sessions_and_traces() {
    let project = Project::new();
    let gateway = Gateway::start(&project, &[]);
    let page = gateway.client.get(&gateway.base).send().unwrap();
    assert!(page.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .contains("frame-ancestors 'none'"));
    assert!(page.text().unwrap().contains("Robot workspace"));
    assert_eq!(
        gateway
            .client
            .get(format!("{}/api/jobs", gateway.base))
            .send()
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        gateway
            .client
            .get(format!("{}/api/jobs", gateway.base))
            .bearer_auth(TOKEN)
            .header("Origin", "http://untrusted.example")
            .send()
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        gateway
            .client
            .get(format!("{}/api/jobs", gateway.base))
            .bearer_auth(TOKEN)
            .header("Host", "untrusted.example")
            .send()
            .unwrap()
            .status(),
        401
    );
    assert_eq!(gateway.get("jobs"), json!([]));
    let plan = gateway.post("plan", json!({"instruction": "wave_arm"}));
    assert_eq!(plan["decision"]["skill"]["name"], "wave_arm");
    assert_eq!(gateway.get("runs"), json!([]));
    let job = gateway.post(
        "jobs",
        json!({"request": {"instruction": "pick and place"}}),
    );
    let result = gateway.wait_job(job["id"].as_str().unwrap(), "completed");
    assert_eq!(result["result"]["backend_state"]["last_pose"], "bin_a");
    assert!(result["request"]["session"]
        .as_str()
        .unwrap()
        .starts_with("job_"));
    let run = result["run_id"].as_str().unwrap();
    assert_eq!(gateway.get(&format!("runs/{run}"))["status"], "completed");
    let events = gateway.get(&format!("runs/{run}/events"));
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "tool_completed"));
    let last = events.as_array().unwrap().last().unwrap()["seq"]
        .as_u64()
        .unwrap();
    assert_eq!(
        gateway.get(&format!("runs/{run}/events?after={last}")),
        json!([])
    );
    assert_eq!(gateway.get("sessions").as_array().unwrap().len(), 1);
    assert_eq!(gateway.get("doctor")["ok"], true);
    let hits = gateway.post(
        "memory/search",
        json!({"session": result["request"]["session"], "query": "pick_and_place"}),
    );
    assert!(!hits.as_array().unwrap().is_empty());
    #[cfg(unix)]
    gateway.shutdown();
}

#[test]
fn malformed_requests_and_scheduled_cancellation_do_not_execute_tasks() {
    let project = Project::new();
    let gateway = Gateway::start(&project, &[]);
    for body in [
        json!({"instruction":"wave_arm", "session":"../escape"}),
        json!({"instruction":"wave_arm", "extra":true}),
        json!({"instruction":""}),
    ] {
        assert_eq!(
            gateway
                .client
                .post(format!("{}/api/plan", gateway.base))
                .bearer_auth(TOKEN)
                .json(&body)
                .send()
                .unwrap()
                .status(),
            400
        );
    }
    assert_eq!(
        gateway
            .client
            .post(format!("{}/api/plan", gateway.base))
            .bearer_auth(TOKEN)
            .body("{}")
            .send()
            .unwrap()
            .status(),
        415
    );
    assert_eq!(
        gateway
            .client
            .post(format!("{}/api/plan", gateway.base))
            .bearer_auth(TOKEN)
            .header("Content-Type", "application/json")
            .body("x".repeat(65_537))
            .send()
            .unwrap()
            .status(),
        413
    );
    let job = gateway.post("jobs", json!({"request":{"instruction":"wave_arm"}, "due_at": roboclaw_rs::storage::now_millis() + 3_600_000}));
    assert_eq!(
        gateway.post(
            &format!("jobs/{}/cancel", job["id"].as_str().unwrap()),
            json!({})
        )["status"],
        "cancelled"
    );
    assert_eq!(gateway.get("runs"), json!([]));
    #[cfg(unix)]
    gateway.shutdown();
}

#[test]
fn gateway_accepts_calendar_jobs_and_rejects_conflicting_or_invalid_schedules() {
    let project = Project::new();
    let gateway = Gateway::start(&project, &[]);
    for schedule in [
        json!({"cron":"0 9 * * *", "every":"1h"}),
        json!({"cron":"0 9 * * *", "due_at":0}),
        json!({"timezone":"Asia/Tokyo"}),
        json!({"cron":"0 9 * * *", "timezone":"Asia/Typo"}),
        json!({"cron":"0 9 31 FEB *"}),
        json!({"cron":"0 0 9 * * *"}),
    ] {
        let mut body = schedule;
        body["request"] = json!({"instruction":"wave_arm"});
        assert_eq!(
            gateway
                .client
                .post(format!("{}/api/jobs", gateway.base))
                .bearer_auth(TOKEN)
                .json(&body)
                .send()
                .unwrap()
                .status(),
            400,
            "{body}"
        );
    }
    assert_eq!(gateway.get("jobs"), json!([]));
    assert_eq!(gateway.get("runs"), json!([]));
    // A yearly slot keeps the test independent of the runner's minute boundary.
    let job = gateway.post(
        "jobs",
        json!({
            "request":{"instruction":"wave_arm", "session":"scheduled"},
            "cron":"0 9 1 JAN *", "timezone":"Asia/Tokyo", "isolated":false
        }),
    );
    assert_eq!(
        job["cron"],
        json!({"expression":"0 9 1 JAN *", "timezone":"Asia/Tokyo"})
    );
    assert_eq!(job["request"]["session"], "scheduled");
    assert_eq!(job["status"], "queued");
    assert_eq!(
        gateway.get(&format!("jobs/{}", job["id"].as_str().unwrap())),
        job
    );
    assert_eq!(gateway.get("runs"), json!([]));
    assert_eq!(
        gateway.post(
            &format!("jobs/{}/cancel", job["id"].as_str().unwrap()),
            json!({})
        )["status"],
        "cancelled"
    );
    let default_zone = gateway.post(
        "jobs",
        json!({"request":{"instruction":"wave_arm"}, "cron":"0 9 1 JAN *"}),
    );
    assert_eq!(default_zone["cron"]["timezone"], "UTC");
    #[cfg(unix)]
    gateway.shutdown();
}

#[test]
fn gateway_delivers_in_background_and_keeps_robot_jobs_independent() {
    use std::sync::mpsc;
    use tiny_http::{Response, Server};
    let project = Project::new();
    let receiver = Server::http("127.0.0.1:0").unwrap();
    project.config(&format!(
        "webhook:\n  url: http://{}/notify\n  timeout: 30s\n",
        receiver.server_addr()
    ));
    let gateway = Gateway::start(&project, &[]);
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut request = receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .unwrap();
        let payload: Value = serde_json::from_reader(request.as_reader()).unwrap();
        started_tx.send(payload).unwrap();
        release_rx.recv_timeout(Duration::from_secs(15)).unwrap();
        request.respond(Response::empty(200)).unwrap();
        if let Some(request) = receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
            request.respond(Response::empty(200)).unwrap();
        }
    });
    let first = gateway.post("jobs", json!({"request":{"instruction":"wave_arm"}}));
    let completed = gateway.wait_job(first["id"].as_str().unwrap(), "completed");
    let payload = started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(payload["run_id"], completed["run_id"]);
    assert_eq!(payload["status"], "completed");
    // Hold the HTTP acknowledgement while another robot job finishes.
    let second = gateway.post("jobs", json!({"request":{"instruction":"pick and place"}}));
    let second_result = gateway.wait_job(second["id"].as_str().unwrap(), "completed");
    assert_eq!(
        second_result["result"]["backend_state"]["last_pose"],
        "bin_a"
    );
    let id = payload["delivery_id"].as_str().unwrap();
    assert_eq!(gateway.get(&format!("webhooks/{id}"))["status"], "sending");
    release_tx.send(()).unwrap();
    let started = Instant::now();
    loop {
        let deliveries = gateway.get("webhooks");
        if deliveries
            .as_array()
            .unwrap()
            .iter()
            .all(|delivery| delivery["status"] == "delivered")
        {
            assert_eq!(deliveries.as_array().unwrap().len(), 2);
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "notifications did not finish: {deliveries}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(gateway.get("runs").as_array().unwrap().len(), 2);
    worker.join().unwrap();
    #[cfg(unix)]
    gateway.shutdown();
}

#[test]
fn gateway_manual_retry_is_authenticated_validates_body_and_preserves_delivery_history() {
    use std::sync::mpsc;
    use tiny_http::{Response, Server};
    let project = Project::new();
    let receiver = Server::http("127.0.0.1:0").unwrap();
    project.config(&format!(
        "webhook:\n  url: http://{}/notify\n  max_attempts: 1\n",
        receiver.server_addr()
    ));
    let (sender, received) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        for status in [401, 200] {
            let mut request = receiver
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap();
            let payload: Value = serde_json::from_reader(request.as_reader()).unwrap();
            sender.send(payload).unwrap();
            request.respond(Response::empty(status)).unwrap();
        }
    });
    let gateway = Gateway::start(&project, &[]);
    let job = gateway.post("jobs", json!({"request":{"instruction":"wave_arm"}}));
    let result = gateway.wait_job(job["id"].as_str().unwrap(), "completed");
    let id = result["run_id"].as_str().unwrap();
    let wait = |status: &str| {
        let started = Instant::now();
        loop {
            let delivery = gateway.get(&format!("webhooks/{id}"));
            if delivery["status"] == status {
                break delivery;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "delivery did not reach {status}: {delivery}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let failed = wait("failed");
    let url = format!("{}/api/webhooks/{id}/retry", gateway.base);
    assert_eq!(
        gateway
            .client
            .post(&url)
            .json(&json!({}))
            .send()
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        gateway
            .client
            .post(&url)
            .bearer_auth(TOKEN)
            .body("{}")
            .send()
            .unwrap()
            .status(),
        415
    );
    for body in [json!({"force":true}), json!(null), json!([])] {
        assert_eq!(
            gateway
                .client
                .post(&url)
                .bearer_auth(TOKEN)
                .json(&body)
                .send()
                .unwrap()
                .status(),
            400
        );
    }
    assert_eq!(gateway.get(&format!("webhooks/{id}")), failed);
    assert_eq!(
        gateway
            .client
            .post(format!("{}/api/webhooks/unknown/retry", gateway.base))
            .bearer_auth(TOKEN)
            .json(&json!({}))
            .send()
            .unwrap()
            .status(),
        404
    );
    let response = gateway
        .client
        .post(&url)
        .bearer_auth(TOKEN)
        .json(&json!({}))
        .send()
        .unwrap();
    assert_eq!(response.status(), 202);
    let queued: Value = response.json().unwrap();
    assert_eq!(queued["status"], "pending");
    assert_eq!(queued["attempts"], 1);
    assert_eq!(queued["payload"], failed["payload"]);
    let delivered = wait("delivered");
    assert_eq!(delivered["attempts"], 2);
    assert_eq!(delivered["retries"][0]["http_status"], 401);
    assert_eq!(
        gateway
            .client
            .post(&url)
            .bearer_auth(TOKEN)
            .json(&json!({}))
            .send()
            .unwrap()
            .status(),
        409
    );
    assert_eq!(
        received.recv_timeout(Duration::from_secs(5)).unwrap(),
        received.recv_timeout(Duration::from_secs(5)).unwrap()
    );
    assert_eq!(gateway.get("runs").as_array().unwrap().len(), 1);
    worker.join().unwrap();
    #[cfg(unix)]
    gateway.shutdown();
}

#[test]
fn running_job_cancellation_survives_a_blocking_model_request() {
    use std::io::{Read, Write};
    use std::sync::mpsc;
    let project = Project::new();
    project.config("planner:\n  provider: local\n");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let host = format!("http://{}", listener.local_addr().unwrap());
    let (connected, connection) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let started = Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if started.elapsed() > Duration::from_secs(10) {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("{error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut bytes = [0; 4096];
        loop {
            let count = stream.read(&mut bytes).unwrap();
            if count == 0 {
                return false;
            }
            request.extend_from_slice(&bytes[..count]);
            if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        connected.send(()).unwrap();
        if resume.recv_timeout(Duration::from_secs(10)).is_err() {
            return false;
        }
        let body = json!({"response": "{\"skill\":\"wave_arm\"}"}).to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        true
    });
    let gateway = Gateway::start(
        &project,
        &[
            ("ROBOCLAW_OLLAMA_HOST", host),
            ("ROBOCLAW_OLLAMA_MODEL", "test-model".into()),
        ],
    );
    let job = gateway.post("jobs", json!({"request":{"instruction":"wave_arm"}}));
    connection.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        gateway.post(
            &format!("jobs/{}/cancel", job["id"].as_str().unwrap()),
            json!({})
        )["cancel_requested"],
        true
    );
    std::thread::sleep(Duration::from_millis(100));
    release.send(()).unwrap();
    assert!(server.join().unwrap());
    let job = gateway.wait_job(job["id"].as_str().unwrap(), "cancelled");
    assert_eq!(job["result"]["status"], "cancelled");
    let events = gateway.get(&format!("runs/{}/events", job["run_id"].as_str().unwrap()));
    assert!(!events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "tool_invoked"));
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "execution_stopped"));
    #[cfg(unix)]
    gateway.shutdown();
}

#[test]
fn local_model_context_contains_only_the_selected_sessions_memory() {
    use std::io::{Read, Write};
    let project = Project::new();
    project.json(&[
        "memory",
        "remember",
        "Wave calibration marker alpha42",
        "--session",
        "alpha",
    ]);
    project.json(&[
        "memory",
        "remember",
        "Wave calibration marker beta73",
        "--session",
        "beta",
    ]);
    std::fs::write(
        project.root.join("context.md"),
        "Gripper reference marker config81",
    )
    .unwrap();
    project.config("planner:\n  provider: local\ncontext_files: [context.md]\n");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let host = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let started = Instant::now();
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(started.elapsed() < Duration::from_secs(10));
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("{error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        let payload = loop {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(end) = bytes.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    break serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + length])
                        .unwrap();
                }
            }
        };
        let body = json!({"response": "{\"skill\":\"wave_arm\"}"}).to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        payload
    });
    let gateway = Gateway::start(
        &project,
        &[
            ("ROBOCLAW_OLLAMA_HOST", host),
            ("ROBOCLAW_OLLAMA_MODEL", "test-model".into()),
        ],
    );
    let result = gateway.post(
        "plan",
        json!({"instruction":"Wave the robot arm.", "session":"alpha"}),
    );
    assert_eq!(result["decision"]["skill"]["name"], "wave_arm");
    let payload = server.join().unwrap();
    let prompt = payload["prompt"].as_str().unwrap();
    assert!(prompt.contains("alpha42"));
    assert!(prompt.contains("config81"));
    assert!(!prompt.contains("beta73"));
    assert_eq!(gateway.get("runs"), json!([]));
    #[cfg(unix)]
    gateway.shutdown();
}
