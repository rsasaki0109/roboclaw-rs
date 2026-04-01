use super::{
    backend_held_object, backend_pose, decision, sensor_confidence, sensor_pose, sensor_visible,
    FusionCase, FusionDecision, FusionVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct FailureAwareMergeVariant;

impl FusionVariant for FailureAwareMergeVariant {
    fn name(&self) -> &'static str {
        "failure_aware_merge"
    }

    fn style(&self) -> &'static str {
        "failure aware merge"
    }

    fn philosophy(&self) -> &'static str {
        "Fuse backend and sensor state differently depending on which subsystem just failed."
    }

    fn source_path(&self) -> &'static str {
        "experiments/backend_observation_fusion/failure_aware_merge.rs"
    }

    fn fuse(&self, case: &FusionCase) -> Result<FusionDecision> {
        let held_object = backend_held_object(case);
        let sensor_visible = sensor_visible(case);
        let sensor_pose = sensor_pose(case);
        let confident_sensor = sensor_confidence(case) >= 0.9;

        let (target_visible, target_pose, replan_hint) = match case.failed_tool.as_deref() {
            Some("sensor") => {
                if sensor_visible == Some(true) && confident_sensor {
                    (Some(true), sensor_pose.clone(), "placement_verified")
                } else {
                    (Some(false), None, "reobserve_target")
                }
            }
            Some("motor_control") => {
                if held_object.is_some() && sensor_visible == Some(false) {
                    (Some(false), None, "verify_grasp_state")
                } else if sensor_visible == Some(true) {
                    (Some(true), sensor_pose.clone(), "recover_grasp")
                } else {
                    (Some(false), None, "recover_grasp")
                }
            }
            Some("simulator") => {
                if case.failed_step.as_deref() == Some("place_object") && held_object.is_some() {
                    (Some(false), backend_pose(case), "retry_place")
                } else {
                    (sensor_visible, sensor_pose.clone(), "resume_motion")
                }
            }
            _ => (sensor_visible, sensor_pose.clone(), "resume_motion"),
        };

        Ok(decision(
            backend_pose(case),
            target_visible,
            held_object,
            target_pose,
            "failure_aware_merge",
            replan_hint,
        ))
    }
}
