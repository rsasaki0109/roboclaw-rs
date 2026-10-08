use crate::config::parse_duration;
use crate::jobs::Jobs;
use crate::memory::Memory;
use crate::runtime::{RunRequest, Workspace};
use crate::storage::now_millis;
use crate::tools::ExecutionControl;
use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Read;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

fn isolated_default() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    request: RunRequest,
    #[serde(default)]
    due_at: Option<u64>,
    #[serde(default)]
    every: Option<String>,
    #[serde(default = "isolated_default")]
    isolated: bool,
}

/// Local control plane. Robot operations are serialized by the durable job runner.
pub fn serve(
    workspace: Workspace,
    address: SocketAddr,
    token: String,
    shutdown: ExecutionControl,
) -> Result<()> {
    if !address.ip().is_loopback() {
        bail!("gateway must bind to a loopback address");
    }
    if token.len() < 16 {
        bail!("ROBOCLAW_GATEWAY_TOKEN must contain at least 16 characters");
    }
    let jobs = Jobs {
        workspace: workspace.clone(),
    };
    let _runner_lease = jobs.runner_lease()?;
    jobs.recover_interrupted()?;
    let server = Arc::new(
        Server::http(address).map_err(|error| anyhow!("failed to bind gateway: {error}"))?,
    );
    let bound = server
        .server_addr()
        .to_ip()
        .ok_or_else(|| anyhow!("gateway did not bind TCP"))?;
    eprintln!("gateway_url=http://{bound}");
    std::thread::scope(|scope| -> Result<()> {
        let worker = scope.spawn(|| -> Result<()> {
            while shutdown.stop_reason().is_none() {
                if let Some(job) = jobs.claim_due(now_millis())? {
                    jobs.execute(job, &shutdown, None)?;
                } else {
                    let _ = shutdown.wait(Duration::from_millis(100));
                }
            }
            Ok(())
        });
        let mut handlers = Vec::new();
        for _ in 0..4 {
            let server = server.clone();
            let workspace = &workspace;
            let token = &token;
            let shutdown = &shutdown;
            handlers.push(scope.spawn(move || -> Result<()> {
                while shutdown.stop_reason().is_none() {
                    if let Some(request) = server.recv_timeout(Duration::from_millis(100))? {
                        respond(request, workspace, token, bound);
                    }
                }
                Ok(())
            }));
        }
        let result = worker.join().map_err(|_| anyhow!("job runner panicked"));
        shutdown.cancel();
        for handler in handlers {
            handler
                .join()
                .map_err(|_| anyhow!("gateway handler panicked"))??;
        }
        result?
    })
}

fn header<'a>(request: &'a Request, name: &'static str) -> Option<&'a str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str())
}

fn respond(mut request: Request, workspace: &Workspace, token: &str, bound: SocketAddr) {
    let url = request.url().to_string();
    let path = url.split('?').next().unwrap_or(&url);
    let response = if path.starts_with("/api/") {
        let host = header(&request, "Host").unwrap_or("");
        let valid_host = host == bound.to_string() || host == format!("localhost:{}", bound.port());
        let valid_origin =
            header(&request, "Origin").is_none_or(|origin| origin == format!("http://{host}"));
        let authorized = header(&request, "Authorization").is_some_and(|value| {
            constant_time_eq(value.as_bytes(), format!("Bearer {token}").as_bytes())
        });
        if !valid_host || !valid_origin || !authorized {
            json_response(401, &json!({"error": "unauthorized"}))
        } else if request.body_length().is_some_and(|length| length > 65_536) {
            json_response(413, &json!({"error": "request body exceeds 64KiB"}))
        } else if request.method() == &Method::Post
            && !header(&request, "Content-Type")
                .is_some_and(|value| value.split(';').next() == Some("application/json"))
        {
            json_response(415, &json!({"error": "use application/json"}))
        } else {
            match api(&mut request, workspace, &url) {
                Ok((status, value)) => json_response(status, &value),
                Err(error) => {
                    let status = if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                    {
                        404
                    } else if format!("{error:#}").contains("resource is busy") {
                        409
                    } else {
                        400
                    };
                    json_response(status, &json!({"error": format!("{error:#}")}))
                }
            }
        }
    } else if request.method() != &Method::Get {
        json_response(405, &json!({"error": "method not allowed"}))
    } else {
        match path {
            "/" => static_response(
                include_str!("../dashboard/index.html"),
                "text/html; charset=utf-8",
            ),
            "/favicon.svg" => static_response(include_str!("../site/favicon.svg"), "image/svg+xml"),
            "/app.js" => static_response(
                include_str!("../dashboard/app.js"),
                "text/javascript; charset=utf-8",
            ),
            "/style.css" => static_response(
                include_str!("../dashboard/style.css"),
                "text/css; charset=utf-8",
            ),
            _ => json_response(404, &json!({"error": "not found"})),
        }
    };
    let _ = request.respond(response);
}

fn api(request: &mut Request, workspace: &Workspace, url: &str) -> Result<(u16, Value)> {
    let parts: Vec<_> = url
        .split('?')
        .next()
        .unwrap_or(url)
        .trim_matches('/')
        .split('/')
        .collect();
    let query = url.split_once('?').map(|(_, query)| query).unwrap_or("");
    let jobs = Jobs {
        workspace: workspace.clone(),
    };
    let value = match (request.method(), parts.as_slice()) {
        (&Method::Get, ["api", "health"]) => {
            json!({"status": "ok", "backend": "in-process simulator", "version": env!("CARGO_PKG_VERSION")})
        }
        (&Method::Get, ["api", "doctor"]) => {
            serde_json::to_value(crate::doctor::inspect(workspace))?
        }
        (&Method::Get, ["api", "skills"]) => {
            serde_json::to_value(workspace.catalog()?.values().collect::<Vec<_>>())?
        }
        (&Method::Get, ["api", "sessions"]) => serde_json::to_value(workspace.store.sessions()?)?,
        (&Method::Get, ["api", "sessions", id]) => {
            serde_json::to_value(workspace.store.session(id)?)?
        }
        (&Method::Get, ["api", "runs"]) => serde_json::to_value(workspace.store.runs()?)?,
        (&Method::Get, ["api", "runs", id]) => serde_json::to_value(workspace.store.run(id)?)?,
        (&Method::Get, ["api", "runs", id, "events"]) => {
            let after = query
                .split('&')
                .find_map(|part| part.strip_prefix("after="))
                .unwrap_or("0")
                .parse()?;
            serde_json::to_value(workspace.store.events(id, after, 1000)?)?
        }
        (&Method::Get, ["api", "jobs"]) => serde_json::to_value(jobs.list()?)?,
        (&Method::Get, ["api", "jobs", id]) => serde_json::to_value(jobs.get(id)?)?,
        (&Method::Post, ["api", "jobs", id, "cancel"]) => serde_json::to_value(jobs.cancel(id)?)?,
        (&Method::Post, ["api", "jobs"]) => {
            let submission: Submission = body(request)?;
            let interval = submission
                .every
                .as_deref()
                .map(parse_duration)
                .transpose()?;
            return Ok((
                202,
                serde_json::to_value(jobs.add(
                    submission.request,
                    submission.due_at.unwrap_or_else(now_millis),
                    interval,
                    submission.isolated,
                )?)?,
            ));
        }
        (&Method::Post, ["api", "plan"]) => serde_json::to_value(workspace.plan(body(request)?)?)?,
        (&Method::Post, ["api", "memory", "search"]) => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Search {
                session: String,
                query: String,
                #[serde(default)]
                limit: Option<usize>,
            }
            let search: Search = body(request)?;
            let memory = Memory::open_readonly(workspace.memory_path(&search.session)?)?;
            serde_json::to_value(memory.search(&search.query, search.limit.unwrap_or(20).min(100)))?
        }
        _ => return Ok((404, json!({"error": "not found"}))),
    };
    Ok((200, value))
}

fn body<T: serde::de::DeserializeOwned>(request: &mut Request) -> Result<T> {
    let mut bytes = Vec::new();
    request.as_reader().take(65_537).read_to_end(&mut bytes)?;
    if bytes.len() > 65_536 {
        bail!("request body exceeds 64KiB");
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |diff, (a, b)| diff | (a ^ b)) == 0
}

fn json_response(status: u16, value: &Value) -> Response<std::io::Cursor<Vec<u8>>> {
    static_response(&value.to_string(), "application/json").with_status_code(StatusCode(status))
}

fn static_response(content: &str, kind: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(content)
        .with_header(Header::from_bytes("Content-Type", kind).unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
        .with_header(Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap())
        .with_header(Header::from_bytes("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'").unwrap())
}
