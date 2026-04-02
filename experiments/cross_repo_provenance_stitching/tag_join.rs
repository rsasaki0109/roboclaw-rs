use super::{
    apply_artifact, classify_stitched_chain, decision, empty_stitched_tags, matching_repo_pair,
    tag_conflict, CrossRepoStitchCase, CrossRepoStitchDecision, CrossRepoStitchVariant,
};
use anyhow::Result;

const MIN_CONFIDENCE: f64 = 0.80;

#[derive(Debug, Default)]
pub struct TagJoinVariant;

impl CrossRepoStitchVariant for TagJoinVariant {
    fn name(&self) -> &'static str {
        "tag_join"
    }

    fn style(&self) -> &'static str {
        "exact tag join"
    }

    fn philosophy(&self) -> &'static str {
        "Only trust a release tag when at least two repositories agree on the same tag-level artifact."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_repo_provenance_stitching/tag_join.rs"
    }

    fn decide(&self, case: &CrossRepoStitchCase) -> Result<CrossRepoStitchDecision> {
        let mut stitched = empty_stitched_tags(case);
        for tag in &case.release_tags {
            let matched = matching_repo_pair(case, tag, MIN_CONFIDENCE);
            if matched.is_none() && tag_conflict(case, tag, MIN_CONFIDENCE) {
                return Ok(decision(
                    "stitched_rejected",
                    None,
                    format!("Repositories disagree on the artifact for {tag}."),
                    4,
                ));
            }

            let Some(artifact) = matched else {
                return Ok(decision(
                    "stitched_gap",
                    None,
                    format!("No two repositories agree strongly enough on {tag}."),
                    4,
                ));
            };
            apply_artifact(&mut stitched, tag, &artifact);
        }

        Ok(classify_stitched_chain(
            case,
            &stitched,
            "Only same-tag agreements across repositories were stitched into the release lineage.",
            4,
        ))
    }
}
