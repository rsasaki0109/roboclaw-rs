use super::{
    decision, latest_environment, PromotionEnvironmentCase, PromotionEnvironmentDecision,
    PromotionEnvironmentVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct LatestEnvironmentOnlyVariant;

impl PromotionEnvironmentVariant for LatestEnvironmentOnlyVariant {
    fn name(&self) -> &'static str {
        "latest_environment_only"
    }

    fn style(&self) -> &'static str {
        "latest environment"
    }

    fn philosophy(&self) -> &'static str {
        "Trust only the newest environment snapshot and ignore the broader rollout picture."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_environment_provenance/latest_environment_only.rs"
    }

    fn decide(&self, case: &PromotionEnvironmentCase) -> Result<PromotionEnvironmentDecision> {
        let Some(snapshot) = latest_environment(case) else {
            return Ok(decision(
                "environment_gap",
                None,
                "No environment snapshots are available.",
                1,
            ));
        };

        Ok(if !snapshot.documented {
            decision(
                "environment_gap",
                None,
                "Latest environment snapshot is undocumented.",
                1,
            )
        } else if snapshot.decision_kind == "rollback_reference" {
            decision(
                "environment_blocked",
                None,
                "Latest environment snapshot already rolled the reference back.",
                1,
            )
        } else if snapshot.reference.as_deref() == Some(case.current_reference.as_str()) {
            decision(
                "environment_confirmed",
                Some(case.current_reference.clone()),
                "Latest environment still supports the current reference.",
                1,
            )
        } else {
            decision(
                "environment_superseded",
                snapshot.reference.clone(),
                "Latest environment points at a different reference.",
                1,
            )
        })
    }
}
