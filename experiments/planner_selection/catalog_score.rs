use super::{
    fallback_to_first, normalize, recovery_candidates, skill_token_overlap_score,
    tokenize_keywords, PlannerVariant, PlanningCase, VariantDecision,
};
use anyhow::Result;
use roboclaw_rs::skills::SkillCatalog;

#[derive(Debug, Default)]
pub struct CatalogScoreVariant;

impl PlannerVariant for CatalogScoreVariant {
    fn name(&self) -> &'static str {
        "catalog_score"
    }

    fn style(&self) -> &'static str {
        "scored ranking"
    }

    fn philosophy(&self) -> &'static str {
        "Score every skill against the same inputs and let catalog metadata drive the decision."
    }

    fn source_path(&self) -> &'static str {
        "experiments/planner_selection/catalog_score.rs"
    }

    fn plan(&self, case: &PlanningCase, catalog: &SkillCatalog) -> Result<VariantDecision> {
        let normalized = normalize(&case.instruction);
        let instruction_tokens = tokenize_keywords(&normalized);
        let recovery = recovery_candidates(catalog, &case.instruction);

        let best_match = catalog
            .values()
            .map(|skill| {
                let token_overlap =
                    skill_token_overlap_score(&instruction_tokens, &skill.name, &skill.description);
                let step_overlap = skill
                    .steps
                    .iter()
                    .map(|step| step.name.as_str())
                    .filter(|step_name| normalized.contains(&normalize(step_name)))
                    .count();
                let recovery_bonus = if recovery.contains(&skill.name) {
                    50
                } else {
                    0
                };
                let resume_bonus = if skill.resume_original_instruction {
                    3
                } else {
                    0
                };
                let score = token_overlap * 10 + step_overlap * 5 + recovery_bonus + resume_bonus;
                (score, token_overlap, step_overlap, skill.name.as_str())
            })
            .max_by_key(|(score, token_overlap, step_overlap, _)| {
                (*score, *token_overlap, *step_overlap)
            });

        if let Some((score, token_overlap, step_overlap, skill_name)) = best_match {
            if score > 0 {
                return Ok(VariantDecision {
                    selected_skill: skill_name.to_string(),
                    rationale: format!(
                        "catalog score selected {skill_name} score={score} token_overlap={token_overlap} step_overlap={step_overlap} recovery_bonus={}",
                        if recovery.contains(&skill_name.to_string()) { 50 } else { 0 }
                    ),
                });
            }
        }

        Ok(fallback_to_first(
            catalog,
            "catalog score had no positive matches; fallback to first skill",
        ))
    }
}
