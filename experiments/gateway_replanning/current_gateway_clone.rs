use super::{
    finish, has_replan_budget, replan_generic, replan_with_recovery, resume_original, stop_failed,
    GatewayCase, GatewayDecision, GatewayLoopVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct CurrentGatewayCloneVariant;

impl GatewayLoopVariant for CurrentGatewayCloneVariant {
    fn name(&self) -> &'static str {
        "current_gateway_clone"
    }

    fn style(&self) -> &'static str {
        "current baseline"
    }

    fn philosophy(&self) -> &'static str {
        "Mirror the current gateway control flow as the baseline loop policy."
    }

    fn source_path(&self) -> &'static str {
        "experiments/gateway_replanning/current_gateway_clone.rs"
    }

    fn decide(&self, case: &GatewayCase) -> Result<GatewayDecision> {
        if case.completed && case.resume_original_instruction {
            return Ok(resume_original(
                case.resume_context_step.clone(),
                "completed recovery skill and resumed original instruction",
            ));
        }

        if case.completed {
            return Ok(finish("completed task or non-recovery skill"));
        }

        if !has_replan_budget(case) {
            return Ok(stop_failed("replan budget exhausted"));
        }

        if !case.recovery_candidates.is_empty() {
            return Ok(replan_with_recovery(
                case.resume_context_step.clone(),
                "replan with recovery candidates and preserve checkpoint hint",
            ));
        }

        Ok(replan_generic(
            case.resume_context_step.clone(),
            "replan even without explicit recovery candidates",
        ))
    }
}
