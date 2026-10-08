# Run result webhooks

Webhooks publish terminal run results to a receiver you configure. They are
disabled by default. Set `webhook` in `roboclaw.yaml` before starting new runs:

```yaml
webhook:
  url: https://receiver.example.com/robot-results
  token_env: ROBOCLAW_WEBHOOK_TOKEN
  timeout: 5s
  retry_delay: 5s
  max_attempts: 5
```

Export the receiver's bearer credential as `ROBOCLAW_WEBHOOK_TOKEN`. `token_env`
is optional for receivers without authentication; credential values are read
at dispatch time and never stored in run records, delivery records or payloads.
`doctor` checks credential presence and header validity without contacting the
receiver. It also reports pending and failed notification counts.

Endpoints require HTTPS, except `http://localhost`, literal IPv4 loopback and
`http://[::1]` for local development. URL userinfo and fragments are rejected.
Redirects are never followed, and environment HTTP proxies are not used.
`timeout` must be positive and at most 30s; `retry_delay` must be between 1s and
1h, and `max_attempts` between 1 and 20. Unknown config fields are rejected.

## Dispatch and inspection

`gateway serve` dispatches notifications in its own joined worker, independently
of the robot job runner. A slow or unavailable receiver does not block subsequent
robot tasks or change their outcomes. Direct CLI runs and `jobs run` only save
results; run the dispatcher manually, or keep a gateway using the same state
directory and webhook configuration active:

```bash
roboclaw run "Wave the robot arm." --session lab --json
roboclaw webhooks list --json
roboclaw webhooks show RUN_ID --json
roboclaw webhooks retry RUN_ID --json
roboclaw webhooks dispatch --limit 100 --json
```

Manual dispatch sends each currently due notification at most once and exits.
Its limit defaults to 100, with a maximum of 1,000. Exit code 1 means an attempted
delivery failed or remains pending; 130 means dispatch was cancelled. A file
lock prevents concurrent senders in one state directory. The gateway and CLI
therefore cannot send the same notification simultaneously. Ctrl+C stops new
requests; an in-flight blocking HTTP request finishes or reaches its timeout
before the dispatcher joins.

The authenticated `GET /api/webhooks` and `GET /api/webhooks/RUN_ID` endpoints
return delivery state. The dashboard's **Notifications** section shows status,
attempt count and a sanitized error. Listing and inspection are read-only and
do not create files or send network requests.

## Manually retry a failed notification

After correcting the receiver, credentials or target configuration, use
`webhooks retry RUN_ID` or the dashboard's **Retry notification** button.
The authenticated `POST /api/webhooks/RUN_ID/retry` accepts an empty JSON object
and returns 202 with the queued delivery. Invalid or unknown fields are rejected;
requests for pending, sending or delivered notifications return 409. Only a
terminal `failed` notification can be requeued. The same sender lock protects
retry and dispatch, so retry may return 409 while a dispatcher holds the lock.

Retry validates the active configuration and credentials, then queues the
notification without making an HTTP request. A gateway picks it up automatically;
otherwise run `webhooks dispatch`. Retry keeps the immutable payload, delivery
ID and total `attempts`, and grants a fresh `max_attempts` budget. Each retry
appends the previous failure's HTTP status, sanitized error, last attempt time,
total attempts and request time to `retries`. `attempts_at_retry` records the
start of the new budget; old delivery files without these fields default to
zero and an empty history. Automatic backoff restarts at `retry_delay`.

The dashboard displays the previous failures in **Retry history**. Failed
attempts after a manual retry can be retried again; prior histories remain.
As with automatic delivery, receivers must deduplicate by the same delivery ID
if they accepted a request whose acknowledgement was lost. Robot runs and
commands are never recreated by a notification retry.

## Payload and delivery identity

Each eligible run has one stable `delivery_id`, equal to its `run_id`. Every
attempt sends JSON with `Idempotency-Key: DELIVERY_ID` and
`X-RoboClaw-Event: run.finished`. `Authorization: Bearer TOKEN` is included only
when `token_env` is configured.

```json
{
  "schema_version": 1,
  "event": "run.finished",
  "delivery_id": "RUN_ID",
  "run_id": "RUN_ID",
  "session": "lab",
  "status": "completed",
  "started_at": 1791446400000,
  "finished_at": 1791446401000,
  "backend_state": {
    "backend": "gazebo",
    "last_action": "wave",
    "last_pose": "home",
    "held_object": null
  }
}
```

Session names and backend state are sent to the configured receiver. Instruction
text, memory, planner reasoning, tool reports, local errors and credentials are
excluded. Backend state can be null when execution failed before producing a
result. Complete details remain in the local run journal.

Runs started while a webhook is configured persist a boolean opt-in marker
before operations. Only opted-in runs with a terminal status and `finished_at`
are eligible. Existing historical files without this marker are excluded.
Completed, failed, cancelled and timed-out runs are eligible; recovered
`interrupted` runs are also eligible after the runner records their interruption.
Cancelling a queued job produces no run and therefore no run notification.
Recurring jobs produce a separate notification for each actual run.

## Retry and recovery

Delivery states are `pending`, `sending`, `delivered` and `failed`, stored
atomically under `STATE_DIR/webhooks/RUN_ID.json`. The immutable payload is
materialized from the persisted terminal run. Keep the run journals as well as
the delivery records: inspection and discovery use those journals. If the
process stops between saving the terminal run and creating the delivery file,
the next dispatcher discovers it without losing the notification.

An attempt is persisted as `sending` before HTTP starts. Any 2xx response
acknowledges delivery. Transport errors, timeouts, HTTP 408/425/429 and 5xx
responses retry with exponential backoff, capped at one hour. A numeric
`Retry-After` in seconds can extend the delay up to one hour; HTTP-date values
are not interpreted. Other responses, including redirects and authentication
errors, become terminal `failed` deliveries. Exhausting attempts also stops
delivery. Response bodies, request URLs and credentials are not stored as errors.
Missing or invalid credentials preserve the pending notification without
consuming an attempt.

If a sender dies before persisting acknowledgement, the next dispatcher retries
the same ID, counting the previous attempt. The receiver may already have
accepted it. Receivers must deduplicate by `delivery_id` for duplicate-safe
handling; delivery does not promise exactly-once receipt or unlimited retries.
Notification failures and retries never invoke a planner or robot operation.

The active configuration supplies the target and credentials for pending
deliveries. Changing the target reroutes pending deliveries when the dispatcher
reloads configuration; delivered notifications are not resent. Removing the
configuration pauses dispatch. Inspect pending records before changing targets.
Gateway configuration is loaded at startup, so restart it to apply changes.
