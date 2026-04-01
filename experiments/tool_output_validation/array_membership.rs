use super::{
    array_membership_subset, decision, leaf_checks, ValidationCase, ValidationDecision,
    ValidationVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct ArrayMembershipVariant;

impl ValidationVariant for ArrayMembershipVariant {
    fn name(&self) -> &'static str {
        "array_membership"
    }

    fn style(&self) -> &'static str {
        "array membership"
    }

    fn philosophy(&self) -> &'static str {
        "Treat arrays as unordered evidence bags while keeping scalar comparisons exact."
    }

    fn source_path(&self) -> &'static str {
        "experiments/tool_output_validation/array_membership.rs"
    }

    fn validate(&self, case: &ValidationCase) -> Result<ValidationDecision> {
        Ok(decision(
            array_membership_subset(&case.output, &case.expectation),
            leaf_checks(&case.expectation),
            "Allow expected array members to appear anywhere inside larger output arrays.",
        ))
    }
}
