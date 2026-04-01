use super::{
    budget_exhausted, decision, RecoveryPatternCase, RecoveryPatternDecision,
    RecoveryPatternVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct VerificationFirstVariant;

impl RecoveryPatternVariant for VerificationFirstVariant {
    fn name(&self) -> &'static str {
        "verification_first"
    }

    fn style(&self) -> &'static str {
        "verification first"
    }

    fn philosophy(&self) -> &'static str {
        "Before resuming, verify the local world state with the cheapest extra observation available."
    }

    fn source_path(&self) -> &'static str {
        "experiments/recovery_skill_patterns/verification_first.rs"
    }

    fn design(&self, case: &RecoveryPatternCase) -> Result<RecoveryPatternDecision> {
        if budget_exhausted(case) {
            return Ok(decision(
                "manual_handoff",
                vec!["request_operator_assistance"],
                false,
                None,
                true,
                "Retry budget exhausted; exit to manual recovery.",
            ));
        }

        let decision = match case.failed_tool.as_str() {
            "sensor" => decision(
                "reset_rescan",
                vec!["move_home", "rescan_target"],
                true,
                Some("detect_object"),
                false,
                "Sensor failures rebuild perception from a known pose.",
            ),
            "motor_control" => {
                if case.held_object.is_some() {
                    decision(
                        "verify_hold",
                        vec!["stabilize_pose", "check_gripper_state", "rescan_target"],
                        true,
                        Some("move_to_object"),
                        false,
                        "A conflicting grasp state should be verified before resuming from the checkpoint.",
                    )
                } else {
                    decision(
                        "pregrasp_verify",
                        vec!["move_to_pregrasp", "verify_target"],
                        true,
                        Some("move_to_object"),
                        false,
                        "A missed grasp returns to pre-grasp and verifies the target.",
                    )
                }
            }
            "simulator" if case.failed_step == "place_object" => decision(
                "reconfirm_place",
                vec!["reconfirm_bin_pose", "retry_place"],
                true,
                Some("place_object"),
                false,
                "Placement failures confirm bin alignment before another place attempt.",
            ),
            _ => decision(
                "direct_resume",
                vec![],
                true,
                Some(case.failed_step.as_str()),
                false,
                "Motion-only interruptions resume directly.",
            ),
        };

        Ok(decision)
    }
}
