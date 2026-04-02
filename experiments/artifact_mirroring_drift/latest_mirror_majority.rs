use super::{
    decision, latest_train, MirroringDriftCase, MirroringDriftDecision, MirroringDriftVariant,
};
use anyhow::Result;
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct LatestMirrorMajorityVariant;

impl MirroringDriftVariant for LatestMirrorMajorityVariant {
    fn name(&self) -> &'static str {
        "latest_mirror_majority"
    }

    fn style(&self) -> &'static str {
        "latest majority"
    }

    fn philosophy(&self) -> &'static str {
        "Reduce the latest train to a simple majority vote across mirrors."
    }

    fn source_path(&self) -> &'static str {
        "experiments/artifact_mirroring_drift/latest_mirror_majority.rs"
    }

    fn decide(&self, case: &MirroringDriftCase) -> Result<MirroringDriftDecision> {
        let Some(train) = latest_train(case) else {
            return Ok(decision(
                "mirror_drift",
                None,
                "No mirrored train exists.",
                4,
            ));
        };

        let mut counts = BTreeMap::<String, usize>::new();
        for artifact in &train.artifacts {
            let bucket = if artifact.decision_kind == "rollback_reference" {
                "rollback".to_string()
            } else if artifact.reference.as_deref() == Some(case.current_reference.as_str()) {
                "current".to_string()
            } else if let Some(reference) = &artifact.reference {
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
                "mirror_drift",
                None,
                "The latest train has no mirror artifacts.",
                4,
            ));
        };

        if counts.values().filter(|value| **value == count).count() > 1 {
            return Ok(decision(
                "mirror_drift",
                None,
                "The latest train majority is tied across mirrors.",
                4,
            ));
        }

        Ok(if winner == "rollback" {
            decision(
                "mirror_rejected",
                None,
                "A rollback message holds the mirror majority.",
                4,
            )
        } else if winner == "current" {
            decision(
                "mirror_confirmed",
                Some(case.current_reference.clone()),
                "The latest train mirror majority still supports the current reference.",
                4,
            )
        } else if let Some(reference) = winner.strip_prefix("challenger:") {
            decision(
                "mirror_superseded",
                Some(reference.to_string()),
                "The latest train mirror majority supports a challenger reference.",
                4,
            )
        } else {
            decision(
                "mirror_drift",
                None,
                "The latest train majority does not identify a stable reference.",
                4,
            )
        })
    }
}
