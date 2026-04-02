use super::{
    all_critical_documented, critical_reference_consensus, decision,
    has_blocking_critical_environment, PromotionEnvironmentCase, PromotionEnvironmentDecision,
    PromotionEnvironmentVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct CriticalEnvironmentGateVariant;

impl PromotionEnvironmentVariant for CriticalEnvironmentGateVariant {
    fn name(&self) -> &'static str {
        "critical_environment_gate"
    }

    fn style(&self) -> &'static str {
        "critical env gate"
    }

    fn philosophy(&self) -> &'static str {
        "Require critical environments to be documented and aligned before trusting broader rollout provenance."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_environment_provenance/critical_environment_gate.rs"
    }

    fn decide(&self, case: &PromotionEnvironmentCase) -> Result<PromotionEnvironmentDecision> {
        if has_blocking_critical_environment(case) {
            return Ok(decision(
                "environment_blocked",
                None,
                "A critical environment rolled the reference back.",
                4,
            ));
        }

        if !all_critical_documented(case) {
            return Ok(decision(
                "environment_gap",
                None,
                "Critical environment provenance is incomplete.",
                4,
            ));
        }

        match critical_reference_consensus(case).as_deref() {
            Some(reference) if reference == case.current_reference => Ok(decision(
                "environment_confirmed",
                Some(case.current_reference.clone()),
                "Critical environments align on the current reference.",
                4,
            )),
            Some(reference) => Ok(decision(
                "environment_superseded",
                Some(reference.to_string()),
                "Critical environments align on a challenger reference.",
                4,
            )),
            None => Ok(decision(
                "environment_gap",
                None,
                "Critical environments are split across multiple references.",
                4,
            )),
        }
    }
}
