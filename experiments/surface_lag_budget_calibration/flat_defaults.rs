use super::{
    apply_budget_decision, default_budget, SurfaceLagCalibrationCase,
    SurfaceLagCalibrationDecision, SurfaceLagCalibrationVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct FlatDefaultsVariant;

impl SurfaceLagCalibrationVariant for FlatDefaultsVariant {
    fn name(&self) -> &'static str {
        "flat_defaults"
    }

    fn style(&self) -> &'static str {
        "flat defaults"
    }

    fn philosophy(&self) -> &'static str {
        "Reuse a fixed default budget per surface and ignore observed lag history."
    }

    fn source_path(&self) -> &'static str {
        "experiments/surface_lag_budget_calibration/flat_defaults.rs"
    }

    fn decide(&self, case: &SurfaceLagCalibrationCase) -> Result<SurfaceLagCalibrationDecision> {
        let selected = default_budget(&case.surface);
        Ok(apply_budget_decision(
            case.current_budget_hours,
            selected,
            "Use the default surface budget without recalibration.",
            1,
        ))
    }
}
