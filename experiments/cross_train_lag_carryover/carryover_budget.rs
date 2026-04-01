use super::{
    collect_train_states, consecutive_pending_chain, decision, CrossTrainLagCase,
    CrossTrainLagDecision, CrossTrainLagVariant,
};
use anyhow::Result;
use std::collections::BTreeSet;

const MIN_CONFIDENCE: f64 = 0.75;
const CURRENT_CONFIRM_THRESHOLD: f64 = 8.0;
const CHALLENGER_SUPERSEDE_THRESHOLD: f64 = 8.0;
const ROLLBACK_BLOCK_THRESHOLD: f64 = 4.5;
const LOW_CHALLENGER_THRESHOLD: f64 = 2.5;
const OVERDUE_CURRENT_THRESHOLD: f64 = 4.5;
const CHAIN_SUPERSEDE_THRESHOLD: f64 = 8.0;

#[derive(Debug, Default)]
pub struct CarryoverBudgetVariant;

impl CrossTrainLagVariant for CarryoverBudgetVariant {
    fn name(&self) -> &'static str {
        "carryover_budget"
    }

    fn style(&self) -> &'static str {
        "carryover budget"
    }

    fn philosophy(&self) -> &'static str {
        "Carry unresolved lag across release cuts only while the same challenger remains coherent and the debt has not exceeded its cross-train budget."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_train_lag_carryover/carryover_budget.rs"
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
                6,
            ));
        };

        Ok(match latest.kind.as_str() {
            "confirmed" => decision(
                "carryover_confirmed",
                Some(case.current_reference.clone()),
                format!(
                    "The latest train clears older lag debt by re-establishing the current reference: current_within={:.2}.",
                    latest.current_within
                ),
                6,
            ),
            "superseded" => decision(
                "carryover_superseded",
                latest.reference.clone(),
                format!(
                    "The latest train alone spends the carryover budget: challenger_score={:.2} challenger_surfaces={}.",
                    latest.challenger_score, latest.challenger_surfaces
                ),
                6,
            ),
            "blocked" => decision(
                "carryover_blocked",
                None,
                format!(
                    "The latest train blocks trust on its own: rollback_score={:.2} rollback_critical={}.",
                    latest.rollback_score, latest.rollback_critical
                ),
                6,
            ),
            _ => {
                let chain = consecutive_pending_chain(&states);
                let unique_challengers = chain
                    .iter()
                    .filter_map(|state| state.reference.clone())
                    .collect::<BTreeSet<_>>();
                let overdue_count = chain
                    .iter()
                    .filter(|state| state.current_overdue >= OVERDUE_CURRENT_THRESHOLD)
                    .count();
                let chain_score: f64 = chain.iter().map(|state| state.challenger_score).sum();
                let chain_critical: usize = chain.iter().map(|state| state.challenger_critical).sum();

                if chain.len() >= 2 && unique_challengers.len() == 1 {
                    let reference = unique_challengers.iter().next().cloned();
                    if chain_score >= CHAIN_SUPERSEDE_THRESHOLD
                        && (overdue_count >= 1 || chain_critical >= 2)
                    {
                        decision(
                            "carryover_superseded",
                            reference,
                            format!(
                                "The same challenger survived multiple pending cuts until the carryover budget was spent: chain_len={} chain_score={chain_score:.2} overdue_count={} chain_critical={}.",
                                chain.len(),
                                overdue_count,
                                chain_critical
                            ),
                            6,
                        )
                    } else {
                        decision(
                            "carryover_pending",
                            None,
                            format!(
                                "The pending chain is coherent but still inside its cross-train lag budget: chain_len={} chain_score={chain_score:.2} chain_critical={}.",
                                chain.len(),
                                chain_critical
                            ),
                            6,
                        )
                    }
                } else if chain.len() >= 2
                    && (unique_challengers.len() > 1 || overdue_count >= 2)
                {
                    decision(
                        "carryover_blocked",
                        None,
                        format!(
                            "Carryover debt spilled across cuts without a stable challenger: chain_len={} unique_challengers={} overdue_count={}.",
                            chain.len(),
                            unique_challengers.len(),
                            overdue_count
                        ),
                        6,
                    )
                } else if latest.current_overdue >= OVERDUE_CURRENT_THRESHOLD
                    && latest.challenger_score < CHAIN_SUPERSEDE_THRESHOLD
                {
                    decision(
                        "carryover_blocked",
                        None,
                        format!(
                            "The latest carryover train is overdue without enough challenger support: current_overdue={:.2} challenger_score={:.2}.",
                            latest.current_overdue, latest.challenger_score
                        ),
                        6,
                    )
                } else {
                    decision(
                        "carryover_pending",
                        None,
                        format!(
                            "The unresolved lag can still carry into the next cut: chain_len={} latest_current_within={:.2} latest_challenger_score={:.2}.",
                            chain.len(),
                            latest.current_within,
                            latest.challenger_score
                        ),
                        6,
                    )
                }
            }
        })
    }
}
