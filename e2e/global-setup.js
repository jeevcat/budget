import { execSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";

const STATE_FILE = path.join(import.meta.dirname, ".e2e-state.json");
const PG = "-U budget -h localhost";

export default async function globalSetup() {
  // Drop any leftover databases from crashed prior runs
  const stale = execSync(
    `psql ${PG} -d postgres -tAc "SELECT datname FROM pg_database WHERE datname LIKE 'budget_e2e_%'"`,
    { encoding: "utf-8" },
  ).trim();
  for (const db of stale.split("\n").filter(Boolean)) {
    execSync(`dropdb ${PG} --if-exists ${db}`, { stdio: "pipe" });
  }

  const dbName = `budget_e2e_${Date.now()}`;
  execSync(`createdb ${PG} ${dbName}`, { stdio: "pipe" });

  const state = {
    dbName,
    databaseUrl: `postgresql://budget@localhost:5432/${dbName}`,
  };
  fs.writeFileSync(STATE_FILE, JSON.stringify(state));

  process.env.DATABASE_URL = state.databaseUrl;
  process.env.BUDGET_PORT = "3002";
  process.env.BUDGET_SECRET_KEY = "e2e-test-secret-key";
}
