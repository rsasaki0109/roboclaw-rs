use super::{
    allowed_skills, base_instruction, catalog_summary, full_schema, structured_recovery_guidance,
    PromptArtifact, PromptShapeCase, PromptShapeVariant,
};
use anyhow::Result;
use roboclaw_rs::skills::SkillCatalog;

#[derive(Debug, Default)]
pub struct MirroredConstraintsVariant;

impl PromptShapeVariant for MirroredConstraintsVariant {
    fn name(&self) -> &'static str {
        "mirrored_constraints"
    }

    fn style(&self) -> &'static str {
        "mirrored constraints"
    }

    fn philosophy(&self) -> &'static str {
        "Mirror the same recovery constraints in prompt text and schema so weaker providers still see the same bounded choice set."
    }

    fn source_path(&self) -> &'static str {
        "experiments/planner_prompt_shaping/mirrored_constraints.rs"
    }

    fn build(&self, case: &PromptShapeCase, catalog: &SkillCatalog) -> Result<PromptArtifact> {
        let allowed = allowed_skills(case, catalog);
        let recovery = structured_recovery_guidance(case, catalog)
            .map(|guidance| format!("\n\nRecovery guidance:\n{guidance}"))
            .unwrap_or_default();

        Ok(PromptArtifact {
            prompt: format!(
                "Available skills:\n{}\nAllowed skills for this turn: {}\n\nInstruction:\n{}{}\n\nDecision rules:\n- Prefer a recovery skill when matching_recovery_skills is not none.\n- Otherwise choose the skill whose steps best fit the user goal.\n- Return exactly one skill.",
                catalog_summary(catalog),
                allowed.join(", "),
                base_instruction(case),
                recovery
            ),
            schema: full_schema(&allowed, catalog, true),
        })
    }
}
