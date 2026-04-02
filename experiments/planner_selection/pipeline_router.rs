use super::{
    choose_named_skill, fallback_to_first, normalize, recovery_candidates,
    skill_token_overlap_score, tokenize_keywords, PlannerVariant, PlanningCase, VariantDecision,
};
use anyhow::Result;
use roboclaw_rs::skills::SkillCatalog;

#[derive(Debug, Default)]
pub struct PipelineRouterVariant;

impl PlannerVariant for PipelineRouterVariant {
    fn name(&self) -> &'static str {
        "pipeline_router"
    }

    fn style(&self) -> &'static str {
        "pipeline router"
    }

    fn philosophy(&self) -> &'static str {
        "Route obvious cases in ordered stages and keep fallback logic as a final stage."
    }

    fn source_path(&self) -> &'static str {
        "experiments/planner_selection/pipeline_router.rs"
    }

    fn plan(&self, case: &PlanningCase, catalog: &SkillCatalog) -> Result<VariantDecision> {
        let normalized = normalize(&case.instruction);

        if let Some(decision) = stage_recovery_router(catalog, &case.instruction) {
            return Ok(decision);
        }

        if let Some(decision) = stage_task_router(catalog, &normalized) {
            return Ok(decision);
        }

        if let Some(decision) = stage_description_router(catalog, &normalized) {
            return Ok(decision);
        }

        Ok(fallback_to_first(
            catalog,
            "pipeline exhausted all stages; fallback to first skill",
        ))
    }
}

fn stage_recovery_router(catalog: &SkillCatalog, instruction: &str) -> Option<VariantDecision> {
    let recovery = recovery_candidates(catalog, instruction);
    if recovery.len() == 1 {
        let selected = recovery.first()?;
        return Some(choose_named_skill(
            catalog,
            selected,
            format!("pipeline stage_recovery_router selected {selected}"),
        ));
    }

    None
}

fn stage_task_router(catalog: &SkillCatalog, normalized: &str) -> Option<VariantDecision> {
    if (normalized.contains("pick") && normalized.contains("place"))
        || (normalized.contains("grasp") && normalized.contains("bin"))
        || (normalized.contains("transport") && normalized.contains("bin"))
    {
        return Some(choose_named_skill(
            catalog,
            "pick_and_place",
            "pipeline stage_task_router matched transport/grasp/bin pattern",
        ));
    }

    if normalized.contains("wave")
        || normalized.contains("greet")
        || normalized.contains("acknowledge")
        || normalized.contains("salute")
        || (normalized.contains("gesture") && normalized.contains("operator"))
    {
        return Some(choose_named_skill(
            catalog,
            "wave_arm",
            "pipeline stage_task_router matched gesture pattern",
        ));
    }

    None
}

fn stage_description_router(catalog: &SkillCatalog, normalized: &str) -> Option<VariantDecision> {
    let instruction_tokens = tokenize_keywords(normalized);
    let best = catalog
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
        .max_by_key(|(overlap, _)| *overlap)?;

    Some(choose_named_skill(
        catalog,
        best.1,
        format!("pipeline stage_description_router overlap={}", best.0),
    ))
}
