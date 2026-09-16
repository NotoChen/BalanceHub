# Browser-assisted check-in integration

This work is isolated in `feat/cloudflare-checkin`. The user declined creating a
Trellis task. Agent asset management is outside this change.

## Accepted scope

- Normal HTTP check-in stays the first path; positively identified verification
  or a failed status GET can hand off to a dedicated browser session.
- One Rust task registry owns manual, batch and scheduled check-in. Commands
  acknowledge immediately. Queuing, human assistance and component installation
  must not hold the normal request concurrency slot or lock the main UI.
- Automatic attempts yield when human assistance is needed. Resuming creates a
  fresh browser session, checks today's state again, and obtains a fresh token.
- A final success event follows persistence. An uncertain submission is reported
  as unconfirmed and is not automatically submitted again.
- Browser support is an optional component under the App data directory, installed
  only after an explicit UI action. The main installer carries no Node or browser.
  Use local Chrome/Edge/Chromium when available; offer an isolated managed browser.
- Detect on component-panel opening, before browser use, and after installation,
  removal or launch failure. Ordinary inventory has a five-minute cache; launch
  always checks executable existence. No periodic network/browser scan. Component
  versions are pinned by the App; an incompatible installation offers an explicit
  update, never an unattended download.
- Downloads use the existing global network proxy, bounded timeouts, progress,
  cancellation, verified archives and staged activation. Cancellation/failure
  leaves the previously installed component usable.
- Successful empty model lists replace cached models; failed fetches retain them.
- AgentRouter uses its login-based NewAPI dialect, with account/password required.
- Provider cards expose an action to open the configured site in the default browser.

## Change owners

`services/check_in_tasks` owns task state, deduplication and events;
`services/browser_check_in` owns browser lifetime; protocol adapters own HTTP,
verification and dialect semantics. `services/browser_runtime` owns optional
component files and installation. Frontend composables project these states into
the existing task center and closable component panel. `desktop.rs` only registers
commands. Model-list parsing and provider-card actions are independent fixes.

## Acceptance

Use focused regression tests for empty-vs-failed model results, task deduplication,
waiting/cancel/resume transitions, persistence ordering and frontend stale-result
and busy-state cleanup. Reuse the recorded real Turnstile check-in evidence;
do not repeat real submissions for unchanged paths. Launch an isolated Tauri dev
App with automatic activity initially disabled and leave it running for the user.
Automated verification does not constitute user acceptance.
