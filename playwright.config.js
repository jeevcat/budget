import { defineConfig } from "@playwright/test";
import { execSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";

function findChromium() {
  if (process.env.CHROMIUM_PATH) return process.env.CHROMIUM_PATH;
  try {
    return execSync("which chromium", { encoding: "utf-8" }).trim();
  } catch {
    return undefined;
  }
}

// --- Temp database setup (must run before webServer starts) ----------------
// Playwright evaluates this config multiple times (main process + each worker).
// Only create the DB on the first evaluation; subsequent ones reuse it via the
// state file that persists for the duration of the run.
const STATE_FILE = path.join(import.meta.dirname, "e2e/.e2e-state.json");
const TEST_DB = path.join(import.meta.dirname, "scripts/test-db");
const E2E_PORT = 3002;
const TEST_SECRET = "test-secret-key";

let dbName;
try {
  const existing = JSON.parse(fs.readFileSync(STATE_FILE, "utf-8"));
  dbName = existing.dbName;
} catch {
  dbName = execSync(`${TEST_DB} create budget_e2e`, { encoding: "utf-8" }).trim();
  fs.writeFileSync(STATE_FILE, JSON.stringify({ dbName }));
}

process.env.DATABASE_URL = `postgresql://budget@localhost:5432/${dbName}`;
process.env.BUDGET_PORT = String(E2E_PORT);
process.env.BUDGET_SECRET_KEY = TEST_SECRET;

// ---------------------------------------------------------------------------

export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  retries: 0,
  globalSetup: "e2e/global-setup.js",
  globalTeardown: "e2e/global-teardown.js",
  use: {
    baseURL: `http://localhost:${E2E_PORT}`,
    screenshot: "only-on-failure",
  },
  webServer: {
    command: "./target/debug/budget",
    port: E2E_PORT,
    reuseExistingServer: false,
    timeout: 30_000,
  },
  projects: [
    {
      name: "setup",
      testMatch: "auth.setup.js",
      use: {
        browserName: "chromium",
        launchOptions: { executablePath: findChromium() },
      },
    },
    {
      name: "chromium",
      dependencies: ["setup"],
      use: {
        browserName: "chromium",
        storageState: "playwright/.auth/user.json",
        launchOptions: { executablePath: findChromium() },
      },
    },
  ],
});
