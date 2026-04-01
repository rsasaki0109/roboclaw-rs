use super::{
    action_event, active_skill_from_state, cmd_vel_event, event_name, failed_step_from_action,
    failed_step_from_state, held_object_from_state, motion_state_from_cmd, pose_from_state,
    resume_step_from_action, skill_from_action, state_event, summary, IngestionCase,
    ObservationIngestionVariant, ObservationSummary,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct ActionStateJoinVariant;

impl ObservationIngestionVariant for ActionStateJoinVariant {
    fn name(&self) -> &'static str {
        "action_state_join"
    }

    fn style(&self) -> &'static str {
        "action-state join"
    }

    fn philosophy(&self) -> &'static str {
        "Use action events for planner context, state messages for backend truth, and cmd_vel for motion."
    }

    fn source_path(&self) -> &'static str {
        "experiments/ros2_observation_ingestion/action_state_join.rs"
    }

    fn ingest(&self, case: &IngestionCase) -> Result<ObservationSummary> {
        let mut latest_state = None;
        let mut latest_action_context = None;
        let mut latest_resume_step = None;
        let mut motion_state = "unknown".to_string();

        for event in &case.events {
            if let Some(payload) = state_event(event) {
                latest_state = Some(payload);
            }

            if let Some(payload) = action_event(event) {
                match event_name(payload) {
                    Some("skill_selected") | Some("step_failed") | Some("recovery_completed") => {
                        latest_action_context = Some(payload);
                    }
                    Some("execution_replan_requested") => {
                        latest_resume_step = resume_step_from_action(payload);
                    }
                    _ => {}
                }
            }

            if let Some(payload) = cmd_vel_event(event) {
                if let Some(next_motion_state) = motion_state_from_cmd(payload) {
                    motion_state = next_motion_state;
                }
            }
        }

        Ok(summary(
            latest_action_context
                .and_then(skill_from_action)
                .or_else(|| latest_state.and_then(active_skill_from_state)),
            latest_state.and_then(pose_from_state),
            latest_state.and_then(held_object_from_state),
            latest_action_context
                .and_then(failed_step_from_action)
                .or_else(|| latest_state.and_then(failed_step_from_state)),
            latest_resume_step.or_else(|| latest_action_context.and_then(resume_step_from_action)),
            motion_state,
        ))
    }
}
