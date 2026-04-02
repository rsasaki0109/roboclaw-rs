use super::{
    decision, leaf_checks, normalized_subset, ValidationCase, ValidationDecision, ValidationVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct NormalizedScalarsVariant;

impl ValidationVariant for NormalizedScalarsVariant {
    fn name(&self) -> &'static str {
        "normalized_scalars"
    }

    fn style(&self) -> &'static str {
        "scalar normalization"
    }

    fn philosophy(&self) -> &'static str {
        "Be strict on structure but normalize booleans, numbers, and strings before comparing leaves."
    }

    fn source_path(&self) -> &'static str {
        "experiments/tool_output_validation/normalized_scalars.rs"
    }

    fn validate(&self, case: &ValidationCase) -> Result<ValidationDecision> {
        Ok(decision(
            normalized_subset(&case.output, &case.expectation),
            leaf_checks(&case.expectation),
            "Normalize scalar leaves but keep object and array structure strict.",
        ))
    }
}
