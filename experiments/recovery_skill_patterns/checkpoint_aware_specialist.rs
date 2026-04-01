use super::{
    budget_exhausted, decision, RecoveryPatternCase, RecoveryPatternDecision,
    RecoveryPatternVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct CheckpointAwareSpecialistVariant;

impl RecoveryPatternVariant for CheckpointAwareSpecialistVariant {
    fn name(&self) -> &'static str {
        "checkpoint_aware_specialist"
    }

    fn style(&self) -> &'static str {
        "checkpoint aware"
    }

    fn philosophy(&self) -> &'static str {
        "Choose specialized recovery patterns that preserve checkpoint value and change the resume target when the world state has already advanced."
    }

    fn source_path(&self) -> &'static str {
        "experiments/recovery_skill_patterns/checkpoint_aware_specialist.rs"
    }

    fn design(&self, case: &RecoveryPatternCase) -> Result<RecoveryPatternDecision> {
        if budget_exhausted(case) {
            return Ok(decision(
                "manual_handoff",
                vec!["request_operator_assistance"],
                false,
                None,
                true,
                "Autonomous recovery budget is exhausted; hand over to the operator.",
            ));
        }

        let decision = match (case.failed_step.as_str(), case.failed_tool.as_str()) {
            ("detect_object", "sensor") => decision(
                "reset_rescan",
                vec!["move_home", "rescan_target"],
                true,
                Some("detect_object"),
                false,
                "Detection failures should reset observation pose and retry detection.",
            ),
            ("grasp", "motor_control") if case.held_object.is_some() => decision(
                "verify_hold",
                vec!["stabilize_pose", "check_gripper_state", "rescan_target"],
                true,
                Some("place_object"),
                false,
                "If the backend already believes the object is held, verify the hold and continue from placement.",
            ),
            ("grasp", "motor_control") if case.target_visible == Some(true) => decision(
                "pregrasp_verify",
                vec!["move_to_pregrasp", "verify_target"],
                true,
                Some("move_to_object"),
                false,
                "Visible grasp failures return to pre-grasp before resuming from the motion checkpoint.",
            ),
            ("place_object", "simulator") if case.held_object.is_some() => decision(
                "reconfirm_place",
                vec!["reconfirm_bin_pose", "retry_place"],
                true,
                Some("place_object"),
                false,
                "Place failures while still holding the object should reconfirm the bin and retry placement.",
            ),
            (_, "simulator") => decision(
                "direct_resume",
                vec![],
                true,
                Some(case.failed_step.as_str()),
                false,
                "Pure motion interruptions can resume from their existing checkpoint.",
            ),
            _ => decision(
                "direct_resume",
                vec![],
                true,
                Some(case.failed_step.as_str()),
                false,
                "Unclassified cases default to checkpoint resume.",
            ),
        };

        Ok(decision)
    }
}
