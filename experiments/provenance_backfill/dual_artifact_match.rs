use super::{
    any_trusted_rollback_artifact, apply_artifact, artifact_conflict, base_resolved_cuts,
    classify_resolved_chain, documented_has_rollback, matching_dual_artifact, missing_tags,
    ProvenanceBackfillCase, ProvenanceBackfillDecision, ProvenanceBackfillVariant,
};
use anyhow::Result;

const DUAL_MIN_CONFIDENCE: f64 = 0.75;

#[derive(Debug, Default)]
pub struct DualArtifactMatchVariant;

impl ProvenanceBackfillVariant for DualArtifactMatchVariant {
    fn name(&self) -> &'static str {
        "dual_artifact_match"
    }

    fn style(&self) -> &'static str {
        "dual source match"
    }

    fn philosophy(&self) -> &'static str {
        "Require changelog and release notes to agree before a missing provenance step can be backfilled."
    }

    fn source_path(&self) -> &'static str {
        "experiments/provenance_backfill/dual_artifact_match.rs"
    }

    fn decide(&self, case: &ProvenanceBackfillCase) -> Result<ProvenanceBackfillDecision> {
        if documented_has_rollback(case) || any_trusted_rollback_artifact(case, DUAL_MIN_CONFIDENCE)
        {
            return Ok(super::decision(
                "backfill_rejected",
                None,
                "A trusted rollback note blocks artifact backfill.",
                4,
            ));
        }

        let mut resolved = base_resolved_cuts(case);
        for tag in missing_tags(case) {
            if artifact_conflict(case, &tag, DUAL_MIN_CONFIDENCE) {
                return Ok(super::decision(
                    "backfill_rejected",
                    None,
                    format!("Trusted artifacts disagree for {tag}."),
                    4,
                ));
            }

            let Some(artifact) = matching_dual_artifact(case, &tag, DUAL_MIN_CONFIDENCE) else {
                return Ok(super::decision(
                    "backfill_gap",
                    None,
                    format!("No matching changelog and release-note pair exists for {tag}."),
                    4,
                ));
            };
            apply_artifact(&mut resolved, &tag, &artifact);
        }

        Ok(classify_resolved_chain(
            case,
            &resolved,
            "Only dual-source artifact matches were accepted as provenance repairs.",
            4,
        ))
    }
}
