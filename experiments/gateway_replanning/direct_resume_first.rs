use super::{
    direct_resume, finish, has_replan_budget, replan_with_recovery, resume_original, stop_failed,
    GatewayCase, GatewayDecision, GatewayLoopVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct DirectResumeFirstVariant;

impl GatewayLoopVariant for DirectResumeFirstVariant {
    fn name(&self) -> &'static str {
        "direct_resume_first"
    }

    fn style(&self) -> &'static str {
        "checkpoint first"
    }

    fn philosophy(&self) -> &'static str {
        "Use checkpoint continuity before invoking another recovery/planning turn."
    }

    fn source_path(&self) -> &'static str {
        "experiments/gateway_replanning/direct_resume_first.rs"
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

        if case.resume_context_step.is_some() {
            return Ok(direct_resume(
                case.resume_context_step.clone(),
                "checkpoint exists, so resume original skill directly",
            ));
        }

        Ok(replan_with_recovery(
            None,
            "no checkpoint available, fall back to recovery replan",
        ))
    }
}
