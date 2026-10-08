# Recovery challenges

Run controlled recovery experiments against the in-process robot simulator. The
mission is to pick up `red_cube` and place it in `bin_a`. Compare your workspace's
skills and planners with repeatable failures, then inspect the normal run traces
to understand why a trial passed or failed.

```bash
# No model credentials needed. Default: 4 scenarios × 2 strategies × 3 repeats.
roboclaw challenges run --json

# Include an unrecoverable fault and a deadline check.
roboclaw challenges run --scenario persistent-stall,sensor-timeout --timeout 1s --repeats 1 --json

# Compare explicit planners using configured model endpoints and credentials.
roboclaw challenges run --provider mock,local --timeout 30s --repeats 2 --json

# Queue for an active gateway; inspect or cancel without running operations.
roboclaw challenges submit --repeats 1 --json
roboclaw challenges list --json
roboclaw challenges show CHALLENGE_ID --json
roboclaw challenges cancel CHALLENGE_ID --json
roboclaw challenges scenarios --json
```

`run` creates and executes a new comparison synchronously. It requires the shared
runner lease, so stop the gateway first or use `submit`. Queued comparisons run
when the gateway has no due job. Jobs and comparisons share one worker; once a
comparison starts, it finishes before another job or comparison starts.

The dashboard's **Recovery challenge** section lets you select scenarios,
strategies, planners, repeats, and a per-trial budget. Start a comparison, select
its result, or cancel it. The results show success rate, time, retries, and
replans. Expand trial details for recovery time and **View events** to follow a
trial in the existing event timeline.

## Scenarios and strategies

| Scenario | Injection | Pass condition |
| --- | --- | --- |
| `baseline` | None | Place the cube in the bin |
| `sensor-glitch` | First red-cube observation fails | Inject the fault and complete placement |
| `sensor-outage` | First two observations fail | Inject the fault and complete placement |
| `grasp-stall` | First two grasps fail | Inject the fault and complete placement |
| `persistent-stall` | Every grasp fails | Inject the fault and complete placement; stock skills fail |
| `sensor-timeout` | Red-cube sensor waits beyond the budget | Inject the wait, time out, and issue no motion commands |

`retry-only` sets the gateway's recovery replan budget to zero. `recovery` allows
one recovery replan. Both retain each YAML step's own retry limit, the same
skills, planner, and per-trial deadline. The stock recovery skills resume the
original task from its checkpoint. The default scenarios with stock skills and
Mock yield **2/4** passes for retries only and **4/4** with recovery per repeat.
Custom skills can produce different results.

Failures are deterministic counters reset for every trial. Repetitions are
interleaved by scenario, provider, and strategy. These experiments exercise the
current simulator and skill loop; they do not estimate physical robot reliability.

## Reading results

`run` and `show` return `{challenge, rankings}`. A comparison's `completed` status
means all trials were evaluated, including failed missions. Accordingly,
`challenges run` exits zero when the experiment finishes; check `passed` or
`rankings[].pass_rate` to apply your own acceptance threshold. Cancellation exits
130. Invalid requests and runner conflicts exit nonzero.

Mission trials require a successful, accepted placement command for the correct
object and destination, a completed run, and the final simulator state at the
bin with no held object. Simply moving to the bin cannot pass. Fault scenarios
also require that their fault was actually injected. A sensor deadline reached
during planning, before fault injection, cannot pass the stop-before-motion case.

Each trial records its run ID, isolated session, status, pass flag, elapsed time,
recovery time, tool attempts, injected faults, step retries, replans, and errors.
Elapsed time includes planning and execution. Recovery time runs from the first
injected fault to successful mission completion. Retries sum each recorded
step's attempts after its first attempt, including recovery and resumed reports.
The rankings sort by pass rate, then fewer retries, then stable provider and
strategy names. Timing is diagnostic and does not break ties; it varies by host
load and model latency. Mean time includes all attempted trials, including failures.
Success rate uses **all planned trials** as its denominator, so cancelling or
interrupting a batch cannot improve its score by hiding unfinished cases.

```bash
roboclaw runs show RUN_ID --json
roboclaw runs events RUN_ID --json
```

## Isolation and persistence

Submission snapshots the effective skill catalog, planner prompt, configured
reference context, and execution policy under
`STATE_DIR/challenges/CHALLENGE_ID/assets`. Editing source assets or configuration
after submission does not change that comparison. A new comparison captures the
new assets. Model names, endpoints, and credentials come from the runner's
environment, so record those separately when comparing results across machines.

Every trial uses a fresh simulator, fault counters, memory, and deadline.
Configured tool restrictions still apply. The bridge is always Mock, even when
`ROBOCLAW_ROS2_BRIDGE` selects ROS 2 for normal runs. Process-wide sensor and motor
fault injection variables do not affect challenge trials. Trial sessions are
unique, memory recall is disabled, and trial runs do not enqueue webhooks.
Normal runs retain their existing behavior.

The default provider is always `mock`, regardless of the workspace planner.
Providers must be explicitly selected (`mock`, `local`, `openai`, `claude`).
There is no fallback, so a broken provider cannot silently score as another
model. Remote provider selections make real model requests using your configured
credentials and may incur costs; configured reference context is included.

Requests allow nonempty, unique scenario/provider/strategy selections, 1–10
repeats, at most four providers, at most 100 total trials, and a positive trial
budget up to 60 seconds. Unknown fields and identifiers are rejected.

Trial results are saved atomically after every trial. Cancellation stops the
active trial cooperatively and marks remaining trials cancelled. On runner
startup, running comparisons and their active run journals become `interrupted`;
interrupted comparisons are never replayed automatically. Create a new comparison
to repeat an interrupted experiment. Local records remain available for inspection.

## Control API

Use the gateway's existing bearer token, host/origin checks, and JSON content type.

| Method | Path | Result |
| --- | --- | --- |
| GET | `/api/challenges/scenarios` | Scenario descriptions |
| GET | `/api/challenges` | Persisted comparisons |
| GET | `/api/challenges/ID` | `{challenge, rankings}` |
| POST | `/api/challenges` | Queue a validated comparison; HTTP 202 |
| POST | `/api/challenges/ID/cancel` | Request cancellation; body must be `{}` |

Submission body (missing fields use defaults):

```json
{
  "scenarios": ["baseline", "sensor-glitch", "sensor-outage", "grasp-stall"],
  "providers": ["mock"],
  "strategies": ["retry-only", "recovery"],
  "repeats": 3,
  "timeout": "5s"
}
```
