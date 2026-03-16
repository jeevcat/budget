import { describe, expect, test } from "bun:test";
import {
  accountDisplayName,
  amountClass,
  budgetModeColor,
  buildCategoryTree,
  cardCounts,
  categoryBudgetMode,
  categoryBudgetType,
  categoryLabel,
  categoryName,
  categoryQualifiedName,
  cleanMerchant,
  formatAmount,
  formatDateRange,
  formatDateShort,
  formatOrdinal,
  formatRemittanceInfo,
  paceBadge,
  paceColor,
  paceLabel,
  QUEUE_CARDS,
  shortType,
  syncUrlFor,
  timeAgo,
  titleCase,
  transactionTitle,
} from "./helpers.js";

// ---------------------------------------------------------------------------
// accountDisplayName
// ---------------------------------------------------------------------------

describe("accountDisplayName", () => {
  test("returns nickname when present", () => {
    expect(accountDisplayName({ nickname: "Savings", name: "DE123" })).toBe(
      "Savings",
    );
  });

  test("falls back to name", () => {
    expect(accountDisplayName({ name: "DE123" })).toBe("DE123");
  });

  test("returns empty string for null", () => {
    expect(accountDisplayName(null)).toBe("");
  });

  test("returns empty string for empty object", () => {
    expect(accountDisplayName({})).toBe("");
  });
});

// ---------------------------------------------------------------------------
// formatAmount
// ---------------------------------------------------------------------------

describe("formatAmount", () => {
  test("formats positive amount", () => {
    const result = formatAmount(1234.5);
    expect(result).toContain("1");
    expect(result).toContain("234");
    expect(result).toContain("50");
    expect(result).toContain("\u20AC");
  });

  test("formats zero decimals", () => {
    const result = formatAmount(100, { decimals: 0 });
    expect(result).toContain("100");
    expect(result).not.toContain(".");
  });

  test("formats with sign positive", () => {
    const result = formatAmount(50, { sign: true });
    expect(result).toContain("+");
    expect(result).toContain("\u20AC");
  });

  test("formats with sign negative", () => {
    const result = formatAmount(-50, { sign: true });
    expect(result).toContain("\u2212");
    expect(result).toContain("50");
  });

  test("zero with sign has no sign prefix", () => {
    const result = formatAmount(0, { sign: true });
    expect(result).not.toContain("+");
    expect(result).not.toContain("\u2212");
  });
});

// ---------------------------------------------------------------------------
// amountClass
// ---------------------------------------------------------------------------

describe("amountClass", () => {
  test("positive amount", () => {
    expect(amountClass(100)).toBe("amount-positive");
  });

  test("negative amount", () => {
    expect(amountClass(-100)).toBe("");
  });

  test("zero", () => {
    expect(amountClass(0)).toBe("");
  });
});

// ---------------------------------------------------------------------------
// categoryLabel
// ---------------------------------------------------------------------------

describe("categoryLabel", () => {
  const catMap = {
    1: { id: 1, name: "Food", parent_id: null },
    2: { id: 2, name: "Food:Groceries", parent_id: 1 },
    3: { id: 3, name: "Transport:Fuel", parent_id: null },
    4: { id: 4, name: "Entertainment", parent_id: null },
  };

  test("returns null for missing id", () => {
    expect(categoryLabel(catMap, null)).toBeNull();
    expect(categoryLabel(catMap, 999)).toBeNull();
  });

  test("child with parent in map", () => {
    const label = categoryLabel(catMap, 2);
    expect(label.parent).toBe("Food");
    expect(label.short).toBe("Groceries");
  });

  test("root with colon in name", () => {
    const label = categoryLabel(catMap, 3);
    expect(label.parent).toBe("Transport");
    expect(label.short).toBe("Fuel");
  });

  test("simple root category", () => {
    const label = categoryLabel(catMap, 4);
    expect(label.parent).toBeNull();
    expect(label.short).toBe("Entertainment");
  });
});

// ---------------------------------------------------------------------------
// categoryName / categoryQualifiedName
// ---------------------------------------------------------------------------

describe("categoryName", () => {
  const catMap = {
    1: { id: 1, name: "Food", parent_id: null },
    2: { id: 2, name: "Food:Groceries", parent_id: 1 },
  };

  test("formats parent > short", () => {
    expect(categoryName(catMap, 2)).toBe("Food > Groceries");
  });

  test("returns null for missing", () => {
    expect(categoryName(catMap, 999)).toBeNull();
  });
});

describe("categoryQualifiedName", () => {
  const catMap = {
    1: { id: 1, name: "Food", parent_id: null },
    2: { id: 2, name: "Food:Groceries", parent_id: 1 },
    3: { id: 3, name: "Entertainment", parent_id: null },
  };

  test("formats parent:short", () => {
    expect(categoryQualifiedName(catMap, 2)).toBe("Food:Groceries");
  });

  test("simple name without parent", () => {
    expect(categoryQualifiedName(catMap, 3)).toBe("Entertainment");
  });
});

// ---------------------------------------------------------------------------
// categoryBudgetMode
// ---------------------------------------------------------------------------

describe("categoryBudgetMode", () => {
  test("returns own budget_mode", () => {
    const catMap = { 1: { budget_mode: "monthly" } };
    expect(categoryBudgetMode(catMap, 1)).toBe("monthly");
  });

  test("inherits from parent", () => {
    const catMap = {
      1: { budget_mode: "annual" },
      2: { parent_id: 1, budget_mode: null },
    };
    expect(categoryBudgetMode(catMap, 2)).toBe("annual");
  });

  test("returns null for no id", () => {
    expect(categoryBudgetMode({}, null)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// categoryBudgetType
// ---------------------------------------------------------------------------

describe("categoryBudgetType", () => {
  test("returns own budget_type", () => {
    const catMap = { 1: { budget_type: "fixed" } };
    expect(categoryBudgetType(catMap, 1)).toBe("fixed");
  });

  test("inherits from parent", () => {
    const catMap = {
      1: { budget_type: "fixed" },
      2: { parent_id: 1, budget_type: null },
    };
    expect(categoryBudgetType(catMap, 2)).toBe("fixed");
  });

  test("returns null for no id", () => {
    expect(categoryBudgetType({}, null)).toBeNull();
  });

  test("returns null when not set", () => {
    const catMap = { 1: {} };
    expect(categoryBudgetType(catMap, 1)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// budgetModeColor
// ---------------------------------------------------------------------------

describe("budgetModeColor", () => {
  test("monthly", () => expect(budgetModeColor("monthly")).toBe("cat-monthly"));
  test("annual", () => expect(budgetModeColor("annual")).toBe("cat-annual"));
  test("project", () => expect(budgetModeColor("project")).toBe("cat-project"));
  test("salary", () => expect(budgetModeColor("salary")).toBe("cat-salary"));
  test("transfer", () =>
    expect(budgetModeColor("transfer")).toBe("cat-transfer"));
  test("unknown", () => expect(budgetModeColor("other")).toBe(""));
});

// ---------------------------------------------------------------------------
// buildCategoryTree
// ---------------------------------------------------------------------------

describe("buildCategoryTree", () => {
  test("sorts roots alphabetically, nests children", () => {
    const categories = [
      { id: 1, name: "Zebra", parent_id: null },
      { id: 2, name: "Alpha", parent_id: null },
      { id: 3, name: "Alpha:Beta", parent_id: 2 },
      { id: 4, name: "Alpha:Aardvark", parent_id: 2 },
    ];
    const tree = buildCategoryTree(categories);
    expect(tree.length).toBe(3);
    // Alpha's children sorted: Aardvark before Beta
    expect(tree[0].name).toBe("Alpha:Aardvark");
    expect(tree[0].depth).toBe(1);
    expect(tree[1].name).toBe("Alpha:Beta");
    expect(tree[1].depth).toBe(1);
    // Zebra (no children) at depth 0
    expect(tree[2].name).toBe("Zebra");
    expect(tree[2].depth).toBe(0);
  });

  test("root without children gets depth 0", () => {
    const tree = buildCategoryTree([{ id: 1, name: "Solo", parent_id: null }]);
    expect(tree.length).toBe(1);
    expect(tree[0].depth).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// titleCase / cleanMerchant
// ---------------------------------------------------------------------------

describe("titleCase", () => {
  test("capitalizes words", () => {
    expect(titleCase("hello world")).toBe("Hello World");
  });

  test("handles separators", () => {
    expect(titleCase("foo-bar/baz.qux")).toBe("Foo-Bar/Baz.Qux");
  });
});

describe("cleanMerchant", () => {
  test("strips VISA prefix", () => {
    expect(cleanMerchant("VISA SOME STORE")).toBe("Some Store");
  });

  test("strips SUMUP prefix", () => {
    expect(cleanMerchant("SUMUP * COFFEE SHOP")).toBe("Coffee Shop");
  });

  test("title-cases all-upper names", () => {
    expect(cleanMerchant("REWE MARKT")).toBe("Rewe Markt");
  });

  test("preserves mixed case", () => {
    expect(cleanMerchant("McDonald's")).toBe("McDonald's");
  });

  test("returns null/undefined as-is", () => {
    expect(cleanMerchant(null)).toBeNull();
    expect(cleanMerchant(undefined)).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// transactionTitle
// ---------------------------------------------------------------------------

describe("transactionTitle", () => {
  test("prefers llm_title when present", () => {
    expect(
      transactionTitle({
        llm_title: "Tim Ho Wan",
        counterparty_name: "BBMSL",
        merchant_name: "BBMSL*Tim Ho Wan Dim Su HK",
      }),
    ).toBe("Tim Ho Wan");
  });

  test("falls back to counterparty_name", () => {
    expect(
      transactionTitle({
        counterparty_name: "Dagmar Jost",
        merchant_name: "SEPA TRANSFER 12345",
      }),
    ).toBe("Dagmar Jost");
  });

  test("falls back to cleanMerchant", () => {
    expect(transactionTitle({ merchant_name: "VISA CITYBUS" })).toBe("Citybus");
  });

  test("uses remittance_information as last resort", () => {
    expect(
      transactionTitle({
        merchant_name: "",
        remittance_information: ["PAYMENT REF 123"],
      }),
    ).toBe("Payment Ref 123");
  });

  test("handles empty transaction", () => {
    expect(transactionTitle({})).toBeFalsy();
  });
});

// ---------------------------------------------------------------------------
// formatRemittanceInfo
// ---------------------------------------------------------------------------

describe("formatRemittanceInfo", () => {
  test("filters empty segments", () => {
    expect(formatRemittanceInfo(["hello", "", "  ", "world"])).toEqual([
      "hello",
      "world",
    ]);
  });

  test("returns null for empty/null input", () => {
    expect(formatRemittanceInfo(null)).toBeNull();
    expect(formatRemittanceInfo([])).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// paceBadge / paceLabel / paceColor
// ---------------------------------------------------------------------------

describe("paceBadge", () => {
  test("maps pace values to badge variants", () => {
    expect(paceBadge("pending")).toBe("secondary");
    expect(paceBadge("under_budget")).toBe("success");
    expect(paceBadge("on_track")).toBe("primary");
    expect(paceBadge("above_pace")).toBe("warning");
    expect(paceBadge("over_budget")).toBe("danger");
  });
});

describe("paceLabel", () => {
  test("returns base label without delta", () => {
    expect(paceLabel("on_track")).toBe("On track");
    expect(paceLabel("pending")).toBe("Pending");
  });

  test("includes delta for under_budget", () => {
    const label = paceLabel("under_budget", -50);
    expect(label).toContain("Under pace");
    expect(label).toContain("50");
  });

  test("includes delta for over_budget", () => {
    const label = paceLabel("over_budget", 100);
    expect(label).toContain("Over budget");
    expect(label).toContain("100");
  });

  test("does not include delta for on_track", () => {
    expect(paceLabel("on_track", 10)).toBe("On track");
  });

  test("appends seasonal when seasonalFactor is present", () => {
    const label = paceLabel("on_track", null, 1.15);
    expect(label).toBe("On track (seasonal)");
  });

  test("combines delta and seasonal", () => {
    const label = paceLabel("above_pace", 30, 1.2);
    expect(label).toContain("Above pace");
    expect(label).toContain("30");
    expect(label).toContain("(seasonal)");
  });

  test("no seasonal suffix when null", () => {
    expect(paceLabel("on_track", null, null)).toBe("On track");
  });
});

describe("paceColor", () => {
  test("maps pace to CSS var", () => {
    expect(paceColor("over_budget")).toBe("var(--danger)");
    expect(paceColor("above_pace")).toBe("var(--warning)");
    expect(paceColor("on_track")).toBe("var(--primary)");
    expect(paceColor("under_budget")).toBe("var(--success)");
    expect(paceColor("pending")).toBe("var(--text-light)");
  });
});

// ---------------------------------------------------------------------------
// timeAgo
// ---------------------------------------------------------------------------

describe("timeAgo", () => {
  test("returns dash for null", () => {
    expect(timeAgo(null)).toBe("\u2014");
  });

  test("seconds ago", () => {
    const iso = new Date(Date.now() - 30000).toISOString();
    expect(timeAgo(iso)).toMatch(/30s ago/);
  });

  test("minutes ago", () => {
    const iso = new Date(Date.now() - 5 * 60000).toISOString();
    expect(timeAgo(iso)).toMatch(/5min ago/);
  });

  test("hours ago", () => {
    const iso = new Date(Date.now() - 2 * 3600000).toISOString();
    expect(timeAgo(iso)).toMatch(/2h ago/);
  });

  test("days ago", () => {
    const iso = new Date(Date.now() - 3 * 86400000).toISOString();
    expect(timeAgo(iso)).toBe("3d ago");
  });

  test("weeks ago", () => {
    const iso = new Date(Date.now() - 14 * 86400000).toISOString();
    expect(timeAgo(iso)).toBe("2w ago");
  });

  test("months ago", () => {
    const iso = new Date(Date.now() - 60 * 86400000).toISOString();
    expect(timeAgo(iso)).toBe("2mo ago");
  });

  test("years ago", () => {
    const iso = new Date(Date.now() - 400 * 86400000).toISOString();
    expect(timeAgo(iso)).toBe("1y ago");
  });

  test("future timestamps", () => {
    const iso = new Date(Date.now() + 120000).toISOString();
    expect(timeAgo(iso)).toMatch(/in 2min/);
  });

  test("future days", () => {
    const iso = new Date(Date.now() + 3 * 86400000).toISOString();
    expect(timeAgo(iso)).toBe("in 3d");
  });
});

// ---------------------------------------------------------------------------
// shortType
// ---------------------------------------------------------------------------

describe("shortType", () => {
  test("maps Vec<u8> to Pipeline", () => {
    expect(shortType("alloc::vec::Vec<u8>")).toBe("Pipeline");
  });

  test("extracts last segment from qualified name", () => {
    expect(shortType("budget_jobs::SyncJob")).toBe("SyncJob");
  });

  test("returns simple names as-is", () => {
    expect(shortType("SyncJob")).toBe("SyncJob");
  });
});

// ---------------------------------------------------------------------------
// syncUrlFor
// ---------------------------------------------------------------------------

describe("syncUrlFor", () => {
  test("amazon account", () => {
    expect(syncUrlFor({ account_type: "amazon", account_id: "abc" })).toBe(
      "/amazon/accounts/abc/sync",
    );
  });

  test("bank account", () => {
    expect(syncUrlFor({ account_type: "bank", account_id: "xyz" })).toBe(
      "/jobs/pipeline/xyz",
    );
  });
});

// ---------------------------------------------------------------------------
// QUEUE_CARDS
// ---------------------------------------------------------------------------

describe("QUEUE_CARDS", () => {
  test("has expected card keys", () => {
    const keys = QUEUE_CARDS.map((c) => c.key);
    expect(keys).toContain("sync");
    expect(keys).toContain("categorize");
    expect(keys).toContain("correlate");
    expect(keys).toContain("amazon");
  });
});

// ---------------------------------------------------------------------------
// cardCounts
// ---------------------------------------------------------------------------

describe("cardCounts", () => {
  const syncCard = QUEUE_CARDS.find((c) => c.key === "sync");

  test("returns zeros for null counts", () => {
    const result = cardCounts(syncCard, null);
    expect(result).toEqual({ active: 0, waiting: 0, completed: 0, failed: 0 });
  });

  test("aggregates matching job types", () => {
    const counts = [
      {
        job_type: "budget_jobs::SyncJob",
        active: 1,
        waiting: 2,
        completed: 3,
        failed: 0,
      },
      {
        job_type: "budget_jobs::Pipeline",
        active: 0,
        waiting: 1,
        completed: 5,
        failed: 1,
      },
    ];
    const result = cardCounts(syncCard, counts);
    expect(result.active).toBe(1);
    expect(result.waiting).toBe(3);
    expect(result.completed).toBe(8);
    expect(result.failed).toBe(1);
  });

  test("ignores non-matching job types", () => {
    const counts = [
      {
        job_type: "budget_jobs::CategorizeJob",
        active: 5,
        waiting: 0,
        completed: 0,
        failed: 0,
      },
    ];
    const result = cardCounts(syncCard, counts);
    expect(result.active).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// formatDateShort / formatDateRange
// ---------------------------------------------------------------------------

describe("formatDateShort", () => {
  test("returns dash for null", () => {
    expect(formatDateShort(null)).toBe("\u2014");
  });

  test("formats a date", () => {
    const result = formatDateShort("2026-01-15");
    expect(result).toContain("Jan");
    expect(result).toContain("15");
  });
});

describe("formatDateRange", () => {
  test("with end date", () => {
    const result = formatDateRange("2026-01-01", "2026-01-31");
    expect(result).toContain("Jan");
    expect(result).toContain("\u2013");
  });

  test("without end date", () => {
    const result = formatDateRange("2026-01-01", null);
    expect(result).toContain("ongoing");
  });
});

// ---------------------------------------------------------------------------
// formatOrdinal
// ---------------------------------------------------------------------------
describe("formatOrdinal", () => {
  test("1st, 2nd, 3rd, 4th", () => {
    expect(formatOrdinal(1)).toBe("1st");
    expect(formatOrdinal(2)).toBe("2nd");
    expect(formatOrdinal(3)).toBe("3rd");
    expect(formatOrdinal(4)).toBe("4th");
  });

  test("11th, 12th, 13th (teens)", () => {
    expect(formatOrdinal(11)).toBe("11th");
    expect(formatOrdinal(12)).toBe("12th");
    expect(formatOrdinal(13)).toBe("13th");
  });

  test("21st, 22nd, 23rd", () => {
    expect(formatOrdinal(21)).toBe("21st");
    expect(formatOrdinal(22)).toBe("22nd");
    expect(formatOrdinal(23)).toBe("23rd");
  });
});
