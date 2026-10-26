import { cargoInvocation } from "./build-cache.mjs";
import { runBuildCommand } from "./build-runner.mjs";

const args = process.argv.slice(2);
process.exitCode = await runBuildCommand("cargo", args, cargoInvocation(args));
