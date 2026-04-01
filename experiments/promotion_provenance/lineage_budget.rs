use super::{
    all_documented, decision, documented_replace_chain_to_current, documented_same_reference_chain,
    has_rollback, missing_documentation_count, origin_reference, PromotionProvenanceCase,
    PromotionProvenanceDecision, PromotionProvenanceVariant,
};
use anyhow::Result;

const CONFIRMED_THRESHOLD: f64 = 18.0;
const SUPERSEDED_THRESHOLD: f64 = 14.0;
const BROKEN_THRESHOLD: f64 = 16.0;

#[derive(Debug, Default)]
pub struct LineageBudgetVariant;

impl PromotionProvenanceVariant for LineageBudgetVariant {
    fn name(&self) -> &'static str {
        "lineage_budget"
    }

    fn style(&self) -> &'static str {
        "lineage budget"
    }

    fn philosophy(&self) -> &'static str {
        "Balance release continuity, replacement lineage, missing documentation, and rollback events before trusting promotion provenance."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_provenance/lineage_budget.rs"
    }

    fn decide(&self, case: &PromotionProvenanceCase) -> Result<PromotionProvenanceDecision> {
        let broken_budget = broken_budget(case);
        let superseded_budget = superseded_budget(case);
        let confirmed_budget = confirmed_budget(case);

        Ok(if broken_budget >= BROKEN_THRESHOLD {
            decision(
                "provenance_broken",
                None,
                format!(
                    "Rollback pressure dominates provenance: broken={broken_budget:.2} superseded={superseded_budget:.2} confirmed={confirmed_budget:.2}."
                ),
                6,
            )
        } else if superseded_budget >= SUPERSEDED_THRESHOLD && superseded_budget > confirmed_budget
        {
            decision(
                "provenance_superseded",
                Some(case.current_reference.clone()),
                format!(
                    "Replacement lineage is strong enough: superseded={superseded_budget:.2} confirmed={confirmed_budget:.2} broken={broken_budget:.2}."
                ),
                6,
            )
        } else if confirmed_budget >= CONFIRMED_THRESHOLD {
            decision(
                "provenance_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "Continuous lineage is strong enough: confirmed={confirmed_budget:.2} superseded={superseded_budget:.2} broken={broken_budget:.2}."
                ),
                6,
            )
        } else {
            decision(
                "provenance_gap",
                None,
                format!(
                    "Lineage exists but is incomplete: confirmed={confirmed_budget:.2} superseded={superseded_budget:.2} broken={broken_budget:.2}."
                ),
                6,
            )
        })
    }
}

fn confirmed_budget(case: &PromotionProvenanceCase) -> f64 {
    let mut score = 0.0;
    if documented_same_reference_chain(case) {
        score += 18.0;
    }
    if all_documented(case) {
        score += 8.0;
    }
    score -= missing_documentation_count(case) as f64 * 8.0;
    if has_rollback(case) {
        score -= 24.0;
    }
    if origin_reference(case).as_deref() != Some(&case.current_reference) {
        score -= 8.0;
    }
    score
}

fn superseded_budget(case: &PromotionProvenanceCase) -> f64 {
    let mut score = 0.0;
    if documented_replace_chain_to_current(case) {
        score += 18.0;
    }
    if all_documented(case) {
        score += 6.0;
    }
    score -= missing_documentation_count(case) as f64 * 8.0;
    if has_rollback(case) {
        score -= 24.0;
    }
    score
}

fn broken_budget(case: &PromotionProvenanceCase) -> f64 {
    let mut score = 0.0;
    if has_rollback(case) {
        score += 22.0;
    }
    score += missing_documentation_count(case) as f64 * 4.0;
    score
}
