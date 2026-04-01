use super::{
    best_challenger_support, current_consensus_train_count, current_overdue_support,
    current_within_budget_support, decision, latest_train, rollback_support, ProvenanceLagCase,
    ProvenanceLagDecision, ProvenanceLagVariant,
};
use anyhow::Result;

const MIN_CONFIDENCE: f64 = 0.75;
const CURRENT_CONFIRM_THRESHOLD: f64 = 8.0;
const CHALLENGER_SUPERSEDE_THRESHOLD: f64 = 8.0;
const OVERDUE_CURRENT_THRESHOLD: f64 = 4.5;
const ROLLBACK_BLOCK_THRESHOLD: f64 = 5.5;
const LOW_CHALLENGER_THRESHOLD: f64 = 2.5;

#[derive(Debug, Default)]
pub struct LagBudgetVariant;

impl ProvenanceLagVariant for LagBudgetVariant {
    fn name(&self) -> &'static str {
        "lag_budget"
    }

    fn style(&self) -> &'static str {
        "lag budget"
    }

    fn philosophy(&self) -> &'static str {
        "Allow bounded publication lag, but only until overdue current signals or critical-surface challenger agreement make the release lineage clear."
    }

    fn source_path(&self) -> &'static str {
        "experiments/provenance_lag_budgets/lag_budget.rs"
    }

    fn decide(&self, case: &ProvenanceLagCase) -> Result<ProvenanceLagDecision> {
        let Some(latest) = latest_train(case) else {
            return Ok(decision(
                "lag_pending",
                None,
                "No publication train is available yet.",
                6,
            ));
        };

        let current_within =
            current_within_budget_support(latest, &case.current_reference, MIN_CONFIDENCE);
        let current_overdue =
            current_overdue_support(latest, &case.current_reference, MIN_CONFIDENCE);
        let challenger = best_challenger_support(latest, &case.current_reference, MIN_CONFIDENCE);
        let challenger_score = challenger
            .as_ref()
            .map(|(_, score, _, _)| *score)
            .unwrap_or(0.0);
        let challenger_surfaces = challenger
            .as_ref()
            .map(|(_, _, count, _)| *count)
            .unwrap_or(0);
        let challenger_critical = challenger
            .as_ref()
            .map(|(_, _, _, critical)| *critical)
            .unwrap_or(0);
        let (rollback_score, rollback_critical) = rollback_support(latest, MIN_CONFIDENCE);
        let current_history =
            current_consensus_train_count(case, MIN_CONFIDENCE, CURRENT_CONFIRM_THRESHOLD);

        Ok(
            if rollback_score >= ROLLBACK_BLOCK_THRESHOLD && rollback_critical >= 1 {
                decision(
                "lag_blocked",
                None,
                format!(
                    "A critical publication surface emitted rollback pressure: rollback_score={rollback_score:.2} rollback_critical={rollback_critical}."
                ),
                6,
            )
            } else if challenger_score >= CHALLENGER_SUPERSEDE_THRESHOLD
                && challenger_surfaces >= 2
                && (challenger_critical >= 2 || current_overdue >= OVERDUE_CURRENT_THRESHOLD)
            {
                decision(
                "lag_superseded",
                challenger.map(|(reference, _, _, _)| reference),
                format!(
                    "Challenger evidence outgrew the lag budget: challenger_score={challenger_score:.2} challenger_surfaces={challenger_surfaces} challenger_critical={challenger_critical} current_overdue={current_overdue:.2}."
                ),
                6,
            )
            } else if current_within >= CURRENT_CONFIRM_THRESHOLD
                && challenger_score == 0.0
                && rollback_score < 1.0
            {
                decision(
                "lag_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "All strong publication signals still support the current reference within their lag budgets: current_within={current_within:.2}."
                ),
                6,
            )
            } else if current_within >= CURRENT_CONFIRM_THRESHOLD
                && current_history >= 1
                && challenger_score <= LOW_CHALLENGER_THRESHOLD
                && rollback_score < 1.0
            {
                decision(
                "lag_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "The current reference was re-established after earlier lag noise: current_history={current_history} current_within={current_within:.2}."
                ),
                6,
            )
            } else if challenger_score > 0.0 && current_within > 0.0 && current_overdue == 0.0 {
                decision(
                "lag_pending",
                None,
                format!(
                    "Some surfaces moved ahead, but the remaining current signals are still within their lag budgets: current_within={current_within:.2} challenger_score={challenger_score:.2}."
                ),
                6,
            )
            } else if current_overdue >= OVERDUE_CURRENT_THRESHOLD && challenger_score > 0.0 {
                decision(
                "lag_blocked",
                None,
                format!(
                    "Publication lag expired without enough challenger agreement: current_overdue={current_overdue:.2} challenger_score={challenger_score:.2} challenger_surfaces={challenger_surfaces}."
                ),
                6,
            )
            } else if current_within >= CURRENT_CONFIRM_THRESHOLD && rollback_score < 1.0 {
                decision(
                "lag_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "The current reference still has enough within-budget publication support: current_within={current_within:.2} challenger_score={challenger_score:.2}."
                ),
                6,
            )
            } else {
                decision(
                "lag_pending",
                None,
                format!(
                    "The latest publication surfaces have not converged enough to spend the lag budget: current_within={current_within:.2} current_overdue={current_overdue:.2} challenger_score={challenger_score:.2} rollback_score={rollback_score:.2}."
                ),
                6,
            )
            },
        )
    }
}
