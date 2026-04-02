use super::{
    decision, latest_mirror_artifact, MirroringDriftCase, MirroringDriftDecision,
    MirroringDriftVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct DocsPortalPreferredVariant;

impl MirroringDriftVariant for DocsPortalPreferredVariant {
    fn name(&self) -> &'static str {
        "docs_portal_preferred"
    }

    fn style(&self) -> &'static str {
        "docs portal preferred"
    }

    fn philosophy(&self) -> &'static str {
        "Treat docs mirrors as the canonical publication surface when mirrors disagree."
    }

    fn source_path(&self) -> &'static str {
        "experiments/artifact_mirroring_drift/docs_portal_preferred.rs"
    }

    fn decide(&self, case: &MirroringDriftCase) -> Result<MirroringDriftDecision> {
        let artifact = latest_mirror_artifact(case, "docs_portal")
            .or_else(|| latest_mirror_artifact(case, "api_docs"))
            .or_else(|| latest_mirror_artifact(case, "release_feed"))
            .or_else(|| latest_mirror_artifact(case, "package_registry"));

        let Some(artifact) = artifact else {
            return Ok(decision(
                "mirror_drift",
                None,
                "No documentation-facing mirror is available in the latest train.",
                3,
            ));
        };

        Ok(if artifact.decision_kind == "rollback_reference" {
            decision(
                "mirror_rejected",
                None,
                "The preferred docs mirror marks the train as rolled back.",
                3,
            )
        } else if artifact.reference.as_deref() == Some(case.current_reference.as_str()) {
            decision(
                "mirror_confirmed",
                Some(case.current_reference.clone()),
                "The preferred docs mirror still points at the current reference.",
                3,
            )
        } else if let Some(reference) = &artifact.reference {
            decision(
                "mirror_superseded",
                Some(reference.clone()),
                "The preferred docs mirror points at a challenger reference.",
                3,
            )
        } else {
            decision(
                "mirror_drift",
                None,
                "The preferred docs mirror does not identify a stable reference.",
                3,
            )
        })
    }
}
