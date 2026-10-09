import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { compileScript, parse } from "vue/compiler-sfc";
import ts from "typescript";

const require = createRequire(import.meta.url);

// Execute application code with explicit in-memory replacements for native boundaries.
export function loadSource<T>(path: string, bindings: Record<string, unknown>): T {
  const source = readFileSync(new URL(`../../src/${path}`, import.meta.url), "utf8");
  const code = path.endsWith(".vue")
    ? compileScript(parse(source, { filename: path }).descriptor, { id: "interaction-test" }).content
    : source;
  const output = ts.transpileModule(code, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const exports = {};
  new Function("require", "exports", output)((specifier: string) => {
    if (Object.hasOwn(bindings, specifier)) return bindings[specifier];
    if (specifier === "vue" || specifier === "pinia") return require(specifier);
    throw new Error(`Unmocked module: ${specifier}`);
  }, exports);
  return exports as T;
}
