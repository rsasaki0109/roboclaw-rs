use super::{
    all_critical_documented, blocking_environment_count, challenger_dominance,
    critical_reference_consensus, decision, documented_coverage, has_any_documentation_gap,
    has_blocking_critical_environment, PromotionEnvironmentCase, PromotionEnvironmentDecision,
    PromotionEnvironmentVariant,
};
use anyhow::Result;

const CONFIRMED_THRESHOLD: f64 = 16.0;
const SUPERSEDED_THRESHOLD: f64 = 14.0;
const BLOCKED_THRESHOLD: f64 = 18.0;

#[derive(Debug, Default)]
pub struct EnvironmentLineageBudgetVariant;

impl PromotionEnvironmentVariant for EnvironmentLineageBudgetVariant {
    fn name(&self) -> &'static str {
        "environment_lineage_budget"
    }

    fn style(&self) -> &'static str {
        "environment budget"
    }

    fn philosophy(&self) -> &'static str {
        "Balance critical-environment safety, documented coverage, challenger dominance, and rollout gaps before trusting multi-environment provenance."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_environment_provenance/environment_lineage_budget.rs"
    }

    fn decide(&self, case: &PromotionEnvironmentCase) -> Result<PromotionEnvironmentDecision> {
        let confirmed_budget = confirmed_budget(case);
        let superseded_budget = superseded_budget(case);
        let blocked_budget = blocked_budget(case);
        let challenger = challenger_dominance(case).map(|(reference, _)| reference);
        let critical_consensus = critical_reference_consensus(case);

        Ok(
            if blocked_budget >= BLOCKED_THRESHOLD && blocked_budget >= confirmed_budget {
                decision(
                "environment_blocked",
                None,
                format!(
                    "Blocked rollout budget dominates: blocked={blocked_budget:.2} confirmed={confirmed_budget:.2} superseded={superseded_budget:.2}."
                ),
                6,
            )
            } else if has_any_documentation_gap(case)
                || documented_coverage(case) < 0.75
                || !all_critical_documented(case)
            {
                decision(
                "environment_gap",
                None,
                format!(
                    "Coverage is incomplete: confirmed={confirmed_budget:.2} superseded={superseded_budget:.2} blocked={blocked_budget:.2}."
                ),
                6,
            )
            } else if critical_consensus.as_deref() == Some(case.current_reference.as_str())
                && challenger.as_deref() != Some(case.current_reference.as_str())
            {
                decision(
                "environment_gap",
                None,
                format!(
                    "Critical environments still converge on the current reference while broader rollout evidence points elsewhere: confirmed={confirmed_budget:.2} superseded={superseded_budget:.2} blocked={blocked_budget:.2}."
                ),
                6,
            )
            } else if superseded_budget >= SUPERSEDED_THRESHOLD
                && superseded_budget > confirmed_budget
            {
                decision(
                "environment_superseded",
                challenger,
                format!(
                    "Challenger rollout dominates: superseded={superseded_budget:.2} confirmed={confirmed_budget:.2} blocked={blocked_budget:.2}."
                ),
                6,
            )
            } else if confirmed_budget >= CONFIRMED_THRESHOLD {
                decision(
                "environment_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "Environment rollout is coherent: confirmed={confirmed_budget:.2} superseded={superseded_budget:.2} blocked={blocked_budget:.2}."
                ),
                6,
            )
            } else {
                decision(
                "environment_gap",
                None,
                format!(
                    "Environment evidence is mixed: confirmed={confirmed_budget:.2} superseded={superseded_budget:.2} blocked={blocked_budget:.2}."
                ),
                6,
            )
            },
        )
    }
}

fn confirmed_budget(case: &PromotionEnvironmentCase) -> f64 {
    let mut score = 0.0;
    if documented_coverage(case) >= 1.0 {
        score += 10.0;
    }
    if all_critical_documented(case) {
        score += 8.0;
    }
    if !has_blocking_critical_environment(case) {
        score += 6.0;
    }
    if let Some((reference, margin)) = challenger_dominance(case) {
        if reference == case.current_reference {
            score += 4.0 + margin as f64 * 2.0;
        } else {
            score -= 8.0;
        }
    }
    score -= blocking_environment_count(case) as f64 * 12.0;
    score
}

fn superseded_budget(case: &PromotionEnvironmentCase) -> f64 {
    let mut score = 0.0;
    if let Some((reference, margin)) = challenger_dominance(case) {
        if reference != case.current_reference {
            score += 10.0 + margin as f64 * 4.0;
        }
    }
    if documented_coverage(case) >= 0.75 {
        score += 4.0;
    }
    if has_blocking_critical_environment(case) {
        score -= 10.0;
    }
    score
}

fn blocked_budget(case: &PromotionEnvironmentCase) -> f64 {
    let mut score = 0.0;
    score += blocking_environment_count(case) as f64 * 10.0;
    if has_blocking_critical_environment(case) {
        score += 8.0;
    }
    if case
        .environment_snapshots
        .iter()
        .any(|snapshot| snapshot.critical && snapshot.decision_kind == "rollback_reference")
    {
        score += 6.0;
    }
    score
}
