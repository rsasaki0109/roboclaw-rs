use super::{
    backend_held_object, backend_pose, decision, sensor_confidence, sensor_pose, sensor_visible,
    FusionCase, FusionDecision, FusionVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct ConfidenceWeightedVariant;

impl FusionVariant for ConfidenceWeightedVariant {
    fn name(&self) -> &'static str {
        "confidence_weighted"
    }

    fn style(&self) -> &'static str {
        "confidence weighted"
    }

    fn philosophy(&self) -> &'static str {
        "Trust sensor data only when confidence is high, otherwise fall back to backend state."
    }

    fn source_path(&self) -> &'static str {
        "experiments/backend_observation_fusion/confidence_weighted.rs"
    }

    fn fuse(&self, case: &FusionCase) -> Result<FusionDecision> {
        let confident_sensor = sensor_confidence(case) >= 0.9;
        let target_visible = if confident_sensor {
            sensor_visible(case)
        } else {
            Some(false)
        };
        let target_pose = if confident_sensor {
            sensor_pose(case)
        } else {
            None
        };
        let held_object = backend_held_object(case);
        let replan_hint = if held_object.is_some() && target_visible == Some(false) {
            "verify_grasp_state"
        } else if case.failed_tool.as_deref() == Some("sensor") {
            "reobserve_target"
        } else if case.failed_tool.as_deref() == Some("simulator") {
            "retry_place"
        } else if target_visible == Some(true) {
            "recover_grasp"
        } else {
            "resume_motion"
        };

        Ok(decision(
            backend_pose(case),
            target_visible,
            held_object,
            target_pose,
            "confidence_weighted",
            replan_hint,
        ))
    }
}
