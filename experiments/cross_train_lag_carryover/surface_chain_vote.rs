use super::{decision, CrossTrainLagCase, CrossTrainLagDecision, CrossTrainLagVariant};
use anyhow::Result;
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub struct SurfaceChainVoteVariant;

impl CrossTrainLagVariant for SurfaceChainVoteVariant {
    fn name(&self) -> &'static str {
        "surface_chain_vote"
    }

    fn style(&self) -> &'static str {
        "chain vote"
    }

    fn philosophy(&self) -> &'static str {
        "Flatten the last two release trains into one vote across surfaces and let the largest bucket win."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_train_lag_carryover/surface_chain_vote.rs"
    }

    fn decide(&self, case: &CrossTrainLagCase) -> Result<CrossTrainLagDecision> {
        let mut counts = BTreeMap::<String, usize>::new();
        for train in case.release_trains.iter().rev().take(2) {
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
        }

        let Some((winner, count)) = counts
            .iter()
            .max_by_key(|(_, count)| **count)
            .map(|(winner, count)| (winner.clone(), *count))
        else {
            return Ok(decision(
                "carryover_pending",
                None,
                "No publication signals exist in the recent carryover window.",
                4,
            ));
        };

        if counts.values().filter(|value| **value == count).count() > 1 {
            return Ok(decision(
                "carryover_pending",
                None,
                "The recent carryover window has no unique vote winner.",
                4,
            ));
        }

        Ok(if winner == "rollback" {
            decision(
                "carryover_blocked",
                None,
                "Rollback wins the vote across the recent carryover window.",
                4,
            )
        } else if winner == "current" {
            decision(
                "carryover_confirmed",
                Some(case.current_reference.clone()),
                "Current-reference signals win the vote across the recent carryover window.",
                4,
            )
        } else if let Some(reference) = winner.strip_prefix("challenger:") {
            decision(
                "carryover_superseded",
                Some(reference.to_string()),
                "A challenger wins the vote across the recent carryover window.",
                4,
            )
        } else {
            decision(
                "carryover_pending",
                None,
                "The recent carryover window does not resolve to a stable reference.",
                4,
            )
        })
    }
}
