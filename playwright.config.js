import { defineConfig } from "@playwright/test";
import { execSync } from "node:child_process";

function findChromium() {
  if (process.env.CHROMIUM_PATH) return process.env.CHROMIUM_PATH;
  try {
    return execSync("which chromium", { encoding: "utf-8" }).trim();
  } catch {
    return undefined;
  }
}

export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  retries: 0,
  globalSetup: "e2e/global-setup.js",
  globalTeardown: "e2e/global-teardown.js",
  use: {
    baseURL: "http://localhost:3002",
    screenshot: "only-on-failure",
  },
  webServer: {
    command:
      "DATABASE_URL=$DATABASE_URL BUDGET_PORT=3002 BUDGET_SECRET_KEY=e2e-test-secret-key ./target/debug/budget",
    port: 3002,
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
