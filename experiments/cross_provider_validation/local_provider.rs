use super::{
    evaluate_with_provider, ProviderAvailability, ProviderCase, ProviderValidationDecision,
    ProviderValidationVariant,
};
use anyhow::Result;
use roboclaw_rs::agent::{planner_for_provider, LlmProvider};
use roboclaw_rs::skills::SkillCatalog;
use std::path::Path;

#[derive(Debug, Default)]
pub struct LocalProviderVariant;

impl ProviderValidationVariant for LocalProviderVariant {
    fn name(&self) -> &'static str {
        "local"
    }

    fn style(&self) -> &'static str {
        "live ollama"
    }

    fn philosophy(&self) -> &'static str {
        "Use a locally discovered generative model when available, otherwise mark the provider unavailable."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_provider_validation/local_provider.rs"
    }

    fn provider(&self) -> LlmProvider {
        LlmProvider::Local
    }

    fn availability(&self, prompt_path: &Path) -> ProviderAvailability {
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
