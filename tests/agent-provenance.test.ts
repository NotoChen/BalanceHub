import assert from "node:assert/strict";
import test from "node:test";
import type { AgentAssetProvenance } from "../src/stores/provider-types.ts";
import { agentAssetProvenanceEntries, agentAssetProvenanceLabel } from "../src/utils/agent-asset-provenance.ts";
import { assetSource } from "./agent-asset-fixtures.ts";

test("unknown authors stay unknown for shared and plugin-provided definitions", () => {
  const independent: AgentAssetProvenance = { sourceId: "shared", declarationId: "shared-definition", scope: "user",
    provision: "independent", installation: "sharedFiles", provider: "unknown" };
  const plugin: AgentAssetProvenance = { ...independent, sourceId: "plugin", declarationId: "plugin-definition",
    provision: "pluginProvided", installation: "nativePackage" };
  for (const evidence of [independent, plugin]) {
    assert.doesNotMatch(agentAssetProvenanceLabel(evidence), /作者未确认|作者证据不足/);
    assert.doesNotMatch(agentAssetProvenanceLabel(evidence), /用户声明自制|Agent 官方/);
  }
  assert.match(agentAssetProvenanceLabel({ ...independent, provider: "userDeclared" }), /用户声明自制/);
});

test("each definition resolves its own source without substituting an inspection or overlay source", () => {
  const physical = assetSource("definition", { scope: "user", origin: "configEntry", path: "/fixture/claude.json" });
  const overlay = assetSource("overlay", { scope: "workspace", origin: "localFiles", path: "/fixture/project/settings.json" });
  const evidence: AgentAssetProvenance = { sourceId: physical.id, declarationId: "project-mcp", scope: "workspace",
    provision: "independent", installation: "configEntry", provider: "unknown" };
  const entries = agentAssetProvenanceEntries([evidence, { ...evidence, sourceId: "missing", declarationId: "missing-definition" }], [overlay, physical]);
  assert.equal(entries[0].source, physical);
  assert.equal(entries[0].evidence.scope, "workspace");
  assert.equal(entries[0].source?.scope, "user");
  assert.equal(entries[0].evidence.installation, "configEntry");
  assert.equal(entries[1].source, null);
});

test("sources alone cannot create provenance for unapplied shared definitions", () => {
  assert.deepEqual(agentAssetProvenanceEntries([], [assetSource("builtin", { origin: "bundled", scope: "system" })]), []);
});
