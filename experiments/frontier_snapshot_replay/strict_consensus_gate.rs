use super::{
    decision, latest_per_provider, winner, FrontierReplayCase, FrontierReplayDecision,
    FrontierReplayVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct StrictConsensusGateVariant;

impl FrontierReplayVariant for StrictConsensusGateVariant {
    fn name(&self) -> &'static str {
        "strict_consensus_gate"
    }

    fn style(&self) -> &'static str {
        "strict consensus"
    }

    fn philosophy(&self) -> &'static str {
        "Require broad multi-provider agreement before promoting or switching a frontier."
    }

    fn source_path(&self) -> &'static str {
        "experiments/frontier_snapshot_replay/strict_consensus_gate.rs"
    }

    fn decide(&self, case: &FrontierReplayCase) -> Result<FrontierReplayDecision> {
        let snapshots = latest_per_provider(case);
        if snapshots.len() < 3 {
            return Ok(decision(
                "hold_experimental",
                None,
                "Consensus gate requires three provider snapshots.",
            ));
        }

        let frontier_wins = snapshots
            .iter()
            .all(|snapshot| winner(snapshot, 5.0) == "frontier");
        let rival_wins = snapshots
            .iter()
            .all(|snapshot| winner(snapshot, 5.0) == "rival");

        Ok(if frontier_wins {
            decision(
                "promote_reference",
                Some(case.frontier_candidate.clone()),
                "Every latest provider snapshot favors the frontier candidate.",
            )
        } else if rival_wins {
            decision(
                "switch_reference",
                Some(case.rival_candidate.clone()),
                "Every latest provider snapshot favors the rival candidate.",
            )
        } else {
            decision(
                "hold_experimental",
                None,
                "Consensus is not strong enough for promotion.",
            )
        })
    }
}
