use super::{
    apply_budget_decision, budget_step, default_budget, latest_lag, round_up_to_step,
    SurfaceLagCalibrationCase, SurfaceLagCalibrationDecision, SurfaceLagCalibrationVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct LatestObservationVariant;

impl SurfaceLagCalibrationVariant for LatestObservationVariant {
    fn name(&self) -> &'static str {
        "latest_observation"
    }

    fn style(&self) -> &'static str {
        "latest sample"
    }

    fn philosophy(&self) -> &'static str {
        "Calibrate the budget entirely from the most recent observed publication lag."
    }

    fn source_path(&self) -> &'static str {
        "experiments/surface_lag_budget_calibration/latest_observation.rs"
    }

    fn decide(&self, case: &SurfaceLagCalibrationCase) -> Result<SurfaceLagCalibrationDecision> {
        let floor = default_budget(&case.surface);
        let selected = floor.max(round_up_to_step(
            latest_lag(case),
            budget_step(&case.surface),
        ));
        Ok(apply_budget_decision(
            case.current_budget_hours,
            selected,
            "Use the latest observed lag as the calibrated budget.",
            3,
        ))
    }
}
