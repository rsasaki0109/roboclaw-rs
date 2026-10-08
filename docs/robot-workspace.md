# Robot workspace guide

Build with `cargo build --locked --bin roboclaw`. The examples below use
`roboclaw` after installation (`cargo install --locked --path .`). Existing
`skills list`, `plan` and `run` commands continue to work with the mock planner
by default. `--project-dir PATH`, `--config FILE` and `--json` are global options.

## Configuration and diagnostics

```bash
roboclaw config init
roboclaw config check
roboclaw config show --json
roboclaw doctor --json
roboclaw skills validate
roboclaw tools list
```

Configuration is loaded at command/server startup. The optional
`PROJECT_DIR/roboclaw.yaml` supports:

```yaml
version: 1
planner:
  provider: mock
  fallbacks: []
skill_dirs: []
context_files: []
memory_recall: true
state_dir: target/roboclaw
timeout: null
max_replans: 1
tools:
  allow: null
  deny: []
```

Relative configuration paths resolve from the project directory. An explicit
`--config` path resolves from the working directory. Secrets and model endpoint
settings use the existing provider environment variables described in README;
they are not written by `config show` or `doctor`. Unknown configuration fields
are rejected. `doctor` reads assets and state without creating state folders or
sending robot/model requests. Credential presence is a configuration check,
not an authentication or network connectivity test. Diagnose local model
availability separately when relying on Ollama model discovery.

Additional skill directories are loaded in listed order, then project `skills/`.
Later directories override earlier skill names; project skills have highest
precedence. Duplicate names within a directory fail. A running instruction uses
a catalog snapshot; each new plan/run reloads YAML files. `skills validate`
checks tools, inputs, step names, retry bounds and checkpoint references.

`context_files` supplies up to 64KiB of explicit workspace reference text to
model planners. Keyword recall supplies up to five matching memory snippets
from the current session, at most 1,000 characters each. Set `memory_recall:
false` to disable it. Reference context is supplied to model planners; the mock
heuristic continues to use the current instruction directly.

## Models, policy and preflight

```bash
roboclaw plan "Wave the robot arm." --provider local --fallback openai --fallback mock
roboclaw run "Wave the robot arm." --no-fallbacks
```

A CLI provider selection is strict unless `--fallback` is also supplied.
Otherwise the primary and fallbacks come from configuration. The chain restarts
at the primary for every planning turn. Reports identify the actual provider
that selected the skill and record the fallback path in its reason. Cancellation
or deadline exhaustion ends the chain. Fallback is planning-only: tool failures
do not cause the instruction to be executed again under another provider.
There are at most four explicit fallback candidates; `auto` is primary-only.

`tools.allow: null` allows registered tools, while `tools.allow: []` allows none.
A deny always wins. For an observation-only configuration, use
`tools.allow: [sensor]`. Policy enforcement happens in the registry for both
normal and controlled tool calls. Before executing a selected skill (or resume
suffix), the agent validates all of its steps. A denied or invalid later step
therefore prevents earlier movement in that skill.

## Sessions, memory and event traces

```bash
roboclaw run "Wave the robot arm." --session lab --timeout 30s --json
roboclaw sessions list --json
roboclaw sessions show lab --json
roboclaw memory remember "The calibration marker is alpha42." --session lab
roboclaw memory search "alpha42" --session lab --json
roboclaw memory show --session lab --json
roboclaw runs list --json
roboclaw runs show RUN_ID --json
roboclaw runs events RUN_ID --after 0 --json
roboclaw run "Wave the robot arm." --session lab --stream
```

IDs contain 1–64 ASCII letters, digits, hyphens or underscores. A writer lock
rejects concurrent execution in the same session or memory directory. Main
session memory defaults to `target/cli-memory`; other sessions use
`STATE_DIR/sessions/SESSION/memory`. `--memory-dir` overrides memory storage and
its absolute path is saved with the session. Run journals stay under
`STATE_DIR/runs/RUN_ID` independently of that override. Plan/list/search commands
read state without initializing a backend or creating memory folders.

Normal run JSON retains the gateway fields and adds `run_id` and `session`.
Each run has an atomic record and an append-only `events.jsonl` trace. Trace
sequence numbers start at 1; `after=N` returns later events, at most 1,000 at a
time. Incomplete trailing event lines are ignored until their append completes.
`--stream` emits one JSON object per line with `type: event`, then a final
`type: result` (or `type: error`). It cannot be combined with `--json`.
Library users can pass an `EventObserver` to `Workspace::run` to handle events.
An observer error fails the run; callbacks should not perform blocking robot
operations. Observers receive events after memory persistence.

Runtime errors are also saved in run journals. On runner startup, abandoned
running records become `interrupted` if their session writer lock is available.
Current executions holding their locks are left alone. Raw history stays local;
explicitly selected model providers receive configured reference context.

## Durable jobs

```bash
roboclaw jobs add "Wave the robot arm." --after 10m --timeout 30s --json
roboclaw jobs add "Wave the robot arm." --every 5m --session lab --json
roboclaw jobs add "Wave the robot arm." --cron '0 9 * * MON-FRI' --timezone Asia/Tokyo --timeout 30s --json
roboclaw jobs list --json
roboclaw jobs show JOB_ID --json
roboclaw jobs cancel JOB_ID --json
roboclaw jobs run --json
```

Submission stores a queued job; execution starts only when a runner is active.
`jobs run` processes each due job at most once per invocation and exits; `gateway serve` continuously
runs due jobs. There is one runner per state directory. CLI execution is bounded
by `--limit` (default 100, maximum 1,000). Jobs use an isolated session per job
unless `--session` is supplied. A recurring isolated job reuses its own session.

The runner persists a `running` claim before starting operations. On restart,
unfinished jobs become `interrupted`, with no automatic replay. This provides
conservative dispatch after process loss, rather than exactly-once physical
robot effects. Submit a new job after inspecting the recorded state if work
needs to be attempted again.

Cancellation is persisted and a scoped monitor shares it with the execution
control token. Queued cancellation prevents dispatch; running cancellation is
cooperative. The monitor and execution worker are joined before returning.

Intervals must be at least 100ms. Successful interval jobs return to `queued`
with their next due time measured from completion.

`--cron` accepts five fields: **minute hour day-of-month month day-of-week**.
Use numbers, `*`, lists (`0,30`), ranges (`9-17`), or steps (`*/15`). Month names
`JAN`–`DEC` and weekday names `SUN`–`SAT` are accepted without case sensitivity;
Sunday is `0` or `7`. If both day fields are restricted, either can match
(standard cron OR behavior). Seconds, years, `@daily` and calendar extensions
such as `L`, `W`, `#` or `?` are rejected. A schedule must have a future occurrence.
`--timezone` takes an IANA name such as `Asia/Tokyo`, `America/New_York` or `UTC`;
the CLI and API default to UTC, independent of the host's timezone. It requires
`--cron`, which cannot be combined with `--after` or `--every`.

The first cron occurrence is strictly after submission. After successful work,
the next slot is strictly after both completion and the previous scheduled slot,
so a backward clock change cannot repeat that slot. When the runner was offline,
an overdue queued job runs once, then advances to a future slot; missed intervals
and cron slots do not produce a catch-up burst. Persisted timestamps remain Unix
milliseconds and `cron` records the expression and timezone. Existing job files
without `cron` continue to load.

Daylight saving transitions follow Croner's Vixie rules: a fixed time in a
spring-forward gap runs at the gap's end; a fixed time in a repeated hour runs
once, at its first occurrence. Wildcard/step time fields follow each matching
real minute, including both passes of a repeated hour. Date search supports
years before 5000. Timezone rules are bundled by `chrono-tz`; dependency updates
are needed when governments change those rules.

Failed, cancelled, interrupted and timed-out jobs stop repeating.
`last_run_status` and `result` expose the latest outcome while a successful job
waits for its next slot. If the next slot cannot be calculated, the job becomes
`failed` with a scheduling error; the successful run's result and
`last_run_status: completed` remain available. Stopping the runner during active work cancels it;
queued jobs remain available for a later start.

## Local gateway and dashboard

Choose a private token of at least 16 characters and export it as
`ROBOCLAW_GATEWAY_TOKEN`, then start:

```bash
roboclaw gateway serve --bind 127.0.0.1:18790
```

Open `http://127.0.0.1:18790` and enter the same token. The dashboard previews
plans, submits simulator jobs, cancels queued/running jobs, displays event
progress and inspects skills/diagnostics. The token stays in page memory.

The server binds only to loopback. All API endpoints require
`Authorization: Bearer TOKEN`; API requests also enforce the bound Host and
same-origin browser requests. Mutations require `Content-Type: application/json`.
Request bodies are limited to 64KiB. No cross-origin API access is enabled.
The static dashboard requires no token and exposes no workspace data on its own.
The gateway is a local development control plane, not a public deployment service.

| Method | Endpoint | Result |
| --- | --- | --- |
| GET | `/api/health`, `/api/doctor`, `/api/skills` | Health, configuration diagnostics, current skill catalog |
| POST | `/api/plan` | Plan a `RunRequest` without robot operations |
| GET | `/api/sessions`, `/api/sessions/ID` | Session metadata |
| GET | `/api/runs`, `/api/runs/ID` | Run records and outcomes |
| GET | `/api/runs/ID/events?after=N` | Cursor-based event trace |
| GET | `/api/jobs`, `/api/jobs/ID` | Scheduled job state |
| POST | `/api/jobs` | Submit a job (202) |
| POST | `/api/jobs/ID/cancel` | Persist cancellation |
| POST | `/api/memory/search` | Search one session's memory |

`RunRequest` contains `instruction`, optional `session` (default `main`),
`provider`, `fallbacks` and `timeout`. Job submission wraps it as:

```json
{
  "request": {"instruction": "Wave the robot arm.", "timeout": "30s"},
  "isolated": true
}
```

Optional `due_at` is Unix time in milliseconds; `every` is a duration string.
For calendar jobs, supply `cron` and optional `timezone` (default `UTC`) instead
of `due_at` and `every`:

```json
{
  "request": {"instruction": "Wave the robot arm.", "timeout": "30s"},
  "cron": "0 9 * * MON-FRI",
  "timezone": "Asia/Tokyo",
  "isolated": true
}
```

The dashboard's **Schedule a recurring task** form uses the displayed browser
timezone by default and shows each queued job's next time in its scheduled zone.
`isolated` defaults to true. To share a named session, specify it in the request
and set `isolated: false`. Memory search accepts `session`, `query`, and an
optional `limit` (maximum 100). Unknown JSON request fields are rejected.

The gateway queues operations through one job runner. Each simulator run starts
with fresh backend state; historical sessions are not live robot connections.
Ctrl+C stops the runner and cancels active work cooperatively. Blocking HTTP
cancellation is observed when the request returns; its timeout is capped by the
remaining execution budget. Exit codes for direct runs remain 0/1/2/124/130.
