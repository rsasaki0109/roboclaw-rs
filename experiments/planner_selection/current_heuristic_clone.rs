use super::{
    choose_named_skill, fallback_to_first, is_replan_instruction, normalize, recovery_candidates,
    skill_token_overlap_score, tokenize_keywords, PlannerVariant, PlanningCase, VariantDecision,
};
use anyhow::Result;
use roboclaw_rs::skills::SkillCatalog;

#[derive(Debug, Default)]
pub struct CurrentHeuristicCloneVariant;

impl PlannerVariant for CurrentHeuristicCloneVariant {
    fn name(&self) -> &'static str {
        "current_heuristic_clone"
    }

    fn style(&self) -> &'static str {
        "current baseline"
    }

    fn philosophy(&self) -> &'static str {
        "Mirror the current mock-planner behavior as a baseline before experimenting."
    }

    fn source_path(&self) -> &'static str {
        "experiments/planner_selection/current_heuristic_clone.rs"
    }

    fn plan(&self, case: &PlanningCase, catalog: &SkillCatalog) -> Result<VariantDecision> {
        let normalized = normalize(&case.instruction);

        if is_replan_instruction(&normalized) {
            let recovery = recovery_candidates(catalog, &case.instruction);
            if let Some(skill) = recovery.first() {
                return Ok(choose_named_skill(
                    catalog,
                    skill,
                    format!("baseline recovery heuristic matched {}", skill),
                ));
            }
        }

        if normalized.contains("pick") && normalized.contains("place") {
            return Ok(choose_named_skill(
                catalog,
                "pick_and_place",
                "baseline keyword match on pick/place",
            ));
        }

        for skill in catalog.values() {
            let skill_name = normalize(&skill.name);
            let skill_description = normalize(&skill.description);
            if normalized.contains(&skill_name) || normalized.contains(&skill_description) {
                return Ok(choose_named_skill(
                    catalog,
                    &skill.name,
                    format!("baseline contains match on '{}'", skill.name),
                ));
            }
        }

        let instruction_tokens = tokenize_keywords(&normalized);
        let best_match = catalog
            .values()
            .filter_map(|skill| {
                let overlap =
                    skill_token_overlap_score(&instruction_tokens, &skill.name, &skill.description);
                if overlap > 0 {
                    Some((overlap, skill.name.as_str()))
                } else {
                    None
                }
            })
            .max_by_key(|(overlap, _)| *overlap);

        if let Some((overlap, skill_name)) = best_match {
            return Ok(choose_named_skill(
                catalog,
                skill_name,
                format!("baseline token overlap score={overlap}"),
            ));
        }

        Ok(fallback_to_first(
            catalog,
            "baseline fallback to first loaded skill",
        ))
    }
}
