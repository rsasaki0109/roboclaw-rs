use super::{choose_resume_step, first_step, ResumeCase, ResumePolicyVariant};
use anyhow::Result;
use roboclaw_rs::skills::Skill;

#[derive(Debug, Default)]
pub struct RestartSkillVariant;

impl ResumePolicyVariant for RestartSkillVariant {
    fn name(&self) -> &'static str {
        "restart_skill"
    }

    fn style(&self) -> &'static str {
        "always restart"
    }

    fn philosophy(&self) -> &'static str {
        "Favor simplicity and determinism over efficiency."
    }

    fn source_path(&self) -> &'static str {
        "experiments/resume_policy/restart_skill.rs"
    }

    fn select_resume_step(
        &self,
        _case: &ResumeCase,
        skill: &Skill,
    ) -> Result<super::ResumeDecision> {
        let step = first_step(skill)?;
        choose_resume_step(skill, &step, "always restart from the first step")
    }
}
