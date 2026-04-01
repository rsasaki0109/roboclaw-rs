use super::{
    decision, leaf_checks, strict_subset, ValidationCase, ValidationDecision, ValidationVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct StrictSubsetCloneVariant;

impl ValidationVariant for StrictSubsetCloneVariant {
    fn name(&self) -> &'static str {
        "strict_subset_clone"
    }

    fn style(&self) -> &'static str {
        "exact subset"
    }

    fn philosophy(&self) -> &'static str {
        "Keep validation identical to the current runtime: exact scalar equality and exact array shape."
    }

    fn source_path(&self) -> &'static str {
        "experiments/tool_output_validation/strict_subset_clone.rs"
    }

    fn validate(&self, case: &ValidationCase) -> Result<ValidationDecision> {
        Ok(decision(
            strict_subset(&case.output, &case.expectation),
            leaf_checks(&case.expectation),
            "Exact subset match with positional arrays and no scalar coercion.",
        ))
    }
}
