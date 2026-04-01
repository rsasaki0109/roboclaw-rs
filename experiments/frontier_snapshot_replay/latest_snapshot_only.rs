use super::{
    decision, latest_snapshot, winner, FrontierReplayCase, FrontierReplayDecision,
    FrontierReplayVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct LatestSnapshotOnlyVariant;

impl FrontierReplayVariant for LatestSnapshotOnlyVariant {
    fn name(&self) -> &'static str {
        "latest_snapshot_only"
    }

    fn style(&self) -> &'static str {
        "latest snapshot"
    }

    fn philosophy(&self) -> &'static str {
        "Replay only the newest comparable snapshot and ignore older evidence."
    }

    fn source_path(&self) -> &'static str {
        "experiments/frontier_snapshot_replay/latest_snapshot_only.rs"
    }

    fn decide(&self, case: &FrontierReplayCase) -> Result<FrontierReplayDecision> {
        let Some(snapshot) = latest_snapshot(case) else {
            return Ok(decision(
                "hold_experimental",
                None,
                "No comparable snapshots are available.",
            ));
        };

        Ok(match winner(snapshot, 5.0) {
            "frontier" => decision(
                "promote_reference",
                Some(case.frontier_candidate.clone()),
                "Newest snapshot favors the frontier candidate.",
            ),
            "rival" => decision(
                "switch_reference",
                Some(case.rival_candidate.clone()),
                "Newest snapshot favors the rival candidate.",
            ),
            _ => decision(
                "hold_experimental",
                None,
                "Newest snapshot is too close to justify promotion.",
            ),
        })
    }
}
