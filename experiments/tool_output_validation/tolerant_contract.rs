use super::{
    decision, leaf_checks, tolerant_subset, ValidationCase, ValidationDecision, ValidationVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct TolerantContractVariant;

impl ValidationVariant for TolerantContractVariant {
    fn name(&self) -> &'static str {
        "tolerant_contract"
    }

    fn style(&self) -> &'static str {
        "tolerant contract"
    }

    fn philosophy(&self) -> &'static str {
        "Treat expectations as semantic contracts: normalize scalar leaves and allow array membership matches."
    }

    fn source_path(&self) -> &'static str {
        "experiments/tool_output_validation/tolerant_contract.rs"
    }

    fn validate(&self, case: &ValidationCase) -> Result<ValidationDecision> {
        Ok(decision(
            tolerant_subset(&case.output, &case.expectation),
            leaf_checks(&case.expectation),
            "Combine scalar normalization with unordered array membership to validate semantic contracts.",
        ))
    }
}
