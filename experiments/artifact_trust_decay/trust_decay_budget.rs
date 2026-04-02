use super::{
    best_challenger_support, conflict_train_count, current_consensus_train_count, current_support,
    decision, dominant_challenger_trains, latest_train, rollback_consensus_train_count,
    rollback_support, strong_signal_count, train_has_conflict, ArtifactTrustCase,
    ArtifactTrustDecision, ArtifactTrustVariant,
};
use anyhow::Result;

const MIN_STRONG_CONFIDENCE: f64 = 0.75;
const LATEST_ROLLBACK_REJECT_THRESHOLD: f64 = 5.5;
const CURRENT_CONFIRM_THRESHOLD: f64 = 7.0;
const HISTORY_NOISE_THRESHOLD: f64 = 3.5;

#[derive(Debug, Default)]
pub struct TrustDecayBudgetVariant;

impl ArtifactTrustVariant for TrustDecayBudgetVariant {
    fn name(&self) -> &'static str {
        "trust_decay_budget"
    }

    fn style(&self) -> &'static str {
        "trust decay budget"
    }

    fn philosophy(&self) -> &'static str {
        "Balance recency, source agreement, rollback pressure, and historical continuity before trusting release artifacts."
    }

    fn source_path(&self) -> &'static str {
        "experiments/artifact_trust_decay/trust_decay_budget.rs"
    }

    fn decide(&self, case: &ArtifactTrustCase) -> Result<ArtifactTrustDecision> {
        let Some(latest) = latest_train(case) else {
            return Ok(decision(
                "trust_decay",
                None,
                "No release-train artifacts exist.",
                6,
            ));
        };

        let latest_current = current_support(latest, &case.current_reference);
        let latest_challenger = best_challenger_support(latest, &case.current_reference);
        let latest_challenger_score = latest_challenger
            .as_ref()
            .map(|(_, score)| *score)
            .unwrap_or(0.0);
        let latest_rollback = rollback_support(latest);
        let latest_strong_signals = strong_signal_count(latest, MIN_STRONG_CONFIDENCE);
        let latest_conflict =
            train_has_conflict(latest, &case.current_reference, MIN_STRONG_CONFIDENCE);

        let current_trains = current_consensus_train_count(case, MIN_STRONG_CONFIDENCE);
        let dominant_challenger = dominant_challenger_trains(case, MIN_STRONG_CONFIDENCE);
        let challenger_trains = dominant_challenger
            .as_ref()
            .map(|(_, count)| *count)
            .unwrap_or(0);
        let rollback_trains = rollback_consensus_train_count(case, MIN_STRONG_CONFIDENCE);
        let conflict_trains = conflict_train_count(case, MIN_STRONG_CONFIDENCE);

        Ok(if latest_rollback >= LATEST_ROLLBACK_REJECT_THRESHOLD {
            decision(
                "trust_rejected",
                None,
                format!(
                    "The latest train contains overwhelming rollback pressure: latest_rollback={latest_rollback:.2}."
                ),
                6,
            )
        } else if latest_conflict {
            decision(
                "trust_decay",
                None,
                format!(
                    "The latest train disagrees internally: latest_current={latest_current:.2} latest_challenger={latest_challenger_score:.2} latest_rollback={latest_rollback:.2}."
                ),
                6,
            )
        } else if rollback_trains > 0 && latest_strong_signals < 2 {
            decision(
                "trust_decay",
                None,
                format!(
                    "Rollback history remains unresolved because the latest train is too sparse: rollback_trains={rollback_trains} latest_strong_signals={latest_strong_signals}."
                ),
                6,
            )
        } else if latest_challenger_score > latest_current
            && latest_strong_signals >= 2
            && challenger_trains >= current_trains
        {
            decision(
                "trust_superseded",
                dominant_challenger.map(|(reference, _)| reference),
                format!(
                    "A challenger now dominates the trusted trains: challenger_trains={challenger_trains} current_trains={current_trains} latest_challenger={latest_challenger_score:.2}."
                ),
                6,
            )
        } else if current_trains >= 2
            && latest_strong_signals < 2
            && latest_challenger_score <= HISTORY_NOISE_THRESHOLD
            && rollback_trains == 0
        {
            decision(
                "trust_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "Historical agreement outweighs a weak latest-train disturbance: current_trains={current_trains} latest_challenger={latest_challenger_score:.2}."
                ),
                6,
            )
        } else if current_trains >= challenger_trains + 1
            && latest_current >= CURRENT_CONFIRM_THRESHOLD
            && (rollback_trains == 0 || (rollback_trains == 1 && current_trains >= 2))
        {
            decision(
                "trust_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "Current-reference support survives the release-train budget: current_trains={current_trains} challenger_trains={challenger_trains} latest_current={latest_current:.2} rollback_trains={rollback_trains}."
                ),
                6,
            )
        } else if conflict_trains > 0 || rollback_trains > 0 {
            decision(
                "trust_decay",
                None,
                format!(
                    "Cross-train disagreement still consumes the trust budget: conflict_trains={conflict_trains} rollback_trains={rollback_trains}."
                ),
                6,
            )
        } else {
            decision(
                "trust_decay",
                None,
                format!(
                    "No reference accumulated enough durable support: current_trains={current_trains} challenger_trains={challenger_trains} latest_current={latest_current:.2} latest_challenger={latest_challenger_score:.2}."
                ),
                6,
            )
        })
    }
}
