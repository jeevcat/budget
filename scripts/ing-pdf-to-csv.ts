#!/usr/bin/env bun
//
// Parses ING Girokonto PDF bank statements (Kontoauszug) into a CSV file.
//
// Usage:
//   bun scripts/ing-pdf-to-csv.ts /tmp/giro/ -o /tmp/giro/transactions.csv
//
// Requires pdftotext (poppler-utils) on PATH or available via nix.

import { execSync } from "child_process";
import { existsSync, readdirSync, writeFileSync } from "fs";
import { join, resolve } from "path";
import { parseArgs } from "util";

// -- Locate pdftotext --------------------------------------------------------

function findPdftotext(): string {
  try {
    return execSync("which pdftotext", { encoding: "utf-8" }).trim();
  } catch {
    // Try nix
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

interface Transaction {
  posted_date: string; // YYYY-MM-DD
  type: string; // Lastschrift, Gutschrift, etc.
  merchant_name: string;
  amount: string; // decimal string, negative = debit
  remittance: string; // full Verwendungszweck text
  referenz: string; // Referenz: field if present
  mandat: string; // Mandat: field if present
  source_file: string;
}

// -- CLI ---------------------------------------------------------------------

const { values, positionals } = parseArgs({
  args: Bun.argv.slice(2),
  options: {
    output: { type: "string", short: "o" },
    help: { type: "boolean", short: "h" },
  },
  allowPositionals: true,
});

if (values.help || positionals.length === 0) {
  console.log(
    "Usage: bun scripts/ing-pdf-to-csv.ts <pdf-dir-or-file>... -o <output.csv>"
  );
  process.exit(0);
}

const outputPath = values.output ?? "/tmp/giro/transactions.csv";

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

// -- Transaction type keywords -----------------------------------------------

const TXN_TYPES = [
  "Lastschrift",
  "Gutschrift",
  "Ueberweisung",
  "Entgelt",
  "Dauerauftrag/Terminueberw.",
  "Gehalt/Rente",
  "Gehalt",
  "Echtzeitüberweisung",
  "Retoure",
  "Kapitalertragsteuer",
  "Auszahlung",
  "Einzahlung",
  "Abschluss",
  "Barabhebung",
  "Abbuchung",
  "Bezuege",
  "Storno",
];

// -- Parsing -----------------------------------------------------------------

// Match a line that starts a new transaction:
//   DD.MM.YYYY   Type MerchantName   Amount
const DATE_RE = /^\s+(\d{2}\.\d{2}\.\d{4})(?:\s+|$)/;
// Amounts end with exactly 2 decimal digits, but pdftotext sometimes appends
// a page number digit (e.g. "-529,881" where "1" is the page number).
const AMOUNT_RE = /(-?[\d.]+,\d{2})\d?\s*$/;

function parseGermanDate(s: string): string {
  const [d, m, y] = s.split(".");
  return `${y}-${m}-${d}`;
}

function parseGermanAmount(s: string): string {
  // "1.234,56" → "1234.56", "-47,98" → "-47.98"
  return s.replace(/\./g, "").replace(",", ".");
}

function extractTypeAndMerchant(
  text: string
): { type: string; merchant: string } | null {
  for (const t of TXN_TYPES) {
    if (text.startsWith(t + " ")) {
      return { type: t, merchant: text.slice(t.length + 1).trim() };
    }
    if (text === t) {
      return { type: t, merchant: "" };
    }
  }
  return null;
}

function isPageNoise(line: string): boolean {
  const trimmed = line.trim();
  if (trimmed === "") return true;
  if (trimmed.startsWith("34G")) return true;
  if (trimmed.startsWith("ING-DiBa AG")) return true;
  if (trimmed.startsWith("Girokonto Nummer")) return true;
  if (trimmed.startsWith("Kontoauszug ")) return true;
  if (/^\s*Datum\s/.test(line)) return true;
  if (/^\s*Seite\s/.test(line)) return true;
  if (trimmed === "Buchung                    Buchung / Verwendungszweck                                                                                                         Betrag (EUR)")
    return true;
  if (trimmed.startsWith("Buchung") && trimmed.includes("Verwendungszweck"))
    return true;
  if (trimmed === "Valuta") return true;
  // Footer lines with board member names, legal text, etc.
  if (
    trimmed.startsWith("Michael Clijdesdale") ||
    trimmed.startsWith("Dr. Joachim") ||
    trimmed.startsWith("Steuernummer") ||
    trimmed.startsWith("HRB 7727")
  )
    return true;
  return false;
}

function isEndMarker(line: string): boolean {
  const trimmed = line.trim();
  if (trimmed.startsWith("Neuer Saldo")) return true;
  if (trimmed.startsWith("Kunden-Information")) return true;
  if (trimmed.startsWith("Bitte beachten")) return true;
  if (trimmed.startsWith("Vorliegender Freistellungsauftrag")) return true;
  if (trimmed.startsWith("Wir wünschen")) return true;
  if (trimmed.startsWith("Wir danken")) return true;
  if (trimmed.startsWith("Ihre ING")) return true;
  // Boilerplate legal text after statement ends
  if (trimmed.startsWith("Kontoauszug ohne Rechnungsabschluss")) return true;
  if (trimmed.startsWith("Rechnungsabschluss")) return true;
  if (trimmed.startsWith("Wir bitten Sie")) return true;
  if (trimmed.startsWith("Einlagensicherung")) return true;
  if (trimmed.startsWith("Sollzins")) return true;
  if (trimmed.startsWith("Guthaben sind")) return true;
  return false;
}

function parsePdf(pdfPath: string): Transaction[] {
  const filename = pdfPath.split("/").pop()!;
  let text: string;
  try {
    text = execSync(`"${PDFTOTEXT}" -layout "${pdfPath}" -`, {
      encoding: "utf-8",
      maxBuffer: 10 * 1024 * 1024,
    });
  } catch (e: any) {
    console.error(`  WARN: pdftotext failed for ${filename}: ${e.message}`);
    return [];
  }

  const lines = text.split("\n");
  const transactions: Transaction[] = [];
  let current: Transaction | null = null;
  let remittanceLines: string[] = [];
  let done = false;
  let afterPageBreak = false;

  function finalize() {
    if (!current) return;
    if (current.amount === "") {
      // Transaction never got its amount (shouldn't happen normally)
      console.error(`  WARN: ${filename}: dropping transaction without amount: ${current.type} ${current.merchant_name} on ${current.posted_date}`);
      current = null;
      remittanceLines = [];
      return;
    }
    const fullRemittance = remittanceLines.join(" ").trim();

    // Extract Referenz: and Mandat: from remittance
    const refMatch = fullRemittance.match(/Referenz:\s*(.+?)(?:\s+Mandat:|$)/);
    const mandatMatch = fullRemittance.match(/Mandat:\s*(.+?)(?:\s+Referenz:|$)/);
    current.remittance = fullRemittance;
    current.referenz = refMatch ? refMatch[1].trim() : "";
    current.mandat = mandatMatch ? mandatMatch[1].trim() : "";
    transactions.push(current);
    current = null;
    remittanceLines = [];
  }

  for (const line of lines) {
    if (done) break;
    if (line.trim().startsWith("34G")) {
      afterPageBreak = true;
    }
    if (isPageNoise(line)) continue;
    if (isEndMarker(line)) {
      // "Neuer Saldo" on the last page signals end of transactions.
      // But some pages have it as a running total mid-statement — only
      // treat it as terminal if followed by non-transaction content.
      // Safe heuristic: finalize current and stop if "Neuer Saldo" has an amount.
      if (line.trim().startsWith("Neuer Saldo")) {
        finalize();
        done = true;
        break;
      }
      finalize();
      done = true;
      break;
    }

    const dateMatch = line.match(DATE_RE);
    const amountMatch = line.match(AMOUNT_RE);

    // Reset page break flag once we process a non-noise content line
    const wasAfterPageBreak = afterPageBreak;
    afterPageBreak = false;

    if (dateMatch && amountMatch) {
      const dateStr = dateMatch[1];
      const afterDate = line.slice(dateMatch[0].length);
      const amountStr = amountMatch[1];
      // Text between date and amount
      const beforeAmount = afterDate
        .slice(0, afterDate.lastIndexOf(amountStr))
        .trim();

      const parsed = extractTypeAndMerchant(beforeAmount);
      if (parsed) {
        // Transaction header line: date + type/merchant + amount
        finalize();
        current = {
          posted_date: parseGermanDate(dateStr),
          type: parsed.type,
          merchant_name: parsed.merchant,
          amount: parseGermanAmount(amountStr),
          remittance: "",
          referenz: "",
          mandat: "",
          source_file: filename,
        };
      } else if (current && current.amount === "" && beforeAmount === "") {
        // Date + amount only, no text — this is the amount for a
        // transaction whose type+merchant was on the previous page
        current.posted_date = parseGermanDate(dateStr);
        current.amount = parseGermanAmount(amountStr);
      } else if (current) {
        // No recognized type — this is a valuta/remittance line that
        // happens to end with an amount-like number (e.g. foreign
        // currency amounts like "KURS 4,9115000 KAUFUMSATZ 15.08 96,06")
        const afterDateText = line.slice(dateMatch[0].length).trim();
        if (afterDateText) {
          remittanceLines.push(afterDateText);
        }
      }
    } else if (dateMatch && !amountMatch) {
      const afterDate = line.slice(dateMatch[0].length).trim();

      // Check if this is a type+merchant line with no amount (split across pages).
      // This only happens at page boundaries where pdftotext puts the type+merchant
      // on one page and the amount on the next. We guard with afterPageBreak to
      // avoid false positives on remittance lines that start with type keywords
      // (e.g. "Einzahlung Smart Invest" as remittance text).
      const parsed = afterDate ? extractTypeAndMerchant(afterDate) : null;
      if (parsed && wasAfterPageBreak) {
        finalize();
        current = {
          posted_date: parseGermanDate(dateMatch[1]),
          type: parsed.type,
          merchant_name: parsed.merchant,
          amount: "", // filled in when the amount line arrives
          remittance: "",
          referenz: "",
          mandat: "",
          source_file: filename,
        };
      } else if (current) {
        // Valuta date line — this is remittance text
        if (afterDate) {
          remittanceLines.push(afterDate);
        }
      }
    } else if (!dateMatch && current) {
      // Continuation line (Mandat:, Referenz:, wrapped text, etc.)
      const trimmed = line.trim();
      if (trimmed) {
        // Check for a page-split transaction: a continuation line that starts
        // with a type keyword is a new transaction whose date+amount are on
        // separate lines. This happens when pdftotext splits the header across
        // the page boundary (e.g. date on previous line, "Lastschrift ..." here,
        // amount after the page break).
        const continuationParsed = extractTypeAndMerchant(trimmed);
        if (continuationParsed && current.amount !== "") {
          const prevDate = current.posted_date;
          finalize();
          current = {
            posted_date: prevDate,
            type: continuationParsed.type,
            merchant_name: continuationParsed.merchant,
            amount: "", // filled in when the amount line arrives
            remittance: "",
            referenz: "",
            mandat: "",
            source_file: filename,
          };
        } else if (
          remittanceLines.length === 0 &&
          current.merchant_name &&
          !trimmed.startsWith("Mandat:") &&
          !trimmed.startsWith("Referenz:") &&
          !trimmed.startsWith("Folgenr.") &&
          !trimmed.startsWith("ARN") &&
          !trimmed.startsWith("NR ") &&
          current.merchant_name.endsWith("-") ||
          // Also detect wraps where the type+merchant line was truncated
          (remittanceLines.length === 0 &&
            current.merchant_name === "" &&
            !trimmed.startsWith("Mandat:") &&
            !trimmed.startsWith("Referenz:"))
        ) {
          // Merchant name continuation
          if (current.merchant_name.endsWith("-")) {
            current.merchant_name += trimmed;
          } else if (current.merchant_name === "") {
            current.merchant_name = trimmed;
          } else {
            current.merchant_name += " " + trimmed;
          }
        } else {
          remittanceLines.push(trimmed);
        }
      }
    }
  }

  finalize();
  return transactions;
}

// -- Main --------------------------------------------------------------------

const allTransactions: Transaction[] = [];

for (const pdf of pdfPaths) {
  const txns = parsePdf(pdf);
  console.log(`  ${pdf.split("/").pop()}: ${txns.length} transactions`);
  allTransactions.push(...txns);
}

// Sort by date
allTransactions.sort((a, b) => a.posted_date.localeCompare(b.posted_date));

// Write CSV
const header =
  "posted_date,type,merchant_name,amount,remittance,referenz,mandat,source_file";
const rows = allTransactions.map((t) => {
  const fields = [
    t.posted_date,
    t.type,
    t.merchant_name,
    t.amount,
    t.remittance,
    t.referenz,
    t.mandat,
    t.source_file,
  ];
  return fields.map(csvEscape).join(",");
});

writeFileSync(outputPath, [header, ...rows].join("\n") + "\n");
console.log(
  `\nWrote ${allTransactions.length} transactions to ${outputPath}`
);

// Date range summary
if (allTransactions.length > 0) {
  const first = allTransactions[0].posted_date;
  const last = allTransactions[allTransactions.length - 1].posted_date;
  console.log(`Date range: ${first} to ${last}`);
}

function csvEscape(s: string): string {
  if (s.includes(",") || s.includes('"') || s.includes("\n")) {
    return '"' + s.replace(/"/g, '""') + '"';
  }
  return s;
}
