use super::{
    decision, latest_cut, PromotionProvenanceCase, PromotionProvenanceDecision,
    PromotionProvenanceVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct LatestCutOnlyVariant;

impl PromotionProvenanceVariant for LatestCutOnlyVariant {
    fn name(&self) -> &'static str {
        "latest_cut_only"
    }

    fn style(&self) -> &'static str {
        "latest cut"
    }

    fn philosophy(&self) -> &'static str {
        "Judge promotion provenance only from the latest release cut."
    }

    fn source_path(&self) -> &'static str {
        "experiments/promotion_provenance/latest_cut_only.rs"
    }

    fn decide(&self, case: &PromotionProvenanceCase) -> Result<PromotionProvenanceDecision> {
        let Some(cut) = latest_cut(case) else {
            return Ok(decision(
                "provenance_gap",
                None,
                "No release cuts are available.",
                1,
            ));
        };

        Ok(match cut.decision_kind.as_str() {
            "rollback_reference" => decision(
                "provenance_broken",
                None,
                "Latest release cut rolled the reference back.",
                1,
            ),
            "replace_reference" if cut.documented => decision(
                "provenance_superseded",
                cut.reference.clone(),
                "Latest cut documents a replacement lineage.",
                1,
            ),
            _ if cut.documented => decision(
                "provenance_confirmed",
                cut.reference.clone(),
                "Latest cut looks documented and healthy.",
                1,
            ),
            _ => decision("provenance_gap", None, "Latest cut is undocumented.", 1),
        })
    }
}
