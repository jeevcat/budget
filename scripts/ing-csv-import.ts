#!/usr/bin/env bun
//
// Imports ING Girokonto transactions from CSV (produced by ing-pdf-to-csv.ts)
// directly into the PostgreSQL database via psql.
//
// Usage:
//   bun scripts/ing-csv-import.ts /tmp/giro/transactions.csv [--dry-run] [--before YYYY-MM-DD]
//
// Requires DATABASE_URL env var (used by psql).

import { execSync } from "child_process";
import { readFileSync, writeFileSync } from "fs";
import { parseArgs } from "util";

// -- CLI ---------------------------------------------------------------------

const { values, positionals } = parseArgs({
  args: Bun.argv.slice(2),
  options: {
    "dry-run": { type: "boolean" },
    before: { type: "string" },
    account: { type: "string", short: "a" },
    help: { type: "boolean", short: "h" },
  },
  allowPositionals: true,
});

if (values.help || positionals.length === 0) {
  console.log(
    "Usage: bun scripts/ing-csv-import.ts <transactions.csv> --account <uuid> [--dry-run] [--before YYYY-MM-DD]"
  );
  console.log("  --account     Account UUID to import into (required)");
  console.log("  --dry-run     Show what would be inserted without writing");
  console.log(
    "  --before      Only import transactions before this date (exclusive)"
  );
  process.exit(0);
}

const csvPath = positionals[0];
const dryRun = values["dry-run"] ?? false;
const beforeDate = values.before ?? null;
const ACCOUNT_ID = values.account;

if (!ACCOUNT_ID) {
  console.error("--account <uuid> is required");
  process.exit(1);
}

// Validate UUID format
if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(ACCOUNT_ID)) {
  console.error("Invalid account UUID: " + ACCOUNT_ID);
  process.exit(1);
}

// -- CSV parsing -------------------------------------------------------------

interface CsvRow {
  posted_date: string;
  type: string;
  merchant_name: string;
  amount: string;
  remittance: string;
  referenz: string;
  mandat: string;
  source_file: string;
}

function parseCSVLine(line: string): string[] {
  const fields: string[] = [];
  let field = "";
  let inQuotes = false;
  for (const ch of line) {
    if (ch === '"') {
      inQuotes = !inQuotes;
    } else if (ch === "," && !inQuotes) {
      fields.push(field);
      field = "";
    } else {
      field += ch;
    }
  }
  fields.push(field);
  return fields;
}

function loadCsv(path: string): CsvRow[] {
  const text = readFileSync(path, "utf-8");
  const lines = text.split("\n").filter((l) => l.trim());
  const header = lines[0];
  const expected =
    "posted_date,type,merchant_name,amount,remittance,referenz,mandat,source_file";
  if (header !== expected) {
    console.error(
      `Unexpected CSV header:\n  got:      ${header}\n  expected: ${expected}`
    );
    process.exit(1);
  }

  const rows: CsvRow[] = [];
  for (let i = 1; i < lines.length; i++) {
    const fields = parseCSVLine(lines[i]);
    if (fields.length < 8) {
      console.error(`Line ${i + 1}: expected 8 fields, got ${fields.length}`);
      continue;
    }
    rows.push({
      posted_date: fields[0],
      type: fields[1],
      merchant_name: fields[2],
      amount: fields[3],
      remittance: fields[4],
      referenz: fields[5],
      mandat: fields[6],
      source_file: fields[7],
    });
  }
  return rows;
}

// -- Map PDF transaction types to DB bank_transaction_code -------------------
//
// The Enable Banking API uses these codes for the same ING account:
//   Lastschrifteinzug, Gutschrift, Gehalt/Rente, Entgelt,
//   Echtzeitüberweisung, Dauerauftrag/Terminueberweisung, Überweisung
//
// The PDF uses slightly different names. Map them to match.

function mapBankTransactionCode(pdfType: string): string {
  switch (pdfType) {
    case "Lastschrift":
      return "Lastschrifteinzug";
    case "Gutschrift":
      return "Gutschrift";
    case "Gehalt/Rente":
    case "Gehalt":
      return "Gehalt/Rente";
    case "Entgelt":
      return "Entgelt";
    case "Echtzeitüberweisung":
      return "Echtzeitüberweisung";
    case "Dauerauftrag/Terminueberw.":
      return "Dauerauftrag/Terminueberweisung";
    case "Ueberweisung":
      return "Überweisung";
    case "Retoure":
      return "Retoure";
    case "Kapitalertragsteuer":
      return "Kapitalertragsteuer";
    case "Auszahlung":
      return "Auszahlung";
    case "Einzahlung":
      return "Einzahlung";
    case "Abschluss":
      return "Abschluss";
    case "Barabhebung":
      return "Barabhebung";
    case "Abbuchung":
      return "Abbuchung";
    case "Bezuege":
      return "Gehalt/Rente";
    case "Storno":
      return "Storno";
    default:
      return pdfType;
  }
}

// -- Build remittance_information matching Enable Banking format --------------
//
// Existing ING transactions store a single element in the format:
//   "mandatereference:<val>,creditorid:<val>,remittanceinformation:<val>"
//
// The PDF doesn't have creditorid, so we leave it empty (matching the pattern
// used by VISA/Entgelt transactions from Enable Banking).

function buildRemittanceInfo(row: CsvRow): string {
  const mandat = row.mandat || "";
  const remittance = row.remittance || "";
  return `mandatereference:${mandat},creditorid:,remittanceinformation:${remittance}`;
}

// -- Generate a stable provider_transaction_id for dedup --------------------
//
// The unique index is (account_id, provider_transaction_id).
// We need deterministic IDs so re-running the import is idempotent.
// Format: "pdf-<date>-<hash>" where hash covers all distinguishing fields.

function hashString(s: string): string {
  const hash = Bun.hash(s);
  return (hash & 0xffffffffffffn).toString(16).padStart(12, "0");
}

function makeProviderTransactionId(row: CsvRow, index: number): string {
  const input = `${row.posted_date}|${row.amount}|${row.merchant_name}|${row.remittance}|${index}`;
  return `pdf-${row.posted_date}-${hashString(input)}`;
}

// -- SQL generation ----------------------------------------------------------

function sqlEscape(s: string): string {
  return s.replace(/'/g, "''");
}

function generateUUID(): string {
  return crypto.randomUUID();
}

function rowToSQL(row: CsvRow, index: number): string {
  const id = generateUUID();
  const merchantName = row.merchant_name || row.type;
  const remittanceInfo = buildRemittanceInfo(row);
  const bankTxnCode = mapBankTransactionCode(row.type);
  const providerTxnId = makeProviderTransactionId(row, index);

  return (
    `  ('${id}', '${ACCOUNT_ID}', ${row.amount}, ` +
    `'${sqlEscape(merchantName)}', ` +
    `ARRAY['${sqlEscape(remittanceInfo)}']::text[], ` +
    `'${row.posted_date}', ` +
    `'${sqlEscape(bankTxnCode)}', ` +
    `'${sqlEscape(providerTxnId)}', ` +
    `false)`
  );
}

// -- Main --------------------------------------------------------------------

const rows = loadCsv(csvPath);
console.log(`Loaded ${rows.length} transactions from CSV`);

// Filter by date if --before is set
let filtered = rows;
if (beforeDate) {
  filtered = rows.filter((r) => r.posted_date < beforeDate);
  console.log(
    `Filtered to ${filtered.length} transactions before ${beforeDate}`
  );
}

if (filtered.length === 0) {
  console.log("Nothing to import.");
  process.exit(0);
}

// Summary
const dateRange = `${filtered[0].posted_date} to ${filtered[filtered.length - 1].posted_date}`;
console.log(`Date range: ${dateRange}`);
console.log(`Transactions to import: ${filtered.length}`);

// Type breakdown
const typeCounts: Record<string, number> = {};
for (const row of filtered) {
  const code = mapBankTransactionCode(row.type);
  typeCounts[code] = (typeCounts[code] || 0) + 1;
}
console.log("\nType breakdown:");
for (const [code, count] of Object.entries(typeCounts).sort(
  (a, b) => b[1] - a[1]
)) {
  console.log(`  ${code}: ${count}`);
}

// Amount sanity check
const totalAmount = filtered.reduce(
  (sum, r) => sum + parseFloat(r.amount),
  0
);
const positiveCount = filtered.filter(
  (r) => parseFloat(r.amount) > 0
).length;
const negativeCount = filtered.filter(
  (r) => parseFloat(r.amount) < 0
).length;
console.log(
  `\nTotal amount: ${totalAmount.toFixed(2)} (${positiveCount} credits, ${negativeCount} debits)`
);

if (dryRun) {
  console.log("\n[DRY RUN] No changes written.");
  console.log("\nSample rows (first 5):");
  for (let i = 0; i < Math.min(5, filtered.length); i++) {
    const row = filtered[i];
    console.log({
      posted_date: row.posted_date,
      merchant_name: row.merchant_name || row.type,
      amount: row.amount,
      bank_transaction_code: mapBankTransactionCode(row.type),
      provider_transaction_id: makeProviderTransactionId(row, i),
      remittance_info:
        buildRemittanceInfo(row).slice(0, 100) + "...",
    });
  }
  process.exit(0);
}

// -- Generate and execute SQL ------------------------------------------------

const databaseUrl = process.env.DATABASE_URL;
if (!databaseUrl) {
  console.error("DATABASE_URL not set");
  process.exit(1);
}

// Build SQL in batches to avoid exceeding psql limits
const BATCH_SIZE = 200;
const sqlFile = "/tmp/ing-import.sql";
let totalInserted = 0;
let totalSkipped = 0;

// Wrap everything in a transaction
let sqlContent = "BEGIN;\n\n";

for (let i = 0; i < filtered.length; i += BATCH_SIZE) {
  const batch = filtered.slice(i, i + BATCH_SIZE);
  const valuesClauses = batch
    .map((row, j) => rowToSQL(row, i + j))
    .join(",\n");

  sqlContent += `INSERT INTO transactions (
  id, account_id, amount, merchant_name, remittance_information,
  posted_date, bank_transaction_code, provider_transaction_id,
  skip_correlation
)
VALUES
${valuesClauses}
ON CONFLICT (account_id, provider_transaction_id) DO NOTHING;\n\n`;
}

sqlContent += "COMMIT;\n";

writeFileSync(sqlFile, sqlContent);
console.log(`\nGenerated SQL: ${sqlFile} (${(sqlContent.length / 1024).toFixed(0)} KB)`);

// Get count before
const beforeCount = execSync(
  `psql "${databaseUrl}" -tAc "SELECT COUNT(*) FROM transactions WHERE account_id = '${ACCOUNT_ID}'"`,
  { encoding: "utf-8" }
).trim();
console.log(`Transactions before import: ${beforeCount}`);

// Execute
try {
  execSync(`psql "${databaseUrl}" -f "${sqlFile}"`, {
    encoding: "utf-8",
    stdio: ["pipe", "pipe", "pipe"],
    maxBuffer: 10 * 1024 * 1024,
  });
} catch (e: any) {
  console.error("SQL execution failed:");
  console.error(e.stderr || e.message);
  process.exit(1);
}

// Get count after
const afterCount = execSync(
  `psql "${databaseUrl}" -tAc "SELECT COUNT(*) FROM transactions WHERE account_id = '${ACCOUNT_ID}'"`,
  { encoding: "utf-8" }
).trim();

const inserted = parseInt(afterCount) - parseInt(beforeCount);
const skipped = filtered.length - inserted;

console.log(
  `\nDone: ${inserted} inserted, ${skipped} skipped (duplicates)`
);
console.log(`Total transactions for account: ${afterCount}`);
