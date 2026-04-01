use super::{decision, frontier_margin, PromotionCase, PromotionDecision, PromotionVariant};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct AccuracyFirstVariant;

impl PromotionVariant for AccuracyFirstVariant {
    fn name(&self) -> &'static str {
        "accuracy_first"
    }

    fn style(&self) -> &'static str {
        "raw accuracy"
    }

    fn philosophy(&self) -> &'static str {
        "Promote whichever candidate wins the current suite by the widest accuracy margin."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_rules/accuracy_first.rs"
    }

    fn decide(&self, case: &PromotionCase) -> Result<PromotionDecision> {
        let margin = frontier_margin(case);
        Ok(if margin >= 10.0 {
            decision(
                "promote_reference",
                Some(case.frontier_candidate.clone()),
                "Frontier accuracy margin alone is treated as enough promotion evidence.",
                1,
            )
        } else if margin <= -10.0 {
            decision(
                "switch_reference",
                Some(case.rival_candidate.clone()),
                "Rival accuracy margin alone is treated as enough switch evidence.",
                1,
            )
        } else {
            decision(
                "hold_experimental",
                None,
                "Accuracy tie or near-tie keeps the suite experimental.",
                1,
            )
        })
    }
}
