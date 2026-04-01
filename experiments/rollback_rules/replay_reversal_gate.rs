use super::{challenger_is_frontier, decision, RollbackCase, RollbackDecision, RollbackVariant};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct ReplayReversalGateVariant;

impl RollbackVariant for ReplayReversalGateVariant {
    fn name(&self) -> &'static str {
        "replay_reversal_gate"
    }

    fn style(&self) -> &'static str {
        "replay reversal"
    }

    fn philosophy(&self) -> &'static str {
        "Use repeated replay reversals as the primary rollback signal and defer when the regression window is still shallow."
    }

    fn source_path(&self) -> &'static str {
        "experiments/rollback_rules/replay_reversal_gate.rs"
    }

    fn decide(&self, case: &RollbackCase) -> Result<RollbackDecision> {
        Ok(
            if case.replay_signal == "switch_reference"
                && case.regression_rounds >= 2
                && challenger_is_frontier(case)
            {
                decision(
                    "replace_reference",
                    Some(case.challenger_reference.clone()),
                    "Repeated replay reversals and a frontier challenger justify replacement.",
                    4,
                )
            } else if case.replay_signal == "hold_experimental" && case.regression_rounds >= 2 {
                decision(
                "rollback_reference",
                Some(case.promoted_reference.clone()),
                "Repeated hold signals mean the promoted surface should return to experimental status.",
                4,
            )
            } else if case.replay_signal == "hold_experimental" {
                decision(
                    "defer_rollback",
                    None,
                    "Replay is wobbling, but the regression window is still too short to act.",
                    4,
                )
            } else {
                decision(
                    "keep_promoted",
                    Some(case.promoted_reference.clone()),
                    "Replay still supports the promoted surface.",
                    4,
                )
            },
        )
    }
}
