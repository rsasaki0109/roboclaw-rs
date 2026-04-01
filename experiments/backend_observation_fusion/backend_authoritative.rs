use super::{
    backend_held_object, backend_pose, decision, sensor_pose, sensor_visible, FusionCase,
    FusionDecision, FusionVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct BackendAuthoritativeVariant;

impl FusionVariant for BackendAuthoritativeVariant {
    fn name(&self) -> &'static str {
        "backend_authoritative"
    }

    fn style(&self) -> &'static str {
        "backend first"
    }

    fn philosophy(&self) -> &'static str {
        "Prefer backend state as the most durable source of truth."
    }

    fn source_path(&self) -> &'static str {
        "experiments/backend_observation_fusion/backend_authoritative.rs"
    }

    fn fuse(&self, case: &FusionCase) -> Result<FusionDecision> {
        let held_object = backend_held_object(case);
        let target_visible = sensor_visible(case).or_else(|| Some(held_object.is_some()));
        let replan_hint = match case.failed_tool.as_deref() {
            Some("sensor") => "reobserve_target",
            Some("motor_control") => "recover_grasp",
            Some("simulator") => "retry_place",
            _ => "resume_motion",
        };

        Ok(decision(
            backend_pose(case),
            target_visible,
            held_object,
            sensor_pose(case),
            "backend_authoritative",
            replan_hint,
        ))
    }
}
