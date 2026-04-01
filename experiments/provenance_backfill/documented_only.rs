use super::{
    all_documented, base_resolved_cuts, classify_resolved_chain, documented_has_rollback,
    missing_documentation_count, ProvenanceBackfillCase, ProvenanceBackfillDecision,
    ProvenanceBackfillVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct DocumentedOnlyVariant;

impl ProvenanceBackfillVariant for DocumentedOnlyVariant {
    fn name(&self) -> &'static str {
        "documented_only"
    }

    fn style(&self) -> &'static str {
        "documented only"
    }

    fn philosophy(&self) -> &'static str {
        "Trust only explicit release-cut provenance and refuse to repair gaps from external artifacts."
    }

    fn source_path(&self) -> &'static str {
        "experiments/provenance_backfill/documented_only.rs"
    }

    fn decide(&self, case: &ProvenanceBackfillCase) -> Result<ProvenanceBackfillDecision> {
        if documented_has_rollback(case) {
            return Ok(super::decision(
                "backfill_rejected",
                None,
                "A documented rollback already breaks the provenance chain.",
                1,
            ));
        }

        if !all_documented(case) {
            return Ok(super::decision(
                "backfill_gap",
                None,
                format!(
                    "{} release cuts still require explicit documentation.",
                    missing_documentation_count(case)
                ),
                1,
            ));
        }

        Ok(classify_resolved_chain(
            case,
            &base_resolved_cuts(case),
            "Every release cut is already documented.",
            1,
        ))
    }
}
