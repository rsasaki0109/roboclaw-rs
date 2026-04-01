use super::{
    decision, has_rollback, majority_reference, reference_count, PromotionProvenanceCase,
    PromotionProvenanceDecision, PromotionProvenanceVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct ReleaseMajorityVariant;

impl PromotionProvenanceVariant for ReleaseMajorityVariant {
    fn name(&self) -> &'static str {
        "release_majority"
    }

    fn style(&self) -> &'static str {
        "release majority"
    }

    fn philosophy(&self) -> &'static str {
        "Trust whichever reference dominates the release history, even if some cuts are missing detail."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_provenance/release_majority.rs"
    }

    fn decide(&self, case: &PromotionProvenanceCase) -> Result<PromotionProvenanceDecision> {
        if has_rollback(case) {
            return Ok(decision(
                "provenance_broken",
                None,
                "Rollback outweighs release-history majority.",
                2,
            ));
        }

        let Some(reference) = majority_reference(case) else {
            return Ok(decision(
                "provenance_gap",
                None,
                "No reference dominates the release history.",
                2,
            ));
        };
        let count = reference_count(case, &reference);

        Ok(if reference == case.current_reference {
            decision(
                "provenance_confirmed",
                Some(reference),
                format!(
                    "The current reference appears in the majority of release cuts count={count}."
                ),
                2,
            )
        } else {
            decision(
                "provenance_superseded",
                Some(reference),
                format!("Another reference dominates the release history count={count}."),
                2,
            )
        })
    }
}
