use super::{
    decision, frontier_contains, ContradictionCase, ContradictionDecision, ContradictionVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct RiskSignalGateVariant;

impl ContradictionVariant for RiskSignalGateVariant {
    fn name(&self) -> &'static str {
        "risk_signal_gate"
    }

    fn style(&self) -> &'static str {
        "risk signal gate"
    }

    fn philosophy(&self) -> &'static str {
        "Use frontier membership plus coarse runtime-risk signals before allowing a promotion to count as consistent."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_suite_contradiction/risk_signal_gate.rs"
    }

    fn detect(&self, case: &ContradictionCase) -> Result<ContradictionDecision> {
        Ok(
            if let Some(reference) = case.promotion_reference.as_deref() {
                if !frontier_contains(case, reference) {
                    decision(
                        "reference_conflict",
                        Some(reference.to_string()),
                        "Promotion escaped the local frontier set.",
                        4,
                    )
                } else if case.promotion_decision_kind != "hold_experimental"
                    && (case.environment_specific || case.runtime_risk >= 60)
                {
                    decision(
                        "risk_conflict",
                        Some(reference.to_string()),
                        "Promotion ignored an environment or runtime-risk gate.",
                        4,
                    )
                } else if case.promotion_decision_kind != case.replay_signal
                    && case.promotion_decision_kind != "hold_experimental"
                {
                    decision(
                        "premature_promotion",
                        Some(reference.to_string()),
                        "Promotion moved ahead of replay evidence.",
                        4,
                    )
                } else {
                    decision(
                        "no_contradiction",
                        None,
                        "No risk or replay gate was violated.",
                        4,
                    )
                }
            } else {
                decision(
                    "no_contradiction",
                    None,
                    "No promoted reference means the gate stays clear.",
                    4,
                )
            },
        )
    }
}
