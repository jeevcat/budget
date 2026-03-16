import { execSync } from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";

const STATE_FILE = path.join(import.meta.dirname, ".e2e-state.json");

export default async function globalTeardown() {
  try {
    const state = JSON.parse(fs.readFileSync(STATE_FILE, "utf-8"));
    execSync(`dropdb -U budget -h localhost --if-exists ${state.dbName}`, {
      stdio: "pipe",
    });
  } catch {
    // Best-effort cleanup
  } finally {
    try {
      fs.unlinkSync(STATE_FILE);
    } catch {
      // ignore
    }
  }
}
