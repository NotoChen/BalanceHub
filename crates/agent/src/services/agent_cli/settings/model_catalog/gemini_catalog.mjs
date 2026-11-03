// Read the selected package's exported catalog; never import its CLI entrypoint.
import * as bh_fs from "node:fs";
import * as bh_path from "node:path";
import { pathToFileURL as bh_fileUrl } from "node:url";
import { createRequire as bh_requireAt } from "node:module";

const bh_limit = 32 * 1024 * 1024;
let bh_budget = 64 * 1024 * 1024;
function bh_read(bh_pathname) {
  const bh_size = bh_fs.statSync(bh_pathname).size;
  if (bh_size > bh_limit || bh_size > bh_budget)
    throw new Error("catalog metadata limit");
  bh_budget -= bh_size;
  return bh_fs.readFileSync(bh_pathname, "utf8");
}
let bh_entry = bh_fs.realpathSync(process.argv[1]);
let bh_manifest;
let bh_root = bh_path.dirname(bh_entry);
for (let bh_depth = 0; bh_depth < 8; bh_depth++) {
  const bh_candidate = bh_path.join(bh_root, "package.json");
  if (bh_fs.existsSync(bh_candidate)) {
    const bh_package = JSON.parse(bh_read(bh_candidate));
    if (bh_package.name === "@google/gemini-cli") {
      bh_manifest = bh_candidate;
      break;
    }
  }
  const bh_parent = bh_path.dirname(bh_root);
  if (bh_parent === bh_root) break;
  bh_root = bh_parent;
}
if (!bh_manifest) {
  // npm's Windows .cmd/.ps1 shims sit beside node_modules rather than in it.
  const bh_require = bh_requireAt(bh_entry);
  bh_manifest = bh_require.resolve("@google/gemini-cli/package.json");
  bh_root = bh_path.dirname(bh_manifest);
  const bh_package = JSON.parse(bh_read(bh_manifest));
  const bh_bin =
    typeof bh_package.bin === "string"
      ? bh_package.bin
      : bh_package.bin?.gemini;
  if (bh_package.name !== "@google/gemini-cli" || !bh_bin)
    throw new Error("unknown package");
  bh_entry = bh_fs.realpathSync(bh_path.resolve(bh_root, bh_bin));
}
const bh_queue = [bh_entry];
const bh_seen = new Set();
// Unbundled npm releases expose the same model constants from gemini-cli-core.
try {
  const bh_require = bh_requireAt(bh_entry);
  const bh_core = bh_require.resolve("@google/gemini-cli-core");
  bh_queue.unshift(bh_core);
} catch {}
let bh_models;
while (bh_queue.length && bh_seen.size < 64) {
  const bh_file = bh_queue.shift();
  if (bh_seen.has(bh_file)) continue;
  bh_seen.add(bh_file);
  const bh_source = bh_read(bh_file);
  if (/\b(?:const|let|var)\s+VALID_GEMINI_MODELS\s*=/.test(bh_source)) {
    const bh_exports = await import(bh_fileUrl(bh_file).href);
    const bh_valid = bh_exports.VALID_GEMINI_MODELS;
    if (bh_valid instanceof Set || Array.isArray(bh_valid)) {
      const bh_ids = new Set(bh_valid);
      for (const [bh_name, bh_value] of Object.entries(bh_exports)) {
        if (
          (bh_name.startsWith("GEMINI_MODEL_ALIAS_") ||
            bh_name.endsWith("_MODEL_AUTO")) &&
          typeof bh_value === "string"
        )
          bh_ids.add(bh_value);
      }
      bh_models = [...bh_ids]
        .filter(
          (bh_id) => typeof bh_id === "string" && bh_id && bh_id !== "none",
        )
        .map((bh_id) => ({
          value: bh_id,
          label:
            typeof bh_exports.getDisplayString === "function"
              ? bh_exports.getDisplayString(bh_id)
              : bh_id,
        }));
      break;
    }
  }
  // Follow only the selected entry's import graph, avoiding stale chunks left
  // behind by another installed version. No recursive disk scan is performed.
  const bh_imports =
    /^\s*(?:import|export)\s+(?:[^;]*?\bfrom\s*)?["'](\.[^"']+\.m?js)["']/gm;
  for (const bh_match of bh_source.matchAll(bh_imports)) {
    const bh_target = bh_path.resolve(bh_path.dirname(bh_file), bh_match[1]);
    if (bh_fs.existsSync(bh_target)) bh_queue.push(bh_target);
  }
}
if (!bh_models?.length) throw new Error("native model metadata unavailable");
process.stdout.write("BALANCEHUB_MODELS=" + JSON.stringify(bh_models) + "\n");
