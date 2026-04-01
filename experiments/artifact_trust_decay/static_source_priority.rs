use super::{
    any_high_confidence_rollback, decision, latest_source_artifact, ArtifactTrustCase,
    ArtifactTrustDecision, ArtifactTrustVariant,
};
use anyhow::Result;

const ROLLBACK_MIN_CONFIDENCE: f64 = 0.80;

#[derive(Debug, Default)]
pub struct StaticSourcePriorityVariant;

impl ArtifactTrustVariant for StaticSourcePriorityVariant {
    fn name(&self) -> &'static str {
        "static_source_priority"
    }

    fn style(&self) -> &'static str {
        "static source priority"
    }

    fn philosophy(&self) -> &'static str {
        "Apply a fixed artifact-source precedence and let high-priority sources dominate regardless of release-train drift."
    }

    fn source_path(&self) -> &'static str {
        "experiments/artifact_trust_decay/static_source_priority.rs"
    }

    fn decide(&self, case: &ArtifactTrustCase) -> Result<ArtifactTrustDecision> {
        if any_high_confidence_rollback(case, ROLLBACK_MIN_CONFIDENCE) {
            return Ok(decision(
                "trust_rejected",
                None,
                "A rollback note exists in the static-priority artifact set.",
                3,
            ));
        }

        for source in ["release_notes", "changelog"] {
            let Some(artifact) = latest_source_artifact(case, source) else {
                continue;
            };

            if artifact.reference.as_deref() == Some(case.current_reference.as_str()) {
                return Ok(decision(
                    "trust_confirmed",
                    Some(case.current_reference.clone()),
                    format!("The latest {source} entry supports the current reference."),
                    3,
                ));
            }

            if let Some(reference) = &artifact.reference {
                return Ok(decision(
                    "trust_superseded",
                    Some(reference.clone()),
                    format!("The latest {source} entry supports a challenger reference."),
                    3,
                ));
            }
        }

        Ok(decision(
            "trust_decay",
            None,
            "No prioritized artifact source can sustain trust.",
            3,
        ))
    }
}
