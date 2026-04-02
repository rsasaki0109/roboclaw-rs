use super::{
    apply_budget_decision, budget_step, default_budget, hold_budget, latest_lag, outlier_count,
    percentile_lag, round_up_to_step, SurfaceLagCalibrationCase, SurfaceLagCalibrationDecision,
    SurfaceLagCalibrationVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct TraceBudgetVariant;

impl SurfaceLagCalibrationVariant for TraceBudgetVariant {
    fn name(&self) -> &'static str {
        "trace_budget"
    }

    fn style(&self) -> &'static str {
        "trace budget"
    }

    fn philosophy(&self) -> &'static str {
        "Calibrate each surface from a guarded percentile of observed lag, while holding the budget steady when the trace is too spiky to trust."
    }

    fn source_path(&self) -> &'static str {
        "experiments/surface_lag_budget_calibration/trace_budget.rs"
    }

    fn decide(&self, case: &SurfaceLagCalibrationCase) -> Result<SurfaceLagCalibrationDecision> {
        let floor = default_budget(&case.surface);
        let step = budget_step(&case.surface);
        let p80 = percentile_lag(case, 4, 5);
        let latest = latest_lag(case);
        let mut selected = floor.max(round_up_to_step(p80, step));

        if latest > p80 {
            selected = selected.max(floor.max(round_up_to_step(latest, step)));
        }

        if outlier_count(case) >= 2 {
            return Ok(hold_budget(
                case.current_budget_hours,
                format!(
                    "The observed lag trace is too spiky to recalibrate safely: outliers={} p80={p80} latest={latest}.",
                    outlier_count(case)
                ),
                6,
            ));
        }

        Ok(apply_budget_decision(
            case.current_budget_hours,
            selected,
            format!(
                "Use a guarded percentile trace budget for this surface: floor={floor} p80={p80} latest={latest} step={step}."
            ),
            6,
        ))
    }
}
