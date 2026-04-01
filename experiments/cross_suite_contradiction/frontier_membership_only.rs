use super::{
    decision, frontier_contains, ContradictionCase, ContradictionDecision, ContradictionVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct FrontierMembershipOnlyVariant;

impl ContradictionVariant for FrontierMembershipOnlyVariant {
    fn name(&self) -> &'static str {
        "frontier_membership_only"
    }

    fn style(&self) -> &'static str {
        "membership check"
    }

    fn philosophy(&self) -> &'static str {
        "Treat contradiction detection as a simple question of whether promotion stays inside the local frontier set."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_suite_contradiction/frontier_membership_only.rs"
    }

    fn detect(&self, case: &ContradictionCase) -> Result<ContradictionDecision> {
        Ok(
            if let Some(reference) = case.promotion_reference.as_deref() {
                if !frontier_contains(case, reference) {
                    decision(
                        "reference_conflict",
                        Some(reference.to_string()),
                        "Promotion escaped the local frontier set.",
                        2,
                    )
                } else {
                    decision(
                        "no_contradiction",
                        None,
                        "Promotion stayed inside the local frontier set.",
                        2,
                    )
                }
            } else {
                decision(
                    "no_contradiction",
                    None,
                    "No promoted reference means no frontier-membership conflict.",
                    2,
                )
            },
        )
    }
}
