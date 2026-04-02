use anyhow::Result;
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
        let mut state = self.state.lock().expect("gazebo backend mutex poisoned");
        let detail = apply_command(&mut state, self.name(), &cmd);
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
        let mut state = self.state.lock().expect("real backend mutex poisoned");
        let detail = apply_command(&mut state, self.name(), &cmd);
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

fn apply_command(state: &mut RobotState, backend: &str, cmd: &Command) -> String {
    state.backend = backend.to_string();
    state.last_action = Some(cmd.action.clone());

    match cmd.action.as_str() {
        "move_to" => {
            if let Some(pose) = cmd.parameters.get("pose").and_then(Value::as_str) {
                state.last_pose = pose.to_string();
            }
            format!("moved {} to {}", backend, state.last_pose)
        }
        "grasp" => {
            let target = cmd
                .parameters
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("unknown_object");
            state.held_object = Some(target.to_string());
            format!("{} grasped {}", backend, target)
        }
        "place" => {
            let location = cmd
                .parameters
                .get("location")
                .and_then(Value::as_str)
                .unwrap_or("drop_zone");
            state.last_pose = location.to_string();
            state.held_object = None;
            format!("{} placed object at {}", backend, location)
        }
        other => format!("{} accepted {}", backend, other),
    }
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
