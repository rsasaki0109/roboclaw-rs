use super::{finish, stop_failed, GatewayCase, GatewayDecision, GatewayLoopVariant};
use anyhow::Result;

#[derive(Debug, Default)]
pub struct FailFastVariant;

impl GatewayLoopVariant for FailFastVariant {
    fn name(&self) -> &'static str {
        "fail_fast"
    }

    fn style(&self) -> &'static str {
        "stop on error"
    }

    fn philosophy(&self) -> &'static str {
        "Prefer operational simplicity: any failure stops the loop immediately."
    }

    fn source_path(&self) -> &'static str {
        "experiments/gateway_replanning/fail_fast.rs"
    }

    fn decide(&self, case: &GatewayCase) -> Result<GatewayDecision> {
        if case.completed {
            return Ok(finish("any completed run ends the loop"));
        }

        Ok(stop_failed(
            "fail-fast policy stops on first failed execution",
        ))
    }
}
