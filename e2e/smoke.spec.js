// @ts-check
import { test, expect } from "@playwright/test";

test("budget page loads and shows Budget heading", async ({ page }) => {
  await page.goto("/");
  await expect(page.locator("h1")).toHaveText("Budget", { timeout: 10_000 });
  await expect(page.locator("nav")).toBeVisible();
});

test("insights page loads", async ({ page }) => {
  await page.goto("/#/insights");
  await expect(page.locator("h2")).toHaveText("Insights", { timeout: 10_000 });
});

test("no burndown card on empty database", async ({ page }) => {
  await page.goto("/#/insights");
  await expect(page.locator("h2")).toHaveText("Insights", { timeout: 10_000 });
  const burndown = page.locator("h3", { hasText: "Budget Burndown" });
  await expect(burndown).not.toBeVisible();
});
