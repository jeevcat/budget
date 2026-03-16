// @ts-check
import { test as setup, expect } from "@playwright/test";

const TEST_SECRET = "e2e-test-secret-key";
export const STORAGE_STATE = "playwright/.auth/user.json";

setup("authenticate", async ({ page }) => {
  await page.goto("/");
  await page.getByPlaceholder("API token").fill(TEST_SECRET);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.locator("h1")).toHaveText("Budget", { timeout: 10_000 });
  await page.context().storageState({ path: STORAGE_STATE });
});
