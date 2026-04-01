use super::{
    choose_named_skill, fallback_to_first, is_replan_instruction, normalize, PlannerVariant,
    PlanningCase, VariantDecision,
};
use anyhow::Result;
use roboclaw_rs::skills::SkillCatalog;

#[derive(Debug, Default)]
pub struct KeywordRulesVariant;

impl PlannerVariant for KeywordRulesVariant {
    fn name(&self) -> &'static str {
        "keyword_rules"
    }

    fn style(&self) -> &'static str {
        "imperative rules"
    }

    fn philosophy(&self) -> &'static str {
        "Prefer short, explicit branches even if skill names leak into the implementation."
    }

    fn source_path(&self) -> &'static str {
        "experiments/planner_selection/keyword_rules.rs"
    }

    fn plan(&self, case: &PlanningCase, catalog: &SkillCatalog) -> Result<VariantDecision> {
        let normalized = normalize(&case.instruction);

        if is_replan_instruction(&normalized) {
            if normalized.contains("failed step: grasp")
                || normalized.contains("failed tool: motor_control")
            {
                return Ok(choose_named_skill(
                    catalog,
                    "recover_grasp",
                    "explicit recovery rule matched grasp or motor_control",
                ));
            }

            if normalized.contains("failed step: detect_object")
                || normalized.contains("failed tool: sensor")
            {
                return Ok(choose_named_skill(
                    catalog,
                    "recover_observation",
                    "explicit recovery rule matched detect_object or sensor",
                ));
            }
        }

        if normalized.contains("pick") && normalized.contains("place") {
            return Ok(choose_named_skill(
                catalog,
                "pick_and_place",
                "explicit task rule matched pick/place",
            ));
        }

        if normalized.contains("wave")
            || normalized.contains("greet")
            || normalized.contains("acknowledge")
        {
            return Ok(choose_named_skill(
                catalog,
                "wave_arm",
                "explicit task rule matched wave/greet/acknowledge",
            ));
        }

        Ok(fallback_to_first(
            catalog,
            "explicit rules exhausted; fallback to first skill",
        ))
    }
}
