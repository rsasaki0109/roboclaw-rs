use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEST: AtomicUsize = AtomicUsize::new(0);

struct Project {
    root: PathBuf,
}

impl Project {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "roboclaw-cli-{}-{}-{}",
            std::process::id(),
            NEXT_TEST.fetch_add(1, Ordering::Relaxed),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(root.join("skills")).unwrap();
        fs::create_dir_all(root.join("prompts")).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"));
        for entry in fs::read_dir(source.join("skills")).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), root.join("skills").join(entry.file_name())).unwrap();
        }
        fs::copy(
            source.join("prompts/planner_prompt.txt"),
            root.join("prompts/planner_prompt.txt"),
        )
        .unwrap();
        Self { root }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_roboclaw"));
        command
            .current_dir(&self.root)
            .env("ROBOCLAW_ROS2_BRIDGE", "mock")
            .env("ROBOCLAW_SENSOR_FAIL_COUNT", "0")
            .env("ROBOCLAW_SENSOR_FAIL_TARGET", "red_cube")
            .env("ROBOCLAW_MOTOR_FAIL_COUNT", "0")
            .env("ROBOCLAW_MOTOR_FAIL_ACTION", "grasp");
        command
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn success_json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout must contain only JSON")
}

#[test]
fn skills_list_needs_only_the_skill_catalog() {
    let project = Project::new();
    fs::remove_dir_all(project.root.join("prompts")).unwrap();
    let working_dir = project.root.join("elsewhere");
    fs::create_dir(&working_dir).unwrap();
    let skills = success_json(
        project
            .command()
            .current_dir(&working_dir)
            .arg("--project-dir")
            .arg(&project.root)
            .args(["skills", "list", "--json"])
            .output()
            .unwrap(),
    );
    let names = skills
        .as_array()
        .unwrap()
        .iter()
        .map(|skill| skill["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "pick_and_place",
            "recover_grasp",
            "recover_observation",
            "wave_arm"
        ]
    );
    assert!(!project.root.join("target").exists());
}

#[test]
fn plan_does_not_initialize_runtime_or_create_memory() {
    let project = Project::new();
    let plan = success_json(
        project
            .command()
            .env("ROBOCLAW_LLM_PROVIDER", "invalid")
            .env("ROBOCLAW_ROS2_BRIDGE", "invalid")
            .env("ROBOCLAW_MOTOR_FAIL_COUNT", "100")
            .args([
                "plan",
                "Pick up the red cube and place it in bin_a.",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(plan["planner_provider"], "mock");
    assert_eq!(plan["decision"]["skill"]["name"], "pick_and_place");
    assert_eq!(
        plan["decision"]["skill"]["steps"].as_array().unwrap().len(),
        4
    );
    assert_eq!(
        plan["decision"]["skill"]["steps"][0]["input"]["target"],
        "red_cube"
    );
    assert!(!project.root.join("target").exists());
}

#[test]
fn auto_provider_honors_environment_selection() {
    let project = Project::new();
    let plan = success_json(
        project
            .command()
            .env("ROBOCLAW_LLM_PROVIDER", "mock")
            .args([
                "plan",
                "Wave the robot arm.",
                "--provider",
                "auto",
                "--json",
            ])
            .output()
            .unwrap(),
    );
    assert_eq!(plan["planner_provider"], "mock");
    assert_eq!(plan["decision"]["skill"]["name"], "wave_arm");

    let output = project
        .command()
        .env("ROBOCLAW_LLM_PROVIDER", "invalid")
        .args([
            "plan",
            "Wave the robot arm.",
            "--provider",
            "auto",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!project.root.join("target").exists());
}

#[test]
fn run_completes_pick_and_place_and_persists_memory() {
    let project = Project::new();
    let memory = project.root.join("custom-memory");
    let result = success_json(
        project
            .command()
            .args([
                "run",
                "Pick up the red cube and place it in bin_a.",
                "--provider",
                "mock",
                "--json",
                "--memory-dir",
            ])
            .arg(&memory)
            .output()
            .unwrap(),
    );
    assert_eq!(result["status"], "completed");
    assert_eq!(result["report"]["status"], "completed");
    assert_eq!(result["report"]["completed"], true);
    assert_eq!(result["report"]["skill"]["name"], "pick_and_place");
    assert_eq!(result["report"]["steps"].as_array().unwrap().len(), 4);
    assert_eq!(result["backend_state"]["last_pose"], "bin_a");
    assert_eq!(result["backend_state"]["held_object"], Value::Null);
    assert_eq!(result["reports"].as_array().unwrap().len(), 1);
    let events: Value =
        serde_json::from_str(&fs::read_to_string(memory.join("short_term.json")).unwrap()).unwrap();
    assert!(!events.as_array().unwrap().is_empty());
    assert!(fs::read_to_string(memory.join("long_term.md"))
        .unwrap()
        .contains("Executed pick_and_place"));
    assert!(!project.root.join("target").exists());
}

#[test]
fn incomplete_execution_returns_failure_with_a_json_report() {
    let project = Project::new();
    let output = project
        .command()
        .env("ROBOCLAW_SENSOR_FAIL_COUNT", "100")
        .args([
            "run",
            "Pick up the red cube and place it in bin_a.",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "failed");
    assert_eq!(result["report"]["completed"], false);
    assert_eq!(result["report"]["failed_step"], "rescan_object");
    assert_eq!(result["replans"], 1);
    assert_eq!(result["backend_state"]["held_object"], Value::Null);
    assert!(project
        .root
        .join("target/cli-memory/short_term.json")
        .exists());
}

#[test]
fn invalid_options_fail_before_runtime_initialization() {
    let project = Project::new();
    for args in [
        vec!["run", "pick and place", "--provider", "unknown"],
        vec!["run", " \t"],
        vec!["skills", "list", "--provider", "mock"],
    ] {
        let output = project.command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    assert!(!project.root.join("target").exists());
}

#[test]
fn invalid_skill_input_returns_runtime_error() {
    let project = Project::new();
    fs::write(
        project.root.join("skills/pick_and_place.yaml"),
        "name: pick_and_place\ndescription: pick and place\nsteps:\n  - name: grasp\n    tool: motor_control\n    input:\n      action: grasp\n",
    ).unwrap();
    let output = project
        .command()
        .args(["run", "pick and place", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("field 'target' must be a non-empty string"));
    let events: Value = serde_json::from_str(
        &fs::read_to_string(project.root.join("target/cli-memory/short_term.json")).unwrap(),
    )
    .unwrap();
    assert!(!events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == json!("tool_completed")));
}

#[test]
fn timeout_before_planning_returns_structured_outcome_and_history() {
    let project = Project::new();
    let output = project
        .command()
        .args(["run", "pick and place", "--timeout", "1ns", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(124));
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "timed_out");
    assert_eq!(result["report"], Value::Null);
    assert_eq!(result["reports"], json!([]));
    assert_eq!(result["backend_state"]["held_object"], Value::Null);
    let events: Value = serde_json::from_str(
        &fs::read_to_string(project.root.join("target/cli-memory/short_term.json")).unwrap(),
    )
    .unwrap();
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "execution_stopped"
            && event["payload"]["status"] == "timed_out"));
    assert!(!events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "tool_invoked"));
}

#[test]
fn timeout_argument_requires_a_positive_valid_duration() {
    let project = Project::new();
    for value in ["0s", "-1s", "forever", "999999999999999999999999999s"] {
        let output = project
            .command()
            .args(["run", "wave_arm", "--timeout", value])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    assert!(!project.root.join("target").exists());
    let result = success_json(
        project
            .command()
            .args(["run", "wave_arm", "--timeout", "2m", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(result["status"], "completed");
}

#[cfg(unix)]
#[test]
fn ctrl_c_during_blocking_planning_returns_cancelled_json_without_robot_commands() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::process::{Child, Stdio};
    use std::time::{Duration, Instant};

    // Always reap the subprocess, including when an assertion fails.
    struct Guard(Option<Child>);
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(child) = &mut self.0 {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
    let project = Project::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let host = format!("http://{}", listener.local_addr().unwrap());
    let child = project
        .command()
        .env("ROBOCLAW_OLLAMA_HOST", &host)
        .env("ROBOCLAW_OLLAMA_MODEL", "test-model")
        .args(["run", "pick and place", "--provider", "local", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut guard = Guard(Some(child));
    let started = Instant::now();
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "planner did not connect"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept failed: {error}"),
        }
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = Vec::new();
    let mut buffer = [0; 4096];
    loop {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0);
        request.extend_from_slice(&buffer[..read]);
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
        assert!(request.len() < 1024 * 1024);
    }
    assert!(String::from_utf8_lossy(&request).starts_with("POST /api/generate"));
    let pid = guard.0.as_ref().unwrap().id().to_string();
    assert!(Command::new("kill")
        .args(["-INT", &pid])
        .status()
        .unwrap()
        .success());
    // Allow the signal handler to record the token before releasing synchronous HTTP.
    std::thread::sleep(Duration::from_millis(100));
    let body =
        json!({"response": "{\"skill\":\"pick_and_place\",\"reason\":\"test\"}"}).to_string();
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    drop(stream);
    while guard.0.as_mut().unwrap().try_wait().unwrap().is_none() {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "CLI did not exit after cancellation"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = guard.0.take().unwrap().wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(130),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "cancelled");
    assert_eq!(result["report"], Value::Null);
    assert_eq!(result["replans"], 0);
    assert_eq!(result["backend_state"]["held_object"], Value::Null);
    let events: Value = serde_json::from_str(
        &fs::read_to_string(project.root.join("target/cli-memory/short_term.json")).unwrap(),
    )
    .unwrap();
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "execution_stopped"
            && event["payload"]["status"] == "cancelled"));
    assert!(!events
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["kind"] == "tool_invoked"));
}
