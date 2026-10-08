#![allow(dead_code)]
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub struct Project {
    pub root: PathBuf,
}
impl Project {
    pub fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("roboclaw-workspace-{}", uuid::Uuid::new_v4()));
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
    pub fn config(&self, yaml: &str) {
        fs::write(self.root.join("roboclaw.yaml"), yaml).unwrap();
    }
    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_roboclaw"));
        command
            .current_dir(&self.root)
            .env("ROBOCLAW_ROS2_BRIDGE", "mock")
            .env("ROBOCLAW_SENSOR_FAIL_COUNT", "0")
            .env("ROBOCLAW_MOTOR_FAIL_COUNT", "0")
            .env_remove("ROBOCLAW_OPENAI_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .env_remove("ROBOCLAW_CLAUDE_API_KEY")
            .env_remove("ANTHROPIC_API_KEY");
        command
    }
    pub fn json(&self, args: &[&str]) -> Value {
        json(self.command().args(args).arg("--json").output().unwrap())
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
pub fn json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
