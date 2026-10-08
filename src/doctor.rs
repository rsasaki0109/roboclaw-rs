use crate::runtime::Workspace;
use anyhow::Result;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub checks: Vec<Check>,
}

pub fn diagnose(project: &Path, config: Option<&Path>) -> DoctorReport {
    match Workspace::load(project, config) {
        Ok(workspace) => inspect(&workspace),
        Err(error) => DoctorReport {
            ok: false,
            checks: vec![Check {
                name: "configuration".into(),
                ok: false,
                detail: format!("{error:#}"),
            }],
        },
    }
}

pub fn inspect(workspace: &Workspace) -> DoctorReport {
    let mut checks = vec![Check {
        name: "configuration".into(),
        ok: true,
        detail: "configuration is valid".into(),
    }];
    let mut check = |name: &str, result: Result<String>| {
        checks.push(match result {
            Ok(detail) => Check {
                name: name.into(),
                ok: true,
                detail,
            },
            Err(error) => Check {
                name: name.into(),
                ok: false,
                detail: format!("{error:#}"),
            },
        });
    };
    check(
        "skills",
        workspace
            .validate_skills()
            .map(|catalog| format!("{} valid skills", catalog.names().len())),
    );
    check(
        "prompt",
        std::fs::read_to_string(workspace.project.join("prompts/planner_prompt.txt"))
            .map(|_| "planner prompt is readable".into())
            .map_err(Into::into),
    );
    check(
        "workspace_context",
        workspace
            .context()
            .map(|context| format!("{} bytes of configured context", context.len())),
    );
    check(
        "sessions",
        workspace
            .store
            .sessions()
            .map(|sessions| format!("{} stored sessions", sessions.len())),
    );
    check(
        "runs",
        workspace.store.runs().map(|runs| {
            format!(
                "{} stored runs; {} incomplete records",
                runs.len(),
                runs.iter()
                    .filter(|run| run.status == "running" || run.status == "interrupted")
                    .count()
            )
        }),
    );
    check(
        "jobs",
        crate::jobs::Jobs {
            workspace: workspace.clone(),
        }
        .list()
        .map(|jobs| format!("{} stored jobs", jobs.len())),
    );
    if let Some(config) = &workspace.config.webhook {
        check(
            "webhook_credentials",
            config
                .authorization()
                .map(|_| "credential configuration is ready; endpoint is not contacted".into()),
        );
        check(
            "webhook_deliveries",
            crate::webhooks::Webhooks {
                workspace: workspace.clone(),
            }
            .list()
            .map(|deliveries| {
                format!(
                    "{} deliveries; {} pending; {} failed",
                    deliveries.len(),
                    deliveries
                        .iter()
                        .filter(
                            |delivery| delivery.status == "pending" || delivery.status == "sending"
                        )
                        .count(),
                    deliveries
                        .iter()
                        .filter(|delivery| delivery.status == "failed")
                        .count()
                )
            }),
        );
    }
    let names = std::iter::once(workspace.config.planner.provider.as_str()).chain(
        workspace
            .config
            .planner
            .fallbacks
            .iter()
            .map(String::as_str),
    );
    let configured: Vec<_> = names
        .map(|provider| {
            let ready = match provider {
                "mock" | "auto" => true,
                "local" => env_present("ROBOCLAW_OLLAMA_MODEL") || env_present("OLLAMA_MODEL"),
                "openai" => env_present("ROBOCLAW_OPENAI_API_KEY") || env_present("OPENAI_API_KEY"),
                "claude" => {
                    env_present("ROBOCLAW_CLAUDE_API_KEY") || env_present("ANTHROPIC_API_KEY")
                }
                _ => false,
            };
            (provider, ready)
        })
        .collect();
    checks.push(Check {
        name: "models".into(),
        ok: configured.iter().any(|(_, ready)| *ready),
        detail: format!(
            "configuration only; network access and credentials are not verified: {}",
            configured
                .iter()
                .map(|(name, ready)| format!("{name}={ready}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    });
    for path in &workspace.config.context_files {
        let result = std::fs::metadata(workspace.project.join(path));
        checks.push(Check {
            name: format!("context:{path:?}"),
            ok: result
                .as_ref()
                .is_ok_and(|meta| meta.is_file() && meta.len() <= 65_536),
            detail: match result {
                Ok(_) => "context file inspected".into(),
                Err(error) => error.to_string(),
            },
        });
    }
    DoctorReport {
        ok: checks.iter().all(|check| check.ok),
        checks,
    }
}

fn env_present(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| !value.trim().is_empty())
}
