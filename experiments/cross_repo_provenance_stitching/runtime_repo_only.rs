use super::{
    apply_artifact, classify_stitched_chain, decision, empty_stitched_tags, repo_artifact,
    CrossRepoStitchCase, CrossRepoStitchDecision, CrossRepoStitchVariant,
};
use anyhow::Result;

const MIN_CONFIDENCE: f64 = 0.70;

#[derive(Debug, Default)]
pub struct RuntimeRepoOnlyVariant;

impl CrossRepoStitchVariant for RuntimeRepoOnlyVariant {
    fn name(&self) -> &'static str {
        "runtime_repo_only"
    }

    fn style(&self) -> &'static str {
        "runtime repo only"
    }

    fn philosophy(&self) -> &'static str {
        "Assume the runtime repository carries the only trustworthy release lineage."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_repo_provenance_stitching/runtime_repo_only.rs"
    }

    fn decide(&self, case: &CrossRepoStitchCase) -> Result<CrossRepoStitchDecision> {
        let mut stitched = empty_stitched_tags(case);
        for tag in &case.release_tags {
            let Some(artifact) = repo_artifact(case, "runtime_repo", tag, MIN_CONFIDENCE) else {
                return Ok(decision(
                    "stitched_gap",
                    None,
                    format!("Runtime repo has no trustworthy artifact for {tag}."),
                    1,
                ));
            };
            apply_artifact(&mut stitched, tag, &artifact);
        }

        Ok(classify_stitched_chain(
            case,
            &stitched,
            "Only runtime-repo artifacts were stitched into the lineage.",
            1,
        ))
    }
}
