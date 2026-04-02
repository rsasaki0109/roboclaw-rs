use super::{challenger_is_frontier, decision, RollbackCase, RollbackDecision, RollbackVariant};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct IncidentReactiveVariant;

impl RollbackVariant for IncidentReactiveVariant {
    fn name(&self) -> &'static str {
        "incident_reactive"
    }

    fn style(&self) -> &'static str {
        "incident reactive"
    }

    fn philosophy(&self) -> &'static str {
        "Prioritize runtime incidents and explicit risk conflicts over slower frontier drift."
    }

    fn source_path(&self) -> &'static str {
        "experiments/rollback_rules/incident_reactive.rs"
    }

    fn decide(&self, case: &RollbackCase) -> Result<RollbackDecision> {
        Ok(
            if case.runtime_incident_severity >= 70 || case.contradiction_signal == "risk_conflict"
            {
                decision(
                    "rollback_reference",
                    Some(case.promoted_reference.clone()),
                    "Runtime incidents or contradiction risk force an immediate rollback.",
                    4,
                )
            } else if case.replay_signal == "switch_reference" && challenger_is_frontier(case) {
                decision(
                    "replace_reference",
                    Some(case.challenger_reference.clone()),
                    "Replay already points at a replacement and the challenger is on the frontier.",
                    4,
                )
            } else {
                decision(
                    "keep_promoted",
                    Some(case.promoted_reference.clone()),
                    "No incident threshold was crossed, so the promoted surface stays.",
                    4,
                )
            },
        )
    }
}
