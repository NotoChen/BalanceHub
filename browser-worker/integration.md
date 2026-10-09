# Browser-assisted check-in integration

Browser-assisted check-in is part of the desktop App. Agent asset management is
outside this component's scope.

## Accepted scope

- Normal HTTP check-in stays the first path; positively identified verification
  or a failed status GET can hand off to a dedicated browser session.
- One Rust task registry owns manual, batch and scheduled check-in. Commands
  acknowledge immediately. Queuing, human assistance and component installation
  must not hold the normal request concurrency slot or lock the main UI.
- Manual and user-triggered batch attempts keep the browser open while human
  verification is pending and continue in the same task when it succeeds. The
  card, batch progress and task center can bring the existing window forward.
- Scheduled attempts yield when human assistance is needed. Resuming creates a
  fresh browser session, checks today's state again, and obtains a fresh token.
- A final success event follows persistence. An uncertain submission is reported
  as unconfirmed and is not automatically submitted again.
- Browser support is an optional component under the App data directory, installed
  only after an explicit UI action. The main installer carries no Node or browser.
  Reuse a selected local Chromium browser with Node.js/Playwright support, or offer
  an isolated managed Chromium. Show required downloads separately from discovery.
- Detect on component-panel opening, before browser use, and after installation,
  removal or launch failure. Ordinary inventory has a five-minute cache; launch
  checks browser/runtime fingerprints and revalidates changed files with an empty
  temporary-profile launch. No periodic network/browser scan. Component
  versions are pinned by the App; an incompatible installation offers an explicit
  update, never an unattended download.
- Downloads use the existing global network proxy, bounded timeouts, progress,
  cancellation, verified archives and staged activation. Cancellation/failure
  leaves the previously installed component usable.
- Persist a browser selection only after a successful launch probe. Switching
  installed browsers does not download components. Windows discovery reads both
  user/machine registry scopes and registry views, as well as default directories;
  all platforms offer manual selection. Native Windows acceptance remains pending.
- Never click CAPTCHA controls. Turnstile callbacks report interactive mode,
  errors, expiry, timeout and unsupported browsers. Manual and batch tasks keep
  the window; scheduled tasks yield for a later user-triggered attempt. Cloudflare's published
  lack of support for automation still applies; do not promise universal clearance.
- Successful empty model lists replace cached models; failed fetches retain them.
- AgentRouter uses its login-based NewAPI dialect. Reauthenticate with saved
  account/password or a bound browser login account; a pasted session alone is insufficient.
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
and busy-state cleanup. Keep recorded real Turnstile check-ins as historical
evidence; changed verification behavior needs separate acceptance and historical
results do not prove current compatibility. Launch an isolated Tauri dev
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

Bound-account reauthentication uses `provider_browser_login/check_in` inside the
existing check-in task. It shares the exclusive account profile lease with login
imports and account management, without holding the HTTP or global refresh gate.
Independent verification profiles and login accounts share a maximum of three
browser slots. Tasks using the same account profile queue before taking a slot;
they do not block other accounts, and cancelling a queued task releases its lease.
Clearing an idle account only locks that account, not all browser tasks.
The worker clears relay credentials and waits for the bound platform's login
button or link to become usable, including after a page challenge. It selects the
entry at most once, never repeats a possibly delivered click, and requires a
successful login response plus same-user readback. OAuth
state, session refresh and localStorage cannot prove reauthentication. Rust checks
the bound account generation, observed platform identity and current provider
context before merging credentials, then uses the common check-in finalizer.
User-triggered bulk runs open the window directly, and also resume previously
suspended scheduled tasks. Scheduled runs yield as `waitingLogin`; a manual resume
opens the window. Unknown platform controls, expired identity-provider sessions
and new authorization prompts stay available for the user to complete.
Progress distinguishes waiting for login from actual login submission so cancellation
after a possible submission remains unconfirmed instead of silently retrying.

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
