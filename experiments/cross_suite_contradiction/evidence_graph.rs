use super::{
    decision, frontier_contains, has_single_frontier, ContradictionCase, ContradictionDecision,
    ContradictionVariant,
};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct EvidenceGraphVariant;

impl ContradictionVariant for EvidenceGraphVariant {
    fn name(&self) -> &'static str {
        "evidence_graph"
    }

    fn style(&self) -> &'static str {
        "evidence graph"
    }

    fn philosophy(&self) -> &'static str {
        "Look at frontier membership, replay state, support drift, and runtime risk together before labeling a contradiction."
    }

    fn source_path(&self) -> &'static str {
        "experiments/cross_suite_contradiction/evidence_graph.rs"
    }

    fn detect(&self, case: &ContradictionCase) -> Result<ContradictionDecision> {
        let Some(reference) = case.promotion_reference.as_deref() else {
            return Ok(decision(
                "no_contradiction",
                None,
                "A hold decision leaves the suite in an explicitly experimental state.",
                6,
            ));
        };

        Ok(if !frontier_contains(case, reference) {
            decision(
                "reference_conflict",
                Some(reference.to_string()),
                "Promotion chose a reference that local suite evidence never placed on the frontier.",
                6,
            )
        } else if case.promotion_decision_kind == "hold_experimental" {
            decision(
                "no_contradiction",
                None,
                "Promotion stayed in experimental mode.",
                6,
            )
        } else if case.replay_signal == "hold_experimental" {
            if case.environment_specific || case.support_gap <= 0 || case.runtime_risk >= 70 {
                decision(
                    "risk_conflict",
                    Some(reference.to_string()),
                    "Promotion ignored runtime-bound or weakly supported evidence.",
                    6,
                )
            } else {
                decision(
                    "premature_promotion",
                    Some(reference.to_string()),
                    "Promotion moved before replay had converged, even though the suite is otherwise stable enough to compare.",
                    6,
                )
            }
        } else if case.promotion_decision_kind == "switch_reference"
            && has_single_frontier(case)
            && case.provisional_reference.as_deref() != Some(reference)
        {
            decision(
                "reference_conflict",
                Some(reference.to_string()),
                "A single-frontier suite switched away from its own only frontier member.",
                6,
            )
        } else {
            decision(
                "no_contradiction",
                None,
                "Replay, promotion, and local frontier remain compatible.",
                6,
            )
        })
    }
}
