use super::{
    base_instruction, catalog_summary, full_schema, PromptArtifact, PromptShapeCase,
    PromptShapeVariant,
};
use anyhow::Result;
use roboclaw_rs::skills::SkillCatalog;

#[derive(Debug, Default)]
pub struct CatalogFreeformVariant;

impl PromptShapeVariant for CatalogFreeformVariant {
    fn name(&self) -> &'static str {
        "catalog_freeform"
    }

    fn style(&self) -> &'static str {
        "catalog freeform"
    }

    fn philosophy(&self) -> &'static str {
        "Rely on catalog exposure and user goal wording, without explicit recovery constraints."
    }

    fn source_path(&self) -> &'static str {
        "experiments/planner_prompt_shaping/catalog_freeform.rs"
    }

    fn build(&self, case: &PromptShapeCase, catalog: &SkillCatalog) -> Result<PromptArtifact> {
        Ok(PromptArtifact {
            prompt: format!(
                "Available skills:\n{}\n\nInstruction:\n{}\n\nChoose one skill from the catalog.",
                catalog_summary(catalog),
                base_instruction(case)
            ),
            schema: full_schema(&catalog.names(), catalog, false),
        })
    }
}
