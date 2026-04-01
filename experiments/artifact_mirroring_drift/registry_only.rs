use super::{
    decision, latest_mirror_artifact, MirroringDriftCase, MirroringDriftDecision,
    MirroringDriftVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct RegistryOnlyVariant;

impl MirroringDriftVariant for RegistryOnlyVariant {
    fn name(&self) -> &'static str {
        "registry_only"
    }

    fn style(&self) -> &'static str {
        "registry only"
    }

    fn philosophy(&self) -> &'static str {
        "Assume the package registry is the only mirror that matters."
    }

    fn source_path(&self) -> &'static str {
        "experiments/artifact_mirroring_drift/registry_only.rs"
    }

    fn decide(&self, case: &MirroringDriftCase) -> Result<MirroringDriftDecision> {
        let Some(artifact) = latest_mirror_artifact(case, "package_registry") else {
            return Ok(decision(
                "mirror_drift",
                None,
                "The latest train has no package-registry artifact.",
                1,
            ));
        };

        Ok(if artifact.decision_kind == "rollback_reference" {
            decision(
                "mirror_rejected",
                None,
                "The package registry marks the release as rolled back.",
                1,
            )
        } else if artifact.reference.as_deref() == Some(case.current_reference.as_str()) {
            decision(
                "mirror_confirmed",
                Some(case.current_reference.clone()),
                "The package registry still points at the current reference.",
                1,
            )
        } else if let Some(reference) = &artifact.reference {
            decision(
                "mirror_superseded",
                Some(reference.clone()),
                "The package registry points at a challenger reference.",
                1,
            )
        } else {
            decision(
                "mirror_drift",
                None,
                "The package registry does not identify a stable reference.",
                1,
            )
        })
    }
}
