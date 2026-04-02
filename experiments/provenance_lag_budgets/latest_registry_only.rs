use super::{
    decision, latest_surface_signal, ProvenanceLagCase, ProvenanceLagDecision, ProvenanceLagVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct LatestRegistryOnlyVariant;

impl ProvenanceLagVariant for LatestRegistryOnlyVariant {
    fn name(&self) -> &'static str {
        "latest_registry_only"
    }

    fn style(&self) -> &'static str {
        "registry only"
    }

    fn philosophy(&self) -> &'static str {
        "Treat the package registry as the only publication surface that matters."
    }

    fn source_path(&self) -> &'static str {
        "experiments/provenance_lag_budgets/latest_registry_only.rs"
    }

    fn decide(&self, case: &ProvenanceLagCase) -> Result<ProvenanceLagDecision> {
        let Some(signal) = latest_surface_signal(case, "package_registry") else {
            return Ok(decision(
                "lag_pending",
                None,
                "No package-registry signal is available for the latest train.",
                1,
            ));
        };

        Ok(if signal.decision_kind == "rollback_reference" {
            decision(
                "lag_blocked",
                None,
                "The package registry marks the current release as rolled back.",
                1,
            )
        } else if signal.reference.as_deref() == Some(case.current_reference.as_str()) {
            decision(
                "lag_confirmed",
                Some(case.current_reference.clone()),
                "The package registry still points at the current reference.",
                1,
            )
        } else if let Some(reference) = &signal.reference {
            decision(
                "lag_superseded",
                Some(reference.clone()),
                "The package registry already points at a challenger reference.",
                1,
            )
        } else {
            decision(
                "lag_pending",
                None,
                "The package registry does not expose a stable reference yet.",
                1,
            )
        })
    }
}
