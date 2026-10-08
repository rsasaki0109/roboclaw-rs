//! Durable run notifications. Delivery never invokes a planner or robot tool.
use crate::config::parse_duration;
use crate::runtime::Workspace;
use crate::storage::{now_millis, read_json, validate_id, write_json, Lease, RunRecord};
use crate::tools::ExecutionControl;
use anyhow::{anyhow, bail, Context, Result};
use reqwest::blocking::Client;
use reqwest::header::{HeaderValue, RETRY_AFTER};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebhookConfig {
    pub url: String,
    /// Optional bearer credential, read at dispatch time and never persisted.
    pub token_env: Option<String>,
    pub timeout: String,
    pub retry_delay: String,
    pub max_attempts: u32,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            token_env: None,
            timeout: "5s".into(),
            retry_delay: "5s".into(),
            max_attempts: 5,
        }
    }
}

impl WebhookConfig {
    pub fn validate(&self) -> Result<()> {
        let url = Url::parse(&self.url).map_err(|_| anyhow!("invalid webhook URL"))?;
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if url.host().is_none()
            || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            bail!("webhook URL requires HTTPS (HTTP allowed only for literal loopback/localhost), without userinfo or fragment");
        }
        if let Some(name) = &self.token_env {
            if name.is_empty()
                || name.len() > 128
                || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                || name.as_bytes()[0].is_ascii_digit()
            {
                bail!("webhook token_env must be an environment variable name");
            }
        }
        let timeout = parse_duration(&self.timeout)?;
        let retry = parse_duration(&self.retry_delay)?;
        if timeout > Duration::from_secs(30) {
            bail!("webhook timeout must not exceed 30s");
        }
        if !(Duration::from_secs(1)..=Duration::from_secs(3600)).contains(&retry) {
            bail!("webhook retry_delay must be between 1s and 1h");
        }
        if !(1..=20).contains(&self.max_attempts) {
            bail!("webhook max_attempts must be between 1 and 20");
        }
        Ok(())
    }

    pub(crate) fn authorization(&self) -> Result<Option<HeaderValue>> {
        self.token_env
            .as_ref()
            .map(|name| {
                let token = std::env::var(name).with_context(|| {
                    format!("webhook credential environment variable {name} is missing")
                })?;
                if token.trim().is_empty() {
                    bail!("webhook credential environment variable {name} is empty");
                }
                let mut header =
                    HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| {
                        anyhow!("webhook credential contains invalid header characters")
                    })?;
                header.set_sensitive(true);
                Ok(header)
            })
            .transpose()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delivery {
    pub id: String,
    pub run_id: String,
    pub status: String,
    pub attempts: u32,
    /// Total attempts when the last manual retry was requested. Legacy files use 0.
    #[serde(default)]
    pub attempts_at_retry: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub retries: Vec<ManualRetry>,
    pub next_attempt_at: u64,
    pub last_attempt_at: Option<u64>,
    pub delivered_at: Option<u64>,
    pub http_status: Option<u16>,
    pub error: Option<String>,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualRetry {
    pub requested_at: u64,
    pub attempts: u32,
    pub last_attempt_at: Option<u64>,
    pub http_status: Option<u16>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct RetryConflict;
impl std::fmt::Display for RetryConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("only failed webhook deliveries can be retried")
    }
}
impl std::error::Error for RetryConflict {}

impl Delivery {
    fn for_run(run: &RunRecord) -> Self {
        Self {
            id: run.id.clone(),
            run_id: run.id.clone(),
            status: "pending".into(),
            attempts: 0,
            attempts_at_retry: 0,
            retries: Vec::new(),
            next_attempt_at: run.finished_at.unwrap_or(run.started_at),
            last_attempt_at: None,
            delivered_at: None,
            http_status: None,
            error: None,
            // Instructions, memory, planner reasoning, errors and credentials stay local.
            payload: json!({
                "schema_version":1, "event":"run.finished", "delivery_id":run.id,
                "run_id":run.id, "session":run.session, "status":run.status,
                "started_at":run.started_at, "finished_at":run.finished_at,
                "backend_state":run.result.as_ref().map(|result| &result.backend_state),
            }),
        }
    }

    fn attempts_since_retry(&self) -> Result<u32> {
        self.attempts
            .checked_sub(self.attempts_at_retry)
            .context("stored webhook retry attempt count exceeds total attempts")
    }
}

#[derive(Debug, Clone)]
pub struct Webhooks {
    pub workspace: Workspace,
}

impl Webhooks {
    fn path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self
            .workspace
            .store
            .root
            .join("webhooks")
            .join(format!("{id}.json")))
    }

    /// Read-only view, including eligible terminal runs not materialized yet.
    /// Legacy runs and runs started without webhook configuration never opt in.
    pub fn list(&self) -> Result<Vec<Delivery>> {
        let mut deliveries = Vec::new();
        for run in self.workspace.store.runs()? {
            if !run.webhook || run.status == "running" || run.finished_at.is_none() {
                continue;
            }
            let path = self.path(&run.id)?;
            let delivery: Delivery = if path.exists() {
                read_json(&path)?
            } else {
                Delivery::for_run(&run)
            };
            if delivery.id != run.id || delivery.run_id != run.id {
                bail!("stored webhook delivery does not match its run ID");
            }
            delivery.attempts_since_retry()?;
            deliveries.push(delivery);
        }
        deliveries.sort_by(|a, b| {
            a.next_attempt_at
                .cmp(&b.next_attempt_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(deliveries)
    }

    pub fn get(&self, id: &str) -> Result<Delivery> {
        validate_id(id)?;
        self.list()?
            .into_iter()
            .find(|delivery| delivery.id == id)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "webhook delivery not found")
                    .into()
            })
    }

    /// Requeue failed delivery with a fresh bounded retry budget. Preserve its
    /// payload, ID, total attempts and previous failure; never send HTTP here.
    pub fn retry(&self, id: &str) -> Result<Delivery> {
        validate_id(id)?;
        let config = self
            .workspace
            .config
            .webhook
            .as_ref()
            .context("webhook is not configured")?;
        config.validate()?;
        config.authorization()?;
        let _lease = Lease::acquire_wait(
            &self.workspace.store.root.join(".webhooks.lock"),
            Duration::from_millis(100),
        )?;
        let mut delivery = self.get(id)?;
        if delivery.status != "failed" {
            return Err(RetryConflict.into());
        }
        if delivery.attempts == u32::MAX {
            bail!("webhook total attempt count is exhausted");
        }
        let requested_at = now_millis();
        delivery.retries.push(ManualRetry {
            requested_at,
            attempts: delivery.attempts,
            last_attempt_at: delivery.last_attempt_at,
            http_status: delivery.http_status,
            error: delivery.error.take(),
        });
        delivery.attempts_at_retry = delivery.attempts;
        delivery.http_status = None;
        delivery.status = "pending".into();
        delivery.next_attempt_at = requested_at;
        write_json(&self.path(id)?, &delivery)?;
        Ok(delivery)
    }

    /// Send each currently due notification at most once in this call. Persist
    /// claims before HTTP, so a crash retries the same delivery ID, never the run.
    pub fn dispatch(&self, limit: usize, shutdown: &ExecutionControl) -> Result<Vec<Delivery>> {
        if limit == 0 || limit > 1000 {
            bail!("webhook limit must be between 1 and 1000");
        }
        let config = self
            .workspace
            .config
            .webhook
            .as_ref()
            .context("webhook is not configured")?;
        config.validate()?;
        let authorization = config.authorization()?;
        if shutdown.stop_reason().is_some() {
            return Ok(Vec::new());
        }
        let _lease = Lease::acquire(&self.workspace.store.root.join(".webhooks.lock"))?;
        let timeout = parse_duration(&config.timeout)?;
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(timeout.min(Duration::from_secs(3)))
            .timeout(timeout)
            .build()?;
        let now = now_millis();
        let mut outcomes = Vec::new();
        for mut delivery in self.list()? {
            if shutdown.stop_reason().is_some() || outcomes.len() >= limit {
                break;
            }
            if delivery.status == "sending" {
                // The receiver may already have accepted a request before process
                // death. Count that attempt and retain the stable idempotency key.
                delivery.status = if delivery.attempts_since_retry()? >= config.max_attempts {
                    "failed"
                } else {
                    "pending"
                }
                .into();
                delivery.error = Some("previous delivery ended without acknowledgement; receiver may have accepted it".into());
                write_json(&self.path(&delivery.id)?, &delivery)?;
                if delivery.status == "failed" {
                    outcomes.push(delivery);
                    continue;
                }
            }
            if delivery.status != "pending" || delivery.next_attempt_at > now {
                continue;
            }
            if delivery.attempts_since_retry()? >= config.max_attempts {
                delivery.status = "failed".into();
                write_json(&self.path(&delivery.id)?, &delivery)?;
                outcomes.push(delivery);
                continue;
            }
            delivery.status = "sending".into();
            delivery.attempts = delivery
                .attempts
                .checked_add(1)
                .context("webhook total attempt count is exhausted")?;
            delivery.last_attempt_at = Some(now_millis());
            write_json(&self.path(&delivery.id)?, &delivery)?;
            let mut request = client
                .post(&config.url)
                .header("Idempotency-Key", &delivery.id)
                .header("X-RoboClaw-Event", "run.finished")
                .json(&delivery.payload);
            if let Some(header) = &authorization {
                request = request.header(reqwest::header::AUTHORIZATION, header.clone());
            }
            let mut retry_after = 0;
            let retryable = match request.send() {
                Ok(response) => {
                    let status = response.status();
                    delivery.http_status = Some(status.as_u16());
                    if status.is_success() {
                        delivery.status = "delivered".into();
                        delivery.delivered_at = Some(now_millis());
                        delivery.error = None;
                        false
                    } else {
                        delivery.error =
                            Some(format!("receiver returned HTTP {}", status.as_u16()));
                        retry_after = response
                            .headers()
                            .get(RETRY_AFTER)
                            .and_then(|value| value.to_str().ok())
                            .and_then(|value| value.parse::<u64>().ok())
                            .unwrap_or(0)
                            .min(3600)
                            * 1000;
                        status.is_server_error() || [408, 425, 429].contains(&status.as_u16())
                    }
                }
                Err(error) => {
                    delivery.http_status = None;
                    // Do not persist request URLs, credentials or response bodies.
                    delivery.error = Some(
                        if error.is_timeout() {
                            "request timed out"
                        } else {
                            "transport error"
                        }
                        .into(),
                    );
                    true
                }
            };
            if delivery.status != "delivered" {
                delivery.status =
                    if retryable && delivery.attempts_since_retry()? < config.max_attempts {
                        "pending"
                    } else {
                        "failed"
                    }
                    .into();
                if delivery.status == "pending" {
                    let base = u64::try_from(parse_duration(&config.retry_delay)?.as_millis())?;
                    let backoff = base
                        .saturating_mul(1u64 << (delivery.attempts_since_retry()? - 1))
                        .min(3_600_000);
                    delivery.next_attempt_at =
                        now_millis().saturating_add(backoff.max(retry_after));
                }
            }
            write_json(&self.path(&delivery.id)?, &delivery)?;
            outcomes.push(delivery);
        }
        Ok(outcomes)
    }
}
