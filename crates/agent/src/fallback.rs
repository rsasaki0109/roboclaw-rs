use crate::{planner_for_provider, planner_from_env, LlmProvider, PlanDecision, Planner};
use anyhow::{anyhow, Result};
use roboclaw_skills::SkillCatalog;
use roboclaw_tools::{ExecutionControl, StopReason};
use std::path::PathBuf;
use std::sync::Mutex;

/// Builds providers only when selected, so an unavailable primary can fall back.
pub struct ProviderPlanner {
    pub provider: LlmProvider,
    pub prompt: PathBuf,
}

impl Planner for ProviderPlanner {
    fn plan(&self, instruction: String, catalog: &SkillCatalog) -> Result<PlanDecision> {
        planner_for_provider(&self.prompt, self.provider)?.plan(instruction, catalog)
    }

    fn plan_with_control(
        &self,
        instruction: String,
        catalog: &SkillCatalog,
        control: &ExecutionControl,
    ) -> Result<PlanDecision> {
        control.check()?;
        let planner = planner_for_provider(&self.prompt, self.provider);
        control.check()?;
        planner?.plan_with_control(instruction, catalog, control)
    }

    fn provider_name(&self) -> &'static str {
        self.provider.as_str()
    }
}

/// Resolves automatic provider discovery inside the controlled planning turn.
pub struct EnvPlanner {
    pub prompt: PathBuf,
    context: String,
    selected: Mutex<&'static str>,
}

impl EnvPlanner {
    pub fn new(prompt: PathBuf) -> Self {
        Self {
            prompt,
            context: String::new(),
            selected: Mutex::new("auto"),
        }
    }
    pub fn with_context(mut self, context: String) -> Self {
        self.context = context;
        self
    }
}

impl Planner for EnvPlanner {
    fn plan(&self, instruction: String, catalog: &SkillCatalog) -> Result<PlanDecision> {
        self.plan_with_control(instruction, catalog, &ExecutionControl::default())
    }
    fn plan_with_control(
        &self,
        instruction: String,
        catalog: &SkillCatalog,
        control: &ExecutionControl,
    ) -> Result<PlanDecision> {
        control.check()?;
        let planner = planner_from_env(&self.prompt);
        control.check()?;
        let planner = planner?;
        let instruction = if self.context.is_empty() || planner.provider_name() == "mock" {
            instruction
        } else {
            format!(
                "{instruction}\n\nConfigured workspace context (reference data):\n{}",
                self.context
            )
        };
        let decision = planner.plan_with_control(instruction, catalog, control)?;
        *self
            .selected
            .lock()
            .map_err(|_| anyhow!("planner state poisoned"))? = planner.provider_name();
        Ok(decision)
    }
    fn provider_name(&self) -> &'static str {
        self.selected.lock().map(|name| *name).unwrap_or("auto")
    }
}

/// Opt-in planning fallback; never retries a tool or an already executed skill.
pub struct FallbackPlanner {
    candidates: Vec<Box<dyn Planner>>,
    selected: Mutex<&'static str>,
}

impl FallbackPlanner {
    pub fn new(candidates: Vec<Box<dyn Planner>>) -> Result<Self> {
        let selected = candidates
            .first()
            .ok_or_else(|| anyhow!("planner fallback chain is empty"))?
            .provider_name();
        Ok(Self {
            candidates,
            selected: Mutex::new(selected),
        })
    }
}

impl Planner for FallbackPlanner {
    fn plan(&self, instruction: String, catalog: &SkillCatalog) -> Result<PlanDecision> {
        self.plan_with_control(instruction, catalog, &ExecutionControl::default())
    }

    fn plan_with_control(
        &self,
        instruction: String,
        catalog: &SkillCatalog,
        control: &ExecutionControl,
    ) -> Result<PlanDecision> {
        let mut attempted = Vec::new();
        let mut last_error = None;
        // Start at the selected primary on every planning turn, including recovery.
        for candidate in &self.candidates {
            control.check()?;
            attempted.push(candidate.provider_name());
            match candidate.plan_with_control(instruction.clone(), catalog, control) {
                Ok(mut decision) => {
                    control.check()?;
                    *self
                        .selected
                        .lock()
                        .map_err(|_| anyhow!("planner state poisoned"))? =
                        candidate.provider_name();
                    if attempted.len() > 1 {
                        decision.reason = Some(format!(
                            "fallback {}: {}",
                            attempted.join(" -> "),
                            decision.reason.as_deref().unwrap_or("selected skill")
                        ));
                    }
                    return Ok(decision);
                }
                Err(error) => {
                    control.check()?;
                    if error.downcast_ref::<StopReason>().is_some() {
                        return Err(error);
                    }
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap()).map_err(|error| {
            error.context(format!(
                "planner chain exhausted: {}",
                attempted.join(" -> ")
            ))
        })
    }

    fn provider_name(&self) -> &'static str {
        self.selected
            .lock()
            .map(|selected| *selected)
            .unwrap_or("fallback")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roboclaw_skills::Skill;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Candidate {
        name: &'static str,
        calls: Arc<AtomicUsize>,
        fail: bool,
        cancel: Option<ExecutionControl>,
    }
    impl Planner for Candidate {
        fn plan(&self, _instruction: String, _catalog: &SkillCatalog) -> Result<PlanDecision> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(control) = &self.cancel {
                control.cancel();
            }
            if self.fail {
                return Err(anyhow!("provider unavailable"));
            }
            Ok(PlanDecision {
                skill: Skill {
                    name: "test".into(),
                    description: "test".into(),
                    resume_original_instruction: false,
                    supports_checkpoint_resume: false,
                    recovery_for: vec![],
                    steps: vec![],
                },
                reason: Some("selected".into()),
            })
        }
        fn provider_name(&self) -> &'static str {
            self.name
        }
    }
    fn candidate(
        name: &'static str,
        fail: bool,
        cancel: Option<ExecutionControl>,
    ) -> (Box<dyn Planner>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Box::new(Candidate {
                name,
                calls: calls.clone(),
                fail,
                cancel,
            }),
            calls,
        )
    }
    #[test]
    fn fallback_records_winner_and_retries_primary_on_every_turn() {
        let (primary, primary_calls) = candidate("primary", true, None);
        let (fallback, fallback_calls) = candidate("fallback", false, None);
        let planner = FallbackPlanner::new(vec![primary, fallback]).unwrap();
        for _ in 0..2 {
            let decision = planner
                .plan("test".into(), &SkillCatalog::default())
                .unwrap();
            assert!(decision.reason.unwrap().contains("primary -> fallback"));
            assert_eq!(planner.provider_name(), "fallback");
        }
        assert_eq!(primary_calls.load(Ordering::SeqCst), 2);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn cancellation_never_advances_to_a_fallback() {
        let control = ExecutionControl::default();
        let (primary, _) = candidate("primary", true, Some(control.clone()));
        let (fallback, calls) = candidate("fallback", false, None);
        let planner = FallbackPlanner::new(vec![primary, fallback]).unwrap();
        let error = planner
            .plan_with_control("test".into(), &SkillCatalog::default(), &control)
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<StopReason>(),
            Some(&StopReason::Cancelled)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
    #[test]
    fn all_failed_candidates_return_a_bounded_chain_error() {
        let (a, _) = candidate("a", true, None);
        let (b, _) = candidate("b", true, None);
        let planner = FallbackPlanner::new(vec![a, b]).unwrap();
        let error = planner
            .plan("test".into(), &SkillCatalog::default())
            .unwrap_err();
        assert!(format!("{error:#}").contains("a -> b"));
        assert!(FallbackPlanner::new(Vec::new()).is_err());
    }
}
