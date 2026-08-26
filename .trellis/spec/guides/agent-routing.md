# Agent Routing Guide

> Purpose: make model selection, subagent boundaries, and handoff evidence deterministic.

## Routing Matrix

| Work type | Model | Reasoning | Preferred role |
|---|---|---|---|
| Decision, architecture, design, investigation, root-cause analysis, cross-layer review | `gpt-5.6-sol` | `max` | `trellis-research`, `trellis-check`, or read-only analysis agent |
| Reviewed implementation plan, mechanical edits, tests, builds, CI repair, packaging, release pipeline | `gpt-5.6-luna` | `max` | `trellis-implement` or execution agent |

`trellis-check` is a judgment role by default and therefore uses `gpt-5.6-sol/max`. Do not use it merely to run known commands. A mechanical verifier or implementer using `gpt-5.6-luna/max` should run those commands and return the raw result.

## Classification Rules

Route to `gpt-5.6-sol/max` when any of these are true:

- requirements or ownership boundaries are still ambiguous;
- more than one credible design exists;
- the cause of a failure has not been demonstrated from code or runtime evidence;
- a change crosses frontend, IPC, Rust, storage, platform, security, or performance boundaries;
- a review must judge behavior, compatibility, regression risk, or whether the abstraction is correct.

Route to `gpt-5.6-luna/max` only when all of these are true:

- inputs, scope, steps, affected files, and acceptance criteria are explicit;
- unresolved product or architecture decisions are absent;
- the agent can report completion from deterministic checks;
- unexpected ambiguity is returned to the main Agent instead of being guessed through.

For mixed tasks, the mandatory flow is:

1. `gpt-5.6-sol/max` researches and records the decision.
2. The main Agent converts the decision into ordered steps, dependencies, file ownership, and acceptance checks.
3. `gpt-5.6-luna/max` implements non-overlapping work units.
4. `gpt-5.6-sol/max` reviews behavioral, architectural, and cross-layer correctness when the risk justifies it.
5. The main Agent integrates, verifies, records progress, commits, pushes, and releases when authorized.

If implementation uncovers an unplanned design choice or an unproven root cause, stop that execution unit and route the question back to `gpt-5.6-sol/max`.

## Dispatch Contract

Every Trellis dispatch must start with:

```text
Active task: .trellis/tasks/<task>
```

The prompt must also contain:

- one bounded objective;
- files or modules the agent owns and any forbidden write areas;
- required task, spec, design, and research inputs;
- expected artifact or code result;
- exact validation commands or evidence requirements.

Only parallelize units with no unmet dependency and no overlapping write ownership. The main Agent owns conflict resolution, task status, decision logs, commits, pushes, releases, and final acceptance.

## Native Subagents and Channels

Use a native one-shot Subagent for a bounded research, implementation, or review unit. Use `trellis channel` only when workers need multiple turns, durable shared events, progress inspection, interruption, or a long-lived collaboration loop.

Codex agent profiles can pin both model and reasoning effort. The Channel CLI can override `--provider` and `--model`, but it has no project-level flag that guarantees reasoning effort. When using Channel, record the model selection and do not claim `max` is enforced unless the worker provider configuration proves it.
