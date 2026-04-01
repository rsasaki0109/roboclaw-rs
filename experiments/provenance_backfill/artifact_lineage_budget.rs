use super::{
    any_trusted_rollback_artifact, apply_artifact, artifact_conflict, base_resolved_cuts,
    best_artifact, classify_resolved_chain, documented_has_rollback, matching_dual_artifact,
    missing_documentation_count, missing_tags, trusted_rollback_artifact, ProvenanceBackfillCase,
    ProvenanceBackfillDecision, ProvenanceBackfillVariant,
};
use anyhow::Result;

const DUAL_SOURCE_CONFIDENCE: f64 = 0.70;
const CONFLICT_CONFIDENCE: f64 = 0.75;
const SINGLE_SOURCE_CONFIDENCE: f64 = 0.90;
const ROLLBACK_CONFIDENCE: f64 = 0.80;

#[derive(Debug, Default)]
pub struct ArtifactLineageBudgetVariant;

impl ProvenanceBackfillVariant for ArtifactLineageBudgetVariant {
    fn name(&self) -> &'static str {
        "artifact_lineage_budget"
    }

    fn style(&self) -> &'static str {
        "artifact budget"
    }

    fn philosophy(&self) -> &'static str {
        "Balance source agreement, artifact confidence, rollback pressure, and remaining gaps before trusting provenance backfill."
    }

    fn source_path(&self) -> &'static str {
        "experiments/provenance_backfill/artifact_lineage_budget.rs"
    }

    fn decide(&self, case: &ProvenanceBackfillCase) -> Result<ProvenanceBackfillDecision> {
        if documented_has_rollback(case) || any_trusted_rollback_artifact(case, ROLLBACK_CONFIDENCE)
        {
            return Ok(super::decision(
                "backfill_rejected",
                None,
                "Rollback evidence is too strong to allow artifact backfill.",
                6,
            ));
        }

        let mut resolved = base_resolved_cuts(case);
        let mut unresolved_tags = Vec::new();

        for tag in missing_tags(case) {
            if trusted_rollback_artifact(case, &tag, ROLLBACK_CONFIDENCE).is_some() {
                return Ok(super::decision(
                    "backfill_rejected",
                    None,
                    format!("Trusted rollback evidence exists for {tag}."),
                    6,
                ));
            }

            if artifact_conflict(case, &tag, CONFLICT_CONFIDENCE) {
                return Ok(super::decision(
                    "backfill_rejected",
                    None,
                    format!("High-confidence artifacts conflict for {tag}."),
                    6,
                ));
            }

            if let Some(artifact) = matching_dual_artifact(case, &tag, DUAL_SOURCE_CONFIDENCE) {
                apply_artifact(&mut resolved, &tag, &artifact);
                continue;
            }

            if let Some(artifact) = best_artifact(case, &tag, SINGLE_SOURCE_CONFIDENCE) {
                apply_artifact(&mut resolved, &tag, &artifact);
                continue;
            }

            unresolved_tags.push(tag);
        }

        if !unresolved_tags.is_empty() {
            return Ok(super::decision(
                "backfill_gap",
                None,
                format!(
                    "Artifact budget could not safely repair {} of {} missing release cuts: {}.",
                    unresolved_tags.len(),
                    missing_documentation_count(case),
                    unresolved_tags.join(", ")
                ),
                6,
            ));
        }

        Ok(classify_resolved_chain(
            case,
            &resolved,
            "Artifact confidence, source agreement, and rollback checks were sufficient to repair the missing release history.",
            6,
        ))
    }
}
