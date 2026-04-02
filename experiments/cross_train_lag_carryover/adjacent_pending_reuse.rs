use super::{
    collect_train_states, decision, CrossTrainLagCase, CrossTrainLagDecision, CrossTrainLagVariant,
};
use anyhow::Result;

const MIN_CONFIDENCE: f64 = 0.75;
const CURRENT_CONFIRM_THRESHOLD: f64 = 8.0;
const CHALLENGER_SUPERSEDE_THRESHOLD: f64 = 8.0;
const ROLLBACK_BLOCK_THRESHOLD: f64 = 4.5;
const LOW_CHALLENGER_THRESHOLD: f64 = 2.5;

#[derive(Debug, Default)]
pub struct AdjacentPendingReuseVariant;

impl CrossTrainLagVariant for AdjacentPendingReuseVariant {
    fn name(&self) -> &'static str {
        "adjacent_pending_reuse"
    }

    fn style(&self) -> &'static str {
        "1-step carry"
    }

    fn philosophy(&self) -> &'static str {
        "Reuse only the immediately previous pending train when the latest train is still unresolved."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_train_lag_carryover/adjacent_pending_reuse.rs"
    }

    fn decide(&self, case: &CrossTrainLagCase) -> Result<CrossTrainLagDecision> {
        let states = collect_train_states(
            case,
            MIN_CONFIDENCE,
            CURRENT_CONFIRM_THRESHOLD,
            CHALLENGER_SUPERSEDE_THRESHOLD,
            ROLLBACK_BLOCK_THRESHOLD,
            LOW_CHALLENGER_THRESHOLD,
        );
        let Some(latest) = states.last() else {
            return Ok(decision(
                "carryover_pending",
                None,
                "No release train exists yet.",
                3,
            ));
        };

        Ok(match latest.kind.as_str() {
            "confirmed" => decision(
                "carryover_confirmed",
                Some(case.current_reference.clone()),
                "The latest release train confirms the current reference.",
                3,
            ),
            "superseded" => decision(
                "carryover_superseded",
                latest.reference.clone(),
                "The latest release train already supersedes the current reference.",
                3,
            ),
            "blocked" => decision(
                "carryover_blocked",
                None,
                "The latest release train already blocks trust.",
                3,
            ),
            _ => {
                if states.len() >= 2 {
                    let previous = &states[states.len() - 2];
                    if previous.kind == "pending" && latest.reference == previous.reference {
                        decision(
                            "carryover_superseded",
                            latest.reference.clone(),
                            "The latest pending train matches the previous pending challenger, so the carryover is spent immediately.",
                            3,
                        )
                    } else if previous.kind == "pending" && latest.reference != previous.reference {
                        decision(
                            "carryover_blocked",
                            None,
                            "The latest pending train conflicts with the previous pending challenger.",
                            3,
                        )
                    } else {
                        decision(
                            "carryover_pending",
                            None,
                            "The latest pending train has no adjacent pending carryover to reuse.",
                            3,
                        )
                    }
                } else {
                    decision(
                        "carryover_pending",
                        None,
                        "There is no previous train to carry unresolved lag from.",
                        3,
                    )
                }
            }
        })
    }
}
