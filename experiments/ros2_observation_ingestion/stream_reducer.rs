use super::{
    action_event, active_skill_from_state, cmd_vel_event, event_name, failed_step_from_action,
    failed_step_from_state, held_object_from_state, joint_event, motion_state_from_cmd,
    pose_from_joint_state, pose_from_state, resume_step_from_action, skill_from_action,
    state_event, summary, IngestionCase, ObservationIngestionVariant, ObservationSummary,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct StreamReducerVariant;

impl ObservationIngestionVariant for StreamReducerVariant {
    fn name(&self) -> &'static str {
        "stream_reducer"
    }

    fn style(&self) -> &'static str {
        "event reducer"
    }

    fn philosophy(&self) -> &'static str {
        "Replay the ordered topic stream and reduce it into a coherent observation snapshot."
    }

    fn source_path(&self) -> &'static str {
        "experiments/ros2_observation_ingestion/stream_reducer.rs"
    }

    fn ingest(&self, case: &IngestionCase) -> Result<ObservationSummary> {
        let mut active_skill = None;
        let mut last_pose = None;
        let mut held_object = None;
        let mut failed_step = None;
        let mut resume_step = None;
        let mut motion_state = "unknown".to_string();
        let mut pending_replan = false;
        let mut pending_resume = false;

        for event in &case.events {
            if let Some(payload) = state_event(event) {
                active_skill = active_skill_from_state(payload).or(active_skill);
                last_pose = pose_from_state(payload).or(last_pose);
                held_object = Some(held_object_from_state(payload));
                failed_step = failed_step_from_state(payload).or(failed_step);
                continue;
            }

            if let Some(payload) = action_event(event) {
                match event_name(payload) {
                    Some("skill_selected") => {
                        let next_skill = skill_from_action(payload);
                        if next_skill.is_some()
                            && next_skill != active_skill
                            && (!pending_replan || pending_resume)
                        {
                            failed_step = None;
                        }
                        active_skill = next_skill.or(active_skill);
                        if let Some(selected_resume_step) = resume_step_from_action(payload) {
                            resume_step = Some(selected_resume_step);
                        }
                        pending_replan = false;
                        pending_resume = false;
                    }
                    Some("step_failed") => {
                        active_skill = skill_from_action(payload).or(active_skill);
                        failed_step = failed_step_from_action(payload).or(failed_step);
                    }
                    Some("execution_replan_requested") => {
                        if let Some(next_resume_step) = resume_step_from_action(payload) {
                            resume_step = Some(next_resume_step);
                        }
                        pending_replan = true;
                        pending_resume = false;
                    }
                    Some("recovery_completed") => {
                        if let Some(next_resume_step) = resume_step_from_action(payload) {
                            resume_step = Some(next_resume_step);
                        }
                        pending_replan = false;
                        pending_resume = true;
                    }
                    _ => {}
                }
                continue;
            }

            if let Some(payload) = joint_event(event) {
                if let Some(next_pose) = pose_from_joint_state(payload) {
                    last_pose = Some(next_pose);
                }
                continue;
            }

            if let Some(payload) = cmd_vel_event(event) {
                if let Some(next_motion_state) = motion_state_from_cmd(payload) {
                    motion_state = next_motion_state;
                }
            }
        }

        Ok(summary(
            active_skill,
            last_pose,
            held_object.flatten(),
            failed_step,
            resume_step,
            motion_state,
        ))
    }
}
