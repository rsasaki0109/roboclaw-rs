use super::{decision, PromotionCase, PromotionDecision, PromotionVariant};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct ReplayGateVariant;

impl PromotionVariant for ReplayGateVariant {
    fn name(&self) -> &'static str {
        "replay_gate"
    }

    fn style(&self) -> &'static str {
        "replay gate"
    }

    fn philosophy(&self) -> &'static str {
        "Trust replay evidence first, then require only light support from neighboring suites."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_rules/replay_gate.rs"
    }

    fn decide(&self, case: &PromotionCase) -> Result<PromotionDecision> {
        Ok(
            if case.replay_signal == "switch_reference" && case.rival_support_count >= 2 {
                decision(
                "switch_reference",
                Some(case.rival_candidate.clone()),
                "Replay already favors the rival and the switch is supported by neighboring suites.",
                3,
            )
            } else if case.replay_signal == "promote_reference" && case.support_count >= 2 {
                decision(
                "promote_reference",
                Some(case.frontier_candidate.clone()),
                "Replay already favors the frontier and the promotion is supported by neighboring suites.",
                3,
            )
            } else {
                decision(
                    "hold_experimental",
                    None,
                    "Replay evidence is too weak or too split to promote.",
                    3,
                )
            },
        )
    }
}
