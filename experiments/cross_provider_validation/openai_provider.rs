use super::{
    evaluate_with_provider, remote_validation_enabled, ProviderAvailability, ProviderCase,
    ProviderValidationDecision, ProviderValidationVariant,
};
use anyhow::Result;
use roboclaw_rs::agent::{planner_for_provider, LlmProvider};
use roboclaw_rs::skills::SkillCatalog;
use std::path::Path;

#[derive(Debug, Default)]
pub struct OpenAiProviderVariant;

impl ProviderValidationVariant for OpenAiProviderVariant {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn style(&self) -> &'static str {
        "remote responses api"
    }

    fn philosophy(&self) -> &'static str {
        "Validate the frontier against the live OpenAI planner only when remote validation is explicitly enabled."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_provider_validation/openai_provider.rs"
    }

    fn provider(&self) -> LlmProvider {
        LlmProvider::OpenAi
    }

    fn availability(&self, prompt_path: &Path) -> ProviderAvailability {
        if !remote_validation_enabled() {
            return ProviderAvailability::Disabled;
        }

        planner_for_provider(prompt_path, self.provider())
            .map(|_| ProviderAvailability::Available)
            .unwrap_or(ProviderAvailability::Unavailable)
    }

    fn evaluate(
        &self,
        case: &ProviderCase,
        prompt_path: &Path,
        catalog: &SkillCatalog,
    ) -> Result<ProviderValidationDecision> {
        evaluate_with_provider(self.provider(), prompt_path, catalog, &case.instruction)
    }
}
