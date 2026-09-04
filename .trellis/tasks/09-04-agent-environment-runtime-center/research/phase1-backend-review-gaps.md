# Phase 1 Backend Review Gaps

Date: 2026-09-04

## Scope

This review covers the Rust Agent environment inventory, installation/version contract,
opaque asset access, and the four registered Agent environment adapters.

## Gap 1: Multiple installation discovery

### Evidence

- `services/agent_cli/environment/inventory.rs` calls the existing `agent_cli::find` once per
  registered Agent.
- `agent_cli::find` deliberately selects one fixed or highest-version executable from its
  candidates. It does not return all valid installations.
- `AgentInstallation.id` is currently derived from `(environment, agent kind)`, so two native
  installations of the same Agent cannot have distinct identities.
- The contract records whether the selected executable came from configured or automatic
  discovery, but it does not identify npm, native installer, Homebrew, pnpm, bun, Volta, or other
  owning installation sources.

### Affected contracts

- `AgentInstallation`
- `AgentDiscoverySource`
- `EnvironmentAdapter`
- Agent CLI discovery candidate/probe APIs
- latest-version cache key and future update actions

### Required follow-up

1. Add a discovery API that returns every validated executable candidate with normalized path,
   raw version, owning source, package identity, and channel evidence.
2. Define installation identity from stable environment/source/package/path facts instead of only
   Agent kind.
3. Keep configured/default selection as a separate effective-selection fact; do not discard other
   installations.
4. Compare each installation against the version source appropriate to its owner. Shared npm
   metadata may still be coalesced by `(package, channel)`.
5. Add fixtures for two versions managed by different sources, duplicate paths, a missing
   configured path with a valid automatic installation, and prerelease/stable coexistence.

## Gap 2: Effective asset, trust, and conflict parsing

### Evidence

- The current four environment adapters declare documented paths and categories through templates.
- Directory children are inventoried without executing their contents; configuration-backed MCP,
  Hook, Plugin/Extension, and Status UI records point to their owning files.
- Presence alone cannot prove native enablement, precedence, trust, shadowing, or conflict.
  Consequently the Rust inventory intentionally reports `unknown` rather than fabricating those
  states.
- No Agent-specific parser currently extracts logical entries or resolves cross-scope precedence.

### Affected contracts

- `AgentAssetRecord.declared_state`
- `AgentAssetRecord.effective_state`
- `AgentAssetRecord.trust_state`
- `AgentAssetRecord.diagnostics`
- `AgentAssetSource.precedence`
- `AgentAssetCapability`
- `EnvironmentAdapter`

### Required follow-up

1. Add versioned, Agent-owned read-only parsers for each documented schema. Unknown schema versions
   must remain `unknown` or `unsupported`.
2. Parse logical assets from configuration files without executing Hook, Plugin, MCP, or status
   commands and without returning credentials through IPC.
3. Resolve scope precedence and collisions in Rust, including workspace trust where the Agent
   exposes reliable evidence.
4. Keep trust independent from enabled/effective state. Absence of a trust record is not proof of
   trust or distrust.
5. Add official-schema fixtures covering enabled, disabled, shadowed, conflicting, untrusted,
   malformed, and unknown-field cases for every supported Agent/category pair.

## Dependency order

1. Complete multi-installation discovery and stable installation identity first.
2. Add per-Agent schema parsers keyed by installation/environment capability.
3. Add the shared Rust precedence reducer over parsed logical assets.
4. Only then expose effective/trust/conflict filters and actions in the UI.
5. Managed Hook mutation must consume the same installation and parsed-source identities; it must
   not create a parallel ownership or configuration model.

Until these steps are complete, the UI may show the existing path-level inventory and explicit
`unknown` states, but it must not describe those states as verified effective configuration.
