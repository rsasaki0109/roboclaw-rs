use super::{
    allowed_skills, base_instruction, catalog_summary, full_schema, structured_recovery_guidance,
    PromptArtifact, PromptShapeCase, PromptShapeVariant,
};
use anyhow::Result;
use roboclaw_rs::skills::SkillCatalog;

#[derive(Debug, Default)]
pub struct PromptConstrainedVariant;

impl PromptShapeVariant for PromptConstrainedVariant {
    fn name(&self) -> &'static str {
        "prompt_constrained"
    }

    fn style(&self) -> &'static str {
        "prompt constrained"
    }

    fn philosophy(&self) -> &'static str {
        "Expose recovery constraints and allowed skills in prompt text while keeping the schema broad."
    }

    fn source_path(&self) -> &'static str {
        "experiments/planner_prompt_shaping/prompt_constrained.rs"
    }

    fn build(&self, case: &PromptShapeCase, catalog: &SkillCatalog) -> Result<PromptArtifact> {
        let allowed = allowed_skills(case, catalog);
        let recovery = structured_recovery_guidance(case, catalog)
            .map(|guidance| format!("\n\nRecovery guidance:\n{guidance}"))
            .unwrap_or_default();

        Ok(PromptArtifact {
            prompt: format!(
                "Available skills:\n{}\nAllowed skills for this turn: {}\n\nInstruction:\n{}{}\n\nChoose exactly one skill.",
                catalog_summary(catalog),
                allowed.join(", "),
                base_instruction(case),
                recovery
            ),
            schema: full_schema(&catalog.names(), catalog, false),
        })
    }
}
