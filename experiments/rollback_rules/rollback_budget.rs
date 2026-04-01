use super::{
    challenger_is_frontier, decision, promoted_is_frontier, RollbackCase, RollbackDecision,
    RollbackVariant,
};
use anyhow::Result;

const KEEP_THRESHOLD: f64 = 14.0;
const REPLACE_THRESHOLD: f64 = 18.0;
const ROLLBACK_THRESHOLD: f64 = 20.0;

#[derive(Debug, Default)]
pub struct RollbackBudgetVariant;

impl RollbackVariant for RollbackBudgetVariant {
    fn name(&self) -> &'static str {
        "rollback_budget"
    }

    fn style(&self) -> &'static str {
        "rollback budget"
    }

    fn philosophy(&self) -> &'static str {
        "Balance replay drift, contradiction pressure, support erosion, environment binding, and incident severity before rolling back stable runtime surface."
    }

    fn source_path(&self) -> &'static str {
        "experiments/rollback_rules/rollback_budget.rs"
    }

    fn decide(&self, case: &RollbackCase) -> Result<RollbackDecision> {
        let keep_budget = promoted_keep_budget(case);
        let replace_budget = challenger_replace_budget(case);
        let rollback_budget = promoted_rollback_budget(case);

        Ok(if should_defer_environmental_rollback(case) {
            decision(
                "defer_rollback",
                None,
                format!(
                    "Environment-bound drift is still shallow: keep={keep_budget:.2} rollback={rollback_budget:.2} replace={replace_budget:.2}."
                ),
                6,
            )
        } else if !case.environment_specific
            && replace_budget >= REPLACE_THRESHOLD
            && replace_budget > rollback_budget
            && replace_budget > keep_budget
        {
            decision(
                "replace_reference",
                Some(case.challenger_reference.clone()),
                format!(
                    "Replacement budget won: replace={replace_budget:.2} rollback={rollback_budget:.2} keep={keep_budget:.2}."
                ),
                6,
            )
        } else if rollback_budget >= ROLLBACK_THRESHOLD && rollback_budget >= keep_budget {
            decision(
                "rollback_reference",
                Some(case.promoted_reference.clone()),
                format!(
                    "Rollback budget won: rollback={rollback_budget:.2} keep={keep_budget:.2} replace={replace_budget:.2}."
                ),
                6,
            )
        } else if keep_budget >= KEEP_THRESHOLD && case.replay_signal == "promote_reference" {
            decision(
                "keep_promoted",
                Some(case.promoted_reference.clone()),
                format!(
                    "Keep budget stayed healthy: keep={keep_budget:.2} rollback={rollback_budget:.2} replace={replace_budget:.2}."
                ),
                6,
            )
        } else {
            decision(
                "defer_rollback",
                None,
                format!(
                    "Evidence is mixed: keep={keep_budget:.2} rollback={rollback_budget:.2} replace={replace_budget:.2}."
                ),
                6,
            )
        })
    }
}

fn should_defer_environmental_rollback(case: &RollbackCase) -> bool {
    case.environment_specific
        && case.replay_signal == "hold_experimental"
        && case.regression_rounds <= 1
        && case.runtime_incident_severity < 70
}

fn promoted_keep_budget(case: &RollbackCase) -> f64 {
    let mut score = 0.0;
    if promoted_is_frontier(case) {
        score += 10.0;
    }
    if case.replay_signal == "promote_reference" {
        score += 10.0;
    }
    if case.contradiction_signal == "no_contradiction" {
        score += 8.0;
    }
    score += case.support_gap as f64 * 4.0;
    score -= case.regression_rounds as f64 * 5.0;
    score -= case.runtime_incident_severity as f64 * 0.20;
    if case.environment_specific {
        score -= 6.0;
    }
    score
}

fn challenger_replace_budget(case: &RollbackCase) -> f64 {
    let mut score = 0.0;
    if challenger_is_frontier(case) {
        score += 12.0;
    }
    if case.replay_signal == "switch_reference" {
        score += 12.0;
    }
    score += (-case.support_gap).max(0) as f64 * 4.0;
    score += case.regression_rounds as f64 * 3.0;
    score -= case.runtime_incident_severity as f64 * 0.10;
    if case.environment_specific {
        score -= 16.0;
    }
    if case.contradiction_signal == "risk_conflict" {
        score -= 12.0;
    }
    score
}

fn promoted_rollback_budget(case: &RollbackCase) -> f64 {
    let mut score = 0.0;
    if case.replay_signal == "hold_experimental" {
        score += 8.0;
    }
    if case.contradiction_signal == "risk_conflict" {
        score += 12.0;
    } else if case.contradiction_signal == "premature_promotion" {
        score += 4.0;
    }
    score += case.regression_rounds as f64 * 4.0;
    score += case.runtime_incident_severity as f64 * 0.18;
    if case.environment_specific {
        score += 6.0;
    }
    if case.support_gap < 0 {
        score += (-case.support_gap) as f64 * 2.0;
    }
    score
}
