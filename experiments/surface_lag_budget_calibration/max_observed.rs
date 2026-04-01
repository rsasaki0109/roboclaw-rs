use super::{
    apply_budget_decision, budget_step, default_budget, max_lag, round_up_to_step,
    SurfaceLagCalibrationCase, SurfaceLagCalibrationDecision, SurfaceLagCalibrationVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct MaxObservedVariant;

impl SurfaceLagCalibrationVariant for MaxObservedVariant {
    fn name(&self) -> &'static str {
        "max_observed"
    }

    fn style(&self) -> &'static str {
        "max observed"
    }

    fn philosophy(&self) -> &'static str {
        "Treat the worst observed lag as the calibration target for future budgets."
    }

    fn source_path(&self) -> &'static str {
        "experiments/surface_lag_budget_calibration/max_observed.rs"
    }

    fn decide(&self, case: &SurfaceLagCalibrationCase) -> Result<SurfaceLagCalibrationDecision> {
        let floor = default_budget(&case.surface);
        let selected = floor.max(round_up_to_step(max_lag(case), budget_step(&case.surface)));
        Ok(apply_budget_decision(
            case.current_budget_hours,
            selected,
            "Use the worst observed lag as the next budget.",
            4,
        ))
    }
}
