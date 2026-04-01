use super::{
    decision, frontier_margin, support_delta, PromotionCase, PromotionDecision, PromotionVariant,
};
use anyhow::Result;

const BUDGET_THRESHOLD: f64 = 18.0;
const BUDGET_SEPARATION: f64 = 6.0;

#[derive(Debug, Default)]
pub struct StabilityBudgetVariant;

impl PromotionVariant for StabilityBudgetVariant {
    fn name(&self) -> &'static str {
        "stability_budget"
    }

    fn style(&self) -> &'static str {
        "stability budget"
    }

    fn philosophy(&self) -> &'static str {
        "Promote only when replay, neighboring suites, interface size, and runtime risk all fit inside a stable-runtime budget."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_rules/stability_budget.rs"
    }

    fn decide(&self, case: &PromotionCase) -> Result<PromotionDecision> {
        let margin = frontier_margin(case);
        let support_delta = support_delta(case) as f64;
        let frontier_budget = margin * 0.8 + support_delta * 8.0
            - case.interface_surface as f64 * 4.0
            - case.runtime_risk as f64 * 0.25
            - environment_penalty(case)
            + replay_bonus(&case.replay_signal, true);
        let rival_budget = -margin * 0.8
            - support_delta * 8.0
            - case.rival_interface_surface as f64 * 4.0
            - case.rival_runtime_risk as f64 * 0.25
            - environment_penalty(case)
            + replay_bonus(&case.replay_signal, false);

        Ok(
            if !case.environment_specific
                && case.support_count >= 3
                && frontier_budget >= BUDGET_THRESHOLD
                && frontier_budget > rival_budget + BUDGET_SEPARATION
            {
                decision(
                "promote_reference",
                Some(case.frontier_candidate.clone()),
                format!(
                    "Frontier promotion budget passed with frontier_budget={frontier_budget:.2} rival_budget={rival_budget:.2}."
                ),
                6,
            )
            } else if !case.environment_specific
                && case.rival_support_count >= 2
                && rival_budget >= BUDGET_THRESHOLD
                && rival_budget > frontier_budget + BUDGET_SEPARATION
            {
                decision(
                "switch_reference",
                Some(case.rival_candidate.clone()),
                format!(
                    "Rival promotion budget passed with rival_budget={rival_budget:.2} frontier_budget={frontier_budget:.2}."
                ),
                6,
            )
            } else {
                decision(
                "hold_experimental",
                None,
                format!(
                    "Stable-runtime budget not met: frontier_budget={frontier_budget:.2} rival_budget={rival_budget:.2}."
                ),
                6,
            )
            },
        )
    }
}

fn replay_bonus(signal: &str, frontier_side: bool) -> f64 {
    match (signal, frontier_side) {
        ("promote_reference", true) => 10.0,
        ("switch_reference", false) => 10.0,
        ("switch_reference", true) => -10.0,
        ("promote_reference", false) => -10.0,
        _ => 0.0,
    }
}

fn environment_penalty(case: &PromotionCase) -> f64 {
    if case.environment_specific {
        18.0
    } else {
        0.0
    }
}
