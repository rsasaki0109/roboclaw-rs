use super::{
    budget_exhausted, decision, RecoveryPatternCase, RecoveryPatternDecision,
    RecoveryPatternVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct AlwaysResetRescanVariant;

impl RecoveryPatternVariant for AlwaysResetRescanVariant {
    fn name(&self) -> &'static str {
        "always_reset_rescan"
    }

    fn style(&self) -> &'static str {
        "global reset"
    }

    fn philosophy(&self) -> &'static str {
        "Any failure should return to a known-safe pose and rebuild context from scratch."
    }

    fn source_path(&self) -> &'static str {
        "experiments/recovery_skill_patterns/always_reset_rescan.rs"
    }

    fn design(&self, case: &RecoveryPatternCase) -> Result<RecoveryPatternDecision> {
        if budget_exhausted(case) {
            return Ok(decision(
                "manual_handoff",
                vec!["request_operator_assistance"],
                false,
                None,
                true,
                "Retry budget exhausted; stop autonomous recovery.",
            ));
        }

        Ok(decision(
            "reset_rescan",
            vec!["move_home", "rescan_target"],
            true,
            Some(case.failed_step.as_str()),
            false,
            "All recoveries reset pose and rebuild the world model before resuming.",
        ))
    }
}
