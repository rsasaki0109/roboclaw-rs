use super::{
    challenger_is_frontier, decision, promoted_is_frontier, RollbackCase, RollbackDecision,
    RollbackVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct FrontierDropOnlyVariant;

impl RollbackVariant for FrontierDropOnlyVariant {
    fn name(&self) -> &'static str {
        "frontier_drop_only"
    }

    fn style(&self) -> &'static str {
        "frontier drop"
    }

    fn philosophy(&self) -> &'static str {
        "Rollback only when the promoted surface visibly falls out of the current frontier."
    }

    fn source_path(&self) -> &'static str {
        "experiments/rollback_rules/frontier_drop_only.rs"
    }

    fn decide(&self, case: &RollbackCase) -> Result<RollbackDecision> {
        Ok(
            if !promoted_is_frontier(case) && challenger_is_frontier(case) {
                decision(
                    "replace_reference",
                    Some(case.challenger_reference.clone()),
                    "The challenger replaced the promoted surface on the frontier.",
                    2,
                )
            } else if !promoted_is_frontier(case) {
                decision(
                    "rollback_reference",
                    Some(case.promoted_reference.clone()),
                    "The promoted surface fell off the frontier without a clear successor.",
                    2,
                )
            } else {
                decision(
                    "keep_promoted",
                    Some(case.promoted_reference.clone()),
                    "The promoted surface is still on the current frontier.",
                    2,
                )
            },
        )
    }
}
