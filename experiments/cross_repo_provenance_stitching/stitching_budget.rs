use super::{
    apply_artifact, best_artifact, classify_stitched_chain, decision, empty_stitched_tags,
    tag_signature_scores, CrossRepoStitchCase, CrossRepoStitchDecision, CrossRepoStitchVariant,
};
use anyhow::Result;

const MIN_CONFIDENCE: f64 = 0.70;
const REJECT_THRESHOLD: f64 = 4.50;
const SINGLE_SOURCE_THRESHOLD: f64 = 3.20;
const CROSS_REPO_THRESHOLD: f64 = 6.50;
const LEAD_THRESHOLD: f64 = 1.40;
const DIRECT_ROLLBACK_CONFIDENCE: f64 = 0.90;

#[derive(Debug, Default)]
pub struct StitchingBudgetVariant;

impl CrossRepoStitchVariant for StitchingBudgetVariant {
    fn name(&self) -> &'static str {
        "stitching_budget"
    }

    fn style(&self) -> &'static str {
        "stitching budget"
    }

    fn philosophy(&self) -> &'static str {
        "Balance source confidence, cross-repo agreement, and rollback pressure before stitching a release chain across repositories."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_repo_provenance_stitching/stitching_budget.rs"
    }

    fn decide(&self, case: &CrossRepoStitchCase) -> Result<CrossRepoStitchDecision> {
        let mut stitched = empty_stitched_tags(case);

        for tag in &case.release_tags {
            if let Some(artifact) = case.artifacts.iter().find(|artifact| {
                artifact.tag == *tag
                    && artifact.decision_kind == "rollback_reference"
                    && artifact.confidence >= DIRECT_ROLLBACK_CONFIDENCE
            }) {
                return Ok(decision(
                    "stitched_rejected",
                    None,
                    format!(
                        "A high-confidence rollback artifact from {} vetoes {tag}.",
                        artifact.repository
                    ),
                    6,
                ));
            }

            let signatures = tag_signature_scores(case, tag, MIN_CONFIDENCE);
            if signatures.is_empty() {
                return Ok(decision(
                    "stitched_gap",
                    None,
                    format!("No trustworthy artifact exists for {tag}."),
                    6,
                ));
            }

            if let Some(rollback) = signatures.iter().find(|signature| {
                signature.decision_kind == "rollback_reference"
                    && signature.score >= REJECT_THRESHOLD
            }) {
                return Ok(decision(
                    "stitched_rejected",
                    None,
                    format!(
                        "Rollback pressure dominates {tag}: rollback_score={:.2}.",
                        rollback.score
                    ),
                    6,
                ));
            }

            let best = &signatures[0];
            let second_score = signatures
                .get(1)
                .map(|signature| signature.score)
                .unwrap_or(0.0);

            let accepted = if best.repositories >= 2 && best.score >= CROSS_REPO_THRESHOLD {
                true
            } else if signatures.len() == 1 && best.score >= SINGLE_SOURCE_THRESHOLD {
                true
            } else {
                best.score >= SINGLE_SOURCE_THRESHOLD
                    && best.score - second_score >= LEAD_THRESHOLD
                    && best.decision_kind != "rollback_reference"
            };

            if !accepted {
                return Ok(decision(
                    "stitched_gap",
                    None,
                    format!(
                        "Cross-repo evidence for {tag} is too weak or too conflicted: best={:.2} second={:.2}.",
                        best.score, second_score
                    ),
                    6,
                ));
            }

            let Some(artifact) = best_artifact(case, tag, MIN_CONFIDENCE).filter(|artifact| {
                artifact.decision_kind == best.decision_kind && artifact.reference == best.reference
            }) else {
                return Ok(decision(
                    "stitched_gap",
                    None,
                    format!("No artifact can materialize the accepted signature for {tag}."),
                    6,
                ));
            };
            apply_artifact(&mut stitched, tag, &artifact);
        }

        Ok(classify_stitched_chain(
            case,
            &stitched,
            "Cross-repo stitching accepted only artifacts that fit the confidence and agreement budget.",
            6,
        ))
    }
}
