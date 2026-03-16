#!/usr/bin/env bun
/// Report which API handler functions have no test coverage.
///
/// Runs `cargo llvm-cov` on integration tests, then cross-references
/// the coverage data with handler functions listed in the OpenAPI spec.
/// Fully deterministic — no manual bookkeeping required.
///
/// Usage:
///   bun scripts/api-coverage.ts           # integration tests only
///   bun scripts/api-coverage.ts --all     # all api tests (unit + integration)
import { $ } from "bun";

const testArgs =
  process.argv.includes("--all")
    ? ["-p", "api"]
    : ["-p", "api", "--test", "integration"];

// ---------------------------------------------------------------------------
// 1. Run tests under coverage
// ---------------------------------------------------------------------------
console.error("Running tests under coverage...");
const cov = await $`cargo llvm-cov --json --no-cfg-coverage ${testArgs}`
  .quiet()
  .nothrow();
if (cov.exitCode !== 0) {
  console.error("cargo llvm-cov failed:");
  console.error(cov.stderr.toString());
  process.exit(1);
}
const data = JSON.parse(cov.stdout.toString());
const funcs: { name: string; count: number; filenames: string[] }[] =
  data.data[0].functions ?? [];

// ---------------------------------------------------------------------------
// 2. Get endpoints from the OpenAPI spec (via ephemeral test server)
// ---------------------------------------------------------------------------
console.error("Fetching OpenAPI spec...");
const specResult =
  await $`./scripts/api --test /docs/openapi.json`.quiet().nothrow();
if (specResult.exitCode !== 0) {
  console.error("Failed to fetch OpenAPI spec");
  process.exit(1);
}
const spec = JSON.parse(specResult.stdout.toString());

// ---------------------------------------------------------------------------
// 3. Build endpoint list from spec
// ---------------------------------------------------------------------------
interface Endpoint {
  method: string;
  path: string;
  operationId: string;
  tag: string;
}
const endpoints: Endpoint[] = [];
for (const [path, methods] of Object.entries(spec.paths) as [
  string,
  Record<string, { operationId: string; tags?: string[] }>,
][]) {
  for (const [method, detail] of Object.entries(methods)) {
    endpoints.push({
      method: method.toUpperCase(),
      path,
      operationId: detail.operationId,
      tag: detail.tags?.[0] ?? "unknown",
    });
  }
}

// ---------------------------------------------------------------------------
// 4. Match handlers to coverage via Rust symbol mangling
// ---------------------------------------------------------------------------
// Top-level async fns have mangled names containing:
//   6routes{modLen}{module}{nameLen}{handler}
// Closures inside them have _RNCN... prefixes; the handler itself is _RNvNtNt...

const tagToModule: Record<string, string> = {
  accounts: "accounts",
  transactions: "transactions",
  categories: "categories",
  rules: "rules",
  budgets: "budgets",
  jobs: "jobs",
  connections: "connections",
  import: "import",
  amazon: "amazon",
  auth: "auth",
};

interface CovResult {
  endpoint: string;
  handler: string;
  covered: boolean;
}
const results: CovResult[] = [];

for (const ep of endpoints) {
  const mod = tagToModule[ep.tag] ?? ep.tag;
  const handler = ep.operationId;
  const pattern = `6routes${mod.length}${mod}${handler.length}${handler}`;

  const matching = funcs.filter(
    (f) =>
      f.name.includes(pattern) &&
      f.filenames?.some((fn: string) => fn.includes("/routes/")),
  );

  // Prefer the top-level function symbol, not inner async closures
  const topLevel = matching.filter((f) => /_RNvNtNt/.test(f.name));
  const best = topLevel.length > 0 ? topLevel[0] : matching[0];
  const covered = best ? best.count > 0 : false;

  results.push({
    endpoint: `${ep.method.padEnd(7)} ${ep.path}`,
    handler: `${mod}::${handler}`,
    covered,
  });
}

// ---------------------------------------------------------------------------
// 5. Report
// ---------------------------------------------------------------------------
const coveredList = results.filter((r) => r.covered);
const untestedList = results.filter((r) => !r.covered);

console.log(
  `\nAPI handler coverage: ${coveredList.length}/${results.length} endpoints\n`,
);

if (untestedList.length > 0) {
  console.log("UNTESTED:");
  for (const r of untestedList) {
    console.log(`  ${r.endpoint.padEnd(55)} ${r.handler}`);
  }
  console.log();
}

if (coveredList.length > 0) {
  console.log("COVERED:");
  for (const r of coveredList) {
    console.log(`  ${r.endpoint.padEnd(55)} ${r.handler}`);
  }
}

process.exit(untestedList.length > 0 ? 1 : 0);
