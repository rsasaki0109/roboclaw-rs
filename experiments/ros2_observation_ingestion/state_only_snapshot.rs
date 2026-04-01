use super::{
    active_skill_from_state, failed_step_from_state, held_object_from_state, latest_topic,
    pose_from_state, summary, IngestionCase, ObservationIngestionVariant, ObservationSummary,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct StateOnlySnapshotVariant;

impl ObservationIngestionVariant for StateOnlySnapshotVariant {
    fn name(&self) -> &'static str {
        "state_only_snapshot"
    }

    fn style(&self) -> &'static str {
        "state only"
    }

    fn philosophy(&self) -> &'static str {
        "Treat /roboclaw/state as the single source of truth and ignore the rest of the event stream."
    }

    fn source_path(&self) -> &'static str {
        "experiments/ros2_observation_ingestion/state_only_snapshot.rs"
    }

    fn ingest(&self, case: &IngestionCase) -> Result<ObservationSummary> {
        let state = latest_topic(case, "/roboclaw/state");
        Ok(summary(
            state.and_then(active_skill_from_state),
            state.and_then(pose_from_state),
            state.and_then(held_object_from_state),
            state.and_then(failed_step_from_state),
            None,
            if state.is_some() { "idle" } else { "unknown" },
        ))
    }
}
