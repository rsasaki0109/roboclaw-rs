use anyhow::{bail, Result};
use roboclaw_ros2::{
    JointStateMessage, RoboclawActionMessage, RoboclawStateMessage, Ros2Bridge, TwistCommand,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Command {
    pub action: String,
    #[serde(default)]
    pub parameters: Value,
}

impl Command {
    /// Parse a tool input without supplying defaults for missing command fields.
    pub fn from_input(input: Value) -> Result<Self> {
        let action = required_string(&input, "action")?.to_string();
        let command = Self {
            action,
            parameters: input,
        };
        command.validate()?;
        Ok(command)
    }

    /// Validate the action and its parameters. Backends also check robot state.
    pub fn validate(&self) -> Result<()> {
        self.validated().map(|_| ())
    }

    fn validated(&self) -> Result<ValidatedCommand<'_>> {
        if !self.parameters.is_object() {
            bail!("command parameters must be an object");
        }
        if let Some(action) = self.parameters.get("action") {
            if action.as_str() != Some(self.action.as_str()) {
                bail!("command action does not match parameters.action");
            }
        }

        match self.action.as_str() {
            "move_to" => Ok(ValidatedCommand::MoveTo {
                pose: required_string(&self.parameters, "pose")?,
            }),
            "grasp" => Ok(ValidatedCommand::Grasp {
                target: required_string(&self.parameters, "target")?,
            }),
            "place" => Ok(ValidatedCommand::Place {
                target: required_string(&self.parameters, "target")?,
                location: required_string(&self.parameters, "location")?,
            }),
            other => bail!("unsupported command action '{other}'"),
        }
    }
}

enum ValidatedCommand<'a> {
    MoveTo { pose: &'a str },
    Grasp { target: &'a str },
    Place { target: &'a str, location: &'a str },
}

fn required_string<'a>(input: &'a Value, field: &str) -> Result<&'a str> {
    match input.get(field).and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => bail!("command field '{field}' must be a non-empty string"),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RobotState {
    pub backend: String,
    pub last_action: Option<String>,
    pub last_pose: String,
    pub held_object: Option<String>,
}

impl Default for RobotState {
    fn default() -> Self {
        Self {
            backend: "unknown".to_string(),
            last_action: None,
            last_pose: "home".to_string(),
            held_object: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendAck {
    pub backend: String,
    pub accepted: bool,
    pub detail: String,
    pub state: RobotState,
}

pub trait RobotBackend: Send + Sync {
    fn name(&self) -> &str;
    fn send_command(&self, cmd: Command) -> Result<BackendAck>;
    fn current_state(&self) -> RobotState;
}

#[derive(Debug, Clone)]
pub struct GazeboBackend {
    state: Arc<Mutex<RobotState>>,
    ros2: Option<Ros2Bridge>,
}

impl GazeboBackend {
    pub fn new() -> Self {
        Self::with_optional_ros2(None)
    }

    pub fn with_ros2(ros2: Ros2Bridge) -> Self {
        Self::with_optional_ros2(Some(ros2))
    }

    fn with_optional_ros2(ros2: Option<Ros2Bridge>) -> Self {
        Self {
            state: Arc::new(Mutex::new(RobotState {
                backend: "gazebo".to_string(),
                ..RobotState::default()
            })),
            ros2,
        }
    }
}

impl Default for GazeboBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl RobotBackend for GazeboBackend {
    fn name(&self) -> &str {
        "gazebo"
    }

    fn send_command(&self, cmd: Command) -> Result<BackendAck> {
        let validated = cmd.validated()?;
        let mut state = self.state.lock().expect("gazebo backend mutex poisoned");
        let detail = apply_command(&mut state, self.name(), validated)?;
        let ack = BackendAck {
            backend: self.name().to_string(),
            accepted: true,
            detail,
            state: state.clone(),
        };
        if let Some(ros2) = &self.ros2 {
            publish_backend_telemetry(ros2, &cmd, &ack)?;
        }
        Ok(ack)
    }

    fn current_state(&self) -> RobotState {
        self.state
            .lock()
            .expect("gazebo backend mutex poisoned")
            .clone()
    }
}

#[derive(Debug, Clone)]
pub struct RealRobotBackend {
    state: Arc<Mutex<RobotState>>,
    ros2: Option<Ros2Bridge>,
}

impl RealRobotBackend {
    pub fn new() -> Self {
        Self::with_optional_ros2(None)
    }

    pub fn with_ros2(ros2: Ros2Bridge) -> Self {
        Self::with_optional_ros2(Some(ros2))
    }

    fn with_optional_ros2(ros2: Option<Ros2Bridge>) -> Self {
        Self {
            state: Arc::new(Mutex::new(RobotState {
                backend: "real_robot".to_string(),
                ..RobotState::default()
            })),
            ros2,
        }
    }
}

impl Default for RealRobotBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl RobotBackend for RealRobotBackend {
    fn name(&self) -> &str {
        "real_robot"
    }

    fn send_command(&self, cmd: Command) -> Result<BackendAck> {
        let validated = cmd.validated()?;
        let mut state = self.state.lock().expect("real backend mutex poisoned");
        let detail = apply_command(&mut state, self.name(), validated)?;
        let ack = BackendAck {
            backend: self.name().to_string(),
            accepted: true,
            detail,
            state: state.clone(),
        };
        if let Some(ros2) = &self.ros2 {
            publish_backend_telemetry(ros2, &cmd, &ack)?;
        }
        Ok(ack)
    }

    fn current_state(&self) -> RobotState {
        self.state
            .lock()
            .expect("real backend mutex poisoned")
            .clone()
    }
}

fn apply_command(
    state: &mut RobotState,
    backend: &str,
    cmd: ValidatedCommand<'_>,
) -> Result<String> {
    let (action, detail) = match cmd {
        ValidatedCommand::MoveTo { pose } => {
            state.last_pose = pose.to_string();
            ("move_to", format!("moved {backend} to {pose}"))
        }
        ValidatedCommand::Grasp { target } => {
            if let Some(held) = &state.held_object {
                bail!("cannot grasp '{target}': already holding '{held}'");
            }
            state.held_object = Some(target.to_string());
            ("grasp", format!("{backend} grasped {target}"))
        }
        ValidatedCommand::Place { target, location } => {
            match state.held_object.as_deref() {
                None => bail!("cannot place '{target}': no object is held"),
                Some(held) if held != target => {
                    bail!("cannot place '{target}': holding '{held}'");
                }
                Some(_) => {}
            }
            state.last_pose = location.to_string();
            state.held_object = None;
            ("place", format!("{backend} placed object at {location}"))
        }
    };
    state.backend = backend.to_string();
    state.last_action = Some(action.to_string());
    Ok(detail)
}

pub fn state_to_ros2_message(state: &RobotState) -> RoboclawStateMessage {
    RoboclawStateMessage {
        backend: state.backend.clone(),
        last_action: state.last_action.clone(),
        last_pose: state.last_pose.clone(),
        held_object: state.held_object.clone(),
        active_skill: None,
        next_action: None,
        steps_executed: None,
        completed: None,
        failed_step: None,
    }
}

pub fn state_to_joint_state_message(state: &RobotState) -> JointStateMessage {
    let positions = match state.last_pose.as_str() {
        "table/front_left/pre_grasp" => vec![0.45, 0.1, 1.0],
        "bin_a" | "table/back_right" => vec![0.7, 0.2, 0.2],
        "gesture/wave_start" => vec![0.8, -0.4, 1.0],
        "gesture/wave_peak" => vec![0.9, 0.45, 1.0],
        "home" => vec![0.0, 0.0, 1.0],
        _ => vec![0.2, 0.0, 1.0],
    };

    JointStateMessage {
        joint_names: vec![
            "arm_lift".to_string(),
            "wrist_roll".to_string(),
            "gripper".to_string(),
        ],
        positions,
    }
}

pub fn command_to_twist_message(source: impl Into<String>, cmd: &Command) -> TwistCommand {
    let (linear, angular) = match cmd.action.as_str() {
        "move_to" => {
            let pose = cmd
                .parameters
                .get("pose")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if pose.contains("wave") {
                (0.05, 0.4)
            } else {
                (0.25, 0.0)
            }
        }
        "grasp" => (0.0, 0.0),
        "place" => (0.1, 0.0),
        _ => (0.0, 0.0),
    };

    TwistCommand {
        linear,
        angular,
        source: source.into(),
    }
}

fn publish_backend_telemetry(ros2: &Ros2Bridge, cmd: &Command, ack: &BackendAck) -> Result<()> {
    ros2.publish_action(&RoboclawActionMessage {
        event: "backend_command_accepted".to_string(),
        instruction: None,
        skill: None,
        step: None,
        tool: None,
        backend: Some(ack.backend.clone()),
        action: Some(cmd.action.clone()),
        detail: Some(ack.detail.clone()),
        data: Some(cmd.parameters.clone()),
    })?;
    ros2.publish_state(&state_to_ros2_message(&ack.state))?;
    ros2.publish_cmd_vel(&command_to_twist_message(ack.backend.clone(), cmd))?;
    ros2.publish_joint_states(&state_to_joint_state_message(&ack.state))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use roboclaw_ros2::{
        CMD_VEL_TOPIC, JOINT_STATES_TOPIC, ROBOCLAW_ACTION_TOPIC, ROBOCLAW_STATE_TOPIC,
    };
    use serde_json::json;

    fn backends() -> [(Box<dyn RobotBackend>, Ros2Bridge); 2] {
        let gazebo_ros2 = Ros2Bridge::mock("gazebo-validation-test");
        let real_ros2 = Ros2Bridge::mock("real-validation-test");
        [
            (
                Box::new(GazeboBackend::with_ros2(gazebo_ros2.clone())),
                gazebo_ros2,
            ),
            (
                Box::new(RealRobotBackend::with_ros2(real_ros2.clone())),
                real_ros2,
            ),
        ]
    }

    fn command(action: &str, parameters: Value) -> Command {
        Command {
            action: action.to_string(),
            parameters,
        }
    }

    fn assert_rejected(
        backend: &dyn RobotBackend,
        ros2: &Ros2Bridge,
        cmd: Command,
        expected_error: &str,
    ) {
        let before = backend.current_state();
        let messages = ros2.published_messages();
        let error = backend.send_command(cmd).expect_err("command must fail");
        assert!(
            error.to_string().contains(expected_error),
            "{}: expected {expected_error:?}, got {error}",
            backend.name()
        );
        assert_eq!(backend.current_state(), before);
        assert_eq!(ros2.published_messages(), messages);
    }

    #[test]
    fn backends_reject_invalid_commands_without_side_effects() {
        for (backend, ros2) in backends() {
            assert_rejected(
                backend.as_ref(),
                &ros2,
                command("launch", json!({})),
                "unsupported command action",
            );
            for parameters in [Value::Null, json!([]), json!("home")] {
                assert_rejected(
                    backend.as_ref(),
                    &ros2,
                    command("move_to", parameters),
                    "parameters must be an object",
                );
            }
            for action in [json!("grasp"), Value::Null, json!(42)] {
                assert_rejected(
                    backend.as_ref(),
                    &ros2,
                    command("move_to", json!({ "action": action, "pose": "home" })),
                    "action does not match",
                );
            }
            for (action, field, valid_parameters) in [
                ("move_to", "pose", json!({ "pose": "home" })),
                ("grasp", "target", json!({ "target": "red_cube" })),
                (
                    "place",
                    "target",
                    json!({ "target": "red_cube", "location": "bin_a" }),
                ),
                (
                    "place",
                    "location",
                    json!({ "target": "red_cube", "location": "bin_a" }),
                ),
            ] {
                for value in [None, Some(Value::Null), Some(json!(42)), Some(json!(" \t"))] {
                    let mut parameters = valid_parameters.clone();
                    match value {
                        None => {
                            parameters.as_object_mut().unwrap().remove(field);
                        }
                        Some(value) => parameters[field] = value,
                    }
                    assert_rejected(
                        backend.as_ref(),
                        &ros2,
                        command(action, parameters),
                        &format!("field '{field}' must be a non-empty string"),
                    );
                }
            }
        }
    }

    #[test]
    fn backends_reject_invalid_grasp_and_place_transitions() {
        for (backend, ros2) in backends() {
            assert_rejected(
                backend.as_ref(),
                &ros2,
                command(
                    "place",
                    json!({ "target": "red_cube", "location": "bin_a" }),
                ),
                "no object is held",
            );
            backend
                .send_command(command("grasp", json!({ "target": "red_cube" })))
                .unwrap();
            for target in ["red_cube", "blue_cube"] {
                assert_rejected(
                    backend.as_ref(),
                    &ros2,
                    command("grasp", json!({ "target": target })),
                    "already holding 'red_cube'",
                );
            }
            assert_rejected(
                backend.as_ref(),
                &ros2,
                command(
                    "place",
                    json!({ "target": "blue_cube", "location": "bin_a" }),
                ),
                "holding 'red_cube'",
            );
        }
    }

    #[test]
    fn backends_complete_valid_pick_and_place() {
        for (backend, _) in backends() {
            let move_ack = backend
                .send_command(command(
                    "move_to",
                    json!({ "pose": "table/front_left/pre_grasp" }),
                ))
                .unwrap();
            assert!(move_ack.accepted);
            assert_eq!(move_ack.state.last_pose, "table/front_left/pre_grasp");

            let grasp_ack = backend
                .send_command(command("grasp", json!({ "target": "red_cube" })))
                .unwrap();
            assert!(grasp_ack.accepted);
            assert_eq!(grasp_ack.state.held_object.as_deref(), Some("red_cube"));

            let place_ack = backend
                .send_command(command(
                    "place",
                    json!({ "target": "red_cube", "location": "bin_a" }),
                ))
                .unwrap();
            assert!(place_ack.accepted);
            assert_eq!(place_ack.state.backend, backend.name());
            assert_eq!(place_ack.state.last_action.as_deref(), Some("place"));
            assert_eq!(place_ack.state.last_pose, "bin_a");
            assert_eq!(place_ack.state.held_object, None);
        }
    }

    #[test]
    fn command_from_input_preserves_tool_payload_and_wire_format() {
        let input = json!({
            "action": "move_to",
            "pose": "home",
            "request_id": "request-1",
        });
        let cmd = Command::from_input(input.clone()).unwrap();
        assert_eq!(cmd.action, "move_to");
        assert_eq!(cmd.parameters, input);
        let encoded = serde_json::to_value(&cmd).unwrap();
        assert_eq!(encoded, json!({ "action": "move_to", "parameters": input }));
        let decoded: Command = serde_json::from_value(encoded).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, cmd);
    }

    #[test]
    fn gazebo_backend_publishes_ros2_telemetry() {
        let ros2 = Ros2Bridge::mock("sim-test");
        let backend = GazeboBackend::with_ros2(ros2.clone());

        let ack = backend
            .send_command(Command {
                action: "move_to".to_string(),
                parameters: json!({
                    "action": "move_to",
                    "pose": "table/front_left/pre_grasp",
                }),
            })
            .expect("gazebo move_to command should succeed");

        assert_eq!(ack.state.last_pose, "table/front_left/pre_grasp");

        let topics = ros2
            .published_messages()
            .into_iter()
            .map(|message| message.topic)
            .collect::<Vec<_>>();

        assert_eq!(
            topics,
            vec![
                ROBOCLAW_ACTION_TOPIC.to_string(),
                ROBOCLAW_STATE_TOPIC.to_string(),
                CMD_VEL_TOPIC.to_string(),
                JOINT_STATES_TOPIC.to_string(),
            ]
        );
    }
}
