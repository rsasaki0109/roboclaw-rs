use super::{
    decision, latest_train, ProvenanceLagCase, ProvenanceLagDecision, ProvenanceLagVariant,
};
use anyhow::Result;
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct SurfaceMajorityVariant;

impl ProvenanceLagVariant for SurfaceMajorityVariant {
    fn name(&self) -> &'static str {
        "surface_majority"
    }

    fn style(&self) -> &'static str {
        "surface majority"
    }

    fn philosophy(&self) -> &'static str {
        "Collapse the latest train into a simple vote across publication surfaces."
    }

    fn source_path(&self) -> &'static str {
        "experiments/provenance_lag_budgets/surface_majority.rs"
    }

    fn decide(&self, case: &ProvenanceLagCase) -> Result<ProvenanceLagDecision> {
        let Some(train) = latest_train(case) else {
            return Ok(decision(
                "lag_pending",
                None,
                "No lagged publication train exists.",
                4,
            ));
        };

        let mut counts = BTreeMap::<String, usize>::new();
        for signal in &train.signals {
            let bucket = if signal.decision_kind == "rollback_reference" {
                "rollback".to_string()
            } else if signal.reference.as_deref() == Some(case.current_reference.as_str()) {
                "current".to_string()
            } else if let Some(reference) = &signal.reference {
                format!("challenger:{reference}")
            } else {
                "unknown".to_string()
            };
            *counts.entry(bucket).or_default() += 1;
        }

        let Some((winner, count)) = counts
            .iter()
            .max_by_key(|(_, count)| **count)
            .map(|(winner, count)| (winner.clone(), *count))
        else {
            return Ok(decision(
                "lag_pending",
                None,
                "The latest train has no publication signals.",
                4,
            ));
        };

        if counts.values().filter(|value| **value == count).count() > 1 {
            return Ok(decision(
                "lag_pending",
                None,
                "The latest train has no unique majority across publication surfaces.",
                4,
            ));
        }

        Ok(if winner == "rollback" {
            decision(
                "lag_blocked",
                None,
                "Rollback is the majority signal across publication surfaces.",
                4,
            )
        } else if winner == "current" {
            decision(
                "lag_confirmed",
                Some(case.current_reference.clone()),
                "A majority of publication surfaces still supports the current reference.",
                4,
            )
        } else if let Some(reference) = winner.strip_prefix("challenger:") {
            decision(
                "lag_superseded",
                Some(reference.to_string()),
                "A majority of publication surfaces supports a challenger reference.",
                4,
            )
        } else {
            decision(
                "lag_pending",
                None,
                "The latest train majority does not resolve to a stable reference.",
                4,
            )
        })
    }
}
