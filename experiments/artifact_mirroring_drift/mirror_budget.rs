use super::{
    best_challenger_support, current_consensus_train_count, current_support, decision,
    dominant_challenger_trains, fresh_signal_count, latest_train, mirror_conflict,
    rollback_consensus_train_count, rollback_support, MirroringDriftCase, MirroringDriftDecision,
    MirroringDriftVariant,
};
use anyhow::Result;

const MAX_FRESHNESS_HOURS: u32 = 48;
const MIN_CONFIDENCE: f64 = 0.75;
const CURRENT_CONFIRM_THRESHOLD: f64 = 6.5;
const ROLLBACK_REJECT_THRESHOLD: f64 = 5.5;
const NOISE_CHALLENGER_THRESHOLD: f64 = 3.5;

#[derive(Debug, Default)]
pub struct MirrorBudgetVariant;

impl MirroringDriftVariant for MirrorBudgetVariant {
    fn name(&self) -> &'static str {
        "mirror_budget"
    }

    fn style(&self) -> &'static str {
        "mirror budget"
    }

    fn philosophy(&self) -> &'static str {
        "Balance freshness, mirror agreement, rollback pressure, and train history before trusting mirrored publication state."
    }

    fn source_path(&self) -> &'static str {
        "experiments/artifact_mirroring_drift/mirror_budget.rs"
    }

    fn decide(&self, case: &MirroringDriftCase) -> Result<MirroringDriftDecision> {
        let Some(latest) = latest_train(case) else {
            return Ok(decision(
                "mirror_drift",
                None,
                "No release train is available.",
                6,
            ));
        };

        let latest_current = current_support(
            latest,
            &case.current_reference,
            MAX_FRESHNESS_HOURS,
            MIN_CONFIDENCE,
        );
        let latest_challenger = best_challenger_support(
            latest,
            &case.current_reference,
            MAX_FRESHNESS_HOURS,
            MIN_CONFIDENCE,
        );
        let latest_challenger_score = latest_challenger
            .as_ref()
            .map(|(_, score, _)| *score)
            .unwrap_or(0.0);
        let latest_challenger_mirrors = latest_challenger
            .as_ref()
            .map(|(_, _, mirrors)| *mirrors)
            .unwrap_or(0);
        let latest_rollback = rollback_support(latest, MAX_FRESHNESS_HOURS, MIN_CONFIDENCE);
        let latest_fresh_signals = fresh_signal_count(latest, MAX_FRESHNESS_HOURS, MIN_CONFIDENCE);
        let latest_conflict = mirror_conflict(
            latest,
            &case.current_reference,
            MAX_FRESHNESS_HOURS,
            MIN_CONFIDENCE,
        );

        let current_trains =
            current_consensus_train_count(case, MAX_FRESHNESS_HOURS, MIN_CONFIDENCE);
        let dominant_challenger =
            dominant_challenger_trains(case, MAX_FRESHNESS_HOURS, MIN_CONFIDENCE);
        let challenger_trains = dominant_challenger
            .as_ref()
            .map(|(_, count)| *count)
            .unwrap_or(0);
        let rollback_trains =
            rollback_consensus_train_count(case, MAX_FRESHNESS_HOURS, MIN_CONFIDENCE);

        Ok(if latest_rollback >= ROLLBACK_REJECT_THRESHOLD {
            decision(
                "mirror_rejected",
                None,
                format!(
                    "Fresh rollback pressure dominates the latest mirrors: latest_rollback={latest_rollback:.2}."
                ),
                6,
            )
        } else if latest_conflict {
            decision(
                "mirror_drift",
                None,
                format!(
                    "Fresh mirrors disagree in the latest train: latest_current={latest_current:.2} latest_challenger={latest_challenger_score:.2}."
                ),
                6,
            )
        } else if latest_challenger_score > latest_current + 1.0
            && latest_challenger_mirrors >= 2
            && challenger_trains >= current_trains
        {
            decision(
                "mirror_superseded",
                dominant_challenger.map(|(reference, _)| reference),
                format!(
                    "Fresh challenger mirrors dominate enough trains: challenger_trains={challenger_trains} current_trains={current_trains} latest_challenger={latest_challenger_score:.2}."
                ),
                6,
            )
        } else if current_trains >= 2
            && latest_current >= CURRENT_CONFIRM_THRESHOLD
            && rollback_trains <= 1
        {
            decision(
                "mirror_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "Fresh current-reference mirrors remain coherent across trains: current_trains={current_trains} latest_current={latest_current:.2}."
                ),
                6,
            )
        } else if latest_current >= CURRENT_CONFIRM_THRESHOLD + 2.0
            && latest_fresh_signals >= 3
            && latest_challenger_score <= NOISE_CHALLENGER_THRESHOLD
            && latest_rollback < 1.0
            && challenger_trains == 0
            && rollback_trains <= 1
        {
            decision(
                "mirror_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "A fully refreshed mirror set re-established the current reference after earlier rollback pressure: latest_current={latest_current:.2} latest_fresh_signals={latest_fresh_signals}."
                ),
                6,
            )
        } else if current_trains >= 2
            && latest_fresh_signals < 2
            && latest_challenger_score <= NOISE_CHALLENGER_THRESHOLD
        {
            decision(
                "mirror_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "Historical agreement outweighs weak latest mirror noise: current_trains={current_trains} latest_challenger={latest_challenger_score:.2}."
                ),
                6,
            )
        } else {
            decision(
                "mirror_drift",
                None,
                format!(
                    "No stable mirror state survives the budget: current_trains={current_trains} challenger_trains={challenger_trains} rollback_trains={rollback_trains} latest_current={latest_current:.2} latest_challenger={latest_challenger_score:.2}."
                ),
                6,
            )
        })
    }
}
