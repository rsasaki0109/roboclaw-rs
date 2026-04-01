use super::{choose_resume_step, failed_step, ResumeCase, ResumePolicyVariant};
use anyhow::Result;
use roboclaw_rs::skills::Skill;

#[derive(Debug, Default)]
pub struct ResumeFailedStepVariant;

impl ResumePolicyVariant for ResumeFailedStepVariant {
    fn name(&self) -> &'static str {
        "resume_failed_step"
    }

    fn style(&self) -> &'static str {
        "direct retry"
    }

    fn philosophy(&self) -> &'static str {
        "Retry only the failed step and assume preconditions still hold."
    }

    fn source_path(&self) -> &'static str {
        "experiments/resume_policy/resume_failed_step.rs"
    }

    fn select_resume_step(
        &self,
        case: &ResumeCase,
        skill: &Skill,
    ) -> Result<super::ResumeDecision> {
        choose_resume_step(skill, failed_step(case), "retry the failed step directly")
    }
}
