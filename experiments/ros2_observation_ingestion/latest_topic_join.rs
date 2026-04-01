use super::{
    active_skill_from_state, failed_step_from_action, failed_step_from_state,
    held_object_from_state, latest_topic, motion_state_from_cmd, pose_from_state,
    resume_step_from_action, skill_from_action, summary, IngestionCase,
    ObservationIngestionVariant, ObservationSummary,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct LatestTopicJoinVariant;

impl ObservationIngestionVariant for LatestTopicJoinVariant {
    fn name(&self) -> &'static str {
        "latest_topic_join"
    }

    fn style(&self) -> &'static str {
        "latest topic join"
    }

    fn philosophy(&self) -> &'static str {
        "Take the latest message per topic and join them without replaying event order."
    }

    fn source_path(&self) -> &'static str {
        "experiments/ros2_observation_ingestion/latest_topic_join.rs"
    }

    fn ingest(&self, case: &IngestionCase) -> Result<ObservationSummary> {
        let state = latest_topic(case, "/roboclaw/state");
        let action = latest_topic(case, "/roboclaw/action");
        let cmd = latest_topic(case, "/cmd_vel");

        Ok(summary(
            action
                .and_then(skill_from_action)
                .or_else(|| state.and_then(active_skill_from_state)),
            state.and_then(pose_from_state),
            state.and_then(held_object_from_state),
            action
                .and_then(failed_step_from_action)
                .or_else(|| state.and_then(failed_step_from_state)),
            action.and_then(resume_step_from_action),
            cmd.and_then(motion_state_from_cmd)
                .unwrap_or_else(|| "unknown".to_string()),
        ))
    }
}
