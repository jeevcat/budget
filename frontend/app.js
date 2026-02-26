import htm from "htm";
import { h, render } from "preact";
import { useEffect, useState } from "preact/hooks";

const html = htm.bind(h);

// ---------------------------------------------------------------------------
// API helper
// ---------------------------------------------------------------------------

const api = {
  token: localStorage.getItem("budget_token") ?? "",

  async fetch(path, opts = {}) {
    const res = await fetch(`/api${path}`, {
      ...opts,
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${api.token}`,
        ...opts.headers,
      },
      body: opts.body ? JSON.stringify(opts.body) : undefined,
    });
    if (!res.ok) {
      const text = await res.text();
      throw new Error(`${res.status}: ${text}`);
    }
    if (res.status === 204 || res.status === 202) return null;
    return res.json();
  },

  get: (path) => api.fetch(path),
  post: (path, body) => api.fetch(path, { method: "POST", body }),
  put: (path, body) => api.fetch(path, { method: "PUT", body }),
  del: (path) => api.fetch(path, { method: "DELETE" }),
};

// ---------------------------------------------------------------------------
// Simple hash router
// ---------------------------------------------------------------------------

function useRoute() {
  const [route, setRoute] = useState(location.hash.slice(1) || "/");
  useEffect(() => {
    const onHash = () => setRoute(location.hash.slice(1) || "/");
    addEventListener("hashchange", onHash);
    return () => removeEventListener("hashchange", onHash);
  }, []);
  return route;
}

function NavLink({ href, children }) {
  const route = location.hash.slice(1) || "/";
  const current = route === href || route.startsWith(`${href}/`);
  return html`<a href="#${href}" aria-current=${current ? "page" : undefined}
    >${children}</a
  >`;
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

function formatAmount(amount) {
  const n = Number(amount);
  const abs = Math.abs(n).toFixed(2);
  if (n > 0) return `+\u202F${abs}\u202F\u20AC`;
  if (n < 0) return `\u2212\u202F${abs}\u202F\u20AC`;
  return `${abs}\u202F\u20AC`;
}

function amountClass(amount) {
  const n = Number(amount);
  if (n > 0) return "amount-positive";
  if (n < 0) return "";
  return "";
}

function categoryLabel(catMap, id) {
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
  return { parent: null, short: cat.name };
}

function categoryName(catMap, id) {
  const label = categoryLabel(catMap, id);
  if (!label) return null;
  return label.parent ? `${label.parent} > ${label.short}` : label.short;
}

function titleCase(s) {
  return s.toLowerCase().replace(/(?:^|\s|[-./])\S/g, (ch) => ch.toUpperCase());
}

function cleanMerchant(name) {
  if (!name) return name;
  let s = name;
  s = s.replace(/^VISA\s+/, "");
  s = s.replace(/^SUMUP\s+\*\s*/, "");
  if (s === s.toUpperCase() && s.length > 2) s = titleCase(s);
  return s;
}

function cleanDescription(desc) {
  if (!desc) return null;
  const m = desc.match(/remittanceinformation:(.*)/);
  if (!m) return desc;
  const info = m[1].trim();
  if (!info) return null;
  if (/^NR XXXX \d{4}\s/.test(info)) return null;
  return info;
}

function paceBadge(pace) {
  if (pace === "under_budget") return "success";
  if (pace === "on_track") return "warning";
  return "danger";
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

function currencyFmt(amount) {
  const n = Number(amount);
  return `${n.toLocaleString(undefined, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  })}\u202F\u20AC`;
}

function paceLabel(pace) {
  if (pace === "under_budget") return "Under";
  if (pace === "on_track") return "On track";
  return "Over";
}

function ProgressRing({ spent, budget, pace, size = 48 }) {
  const r = (size - 6) / 2;
  const circ = 2 * Math.PI * r;
  const pct = budget > 0 ? Math.min(Number(spent) / Number(budget), 1) : 0;
  const offset = circ * (1 - pct);
  const color =
    pace === "over_budget"
      ? "var(--danger)"
      : pace === "on_track"
        ? "var(--warning)"
        : "var(--success)";

  return html`
    <svg
      width=${size}
      height=${size}
      viewBox="0 0 ${size} ${size}"
      class="progress-ring"
    >
      <circle
        cx=${size / 2}
        cy=${size / 2}
        r=${r}
        fill="none"
        stroke="var(--border)"
        stroke-width="5"
      />
      <circle
        cx=${size / 2}
        cy=${size / 2}
        r=${r}
        fill="none"
        stroke=${color}
        stroke-width="5"
        stroke-dasharray=${circ}
        stroke-dashoffset=${offset}
        stroke-linecap="round"
        transform="rotate(-90 ${size / 2} ${size / 2})"
        style="transition: stroke-dashoffset 0.6s ease"
      />
    </svg>
  `;
}

function SpendBar({ items, maxVal }) {
  return html`
    <div class="vstack gap-2">
      ${items.map(
        (item) => html`
          <div class="spend-bar-row" key=${item.id}>
            <span class="spend-bar-label">${item.name}</span>
            <div class="spend-bar-track">
              <div
                class="spend-bar-fill spend-bar-${item.pace}"
                style="width:${maxVal > 0 ? (Math.abs(Number(item.spent)) / maxVal) * 100 : 0}%"
              ></div>
              <div
                class="spend-bar-budget-mark"
                style="left:${maxVal > 0 ? (Number(item.budget) / maxVal) * 100 : 0}%"
                title="Budget: ${currencyFmt(item.budget)}"
              ></div>
            </div>
            <span class="spend-bar-amount">${currencyFmt(item.spent)}</span>
          </div>
        `,
      )}
    </div>
  `;
}

function Dashboard() {
  const [status, setStatus] = useState(null);
  const [categories, setCategories] = useState(null);
  const [months, setMonths] = useState(null);
  const [transactions, setTransactions] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    Promise.all([
      api.get("/budgets/status"),
      api.get("/categories"),
      api.get("/budgets/months"),
      api.get("/transactions"),
    ])
      .then(([s, c, m, t]) => {
        setStatus(s);
        setCategories(c);
        setMonths(m);
        setTransactions(t);
      })
      .catch(setError);
  }, []);

  if (error) return html`<p class="text-light">${error.message}</p>`;
  if (!status || !categories || !months || !transactions)
    return html`<p class="text-light">Loading...</p>`;

  const catMap = Object.fromEntries(categories.map((c) => [c.id, c]));

  // Current budget month (open-ended)
  const currentMonth = months.find((m) => !m.end_date);
  const monthStart = currentMonth?.start_date;

  // Enrich status with display names and hierarchy info
  const enriched = status
    .map((s) => {
      const cat = catMap[s.category_id];
      const label = categoryLabel(catMap, s.category_id);
      return {
        ...s,
        name: label
          ? label.parent
            ? `${label.parent} > ${label.short}`
            : label.short
          : s.category_name,
        shortName: label?.short ?? s.category_name,
        parentName: label?.parent ?? null,
        budgetMode: cat?.budget_mode ?? null,
      };
    })
    .sort((a, b) => Number(b.spent) - Number(a.spent));

  // Totals
  const totalBudget = enriched.reduce(
    (sum, s) => sum + Number(s.budget_amount),
    0,
  );
  const totalSpent = enriched.reduce((sum, s) => sum + Number(s.spent), 0);
  const totalRemaining = totalBudget - totalSpent;

  // Uncategorized count
  const uncategorizedCount = transactions.filter(
    (t) => !t.category_id && !t.correlation_type,
  ).length;

  // Recent transactions (last 8)
  const recentTxns = [...transactions]
    .sort((a, b) => b.posted_date.localeCompare(a.posted_date))
    .slice(0, 8);

  // Max value for the bar chart (to scale bars)
  const barMax = Math.max(
    ...enriched.map((s) =>
      Math.max(Math.abs(Number(s.spent)), Number(s.budget_amount)),
    ),
    1,
  );

  // Days into the month / days left
  const daysLeft = enriched.length > 0 ? enriched[0].days_left : 0;

  // Over-budget categories
  const overBudget = enriched.filter((s) => s.pace === "over_budget");

  return html`
    ${
      currentMonth &&
      html`
        <div class="hstack" style="margin-bottom:1.25rem">
          <div>
            <h2 style="margin:0">Budget Month</h2>
            <span class="text-light">
              Started ${monthStart}${" "}
              <span class="mono">${daysLeft}d left</span>
            </span>
          </div>
          ${
            uncategorizedCount > 0 &&
            html`
              <a
                href="#/transactions"
                class="badge warning"
                style="margin-left:auto;text-decoration:none"
              >
                ${uncategorizedCount} uncategorized
              </a>
            `
          }
        </div>
      `
    }

    <div class="dash-totals">
      <article class="card dash-stat-card">
        <span class="dash-stat-label">Total Budget</span>
        <span class="dash-stat-value">${currencyFmt(totalBudget)}</span>
      </article>
      <article class="card dash-stat-card">
        <span class="dash-stat-label">Spent</span>
        <span class="dash-stat-value">${currencyFmt(totalSpent)}</span>
      </article>
      <article class="card dash-stat-card">
        <span class="dash-stat-label">Remaining</span>
        <span
          class="dash-stat-value ${totalRemaining < 0 ? "dash-negative" : ""}"
        >
          ${currencyFmt(totalRemaining)}
        </span>
      </article>
      <article class="card dash-stat-card">
        <span class="dash-stat-label">Categories</span>
        <span class="dash-stat-value">
          ${
            overBudget.length > 0
              ? html`<span class="badge danger">${overBudget.length}</span>
                  over`
              : html`All on track`
          }
        </span>
      </article>
    </div>

    <div class="dash-grid">
      <article class="card" style="padding:var(--space-4)">
        <h3 style="margin:0 0 0.75rem">Spending vs Budget</h3>
        <${SpendBar}
          items=${enriched.map((s) => ({
            id: s.category_id,
            name: s.shortName,
            spent: s.spent,
            budget: s.budget_amount,
            pace: s.pace,
          }))}
          maxVal=${barMax}
        />
      </article>

      <article class="card" style="padding:var(--space-4)">
        <h3 style="margin:0 0 0.75rem">Category Breakdown</h3>
        <div class="vstack" style="gap:0">
          ${enriched.map(
            (s) => html`
              <div class="dash-cat-row" key=${s.category_id}>
                <${ProgressRing}
                  spent=${s.spent}
                  budget=${s.budget_amount}
                  pace=${s.pace}
                />
                <div class="dash-cat-info">
                  <div class="dash-cat-name">
                    ${
                      s.parentName &&
                      html`<span class="cat-parent">${s.parentName}</span>`
                    }${s.shortName}
                  </div>
                  <div class="dash-cat-sub">
                    <span>${currencyFmt(s.spent)}</span>
                    <span class="text-light">
                      ${" "}/ ${currencyFmt(s.budget_amount)}</span
                    >
                  </div>
                </div>
                <div class="dash-cat-end">
                  <span class="badge small ${paceBadge(s.pace)}"
                    >${paceLabel(s.pace)}</span
                  >
                  <span
                    class="dash-cat-remaining ${Number(s.remaining) < 0 ? "dash-negative" : ""}"
                  >
                    ${formatAmount(s.remaining)}
                  </span>
                </div>
              </div>
            `,
          )}
        </div>
      </article>
    </div>

    <article class="card" style="padding:var(--space-4);margin-top:1rem">
      <div
        class="hstack"
        style="align-items:baseline;margin-bottom:0.75rem"
      >
        <h3 style="margin:0">Recent Transactions</h3>
        <a
          href="#/transactions"
          class="text-light"
          style="margin-left:auto;font-size:0.85rem"
          >View all</a
        >
      </div>
      <div class="table">
        <table class="dash-txn-table">
          <tbody>
            ${recentTxns.map(
              (t) => html`
                <tr class=${t.correlation_type ? "row-correlated" : ""}>
                  <td class="mono text-light" style="width:7rem">
                    ${t.posted_date}
                  </td>
                  <td style="font-weight:500">
                    ${cleanMerchant(t.merchant_name || t.description)}
                  </td>
                  <td
                    class="${amountClass(t.amount)}"
                    style="text-align:right"
                  >
                    ${formatAmount(t.amount)}
                  </td>
                  <td>
                    <${CategoryBadge}
                      catMap=${catMap}
                      id=${t.category_id}
                      suggested=${t.suggested_category}
                    />
                  </td>
                </tr>
              `,
            )}
          </tbody>
        </table>
      </div>
    </article>
  `;
}

// ---------------------------------------------------------------------------
// Transactions
// ---------------------------------------------------------------------------

function CategoryBadge({ catMap, id, suggested }) {
  const label = categoryLabel(catMap, id);
  if (label) {
    if (label.parent) {
      return html`<span title="${label.parent} > ${label.short}">
        <span class="cat-parent">${label.parent}</span>${label.short}
      </span>`;
    }
    return html`<span>${label.short}</span>`;
  }
  if (suggested) {
    return html`<span class="llm-suggestion" title="LLM suggestion: ${suggested}"><span class="llm-suggestion-icon">✦</span> ${suggested}</span>`;
  }
  return html`<span class="chip outline warning">uncategorized</span>`;
}

function MethodDot({ method }) {
  if (!method) return null;
  const labels = { manual: "Manual", rule: "Rule", llm: "LLM" };
  return html`<span class="method-dot method-${method}" title="Categorized by ${labels[method] ?? method}"></span>`;
}

function TxnDetail({
  txn,
  catMap,
  categories,
  acctMap,
  onCategorize,
  onClose,
  onRuleCreated,
}) {
  const [saving, setSaving] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [ruleProposals, setRuleProposals] = useState(null);
  const [selectedProposal, setSelectedProposal] = useState(null);
  const [editPattern, setEditPattern] = useState("");
  const [creatingRule, setCreatingRule] = useState(false);
  if (!txn) return null;

  // Pre-select LLM suggestion when no manual category is set
  const suggestedCategoryId =
    !txn.category_id && txn.suggested_category
      ? ((categories ?? []).find((c) => c.name === txn.suggested_category)
          ?.id ?? null)
      : null;

  const ref = (el) => {
    if (el && !el.open) {
      el.addEventListener(
        "close",
        () => {
          setRuleProposals(null);
          setSelectedProposal(null);
          onClose();
        },
        { once: true },
      );
      el.showModal();
    }
  };
  const desc = cleanDescription(txn.description);

  const canGenerateRule = txn.category_id && txn.category_method !== "rule";

  async function handleCategorize(categoryId) {
    if (!categoryId || categoryId === txn.category_id) return;
    setSaving(true);
    try {
      await api.post(`/transactions/${txn.id}/categorize`, {
        category_id: categoryId,
      });
      onCategorize(txn.id, categoryId);
    } finally {
      setSaving(false);
    }
  }

  async function handleGenerateRule() {
    setGenerating(true);
    setRuleProposals(null);
    setSelectedProposal(null);
    try {
      const result = await api.post(`/transactions/${txn.id}/generate-rule`);
      setRuleProposals(result);
    } finally {
      setGenerating(false);
    }
  }

  function handleSelectProposal(idx) {
    if (selectedProposal === idx) {
      setSelectedProposal(null);
    } else {
      setSelectedProposal(idx);
      setEditPattern(ruleProposals.proposals[idx].match_pattern);
    }
  }

  async function handleAcceptRule() {
    if (selectedProposal == null || !ruleProposals) return;
    const proposal = ruleProposals.proposals[selectedProposal];
    setCreatingRule(true);
    try {
      await api.post("/rules", {
        rule_type: "categorization",
        match_field: proposal.match_field,
        match_pattern: editPattern,
        target_category_id: ruleProposals.target_category_id,
        target_correlation_type: null,
        priority: 0,
      });
      setRuleProposals(null);
      setSelectedProposal(null);
      if (onRuleCreated) onRuleCreated();
    } finally {
      setCreatingRule(false);
    }
  }

  return html`
    <dialog ref=${ref} closedby="any">
      <form method="dialog">
        <header>
          <h3>${cleanMerchant(txn.merchant_name || txn.description)}</h3>
        </header>
        <div>
          <dl class="txn-dl">
            <dt>Date</dt><dd>${txn.posted_date}</dd>
            <dt>Amount</dt><dd class="${amountClass(txn.amount)}">${formatAmount(txn.amount)}</dd>
            ${
              txn.original_amount
                ? html`
              <dt>Original</dt><dd>${txn.original_amount} ${txn.original_currency}</dd>
            `
                : null
            }
            <dt>Category</dt>
            <dd>
              <select
                value=${txn.category_id ?? suggestedCategoryId ?? ""}
                disabled=${saving}
                onChange=${(e) => handleCategorize(e.target.value)}
              >
                <option value="">uncategorized</option>
                ${(categories ?? []).map(
                  (c) =>
                    html`<option value=${c.id}>${categoryName(catMap, c.id)}</option>`,
                )}
              </select>
              ${
                txn.category_id && txn.category_method
                  ? html`<span style="margin-left:0.5rem"><${MethodDot} method=${txn.category_method} /></span>`
                  : null
              }
              ${
                !txn.category_id && txn.suggested_category
                  ? html`<span class="llm-suggestion" style="margin-left:0.5rem" title="LLM suggestion"><span class="llm-suggestion-icon">✦</span> ${txn.suggested_category}</span>`
                  : null
              }
            </dd>
            <dt>Account</dt><dd>${acctMap[txn.account_id]?.name ?? txn.account_id}</dd>
            ${
              txn.correlation_type
                ? html`
              <dt>Correlation</dt><dd><span class="chip outline">${txn.correlation_type}</span></dd>
            `
                : null
            }
            ${
              txn.merchant_name
                ? html`
              <dt>Raw merchant</dt><dd><code>${txn.merchant_name}</code></dd>
            `
                : null
            }
            ${
              desc
                ? html`
              <dt>Note</dt><dd>${desc}</dd>
            `
                : null
            }
          </dl>

          ${
            ruleProposals &&
            html`
              <div style="margin-top:1rem">
                <h4 style="margin:0 0 0.5rem">Rule Proposals</h4>
                <p class="text-light" style="margin:0 0 0.5rem">
                  Category: <strong>${ruleProposals.category_name}</strong>
                </p>
                ${ruleProposals.proposals.map(
                  (p, idx) => html`
                    <div
                      key=${idx}
                      style="border:1px solid var(--border);border-radius:4px;padding:0.75rem;margin-bottom:0.5rem;cursor:pointer;${selectedProposal === idx ? "background:var(--bg-light)" : ""}"
                      onClick=${() => handleSelectProposal(idx)}
                    >
                      <div class="hstack" style="gap:0.5rem">
                        <code style="font-size:0.85rem">${p.match_pattern}</code>
                      </div>
                      <p class="text-light" style="margin:0.25rem 0 0;font-size:0.85rem">${p.explanation}</p>
                      ${
                        selectedProposal === idx &&
                        html`
                          <div style="margin-top:0.5rem" onClick=${(e) => e.stopPropagation()}>
                            <input
                              type="text"
                              value=${editPattern}
                              onInput=${(e) => setEditPattern(e.target.value)}
                              style="width:100%;margin-bottom:0.5rem;font-family:monospace"
                            />
                            <button
                              type="button"
                              data-variant="primary"
                              class="small"
                              onClick=${handleAcceptRule}
                              disabled=${creatingRule}
                            >
                              ${creatingRule ? "Creating..." : "Create Rule"}
                            </button>
                          </div>
                        `
                      }
                    </div>
                  `,
                )}
                ${
                  ruleProposals.proposals.length === 0 &&
                  html`<p class="text-light">No valid patterns could be generated.</p>`
                }
              </div>
            `
          }
        </div>
        <footer>
          ${
            canGenerateRule &&
            html`
              <button
                type="button"
                onClick=${handleGenerateRule}
                disabled=${generating}
              >
                ${generating ? "Generating..." : "Generate Rule"}
              </button>
            `
          }
          <button value="close">Close</button>
        </footer>
      </form>
    </dialog>
  `;
}

function Transactions() {
  const [txns, setTxns] = useState(null);
  const [categories, setCategories] = useState(null);
  const [accounts, setAccounts] = useState(null);
  const [error, setError] = useState(null);
  const [search, setSearch] = useState("");
  const [filterCat, setFilterCat] = useState("");
  const [filterAcct, setFilterAcct] = useState("");
  const [filterMethod, setFilterMethod] = useState("");
  const [selected, setSelected] = useState(null);
  const [sortCol, setSortCol] = useState("date");
  const [sortAsc, setSortAsc] = useState(false);

  useEffect(() => {
    Promise.all([
      api.get("/transactions"),
      api.get("/categories"),
      api.get("/accounts"),
    ])
      .then(([t, c, a]) => {
        setTxns(t);
        setCategories(c);
        setAccounts(a);
      })
      .catch(setError);
  }, []);

  if (error) return html`<p class="text-light">${error.message}</p>`;
  if (!txns) return html`<p class="text-light">Loading...</p>`;

  const catMap = Object.fromEntries((categories ?? []).map((c) => [c.id, c]));
  const acctMap = Object.fromEntries((accounts ?? []).map((a) => [a.id, a]));

  if (txns.length === 0)
    return html`
      <h2>Transactions</h2>
      <p class="text-light">
        No transactions yet. Connect an account and run a sync job to pull in
        data.
      </p>
    `;

  const q = search.toLowerCase();
  const filtered = txns.filter((t) => {
    if (
      q &&
      !(t.merchant_name || "").toLowerCase().includes(q) &&
      !(t.description || "").toLowerCase().includes(q)
    )
      return false;
    if (filterCat === "__none" && t.category_id) return false;
    if (filterCat && filterCat !== "__none" && t.category_id !== filterCat)
      return false;
    if (filterAcct && t.account_id !== filterAcct) return false;
    if (filterMethod === "__none" && t.category_method) return false;
    if (
      filterMethod &&
      filterMethod !== "__none" &&
      t.category_method !== filterMethod
    )
      return false;
    return true;
  });

  const sorted = [...filtered].sort((a, b) => {
    let cmp = 0;
    switch (sortCol) {
      case "date":
        cmp = a.posted_date.localeCompare(b.posted_date);
        break;
      case "merchant":
        cmp = (a.merchant_name || "").localeCompare(b.merchant_name || "");
        break;
      case "amount":
        cmp = Number(a.amount) - Number(b.amount);
        break;
      case "category":
        cmp = (categoryName(catMap, a.category_id) || "\uffff").localeCompare(
          categoryName(catMap, b.category_id) || "\uffff",
        );
        break;
      case "account":
        cmp = (acctMap[a.account_id]?.name || "").localeCompare(
          acctMap[b.account_id]?.name || "",
        );
        break;
    }
    return sortAsc ? cmp : -cmp;
  });

  function toggleSort(col) {
    if (sortCol === col) {
      setSortAsc((prev) => !prev);
    } else {
      setSortCol(col);
      setSortAsc(col === "merchant" || col === "category" || col === "account");
    }
  }

  function SortTh({ col, children }) {
    const active = sortCol === col;
    const arrow = active ? (sortAsc ? "\u25B2" : "\u25BC") : "\u25BC";
    return html`<th
      class="sortable ${active ? "sort-active" : ""}"
      onClick=${() => toggleSort(col)}
    >
      ${children}<span class="sort-arrow">${arrow}</span>
    </th>`;
  }

  const usedCatIds = [
    ...new Set(txns.map((t) => t.category_id).filter(Boolean)),
  ];
  const usedAcctIds = [...new Set(txns.map((t) => t.account_id))];
  const hasActiveFilter = filterCat || filterAcct || filterMethod;

  return html`
    <div class="hstack" style="align-items:baseline;margin-bottom:0.75rem">
      <h2 style="margin:0">Transactions</h2>
      <span class="text-lighter small" style="margin-left:0.75rem">
        ${
          filtered.length === txns.length
            ? `${txns.length}`
            : `${filtered.length} / ${txns.length}`
        }
      </span>
    </div>

    <div class="hstack txn-filters" style="margin-bottom:0.75rem">
      <input
        type="search"
        placeholder="Search merchants..."
        class="txn-search"
        value=${search}
        onInput=${(e) => setSearch(e.target.value)}
      />
      <select value=${filterCat} onChange=${(e) => setFilterCat(e.target.value)}>
        <option value="">All categories</option>
        <option value="__none">Uncategorized</option>
        ${usedCatIds.map(
          (id) =>
            html`<option value=${id}>${categoryName(catMap, id)}</option>`,
        )}
      </select>
      <select value=${filterAcct} onChange=${(e) => setFilterAcct(e.target.value)}>
        <option value="">All accounts</option>
        ${usedAcctIds.map(
          (id) => html`<option value=${id}>${acctMap[id]?.name ?? id}</option>`,
        )}
      </select>
      <select value=${filterMethod} onChange=${(e) => setFilterMethod(e.target.value)}>
        <option value="">All methods</option>
        <option value="manual">Manual</option>
        <option value="rule">Rule</option>
        <option value="llm">LLM</option>
        <option value="__none">Uncategorized</option>
      </select>
    </div>

    ${
      hasActiveFilter &&
      html`
      <div class="hstack gap-2" style="margin-bottom:0.75rem">
        ${
          filterCat &&
          html`
          <button class="chip" onClick=${() => setFilterCat("")}>
            <span>${filterCat === "__none" ? "Uncategorized" : categoryName(catMap, filterCat)}</span>
            <span class="chip-close" aria-label="Remove filter">\u00d7</span>
          </button>
        `
        }
        ${
          filterAcct &&
          html`
          <button class="chip" onClick=${() => setFilterAcct("")}>
            <span>${acctMap[filterAcct]?.name ?? filterAcct}</span>
            <span class="chip-close" aria-label="Remove filter">\u00d7</span>
          </button>
        `
        }
        ${
          filterMethod &&
          html`
          <button class="chip" onClick=${() => setFilterMethod("")}>
            <span>${filterMethod === "__none" ? "Uncategorized" : filterMethod === "llm" ? "LLM" : filterMethod.charAt(0).toUpperCase() + filterMethod.slice(1)}</span>
            <span class="chip-close" aria-label="Remove filter">\u00d7</span>
          </button>
        `
        }
      </div>
    `
    }

    <div class="table txn-table">
      <table>
        <thead>
          <tr>
            <${SortTh} col="date">Date<//>
            <${SortTh} col="merchant">Merchant<//>
            <${SortTh} col="amount">Amount<//>
            <${SortTh} col="category">Category<//>
            <${SortTh} col="account">Account<//>
          </tr>
        </thead>
        <tbody>
          ${sorted.map(
            (t) => html`
              <tr
                class=${t.correlation_type ? "row-correlated" : ""}
                onClick=${() => setSelected(t)}
              >
                <td class="mono">${t.posted_date}</td>
                <td style="font-weight:500">${cleanMerchant(t.merchant_name || t.description)}</td>
                <td class="${amountClass(t.amount)}">${formatAmount(t.amount)}</td>
                <td>
                  <${MethodDot} method=${t.category_method} />
                  <${CategoryBadge} catMap=${catMap} id=${t.category_id} suggested=${t.suggested_category} />
                  ${
                    t.correlation_type
                      ? html`<span class="chip outline small">${t.correlation_type}</span>`
                      : null
                  }
                </td>
                <td class="text-light">${acctMap[t.account_id]?.name ?? ""}</td>
              </tr>
            `,
          )}
        </tbody>
      </table>
    </div>

    <${TxnDetail}
      txn=${selected}
      catMap=${catMap}
      categories=${categories}
      acctMap=${acctMap}
      onCategorize=${(txnId, categoryId) => {
        setTxns((prev) =>
          prev.map((t) =>
            t.id === txnId
              ? { ...t, category_id: categoryId, suggested_category: null }
              : t,
          ),
        );
        setSelected((prev) =>
          prev && prev.id === txnId
            ? { ...prev, category_id: categoryId, suggested_category: null }
            : prev,
        );
      }}
      onClose=${() => setSelected(null)}
      onRuleCreated=${() => {}}
    />
  `;
}

// ---------------------------------------------------------------------------
// Categories
// ---------------------------------------------------------------------------

function Categories() {
  const [categories, setCategories] = useState(null);
  const [suggestions, setSuggestions] = useState(null);
  const [error, setError] = useState(null);
  const [name, setName] = useState("");
  const [parentId, setParentId] = useState("");
  const [adding, setAdding] = useState(false);
  const [selectedSuggestions, setSelectedSuggestions] = useState(new Set());
  const [editingId, setEditingId] = useState(null);
  const [editForm, setEditForm] = useState(null);
  const [saving, setSaving] = useState(false);

  function load() {
    Promise.all([api.get("/categories"), api.get("/categories/suggestions")])
      .then(([c, s]) => {
        setCategories(c);
        setSuggestions(s);
      })
      .catch(setError);
  }

  useEffect(() => {
    load();
  }, []);

  async function handleAdd(e) {
    e.preventDefault();
    if (!name.trim()) return;
    setAdding(true);
    try {
      await api.post("/categories", {
        name: name.trim(),
        parent_id: parentId || undefined,
      });
      setName("");
      setParentId("");
      load();
    } catch (err) {
      setError(err);
    } finally {
      setAdding(false);
    }
  }

  async function handleDelete(id) {
    try {
      await api.del(`/categories/${id}`);
      if (editingId === id) cancelEdit();
      load();
    } catch (err) {
      setError(err);
    }
  }

  async function createSuggestedCategory(catName) {
    const parts = catName.split(":");
    let parentIdForNew;
    if (parts.length > 1) {
      const parentName = parts.slice(0, -1).join(":");
      const existingParent = (categories ?? []).find(
        (c) => c.name === parentName,
      );
      if (existingParent) {
        parentIdForNew = existingParent.id;
      } else {
        const created = await api.post("/categories", { name: parentName });
        parentIdForNew = created.id;
      }
    }
    await api.post("/categories", {
      name: catName,
      parent_id: parentIdForNew,
    });
  }

  function toggleSuggestion(catName) {
    setSelectedSuggestions((prev) => {
      const next = new Set(prev);
      if (next.has(catName)) next.delete(catName);
      else next.add(catName);
      return next;
    });
  }

  async function acceptSelected() {
    setAdding(true);
    try {
      for (const catName of selectedSuggestions) {
        await createSuggestedCategory(catName);
      }
      setSelectedSuggestions(new Set());
      load();
    } catch (err) {
      setError(err);
    } finally {
      setAdding(false);
    }
  }

  function startEdit(cat) {
    setEditingId(cat.id);
    setEditForm({
      name: cat.name,
      parent_id: cat.parent_id ?? "",
      budget_mode: cat.budget_mode ?? "",
      budget_amount: cat.budget_amount ?? "",
      project_start_date: cat.project_start_date ?? "",
      project_end_date: cat.project_end_date ?? "",
    });
  }

  function cancelEdit() {
    setEditingId(null);
    setEditForm(null);
  }

  function setEditField(key, value) {
    setEditForm((prev) => ({ ...prev, [key]: value }));
  }

  async function handleEditSubmit(e) {
    e.preventDefault();
    setSaving(true);
    try {
      await api.put(`/categories/${editingId}`, {
        name: editForm.name,
        parent_id: editForm.parent_id || null,
        budget_mode: editForm.budget_mode || null,
        budget_amount: editForm.budget_amount || null,
        project_start_date: editForm.project_start_date || null,
        project_end_date: editForm.project_end_date || null,
      });
      cancelEdit();
      load();
    } catch (err) {
      setError(err);
    } finally {
      setSaving(false);
    }
  }

  const editDialogRef = (el) => {
    if (el && !el.open) {
      el.addEventListener("close", cancelEdit, { once: true });
      el.showModal();
    }
  };

  if (error) return html`<p class="text-light">${error.message}</p>`;
  if (!categories) return html`<p class="text-light">Loading...</p>`;

  const catMap = Object.fromEntries(categories.map((c) => [c.id, c]));
  const existingNames = new Set(categories.map((c) => c.name));
  const roots = categories.filter((c) => !c.parent_id || !catMap[c.parent_id]);

  function withDepth(cats) {
    const parentIds = new Set(
      cats.filter((c) => !c.parent_id).map((c) => c.id),
    );
    const childMap = {};
    for (const c of cats) {
      if (c.parent_id && parentIds.has(c.parent_id)) {
        if (!childMap[c.parent_id]) childMap[c.parent_id] = [];
        childMap[c.parent_id].push(c);
      }
    }
    const result = [];
    for (const r of cats.filter(
      (c) => !c.parent_id || !parentIds.has(c.parent_id),
    )) {
      const isRoot = !r.parent_id;
      result.push({ ...r, depth: isRoot ? 0 : 1 });
      if (isRoot) {
        for (const ch of childMap[r.id] ?? []) {
          result.push({ ...ch, depth: 1 });
        }
      }
    }
    return result;
  }

  const groups = [
    { key: null, label: "Unbudgeted", desc: "No budget assigned yet" },
    { key: "monthly", label: "Monthly", desc: null },
    { key: "annual", label: "Annual", desc: null },
    { key: "project", label: "Project", desc: null },
  ];

  // Determine which budget group a category belongs to:
  // - Own budget_mode if set, otherwise inherit from parent
  function effectiveGroup(c) {
    if (c.budget_mode) return c.budget_mode;
    const parent = c.parent_id ? catMap[c.parent_id] : null;
    return parent?.budget_mode ?? null;
  }

  const grouped = {};
  for (const g of groups) {
    const cats = categories.filter((c) => effectiveGroup(c) === g.key);
    grouped[g.key] = withDepth(cats);
  }

  const pendingSuggestions = (suggestions ?? []).filter(
    (s) => !existingNames.has(s.category_name),
  );

  function budgetBadge(cat) {
    if (!cat.budget_mode) return null;
    if (cat.budget_mode === "project") {
      const parts = [];
      if (cat.project_start_date) parts.push(cat.project_start_date);
      if (cat.project_end_date) parts.push(cat.project_end_date);
      return parts.length > 0
        ? html`<span class="text-light" style="font-size:0.85rem">${parts.join(" \u2013 ")}</span>`
        : null;
    }
    const amt =
      cat.budget_amount != null
        ? `${Number(cat.budget_amount).toFixed(0)}\u202F\u20AC`
        : "?";
    return html`<span>${amt}</span>`;
  }

  return html`
    <h2>Categories</h2>
    <p class="text-light" style="margin-bottom:1rem">
      ${categories.length} categor${categories.length !== 1 ? "ies" : "y"}
    </p>

    ${
      pendingSuggestions.length > 0 &&
      html`
        <div style="margin-bottom:1.5rem">
          <h3>LLM Suggestions</h3>
          <p class="text-light" style="margin-bottom:0.5rem">
            The LLM suggested these categories for uncategorized transactions.
            Select to accept, then re-run categorize.
          </p>
          <div class="hstack gap-2" role="group" aria-label="Suggested categories" style="margin-bottom:0.75rem">
            ${pendingSuggestions.map(
              (s) => html`
                <button
                  class="chip"
                  aria-pressed=${selectedSuggestions.has(s.category_name) ? "true" : "false"}
                  onClick=${() => toggleSuggestion(s.category_name)}
                  title="${s.count} transaction${s.count !== 1 ? "s" : ""}"
                >
                  ${s.category_name}
                </button>
              `,
            )}
          </div>
          ${
            selectedSuggestions.size > 0 &&
            html`
            <button
              data-variant="primary"
              onClick=${acceptSelected}
              disabled=${adding}
            >
              ${adding ? "Accepting..." : `Accept ${selectedSuggestions.size} Selected`}
            </button>
          `
          }
        </div>
      `
    }

    <form style="display:flex;gap:0.5rem;align-items:center;flex-wrap:wrap;margin-bottom:1rem" onSubmit=${handleAdd}>
      <input
        type="text"
        placeholder="Category name"
        value=${name}
        onInput=${(e) => setName(e.target.value)}
        required
      />
      <select value=${parentId} onChange=${(e) => setParentId(e.target.value)}>
        <option value="">No parent (top-level)</option>
        ${roots.map((c) => html`<option value=${c.id}>${c.name}</option>`)}
      </select>
      <button data-variant="primary" type="submit" disabled=${adding}>
        ${adding ? "Adding..." : "Add"}
      </button>
    </form>

    ${
      categories.length === 0
        ? html`<p class="text-light">No categories yet. Add one above.</p>`
        : groups
            .filter((g) => grouped[g.key].length > 0)
            .map(
              (g) => html`
        <div key=${g.key} style="margin-bottom:1.5rem">
          <h3 style="margin-bottom:0.25rem">${g.label}</h3>
          ${g.desc && html`<p class="text-light" style="margin-bottom:0.5rem">${g.desc}</p>`}
          <table>
            <tbody>
              ${grouped[g.key].map(
                (c) => html`
                <tr key=${c.id}>
                  <td>
                    <span style="padding-left:${c.depth * 1.5}rem">
                      ${
                        c.depth > 0
                          ? html`<span class="text-light" style="font-size:0.85rem;margin-right:0.25rem"
                          >${catMap[c.parent_id]?.name} ></span
                        > `
                          : null
                      }
                      ${c.name}
                    </span>
                  </td>
                  <td style="text-align:right">${budgetBadge(c)}</td>
                  <td style="text-align:right;white-space:nowrap">
                    <button class="small" onClick=${() => startEdit(c)}>Edit</button>
                    <button
                      data-variant="danger" class="small"
                      onClick=${() => handleDelete(c.id)}
                    >
                      Delete
                    </button>
                  </td>
                </tr>
              `,
              )}
            </tbody>
          </table>
        </div>
      `,
            )
    }

    ${
      editForm &&
      html`
      <dialog ref=${editDialogRef} closedby="any">
        <form onSubmit=${handleEditSubmit}>
          <header>
            <h3>Edit Category</h3>
          </header>
          <div class="vstack">
            <label data-field>
              Name
              <input
                type="text"
                value=${editForm.name}
                onInput=${(e) => setEditField("name", e.target.value)}
                required
              />
            </label>
            <div data-field>
              <label>Parent</label>
              <select
                value=${editForm.parent_id}
                onChange=${(e) => setEditField("parent_id", e.target.value)}
              >
                <option value="">No parent (top-level)</option>
                ${roots
                  .filter((r) => r.id !== editingId)
                  .map((r) => html`<option value=${r.id}>${r.name}</option>`)}
              </select>
            </div>
            <hr />
            <div data-field>
              <label>Budget</label>
              <select
                value=${editForm.budget_mode}
                onChange=${(e) => setEditField("budget_mode", e.target.value)}
              >
                <option value="">No budget</option>
                <option value="monthly">Monthly</option>
                <option value="annual">Annual</option>
                <option value="project">Project</option>
              </select>
            </div>
            ${
              editForm.budget_mode &&
              html`
              <label data-field>
                Amount
                <input
                  type="number"
                  step="0.01"
                  min="0"
                  placeholder="0.00"
                  value=${editForm.budget_amount}
                  onInput=${(e) => setEditField("budget_amount", e.target.value)}
                />
              </label>
            `
            }
            ${
              editForm.budget_mode === "project" &&
              html`
              <div class="hstack gap-2">
                <label data-field style="flex:1">
                  Start date
                  <input
                    type="date"
                    value=${editForm.project_start_date}
                    onInput=${(e) => setEditField("project_start_date", e.target.value)}
                  />
                </label>
                <label data-field style="flex:1">
                  End date
                  <input
                    type="date"
                    value=${editForm.project_end_date}
                    onInput=${(e) => setEditField("project_end_date", e.target.value)}
                  />
                </label>
              </div>
            `
            }
          </div>
          <footer>
            <button type="button" class="outline" onClick=${(e) => e.target.closest("dialog").close()}>Cancel</button>
            <button type="submit" disabled=${saving}>
              ${saving ? "Saving..." : "Save"}
            </button>
          </footer>
        </form>
      </dialog>
    `
    }
  `;
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

function Rules() {
  const [rules, setRules] = useState(null);
  const [categories, setCategories] = useState(null);
  const [error, setError] = useState(null);
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState(null);
  const [saving, setSaving] = useState(false);
  const [applying, setApplying] = useState(false);

  const emptyForm = {
    rule_type: "categorization",
    match_field: "merchant",
    match_pattern: "",
    target_category_id: "",
    target_correlation_type: "",
    priority: 0,
  };
  const [form, setForm] = useState(emptyForm);

  function load() {
    Promise.all([api.get("/rules"), api.get("/categories")])
      .then(([r, c]) => {
        setRules(r);
        setCategories(c);
      })
      .catch(setError);
  }

  useEffect(load, []);

  if (error) return html`<p class="text-light">${error.message}</p>`;
  if (!rules) return html`<p class="text-light">Loading...</p>`;

  const catMap = Object.fromEntries((categories ?? []).map((c) => [c.id, c]));

  function setField(key, value) {
    setForm((prev) => ({ ...prev, [key]: value }));
  }

  function startEdit(rule) {
    setEditingId(rule.id);
    setForm({
      rule_type: rule.rule_type,
      match_field: rule.match_field,
      match_pattern: rule.match_pattern,
      target_category_id: rule.target_category_id ?? "",
      target_correlation_type: rule.target_correlation_type ?? "",
      priority: rule.priority,
    });
    setShowForm(false);
  }

  function cancelEdit() {
    setEditingId(null);
    setForm(emptyForm);
  }

  async function handleSubmit(e) {
    e.preventDefault();
    setSaving(true);
    const body = {
      rule_type: form.rule_type,
      match_field: form.match_field,
      match_pattern: form.match_pattern,
      target_category_id: form.target_category_id || null,
      target_correlation_type: form.target_correlation_type || null,
      priority: Number(form.priority),
    };

    try {
      if (editingId) {
        await api.put(`/rules/${editingId}`, body);
        setEditingId(null);
      } else {
        await api.post("/rules", body);
        setShowForm(false);
      }
      setForm(emptyForm);
      load();
    } catch (err) {
      setError(err);
    } finally {
      setSaving(false);
    }
  }

  async function handleDelete(id) {
    try {
      await api.del(`/rules/${id}`);
      load();
    } catch (err) {
      setError(err);
    }
  }

  async function handleApplyAll() {
    setApplying(true);
    setError(null);
    try {
      const result = await api.post("/rules/apply");
      setApplying(false);
      load();
      if (result.categorized_count > 0) {
        alert(
          `Categorized ${result.categorized_count} transaction${result.categorized_count !== 1 ? "s" : ""}`,
        );
      }
    } catch (err) {
      setError(err);
      setApplying(false);
    }
  }

  const categorizationRules = rules.filter(
    (r) => r.rule_type === "categorization",
  );
  const correlationRules = rules.filter((r) => r.rule_type === "correlation");

  function fieldLabel(field) {
    if (field === "amount_range") return "amount range";
    return field;
  }

  function ruleTarget(rule) {
    if (rule.rule_type === "categorization") {
      return categoryName(catMap, rule.target_category_id) ?? "none";
    }
    return rule.target_correlation_type ?? "none";
  }

  function renderFormFields() {
    return html`
      <select
        value=${form.rule_type}
        onInput=${(e) => setField("rule_type", e.target.value)}
      >
        <option value="categorization">Categorization</option>
        <option value="correlation">Correlation</option>
      </select>
      <select
        value=${form.match_field}
        onInput=${(e) => setField("match_field", e.target.value)}
      >
        <option value="merchant">Merchant</option>
        <option value="description">Description</option>
        <option value="amount_range">Amount Range</option>
      </select>
      <input
        type="text"
        placeholder="Pattern"
        value=${form.match_pattern}
        onInput=${(e) => setField("match_pattern", e.target.value)}
        required
      />
      ${
        form.rule_type === "categorization"
          ? html`<select
            value=${form.target_category_id}
            onInput=${(e) => setField("target_category_id", e.target.value)}
          >
            <option value="">-- Category --</option>
            ${(categories ?? []).map(
              (c) =>
                html`<option value=${c.id}>${categoryName(catMap, c.id)}</option>`,
            )}
          </select>`
          : html`<select
            value=${form.target_correlation_type}
            onInput=${(e) =>
              setField("target_correlation_type", e.target.value)}
          >
            <option value="">-- Correlation --</option>
            <option value="transfer">Transfer</option>
            <option value="reimbursement">Reimbursement</option>
          </select>`
      }
      <input
        type="number"
        placeholder="Priority"
        value=${form.priority}
        onInput=${(e) => setField("priority", e.target.value)}
        style="width:5rem"
      />
    `;
  }

  function renderRow(rule) {
    if (editingId === rule.id) {
      return html`
        <tr key=${rule.id} class="">
          <td>${renderFormFields()}</td>
          <td style="white-space:nowrap">
            <button data-variant="primary" class="small" onClick=${handleSubmit} disabled=${saving}>
              Save
            </button>
            <button class="small" onClick=${cancelEdit}>Cancel</button>
          </td>
        </tr>
      `;
    }

    return html`
      <tr key=${rule.id}>
        <td>
          <div class="hstack">
            <span class="mono" style="font-size:0.8rem;min-width:1.5rem;text-align:right">${rule.priority}</span>
            <span class="chip outline ${rule.rule_type === "categorization" ? "success" : ""}"
              >${rule.rule_type}</span
            >
            <span class="text-light">${fieldLabel(rule.match_field)}</span>
            <code class="">${rule.match_pattern}</code>
            <span class="text-light">\u2192</span>
            <span>${ruleTarget(rule)}</span>
          </div>
        </td>
        <td style="white-space:nowrap">
          <button class="small" onClick=${() => startEdit(rule)}>Edit</button>
          <button data-variant="danger" class="small" onClick=${() => handleDelete(rule.id)}>
            Delete
          </button>
        </td>
      </tr>
    `;
  }

  const sorted = [...rules].sort((a, b) => b.priority - a.priority);

  return html`
    <h2>Rules</h2>
    <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:1rem">
      <span class="text-light">
        ${rules.length} rule${rules.length !== 1 ? "s" : ""}${" \u2014 "}
        ${categorizationRules.length} categorization, ${correlationRules.length} correlation
      </span>
      <div class="hstack">
        <button
          onClick=${handleApplyAll}
          disabled=${applying}
        >
          ${applying ? "Applying..." : "Apply All Rules"}
        </button>
        <button
          data-variant="primary"
          onClick=${() => {
            setShowForm(!showForm);
            setEditingId(null);
            setForm(emptyForm);
          }}
        >
          ${showForm ? "Cancel" : "Add Rule"}
        </button>
      </div>
    </div>

    ${
      showForm &&
      html`
      <form style="border:1px solid var(--border);border-radius:4px;padding:1rem;margin-bottom:1rem;display:flex;flex-direction:column;gap:0.75rem" onSubmit=${handleSubmit}>
        <div style="display:flex;flex-wrap:wrap;gap:0.5rem;align-items:center">${renderFormFields()}</div>
        <button data-variant="primary" type="submit" disabled=${saving}>
          Create Rule
        </button>
      </form>
    `
    }

    ${
      rules.length === 0
        ? html`<p class="text-light" style="margin-top:1rem">
          No rules yet. Add a rule to teach the system how to categorize and
          correlate your transactions.
        </p>`
        : html`
          <table>
            <thead>
              <tr>
                <th>Rule</th>
                <th style="width:10rem">Actions</th>
              </tr>
            </thead>
            <tbody>
              ${sorted.map(renderRow)}
            </tbody>
          </table>
        `
    }
  `;
}

// ---------------------------------------------------------------------------
// Connections
// ---------------------------------------------------------------------------

function Connections() {
  const [connections, setConnections] = useState(null);
  const [accounts, setAccounts] = useState(null);
  const [error, setError] = useState(null);
  const [searchCountry, setSearchCountry] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [aspsps, setAspsps] = useState(null);
  const [searchLoading, setSearchLoading] = useState(false);
  const [searchError, setSearchError] = useState(null);
  const [authorizing, setAuthorizing] = useState(null);
  const [authError, setAuthError] = useState(null);

  function load() {
    Promise.all([api.get("/connections"), api.get("/accounts")])
      .then(([c, a]) => {
        setConnections(c);
        setAccounts(a);
      })
      .catch(setError);
  }

  useEffect(load, []);

  function statusBadge(status) {
    if (status === "active") return "success";
    if (status === "expired") return "warning";
    return "danger";
  }

  function accountCount(connectionId) {
    if (!accounts) return 0;
    return accounts.filter((a) => a.connection_id === connectionId).length;
  }

  async function searchAspsps() {
    setSearchLoading(true);
    setSearchError(null);
    setAspsps(null);
    try {
      const params = searchCountry
        ? `?country=${encodeURIComponent(searchCountry)}`
        : "";
      const results = await api.get(`/connections/aspsps${params}`);
      setAspsps(results);
    } catch (e) {
      setSearchError(e.message);
    } finally {
      setSearchLoading(false);
    }
  }

  async function startAuth(aspsp) {
    setAuthorizing(aspsp.name);
    setAuthError(null);
    try {
      const res = await api.post("/connections/authorize", {
        aspsp_name: aspsp.name,
        aspsp_country: aspsp.country,
      });
      window.open(res.authorization_url, "_blank");
    } catch (e) {
      setAuthError(e.message);
    } finally {
      setAuthorizing(null);
    }
  }

  async function revokeConnection(id) {
    try {
      await api.del(`/connections/${id}`);
      load();
    } catch (e) {
      setError(e);
    }
  }

  const filteredAspsps =
    aspsps && searchQuery
      ? aspsps.filter((a) =>
          a.name.toLowerCase().includes(searchQuery.toLowerCase()),
        )
      : aspsps;

  if (error) return html`<p class="text-light">${error.message}</p>`;
  if (!connections) return html`<p class="text-light">Loading...</p>`;

  return html`
    <h2>Connections</h2>

    ${
      connections.length === 0
        ? html`<p class="text-light" style="margin-bottom:1.5rem">
            No bank connections yet. Search for your bank below to get started.
          </p>`
        : html`
            <p class="text-light" style="margin-bottom:1rem">
              ${connections.length} connection${connections.length !== 1 ? "s" : ""}
            </p>
            <table>
              <thead>
                <tr>
                  <th>Institution</th>
                  <th>Status</th>
                  <th>Valid Until</th>
                  <th>Accounts</th>
                  <th>Actions</th>
                </tr>
              </thead>
              <tbody>
                ${connections.map(
                  (c) => html`
                    <tr>
                      <td>${c.institution_name}</td>
                      <td>
                        <span class="chip ${statusBadge(c.status)}">${c.status}</span>
                      </td>
                      <td class="mono">${c.valid_until}</td>
                      <td>${accountCount(c.id)}</td>
                      <td>
                        ${
                          c.status === "expired"
                            ? html`<button
                                data-variant="primary" class="small"
                                style="margin-right:0.5rem"
                                onClick=${() =>
                                  startAuth({
                                    name: c.institution_name,
                                    country: "",
                                  })}
                              >
                                Reconnect
                              </button>`
                            : null
                        }
                        ${
                          c.status !== "revoked"
                            ? html`<button data-variant="danger" class="small" onClick=${() => revokeConnection(c.id)}>
                                Revoke
                              </button>`
                            : null
                        }
                      </td>
                    </tr>
                  `,
                )}
              </tbody>
            </table>
          `
    }

    <div style="margin-top:2rem">
      <h3>Connect Bank</h3>
      <div style="display:flex;gap:0.5rem;align-items:center;flex-wrap:wrap;margin-bottom:1rem" style="margin-top:0.75rem;margin-bottom:0.75rem">
        <input
          type="text"
          placeholder="Country code (e.g. FI)"
          value=${searchCountry}
          onInput=${(e) => setSearchCountry(e.target.value)}
          style="width:140px"
        />
        <button data-variant="primary" onClick=${searchAspsps} disabled=${searchLoading}>
          ${searchLoading ? "Searching..." : "Search Banks"}
        </button>
      </div>

      ${searchError ? html`<p role="alert" data-variant="error">${searchError}</p>` : null}

      ${
        aspsps
          ? html`
              <input
                type="text"
                placeholder="Filter results..."
                value=${searchQuery}
                onInput=${(e) => setSearchQuery(e.target.value)}
                style="width:100%;margin-bottom:0.75rem"
              />
              ${authError ? html`<p role="alert" data-variant="error">${authError}</p>` : null}
              ${
                filteredAspsps && filteredAspsps.length > 0
                  ? html`
                      <table>
                        <thead>
                          <tr>
                            <th>Bank</th>
                            <th>Country</th>
                            <th></th>
                          </tr>
                        </thead>
                        <tbody>
                          ${filteredAspsps.map(
                            (a) => html`
                              <tr>
                                <td>${a.name}</td>
                                <td>${a.country}</td>
                                <td>
                                  <button
                                    data-variant="primary" class="small"
                                    onClick=${() => startAuth(a)}
                                    disabled=${authorizing === a.name}
                                  >
                                    ${authorizing === a.name ? "Redirecting..." : "Connect"}
                                  </button>
                                </td>
                              </tr>
                            `,
                          )}
                        </tbody>
                      </table>
                    `
                  : html`<p class="text-light">No banks found matching your search.</p>`
              }
            `
          : null
      }
    </div>
  `;
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

// Queue card definitions for the Immich-style display
const QUEUE_CARDS = [
  {
    key: "sync",
    title: "Sync",
    desc: "Fetch new transactions from connected bank accounts",
    types: ["SyncJob", "Vec<u8>"],
  },
  {
    key: "categorize",
    title: "Categorize",
    desc: "Classify transactions using rules and LLM",
    types: ["CategorizeJob", "CategorizeTransactionJob"],
  },
  {
    key: "correlate",
    title: "Correlate",
    desc: "Link related transactions (transfers, reimbursements)",
    types: ["CorrelateJob", "CorrelateTransactionJob"],
  },
  {
    key: "recompute",
    title: "Recompute",
    desc: "Recompute budget month boundaries",
    types: ["BudgetRecomputeJob"],
  },
];

function timeAgo(iso) {
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

function Jobs() {
  const [counts, setCounts] = useState(null);
  const [accounts, setAccounts] = useState(null);
  const [schedule, setSchedule] = useState(null);
  const [error, setError] = useState(null);
  const [syncAccountId, setSyncAccountId] = useState("");
  const [triggering, setTriggering] = useState(null);

  function load() {
    Promise.all([
      api.get("/jobs/counts"),
      api.get("/accounts"),
      api.get("/jobs/schedule"),
    ])
      .then(([c, a, s]) => {
        setCounts(c);
        setAccounts(a);
        setSchedule(s);
      })
      .catch(setError);
  }

  useEffect(() => {
    load();
    const interval = setInterval(() => {
      Promise.all([api.get("/jobs/counts"), api.get("/jobs/schedule")])
        .then(([c, s]) => {
          setCounts(c);
          setSchedule(s);
        })
        .catch(() => {});
    }, 5000);
    return () => clearInterval(interval);
  }, []);

  async function trigger(path, name) {
    setTriggering(name);
    setError(null);
    try {
      await api.post(path);
      load();
    } catch (err) {
      setError(err);
    } finally {
      setTriggering(null);
    }
  }

  async function triggerSync() {
    if (!syncAccountId) return;
    await trigger(`/jobs/pipeline/${syncAccountId}`, "sync");
  }

  // Aggregate queue counts for a card's job types
  function cardCounts(card) {
    if (!counts) return { active: 0, waiting: 0, completed: 0, failed: 0 };
    const agg = { active: 0, waiting: 0, completed: 0, failed: 0 };
    for (const c of counts) {
      const shortName = c.job_type.includes("::")
        ? c.job_type.split("::").pop()
        : c.job_type;
      if (card.types.includes(shortName)) {
        agg.active += c.active;
        agg.waiting += c.waiting;
        agg.completed += c.completed;
        agg.failed += c.failed;
      }
    }
    return agg;
  }

  function renderQueueCard(card) {
    const c = cardCounts(card);
    const isSync = card.key === "sync";

    return html`
      <div class="queue-card">
        <span class="queue-card-title">${card.title}</span>
        ${
          c.failed > 0
            ? html`<span class="chip danger">${c.failed} failed</span>`
            : null
        }
        ${
          isSync
            ? html`
                <select
                  value=${syncAccountId}
                  onChange=${(e) => setSyncAccountId(e.target.value)}
                  style="font-size:0.85rem"
                >
                  <option value="">Account...</option>
                  ${(accounts ?? []).map(
                    (a) => html`<option value=${a.id}>${a.name}</option>`,
                  )}
                </select>
                <button
                  data-variant="primary" class="small"
                  onClick=${triggerSync}
                  disabled=${!syncAccountId || triggering === "sync"}
                >
                  ${triggering === "sync" ? "..." : "Sync"}
                </button>
              `
            : html`
                <button
                  data-variant="primary" class="small"
                  onClick=${() => trigger(`/jobs/${card.key === "recompute" ? "recompute" : card.key}`, card.key)}
                  disabled=${triggering === card.key}
                >
                  ${triggering === card.key ? "..." : card.key === "recompute" ? "Run" : "Run All"}
                </button>
              `
        }
        <span class="queue-card-desc">${card.desc}</span>
        <div class="hstack gap-2" style="width:100%;margin-top:0.25rem">
          <span class="chip outline"><span class="text-light">Active</span> <span class="mono">${c.active}</span></span>
          <span class="chip outline"><span class="text-light">Waiting</span> <span class="mono">${c.waiting}</span></span>
        </div>
      </div>
    `;
  }

  if (error && !counts) return html`<p class="text-light">${error.message}</p>`;
  if (!counts) return html`<p class="text-light">Loading...</p>`;

  function renderScheduleRow(s) {
    const isOk = s.last_run_status === "succeeded";
    const isFailed = s.last_run_status === "failed";
    const isRunning = s.last_run_status === "running";
    const nextReason = s.next_run_reason ? ` (${s.next_run_reason})` : "";
    return html`
      <tr>
        <td>${s.account_name}</td>
        <td>${timeAgo(s.last_run_at)}</td>
        <td>
          ${isOk && html`<span class="chip success">OK</span>`}
          ${isFailed && html`<span class="chip danger" title=${s.last_error || ""}>Failed</span>`}
          ${isRunning && html`<span class="chip outline">Running</span>`}
          ${!s.last_run_status && html`<span class="text-light">\u2014</span>`}
        </td>
        <td>
          ${s.next_run_at ? html`${timeAgo(s.next_run_at)}${nextReason}` : html`<span class="text-light">\u2014</span>`}
        </td>
      </tr>
    `;
  }

  return html`
    <h2>Jobs</h2>

    ${error && html`<p role="alert" data-variant="error">${error.message}</p>`}

    ${
      schedule &&
      schedule.length > 0 &&
      html`
      <h3>Schedule</h3>
      <table style="margin-bottom:1.5rem">
        <thead>
          <tr>
            <th>Account</th>
            <th>Last Run</th>
            <th>Status</th>
            <th>Next Run</th>
          </tr>
        </thead>
        <tbody>
          ${schedule.map(renderScheduleRow)}
        </tbody>
      </table>
    `
    }

    <div class="queue-cards">
      ${QUEUE_CARDS.map(renderQueueCard)}
    </div>
  `;
}

// ---------------------------------------------------------------------------
// Auth gate
// ---------------------------------------------------------------------------

function Login({ onLogin }) {
  const [token, setToken] = useState("");
  const [error, setError] = useState(null);

  const submit = async (e) => {
    e.preventDefault();
    try {
      api.token = token;
      await api.get("/accounts");
      localStorage.setItem("budget_token", token);
      onLogin();
    } catch {
      api.token = "";
      setError("Invalid token");
    }
  };

  return html`
    <div
      style="display:flex;align-items:center;justify-content:center;min-height:100vh"
    >
      <form onSubmit=${submit} style="width:320px">
        <h2>Budget</h2>
        <p class="text-light" style="margin-bottom:1rem">Enter your API token.</p>
        ${error && html`<p role="alert" data-variant="error">${error}</p>`}
        <input
          type="password"
          value=${token}
          onInput=${(e) => setToken(e.target.value)}
          placeholder="Bearer token"
          style="width:100%;margin-bottom:0.5rem"
        />
        <button data-variant="primary" style="width:100%">Sign in</button>
      </form>
    </div>
  `;
}

// ---------------------------------------------------------------------------
// App shell
// ---------------------------------------------------------------------------

function App() {
  const [authed, setAuthed] = useState(!!api.token);
  const route = useRoute();

  if (!authed) return html`<${Login} onLogin=${() => setAuthed(true)} />`;

  const page = () => {
    if (route === "/") return html`<${Dashboard} />`;
    if (route === "/transactions") return html`<${Transactions} />`;
    if (route === "/categories") return html`<${Categories} />`;
    if (route === "/rules") return html`<${Rules} />`;
    if (route === "/connections") return html`<${Connections} />`;
    if (route === "/jobs") return html`<${Jobs} />`;
    return html`<p class="text-light">Not found.</p>`;
  };

  return html`
    <div data-sidebar-layout>
      <aside data-sidebar>
        <h1>Budget</h1>
        <nav>
          <${NavLink} href="/">Dashboard<//>
          <${NavLink} href="/transactions">Transactions<//>
          <${NavLink} href="/categories">Categories<//>
          <${NavLink} href="/rules">Rules<//>
          <${NavLink} href="/connections">Connections<//>
          <${NavLink} href="/jobs">Jobs<//>
        </nav>
        <a
          href="#"
          style="margin-top:auto;opacity:0.5"
          onClick=${(e) => {
            e.preventDefault();
            localStorage.removeItem("budget_token");
            api.token = "";
            setAuthed(false);
          }}
          >Sign out</a
        >
      </aside>
      <main class="main">${page()}</main>
    </div>
  `;
}

render(html`<${App} />`, document.getElementById("app"));
