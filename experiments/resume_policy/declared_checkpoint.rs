use super::{
    choose_resume_step, declared_checkpoint, failed_step, ResumeCase, ResumePolicyVariant,
};
use anyhow::Result;
use roboclaw_rs::skills::Skill;

#[derive(Debug, Default)]
pub struct DeclaredCheckpointVariant;

impl ResumePolicyVariant for DeclaredCheckpointVariant {
    fn name(&self) -> &'static str {
        "declared_checkpoint"
    }

    fn style(&self) -> &'static str {
        "metadata-driven"
    }

    fn philosophy(&self) -> &'static str {
        "Let skill YAML own resume semantics through explicit checkpoint metadata."
    }

    fn source_path(&self) -> &'static str {
        "experiments/resume_policy/declared_checkpoint.rs"
    }

    fn select_resume_step(
        &self,
        case: &ResumeCase,
        skill: &Skill,
    ) -> Result<super::ResumeDecision> {
        let step = declared_checkpoint(skill, failed_step(case))?;
        choose_resume_step(
            skill,
            &step,
            format!("resume using declared checkpoint for {}", failed_step(case)),
        )
    }
}
