use super::{
    decision, latest_train, train_state, CrossTrainLagCase, CrossTrainLagDecision,
    CrossTrainLagVariant,
};
use anyhow::Result;

const MIN_CONFIDENCE: f64 = 0.75;
const CURRENT_CONFIRM_THRESHOLD: f64 = 8.0;
const CHALLENGER_SUPERSEDE_THRESHOLD: f64 = 8.0;
const ROLLBACK_BLOCK_THRESHOLD: f64 = 4.5;
const LOW_CHALLENGER_THRESHOLD: f64 = 2.5;

#[derive(Debug, Default)]
pub struct LatestTrainOnlyVariant;

impl CrossTrainLagVariant for LatestTrainOnlyVariant {
    fn name(&self) -> &'static str {
        "latest_train_only"
    }

    fn style(&self) -> &'static str {
        "latest train"
    }

    fn philosophy(&self) -> &'static str {
        "Treat each new release train as a full reset and ignore carryover debt from older pending trains."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_train_lag_carryover/latest_train_only.rs"
    }

    fn decide(&self, case: &CrossTrainLagCase) -> Result<CrossTrainLagDecision> {
        let Some(train) = latest_train(case) else {
            return Ok(decision(
                "carryover_pending",
                None,
                "No release train exists yet.",
                1,
            ));
        };
        let state = train_state(
            train,
            &case.current_reference,
            MIN_CONFIDENCE,
            CURRENT_CONFIRM_THRESHOLD,
            CHALLENGER_SUPERSEDE_THRESHOLD,
            ROLLBACK_BLOCK_THRESHOLD,
            LOW_CHALLENGER_THRESHOLD,
        );

        Ok(match state.kind.as_str() {
            "confirmed" => decision(
                "carryover_confirmed",
                Some(case.current_reference.clone()),
                "The latest release train alone confirms the current reference.",
                1,
            ),
            "superseded" => decision(
                "carryover_superseded",
                state.reference,
                "The latest release train alone is enough to supersede the current reference.",
                1,
            ),
            "blocked" => decision(
                "carryover_blocked",
                None,
                "The latest release train alone blocks publication trust.",
                1,
            ),
            _ => decision(
                "carryover_pending",
                None,
                "The latest release train alone is still unresolved.",
                1,
            ),
        })
    }
}
