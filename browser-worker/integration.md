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
- Verification uses a separate compact application window without browser tabs or
  an address bar. The generated Turnstile page fits its content and follows widget
  expansion; unchanged content does not undo manual resizing. Full-page site
  challenges keep their original content in a small, scrollable window.

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

## Configurable check-in policy

The accepted follow-up replaces host-bound check-in dispatch with three persisted
provider settings: `checkInMethod` (`auto`, `standard`, `sessionSignIn`,
`freshLogin`), `autoShield` (default `true`), and `turnstileMode` (`auto`, `always`).
Explicit methods override known-site presets; otherwise AnyRouter selects session
sign-in, AgentRouter selects fresh login, and other NewAPI sites select standard
check-in. This is a method choice, not a new provider protocol.

Rust provider models/domain policy own defaults, effective method and credential
requirements. A pure preview command exposes safe display metadata to the editor.
The editor's inline check-in section stores these settings through the
existing provider input/save/import/export paths. Standard endpoint capability
probes cannot disable an explicitly chosen method. Changing policy invalidates
request contexts and cached check-in capability, while preserving account history.

The protocol adapter selects the method once. Each method has one business flow
using a small HTTP/browser request executor: standard status/submit/confirmation,
session-only `/api/user/sign_in`, or fresh login/account confirmation. Browser
services retain only lifecycle, profile isolation, cancellation and progress.
Replacing the duplicate flows must retain bounded challenge retries and must not
repeat an uncertain submission. Fresh login confirms the account/session; it does
not prove that a daily reward was credited.
The in-process verification handoff retains whether login is still pending, so
cached credentials cannot skip a login that was intercepted by a page challenge.

`autoShield = false` disables cached shield injection and solving in provider HTTP
transport, and page-challenge navigation in the browser executor. Browser profiles
separate this policy and discard shield cookies when disabled. Turnstile is an
independent operation-level setting: `always` obtains a fresh token before a
submission, and `auto` escalates only after an explicit challenge rejection.

Changes are limited to provider models/domain policy, provider preview/registration,
request guards, NewAPI check-in adapters, existing HTTP/browser transport, the
provider editor/types/conversions, worker documentation and focused regression
tests. Agent asset management and a generic workflow engine remain outside this scope.
Validation covers settings round trips, arbitrary-domain overrides, stale results,
disabled protection, bounded/uncertain submissions and a usable native editor.

Native UI acceptance was exercised with a disposable provider: all three controls
changed, the backend credential explanation updated, and saving/reopening retained
the exact selections. The original isolated development data is restored afterwards.

The configurable-policy follow-up passes the frontend production build, 92 frontend
tests, 29 focused Rust check-in tests, three browser tests (including the actual
compact Chromium window), strict Clippy, platform script checks and diff checks.
The existing shared Cargo cache is reused; no clean or full test rebuild is run.
Real-account reward confirmation remains subject to the site records.


## Editor and release acceptance

The editor displays the original basics and credential sections on one scrolling
page, without the three tabs or previous/next navigation. Their URL, protocol and
authentication controls are preserved. Only runtime policy controls are arranged
as consistent selects, checkboxes and adjacent conditional fields. The user
accepted this scope and authorized the v0.5.10 commit, push and release.
