use super::{
    decision, latest_train, ArtifactTrustCase, ArtifactTrustDecision, ArtifactTrustVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct LatestTrainOnlyVariant;

impl ArtifactTrustVariant for LatestTrainOnlyVariant {
    fn name(&self) -> &'static str {
        "latest_train_only"
    }

    fn style(&self) -> &'static str {
        "latest train only"
    }

    fn philosophy(&self) -> &'static str {
        "Trust the newest artifact entry and ignore cross-train history."
    }

    fn source_path(&self) -> &'static str {
        "experiments/artifact_trust_decay/latest_train_only.rs"
    }

    fn decide(&self, case: &ArtifactTrustCase) -> Result<ArtifactTrustDecision> {
        let Some(train) = latest_train(case) else {
            return Ok(decision(
                "trust_decay",
                None,
                "No release-train artifacts exist.",
                1,
            ));
        };
        let Some(artifact) = train.artifacts.iter().max_by(|left, right| {
            left.confidence
                .partial_cmp(&right.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) else {
            return Ok(decision(
                "trust_decay",
                None,
                "The latest train contains no artifacts.",
                1,
            ));
        };

        Ok(if artifact.decision_kind == "rollback_reference" {
            decision(
                "trust_rejected",
                None,
                "The strongest latest artifact is a rollback signal.",
                1,
            )
        } else if artifact.reference.as_deref() == Some(case.current_reference.as_str()) {
            decision(
                "trust_confirmed",
                Some(case.current_reference.clone()),
                "The strongest latest artifact supports the current reference.",
                1,
            )
        } else if let Some(reference) = &artifact.reference {
            decision(
                "trust_superseded",
                Some(reference.clone()),
                "The strongest latest artifact supports a challenger reference.",
                1,
            )
        } else {
            decision(
                "trust_decay",
                None,
                "The latest artifact does not identify a trustworthy reference.",
                1,
            )
        })
    }
}
