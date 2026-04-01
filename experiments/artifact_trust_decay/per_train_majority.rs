use super::{
    decision, latest_train, ArtifactTrustCase, ArtifactTrustDecision, ArtifactTrustVariant,
};
use anyhow::Result;
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct PerTrainMajorityVariant;

impl ArtifactTrustVariant for PerTrainMajorityVariant {
    fn name(&self) -> &'static str {
        "per_train_majority"
    }

    fn style(&self) -> &'static str {
        "per-train majority"
    }

    fn philosophy(&self) -> &'static str {
        "Reduce each latest release train to a simple majority vote across artifact messages."
    }

    fn source_path(&self) -> &'static str {
        "experiments/artifact_trust_decay/per_train_majority.rs"
    }

    fn decide(&self, case: &ArtifactTrustCase) -> Result<ArtifactTrustDecision> {
        let Some(train) = latest_train(case) else {
            return Ok(decision(
                "trust_decay",
                None,
                "No latest train is available.",
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
                "trust_decay",
                None,
                "The latest train contains no countable artifacts.",
                4,
            ));
        };

        let tied = counts.values().filter(|value| **value == count).count() > 1;
        if tied {
            return Ok(decision(
                "trust_decay",
                None,
                "The latest train majority vote is tied.",
                4,
            ));
        }

        Ok(if winner == "rollback" {
            decision(
                "trust_rejected",
                None,
                "Rollback artifacts hold the majority in the latest train.",
                4,
            )
        } else if winner == "current" {
            decision(
                "trust_confirmed",
                Some(case.current_reference.clone()),
                "The latest train majority supports the current reference.",
                4,
            )
        } else if let Some(reference) = winner.strip_prefix("challenger:") {
            decision(
                "trust_superseded",
                Some(reference.to_string()),
                "The latest train majority supports a challenger reference.",
                4,
            )
        } else {
            decision(
                "trust_decay",
                None,
                "The latest train majority does not point to a stable reference.",
                4,
            )
        })
    }
}
