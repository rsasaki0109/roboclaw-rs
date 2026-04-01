use super::{
    all_documented, decision, has_replace, has_rollback, origin_reference, PromotionProvenanceCase,
    PromotionProvenanceDecision, PromotionProvenanceVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct DocumentedChainVariant;

impl PromotionProvenanceVariant for DocumentedChainVariant {
    fn name(&self) -> &'static str {
        "documented_chain"
    }

    fn style(&self) -> &'static str {
        "documented chain"
    }

    fn philosophy(&self) -> &'static str {
        "Require a fully documented release chain before trusting provenance."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_provenance/documented_chain.rs"
    }

    fn decide(&self, case: &PromotionProvenanceCase) -> Result<PromotionProvenanceDecision> {
        if has_rollback(case) {
            return Ok(decision(
                "provenance_broken",
                None,
                "A rollback broke the release chain.",
                3,
            ));
        }

        if !all_documented(case) {
            return Ok(decision(
                "provenance_gap",
                None,
                "At least one release cut omitted provenance.",
                3,
            ));
        }

        if has_replace(case) && origin_reference(case).as_deref() != Some(&case.current_reference) {
            return Ok(decision(
                "provenance_superseded",
                Some(case.current_reference.clone()),
                "The documented chain shows a replacement from the original promoted surface.",
                3,
            ));
        }

        Ok(decision(
            "provenance_confirmed",
            Some(case.current_reference.clone()),
            "Every release cut is documented and no rollback broke the chain.",
            3,
        ))
    }
}
