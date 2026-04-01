use super::{
    comparable_snapshots, decision, latest_per_provider, winner, FrontierReplayCase,
    FrontierReplayDecision, FrontierReplayVariant, SnapshotEvidence,
};
use anyhow::Result;

const PROMOTION_MARGIN: f64 = 20.0;
const RECENT_WIN_MARGIN: f64 = 3.0;

#[derive(Debug, Default)]
pub struct WeightedStabilityVariant;

impl FrontierReplayVariant for WeightedStabilityVariant {
    fn name(&self) -> &'static str {
        "weighted_stability"
    }

    fn style(&self) -> &'static str {
        "weighted replay"
    }

    fn philosophy(&self) -> &'static str {
        "Use every comparable snapshot, weighted by recency and provider type, before changing the frontier."
    }

    fn source_path(&self) -> &'static str {
        "experiments/frontier_snapshot_replay/weighted_stability.rs"
    }

    fn decide(&self, case: &FrontierReplayCase) -> Result<FrontierReplayDecision> {
        let snapshots = comparable_snapshots(case);
        let latest = latest_per_provider(case);
        if latest.len() < 2 {
            return Ok(decision(
                "hold_experimental",
                None,
                "Weighted replay requires at least two providers to avoid overfitting one environment.",
            ));
        }

        let weighted_margin = snapshots
            .iter()
            .map(|snapshot| {
                (snapshot.frontier_accuracy - snapshot.rival_accuracy) * snapshot_weight(snapshot)
            })
            .sum::<f64>();
        let recent_frontier_wins = latest
            .iter()
            .filter(|snapshot| winner(snapshot, RECENT_WIN_MARGIN) == "frontier")
            .count();
        let recent_rival_wins = latest
            .iter()
            .filter(|snapshot| winner(snapshot, RECENT_WIN_MARGIN) == "rival")
            .count();

        Ok(
            if weighted_margin >= PROMOTION_MARGIN && recent_frontier_wins >= 2 {
                decision(
                "promote_reference",
                Some(case.frontier_candidate.clone()),
                "Weighted replay shows the frontier remaining stable across recent provider snapshots.",
            )
            } else if weighted_margin <= -PROMOTION_MARGIN && recent_rival_wins >= 2 {
                decision(
                    "switch_reference",
                    Some(case.rival_candidate.clone()),
                    "Weighted replay shows the rival taking over across recent provider snapshots.",
                )
            } else {
                decision(
                    "hold_experimental",
                    None,
                    "Snapshot history is mixed, stale, or too narrow for a stable promotion.",
                )
            },
        )
    }
}

fn snapshot_weight(snapshot: &SnapshotEvidence) -> f64 {
    let provider_weight = match snapshot.provider.as_str() {
        "openai" | "claude" => 1.2,
        "local" => 1.0,
        "mock" => 0.8,
        _ => 1.0,
    };
    let recency_weight = 1.0 / (1.0 + snapshot.days_ago as f64 / 7.0);
    provider_weight * recency_weight
}
