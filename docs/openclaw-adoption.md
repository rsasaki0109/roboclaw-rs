# OpenClaw features for robot development

This implementation adapts OpenClaw's local gateway, sessions, skills, memory,
model fallback, diagnostics, automation and Control UI concepts to RoboClaw's
Rust robotics runtime. The selected scope prioritizes robot development. It is
an independent implementation; it does not run OpenClaw or its plugin SDK.

Reference snapshot: [`openclaw/openclaw@2f1f73c`](https://github.com/openclaw/openclaw/tree/2f1f73ccb2224bbe7d961b4c0d3b29cfdd51e7f8), reviewed 2026-10-08.

| OpenClaw strength | RoboClaw implementation | Entry point |
| --- | --- | --- |
| Local, user-owned state | Atomic local JSON, Markdown memory, UUID run journals | `config show`, `runs list` |
| Central configuration | Versioned YAML, strict unknown-field checks, CLI overrides | `config init`, `config check` |
| Session isolation and continuity | Named sessions, separate memory, OS writer locks, persistent history | `run --session lab`, `sessions show lab` |
| Durable memory and recall | User notes, local ranked keyword search, bounded reference context for model planners | `memory remember`, `memory search` |
| Replaceable model providers and fallback | Mock/local/OpenAI/Claude providers, explicit planning fallback, actual winner in reports | `--provider`, `--fallback` |
| Extensible workspace skills | Additional YAML directories, workspace precedence, duplicate and structural validation | `skill_dirs`, `skills validate` |
| Deterministic tool policy | Host allow/deny policy, deny precedence, whole selected-skill preflight before operations | `tools list`, `tools.allow`, `tools.deny` |
| Observable execution and hooks | Structured event callback API, run event journals, NDJSON CLI stream and cursor API | `run --stream`, `runs events` |
| Persistent automation | Delayed, interval and five-field cron jobs with IANA timezones, a single runner, cancellation, interruption recovery without replay | `jobs add`, `jobs run` |
| Local gateway and Control UI | Authenticated loopback HTTP API, responsive dashboard, plan preview, tasks, cancellation and event inspection | `gateway serve` |
| Operational diagnostics | Read-only asset/config/state checks and credential-presence checks | `doctor` |
| Bounded execution and recovery | Shared cancellation/deadline across planning, tool retries and recovery | `--timeout` (already present; integrated throughout) |

The design references are [architecture](https://github.com/openclaw/openclaw/blob/2f1f73ccb2224bbe7d961b4c0d3b29cfdd51e7f8/docs/concepts/architecture.md),
[sessions](https://github.com/openclaw/openclaw/blob/2f1f73ccb2224bbe7d961b4c0d3b29cfdd51e7f8/docs/concepts/session.md),
[memory](https://github.com/openclaw/openclaw/blob/2f1f73ccb2224bbe7d961b4c0d3b29cfdd51e7f8/docs/concepts/memory.md),
[model fallback](https://github.com/openclaw/openclaw/blob/2f1f73ccb2224bbe7d961b4c0d3b29cfdd51e7f8/docs/concepts/model-failover.md),
[skills](https://github.com/openclaw/openclaw/blob/2f1f73ccb2224bbe7d961b4c0d3b29cfdd51e7f8/docs/tools/skills.md),
[automation](https://github.com/openclaw/openclaw/blob/2f1f73ccb2224bbe7d961b4c0d3b29cfdd51e7f8/docs/automation/cron-jobs.md), and
[diagnostics](https://github.com/openclaw/openclaw/blob/2f1f73ccb2224bbe7d961b4c0d3b29cfdd51e7f8/docs/gateway/doctor.md).

## Scope and future integrations

OpenClaw is a much larger assistant platform. The following remain separate
work, rather than being represented as supported features:

- Chat channels, sender pairing and delivery adapters (Slack, Telegram, WhatsApp, etc.).
- Voice, native mobile/desktop companion apps, cameras and remote device nodes.
- Browser/computer operation, arbitrary shell tools and operating-system sandboxing.
- Third-party OpenClaw plugins, ClawHub packages and OpenClaw wire-protocol compatibility.
- Agent delegation and distributed robot coordination.
- Vector/semantic memory search, embedding providers and model-driven memory compaction.
- OAuth profile rotation, credential cooldowns and webhook delivery.
- A persistent physical robot connection and hardware emergency-stop integration.

The public `Planner`, `Tool` and `EventObserver` interfaces are extension points
for Rust integrations. The current gateway runs an in-process simulator. Each
run starts with a fresh simulator state; sessions preserve instructions, notes,
reports and events. Saved reports are never restored as live robot state.
