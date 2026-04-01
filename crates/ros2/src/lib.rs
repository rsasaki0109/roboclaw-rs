use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env;
use std::fmt;
use std::sync::{Arc, Mutex};

pub const CMD_VEL_TOPIC: &str = "/cmd_vel";
pub const JOINT_STATES_TOPIC: &str = "/joint_states";
pub const ROBOCLAW_ACTION_TOPIC: &str = "/roboclaw/action";
pub const ROBOCLAW_STATE_TOPIC: &str = "/roboclaw/state";

pub const MOCK_TRANSPORT_NAME: &str = "mock";
pub const RCLRS_TRANSPORT_NAME: &str = "rclrs";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublishedMessage {
    pub topic: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TwistCommand {
    pub linear: f64,
    pub angular: f64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JointStateMessage {
    pub joint_names: Vec<String>,
    pub positions: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoboclawActionMessage {
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoboclawStateMessage {
    pub backend: String,
    pub last_action: Option<String>,
    pub last_pose: String,
    pub held_object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_skill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps_executed: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_step: Option<String>,
}

#[derive(Clone)]
pub struct Ros2Bridge {
    node_name: String,
    published_messages: Arc<Mutex<Vec<PublishedMessage>>>,
    received_messages: Arc<Mutex<Vec<PublishedMessage>>>,
    #[cfg(feature = "ros2")]
    live: Option<Arc<rclrs_support::LiveRos2Bridge>>,
}

impl fmt::Debug for Ros2Bridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Ros2Bridge")
            .field("node_name", &self.node_name)
            .field("transport", &self.transport_name())
            .field("published_messages", &self.published_messages().len())
            .field("received_messages", &self.received_messages().len())
            .finish()
    }
}

impl Default for Ros2Bridge {
    fn default() -> Self {
        Self::mock("roboclaw_bridge")
    }
}

impl Ros2Bridge {
    pub fn mock(node_name: impl Into<String>) -> Self {
        Self {
            node_name: node_name.into(),
            published_messages: Arc::new(Mutex::new(Vec::new())),
            received_messages: Arc::new(Mutex::new(Vec::new())),
            #[cfg(feature = "ros2")]
            live: None,
        }
    }

    pub fn from_env(node_name: impl Into<String>) -> Result<Self> {
        let node_name = node_name.into();
        let mode =
            env::var("ROBOCLAW_ROS2_BRIDGE").unwrap_or_else(|_| MOCK_TRANSPORT_NAME.to_string());

        match mode.as_str() {
            MOCK_TRANSPORT_NAME => Ok(Self::mock(node_name)),
            RCLRS_TRANSPORT_NAME => {
                #[cfg(feature = "ros2")]
                {
                    return rclrs_support::bridge_with_rclrs(&node_name);
                }
                #[cfg(not(feature = "ros2"))]
                {
                    Err(anyhow!(
                        "ROBOCLAW_ROS2_BRIDGE=rclrs requires building with --features ros2"
                    ))
                }
            }
            other => Err(anyhow!(
                "unsupported ROBOCLAW_ROS2_BRIDGE='{other}', expected 'mock' or 'rclrs'"
            )),
        }
    }

    pub fn publish(&self, topic: &str, payload: Value) -> Result<()> {
        self.published_messages
            .lock()
            .expect("ros2 bridge mutex poisoned")
            .push(PublishedMessage {
                topic: topic.to_string(),
                payload: payload.clone(),
            });

        #[cfg(feature = "ros2")]
        if let Some(live) = &self.live {
            live.publish(topic, &payload)?;
        }

        Ok(())
    }

    pub fn publish_cmd_vel(&self, message: &TwistCommand) -> Result<()> {
        self.publish_serialized(CMD_VEL_TOPIC, message)
    }

    pub fn publish_joint_states(&self, message: &JointStateMessage) -> Result<()> {
        self.publish_serialized(JOINT_STATES_TOPIC, message)
    }

    pub fn publish_action(&self, message: &RoboclawActionMessage) -> Result<()> {
        self.publish_serialized(ROBOCLAW_ACTION_TOPIC, message)
    }

    pub fn publish_state(&self, message: &RoboclawStateMessage) -> Result<()> {
        self.publish_serialized(ROBOCLAW_STATE_TOPIC, message)
    }

    pub fn published_messages(&self) -> Vec<PublishedMessage> {
        self.published_messages
            .lock()
            .expect("ros2 bridge mutex poisoned")
            .clone()
    }

    pub fn received_messages(&self) -> Vec<PublishedMessage> {
        self.received_messages
            .lock()
            .expect("ros2 bridge mutex poisoned")
            .clone()
    }

    pub fn node_name(&self) -> &str {
        &self.node_name
    }

    pub fn transport_name(&self) -> &'static str {
        #[cfg(feature = "ros2")]
        if self.live.is_some() {
            return RCLRS_TRANSPORT_NAME;
        }

        MOCK_TRANSPORT_NAME
    }

    fn publish_serialized<T: Serialize>(&self, topic: &str, message: &T) -> Result<()> {
        self.publish(topic, serde_json::to_value(message)?)
    }
}

#[cfg(feature = "ros2")]
pub mod rclrs_support {
    use super::{
        PublishedMessage, Ros2Bridge, CMD_VEL_TOPIC, JOINT_STATES_TOPIC, ROBOCLAW_ACTION_TOPIC,
        ROBOCLAW_STATE_TOPIC,
    };
    use anyhow::{anyhow, Context, Result};
    use rclrs::{
        CreateBasicExecutor, DynamicMessage, DynamicPublisher, DynamicSubscription, MessageInfo,
        MessageTypeName, RclrsErrorFilter, SimpleValue, SimpleValueMut, SpinOptions,
        Value as DynamicValue, ValueMut as DynamicValueMut,
    };
    use rosidl_runtime_rs::{Sequence, String as RosString};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::convert::TryInto;
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;

    const STD_STRING_TYPE: &str = "std_msgs/msg/String";
    const TWIST_TYPE: &str = "geometry_msgs/msg/Twist";
    const JOINT_STATE_TYPE: &str = "sensor_msgs/msg/JointState";

    pub struct LiveRos2Bridge {
        publishers: HashMap<String, DynamicPublisher>,
        _subscriptions: Vec<DynamicSubscription>,
        executor_commands: Arc<rclrs::ExecutorCommands>,
        spin_thread: Mutex<Option<JoinHandle<()>>>,
    }

    pub fn bridge_with_rclrs(node_name: &str) -> Result<Ros2Bridge> {
        let published_messages = Arc::new(Mutex::new(Vec::new()));
        let received_messages = Arc::new(Mutex::new(Vec::new()));
        let live = Arc::new(LiveRos2Bridge::new(
            node_name,
            Arc::clone(&received_messages),
        )?);

        Ok(Ros2Bridge {
            node_name: node_name.to_string(),
            published_messages,
            received_messages,
            live: Some(live),
        })
    }

    pub fn bridge_from_env(node_name: &str) -> Result<Ros2Bridge> {
        Ros2Bridge::from_env(node_name.to_string())
    }

    impl LiveRos2Bridge {
        fn new(
            node_name: &str,
            received_messages: Arc<Mutex<Vec<PublishedMessage>>>,
        ) -> Result<Self> {
            let context = rclrs::Context::default_from_env()
                .context("failed to initialize ROS2 context from environment")?;
            let mut executor = context.create_basic_executor();
            let node = executor
                .create_node(node_name)
                .with_context(|| format!("failed to create ROS2 node '{node_name}'"))?;

            let mut publishers = HashMap::new();
            let mut subscriptions = Vec::new();
            for (topic, message_type) in topic_type_map() {
                let publisher = node
                    .create_dynamic_publisher(message_type_name(message_type)?, topic)
                    .with_context(|| {
                        format!("failed to create ROS2 publisher for topic '{topic}'")
                    })?;
                publishers.insert(topic.to_string(), publisher);

                let topic_name = topic.to_string();
                let subscription_store = Arc::clone(&received_messages);
                let subscription = node
                    .create_dynamic_subscription(
                        message_type_name(message_type)?,
                        topic,
                        move |message: DynamicMessage, _info: MessageInfo| {
                            if let Ok(payload) = decode_message_for_topic(&topic_name, &message) {
                                subscription_store
                                    .lock()
                                    .expect("ros2 received message mutex poisoned")
                                    .push(PublishedMessage {
                                        topic: topic_name.clone(),
                                        payload,
                                    });
                            }
                        },
                    )
                    .with_context(|| {
                        format!("failed to create ROS2 subscription for topic '{topic}'")
                    })?;
                subscriptions.push(subscription);
            }

            let executor_commands = Arc::clone(executor.commands());
            let spin_thread = std::thread::Builder::new()
                .name(format!("roboclaw-rclrs-{node_name}"))
                .spawn(move || {
                    let _ = executor.spin(SpinOptions::default()).first_error();
                })
                .context("failed to spawn ROS2 executor thread")?;

            Ok(Self {
                publishers,
                _subscriptions: subscriptions,
                executor_commands,
                spin_thread: Mutex::new(Some(spin_thread)),
            })
        }

        pub fn publish(&self, topic: &str, payload: &Value) -> Result<()> {
            let publisher = self
                .publishers
                .get(topic)
                .ok_or_else(|| anyhow!("no ROS2 publisher registered for topic '{topic}'"))?;
            let message = encode_message_for_topic(topic, payload)?;
            publisher
                .publish(message)
                .with_context(|| format!("failed to publish ROS2 message on topic '{topic}'"))?;
            Ok(())
        }
    }

    fn topic_type_map() -> [(&'static str, &'static str); 4] {
        [
            (CMD_VEL_TOPIC, TWIST_TYPE),
            (JOINT_STATES_TOPIC, JOINT_STATE_TYPE),
            (ROBOCLAW_ACTION_TOPIC, STD_STRING_TYPE),
            (ROBOCLAW_STATE_TOPIC, STD_STRING_TYPE),
        ]
    }

    fn message_type_name(type_name: &str) -> Result<MessageTypeName> {
        type_name
            .try_into()
            .with_context(|| format!("failed to parse ROS2 message type '{type_name}'"))
    }

    fn encode_message_for_topic(topic: &str, payload: &Value) -> Result<DynamicMessage> {
        match topic {
            CMD_VEL_TOPIC => encode_twist_message(payload),
            JOINT_STATES_TOPIC => encode_joint_state_message(payload),
            ROBOCLAW_ACTION_TOPIC | ROBOCLAW_STATE_TOPIC => encode_json_string_message(payload),
            _ => Err(anyhow!("unsupported ROS2 topic '{topic}'")),
        }
    }

    fn decode_message_for_topic(topic: &str, message: &DynamicMessage) -> Result<Value> {
        match topic {
            CMD_VEL_TOPIC => decode_twist_message(message),
            JOINT_STATES_TOPIC => decode_joint_state_message(message),
            ROBOCLAW_ACTION_TOPIC | ROBOCLAW_STATE_TOPIC => decode_json_string_message(message),
            _ => Err(anyhow!("unsupported ROS2 topic '{topic}'")),
        }
    }

    fn encode_json_string_message(payload: &Value) -> Result<DynamicMessage> {
        let mut message = DynamicMessage::new(message_type_name(STD_STRING_TYPE)?)
            .context("failed to create std_msgs/String message")?;

        match message.get_mut("data") {
            Some(DynamicValueMut::Simple(SimpleValueMut::String(value))) => {
                *value = RosString::from(payload.to_string());
                Ok(message)
            }
            _ => Err(anyhow!("std_msgs/String missing mutable 'data' field")),
        }
    }

    fn decode_json_string_message(message: &DynamicMessage) -> Result<Value> {
        match message.get("data") {
            Some(DynamicValue::Simple(SimpleValue::String(value))) => {
                let raw = value.to_string();
                Ok(serde_json::from_str(&raw).unwrap_or_else(|_| Value::String(raw)))
            }
            _ => Err(anyhow!("std_msgs/String missing readable 'data' field")),
        }
    }

    fn encode_twist_message(payload: &Value) -> Result<DynamicMessage> {
        let mut message = DynamicMessage::new(message_type_name(TWIST_TYPE)?)
            .context("failed to create geometry_msgs/Twist message")?;
        let linear = payload
            .get("linear")
            .and_then(Value::as_f64)
            .unwrap_or_default();
        let angular = payload
            .get("angular")
            .and_then(Value::as_f64)
            .unwrap_or_default();

        match message.get_mut("linear") {
            Some(DynamicValueMut::Simple(SimpleValueMut::Message(mut vector))) => {
                set_double_field(&mut vector, "x", linear)?;
            }
            _ => {
                return Err(anyhow!(
                    "geometry_msgs/Twist missing mutable 'linear' field"
                ))
            }
        }

        match message.get_mut("angular") {
            Some(DynamicValueMut::Simple(SimpleValueMut::Message(mut vector))) => {
                set_double_field(&mut vector, "z", angular)?;
            }
            _ => {
                return Err(anyhow!(
                    "geometry_msgs/Twist missing mutable 'angular' field"
                ))
            }
        }

        Ok(message)
    }

    fn decode_twist_message(message: &DynamicMessage) -> Result<Value> {
        let linear = match message.get("linear") {
            Some(DynamicValue::Simple(SimpleValue::Message(vector))) => {
                get_double_field(&vector, "x")?
            }
            _ => return Err(anyhow!("geometry_msgs/Twist missing 'linear' field")),
        };
        let angular = match message.get("angular") {
            Some(DynamicValue::Simple(SimpleValue::Message(vector))) => {
                get_double_field(&vector, "z")?
            }
            _ => return Err(anyhow!("geometry_msgs/Twist missing 'angular' field")),
        };

        Ok(json!({
            "linear": linear,
            "angular": angular,
        }))
    }

    fn encode_joint_state_message(payload: &Value) -> Result<DynamicMessage> {
        let mut message = DynamicMessage::new(message_type_name(JOINT_STATE_TYPE)?)
            .context("failed to create sensor_msgs/JointState message")?;
        let joint_names = payload
            .get("joint_names")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let positions = payload
            .get("positions")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_f64).collect::<Vec<_>>())
            .unwrap_or_default();

        match message.get_mut("name") {
            Some(DynamicValueMut::Sequence(rclrs::SequenceValueMut::StringSequence(sequence))) => {
                *sequence = Sequence::from(
                    joint_names
                        .into_iter()
                        .map(RosString::from)
                        .collect::<Vec<_>>(),
                );
            }
            _ => {
                return Err(anyhow!(
                    "sensor_msgs/JointState missing mutable 'name' field"
                ))
            }
        }

        match message.get_mut("position") {
            Some(DynamicValueMut::Sequence(rclrs::SequenceValueMut::DoubleSequence(sequence))) => {
                *sequence = Sequence::from(positions);
            }
            _ => {
                return Err(anyhow!(
                    "sensor_msgs/JointState missing mutable 'position' field"
                ))
            }
        }

        Ok(message)
    }

    fn decode_joint_state_message(message: &DynamicMessage) -> Result<Value> {
        let joint_names = match message.get("name") {
            Some(DynamicValue::Sequence(rclrs::SequenceValue::StringSequence(sequence))) => {
                sequence
                    .iter()
                    .map(|name| name.to_string())
                    .collect::<Vec<_>>()
            }
            _ => return Err(anyhow!("sensor_msgs/JointState missing 'name' field")),
        };

        let positions = match message.get("position") {
            Some(DynamicValue::Sequence(rclrs::SequenceValue::DoubleSequence(sequence))) => {
                sequence.to_vec()
            }
            _ => return Err(anyhow!("sensor_msgs/JointState missing 'position' field")),
        };

        Ok(json!({
            "joint_names": joint_names,
            "positions": positions,
        }))
    }

    fn set_double_field(
        message: &mut rclrs::DynamicMessageViewMut<'_>,
        field_name: &str,
        value: f64,
    ) -> Result<()> {
        match message.get_mut(field_name) {
            Some(DynamicValueMut::Simple(SimpleValueMut::Double(slot))) => {
                *slot = value;
                Ok(())
            }
            _ => Err(anyhow!("expected mutable double field '{field_name}'")),
        }
    }

    fn get_double_field(message: &rclrs::DynamicMessageView<'_>, field_name: &str) -> Result<f64> {
        match message.get(field_name) {
            Some(DynamicValue::Simple(SimpleValue::Double(value))) => Ok(*value),
            _ => Err(anyhow!("expected readable double field '{field_name}'")),
        }
    }

    impl Drop for LiveRos2Bridge {
        fn drop(&mut self) {
            self.executor_commands.halt_spinning();
            if let Some(handle) = self
                .spin_thread
                .lock()
                .expect("ros2 spin thread mutex poisoned")
                .take()
            {
                let _ = handle.join();
            }
        }
    }
}
