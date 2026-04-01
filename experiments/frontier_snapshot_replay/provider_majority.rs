use super::{
    decision, latest_per_provider, winner, FrontierReplayCase, FrontierReplayDecision,
    FrontierReplayVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct ProviderMajorityVariant;

impl FrontierReplayVariant for ProviderMajorityVariant {
    fn name(&self) -> &'static str {
        "provider_majority"
    }

    fn style(&self) -> &'static str {
        "provider majority"
    }

    fn philosophy(&self) -> &'static str {
        "Let the most recent snapshot from each provider vote on the frontier."
    }

    fn source_path(&self) -> &'static str {
        "experiments/frontier_snapshot_replay/provider_majority.rs"
    }

    fn decide(&self, case: &FrontierReplayCase) -> Result<FrontierReplayDecision> {
        let snapshots = latest_per_provider(case);
        if snapshots.len() < 2 {
            return Ok(decision(
                "hold_experimental",
                None,
                "Need at least two provider snapshots before trusting a majority vote.",
            ));
        }

        let mut frontier_votes = 0usize;
        let mut rival_votes = 0usize;
        for snapshot in snapshots {
            match winner(snapshot, 5.0) {
                "frontier" => frontier_votes += 1,
                "rival" => rival_votes += 1,
                _ => {}
            }
        }

        Ok(if frontier_votes > rival_votes && frontier_votes >= 2 {
            decision(
                "promote_reference",
                Some(case.frontier_candidate.clone()),
                "Most providers favor the frontier candidate.",
            )
        } else if rival_votes > frontier_votes && rival_votes >= 2 {
            decision(
                "switch_reference",
                Some(case.rival_candidate.clone()),
                "Most providers favor the rival candidate.",
            )
        } else {
            decision(
                "hold_experimental",
                None,
                "Provider votes are split or too weak to promote.",
            )
        })
    }
}
