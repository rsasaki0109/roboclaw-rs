use super::{
    backend_held_object, backend_pose, decision, sensor_confidence, sensor_pose, sensor_visible,
    FusionCase, FusionDecision, FusionVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct SensorFirstVariant;

impl FusionVariant for SensorFirstVariant {
    fn name(&self) -> &'static str {
        "sensor_first"
    }

    fn style(&self) -> &'static str {
        "sensor first"
    }

    fn philosophy(&self) -> &'static str {
        "Prefer direct perception whenever any sensor signal is available."
    }

    fn source_path(&self) -> &'static str {
        "experiments/backend_observation_fusion/sensor_first.rs"
    }

    fn fuse(&self, case: &FusionCase) -> Result<FusionDecision> {
        let target_visible = sensor_visible(case);
        let fused_pose = sensor_pose(case).or_else(|| backend_pose(case));
        let held_object = if target_visible == Some(false) && sensor_confidence(case) <= 0.1 {
            None
        } else {
            backend_held_object(case)
        };
        let replan_hint = if target_visible == Some(true) {
            match case.failed_tool.as_deref() {
                Some("motor_control") => "recover_grasp",
                Some("sensor") => "placement_verified",
                _ => "placement_verified",
            }
        } else {
            "reobserve_target"
        };

        Ok(decision(
            fused_pose,
            target_visible,
            held_object,
            sensor_pose(case),
            "sensor_first",
            replan_hint,
        ))
    }
}
