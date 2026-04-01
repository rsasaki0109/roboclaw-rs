use super::{decision, PromotionCase, PromotionDecision, PromotionVariant};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct SurfaceGuardVariant;

impl PromotionVariant for SurfaceGuardVariant {
    fn name(&self) -> &'static str {
        "surface_guard"
    }

    fn style(&self) -> &'static str {
        "surface guard"
    }

    fn philosophy(&self) -> &'static str {
        "Prefer candidates that add the least stable API surface and avoid risky runtime changes."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_rules/surface_guard.rs"
    }

    fn decide(&self, case: &PromotionCase) -> Result<PromotionDecision> {
        Ok(if case.environment_specific || case.runtime_risk >= 60 {
            decision(
                "hold_experimental",
                None,
                "Environment-specific or high-risk behavior stays outside the stable runtime.",
                4,
            )
        } else if case.rival_interface_surface + 1 < case.interface_surface
            && case.rival_runtime_risk <= case.runtime_risk
            && case.rival_accuracy_pct + 5.0 >= case.frontier_accuracy_pct
        {
            decision(
                "switch_reference",
                Some(case.rival_candidate.clone()),
                "The rival offers comparable behavior with a narrower stable interface surface.",
                4,
            )
        } else if case.interface_surface <= 2
            && case.runtime_risk <= 35
            && case.frontier_accuracy_pct >= case.rival_accuracy_pct
        {
            decision(
                "promote_reference",
                Some(case.frontier_candidate.clone()),
                "The frontier is accurate enough and small enough to expose in stable runtime.",
                4,
            )
        } else {
            decision(
                "hold_experimental",
                None,
                "Surface and risk signals do not yet justify promotion.",
                4,
            )
        })
    }
}
