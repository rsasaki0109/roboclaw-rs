use super::{
    allowed_skills, base_instruction, full_schema, structured_recovery_guidance, PromptArtifact,
    PromptShapeCase, PromptShapeVariant,
};
use anyhow::Result;
use roboclaw_rs::skills::SkillCatalog;

#[derive(Debug, Default)]
pub struct SchemaConstrainedVariant;

impl PromptShapeVariant for SchemaConstrainedVariant {
    fn name(&self) -> &'static str {
        "schema_constrained"
    }

    fn style(&self) -> &'static str {
        "schema constrained"
    }

    fn philosophy(&self) -> &'static str {
        "Keep prompt text compact and rely on the schema enum and schema description to carry the recovery constraints."
    }

    fn source_path(&self) -> &'static str {
        "experiments/planner_prompt_shaping/schema_constrained.rs"
    }

    fn build(&self, case: &PromptShapeCase, catalog: &SkillCatalog) -> Result<PromptArtifact> {
        let recovery = structured_recovery_guidance(case, catalog)
            .map(|guidance| format!("\n\nRecovery context:\n{guidance}"))
            .unwrap_or_default();
        let allowed = allowed_skills(case, catalog);

        Ok(PromptArtifact {
            prompt: format!(
                "Instruction:\n{}{}\n\nReturn compact JSON.",
                base_instruction(case),
                recovery
            ),
            schema: full_schema(&allowed, catalog, true),
        })
    }
}
