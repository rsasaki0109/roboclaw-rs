use anyhow::{anyhow, Result};
use roboclaw_sim::{Command, RobotBackend};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::sync::Mutex;

pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn execute(&self, input: Value) -> Result<Value>;
}

#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn register_tool<T>(&mut self, tool: T)
    where
        T: Tool + 'static,
    {
        self.register(Arc::new(tool));
    }

    pub fn execute(&self, name: &str, input: Value) -> Result<Value> {
        let tool = self
            .tools
            .get(name)
            .cloned()
            .ok_or_else(|| anyhow!("tool '{}' is not registered", name))?;
        tool.execute(input)
    }

    pub fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.keys().cloned().collect();
        names.sort();
        names
    }
}

pub struct MotorControlTool {
    backend: Arc<dyn RobotBackend>,
    transient_failures: Mutex<HashMap<String, usize>>,
}

impl MotorControlTool {
    pub fn new(backend: Arc<dyn RobotBackend>) -> Self {
        let mut transient_failures = HashMap::new();
        if let Ok(raw_fail_count) = env::var("ROBOCLAW_MOTOR_FAIL_COUNT") {
            if let Ok(fail_count) = raw_fail_count.parse::<usize>() {
                if fail_count > 0 {
                    let action = env::var("ROBOCLAW_MOTOR_FAIL_ACTION")
                        .unwrap_or_else(|_| "grasp".to_string());
                    transient_failures.insert(action, fail_count);
                }
            }
        }
        Self {
            backend,
            transient_failures: Mutex::new(transient_failures),
        }
    }

    pub fn with_transient_failures(
        backend: Arc<dyn RobotBackend>,
        action: impl Into<String>,
        fail_count: usize,
    ) -> Self {
        let tool = Self::new(backend);
        tool.transient_failures
            .lock()
            .expect("motor transient failure mutex poisoned")
            .insert(action.into(), fail_count);
        tool
    }
}

impl Tool for MotorControlTool {
    fn name(&self) -> &str {
        "motor_control"
    }

    fn execute(&self, input: Value) -> Result<Value> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("grasp")
            .to_string();

        let mut transient_failures = self
            .transient_failures
            .lock()
            .expect("motor transient failure mutex poisoned");
        if let Some(remaining) = transient_failures.get_mut(&action) {
            if *remaining > 0 {
                *remaining -= 1;
                return Ok(json!({
                    "tool": self.name(),
                    "backend": self.backend.name(),
                    "accepted": false,
                    "detail": "transient motor stall",
                    "remaining_failures": *remaining,
                    "state": self.backend.current_state(),
                }));
            }
        }

        let ack = self.backend.send_command(Command {
            action,
            parameters: input,
        })?;
        Ok(json!({
            "tool": self.name(),
            "backend": ack.backend,
            "accepted": ack.accepted,
            "detail": ack.detail,
            "state": ack.state,
        }))
    }
}

pub struct SensorTool {
    observations: HashMap<String, SensorReading>,
    transient_failures: Mutex<HashMap<String, usize>>,
}

impl Default for SensorTool {
    fn default() -> Self {
        let mut observations = HashMap::new();
        observations.insert(
            "red_cube".to_string(),
            SensorReading {
                pose: "table/front_left".to_string(),
                confidence: 0.98,
            },
        );
        observations.insert(
            "bin_a".to_string(),
            SensorReading {
                pose: "table/back_right".to_string(),
                confidence: 0.95,
            },
        );
        let mut transient_failures = HashMap::new();
        if let Ok(raw_fail_count) = env::var("ROBOCLAW_SENSOR_FAIL_COUNT") {
            if let Ok(fail_count) = raw_fail_count.parse::<usize>() {
                if fail_count > 0 {
                    let target = env::var("ROBOCLAW_SENSOR_FAIL_TARGET")
                        .unwrap_or_else(|_| "red_cube".to_string());
                    transient_failures.insert(target, fail_count);
                }
            }
        }
        Self {
            observations,
            transient_failures: Mutex::new(transient_failures),
        }
    }
}

impl SensorTool {
    pub fn with_transient_failures(target: impl Into<String>, fail_count: usize) -> Self {
        let tool = Self::default();
        tool.transient_failures
            .lock()
            .expect("sensor transient failure mutex poisoned")
            .insert(target.into(), fail_count);
        tool
    }
}

impl Tool for SensorTool {
    fn name(&self) -> &str {
        "sensor"
    }

    fn execute(&self, input: Value) -> Result<Value> {
        let target = input
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("red_cube");

        let mut transient_failures = self
            .transient_failures
            .lock()
            .expect("sensor transient failure mutex poisoned");
        if let Some(remaining) = transient_failures.get_mut(target) {
            if *remaining > 0 {
                *remaining -= 1;
                return Ok(json!({
                    "tool": self.name(),
                    "target": target,
                    "detected": false,
                    "confidence": 0.0,
                    "detail": "transient sensor miss",
                    "remaining_failures": *remaining,
                }));
            }
        }

        let reading = self
            .observations
            .get(target)
            .ok_or_else(|| anyhow!("sensor target '{}' not found", target))?;

        Ok(json!({
            "tool": self.name(),
            "target": target,
            "detected": true,
            "pose": reading.pose,
            "confidence": reading.confidence,
        }))
    }
}

pub struct SimulatorTool {
    backend: Arc<dyn RobotBackend>,
}

impl SimulatorTool {
    pub fn new(backend: Arc<dyn RobotBackend>) -> Self {
        Self { backend }
    }
}

impl Tool for SimulatorTool {
    fn name(&self) -> &str {
        "simulator"
    }

    fn execute(&self, input: Value) -> Result<Value> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("move_to")
            .to_string();

        let ack = self.backend.send_command(Command {
            action,
            parameters: input,
        })?;

        Ok(json!({
            "tool": self.name(),
            "backend": ack.backend,
            "accepted": ack.accepted,
            "detail": ack.detail,
            "state": ack.state,
        }))
    }
}

struct SensorReading {
    pose: String,
    confidence: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn sensor_tool_supports_transient_failures() {
        let tool = SensorTool::with_transient_failures("red_cube", 1);

        let first = tool
            .execute(json!({ "target": "red_cube" }))
            .expect("first sensor call should return a transient miss");
        assert_eq!(first["detected"], Value::Bool(false));

        let second = tool
            .execute(json!({ "target": "red_cube" }))
            .expect("second sensor call should recover");
        assert_eq!(second["detected"], Value::Bool(true));
    }

    #[test]
    fn motor_tool_supports_transient_failures() {
        let backend: Arc<dyn RobotBackend> = Arc::new(roboclaw_sim::GazeboBackend::new());
        let tool = MotorControlTool::with_transient_failures(backend, "grasp", 1);

        let first = tool
            .execute(json!({ "action": "grasp", "target": "red_cube" }))
            .expect("first motor call should return a transient stall");
        assert_eq!(first["accepted"], Value::Bool(false));

        let second = tool
            .execute(json!({ "action": "grasp", "target": "red_cube" }))
            .expect("second motor call should recover");
        assert_eq!(second["accepted"], Value::Bool(true));
    }
}
