// Pure utility functions extracted for testability.
// Imported by app.js and frontend tests.

// ---------------------------------------------------------------------------
// Account display
// ---------------------------------------------------------------------------

export function accountDisplayName(account) {
  return account?.nickname || account?.name || "";
}

// ---------------------------------------------------------------------------
// Date formatting (Intl APIs — zero dependencies)
// ---------------------------------------------------------------------------

const relFmt = new Intl.RelativeTimeFormat("en", { numeric: "auto" });
const shortDateFmt = new Intl.DateTimeFormat("en", {
  month: "short",
  day: "numeric",
});
const fullDateFmt = new Intl.DateTimeFormat("en", {
  month: "short",
  day: "numeric",
  year: "numeric",
});

export function formatDate(iso) {
  if (!iso) return "\u2014";
  const date = new Date(iso + (iso.includes("T") ? "" : "T00:00:00"));
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const target = new Date(date.getFullYear(), date.getMonth(), date.getDate());
  const days = Math.round((target - today) / 86400000);
  if (Math.abs(days) <= 6) return relFmt.format(days, "day");
  if (target.getFullYear() === today.getFullYear())
    return shortDateFmt.format(date);
  return fullDateFmt.format(date);
}

export function formatDateFull(iso) {
  if (!iso) return "\u2014";
  const date = new Date(iso + (iso.includes("T") ? "" : "T00:00:00"));
  const relative = formatDate(iso);
  return `${fullDateFmt.format(date)} (${relative})`;
}

export function formatDateShort(iso) {
  if (!iso) return "\u2014";
  const date = new Date(iso + (iso.includes("T") ? "" : "T00:00:00"));
  return shortDateFmt.format(date);
}

export function formatDateRange(start, end) {
  const s = formatDateShort(start);
  return end ? `${s} \u2013 ${formatDateShort(end)}` : `${s} \u2013 ongoing`;
}

// ---------------------------------------------------------------------------
// Amount formatting
// ---------------------------------------------------------------------------

export function formatAmount(amount, { decimals = 2, sign = false } = {}) {
  const n = Number(amount);
  const abs = Math.abs(n).toLocaleString(undefined, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
  if (sign && n > 0) return `+\u202F${abs}\u202F\u20AC`;
  if (sign && n < 0) return `\u2212\u202F${abs}\u202F\u20AC`;
  return `${abs}\u202F\u20AC`;
}

export function amountClass(amount) {
  const n = Number(amount);
  if (n > 0) return "amount-positive";
  return "";
}

// ---------------------------------------------------------------------------
// Category helpers
// ---------------------------------------------------------------------------

export function categoryLabel(catMap, id) {
  if (!id) return null;
  const cat = catMap[id];
  if (!cat) return null;
  if (cat.parent_id && catMap[cat.parent_id]) {
    const parent = catMap[cat.parent_id].name;
    const short = cat.name.startsWith(`${parent}:`)
      ? cat.name.slice(parent.length + 1)
      : cat.name;
    return { parent, short };
  }
  if (cat.name.includes(":")) {
    const idx = cat.name.indexOf(":");
    return { parent: cat.name.slice(0, idx), short: cat.name.slice(idx + 1) };
  }
  return { parent: null, short: cat.name };
}

export function categoryName(catMap, id) {
  const label = categoryLabel(catMap, id);
  if (!label) return null;
  return label.parent ? `${label.parent} > ${label.short}` : label.short;
}

export function categoryQualifiedName(catMap, id) {
  const label = categoryLabel(catMap, id);
  if (!label) return null;
  return label.parent ? `${label.parent}:${label.short}` : label.short;
}

export function categoryBudgetMode(catMap, id) {
  if (!id) return null;
  const cat = catMap[id];
  if (!cat) return null;
  if (cat.budget_mode) return cat.budget_mode;
  if (cat.parent_id) {
    const parent = catMap[cat.parent_id];
    return parent?.budget_mode ?? null;
  }
  return null;
}

export function budgetModeColor(mode) {
  if (mode === "monthly") return "cat-monthly";
  if (mode === "annual") return "cat-annual";
  if (mode === "project") return "cat-project";
  if (mode === "salary") return "cat-salary";
  if (mode === "transfer") return "cat-transfer";
  return "";
}

export function buildCategoryTree(categories) {
  const roots = [];
  const childrenOf = {};
  for (const c of categories) {
    if (c.parent_id) {
      if (!childrenOf[c.parent_id]) childrenOf[c.parent_id] = [];
      childrenOf[c.parent_id].push(c);
    } else {
      roots.push(c);
    }
  }
  const sorted = (arr) =>
    arr.slice().sort((a, b) => a.name.localeCompare(b.name));
  const result = [];
  for (const root of sorted(roots)) {
    const children = sorted(childrenOf[root.id] ?? []);
    if (children.length === 0) {
      result.push({ ...root, depth: 0 });
    } else {
      for (const child of children) {
        result.push({ ...child, depth: 1 });
      }
    }
  }
  return result;
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

export function titleCase(s) {
  return s.toLowerCase().replace(/(?:^|\s|[-./])\S/g, (ch) => ch.toUpperCase());
}

export function cleanMerchant(name) {
  if (!name) return name;
  let s = name;
  s = s.replace(/^VISA\s+/, "");
  s = s.replace(/^SUMUP\s+\*\s*/, "");
  if (s === s.toUpperCase() && s.length > 2) s = titleCase(s);
  return s;
}

export function transactionTitle(t) {
  return (
    t.llm_title ||
    t.counterparty_name ||
    cleanMerchant(t.merchant_name || t.remittance_information?.[0] || "")
  );
}

export function formatRemittanceInfo(segments) {
  if (!segments || !segments.length) return null;
  return segments.filter((s) => s?.trim());
}

// ---------------------------------------------------------------------------
// Budget pace helpers
// ---------------------------------------------------------------------------

export function paceBadge(pace) {
  if (pace === "pending") return "secondary";
  if (pace === "under_budget") return "success";
  if (pace === "on_track") return "primary";
  if (pace === "above_pace") return "warning";
  return "danger";
}

export function paceLabel(pace, delta) {
  const base =
    pace === "pending"
      ? "Pending"
      : pace === "under_budget"
        ? "Under pace"
        : pace === "on_track"
          ? "On track"
          : pace === "above_pace"
            ? "Above pace"
            : "Over budget";
  if (
    delta != null &&
    (pace === "under_budget" || pace === "over_budget" || pace === "above_pace")
  )
    return `${base} (${formatAmount(delta, { decimals: 0, sign: true })})`;
  return base;
}

export function paceColor(pace) {
  if (pace === "over_budget") return "var(--danger)";
  if (pace === "above_pace") return "var(--warning)";
  if (pace === "on_track") return "var(--primary)";
  if (pace === "under_budget") return "var(--success)";
  return "var(--text-light)";
}

// ---------------------------------------------------------------------------
// Time ago (relative timestamps)
// ---------------------------------------------------------------------------

export function timeAgo(iso) {
  if (!iso) return "\u2014";
  const diff = (Date.now() - new Date(iso).getTime()) / 1000;
  if (diff < 0) {
    const abs = -diff;
    if (abs < 60) return `in ${Math.round(abs)}s`;
    if (abs < 3600) return `in ${Math.round(abs / 60)}min`;
    return `in ${Math.round(abs / 3600)}h`;
  }
  if (diff < 60) return `${Math.round(diff)}s ago`;
  if (diff < 3600) return `${Math.round(diff / 60)}min ago`;
  return `${Math.round(diff / 3600)}h ago`;
}

// ---------------------------------------------------------------------------
// Job queue helpers
// ---------------------------------------------------------------------------

export function shortType(t) {
  if (t === "alloc::vec::Vec<u8>") return "Pipeline";
  return t.includes("::") ? t.split("::").pop() : t;
}

export function syncUrlFor(s) {
  return s.account_type === "amazon"
    ? `/amazon/accounts/${s.account_id}/sync`
    : `/jobs/pipeline/${s.account_id}`;
}

export const QUEUE_CARDS = [
  {
    key: "sync",
    title: "Sync",
    desc: "Fetch new transactions from connected bank and Amazon accounts",
    types: ["SyncJob", "Pipeline"],
  },
  {
    key: "categorize",
    title: "Categorize",
    desc: "Apply rules and LLM to assign categories",
    types: ["CategorizeJob", "CategorizeTransactionJob"],
  },
  {
    key: "correlate",
    title: "Correlate",
    desc: "Match transfers and reimbursements between accounts",
    types: ["CorrelateJob", "CorrelateTransactionJob"],
  },
  {
    key: "amazon",
    title: "Amazon",
    desc: "Fetch Amazon order details and match to bank transactions",
    types: [
      "AmazonSyncJob",
      "AmazonPageJob",
      "AmazonFetchOrderJob",
      "AmazonMatchJob",
    ],
  },
];

export function cardCounts(card, counts) {
  if (!counts) return { active: 0, waiting: 0, completed: 0, failed: 0 };
  const agg = { active: 0, waiting: 0, completed: 0, failed: 0 };
  for (const c of counts) {
    const name = c.job_type.includes("::")
      ? c.job_type.split("::").pop()
      : c.job_type;
    if (card.types.includes(name)) {
      agg.active += c.active;
      agg.waiting += c.waiting;
      agg.completed += c.completed;
      agg.failed += c.failed;
    }
  }
  return agg;
}
