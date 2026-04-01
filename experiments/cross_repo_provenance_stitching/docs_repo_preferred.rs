use super::{
    apply_artifact, classify_stitched_chain, decision, empty_stitched_tags, repo_artifact,
    CrossRepoStitchCase, CrossRepoStitchDecision, CrossRepoStitchVariant,
};
use anyhow::Result;

const MIN_CONFIDENCE: f64 = 0.70;

#[derive(Debug, Default)]
pub struct DocsRepoPreferredVariant;

impl CrossRepoStitchVariant for DocsRepoPreferredVariant {
    fn name(&self) -> &'static str {
        "docs_repo_preferred"
    }

    fn style(&self) -> &'static str {
        "docs repo preferred"
    }

    fn philosophy(&self) -> &'static str {
        "Prefer docs and release repositories over runtime artifacts when stitching provenance."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_repo_provenance_stitching/docs_repo_preferred.rs"
    }

    fn decide(&self, case: &CrossRepoStitchCase) -> Result<CrossRepoStitchDecision> {
        let mut stitched = empty_stitched_tags(case);
        for tag in &case.release_tags {
            let artifact = repo_artifact(case, "docs_repo", tag, MIN_CONFIDENCE)
                .or_else(|| repo_artifact(case, "release_repo", tag, MIN_CONFIDENCE))
                .or_else(|| repo_artifact(case, "runtime_repo", tag, MIN_CONFIDENCE));

            let Some(artifact) = artifact else {
                return Ok(decision(
                    "stitched_gap",
                    None,
                    format!("No preferred repository carries a trustworthy artifact for {tag}."),
                    3,
                ));
            };
            apply_artifact(&mut stitched, tag, &artifact);
        }

        Ok(classify_stitched_chain(
            case,
            &stitched,
            "Docs and release repositories were preferred over runtime artifacts.",
            3,
        ))
    }
}
