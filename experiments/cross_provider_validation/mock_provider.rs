use super::{
    evaluate_with_provider, ProviderAvailability, ProviderCase, ProviderValidationDecision,
    ProviderValidationVariant,
};
use anyhow::Result;
use roboclaw_rs::agent::LlmProvider;
use roboclaw_rs::skills::SkillCatalog;
use std::path::Path;

#[derive(Debug, Default)]
pub struct MockProviderVariant;

impl ProviderValidationVariant for MockProviderVariant {
    fn name(&self) -> &'static str {
        "mock"
    }

    fn style(&self) -> &'static str {
        "deterministic fallback"
    }

    fn philosophy(&self) -> &'static str {
        "Always available deterministic baseline for offline validation."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_provider_validation/mock_provider.rs"
    }

    fn provider(&self) -> LlmProvider {
        LlmProvider::Mock
    }

    fn availability(&self, _prompt_path: &Path) -> ProviderAvailability {
        ProviderAvailability::Available
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
