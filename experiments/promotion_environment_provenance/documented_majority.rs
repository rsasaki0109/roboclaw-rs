use super::{
    decision, documented_coverage, documented_majority_reference, has_any_documentation_gap,
    has_blocking_environment, PromotionEnvironmentCase, PromotionEnvironmentDecision,
    PromotionEnvironmentVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct DocumentedMajorityVariant;

impl PromotionEnvironmentVariant for DocumentedMajorityVariant {
    fn name(&self) -> &'static str {
        "documented_majority"
    }

    fn style(&self) -> &'static str {
        "documented majority"
    }

    fn philosophy(&self) -> &'static str {
        "Trust the majority among documented environments and treat low documented coverage as a gap."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_environment_provenance/documented_majority.rs"
    }

    fn decide(&self, case: &PromotionEnvironmentCase) -> Result<PromotionEnvironmentDecision> {
        if documented_coverage(case) < 0.75 || has_any_documentation_gap(case) {
            return Ok(decision(
                "environment_gap",
                None,
                "Documented environment coverage is not broad enough.",
                3,
            ));
        }

        if has_blocking_environment(case) && documented_coverage(case) >= 1.0 {
            return Ok(decision(
                "environment_blocked",
                None,
                "A documented environment already rolled the reference back.",
                3,
            ));
        }

        let Some(reference) = documented_majority_reference(case) else {
            return Ok(decision(
                "environment_gap",
                None,
                "No documented environment majority exists.",
                3,
            ));
        };

        Ok(if reference == case.current_reference {
            decision(
                "environment_confirmed",
                Some(reference),
                "The current reference dominates documented environments.",
                3,
            )
        } else {
            decision(
                "environment_superseded",
                Some(reference),
                "Another reference dominates documented environments.",
                3,
            )
        })
    }
}
