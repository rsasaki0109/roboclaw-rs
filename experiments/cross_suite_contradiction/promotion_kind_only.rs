use super::{decision, ContradictionCase, ContradictionDecision, ContradictionVariant};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct PromotionKindOnlyVariant;

impl ContradictionVariant for PromotionKindOnlyVariant {
    fn name(&self) -> &'static str {
        "promotion_kind_only"
    }

    fn style(&self) -> &'static str {
        "decision kind only"
    }

    fn philosophy(&self) -> &'static str {
        "Classify contradiction only from whether promotion and replay choose the same decision kind."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_suite_contradiction/promotion_kind_only.rs"
    }

    fn detect(&self, case: &ContradictionCase) -> Result<ContradictionDecision> {
        Ok(if case.promotion_decision_kind == case.replay_signal {
            decision(
                "no_contradiction",
                None,
                "Promotion and replay agree on the same decision kind.",
                2,
            )
        } else if case.promotion_decision_kind != "hold_experimental"
            && case.replay_signal == "hold_experimental"
        {
            decision(
                "premature_promotion",
                case.promotion_reference.clone(),
                "Promotion moved while replay still says hold.",
                2,
            )
        } else {
            decision(
                "reference_conflict",
                case.promotion_reference.clone(),
                "Decision kinds disagree in a way this detector treats as a contradiction.",
                2,
            )
        })
    }
}
