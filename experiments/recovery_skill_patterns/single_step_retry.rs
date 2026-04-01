use super::{
    budget_exhausted, decision, RecoveryPatternCase, RecoveryPatternDecision,
    RecoveryPatternVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct SingleStepRetryVariant;

impl RecoveryPatternVariant for SingleStepRetryVariant {
    fn name(&self) -> &'static str {
        "single_step_retry"
    }

    fn style(&self) -> &'static str {
        "micro retry"
    }

    fn philosophy(&self) -> &'static str {
        "Prefer the smallest possible intervention and retry the failed step directly."
    }

    fn source_path(&self) -> &'static str {
        "experiments/recovery_skill_patterns/single_step_retry.rs"
    }

    fn design(&self, case: &RecoveryPatternCase) -> Result<RecoveryPatternDecision> {
        if budget_exhausted(case) {
            return Ok(decision(
                "manual_handoff",
                vec!["request_operator_assistance"],
                false,
                None,
                true,
                "Retry budget exhausted; escalate to the operator.",
            ));
        }

        let decision = match case.failed_step.as_str() {
            "detect_object" => decision(
                "rescan_only",
                vec!["rescan_target"],
                true,
                Some("detect_object"),
                false,
                "Detection failures only trigger another scan.",
            ),
            "grasp" => decision(
                "retry_grasp",
                vec!["retry_grasp"],
                true,
                Some("grasp"),
                false,
                "Motor failures retry the grasp itself.",
            ),
            "place_object" => decision(
                "retry_place",
                vec!["retry_place"],
                true,
                Some("place_object"),
                false,
                "Placement failures retry only the release step.",
            ),
            _ => decision(
                "direct_resume",
                vec![],
                true,
                Some(case.failed_step.as_str()),
                false,
                "Non-manipulation failures resume the interrupted step directly.",
            ),
        };

        Ok(decision)
    }
}
