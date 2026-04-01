use super::{
    any_trusted_rollback_artifact, apply_artifact, base_resolved_cuts, classify_resolved_chain,
    documented_has_rollback, missing_tags, source_artifact, ProvenanceBackfillCase,
    ProvenanceBackfillDecision, ProvenanceBackfillVariant,
};
use anyhow::Result;

const CHANGELOG_MIN_CONFIDENCE: f64 = 0.50;

#[derive(Debug, Default)]
pub struct ChangelogFirstVariant;

impl ProvenanceBackfillVariant for ChangelogFirstVariant {
    fn name(&self) -> &'static str {
        "changelog_first"
    }

    fn style(&self) -> &'static str {
        "changelog first"
    }

    fn philosophy(&self) -> &'static str {
        "Use changelog entries as the primary repair surface for missing provenance and ignore weaker cross-artifact consistency checks."
    }

    fn source_path(&self) -> &'static str {
        "experiments/provenance_backfill/changelog_first.rs"
    }

    fn decide(&self, case: &ProvenanceBackfillCase) -> Result<ProvenanceBackfillDecision> {
        if documented_has_rollback(case)
            || any_trusted_rollback_artifact(case, CHANGELOG_MIN_CONFIDENCE)
        {
            return Ok(super::decision(
                "backfill_rejected",
                None,
                "A rollback signal is present in the changelog-visible evidence.",
                3,
            ));
        }

        let mut resolved = base_resolved_cuts(case);
        for tag in missing_tags(case) {
            let Some(artifact) = source_artifact(case, &tag, "changelog", CHANGELOG_MIN_CONFIDENCE)
            else {
                return Ok(super::decision(
                    "backfill_gap",
                    None,
                    format!("No changelog entry is available for {tag}."),
                    3,
                ));
            };
            apply_artifact(&mut resolved, &tag, &artifact);
        }

        Ok(classify_resolved_chain(
            case,
            &resolved,
            "Changelog entries were used to repair every missing release cut.",
            3,
        ))
    }
}
