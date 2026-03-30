// @ts-check
import { test, expect } from "@playwright/test";

// Amex CSV date format: DD/MM/YYYY
function amexToday() {
  const d = new Date();
  const dd = String(d.getDate()).padStart(2, "0");
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  return `${dd}/${mm}/${d.getFullYear()}`;
}

// Amex CSV amount format: German locale, positive = charge (money out)
function makeAmexCsv(description, betreff, euros) {
  const header =
    "Datum,Beschreibung,Karteninhaber,Konto #,Betrag,Weitere Details,Erscheint auf Ihrer Abrechnung als,Adresse,Stadt,PLZ,Land,Betreff,Kategorie";
  const amount = String(euros).replace(".", ",");
  const row = `${amexToday()},"${description}","Test User","1234","${amount}","","","","","","","${betreff}","Einkaufen"`;
  return `${header}\n${row}`;
}

test("dashboard spending card renders without JS errors when budget data exists", async ({
  page,
}) => {
  const errors = [];
  page.on("pageerror", (err) => errors.push(err.message));

  // Navigate first so cookies are attached to the page context
  await page.goto("/");
  await expect(page.locator("h2")).toHaveText("Dashboard", { timeout: 10_000 });

  // Create a manual account
  const accountResp = await page.request.post("/api/accounts", {
    data: {
      provider_account_id: "e2e-dashboard-account",
      name: "E2E Checking",
      institution: "E2E Bank",
      account_type: "checking",
      currency: "EUR",
    },
  });
  expect(accountResp.status()).toBe(201);
  const account = await accountResp.json();

  // Import a transaction via CSV
  const csv = makeAmexCsv("Supermarket", "e2e-txn-001", 42.0);
  const importResp = await page.request.post(`/api/accounts/${account.id}/import`, {
    headers: { "Content-Type": "text/csv" },
    data: csv,
  });
  expect(importResp.status()).toBe(200);
  const importResult = await importResp.json();
  expect(importResult.imported).toBe(1);

  // Create a salary category so budget month boundaries can be detected
  const salaryCatResp = await page.request.post("/api/categories", {
    data: { name: "Salary", budget_mode: "salary" },
  });
  expect(salaryCatResp.status()).toBe(201);
  const salaryCat = await salaryCatResp.json();

  // Import a salary transaction (positive = income in Amex CSV = negative amount, so use negative)
  // Amex CSV: positive Betrag = charge (money out). For a salary (money in), use negative Betrag.
  const salaryCsv = makeAmexCsv("Employer Inc", "e2e-salary-001", -3000.0);
  const salaryImportResp = await page.request.post(
    `/api/accounts/${account.id}/import`,
    { headers: { "Content-Type": "text/csv" }, data: salaryCsv }
  );
  expect(salaryImportResp.status()).toBe(200);

  // Create a monthly variable category with a budget
  const catResp = await page.request.post("/api/categories", {
    data: {
      name: "Groceries",
      budget_mode: "monthly",
      budget_type: "variable",
      budget_amount: "200",
    },
  });
  expect(catResp.status()).toBe(201);
  const category = await catResp.json();

  // Find and categorize both transactions
  const txnsResp = await page.request.get("/api/transactions?limit=20");
  expect(txnsResp.status()).toBe(200);
  const txnsBody = await txnsResp.json();

  const salaryTxn = txnsBody.items.find((t) => t.merchant_name === "Employer Inc");
  expect(salaryTxn).toBeTruthy();
  const categorizeSalary = await page.request.post(
    `/api/transactions/${salaryTxn.id}/categorize`,
    { data: { category_id: salaryCat.id } }
  );
  expect(categorizeSalary.status()).toBe(204);

  const groceryTxn = txnsBody.items.find((t) => t.merchant_name === "Supermarket");
  expect(groceryTxn).toBeTruthy();
  const categorizeGrocery = await page.request.post(
    `/api/transactions/${groceryTxn.id}/categorize`,
    { data: { category_id: category.id } }
  );
  expect(categorizeGrocery.status()).toBe(204);

  // Navigate to dashboard and assert spending card renders without errors
  await page.goto("/");
  await expect(page.locator("h1")).toHaveText("Budget", { timeout: 10_000 });

  // Wait for loading to complete (h2 appears once data loads)
  await expect(page.locator("h2")).toHaveText("Dashboard", { timeout: 10_000 });

  // The spending card should be visible (requires budgetData + burndown_points in scope)
  const spendingCard = page.locator("h3", { hasText: "Spending" });
  await expect(spendingCard).toBeVisible();

  // No JS errors should have occurred
  expect(errors).toEqual([]);
});
