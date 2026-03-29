#!/usr/bin/env bun
//
// Extracts balance snapshots from ING Girokonto PDF bank statements and
// imports them into the balance_snapshots table.
//
// Each PDF's first-page header contains:
//   Datum              DD.MM.YYYY        (statement date)
//   Neuer Saldo        1.234,56 Euro     (closing balance)
//
// Usage:
//   bun scripts/ing-balance-import.ts /tmp/giro/ --account <uuid> [--dry-run]

import { execSync } from "child_process";
import { readdirSync, writeFileSync } from "fs";
import { join, resolve } from "path";
import { parseArgs } from "util";

// -- Locate pdftotext --------------------------------------------------------

function findPdftotext(): string {
  try {
    return execSync("which pdftotext", { encoding: "utf-8" }).trim();
  } catch {
    try {
      const out = execSync(
        'nix-shell -p poppler-utils --run "which pdftotext" 2>/dev/null',
        { encoding: "utf-8" }
      );
      return out.trim();
    } catch {
      console.error(
        "pdftotext not found. Install poppler-utils or use nix-shell."
      );
      process.exit(1);
    }
  }
}

const PDFTOTEXT = findPdftotext();
console.log(`Using pdftotext: ${PDFTOTEXT}`);

// -- Types -------------------------------------------------------------------

interface BalanceSnapshot {
  date: string; // YYYY-MM-DD
  balance: string; // decimal string
  source_file: string;
}

// -- CLI ---------------------------------------------------------------------

const { values, positionals } = parseArgs({
  args: Bun.argv.slice(2),
  options: {
    account: { type: "string", short: "a" },
    "dry-run": { type: "boolean" },
    help: { type: "boolean", short: "h" },
  },
  allowPositionals: true,
});

if (values.help || positionals.length === 0) {
  console.log(
    "Usage: bun scripts/ing-balance-import.ts <pdf-dir-or-file>... --account <uuid> [--dry-run]"
  );
  process.exit(0);
}

const ACCOUNT_ID = values.account;
const dryRun = values["dry-run"] ?? false;

if (!ACCOUNT_ID) {
  console.error("--account <uuid> is required");
  process.exit(1);
}

if (
  !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
    ACCOUNT_ID
  )
) {
  console.error("Invalid account UUID: " + ACCOUNT_ID);
  process.exit(1);
}

// -- Collect PDF paths -------------------------------------------------------

const pdfPaths: string[] = [];
for (const p of positionals) {
  const abs = resolve(p);
  try {
    const entries = readdirSync(abs);
    for (const e of entries) {
      if (e.endsWith(".pdf")) pdfPaths.push(join(abs, e));
    }
  } catch {
    if (abs.endsWith(".pdf")) pdfPaths.push(abs);
  }
}
pdfPaths.sort();
console.log(`Found ${pdfPaths.length} PDFs`);

// -- Parsing -----------------------------------------------------------------

// Header lines use German number format: "12.839,94 Euro"
const AMOUNT_RE = /(-?[\d.]+,\d{2})\s+Euro/;
const DATE_RE = /(\d{2}\.\d{2}\.\d{4})/;

function parseGermanDate(s: string): string {
  const [d, m, y] = s.split(".");
  return `${y}-${m}-${d}`;
}

function parseGermanAmount(s: string): string {
  return s.replace(/\./g, "").replace(",", ".");
}

function extractBalance(pdfPath: string): BalanceSnapshot | null {
  const filename = pdfPath.split("/").pop()!;
  let text: string;
  try {
    // Only need the first page for header data
    text = execSync(`"${PDFTOTEXT}" -f 1 -l 1 -layout "${pdfPath}" -`, {
      encoding: "utf-8",
      maxBuffer: 10 * 1024 * 1024,
    });
  } catch (e: any) {
    console.error(`  WARN: pdftotext failed for ${filename}: ${e.message}`);
    return null;
  }

  const lines = text.split("\n");
  let datum: string | null = null;
  let neuerSaldo: string | null = null;

  for (const line of lines) {
    // The header lines have the label mid-line (preceded by address text),
    // so match anywhere in the line rather than using startsWith.

    // "...  Datum    29.01.2021"
    if (line.includes("Datum") && !datum) {
      const dateMatch = line.match(DATE_RE);
      if (dateMatch) {
        datum = parseGermanDate(dateMatch[1]);
      }
    }

    // "...  Neuer Saldo    27.178,04 Euro"
    // The header line always ends with "Euro"; the end-of-transactions
    // "Neuer Saldo" line does not, so AMOUNT_RE (which requires "Euro")
    // naturally selects the right one.
    if (line.includes("Neuer Saldo")) {
      const amountMatch = line.match(AMOUNT_RE);
      if (amountMatch) {
        neuerSaldo = parseGermanAmount(amountMatch[1]);
      }
    }
  }

  if (!datum || !neuerSaldo) {
    console.error(
      `  WARN: ${filename}: could not extract balance (datum=${datum}, saldo=${neuerSaldo})`
    );
    return null;
  }

  return { date: datum, balance: neuerSaldo, source_file: filename };
}

// -- Main --------------------------------------------------------------------

const snapshots: BalanceSnapshot[] = [];

for (const pdf of pdfPaths) {
  const snapshot = extractBalance(pdf);
  if (snapshot) {
    console.log(
      `  ${snapshot.source_file}: ${snapshot.date}  ${snapshot.balance} EUR`
    );
    snapshots.push(snapshot);
  }
}

snapshots.sort((a, b) => a.date.localeCompare(b.date));
console.log(`\nExtracted ${snapshots.length} balance snapshots`);

if (snapshots.length === 0) {
  console.log("Nothing to import.");
  process.exit(0);
}

console.log(
  `Date range: ${snapshots[0].date} to ${snapshots[snapshots.length - 1].date}`
);

if (dryRun) {
  console.log("\n[DRY RUN] No changes written.");
  for (const s of snapshots) {
    console.log(`  ${s.date}  ${s.balance} EUR  (${s.source_file})`);
  }
  process.exit(0);
}

// -- Generate and execute SQL ------------------------------------------------

const databaseUrl = process.env.DATABASE_URL;
if (!databaseUrl) {
  console.error("DATABASE_URL not set");
  process.exit(1);
}

function sqlEscape(s: string): string {
  return s.replace(/'/g, "''");
}

// Use end-of-day timestamp for the snapshot so it sorts after any intraday
// balance fetches from the bank provider.
const valuesClauses = snapshots
  .map((s) => {
    const id = crypto.randomUUID();
    const snapshotAt = `${s.date}T23:59:59Z`;
    return `  ('${id}', '${ACCOUNT_ID}', ${s.balance}, NULL, 'EUR', '${snapshotAt}')`;
  })
  .join(",\n");

// Guard against duplicates: skip rows where a snapshot already exists for
// this account at the same timestamp.
const sql = `INSERT INTO balance_snapshots (id, account_id, current_balance, available_balance, currency, snapshot_at)
VALUES
${valuesClauses}
ON CONFLICT (id) DO NOTHING;\n`;

const sqlFile = "/tmp/ing-balance-import.sql";
writeFileSync(sqlFile, sql);
console.log(`\nGenerated SQL: ${sqlFile}`);

// Count before
const beforeCount = execSync(
  `psql "${databaseUrl}" -tAc "SELECT COUNT(*) FROM balance_snapshots WHERE account_id = '${ACCOUNT_ID}'"`,
  { encoding: "utf-8" }
).trim();
console.log(`Balance snapshots before import: ${beforeCount}`);

// Check for existing snapshots at the same timestamps to report skips
const existingDates = execSync(
  `psql "${databaseUrl}" -tAc "SELECT snapshot_at::date FROM balance_snapshots WHERE account_id = '${ACCOUNT_ID}'"`,
  { encoding: "utf-8" }
)
  .trim()
  .split("\n")
  .filter(Boolean);

const existingDateSet = new Set(existingDates);
const newSnapshots = snapshots.filter((s) => !existingDateSet.has(s.date));
const skipCount = snapshots.length - newSnapshots.length;

if (skipCount > 0) {
  console.log(`Skipping ${skipCount} snapshots (already exist for those dates)`);
}

if (newSnapshots.length === 0) {
  console.log("All snapshots already exist. Nothing to import.");
  process.exit(0);
}

// Regenerate SQL with only new snapshots
const newValuesClauses = newSnapshots
  .map((s) => {
    const id = crypto.randomUUID();
    const snapshotAt = `${s.date}T23:59:59Z`;
    return `  ('${id}', '${ACCOUNT_ID}', ${s.balance}, NULL, 'EUR', '${snapshotAt}')`;
  })
  .join(",\n");

const newSql = `INSERT INTO balance_snapshots (id, account_id, current_balance, available_balance, currency, snapshot_at)
VALUES
${newValuesClauses};\n`;

writeFileSync(sqlFile, newSql);

try {
  execSync(`psql "${databaseUrl}" -f "${sqlFile}"`, {
    encoding: "utf-8",
    stdio: ["pipe", "pipe", "pipe"],
  });
} catch (e: any) {
  console.error("SQL execution failed:");
  console.error(e.stderr || e.message);
  process.exit(1);
}

const afterCount = execSync(
  `psql "${databaseUrl}" -tAc "SELECT COUNT(*) FROM balance_snapshots WHERE account_id = '${ACCOUNT_ID}'"`,
  { encoding: "utf-8" }
).trim();

const inserted = parseInt(afterCount) - parseInt(beforeCount);
console.log(`\nDone: ${inserted} inserted, ${skipCount} skipped (already exist)`);
console.log(`Total balance snapshots for account: ${afterCount}`);
