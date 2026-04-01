use super::{
    decision, freshest_signal, ProvenanceLagCase, ProvenanceLagDecision, ProvenanceLagVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct FreshestSignalWinsVariant;

impl ProvenanceLagVariant for FreshestSignalWinsVariant {
    fn name(&self) -> &'static str {
        "freshest_signal_wins"
    }

    fn style(&self) -> &'static str {
        "freshest signal"
    }

    fn philosophy(&self) -> &'static str {
        "Trust whichever publication surface updated first in the latest train."
    }

    fn source_path(&self) -> &'static str {
        "experiments/provenance_lag_budgets/freshest_signal_wins.rs"
    }

    fn decide(&self, case: &ProvenanceLagCase) -> Result<ProvenanceLagDecision> {
        let Some(signal) = freshest_signal(case) else {
            return Ok(decision(
                "lag_pending",
                None,
                "No publication surfaces reported for the latest train.",
                3,
            ));
        };

        Ok(if signal.decision_kind == "rollback_reference" {
            decision(
                "lag_blocked",
                None,
                "The freshest publication signal is a rollback.",
                3,
            )
        } else if signal.reference.as_deref() == Some(case.current_reference.as_str()) {
            decision(
                "lag_confirmed",
                Some(case.current_reference.clone()),
                "The freshest publication signal still supports the current reference.",
                3,
            )
        } else if let Some(reference) = &signal.reference {
            decision(
                "lag_superseded",
                Some(reference.clone()),
                "The freshest publication signal supports a challenger reference.",
                3,
            )
        } else {
            decision(
                "lag_pending",
                None,
                "The freshest publication signal does not expose a stable reference.",
                3,
            )
        })
    }
}
