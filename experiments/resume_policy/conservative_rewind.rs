use super::{
    choose_resume_step, declared_checkpoint, failed_step, first_step, step_index, ResumeCase,
    ResumePolicyVariant,
};
use anyhow::Result;
use roboclaw_rs::skills::Skill;

#[derive(Debug, Default)]
pub struct ConservativeRewindVariant;

impl ResumePolicyVariant for ConservativeRewindVariant {
    fn name(&self) -> &'static str {
        "conservative_rewind"
    }

    fn style(&self) -> &'static str {
        "safety-first rewind"
    }

    fn philosophy(&self) -> &'static str {
        "Rewind to a safer observation boundary when actuator failures might invalidate local state."
    }

    fn source_path(&self) -> &'static str {
        "experiments/resume_policy/conservative_rewind.rs"
    }

    fn select_resume_step(
        &self,
        case: &ResumeCase,
        skill: &Skill,
    ) -> Result<super::ResumeDecision> {
        let failed_step_name = failed_step(case);
        let failed_index = step_index(skill, failed_step_name)?;
        let failed_tool = skill
            .steps
            .get(failed_index)
            .map(|step| step.tool.as_str())
            .unwrap_or("unknown");

        if failed_tool == "sensor" {
            return choose_resume_step(
                skill,
                failed_step_name,
                "sensor failure keeps local physical state intact, retry failed step",
            );
        }

        if let Some(sensor_step) = skill.steps[..failed_index]
            .iter()
            .rev()
            .find(|step| step.tool == "sensor")
            .map(|step| step.name.clone())
        {
            return choose_resume_step(
                skill,
                &sensor_step,
                format!("rewind to previous sensor boundary '{sensor_step}'"),
            );
        }

        let checkpoint = declared_checkpoint(skill, failed_step_name)?;
        if checkpoint != failed_step_name {
            return choose_resume_step(
                skill,
                &checkpoint,
                format!("fallback to declared checkpoint '{checkpoint}'"),
            );
        }

        let first = first_step(skill)?;
        choose_resume_step(
            skill,
            &first,
            format!("no safe checkpoint found, restart at '{first}'"),
        )
    }
}
