import assert from "node:assert/strict";
import test from "node:test";

import { providerDuplicateSaveResolution } from "../src/composables/provider-editor-shared.ts";
import type { ProviderSaveConflict } from "../src/stores/provider-types.ts";

function conflict(kind: ProviderSaveConflict["kind"]): ProviderSaveConflict {
  return {
    kind,
    existingProviderId: "provider-existing",
    existingProviderName: "已有中转站",
  };
}

test("same-site API Key conflict can create a separate provider card", () => {
  assert.deepEqual(
    providerDuplicateSaveResolution(
      conflict("sameUrlDifferentApiKey"),
      "createSeparate",
    ),
    {
      options: { createSeparateFromProviderId: "provider-existing" },
      completion: "standard",
    },
  );
});

test("same-site API Key conflict can merge and then focus credentials", () => {
  assert.deepEqual(
    providerDuplicateSaveResolution(conflict("sameUrlDifferentApiKey"), "merge"),
    {
      options: { mergeApiKeyIntoProviderId: "provider-existing" },
      completion: "mergedApiKey",
    },
  );
});

test("exact credential conflicts cannot use the same-URL bypass", () => {
  assert.equal(
    providerDuplicateSaveResolution(conflict("sameApiKey"), "createSeparate"),
    null,
  );
  assert.equal(
    providerDuplicateSaveResolution(conflict("sameAccount"), "merge"),
    null,
  );
  assert.deepEqual(
    providerDuplicateSaveResolution(conflict("sameApiKey"), "overwrite"),
    {
      options: { overwriteProviderId: "provider-existing" },
      completion: "standard",
    },
  );
});

test("cancelling a duplicate decision never retries the save", () => {
  assert.equal(
    providerDuplicateSaveResolution(conflict("sameUrlDifferentApiKey"), "cancel"),
    null,
  );
});
