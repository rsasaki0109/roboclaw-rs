use super::{
    direct_resume, finish, has_replan_budget, replan_generic, replan_with_recovery,
    resume_original, stop_failed, GatewayCase, GatewayDecision, GatewayLoopVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct ResumeAwareReplanVariant;

impl GatewayLoopVariant for ResumeAwareReplanVariant {
    fn name(&self) -> &'static str {
        "resume_aware_replan"
    }

    fn style(&self) -> &'static str {
        "hybrid loop"
    }

    fn philosophy(&self) -> &'static str {
        "Use recovery skills when they exist, otherwise resume directly from checkpointed state."
    }

    fn source_path(&self) -> &'static str {
        "experiments/gateway_replanning/resume_aware_replan.rs"
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
                "recovery candidates exist, so ask planner for recovery skill",
            ));
        }

        if case.resume_context_step.is_some() {
            return Ok(direct_resume(
                case.resume_context_step.clone(),
                "no recovery candidate exists, so resume directly from checkpoint",
            ));
        }

        Ok(replan_generic(
            None,
            "no recovery candidate or checkpoint; generic replanning is the only option",
        ))
    }
}
