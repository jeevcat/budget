import htm from "htm";
import { h, render } from "preact";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "preact/hooks";
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
  formatAmount,
  formatDate,
  formatDateFull,
  formatDateRange,
  formatDateShort,
  formatRemittanceInfo,
  paceBadge,
  paceColor,
  paceLabel,
  QUEUE_CARDS,
  shortType,
  syncUrlFor,
  timeAgo,
  transactionTitle,
} from "./helpers.js";

const html = htm.bind(h);

// ---------------------------------------------------------------------------
// API helper
// ---------------------------------------------------------------------------

const api = {
  async fetch(path, opts = {}) {
    const res = await fetch(`/api${path}`, {
      ...opts,
      credentials: "same-origin",
      headers: {
        "Content-Type": "application/json",
        ...opts.headers,
      },
      body: opts.body
        ? opts.rawBody
          ? opts.body
          : JSON.stringify(opts.body)
        : undefined,
    });
    if (!res.ok) {
      let msg;
      try {
        const body = await res.json();
        msg = body.error || JSON.stringify(body);
      } catch {
        msg = await res.text();
      }
      throw new Error(`${res.status}: ${msg}`);
    }
    if (res.status === 204 || res.status === 202) return null;
    return res.json();
  },

  get: (path) => api.fetch(path),
  post: (path, body) => api.fetch(path, { method: "POST", body }),
  put: (path, body) => api.fetch(path, { method: "PUT", body }),
  patch: (path, body) => api.fetch(path, { method: "PATCH", body }),
  del: (path) => api.fetch(path, { method: "DELETE" }),
  raw: (path, body, contentType) =>
    api.fetch(path, {
      method: "POST",
      body,
      rawBody: true,
      headers: { "Content-Type": contentType },
    }),
};

/** Poll /jobs/counts until no jobs are active or waiting, then resolve. */
async function waitForJobsDrained() {
  for (let i = 0; i < 20; i++) {
    await new Promise((r) => setTimeout(r, 500));
    try {
      const counts = await api.get("/jobs/counts");
      const busy = counts.some((q) => q.active > 0 || q.waiting > 0);
      if (!busy) return;
    } catch {
      return;
    }
  }
}

// Date formatting, account display, amount formatting, category helpers,
// text helpers, pace helpers, time ago, job queue helpers — all in helpers.js

// ---------------------------------------------------------------------------
// Shared error display
// ---------------------------------------------------------------------------

function ErrorPanel({ error, onRetry }) {
  return html`
    <div role="alert" data-variant="error">
      <p>${error?.message ?? error}</p>
      ${onRetry && html`<button data-variant="primary" onClick=${onRetry}>Retry</button>`}
    </div>
  `;
}

// ---------------------------------------------------------------------------
// Simple hash router
// ---------------------------------------------------------------------------

function hashPath() {
  const raw = location.hash.slice(1) || "/";
  const i = raw.indexOf("?");
  return i < 0 ? raw : raw.slice(0, i);
}

function hashParams() {
  const i = location.hash.indexOf("?");
  return i < 0
    ? new URLSearchParams()
    : new URLSearchParams(location.hash.slice(i + 1));
}

function useRoute() {
  const [route, setRoute] = useState(hashPath());
  useEffect(() => {
    const onHash = () => setRoute(hashPath());
    addEventListener("hashchange", onHash);
    return () => removeEventListener("hashchange", onHash);
  }, []);
  return route;
}

function NavLink({ href, match, children }) {
  const route = hashPath();
  const current = match
    ? match(route)
    : route === href || route.startsWith(`${href}/`);
  return html`<a href="#${href}" aria-current=${current ? "page" : undefined}
    >${children}</a
  >`;
}

/** Navigate by replacing the current history entry (no back/forward noise). */
function navigateReplace(path) {
  const url = new URL(location.href);
  url.hash = `#${path}`;
  history.replaceState(null, "", url);
  dispatchEvent(new HashChangeEvent("hashchange"));
}

// ---------------------------------------------------------------------------
// Searchable category select (combobox)
// ---------------------------------------------------------------------------

function CategorySelect({
  value,
  onChange,
  categories,
  catMap,
  placeholder,
  disabled,
  extraOptions,
  clearable,
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIdx, setActiveIdx] = useState(-1);
  const wrapRef = useRef(null);
  const inputRef = useRef(null);
  const listRef = useRef(null);

  const tree = buildCategoryTree(categories ?? []);

  const filtered = query
    ? tree.filter((c) => {
        const name = categoryName(catMap, c.id) || c.name;
        return name.toLowerCase().includes(query.toLowerCase());
      })
    : tree;

  const selectedName = value ? categoryName(catMap, value) || "" : "";

  useEffect(() => {
    if (!open) return;
    const onDown = (e) => {
      if (wrapRef.current && !wrapRef.current.contains(e.target)) {
        setOpen(false);
        setQuery("");
      }
    };
    document.addEventListener("mousedown", onDown);
    const scrollParent =
      wrapRef.current?.closest(".txn-panel-body") ||
      wrapRef.current?.closest("dialog > form > div");
    if (scrollParent) scrollParent.style.overflow = "visible";
    return () => {
      document.removeEventListener("mousedown", onDown);
      if (scrollParent) scrollParent.style.overflow = "";
    };
  }, [open]);

  useEffect(() => {
    if (!open || activeIdx < 0 || !listRef.current) return;
    const item = listRef.current.children[activeIdx];
    if (item) item.scrollIntoView({ block: "nearest" });
  }, [activeIdx, open]);

  function handleOpen() {
    if (disabled) return;
    setOpen(true);
    setQuery("");
    setActiveIdx(-1);
    setTimeout(() => inputRef.current?.focus(), 0);
  }

  function selectItem(id) {
    onChange(id);
    setOpen(false);
    setQuery("");
  }

  const extraCount = extraOptions ? extraOptions.length : 0;

  function onKeyDown(e) {
    if (!open) {
      if (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        handleOpen();
      }
      return;
    }
    const totalItems = extraCount + filtered.length;

    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIdx((prev) => (prev + 1) % totalItems);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIdx((prev) => (prev <= 0 ? totalItems - 1 : prev - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (activeIdx >= 0 && activeIdx < extraCount) {
        selectItem(extraOptions[activeIdx].value);
      } else if (activeIdx >= extraCount) {
        const cat = filtered[activeIdx - extraCount];
        if (cat) selectItem(cat.id);
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
      setQuery("");
    }
  }

  return html`
    <div class="cat-select" ref=${wrapRef}>
      <button
        type="button"
        class="cat-select-trigger"
        disabled=${disabled}
        onClick=${handleOpen}
        onKeyDown=${onKeyDown}
      >
        <span
          class=${
            value
              ? budgetModeColor(categoryBudgetMode(catMap, value))
              : "cat-select-placeholder"
          }
        >
          ${value ? selectedName : placeholder || "Select category..."}
        </span>
        ${
          clearable && value
            ? html`<span
              class="cat-select-clear"
              role="button"
              tabindex="-1"
              onClick=${(e) => {
                e.stopPropagation();
                onChange(null);
              }}
            >×</span>`
            : html`<svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2.5"
            >
              <path d="m6 9 6 6 6-6" />
            </svg>`
        }
      </button>
      ${
        open &&
        html`
          <div class="cat-select-dropdown">
            <input
              ref=${inputRef}
              type="text"
              class="cat-select-search"
              placeholder="Search categories..."
              value=${query}
              onInput=${(e) => {
                setQuery(e.target.value);
                setActiveIdx(-1);
              }}
              onKeyDown=${onKeyDown}
            />
            <div class="cat-select-list" ref=${listRef} role="listbox">
              ${extraOptions?.map(
                (opt, i) => html`
                  <div
                    key=${opt.value}
                    role="option"
                    class=${`cat-select-item${activeIdx === i ? " active" : ""}`}
                    onMouseEnter=${() => setActiveIdx(i)}
                    onClick=${() => selectItem(opt.value)}
                  >
                    ${opt.label}
                  </div>
                `,
              )}
              ${filtered.map((c, i) => {
                const idx = extraCount + i;
                const mode = categoryBudgetMode(catMap, c.id);
                const label = categoryLabel(catMap, c.id);
                const isActive = activeIdx === idx;
                const isSelected = c.id === value;
                return html`
                  <div
                    key=${c.id}
                    role="option"
                    aria-selected=${isSelected}
                    class=${
                      "cat-select-item" +
                      (c.depth ? " child" : "") +
                      (isActive ? " active" : "") +
                      (isSelected ? " selected" : "")
                    }
                    onMouseEnter=${() => setActiveIdx(idx)}
                    onClick=${() => selectItem(c.id)}
                  >
                    <span class=${budgetModeColor(mode)}>
                      ${
                        c.depth && label?.parent
                          ? html`<span class="cat-parent">${label.parent}</span>`
                          : ""
                      }${label?.short || c.name}
                    </span>
                  </div>
                `;
              })}
              ${
                filtered.length === 0 &&
                html`
                  <div class="cat-select-empty">No matching categories</div>
                `
              }
            </div>
          </div>
        `
      }
    </div>
  `;
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

function BudgetBar({ pace, fillPct, markPct }) {
  return html`
    <div class="ledger-bar-track">
      <div
        class="ledger-bar-fill ledger-bar-fill-${pace}"
        style="width:${fillPct}%"
      ></div>
      ${
        markPct != null &&
        html`<div
        class="ledger-bar-mark"
        style="left:${markPct}%"
      ></div>`
      }
    </div>
  `;
}

function FixedStatusIcon({ pace }) {
  const size = 16;
  const r = 6;
  const cx = size / 2;
  const cy = size / 2;
  if (pace === "pending") {
    return html`
      <svg width=${size} height=${size} viewBox="0 0 ${size} ${size}" class="fixed-status-icon">
        <circle cx=${cx} cy=${cy} r=${r}
          fill="none" stroke="var(--text-light)" stroke-width="1.5"
          stroke-dasharray="3 2.5" opacity="0.7" />
      </svg>
    `;
  }
  if (pace === "over_budget") {
    return html`
      <svg width=${size} height=${size} viewBox="0 0 ${size} ${size}" class="fixed-status-icon">
        <circle cx=${cx} cy=${cy} r=${r} fill="var(--danger)" opacity="0.85" />
        <line x1="5.5" y1="5.5" x2="10.5" y2="10.5" stroke="white" stroke-width="1.5" stroke-linecap="round" />
        <line x1="10.5" y1="5.5" x2="5.5" y2="10.5" stroke="white" stroke-width="1.5" stroke-linecap="round" />
      </svg>
    `;
  }
  return html`
    <svg width=${size} height=${size} viewBox="0 0 ${size} ${size}" class="fixed-status-icon">
      <circle cx=${cx} cy=${cy} r=${r} fill="var(--primary)" opacity="0.85" />
      <polyline points="5.5,8 7.5,10 11,5.5" fill="none" stroke="white" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  `;
}

function NetSummary({ ledger }) {
  if (!ledger) return null;
  const net = Number(ledger.net);
  const netColor = net < 0 ? "var(--danger)" : "var(--success)";
  return html`
    <div class="net-summary">
      <div class="net-summary-side">
        <span class="net-summary-label text-light">In</span>
        <span class="net-summary-value" style="color:var(--success)">${formatAmount(ledger.total_in, { decimals: 0 })}</span>
      </div>
      <div class="net-summary-center">
        <span class="net-summary-label text-light">Net</span>
        <span class="net-summary-net" style="color:${netColor}">${formatAmount(net, { decimals: 0, sign: true })}</span>
      </div>
      <div class="net-summary-side">
        <span class="net-summary-label text-light">Out</span>
        <span class="net-summary-value" style="color:var(--danger)">${formatAmount(ledger.total_out, { decimals: 0 })}</span>
      </div>
    </div>
  `;
}

function TrendArrow({ trend_monthly, budget_amount }) {
  if (trend_monthly == null || !budget_amount) return null;
  const annual = trend_monthly * 12;
  if (Math.abs(trend_monthly) < Number(budget_amount) * 0.02) return null;
  const arrow = annual > 0 ? "\u2197" : "\u2198";
  const label = `Trending ${annual > 0 ? "up" : "down"} ~${formatAmount(Math.abs(annual), { decimals: 0 })}/yr`;
  return html`<span class="trend-arrow" title=${label}>${arrow}</span>`;
}

function Ledger({
  items,
  ledger,
  selectedCategoryId,
  onCategoryClick,
  onMonthlyClick,
  catMap,
}) {
  if (!ledger) return null;
  const barMax = Number(ledger.bar_max) || 1;

  return html`
    <div class="ledger">
      <!-- IN -->
      ${
        ledger.income.length > 0 &&
        html`
        <div class="ledger-section">
          <div class="ledger-section-label text-light">In</div>
          ${ledger.income.map(
            (item) => html`
              <div
                class="ledger-income-row"
                key=${item.category_id || item.label}
                onClick=${() => onCategoryClick?.(item.category_id || "__none")}
              >
                <span class="ledger-row-name">
                  <span class="ledger-pace-dot" title=${catMap && categoryBudgetMode(catMap, item.category_id) === "salary" ? "Salary" : "Other income"} style="background:${catMap && categoryBudgetMode(catMap, item.category_id) === "salary" ? "var(--success)" : "var(--muted-foreground)"}"></span>
                  ${item.label}
                </span>
                <span class="ledger-amount">${formatAmount(item.amount, { decimals: 0 })}</span>
              </div>
            `,
          )}
          <div class="ledger-subtotal">
            <span>Total In</span>
            <span class="ledger-amount" style="color:var(--success)">${formatAmount(ledger.total_in, { decimals: 0, sign: true })}</span>
          </div>
        </div>
      `
      }

      <!-- OUT -->
      <div class="ledger-section">
        <div class="ledger-section-label text-light">Out</div>
        ${(() => {
          const variableItems = items.filter((s) => s.budgetType !== "fixed");
          const fixedItems = items.filter((s) => s.budgetType === "fixed");
          const variableWithSpend = variableItems.filter(
            (s) => Number(s.spent) !== 0,
          );
          const variableZeroSpend = variableItems.filter(
            (s) => Number(s.spent) === 0,
          );
          return html`
          ${
            variableWithSpend.length > 0 &&
            html`
            <div class="ledger-col-headers">
              <span>Name</span>
              <span></span>
              <span>Budget</span>
              <span>Spent</span>
              <span style="text-align:right">\u0394</span>
            </div>
          `
          }
          ${variableWithSpend.map(
            (s) => html`
            <div
              class="ledger-row${s.pace === "over_budget" ? " ledger-row-over" : ""}${selectedCategoryId === s.category_id ? " ledger-row-selected" : ""}"
              key=${s.category_id}
              title="${paceLabel(s.pace, s.pace_delta, s.seasonal_factor)}"
              onClick=${() => onCategoryClick?.(s.category_id)}
            >
              <span class="ledger-row-name">
                <span class="ledger-pace-dot" style="background:${paceColor(s.pace)}"></span>
                ${s.shortName}
                <${TrendArrow} trend_monthly=${s.trend_monthly} budget_amount=${s.budget_amount} />
              </span>
              <${BudgetBar}
                pace=${s.pace}
                fillPct=${barMax > 0 ? (Math.abs(Number(s.spent)) / barMax) * 100 : 0}
                markPct=${barMax > 0 ? (Number(s.budget_amount) / barMax) * 100 : 0}
              />
              <span class="ledger-amount">${formatAmount(s.budget_amount, { decimals: 0 })}</span>
              <span class="ledger-amount">${formatAmount(s.spent, { decimals: 0 })}</span>
              <span class="ledger-amount" style="color:${Number(s.remaining) < 0 ? "var(--danger)" : ""}">${formatAmount(s.remaining, { decimals: 0, sign: true })}</span>
            </div>
          `,
          )}
          ${
            variableZeroSpend.length > 0 &&
            html`
            <div class="ledger-zero-spend">
              ${variableZeroSpend.map(
                (s) => html`
                <span
                  class="ledger-zero-chip${selectedCategoryId === s.category_id ? " ledger-zero-chip-selected" : ""}"
                  key=${s.category_id}
                  title="${s.shortName}: ${formatAmount(s.budget_amount, { decimals: 0 })} budgeted, no spend"
                  onClick=${() => onCategoryClick?.(s.category_id)}
                >
                  <span class="ledger-pace-dot" style="background:${paceColor(s.pace)}"></span>${s.shortName}
                  <span class="text-light">${formatAmount(s.budget_amount, { decimals: 0 })}</span>
                </span>
              `,
              )}
            </div>
          `
          }
          ${
            fixedItems.length > 0 &&
            html`
            <div class="ledger-fixed-label text-light">Fixed</div>
            <div class="ledger-fixed-grid">
              ${fixedItems.map(
                (s) => html`
                <div
                  class="ledger-fixed-item${s.pace === "over_budget" ? " ledger-fixed-over" : ""}${s.pace === "pending" ? " ledger-fixed-pending" : ""}${selectedCategoryId === s.category_id ? " ledger-row-selected" : ""}"
                  key=${s.category_id}
                  title="${paceLabel(s.pace, s.pace_delta, s.seasonal_factor)}"
                  onClick=${() => onCategoryClick?.(s.category_id)}
                >
                  <${FixedStatusIcon} pace=${s.pace} />
                  <span class="ledger-fixed-name">${s.shortName}</span>
                  <span class="ledger-fixed-amount">${formatAmount(
                    Number(s.spent) !== 0 ? s.spent : s.budget_amount,
                    { decimals: 0 },
                  )}</span>
                </div>
              `,
              )}
            </div>
          `
          }
          `;
        })()}

        ${
          ledger.unbudgeted.length > 0 &&
          html`
          <div class="ledger-divider"></div>
          ${ledger.unbudgeted.map(
            (item) => html`
              <div
                class="ledger-unbudgeted-row${selectedCategoryId === (item.category_id || "__none") ? " ledger-row-selected" : ""}"
                key=${item.category_id || item.label}
                onClick=${() => onCategoryClick?.(item.category_id || "__none")}
              >
                <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${item.category_id && item.label !== "Uncategorized" ? html`<span title="No budget mode set — consider setting one" style="color:var(--warning);cursor:help">⚠ </span>` : ""}${item.label}</span>
                <span></span>
                <span></span>
                <span class="ledger-amount">${formatAmount(item.amount, { decimals: 0 })}</span>
                <span></span>
              </div>
            `,
          )}
        `
        }
        ${
          Number(ledger.monthly_spent) > 0 &&
          html`
          <div class="ledger-divider"></div>
          <div class="ledger-row clickable-row" onClick=${onMonthlyClick} style="color:var(--muted-foreground)">
            <span class="ledger-row-name">Monthly budgets</span>
            <span></span>
            <span class="ledger-amount">${formatAmount(ledger.monthly_budget, { decimals: 0 })}</span>
            <span class="ledger-amount">${formatAmount(ledger.monthly_spent, { decimals: 0 })}</span>
            <span class="ledger-amount" style="color:${Number(ledger.monthly_remaining) < 0 ? "var(--danger)" : ""}">${formatAmount(ledger.monthly_remaining, { decimals: 0, sign: true })}</span>
          </div>
        `
        }
        <div class="ledger-subtotal">
          <span>Total Out</span>
          <span class="ledger-amount" style="color:var(--danger)">${formatAmount(ledger.total_out, { decimals: 0 })}</span>
        </div>
      </div>

      <!-- NET -->
      <div class="ledger-net">
        <span>Net</span>
        <span class="ledger-amount" style="color:${Number(ledger.net) < 0 ? "var(--danger)" : "var(--success)"}">${formatAmount(ledger.net, { decimals: 0, sign: true })}</span>
      </div>
    </div>
  `;
}

function BudgetSection({
  items,
  summary,
  barMax,
  selectedCategoryId,
  onCategoryClick,
  showDateSubtitle,
}) {
  return html`
    <div class="proj-stat-cards">
      <article class="card proj-stat-card">
        <span class="proj-stat-label text-light">Total Budget</span>
        <span class="proj-stat-value">${formatAmount(summary.total_budget, { decimals: 0 })}</span>
      </article>
      <article class="card proj-stat-card">
        <span class="proj-stat-label text-light">Spent</span>
        <span class="proj-stat-value">${formatAmount(summary.total_spent, { decimals: 0 })}</span>
      </article>
      <article class="card proj-stat-card">
        <span class="proj-stat-label text-light">Remaining</span>
        <span
          class="proj-stat-value"
          style=${Number(summary.remaining) < 0 ? "color:var(--danger)" : ""}
        >
          ${formatAmount(summary.remaining, { decimals: 0 })}
        </span>
      </article>
      <article class="card proj-stat-card">
        <span class="proj-stat-label text-light">Categories</span>
        <span class="proj-stat-value">
          ${
            summary.over_budget_count > 0
              ? html`<span class="badge danger">${summary.over_budget_count}</span>
                  over`
              : html`All on track`
          }
        </span>
      </article>
    </div>

    <div class="vstack" style="gap:0">
      ${items
        .filter((s) => Number(s.spent) !== 0)
        .map(
          (s) => html`
          <div
            class="ledger-row${s.pace === "over_budget" ? " ledger-row-over" : ""}${selectedCategoryId === s.category_id ? " ledger-row-selected" : ""}"
            key=${s.category_id}
            title="${paceLabel(s.pace, s.pace_delta, s.seasonal_factor)}"
            onClick=${() => onCategoryClick?.(s.category_id)}
          >
            <span class="ledger-row-name">
              <span class="ledger-pace-dot" style="background:${paceColor(s.pace)}"></span>
              ${
                s.parentName
                  ? html`<span class="cat-parent">${s.parentName}</span>${s.shortName}`
                  : s.shortName
              }
              ${
                showDateSubtitle &&
                s.project_start_date &&
                html` <span class="text-light text-caption">${formatDateRange(s.project_start_date, s.project_end_date)}</span>`
              }
              <${TrendArrow} trend_monthly=${s.trend_monthly} budget_amount=${s.budget_amount} />
            </span>
            <${BudgetBar}
              pace=${s.pace}
              fillPct=${barMax > 0 ? (Math.abs(Number(s.spent)) / barMax) * 100 : 0}
              markPct=${barMax > 0 ? (Number(s.budget_amount) / barMax) * 100 : 0}
            />
            <span class="ledger-amount">${formatAmount(s.budget_amount, { decimals: 0 })}</span>
            <span class="ledger-amount">${formatAmount(s.spent, { decimals: 0 })}</span>
            <span class="ledger-amount" style="color:${Number(s.remaining) < 0 ? "var(--danger)" : ""}">${formatAmount(s.remaining, { decimals: 0, sign: true })}</span>
          </div>
        `,
        )}

      ${
        items.filter((s) => Number(s.spent) === 0).length > 0 &&
        html`
        <div class="ledger-zero-spend">
          ${items
            .filter((s) => Number(s.spent) === 0)
            .map(
              (s) => html`
              <span
                class="ledger-zero-chip${selectedCategoryId === s.category_id ? " ledger-zero-chip-selected" : ""}"
                key=${s.category_id}
                title="${s.shortName}: ${formatAmount(s.budget_amount, { decimals: 0 })} budgeted, no spend"
                onClick=${() => onCategoryClick?.(s.category_id)}
              >
                <span class="ledger-pace-dot" style="background:${paceColor(s.pace)}"></span>${s.shortName}
                <span class="text-light">${formatAmount(s.budget_amount, { decimals: 0 })}</span>
              </span>
            `,
            )}
        </div>
      `
      }
    </div>
  `;
}

function guessMonthName(month) {
  const start = new Date(`${month.start_date}T00:00:00`);
  const end = month.end_date ? new Date(`${month.end_date}T00:00:00`) : null;
  const mid = end ? new Date((start.getTime() + end.getTime()) / 2) : start;
  return mid.toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

function formatMonthRange(month) {
  const fmt = (d) => {
    const date = new Date(`${d}T00:00:00`);
    return date.toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
    });
  };
  const start = fmt(month.start_date);
  const end = month.end_date ? fmt(month.end_date) : "now";
  return `${start} \u2013 ${end}`;
}

function ProjectDrillDown({
  project,
  childBreakdown,
  totalSpent,
  selectedCategoryId,
  onCategoryClick,
  onBack,
}) {
  const remaining = Number(project.remaining);

  return html`
    <nav
      aria-label="Breadcrumb"
      class="hstack gap-2"
      style="align-items:center;margin-bottom:0.75rem;cursor:pointer"
      onClick=${onBack}
    >
      <span class="text-display">\u2190</span>
      <span class="text-light">All Projects</span>
      <span class="text-light">\u203A</span>
      <span class="cat-project" style="font-weight:600" aria-current="page">${project.name}</span>
    </nav>

    <div class="proj-stat-cards">
      <article class="card proj-stat-card">
        <span class="proj-stat-label text-light">Project Budget</span>
        <span class="proj-stat-value">${formatAmount(project.budget_amount, { decimals: 0 })}</span>
      </article>
      <article class="card proj-stat-card">
        <span class="proj-stat-label text-light">Spent</span>
        <span class="proj-stat-value">${formatAmount(totalSpent, { decimals: 0 })}</span>
      </article>
      <article class="card proj-stat-card">
        <span class="proj-stat-label text-light">Remaining</span>
        <span
          class="proj-stat-value"
          style=${remaining < 0 ? "color:var(--danger)" : ""}
        >
          ${formatAmount(remaining, { decimals: 0 })}
        </span>
      </article>
      <article class="card proj-stat-card">
        <span class="proj-stat-label text-light">Status</span>
        <span class="proj-stat-value">
          <span class="badge small ${paceBadge(project.pace)}">${paceLabel(project.pace, project.pace_delta, null)}</span>
        </span>
      </article>
    </div>

    ${
      childBreakdown.length === 0
        ? html`<p class="text-light mt-4">No spending yet.</p>`
        : html`
    <div class="proj-grid">
      <article class="card" style="padding:var(--space-4)">
        <h3 style="margin:0 0 0.75rem">Spending Distribution</h3>
        <div class="vstack gap-2">
          ${childBreakdown.map(
            (item) => html`
              <div
                class="ledger-row${selectedCategoryId === item.id ? " ledger-row-selected" : ""}"
                style="grid-template-columns:7rem 1fr auto"
                key=${item.id}
                onClick=${() => onCategoryClick?.(item.id)}
              >
                <span class="ledger-row-name">${item.name}</span>
                <${BudgetBar}
                  pace=${project.pace}
                  fillPct=${totalSpent > 0 ? (item.spent / totalSpent) * 100 : 0}
                />
                <span class="ledger-amount">${formatAmount(item.spent, { decimals: 0 })}</span>
              </div>
            `,
          )}
        </div>
      </article>

      <article class="card" style="padding:var(--space-4)">
        <h3 style="margin:0 0 0.75rem">Sub-Category Breakdown</h3>
        <div class="vstack" style="gap:0">
          ${childBreakdown.map(
            (item) => html`
              <div
                class="hstack clickable-row clickable-item${selectedCategoryId === item.id ? " ledger-row-selected" : ""}"
                key=${item.id}
                onClick=${() => onCategoryClick?.(item.id)}
              >
                <div class="proj-pct-circle">
                  ${totalSpent > 0 ? Math.round((item.spent / totalSpent) * 100) : 0}%
                </div>
                <div style="flex:1;min-width:0">
                  <div class="proj-item-name">${item.name}</div>
                  <div class="text-caption">
                    <span>${formatAmount(item.spent, { decimals: 0 })}</span>
                    <span class="text-light">
                      ${" "}of ${formatAmount(totalSpent, { decimals: 0 })} total</span>
                  </div>
                </div>
              </div>
            `,
          )}
        </div>
      </article>
    </div>
    `
    }
  `;
}

const TAB_NAMES = ["monthly", "annual", "projects"];

function Dashboard({ tab = "monthly", monthId = null }) {
  const [statusResp, setStatusResp] = useState(null);
  const [categories, setCategories] = useState(null);
  const [months, setMonths] = useState(null);
  const [netWorth, setNetWorth] = useState(null);
  const [error, setError] = useState(null);

  const selectedMonthId = monthId;
  function setSelectedMonthId(id) {
    navigateReplace(`/monthly/${id}`);
  }

  const statusUrl = selectedMonthId
    ? `/budgets/status?month_id=${selectedMonthId}`
    : "/budgets/status";

  const load = useCallback(() => {
    Promise.all([
      api.get(statusUrl),
      api.get("/categories"),
      api.get("/budgets/months"),
      api.get("/accounts/net-worth"),
    ])
      .then(([s, c, m, nw]) => {
        setStatusResp(s);
        setCategories(c);
        setMonths(m);
        setNetWorth(nw);
      })
      .catch(setError);
  }, []);

  useEffect(() => {
    load();
  }, []);

  // Re-fetch status when month changes via URL
  const prevMonthRef = useRef(selectedMonthId);
  useEffect(() => {
    if (selectedMonthId === prevMonthRef.current) return;
    prevMonthRef.current = selectedMonthId;
    if (selectedMonthId === null) return;
    api.get(statusUrl).then(setStatusResp).catch(setError);
  }, [statusUrl]);

  if (error)
    return html`<${ErrorPanel} error=${error} onRetry=${() => {
      setError(null);
      load();
    }} />`;
  if (!statusResp || !categories || !months)
    return html`<p class="text-light">Loading...</p>`;

  const catMap = Object.fromEntries(categories.map((c) => [c.id, c]));

  // The month we're viewing comes from the status response
  const activeMonth = statusResp.month;
  const isCurrentMonth = !activeMonth.end_date;
  const status = statusResp.statuses;

  // Sort months chronologically for navigation
  const sortedMonths = [...months].sort((a, b) =>
    a.start_date.localeCompare(b.start_date),
  );
  const activeIdx = sortedMonths.findIndex((m) => m.id === activeMonth.id);
  const hasPrev = activeIdx > 0;
  const hasNext = activeIdx < sortedMonths.length - 1;

  function goPrev() {
    if (!hasPrev) return;
    const prev = sortedMonths[activeIdx - 1];
    setSelectedMonthId(prev.id);
  }

  function goNext() {
    if (!hasNext) return;
    const next = sortedMonths[activeIdx + 1];
    setSelectedMonthId(next.id);
  }

  // Enrich status with display names and hierarchy info
  const enrichStatus = (s) => {
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
      budgetType: categoryBudgetType(catMap, s.category_id),
    };
  };

  const paceOrdinal = (pace) => {
    switch (pace) {
      case "over_budget":
        return 0;
      case "above_pace":
        return 1;
      case "on_track":
        return 2;
      case "under_budget":
        return 3;
      default:
        return 4;
    }
  };
  const byUrgency = (a, b) =>
    paceOrdinal(a.pace) - paceOrdinal(b.pace) ||
    Number(b.spent) - Number(a.spent);

  const enriched = status.map(enrichStatus).sort(byUrgency);

  // Split by budget mode
  const monthly = enriched.filter(
    (s) => s.budgetMode === "monthly" || !s.budgetMode,
  );
  const annual = enriched.filter((s) => s.budgetMode === "annual");
  const allProjects = (statusResp.projects || [])
    .map(enrichStatus)
    .sort(byUrgency);
  const activeProjects = allProjects.filter((s) => !s.finished);
  const finishedProjects = allProjects.filter((s) => s.finished);

  // Transaction lists and summaries from backend
  const monthBudgetTxns = statusResp.monthly_transactions;
  const annualBudgetTxns = statusResp.annual_transactions;
  const projectBudgetTxns = statusResp.project_transactions;
  const budgetYear = statusResp.budget_year;

  const monthlyLedger = statusResp.monthly_ledger;
  const annualLedger = statusResp.annual_ledger;
  // Collect all non-budget transactions from ledger income + unbudgeted sections
  const monthlyCashflowTxns = [
    ...(monthlyLedger?.income || []).flatMap((i) => i.transactions || []),
    ...(monthlyLedger?.unbudgeted || []).flatMap((i) => i.transactions || []),
  ];
  const annualCashflowTxns = [
    ...(annualLedger?.income || []).flatMap((i) => i.transactions || []),
    ...(annualLedger?.unbudgeted || []).flatMap((i) => i.transactions || []),
  ];

  // Time left label per mode
  const timeLeft = (items, unit) => {
    const entry = items[0];
    if (!entry) return "";
    if (entry.time_left === null || entry.time_left === undefined)
      return "open-ended";
    return `${entry.time_left}${unit} left`;
  };
  const monthlyTimeLabel = timeLeft(monthly, "d");

  const hasProjects = allProjects.length > 0;

  // Category filter for transaction list (click a category in charts to filter)
  const [selectedCategoryId, setSelectedCategoryId] = useState(null);
  // Drill into a specific project to see sub-category breakdown
  const [drilledProjectId, setDrilledProjectId] = useState(null);
  const handleCategoryClick = useCallback((catId) => {
    setSelectedCategoryId((prev) => (prev === catId ? null : catId));
  }, []);

  // Handle click on a project row: drill in if it has children, otherwise filter transactions
  const handleProjectClick = useCallback(
    (catId) => {
      const proj = (statusResp.projects || []).find(
        (p) => p.category_id === catId,
      );
      if (proj?.has_children) {
        setDrilledProjectId(catId);
        setSelectedCategoryId(null);
      } else {
        setSelectedCategoryId((prev) => (prev === catId ? null : catId));
      }
    },
    [statusResp],
  );

  // Project drill-down: use backend-provided child breakdowns
  const drilledProject = drilledProjectId
    ? allProjects.find((p) => p.category_id === drilledProjectId)
    : null;

  const { childBreakdown, drilledTotalSpent } = useMemo(() => {
    if (!drilledProjectId || !drilledProject) {
      return { childBreakdown: [], drilledTotalSpent: 0 };
    }
    // Backend provides children with {category_id, category_name, spent}
    const backendProject = (statusResp.projects || []).find(
      (p) => p.category_id === drilledProjectId,
    );
    const rows = (backendProject?.children || []).map((c) => ({
      id: c.category_id,
      name: c.category_name,
      spent: Number(c.spent),
    }));
    const total = rows.reduce((sum, r) => sum + r.spent, 0);
    return { childBreakdown: rows, drilledTotalSpent: total };
  }, [drilledProjectId, drilledProject, statusResp]);

  // Tab derived from route prop
  const activeTab = Math.max(0, TAB_NAMES.indexOf(tab));
  const syncedTabRef = useRef(-1);
  function setActiveTab(idx) {
    syncedTabRef.current = idx;
    setSelectedCategoryId(null);
    setDrilledProjectId(null);
    location.hash = `#/${TAB_NAMES[idx]}`;
  }

  const tabsRef = useRef(null);
  const tabsCallbackRef = useCallback((el) => {
    if (tabsRef.current === el) return;
    tabsRef.current = el;
    if (!el) return;
    el.addEventListener("click", (e) => {
      if (!e.isTrusted) return;
      const tab = e.target.closest("[role=tab]");
      if (!tab) return;
      const tabs = [...el.querySelectorAll("[role=tab]")];
      const idx = tabs.indexOf(tab);
      if (idx >= 0) setActiveTab(idx);
    });
  }, []);

  // Sync ot-tabs web component when URL drives a tab change (initial load + back/forward)
  useEffect(() => {
    if (activeTab === syncedTabRef.current) return;
    syncedTabRef.current = activeTab;
    const el = tabsRef.current;
    if (!el) return;
    const tabs = [...el.querySelectorAll("[role=tab]")];
    if (tabs[activeTab]) tabs[activeTab].click();
  });

  const annualTimeLabel = timeLeft(annual, "mo");

  // Pick the right base list depending on which tab is active
  const baseTxns =
    activeTab === 2
      ? projectBudgetTxns
      : activeTab === 1
        ? [...annualBudgetTxns, ...annualCashflowTxns]
        : [...monthBudgetTxns, ...monthlyCashflowTxns];

  // Collect category subtree IDs (UI-only: narrows the already-filtered
  // backend transaction lists when the user clicks a category bar)
  const collectSubtree = (rootId) => {
    const ids = new Set([rootId]);
    const stack = [rootId];
    while (stack.length > 0) {
      const current = stack.pop();
      for (const c of categories) {
        if (c.parent_id === current && !ids.has(c.id)) {
          ids.add(c.id);
          stack.push(c.id);
        }
      }
    }
    return ids;
  };

  // Apply optional category filter from chart clicks
  const displayTxns = (() => {
    let list = baseTxns;
    if (drilledProjectId) {
      const subtree = collectSubtree(drilledProjectId);
      list = list.filter((t) => t.category_id && subtree.has(t.category_id));
    }
    if (selectedCategoryId) {
      if (selectedCategoryId === "__none") {
        list = list.filter((t) => !t.category_id);
      } else {
        const subtree = collectSubtree(selectedCategoryId);
        list = list.filter((t) => t.category_id && subtree.has(t.category_id));
      }
    }
    return [...list].sort((a, b) => b.posted_date.localeCompare(a.posted_date));
  })();

  return html`
    <ot-tabs ref=${tabsCallbackRef}>
      <div role="tablist">
        <button role="tab">Monthly</button>
        <button role="tab">Annual</button>
        ${hasProjects && html`<button role="tab">Projects</button>`}
      </div>
      <div role="tabpanel">
        <div class="hstack" style="margin-bottom:1.25rem">
          <div class="hstack gap-2" style="align-items:center">
            <button
              onClick=${goPrev}
              disabled=${!hasPrev}
              style="padding:0.25rem 0.5rem"
              aria-label="Previous month"
            >\u2039</button>
            <div style="text-align:center">
              <strong>${guessMonthName(activeMonth)}</strong>
              <div class="text-light text-body">${formatMonthRange(activeMonth)}</div>
              ${
                isCurrentMonth
                  ? html`<div class="text-light mono text-body">${monthlyTimeLabel}</div>`
                  : html`<div class="text-light text-body">Closed</div>`
              }
            </div>
            <button
              onClick=${goNext}
              disabled=${!hasNext}
              style="padding:0.25rem 0.5rem"
              aria-label="Next month"
            >\u203A</button>
          </div>
        </div>
        <${NetSummary} ledger=${monthlyLedger} />
        ${
          monthlyLedger
            ? html`<${Ledger}
              items=${monthly}
              ledger=${monthlyLedger}
              selectedCategoryId=${selectedCategoryId}
              onCategoryClick=${handleCategoryClick}
              catMap=${catMap}
            />`
            : html`<p class="text-light">No monthly budgets.</p>`
        }
      </div>
      <div role="tabpanel">
        <div class="hstack" style="margin-bottom:1.25rem;align-items:center">
          <div style="text-align:center">
            <strong>${budgetYear}</strong>
            ${annualTimeLabel && html`<div class="text-light mono text-body">${annualTimeLabel}</div>`}
          </div>
        </div>
        <${NetSummary} ledger=${annualLedger} />
        ${
          annualLedger
            ? html`<${Ledger}
              items=${annual}
              ledger=${annualLedger}
              selectedCategoryId=${selectedCategoryId}
              onCategoryClick=${handleCategoryClick}
              onMonthlyClick=${() => setActiveTab(0)}
              catMap=${catMap}
            />`
            : html`<p class="text-light">No annual budgets.</p>`
        }
      </div>
      ${
        hasProjects &&
        html`
        <div role="tabpanel">
          ${
            drilledProject
              ? html`<${ProjectDrillDown}
                project=${drilledProject}
                childBreakdown=${childBreakdown}
                totalSpent=${drilledTotalSpent}
                selectedCategoryId=${selectedCategoryId}
                onCategoryClick=${handleCategoryClick}
                onBack=${() => {
                  setDrilledProjectId(null);
                  setSelectedCategoryId(null);
                }}
              />`
              : html`<${BudgetSection}
                items=${activeProjects}
                summary=${statusResp.project_summary}
                barMax=${Number(statusResp.project_summary.bar_max)}
                selectedCategoryId=${selectedCategoryId}
                onCategoryClick=${handleProjectClick}
                showDateSubtitle
              />
              ${
                finishedProjects.length > 0 &&
                html`
                <details class="mt-4">
                  <summary class="text-light" style="cursor:pointer;padding:0.5rem 0">Finished (${finishedProjects.length})</summary>
                  <div class="vstack" style="gap:0;margin-top:0.5rem;opacity:0.5">
                    ${finishedProjects.map(
                      (s) => html`
                        <div
                          class="hstack clickable-row clickable-item${selectedCategoryId === s.category_id ? " ledger-row-selected" : ""}"
                          key=${s.category_id}
                          title="${paceLabel(s.pace, s.pace_delta, s.seasonal_factor)}"
                          onClick=${() => handleProjectClick?.(s.category_id)}
                        >
                          <span class="ledger-pace-dot" style="background:${paceColor(s.pace)};flex-shrink:0"></span>
                          <div style="flex:1;min-width:0">
                            <div class="proj-item-name">
                              ${s.parentName && html`<span class="cat-parent">${s.parentName}</span>`}${s.shortName}
                            </div>
                            ${
                              s.project_start_date &&
                              html`
                              <div class="text-light text-caption">${formatDateRange(s.project_start_date, s.project_end_date)}</div>
                            `
                            }
                            <div class="text-caption">
                              <span>${formatAmount(s.spent, { decimals: 0 })}</span>
                              <span class="text-light">
                                ${" "}/ ${Number(s.budget_amount) > 0 ? formatAmount(s.budget_amount, { decimals: 0 }) : "no budget"}</span>
                            </div>
                          </div>
                          <div class="vstack" style="align-items:flex-end;gap:0.15rem;white-space:nowrap">
                            <span class="badge small ${paceBadge(s.pace)}">${paceLabel(s.pace, s.pace_delta, s.seasonal_factor)}</span>
                            <span class="text-body" style="${Number(s.remaining) < 0 ? "color:var(--danger)" : ""}">
                              ${formatAmount(s.remaining, { decimals: 0, sign: true })}
                            </span>
                          </div>
                        </div>
                      `,
                    )}
                  </div>
                </details>
              `
              }`
          }
        </div>
      `
      }
    </ot-tabs>

    ${
      netWorth &&
      netWorth.accounts.length > 0 &&
      html`
      <a href="#/balances" class="card" style="display:block;padding:var(--space-4);margin-top:1rem;text-decoration:none;color:inherit">
        <div class="hstack" style="align-items:baseline;margin-bottom:0.5rem">
          <h3 style="margin:0">Net Worth</h3>
          ${(() => {
            const stale = netWorth.accounts.filter(
              (a) =>
                a.is_manual &&
                Date.now() - new Date(a.snapshot_at).getTime() > 7 * 86400_000,
            ).length;
            return stale > 0
              ? html`<span class="badge warning small" style="margin-left:0.5rem">${stale} stale</span>`
              : "";
          })()}
          <span class="text-light text-body" style="margin-left:auto">\u203A</span>
        </div>
        <span style="font-size:var(--text-2);font-weight:700;color:${Number(netWorth.total) >= 0 ? "var(--success)" : "var(--danger)"}">
          ${formatAmount(netWorth.total, { decimals: 0 })}
        </span>
        <div class="hstack gap-3" style="margin-top:0.75rem;flex-wrap:wrap">
          ${netWorth.accounts.map(
            (a) => html`
            <div key=${a.account_id} style="min-width:0">
              <div class="text-light text-caption" style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${a.account_name}</div>
              <div class="mono text-body" style="color:${Number(a.balance) >= 0 ? "var(--success)" : "var(--danger)"}">
                ${formatAmount(a.balance, { decimals: 0 })}
              </div>
            </div>
          `,
          )}
        </div>
      </a>
    `
    }

    <article class="card" style="padding:var(--space-4);margin-top:1rem">
      <div
        class="hstack"
        style="align-items:baseline;margin-bottom:0.75rem"
      >
        <h3 style="margin:0">Transactions</h3>
        ${
          drilledProject &&
          html`
          <span
            class="chip outline small cat-project"
            style="margin-left:0.5rem"
          >
            ${drilledProject.shortName}
          </span>
        `
        }
        ${
          selectedCategoryId &&
          html`
          <button
            class="chip outline small${selectedCategoryId !== "__none" ? ` ${budgetModeColor(categoryBudgetMode(catMap, selectedCategoryId))}` : ""}"
            style="margin-left:0.5rem"
            onClick=${() => setSelectedCategoryId(null)}
          >
            ${selectedCategoryId === "__none" ? "Uncategorized" : categoryName(catMap, selectedCategoryId)} \u00d7
          </button>
        `
        }
        <span class="text-light text-body" style="margin-left:auto">
          ${displayTxns.length} transaction${displayTxns.length !== 1 ? "s" : ""}
        </span>
      </div>
      <${TransactionTable}
        transactions=${displayTxns}
        categories=${categories}
        catMap=${catMap}
        onTransactionUpdate=${(txnId, patch) => {
          const updateList = (txns) =>
            txns.map((t) => (t.id === txnId ? { ...t, ...patch } : t));
          setStatusResp((prev) => ({
            ...prev,
            monthly_transactions: updateList(prev.monthly_transactions),
            annual_transactions: updateList(prev.annual_transactions),
            project_transactions: updateList(prev.project_transactions),
          }));
        }}
        onRuleCreated=${() => waitForJobsDrained().then(load)}
        compact=${true}
      />
    </article>
  `;
}

// ---------------------------------------------------------------------------
// Transactions
// ---------------------------------------------------------------------------

function CategoryBadge({ catMap, id, suggested }) {
  const label = categoryLabel(catMap, id);
  if (label) {
    const cls = budgetModeColor(categoryBudgetMode(catMap, id));
    if (label.parent) {
      return html`<span class=${cls} title="${label.parent} > ${label.short}">
        <span class="cat-parent">${label.parent}</span>${label.short}
      </span>`;
    }
    return html`<span class=${cls}>${label.short}</span>`;
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
  const [proposalPreview, setProposalPreview] = useState(null);
  const [proposalPreviewing, setProposalPreviewing] = useState(false);
  const [amazonEnrichment, setAmazonEnrichment] = useState(null);
  const debounceRef = useRef(null);
  if (!txn) return null;

  const [open, setOpen] = useState(false);
  const prevTxnId = useRef(null);

  useEffect(() => {
    if (txn && txn.id !== prevTxnId.current) {
      prevTxnId.current = txn.id;
      setRuleProposals(null);
      setSelectedProposal(null);
      setProposalPreview(null);
      setAmazonEnrichment(null);
      // Small delay so the panel animates in
      requestAnimationFrame(() => setOpen(true));
      api
        .get(`/amazon/enrichment/${txn.id}`)
        .then(setAmazonEnrichment)
        .catch(() => setAmazonEnrichment(null));
    }
  }, [txn?.id]);

  function closePanel() {
    setOpen(false);
    prevTxnId.current = null;
    setTimeout(() => {
      setRuleProposals(null);
      setSelectedProposal(null);
      setProposalPreview(null);
      setAmazonEnrichment(null);
      onClose();
    }, 250);
  }

  useEffect(() => {
    if (!open) return;
    function onKey(e) {
      if (e.key === "Escape") closePanel();
    }
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open]);
  const remittanceSegments = formatRemittanceInfo(txn.remittance_information);

  const canGenerateRule = txn.category_id && txn.category_method !== "rule";

  async function handleCategorize(categoryId) {
    if (categoryId === txn.category_id) return;
    if (!categoryId) return handleUncategorize();
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

  async function handleUncategorize() {
    setSaving(true);
    try {
      await api.del(`/transactions/${txn.id}/categorize`);
      onCategorize(txn.id, null);

      for (let i = 0; i < 10; i++) {
        await new Promise((r) => setTimeout(r, 500));
        const updated = await api.get(`/transactions/${txn.id}`);
        if (updated.category_id) {
          onCategorize(txn.id, updated.category_id, updated.category_method);
          break;
        }
      }
    } finally {
      setSaving(false);
    }
  }

  async function handleGenerateRule() {
    setGenerating(true);
    setRuleProposals(null);
    setSelectedProposal(null);
    setProposalPreview(null);
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
      setProposalPreview(null);
    } else {
      setSelectedProposal(idx);
      const pattern = ruleProposals.proposals[idx].match_pattern ?? "";
      setEditPattern(pattern);
      fetchProposalPreview(ruleProposals.proposals[idx].match_field, pattern);
    }
  }

  async function fetchProposalPreview(field, pattern) {
    setProposalPreviewing(true);
    setProposalPreview(null);
    try {
      const result = await api.post("/rules/preview", {
        rule_type: "categorization",
        conditions: [{ field, pattern }],
        target_category_id: ruleProposals.target_category_id,
        target_correlation_type: null,
        priority: 0,
        include_transaction_id: txn.id,
      });
      setProposalPreview(result);
    } catch {
      setProposalPreview(null);
    } finally {
      setProposalPreviewing(false);
    }
  }

  async function handleAcceptRule() {
    if (selectedProposal == null || !ruleProposals) return;
    const proposal = ruleProposals.proposals[selectedProposal];
    setCreatingRule(true);
    try {
      await api.post("/rules", {
        rule_type: "categorization",
        conditions: [{ field: proposal.match_field, pattern: editPattern }],
        target_category_id: ruleProposals.target_category_id,
        target_correlation_type: null,
        priority: 0,
      });
      if (proposalPreview) {
        ot.toast(
          `Rule created — matches ${proposalPreview.match_count} transaction${proposalPreview.match_count !== 1 ? "s" : ""}`,
          "",
          { variant: "success" },
        );
      } else {
        ot.toast("Rule created", "", { variant: "success" });
      }
      setRuleProposals(null);
      setSelectedProposal(null);
      if (onRuleCreated) onRuleCreated();
    } finally {
      setCreatingRule(false);
    }
  }

  return html`
    <div class="txn-panel-backdrop${open ? " open" : ""}" onClick=${closePanel}></div>
    <aside class="txn-panel${open ? " open" : ""}" role="complementary" aria-label="Transaction detail">
      <div class="txn-panel-header">
        <h3>${transactionTitle(txn)}</h3>
        <button class="ghost small" onClick=${closePanel} aria-label="Close">\u2715</button>
      </div>
      <div class="txn-panel-body">
        <dl class="txn-dl">
          <dt>Date</dt><dd>${formatDateFull(txn.posted_date)}</dd>
          <dt>Amount</dt><dd class="${amountClass(txn.amount)}">${formatAmount(txn.amount, { sign: true })}</dd>
          ${
            txn.original_amount
              ? html`
            <dt>Original</dt><dd>${txn.original_amount} ${txn.original_currency}</dd>
          `
              : null
          }
          <dt>Category</dt>
          <dd>
            <${CategorySelect}
              value=${txn.category_id ?? ""}
              onChange=${handleCategorize}
              categories=${categories}
              catMap=${catMap}
              placeholder="uncategorized"
              disabled=${saving}
              clearable
            />
            ${
              txn.category_id && txn.category_method
                ? html`<span class="chip outline small" style="margin-left:0.5rem">${{ manual: "Manual", rule: "Rule", llm: "LLM" }[txn.category_method] ?? txn.category_method}</span>`
                : null
            }
            ${
              txn.category_id
                ? html`<button type="button" class="small" style="margin-left:0.5rem" onClick=${handleUncategorize} disabled=${saving}>Clear & recategorize</button>`
                : null
            }
            ${
              !txn.category_id && txn.suggested_category
                ? html`<span class="llm-suggestion" style="margin-left:0.5rem" title="LLM suggestion"><span class="llm-suggestion-icon">✦</span> ${txn.suggested_category}</span>`
                : null
            }
          </dd>
          ${
            txn.llm_justification
              ? html`<dt></dt><dd class="text-light text-body" style="font-style:italic">✦ ${txn.llm_justification}</dd>`
              : null
          }
          <dt>Account</dt><dd>${accountDisplayName(acctMap[txn.account_id]) || txn.account_id}</dd>
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
            remittanceSegments?.length
              ? remittanceSegments.map((seg) => {
                  const colon = seg.indexOf(": ");
                  if (colon > 0 && colon < 40) {
                    return html`<dt>${seg.slice(0, colon)}</dt><dd>${seg.slice(colon + 2)}</dd>`;
                  }
                  return html`<dt>Remittance</dt><dd>${seg}</dd>`;
                })
              : null
          }
          ${
            txn.counterparty_name
              ? html`
            <dt>Counterparty</dt><dd>${txn.counterparty_name}</dd>
          `
              : null
          }
          ${
            txn.counterparty_iban
              ? html`
            <dt>IBAN</dt><dd><code>${txn.counterparty_iban}</code></dd>
          `
              : null
          }
          ${
            txn.counterparty_bic
              ? html`
            <dt>BIC</dt><dd><code>${txn.counterparty_bic}</code></dd>
          `
              : null
          }
          ${
            txn.bank_transaction_code
              ? html`
            <dt>Bank code</dt><dd>${txn.bank_transaction_code}</dd>
          `
              : null
          }
          ${
            txn.merchant_category_code
              ? html`
            <dt>MCC</dt><dd><code>${txn.merchant_category_code}</code></dd>
          `
              : null
          }
          ${
            txn.bank_transaction_code_code
              ? html`
            <dt>ISO 20022</dt><dd><code>${txn.bank_transaction_code_code}${txn.bank_transaction_code_sub_code ? `-${txn.bank_transaction_code_sub_code}` : ""}</code></dd>
          `
              : null
          }
          ${
            txn.reference_number
              ? html`
            <dt>Reference</dt><dd><code>${txn.reference_number}</code></dd>
          `
              : null
          }
          ${
            txn.note
              ? html`
            <dt>Note</dt><dd>${txn.note}</dd>
          `
              : null
          }
          ${
            txn.exchange_rate
              ? html`
            <dt>FX rate</dt><dd>${txn.exchange_rate}${txn.exchange_rate_unit_currency ? ` ${txn.exchange_rate_unit_currency}` : ""}${txn.exchange_rate_type ? ` (${txn.exchange_rate_type})` : ""}</dd>
          `
              : null
          }
          ${
            txn.exchange_rate_contract_id
              ? html`
            <dt>FX contract</dt><dd><code>${txn.exchange_rate_contract_id}</code></dd>
          `
              : null
          }
          ${
            txn.balance_after_transaction != null
              ? html`
            <dt>Balance after</dt><dd>${formatAmount(txn.balance_after_transaction)}${txn.balance_after_transaction_currency ? ` ${txn.balance_after_transaction_currency}` : ""}</dd>
          `
              : null
          }
        </dl>

        ${
          amazonEnrichment &&
          html`
            <div class="mt-4">
              <h4 style="margin:0 0 0.5rem">Amazon Order Details</h4>
              ${amazonEnrichment.orders.map(
                (order) => html`
                  <div class="card" style="margin-top:0.5rem;padding:0.75rem">
                    <div class="hstack" style="justify-content:space-between;margin-bottom:0.5rem">
                      <code class="small">${order.order_id}</code>
                      ${order.order_date ? html`<span class="text-light small">${formatDate(order.order_date)}</span>` : null}
                    </div>
                    ${order.items.map(
                      (item) => html`
                        <div class="hstack gap-2" style="padding:0.25rem 0;align-items:start">
                          <span style="flex:1">${item.quantity > 1 ? `${item.quantity}\u00d7 ` : ""}${item.title}</span>
                          ${item.price != null ? html`<span class="mono">${formatAmount(item.price)}</span>` : null}
                        </div>
                      `,
                    )}
                    ${
                      order.grand_total != null
                        ? html`
                        <div class="hstack" style="justify-content:space-between;border-top:1px solid var(--border);padding-top:0.5rem;margin-top:0.5rem">
                          <span class="text-light">Total</span>
                          <span class="mono" style="font-weight:600">${formatAmount(order.grand_total)}</span>
                        </div>
                      `
                        : null
                    }
                  </div>
                `,
              )}
            </div>
          `
        }

        ${
          ruleProposals &&
          html`
            <div class="mt-4">
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
                    <div class="hstack gap-2" style="align-items:center">
                      <span class="chip outline text-caption">${p.match_field.replace(/_/g, " ")}</span>
                      <code class="text-body">${p.match_pattern}</code>
                    </div>
                    <p class="text-light text-body" style="margin:0.25rem 0 0">${p.explanation}</p>
                    ${
                      selectedProposal === idx &&
                      html`
                        <div style="margin-top:0.5rem" onClick=${(e) => e.stopPropagation()}>
                          <input
                            type="text"
                            value=${editPattern}
                            onInput=${(e) => {
                              const val = e.target.value;
                              setEditPattern(val);
                              clearTimeout(debounceRef.current);
                              debounceRef.current = setTimeout(() => {
                                fetchProposalPreview(
                                  ruleProposals.proposals[selectedProposal]
                                    .match_field,
                                  val,
                                );
                              }, 400);
                            }}
                            style="width:100%;margin-bottom:0.5rem;font-family:monospace"
                          />
                          <div class="hstack gap-sm" style="align-items:center">
                            <button
                              type="button"
                              data-variant="primary"
                              class="small"
                              onClick=${handleAcceptRule}
                              disabled=${creatingRule}
                            >
                              ${creatingRule ? "Creating..." : "Create Rule"}
                            </button>
                            ${proposalPreviewing && html`<span class="text-light text-body">Checking...</span>`}
                            ${
                              proposalPreview &&
                              html`
                              <span class="text-light text-body">
                                Matches <strong>${proposalPreview.match_count}</strong> transaction${proposalPreview.match_count !== 1 ? "s" : ""}${proposalPreview.sample.length > 0 ? ` — ${proposalPreview.sample.map((s) => s.merchant_name).join(", ")}` : ""}
                              </span>
                            `
                            }
                          </div>
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
      <div class="txn-panel-footer">
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
        <button class="outline" onClick=${closePanel}>Close</button>
      </div>
    </aside>
  `;
}

function TransactionTable({
  transactions,
  categories,
  catMap,
  accounts,
  onTransactionUpdate,
  onRuleCreated,
  compact,
}) {
  const [selected, setSelected] = useState(null);
  const [sortCol, setSortCol] = useState("date");
  const [sortAsc, setSortAsc] = useState(false);

  const acctMap = accounts
    ? Object.fromEntries(accounts.map((a) => [a.id, a]))
    : {};
  const showAccounts = !!accounts;

  const sorted = [...transactions].sort((a, b) => {
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
        cmp = accountDisplayName(acctMap[a.account_id]).localeCompare(
          accountDisplayName(acctMap[b.account_id]),
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

  function handleCategorize(txnId, categoryId, method) {
    const patch = categoryId
      ? {
          category_id: categoryId,
          category_method: method || "manual",
          suggested_category: null,
        }
      : { category_id: null, category_method: null };
    onTransactionUpdate(txnId, patch);
    setSelected((prev) =>
      prev && prev.id === txnId ? { ...prev, ...patch } : prev,
    );
  }

  return html`
    <div class="${compact ? "table dash-txn-table" : "table txn-table"}" style="${compact ? "max-height:24rem;overflow-y:auto" : ""}">
      <table>
        <thead>
          <tr>
            <${SortTh} col="date">Date<//>
            <${SortTh} col="merchant">Merchant<//>
            <${SortTh} col="amount">Amount<//>
            <${SortTh} col="category">Category<//>
            ${showAccounts && html`<${SortTh} col="account">Account<//>`}
          </tr>
        </thead>
        <tbody>
          ${sorted.map(
            (t) => html`
              <tr
                class="clickable-row ${t.correlation_type ? "row-correlated" : ""}"
                onClick=${() => setSelected(t)}
              >
                <td class="mono${compact ? " text-light" : ""}" style="${compact ? "width:7rem" : ""}">${formatDate(t.posted_date)}</td>
                <td style="font-weight:500">${transactionTitle(t)}</td>
                <td class="${amountClass(t.amount)}" style="${compact ? "text-align:right" : ""}">${formatAmount(t.amount, compact ? { decimals: 0, sign: true } : { sign: true })}</td>
                <td>
                  ${!compact && html`<${MethodDot} method=${t.category_method} />`}
                  <${CategoryBadge} catMap=${catMap} id=${t.category_id} suggested=${t.suggested_category} />
                  ${
                    !compact && t.correlation_type
                      ? html`<span class="chip outline small">${t.correlation_type}</span>`
                      : null
                  }
                </td>
                ${showAccounts && html`<td class="text-light">${accountDisplayName(acctMap[t.account_id])}</td>`}
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
      onCategorize=${handleCategorize}
      onClose=${() => setSelected(null)}
      onRuleCreated=${onRuleCreated}
    />
  `;
}

const TXN_PAGE_SIZE = 50;

function Transactions() {
  const [pageData, setPageData] = useState(null);
  const [categories, setCategories] = useState(null);
  const [accounts, setAccounts] = useState(null);
  const [error, setError] = useState(null);
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [filterCat, setFilterCat] = useState(
    () => hashParams().get("cat") || "",
  );
  const [filterAcct, setFilterAcct] = useState("");
  const [filterMethod, setFilterMethod] = useState("");
  const [page, setPage] = useState(0);
  const searchTimer = useRef(null);

  // Debounce search input (300ms)
  useEffect(() => {
    clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(() => {
      setDebouncedSearch(search);
      setPage(0);
    }, 300);
    return () => clearTimeout(searchTimer.current);
  }, [search]);

  // Reset to first page when filters change
  useEffect(() => {
    setPage(0);
  }, [filterCat, filterAcct, filterMethod]);

  // Build query string and fetch transactions
  const fetchTransactions = useCallback(() => {
    const params = new URLSearchParams();
    params.set("limit", String(TXN_PAGE_SIZE));
    params.set("offset", String(page * TXN_PAGE_SIZE));
    if (debouncedSearch) params.set("search", debouncedSearch);
    if (filterCat) params.set("category_id", filterCat);
    if (filterAcct) params.set("account_id", filterAcct);
    if (filterMethod) params.set("category_method", filterMethod);
    return api.get(`/transactions?${params}`);
  }, [page, debouncedSearch, filterCat, filterAcct, filterMethod]);

  // Load categories + accounts once
  useEffect(() => {
    Promise.all([api.get("/categories"), api.get("/accounts")])
      .then(([c, a]) => {
        setCategories(c);
        setAccounts(a);
      })
      .catch(setError);
  }, []);

  // Fetch transaction page whenever filters/page change
  useEffect(() => {
    fetchTransactions().then(setPageData).catch(setError);
  }, [fetchTransactions]);

  const reload = useCallback(() => {
    fetchTransactions().then(setPageData).catch(setError);
  }, [fetchTransactions]);

  if (error)
    return html`<${ErrorPanel} error=${error} onRetry=${() => {
      setError(null);
      reload();
    }} />`;
  if (!pageData) return html`<p class="text-light">Loading...</p>`;

  const catMap = Object.fromEntries((categories ?? []).map((c) => [c.id, c]));
  const acctMap = Object.fromEntries((accounts ?? []).map((a) => [a.id, a]));

  const { items: txns, total } = pageData;
  const totalPages = Math.max(1, Math.ceil(total / TXN_PAGE_SIZE));
  const rangeStart = page * TXN_PAGE_SIZE + 1;
  const rangeEnd = Math.min((page + 1) * TXN_PAGE_SIZE, total);
  const hasActiveFilter =
    filterCat || filterAcct || filterMethod || debouncedSearch;

  if (total === 0 && !hasActiveFilter)
    return html`
      <h2>Transactions</h2>
      <p class="text-light">
        No transactions yet. Connect an account and run a sync job to pull in
        data.
      </p>
    `;

  return html`
    <div class="hstack" style="align-items:baseline;margin-bottom:0.75rem">
      <h2 style="margin:0">Transactions</h2>
      <span class="text-lighter small" style="margin-left:0.75rem">
        ${total === 0 ? "0" : `${rangeStart}\u2013${rangeEnd} of ${total}`}
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
      <${CategorySelect}
        value=${filterCat}
        onChange=${setFilterCat}
        categories=${categories ?? []}
        catMap=${catMap}
        placeholder="All categories"
        extraOptions=${[
          { value: "", label: "All categories" },
          { value: "__none", label: "Uncategorized" },
        ]}
      />
      <select value=${filterAcct} onChange=${(e) => setFilterAcct(e.target.value)}>
        <option value="">All accounts</option>
        ${(accounts ?? []).map(
          (a) =>
            html`<option value=${a.id}>${accountDisplayName(a) || a.id}</option>`,
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
      (filterCat || filterAcct || filterMethod) &&
      html`
      <div class="hstack gap-2" style="margin-bottom:0.75rem">
        ${
          filterCat &&
          html`
          <button class="chip ${filterCat !== "__none" ? budgetModeColor(categoryBudgetMode(catMap, filterCat)) : ""}" onClick=${() => setFilterCat("")}>
            <span>${filterCat === "__none" ? "Uncategorized" : categoryName(catMap, filterCat)}</span>
            <span class="chip-close" aria-label="Remove filter">\u00d7</span>
          </button>
        `
        }
        ${
          filterAcct &&
          html`
          <button class="chip" onClick=${() => setFilterAcct("")}>
            <span>${accountDisplayName(acctMap[filterAcct]) || filterAcct}</span>
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

    <${TransactionTable}
      transactions=${txns}
      categories=${categories}
      catMap=${catMap}
      accounts=${accounts}
      onTransactionUpdate=${(txnId, patch) => {
        setPageData((prev) => ({
          ...prev,
          items: prev.items.map((t) =>
            t.id === txnId ? { ...t, ...patch } : t,
          ),
        }));
      }}
      onRuleCreated=${() => waitForJobsDrained().then(reload)}
    />

    ${
      total > TXN_PAGE_SIZE &&
      html`
      <nav aria-label="Pagination" class="hstack" style="justify-content:center;gap:0.75rem;margin-top:1rem">
        <button disabled=${page === 0} onClick=${() => setPage((p) => p - 1)}>
          \u2190 Prev
        </button>
        <span class="text-light small">
          Page ${page + 1} of ${totalPages}
        </span>
        <button disabled=${page >= totalPages - 1} onClick=${() => setPage((p) => p + 1)}>
          Next \u2192
        </button>
      </nav>
    `
    }
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
      const createdParents = {};
      for (const catName of selectedSuggestions) {
        const colonIdx = catName.indexOf(":");
        let parentIdForNew;
        let leafName = catName;
        if (colonIdx !== -1) {
          const parentName = catName.slice(0, colonIdx);
          leafName = catName.slice(colonIdx + 1);
          const existingParent = (categories ?? []).find(
            (c) => c.name === parentName && !c.parent_id,
          );
          if (existingParent) {
            parentIdForNew = existingParent.id;
          } else if (createdParents[parentName]) {
            parentIdForNew = createdParents[parentName];
          } else {
            const created = await api.post("/categories", { name: parentName });
            createdParents[parentName] = created.id;
            parentIdForNew = created.id;
          }
        }
        await api.post("/categories", {
          name: leafName,
          parent_id: parentIdForNew,
        });
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
      budget_type: cat.budget_type ?? "",
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
        budget_type:
          editForm.budget_mode &&
          editForm.budget_mode !== "salary" &&
          editForm.budget_mode !== "transfer"
            ? editForm.budget_type || "variable"
            : null,
        budget_amount:
          editForm.budget_mode &&
          editForm.budget_mode !== "salary" &&
          editForm.budget_mode !== "transfer"
            ? editForm.budget_amount || null
            : null,
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

  if (error)
    return html`<${ErrorPanel} error=${error} onRetry=${() => {
      setError(null);
      load();
    }} />`;
  if (!categories) return html`<p class="text-light">Loading...</p>`;

  const catMap = Object.fromEntries(categories.map((c) => [c.id, c]));
  // Build a set of all known names: both raw stored names and qualified "Parent:Child" forms.
  // This ensures LLM suggestions that already exist (under either naming convention) are filtered out.
  const existingNames = new Set(
    categories.flatMap((c) => {
      const names = [c.name];
      const q = categoryQualifiedName(catMap, c.id);
      if (q && q !== c.name) names.push(q);
      return names;
    }),
  );
  const roots = categories.filter((c) => !c.parent_id || !catMap[c.parent_id]);

  // Build hierarchy tree: roots sorted alphabetically, children nested under parents
  const childrenOf = {};
  for (const c of categories) {
    if (c.parent_id) {
      if (!childrenOf[c.parent_id]) childrenOf[c.parent_id] = [];
      childrenOf[c.parent_id].push(c);
    }
  }
  for (const k of Object.keys(childrenOf)) {
    childrenOf[k].sort((a, b) => a.name.localeCompare(b.name));
  }

  function buildTree(parentId, depth) {
    const result = [];
    const children = parentId
      ? (childrenOf[parentId] ?? [])
      : roots.sort((a, b) => a.name.localeCompare(b.name));
    for (const c of children) {
      const hasChildren = (childrenOf[c.id] ?? []).length > 0;
      result.push({ ...c, depth, hasChildren });
      result.push(...buildTree(c.id, depth + 1));
    }
    return result;
  }

  const tree = buildTree(null, 0);

  // Group tree items into root-level groups (each root + its descendants)
  const rootGroups = [];
  let currentGroup = null;
  for (const item of tree) {
    if (item.depth === 0) {
      currentGroup = [item];
      rootGroups.push(currentGroup);
    } else if (currentGroup) {
      currentGroup.push(item);
    }
  }

  const pendingSuggestions = (suggestions ?? []).filter(
    (s) => !existingNames.has(s.category_name),
  );

  function budgetBadge(cat) {
    if (!cat.budget_mode) return null;
    if (cat.budget_mode === "salary" || cat.budget_mode === "transfer")
      return null;
    if (cat.budget_mode === "project") {
      const parts = [];
      if (cat.project_start_date)
        parts.push(formatDateShort(cat.project_start_date));
      if (cat.project_end_date)
        parts.push(formatDateShort(cat.project_end_date));
      return parts.length > 0
        ? html`<span class="text-light text-body">${parts.join(" \u2013 ")}</span>`
        : null;
    }
    const amt =
      cat.budget_amount != null
        ? formatAmount(cat.budget_amount, { decimals: 0 })
        : "?";
    return html`<span>${amt}</span>`;
  }

  function modeDot(mode) {
    if (!mode) return null;
    const cls = budgetModeColor(mode);
    const label =
      mode === "monthly"
        ? "Monthly"
        : mode === "annual"
          ? "Annual"
          : mode === "salary"
            ? "Salary"
            : mode === "transfer"
              ? "Transfer"
              : "Project";
    return html`<span class="hstack" style="gap:0.3rem;align-items:center"><span class="method-dot ${cls}" style="cursor:default"></span><span class="text-light text-caption">${label}</span></span>`;
  }

  return html`
    <div class="hstack" style="align-items:baseline;margin-bottom:0.5rem">
      <h2 style="margin:0">Categories</h2>
      <span class="text-light text-body">${categories.length}</span>
    </div>

    ${
      pendingSuggestions.length > 0 &&
      html`
        <div style="margin-bottom:1rem">
          <h4 style="margin-bottom:0.15rem">LLM Suggestions</h4>
          <p class="text-light text-body" style="margin-bottom:0.4rem">
            Select to accept, then re-run categorize.
          </p>
          <div class="hstack gap-2" role="group" aria-label="Suggested categories" style="margin-bottom:0.5rem;flex-wrap:wrap">
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
              data-variant="primary" class="small"
              onClick=${acceptSelected}
              disabled=${adding}
            >
              ${adding ? "Accepting..." : `Accept ${selectedSuggestions.size}`}
            </button>
          `
          }
        </div>
      `
    }

    <form class="hstack gap-2" style="flex-wrap:wrap;margin-bottom:0.75rem" onSubmit=${handleAdd}>
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
        : html`
          <div class="vstack gap-3">
            ${rootGroups.map((group) => {
              const root = group[0];
              const children = group.slice(1);
              return html`
                <details key=${root.id} open class="cat-group">
                  <summary class="cat-group-summary clickable-row" onClick=${(
                    e,
                  ) => {
                    e.preventDefault();
                    startEdit(root);
                  }}>
                    <span style="font-weight:600">${root.name}</span>
                    <span style="margin-left:auto" class="hstack gap-2" style="align-items:center">
                      ${modeDot(root.budget_mode)}
                      <span>${budgetBadge(root) ?? html`<span class="text-light">\u2014</span>`}</span>
                      ${
                        children.length > 0 &&
                        html`<span
                        class="cat-group-toggle"
                        onClick=${(e) => {
                          e.stopPropagation();
                          e.currentTarget
                            .closest("details")
                            .toggleAttribute("open");
                        }}
                      >\u25BE</span>`
                      }
                    </span>
                  </summary>
                  ${
                    children.length > 0 &&
                    html`
                    <div class="cat-group-children">
                      ${children.map(
                        (c) => html`
                        <div key=${c.id} class="cat-tree-row clickable-row" style="padding-left:${c.depth * 1.5}rem" onClick=${() => startEdit(c)}>
                          <span>${c.name}</span>
                          <span class="hstack gap-2" style="margin-left:auto;align-items:center">
                            ${modeDot(c.budget_mode)}
                            <span class="text-light text-body">${c.transaction_count || ""}</span>
                            <span style="min-width:5rem;text-align:right">${budgetBadge(c) ?? html`<span class="text-light">\u2014</span>`}</span>
                          </span>
                        </div>
                      `,
                      )}
                    </div>
                  `
                  }
                </details>
              `;
            })}
          </div>
        `
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
                <option value="salary">Salary</option>
                <option value="transfer">Transfer</option>
              </select>
            </div>
            ${
              editForm.budget_mode &&
              editForm.budget_mode !== "salary" &&
              editForm.budget_mode !== "transfer" &&
              html`
              <div data-field>
                <label>Type</label>
                <select
                  value=${editForm.budget_type || "variable"}
                  onChange=${(e) => setEditField("budget_type", e.target.value)}
                >
                  <option value="variable">Variable</option>
                  <option value="fixed">Fixed</option>
                </select>
              </div>
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
          <footer style="justify-content:space-between">
            <button type="button" data-variant="danger" class="outline small" onClick=${() => handleDelete(editingId)}>Delete</button>
            <div class="hstack gap-2">
              <button type="button" class="outline" onClick=${(e) => e.target.closest("dialog").close()}>Cancel</button>
              <button type="submit" disabled=${saving}>
                ${saving ? "Saving..." : "Save"}
              </button>
            </div>
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
  const [preview, setPreview] = useState(null);
  const [previewing, setPreviewing] = useState(false);

  const emptyForm = {
    rule_type: "categorization",
    conditions: [{ field: "merchant", pattern: "" }],
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

  if (error)
    return html`<${ErrorPanel} error=${error} onRetry=${() => {
      setError(null);
      load();
    }} />`;
  if (!rules) return html`<p class="text-light">Loading...</p>`;

  const catMap = Object.fromEntries((categories ?? []).map((c) => [c.id, c]));

  function setField(key, value) {
    setForm((prev) => ({ ...prev, [key]: value }));
    setPreview(null);
  }

  function startEdit(rule) {
    setEditingId(rule.id);
    setForm({
      rule_type: rule.rule_type,
      conditions: rule.conditions.map((c) => ({
        field: c.field,
        pattern: c.pattern,
      })),
      target_category_id: rule.target_category_id ?? "",
      target_correlation_type: rule.target_correlation_type ?? "",
      priority: rule.priority,
    });
    setShowForm(false);
    setPreview(null);
  }

  function cancelEdit() {
    setEditingId(null);
    setForm(emptyForm);
    setPreview(null);
  }

  async function handleSubmit(e) {
    e.preventDefault();
    setSaving(true);
    const body = {
      rule_type: form.rule_type,
      conditions: form.conditions,
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
        ot.toast(
          `Categorized ${result.categorized_count} transaction${result.categorized_count !== 1 ? "s" : ""}`,
          "Rules applied",
          { variant: "success" },
        );
      } else {
        ot.toast(
          "No uncategorized transactions matched any rule",
          "Rules applied",
          { variant: "warning" },
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
    const labels = {
      merchant: "merchant",
      description: "description",
      amount_range: "amount range",
      counterparty_name: "counterparty name",
      counterparty_iban: "counterparty IBAN",
      counterparty_bic: "counterparty BIC",
      bank_transaction_code: "bank txn code",
      amazon_item_title: "Amazon item",
    };
    return labels[field] ?? field;
  }

  function ruleTarget(rule) {
    if (rule.rule_type === "categorization") {
      const name = categoryName(catMap, rule.target_category_id) ?? "none";
      const cls = budgetModeColor(
        categoryBudgetMode(catMap, rule.target_category_id),
      );
      return html`<span class=${cls}>${name}</span>`;
    }
    return rule.target_correlation_type ?? "none";
  }

  function setCondition(idx, key, value) {
    setForm((prev) => {
      const conditions = prev.conditions.map((c, i) =>
        i === idx ? { ...c, [key]: value } : c,
      );
      return { ...prev, conditions };
    });
    setPreview(null);
  }

  function addCondition() {
    setForm((prev) => ({
      ...prev,
      conditions: [...prev.conditions, { field: "merchant", pattern: "" }],
    }));
    setPreview(null);
  }

  function removeCondition(idx) {
    setForm((prev) => ({
      ...prev,
      conditions: prev.conditions.filter((_, i) => i !== idx),
    }));
    setPreview(null);
  }

  async function handlePreview() {
    setPreviewing(true);
    setPreview(null);
    try {
      const result = await api.post("/rules/preview", {
        rule_type: form.rule_type,
        conditions: form.conditions,
        target_category_id: form.target_category_id || null,
        target_correlation_type: form.target_correlation_type || null,
        priority: Number(form.priority),
      });
      setPreview(result);
    } catch (err) {
      setPreview({ error: err.message });
    } finally {
      setPreviewing(false);
    }
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
      <div class="vstack gap-xs" style="flex:1">
        ${form.conditions.map(
          (cond, idx) => html`
            <div class="hstack gap-xs" key=${idx}>
              <select
                value=${cond.field}
                onInput=${(e) => setCondition(idx, "field", e.target.value)}
              >
                <option value="merchant">Merchant</option>
                <option value="description">Description</option>
                <option value="amount_range">Amount Range</option>
                <option value="counterparty_name">Counterparty Name</option>
                <option value="counterparty_iban">Counterparty IBAN</option>
                <option value="counterparty_bic">Counterparty BIC</option>
                <option value="bank_transaction_code">Bank Transaction Code</option>
                <option value="amazon_item_title">Amazon Item Title</option>
              </select>
              <input
                type="text"
                placeholder=${cond.field === "amount_range" ? "e.g. 50..200, >100, <=50" : "Pattern"}
                value=${cond.pattern}
                onInput=${(e) => setCondition(idx, "pattern", e.target.value)}
                required
                style="flex:1"
              />
              ${
                form.conditions.length > 1 &&
                html`
                <button type="button" class="small" data-variant="danger" onClick=${() => removeCondition(idx)} title="Remove condition">&times;</button>
              `
              }
            </div>
          `,
        )}
        <button type="button" class="small" onClick=${addCondition} style="align-self:start">+ Add condition</button>
      </div>
      ${
        form.rule_type === "categorization"
          ? html`<${CategorySelect}
            value=${form.target_category_id}
            onChange=${(id) => setField("target_category_id", id)}
            categories=${categories}
            catMap=${catMap}
            placeholder="-- Category --"
            clearable
          />`
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
          <td>
            ${renderFormFields()}
            ${
              preview &&
              !preview.error &&
              html`
              <div class="text-light text-body" style="margin-top:0.5rem">
                Matches <strong>${preview.match_count}</strong> transaction${preview.match_count !== 1 ? "s" : ""}${preview.sample.length > 0 ? ` \u2014 ${preview.sample.map((s) => s.merchant_name).join(", ")}` : ""}
              </div>
            `
            }
            ${preview?.error && html`<div class="text-body" style="color:var(--danger);margin-top:0.5rem">${preview.error}</div>`}
          </td>
          <td style="white-space:nowrap">
            <button data-variant="primary" class="small" onClick=${handleSubmit} disabled=${saving}>
              Save
            </button>
            <button class="small" onClick=${handlePreview} disabled=${previewing || form.conditions.every((c) => !c.pattern)}>
              ${previewing ? "..." : "Preview"}
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
            <span class="mono text-caption" style="min-width:1.5rem;text-align:right">${rule.priority}</span>
            <span class="chip outline ${rule.rule_type === "categorization" ? "success" : ""}"
              >${rule.rule_type}</span
            >
            ${rule.conditions.map(
              (c, i) => html`
                ${i > 0 && html`<span class="text-light text-caption">AND</span>`}
                <span class="text-light">${fieldLabel(c.field)}</span>
                <code>${c.pattern}</code>
              `,
            )}
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
        <div class="hstack gap-2" style="flex-wrap:wrap;align-items:center">${renderFormFields()}</div>
        <div class="hstack gap-sm">
          <button data-variant="primary" type="submit" disabled=${saving}>
            Create Rule
          </button>
          <button type="button" disabled=${previewing || form.conditions.every((c) => !c.pattern)} onClick=${handlePreview}>
            ${previewing ? "Checking..." : "Preview"}
          </button>
          ${
            preview &&
            !preview.error &&
            html`
            <span class="text-light">
              Matches <strong>${preview.match_count}</strong> transaction${preview.match_count !== 1 ? "s" : ""}${preview.sample.length > 0 ? ` \u2014 ${preview.sample.map((s) => s.merchant_name).join(", ")}` : ""}
            </span>
          `
          }
          ${preview?.error && html`<span style="color:var(--danger)">${preview.error}</span>`}
        </div>
      </form>
    `
    }

    ${
      rules.length === 0
        ? html`<p class="text-light mt-4">
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

function AccountNickname({ account, onRenamed }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(account.nickname ?? "");
  const [saving, setSaving] = useState(false);

  async function save() {
    const trimmed = value.trim();
    const nickname = trimmed || null;
    if (nickname === (account.nickname ?? null)) {
      setEditing(false);
      return;
    }
    setSaving(true);
    try {
      const updated = await api.patch(`/accounts/${account.id}`, { nickname });
      onRenamed(updated);
      setEditing(false);
    } finally {
      setSaving(false);
    }
  }

  if (editing) {
    return html`
      <span class="hstack" style="gap:0.25rem">
        <input
          type="text"
          value=${value}
          placeholder=${account.name}
          onInput=${(e) => setValue(e.target.value)}
          onKeyDown=${(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") {
              setValue(account.nickname ?? "");
              setEditing(false);
            }
          }}
          disabled=${saving}
          style="width:14rem"
          ref=${(el) => el?.focus()}
        />
        <button class="small" onClick=${save} disabled=${saving}>Save</button>
        <button class="small" onClick=${() => {
          setValue(account.nickname ?? "");
          setEditing(false);
        }}>Cancel</button>
      </span>
    `;
  }

  const display = account.nickname || account.name;
  return html`
    <span
      style="cursor:pointer;border-bottom:1px dashed var(--border)"
      title="Click to rename"
      onClick=${() => setEditing(true)}
    >
      ${display}
    </span>
    ${account.nickname && html`<span class="text-lighter text-body" style="margin-left:0.5rem">(${account.name})</span>`}
  `;
}

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
  const [importingAccount, setImportingAccount] = useState(null);
  const [showBankSearch, setShowBankSearch] = useState(false);
  const [showManualForm, setShowManualForm] = useState(false);
  const [manualForm, setManualForm] = useState({
    name: "",
    institution: "",
    account_type: "credit_card",
    currency: "EUR",
  });
  const [manualSaving, setManualSaving] = useState(false);

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

  async function importCsv(accountId, file) {
    setImportingAccount(accountId);
    try {
      const text = await file.text();
      const result = await api.raw(
        `/accounts/${accountId}/import`,
        text,
        "text/csv",
      );
      ot.toast(
        `Imported ${result.imported} transactions (${result.duplicates} duplicates, ${result.failed} failed)`,
        "",
        { variant: result.failed > 0 ? "warning" : "success" },
      );
    } catch (e) {
      ot.toast(e.message, "Import failed", { variant: "danger" });
    } finally {
      setImportingAccount(null);
    }
  }

  async function createManualAccount(e) {
    e.preventDefault();
    setManualSaving(true);
    try {
      await api.post("/accounts", {
        provider_account_id: `manual-${Date.now()}`,
        name: manualForm.name,
        institution: manualForm.institution,
        account_type: manualForm.account_type,
        currency: manualForm.currency.toUpperCase(),
      });
      setManualForm({
        name: "",
        institution: "",
        account_type: "credit_card",
        currency: "EUR",
      });
      setShowManualForm(false);
      load();
      ot.toast("Account created", "", { variant: "success" });
    } catch (e) {
      ot.toast(e.message, "Failed to create account", { variant: "danger" });
    } finally {
      setManualSaving(false);
    }
  }

  const filteredAspsps =
    aspsps && searchQuery
      ? aspsps.filter((a) =>
          a.name.toLowerCase().includes(searchQuery.toLowerCase()),
        )
      : aspsps;

  if (error)
    return html`<${ErrorPanel} error=${error} onRetry=${() => {
      setError(null);
      load();
    }} />`;
  if (!connections) return html`<p class="text-light">Loading...</p>`;

  const connectedAccounts = accounts
    ? connections.map((c) => ({
        connection: c,
        accounts: accounts.filter((a) => a.connection_id === c.id),
      }))
    : [];
  const manualAccounts = accounts
    ? accounts.filter((a) => !a.connection_id)
    : [];
  const hasAny = (accounts && accounts.length > 0) || connections.length > 0;

  function renderAccountRow(a) {
    return html`
      <div class="conn-account-row" key=${a.id}>
        <div class="conn-account-info">
          <${AccountNickname}
            account=${a}
            onRenamed=${(updated) =>
              setAccounts((prev) =>
                prev.map((x) => (x.id === updated.id ? updated : x)),
              )}
          />
          <span class="conn-account-meta">
            ${a.account_type}${" "}<span class="mono">${a.currency}</span>
          </span>
        </div>
        <label class="button outline small" style="cursor:pointer;margin:0">
          ${importingAccount === a.id ? "Importing\u2026" : "Import CSV"}
          <input
            type="file"
            accept=".csv"
            style="display:none"
            disabled=${importingAccount === a.id}
            onChange=${(e) => {
              const file = e.target.files[0];
              if (file) importCsv(a.id, file);
              e.target.value = "";
            }}
          />
        </label>
      </div>
    `;
  }

  return html`
    <div class="conn-header">
      <h2 style="margin:0">Accounts</h2>
      <div class="hstack gap-2">
        <button
          class="small ${showBankSearch ? "" : "outline"}"
          data-variant="primary"
          onClick=${() => {
            setShowBankSearch(!showBankSearch);
            setShowManualForm(false);
          }}
        >
          Connect Bank
        </button>
        <button
          class="small ${showManualForm ? "" : "outline"}"
          data-variant="primary"
          onClick=${() => {
            setShowManualForm(!showManualForm);
            setShowBankSearch(false);
          }}
        >
          Add Manual Account
        </button>
      </div>
    </div>

    ${
      showManualForm &&
      html`
      <article class="card" style="margin-bottom:1.25rem">
        <h3 style="margin-top:0;margin-bottom:0.75rem">New Manual Account</h3>
        <p class="text-light text-body" style="margin-bottom:1rem">
          For providers without API access (e.g. Amex EU). Import transactions later via CSV.
        </p>
        <form onSubmit=${createManualAccount}>
          <div class="conn-manual-grid">
            <label data-field>
              Name
              <input
                type="text"
                placeholder="e.g. Amex Platinum"
                required
                value=${manualForm.name}
                onInput=${(e) => setManualForm({ ...manualForm, name: e.target.value })}
              />
            </label>
            <label data-field>
              Institution
              <input
                type="text"
                placeholder="e.g. American Express"
                required
                value=${manualForm.institution}
                onInput=${(e) => setManualForm({ ...manualForm, institution: e.target.value })}
              />
            </label>
            <label data-field>
              Account Type
              <select
                value=${manualForm.account_type}
                onChange=${(e) => setManualForm({ ...manualForm, account_type: e.target.value })}
              >
                <option value="checking">Checking</option>
                <option value="savings">Savings</option>
                <option value="credit_card">Credit Card</option>
                <option value="investment">Investment</option>
                <option value="loan">Loan</option>
                <option value="other">Other</option>
              </select>
            </label>
            <label data-field>
              Currency
              <input
                type="text"
                placeholder="EUR"
                maxlength="3"
                required
                value=${manualForm.currency}
                onInput=${(e) => setManualForm({ ...manualForm, currency: e.target.value })}
                style="width:6rem"
              />
            </label>
          </div>
          <div class="hstack gap-2 mt-4">
            <button type="submit" data-variant="primary" class="small" disabled=${manualSaving}>
              ${manualSaving ? "Creating\u2026" : "Create Account"}
            </button>
            <button type="button" class="small outline" onClick=${() => setShowManualForm(false)}>Cancel</button>
          </div>
        </form>
      </article>
    `
    }

    ${
      showBankSearch &&
      html`
      <article class="card" style="margin-bottom:1.25rem">
        <h3 style="margin-top:0;margin-bottom:0.75rem">Connect Bank</h3>
        <p class="text-light text-body" style="margin-bottom:1rem">
          Link a bank account via Open Banking API for automatic transaction sync.
        </p>
        <div class="hstack gap-2" style="flex-wrap:wrap;margin-bottom:0.75rem">
          <input
            type="text"
            placeholder="Country code (e.g. FI)"
            value=${searchCountry}
            onInput=${(e) => setSearchCountry(e.target.value)}
            style="width:140px"
          />
          <button data-variant="primary" class="small" onClick=${searchAspsps} disabled=${searchLoading}>
            ${searchLoading ? "Searching\u2026" : "Search"}
          </button>
          <button class="small outline" onClick=${() => {
            setShowBankSearch(false);
            setAspsps(null);
          }}>Cancel</button>
        </div>

        ${searchError ? html`<p role="alert" data-variant="error">${searchError}</p>` : null}

        ${
          aspsps
            ? html`
                <input
                  type="text"
                  placeholder="Filter results\u2026"
                  value=${searchQuery}
                  onInput=${(e) => setSearchQuery(e.target.value)}
                  style="width:100%;margin-bottom:0.75rem"
                />
                ${authError ? html`<p role="alert" data-variant="error">${authError}</p>` : null}
                ${
                  filteredAspsps && filteredAspsps.length > 0
                    ? html`
                        <div class="conn-bank-list">
                          ${filteredAspsps.map(
                            (a) => html`
                              <div class="conn-bank-item" key=${a.name}>
                                <span>${a.name}</span>
                                <span class="text-light">${a.country}</span>
                                <button
                                  data-variant="primary" class="small"
                                  onClick=${() => startAuth(a)}
                                  disabled=${authorizing === a.name}
                                >
                                  ${authorizing === a.name ? "Redirecting\u2026" : "Connect"}
                                </button>
                              </div>
                            `,
                          )}
                        </div>
                      `
                    : html`<p class="text-light">No banks found.</p>`
                }
              `
            : null
        }
      </article>
    `
    }

    ${
      !hasAny
        ? html`
          <div class="conn-empty">
            <p class="text-light">No accounts yet.</p>
            <p class="text-light text-body">
              Connect a bank for automatic sync, or add a manual account to import CSV files.
            </p>
          </div>
        `
        : html`
          <div class="conn-groups">
            ${connectedAccounts.map(
              ({ connection: c, accounts: accts }) => html`
                <div class="conn-group" key=${c.id}>
                  <div class="conn-group-header">
                    <div class="conn-group-title">
                      <span style="font-weight:600">${c.institution_name}</span>
                      <span class="chip ${statusBadge(c.status)}">${c.status}</span>
                      ${
                        c.valid_until &&
                        html`
                        <span class="text-light text-body">
                          expires ${formatDate(c.valid_until)}
                        </span>
                      `
                      }
                    </div>
                    <div class="hstack" style="gap:0.35rem">
                      ${
                        c.status === "expired" &&
                        html`
                        <button
                          data-variant="primary" class="small"
                          onClick=${() => startAuth({ name: c.institution_name, country: "" })}
                        >
                          Reconnect
                        </button>
                      `
                      }
                      ${
                        c.status !== "revoked" &&
                        html`
                        <button data-variant="danger" class="small ghost" onClick=${() => revokeConnection(c.id)}>
                          Revoke
                        </button>
                      `
                      }
                    </div>
                  </div>
                  ${
                    accts.length > 0
                      ? accts.map(renderAccountRow)
                      : html`<div class="conn-account-row"><span class="text-light">No accounts</span></div>`
                  }
                </div>
              `,
            )}

            ${
              manualAccounts.length > 0 &&
              html`
              <div class="conn-group">
                <div class="conn-group-header">
                  <div class="conn-group-title">
                    <span style="font-weight:600">Manual Accounts</span>
                    <span class="chip">csv</span>
                  </div>
                </div>
                ${manualAccounts.map(renderAccountRow)}
              </div>
            `
            }
          </div>
        `
    }

    <${AmazonPanel} />
  `;
}

// ---------------------------------------------------------------------------
// Amazon Enrichment
// ---------------------------------------------------------------------------

function AmazonPanel() {
  const [accounts, setAccounts] = useState([]);
  const [statuses, setStatuses] = useState({});
  const [showForm, setShowForm] = useState(false);
  const [newLabel, setNewLabel] = useState("");
  const [creating, setCreating] = useState(false);

  function loadAccounts() {
    api
      .get("/amazon/accounts")
      .then((accts) => {
        setAccounts(accts);
        for (const a of accts) {
          api
            .get(`/amazon/accounts/${a.id}/status`)
            .then((s) => setStatuses((prev) => ({ ...prev, [a.id]: s })))
            .catch(() => {});
        }
      })
      .catch(() => setAccounts([]));
  }

  useEffect(loadAccounts, []);

  async function addAccount(e) {
    e.preventDefault();
    if (!newLabel.trim()) return;
    setCreating(true);
    try {
      await api.post("/amazon/accounts", { label: newLabel.trim() });
      setNewLabel("");
      setShowForm(false);
      loadAccounts();
      ot.toast("Account created", "", { variant: "success" });
    } catch (e) {
      ot.toast(e.message, "Failed to create account", { variant: "danger" });
    } finally {
      setCreating(false);
    }
  }

  async function deleteAccount(id, label) {
    if (
      !confirm(
        `Delete Amazon account "${label}"? This removes all its transactions and matches.`,
      )
    )
      return;
    try {
      await api.del(`/amazon/accounts/${id}`);
      loadAccounts();
      ot.toast("Account deleted", "", { variant: "success" });
    } catch (e) {
      ot.toast(e.message, "Failed to delete account", { variant: "danger" });
    }
  }

  return html`
    <div style="margin-top:2rem">
      <div class="conn-header">
        <h3 style="margin:0">Amazon Enrichment</h3>
        <button
          class="small ${showForm ? "" : "outline"}"
          data-variant="primary"
          onClick=${() => setShowForm(!showForm)}
        >
          Add Account
        </button>
      </div>

      ${
        showForm &&
        html`
        <article class="card" style="margin-bottom:1.25rem">
          <form class="hstack gap-2" onSubmit=${addAccount}>
            <input
              type="text"
              placeholder="Account label (e.g. Amazon DE)"
              required
              value=${newLabel}
              onInput=${(e) => setNewLabel(e.target.value)}
              style="flex:1"
            />
            <button type="submit" data-variant="primary" class="small" disabled=${creating}>
              ${creating ? "Creating\u2026" : "Create Account"}
            </button>
            <button type="button" class="small outline" onClick=${() => setShowForm(false)}>Cancel</button>
          </form>
        </article>
      `
      }

      <div class="vstack gap-3">
        ${accounts.map(
          (account) => html`
            <${AmazonAccountCard}
              key=${account.id}
              account=${account}
              status=${statuses[account.id]}
              onReload=${loadAccounts}
              onDelete=${() => deleteAccount(account.id, account.label)}
            />
          `,
        )}
      </div>
    </div>
  `;
}

function AmazonAccountCard({ account, status, onReload, onDelete }) {
  const [cookieText, setCookieText] = useState("");
  const [uploading, setUploading] = useState(false);
  const [syncing, setSyncing] = useState(false);

  async function uploadCookies() {
    setUploading(true);
    try {
      const trimmed = cookieText.trim();
      const cookies = trimmed.startsWith("[") ? JSON.parse(trimmed) : trimmed;
      await api.post(`/amazon/accounts/${account.id}/cookies`, { cookies });
      setCookieText("");
      onReload();
      ot.toast("Cookies saved", "", { variant: "success" });
    } catch (e) {
      ot.toast(e.message, "Cookie upload failed", { variant: "danger" });
    } finally {
      setUploading(false);
    }
  }

  async function triggerSync() {
    setSyncing(true);
    try {
      await api.post(`/amazon/accounts/${account.id}/sync`);
      ot.toast("Amazon sync queued", "", { variant: "success" });
    } catch (e) {
      ot.toast(e.message, "Sync failed", { variant: "danger" });
    } finally {
      setSyncing(false);
    }
  }

  return html`
    <div class="card" style="padding:1rem">
      <div class="hstack gap-3" style="align-items:start;justify-content:space-between">
        <div class="vstack gap-2" style="flex:1">
          <div class="hstack gap-2" style="align-items:center">
            <strong>${account.label}</strong>
            ${
              status?.cookies_valid === true
                ? html`<span class="chip success">Valid</span>`
                : status?.cookies_valid === false
                  ? html`<span class="chip danger">Expired</span>`
                  : html`<span class="chip">No cookies</span>`
            }
            ${
              status?.cookies_expiry
                ? html`<span class="text-light small">expires ${formatDate(status.cookies_expiry)}${status.cookies_days_remaining != null ? ` (${status.cookies_days_remaining}d)` : ""}</span>`
                : null
            }
          </div>

          ${
            status?.stats
              ? html`
              <div class="hstack gap-3" style="flex-wrap:wrap">
                <span class="text-light">${status.stats.total_transactions} transactions</span>
                <span class="text-light">${status.stats.matched_transactions} matched</span>
                <span class="text-light">${status.stats.total_orders} orders</span>
                <span class="text-light">${status.stats.total_items} items</span>
              </div>
            `
              : null
          }

          <details>
            <summary>Update Cookies</summary>
            <div style="margin-top:0.5rem">
              <textarea
                placeholder="Paste cookies here (JSON array or Netscape cookies.txt)..."
                rows="4"
                style="width:100%;font-family:monospace;font-size:0.85rem"
                value=${cookieText}
                onInput=${(e) => setCookieText(e.target.value)}
              ></textarea>
              <button
                class="small"
                data-variant="primary"
                style="margin-top:0.5rem"
                onClick=${uploadCookies}
                disabled=${uploading || !cookieText.trim()}
              >
                ${uploading ? "Saving\u2026" : "Save Cookies"}
              </button>
            </div>
          </details>
        </div>

        <div class="hstack gap-2">
          <button
            data-variant="primary"
            class="small"
            onClick=${triggerSync}
            disabled=${syncing || !status?.cookies_valid}
          >
            ${syncing ? "Syncing\u2026" : "Sync Now"}
          </button>
          <button
            data-variant="danger"
            class="small"
            onClick=${onDelete}
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  `;
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

function Jobs() {
  const [counts, setCounts] = useState(null);
  const [schedule, setSchedule] = useState(null);
  const [triggering, setTriggering] = useState(new Set());

  function load() {
    Promise.all([api.get("/jobs/counts"), api.get("/jobs/schedule")])
      .then(([c, s]) => {
        setCounts(c);
        setSchedule(s);
      })
      .catch((err) => ot.toast(err.message, "Error", { variant: "danger" }));
  }

  const pollRef = useRef(null);

  useEffect(() => {
    let cancelled = false;
    function poll() {
      Promise.all([api.get("/jobs/counts"), api.get("/jobs/schedule")])
        .then(([c, s]) => {
          if (cancelled) return;
          setCounts(c);
          setSchedule(s);
          const busy = c.some((q) => q.active > 0 || q.waiting > 0);
          pollRef.current = setTimeout(poll, busy ? 2000 : 10000);
        })
        .catch(() => {
          if (!cancelled) pollRef.current = setTimeout(poll, 10000);
        });
    }
    poll();
    return () => {
      cancelled = true;
      clearTimeout(pollRef.current);
    };
  }, []);

  function addTriggering(key) {
    setTriggering((prev) => new Set(prev).add(key));
  }
  function removeTriggering(key) {
    setTriggering((prev) => {
      const next = new Set(prev);
      next.delete(key);
      return next;
    });
  }

  async function trigger(path, key, successMsg) {
    addTriggering(key);
    try {
      await api.post(path);
      load();
      if (successMsg) ot.toast(successMsg, "", { variant: "success" });
    } catch (err) {
      ot.toast(err.message, "Error", { variant: "danger" });
    } finally {
      removeTriggering(key);
    }
  }

  async function triggerSyncAll() {
    if (!schedule || schedule.length === 0) return;
    addTriggering("sync-all");
    try {
      await Promise.all(
        schedule.map((s) => {
          addTriggering(`sync-${s.account_id}`);
          return api
            .post(syncUrlFor(s))
            .finally(() => removeTriggering(`sync-${s.account_id}`));
        }),
      );
      load();
      ot.toast("Sync queued for all accounts", "", { variant: "success" });
    } catch (err) {
      ot.toast(err.message, "Error", { variant: "danger" });
    } finally {
      removeTriggering("sync-all");
    }
  }

  function renderSyncCard() {
    const card = QUEUE_CARDS.find((c) => c.key === "sync");
    const c = cardCounts(card, counts);
    const syncAllBusy = triggering.has("sync-all");

    return html`
      <div class="queue-card">
        <span class="queue-card-title">${card.title}</span>
        ${c.failed > 0 ? html`<span class="chip danger">${c.failed} failed</span>` : null}
        ${c.active > 0 ? html`<span class="chip outline"><span class="text-light">Active</span> <span class="mono">${c.active}</span></span>` : null}
        ${c.waiting > 0 ? html`<span class="chip outline"><span class="text-light">Waiting</span> <span class="mono">${c.waiting}</span></span>` : null}
        <button
          data-variant="primary" class="small"
          onClick=${triggerSyncAll}
          disabled=${syncAllBusy || !schedule || schedule.length === 0}
        >
          ${syncAllBusy ? "..." : "Sync All"}
        </button>
        <span class="queue-card-desc">${card.desc}</span>
        ${
          schedule && schedule.length > 0
            ? html`
          <div class="sync-schedule">
            ${schedule.map((s) => {
              const isOk = s.last_run_status === "succeeded";
              const isFailed = s.last_run_status === "failed";
              const isRunning = s.last_run_status === "running";
              const nextReason = s.next_run_reason
                ? ` (${s.next_run_reason})`
                : "";
              const busy =
                triggering.has(`sync-${s.account_id}`) || syncAllBusy;
              return html`
                <div class="sync-row hstack gap-3">
                  <span class="sync-row-name">${s.account_name}</span>
                  <span class="text-light">${timeAgo(s.last_run_at)}</span>
                  ${isOk ? html`<span class="chip success">OK</span>` : null}
                  ${isFailed ? html`<span class="chip danger" title=${s.last_error || ""}>Failed</span>` : null}
                  ${isRunning ? html`<span class="chip outline">Running</span>` : null}
                  ${!s.last_run_status ? html`<span class="text-light">\u2014</span>` : null}
                  <span class="sync-row-next text-light">
                    ${s.next_run_at ? html`${timeAgo(s.next_run_at)}${nextReason}` : "\u2014"}
                  </span>
                  <button
                    class="small outline"
                    onClick=${() => trigger(syncUrlFor(s), `sync-${s.account_id}`, `Sync queued for ${s.account_name}`)}
                    disabled=${busy}
                  >
                    ${busy ? "..." : "Sync"}
                  </button>
                </div>
              `;
            })}
          </div>
        `
            : null
        }
      </div>
    `;
  }

  function renderQueueCard(card) {
    if (card.key === "sync") return renderSyncCard();
    const c = cardCounts(card, counts);

    return html`
      <div class="queue-card">
        <span class="queue-card-title">${card.title}</span>
        ${c.failed > 0 ? html`<span class="chip danger">${c.failed} failed</span>` : null}
        ${c.active > 0 ? html`<span class="chip outline"><span class="text-light">Active</span> <span class="mono">${c.active}</span></span>` : null}
        ${c.waiting > 0 ? html`<span class="chip outline"><span class="text-light">Waiting</span> <span class="mono">${c.waiting}</span></span>` : null}
        <button
          data-variant="primary" class="small"
          onClick=${() => trigger(`/jobs/${card.key === "recompute" ? "recompute" : card.key}`, card.key, `${card.title} queued`)}
          disabled=${triggering.has(card.key)}
        >
          ${triggering.has(card.key) ? "..." : card.key === "recompute" ? "Run" : "Run All"}
        </button>
        <span class="queue-card-desc">${card.desc}</span>
      </div>
    `;
  }

  const [allJobs, setAllJobs] = useState(null);
  const [jobsOpen, setJobsOpen] = useState(false);

  useEffect(() => {
    if (!jobsOpen) {
      setAllJobs(null);
      return;
    }
    let cancelled = false;
    let timer = null;
    function poll() {
      api
        .get("/jobs")
        .then((jobs) => {
          if (cancelled) return;
          setAllJobs(jobs);
          const busy = jobs.some(
            (j) => j.status === "Running" || j.status === "Pending",
          );
          timer = setTimeout(poll, busy ? 2000 : 10000);
        })
        .catch(() => {
          if (!cancelled) timer = setTimeout(poll, 10000);
        });
    }
    poll();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [jobsOpen]);

  function statusChip(s) {
    if (s === "Done") return html`<span class="chip success">Done</span>`;
    if (s === "Failed" || s === "Killed")
      return html`<span class="chip danger">${s}</span>`;
    if (s === "Running") return html`<span class="chip outline">Running</span>`;
    if (s === "Pending") return html`<span class="chip outline">Pending</span>`;
    return html`<span class="chip outline">${s}</span>`;
  }

  if (!counts) return html`<p class="text-light">Loading...</p>`;

  return html`
    <h2>Jobs</h2>

    <div class="queue-cards">
      ${QUEUE_CARDS.map(renderQueueCard)}
    </div>

    <details onToggle=${(e) => setJobsOpen(e.target.open)}>
      <summary>All Jobs (debug)</summary>
      ${
        allJobs
          ? html`
          <div class="table" style="margin-top:0.5rem">
            <table>
              <thead>
                <tr>
                  <th>Type</th>
                  <th>Status</th>
                  <th>Count</th>
                  <th>Last Run</th>
                  <th>Last Done</th>
                  <th>Errors</th>
                </tr>
              </thead>
              <tbody>
                ${Object.values(
                  allJobs.reduce((groups, j) => {
                    const key = `${j.job_type}|${j.status}`;
                    if (!groups[key]) {
                      groups[key] = {
                        job_type: j.job_type,
                        status: j.status,
                        count: 0,
                        last_run_at: j.run_at,
                        last_done_at: j.done_at,
                        errors: 0,
                      };
                    }
                    const g = groups[key];
                    g.count++;
                    if (j.run_at > g.last_run_at) g.last_run_at = j.run_at;
                    if (
                      j.done_at &&
                      (!g.last_done_at || j.done_at > g.last_done_at)
                    )
                      g.last_done_at = j.done_at;
                    if (j.error) g.errors++;
                    return groups;
                  }, {}),
                ).map(
                  (g) => html`
                    <tr>
                      <td class="mono">${shortType(g.job_type)}</td>
                      <td>${statusChip(g.status)}</td>
                      <td class="mono">${g.count}</td>
                      <td class="text-light">${timeAgo(g.last_run_at)}</td>
                      <td class="text-light">${g.last_done_at ? timeAgo(g.last_done_at) : "\u2014"}</td>
                      <td class="mono">${g.errors || "\u2014"}</td>
                    </tr>
                  `,
                )}
              </tbody>
            </table>
          </div>
        `
          : html`<p class="text-light">Loading...</p>`
      }
    </details>
  `;
}

// ---------------------------------------------------------------------------
// Charts (shared by Insights + Balances)
// ---------------------------------------------------------------------------

function NetWorthChart({ data }) {
  const padding = { top: 20, right: 20, bottom: 40, left: 60 };
  const width = 720;
  const height = 340;
  const innerW = width - padding.left - padding.right;
  const innerH = height - padding.top - padding.bottom;

  const allPoints = useMemo(() => {
    const hist = (data.history || []).map((p) => ({
      date: new Date(`${p.date}T00:00:00`),
      value: Number(p.value),
    }));
    const fore = (data.forecast || []).map((p) => ({
      date: new Date(`${p.date}T00:00:00`),
      value: p.value,
      lower: p.lower,
      upper: p.upper,
    }));
    return { hist, fore };
  }, [data]);

  const { hist, fore } = allPoints;

  const allDates = [...hist.map((p) => p.date), ...fore.map((p) => p.date)];
  const allValues = [
    ...hist.map((p) => p.value),
    ...fore.map((p) => p.value),
    ...fore.map((p) => p.lower),
    ...fore.map((p) => p.upper),
  ];

  if (allDates.length === 0) return html`<p class="text-light">No data.</p>`;

  const minDate = Math.min(...allDates);
  const maxDate = Math.max(...allDates);
  const minVal = Math.min(...allValues);
  const maxVal = Math.max(...allValues);
  const valRange = maxVal - minVal || 1;
  const dateRange = maxDate - minDate || 1;

  const x = (d) => padding.left + ((d - minDate) / dateRange) * innerW;
  const y = (v) => padding.top + (1 - (v - minVal) / valRange) * innerH;

  const histLine =
    hist.length > 0
      ? hist
          .map((p, i) => `${i === 0 ? "M" : "L"}${x(p.date)},${y(p.value)}`)
          .join(" ")
      : "";

  const foreLine =
    fore.length > 0
      ? fore
          .map((p, i) => `${i === 0 ? "M" : "L"}${x(p.date)},${y(p.value)}`)
          .join(" ")
      : "";

  const bandPath =
    fore.length > 0
      ? `M${fore.map((p) => `${x(p.date)},${y(p.upper)}`).join(" L")} ` +
        `L${[...fore]
          .reverse()
          .map((p) => `${x(p.date)},${y(p.lower)}`)
          .join(" L")} Z`
      : "";

  // Dashed line from last history point to first forecast point
  const bridgeLine =
    hist.length > 0 && fore.length > 0
      ? `M${x(hist[hist.length - 1].date)},${y(hist[hist.length - 1].value)} L${x(fore[0].date)},${y(fore[0].value)}`
      : "";

  const yTickCount = 5;
  const yTicks = Array.from({ length: yTickCount + 1 }, (_, i) => {
    const v = minVal + (valRange * i) / yTickCount;
    return { value: v, y: y(v) };
  });

  const xTickCount = Math.min(6, allDates.length);
  const xTicks = Array.from({ length: xTickCount }, (_, i) => {
    const d = new Date(minDate + (dateRange * i) / (xTickCount - 1 || 1));
    return { date: d, x: x(d) };
  });

  const monthFmt = new Intl.DateTimeFormat("en", {
    month: "short",
    year: "2-digit",
  });

  const [hover, setHover] = useState(null);
  const svgRef = useRef(null);

  const onMouseMove = useCallback(
    (e) => {
      const svg = svgRef.current;
      if (!svg) return;
      const rect = svg.getBoundingClientRect();
      const mx = ((e.clientX - rect.left) / rect.width) * width;
      const dateAtMouse = minDate + ((mx - padding.left) / innerW) * dateRange;

      let closest = null;
      let closestDist = Infinity;
      for (const p of hist) {
        const dist = Math.abs(p.date - dateAtMouse);
        if (dist < closestDist) {
          closestDist = dist;
          closest = { ...p, type: "history" };
        }
      }
      for (const p of fore) {
        const dist = Math.abs(p.date - dateAtMouse);
        if (dist < closestDist) {
          closestDist = dist;
          closest = { ...p, type: "forecast" };
        }
      }
      if (closest) setHover(closest);
    },
    [hist, fore, minDate, dateRange, innerW],
  );

  const onMouseLeave = useCallback(() => setHover(null), []);

  return html`
    <svg
      ref=${svgRef}
      viewBox="0 0 ${width} ${height}"
      class="nw-chart"
      onMouseMove=${onMouseMove}
      onMouseLeave=${onMouseLeave}
    >
      ${yTicks.map(
        (t) => html`
          <line
            x1=${padding.left}
            y1=${t.y}
            x2=${width - padding.right}
            y2=${t.y}
            class="nw-grid-line"
          />
        `,
      )}
      ${bandPath && html`<path d=${bandPath} class="nw-band" />`}
      ${histLine && html`<path d=${histLine} class="nw-line-history" />`}
      ${bridgeLine && html`<path d=${bridgeLine} class="nw-line-bridge" />`}
      ${foreLine && html`<path d=${foreLine} class="nw-line-forecast" />`}
      ${yTicks.map(
        (t) => html`
          <text x=${padding.left - 8} y=${t.y + 4} class="nw-axis-label" text-anchor="end">
            ${formatAmount(t.value, { decimals: 0 })}
          </text>
        `,
      )}
      ${xTicks.map(
        (t) => html`
          <text x=${t.x} y=${height - 8} class="nw-axis-label" text-anchor="middle">
            ${monthFmt.format(t.date)}
          </text>
        `,
      )}
      ${
        hover &&
        html`
        <line
          x1=${x(hover.date)}
          y1=${padding.top}
          x2=${x(hover.date)}
          y2=${padding.top + innerH}
          class="nw-crosshair"
        />
        <circle cx=${x(hover.date)} cy=${y(hover.value)} r="4" class="nw-dot-${hover.type}" />
        ${
          hover.type === "forecast" &&
          hover.lower != null &&
          html`
          <circle cx=${x(hover.date)} cy=${y(hover.lower)} r="2.5" class="nw-dot-band" />
          <circle cx=${x(hover.date)} cy=${y(hover.upper)} r="2.5" class="nw-dot-band" />
        `
        }
      `
      }
    </svg>
    ${
      hover &&
      html`
      <div class="nw-tooltip hstack gap-4">
        <span class="text-light">${monthFmt.format(hover.date)}</span>
        <span style="font-weight:600">${formatAmount(hover.value, { decimals: 0 })}</span>
        ${
          hover.type === "forecast" &&
          hover.lower != null &&
          html`
          <span class="text-light" style="font-size:var(--text-8)">
            ${formatAmount(hover.lower, { decimals: 0 })} – ${formatAmount(hover.upper, { decimals: 0 })}
          </span>
        `
        }
        <span class="badge" data-variant=${hover.type === "history" ? "default" : "primary"}>
          ${hover.type === "history" ? "Actual" : "Forecast"}
        </span>
      </div>
    `
    }
  `;
}

// ---------------------------------------------------------------------------
// Budget burndown chart
// ---------------------------------------------------------------------------

function BurndownChart({ data }) {
  const padding = { top: 20, right: 20, bottom: 40, left: 60 };
  const width = 720;
  const height = 340;
  const innerW = width - padding.left - padding.right;
  const innerH = height - padding.top - padding.bottom;

  const budget = Number(data.budget_amount);
  const current = data.current;
  const prior = data.prior || [];
  const predicted =
    data.predicted_landing != null ? Number(data.predicted_landing) : null;

  if (!current?.points?.length)
    return html`<p class="text-light">No spending data yet.</p>`;

  // Find max total_days across all series for X-axis normalization
  const maxTotalDays = Math.max(
    current.total_days,
    ...prior.map((s) => s.total_days),
  );

  // Y range: max of budget, current cumulative, predicted, and prior peaks
  const currentMax = Math.max(
    ...current.points.map((p) => Number(p.cumulative)),
  );
  const priorMax =
    prior.length > 0
      ? Math.max(
          ...prior.flatMap((s) => s.points.map((p) => Number(p.cumulative))),
        )
      : 0;
  const maxVal =
    Math.max(budget, currentMax, priorMax, predicted || 0) * 1.1 || 1;
  const minVal = 0;
  const valRange = maxVal - minVal;

  const x = (fraction) => padding.left + fraction * innerW;
  const y = (v) => padding.top + (1 - (v - minVal) / valRange) * innerH;

  // Normalize a series to 0-1 fraction of its own total_days
  const seriesPath = (points, totalDays) =>
    points
      .map((p, i) => {
        const frac = (p.day - 1) / (totalDays - 1 || 1);
        return `${i === 0 ? "M" : "L"}${x(frac)},${y(Number(p.cumulative))}`;
      })
      .join(" ");

  const currentLine = seriesPath(current.points, current.total_days);

  // Predicted dashed line from last current point to end
  const lastCurrent = current.points[current.points.length - 1];
  const lastFrac = (lastCurrent.day - 1) / (current.total_days - 1 || 1);
  const predictedLine =
    predicted != null
      ? `M${x(lastFrac)},${y(Number(lastCurrent.cumulative))} L${x(1)},${y(predicted)}`
      : "";

  // Ghost lines (prior months)
  const ghostLines = prior.map((s) => seriesPath(s.points, s.total_days));
  const ghostOpacities = [0.5, 0.35, 0.2];

  // Y-axis ticks
  const yTickCount = 5;
  const yTicks = Array.from({ length: yTickCount + 1 }, (_, i) => {
    const v = minVal + (valRange * i) / yTickCount;
    return { value: v, y: y(v) };
  });

  // X-axis ticks (day numbers)
  const dayLabels = [1];
  for (let d = 5; d <= maxTotalDays; d += 5) dayLabels.push(d);
  if (dayLabels[dayLabels.length - 1] !== maxTotalDays)
    dayLabels.push(maxTotalDays);
  const xTicks = dayLabels.map((d) => ({
    day: d,
    x: x((d - 1) / (current.total_days - 1 || 1)),
  }));

  // Hover state
  const [hover, setHover] = useState(null);
  const svgRef = useRef(null);

  const onMouseMove = useCallback(
    (e) => {
      const svg = svgRef.current;
      if (!svg) return;
      const rect = svg.getBoundingClientRect();
      const mx = ((e.clientX - rect.left) / rect.width) * width;
      const frac = (mx - padding.left) / innerW;

      // Find closest current point
      let closest = null;
      let closestDist = Infinity;
      for (const p of current.points) {
        const pFrac = (p.day - 1) / (current.total_days - 1 || 1);
        const dist = Math.abs(pFrac - frac);
        if (dist < closestDist) {
          closestDist = dist;
          closest = p;
        }
      }
      if (closest) setHover(closest);
    },
    [current],
  );

  const onMouseLeave = useCallback(() => setHover(null), []);

  return html`
    <svg
      ref=${svgRef}
      viewBox="0 0 ${width} ${height}"
      class="bd-chart"
      onMouseMove=${onMouseMove}
      onMouseLeave=${onMouseLeave}
    >
      ${yTicks.map(
        (t) => html`
          <line
            x1=${padding.left}
            y1=${t.y}
            x2=${width - padding.right}
            y2=${t.y}
            class="bd-grid-line"
          />
        `,
      )}
      <line
        x1=${padding.left}
        y1=${y(budget)}
        x2=${width - padding.right}
        y2=${y(budget)}
        class="bd-line-budget"
      />
      ${ghostLines.map(
        (path, i) => html`
          <path d=${path} class="bd-line-ghost" style="opacity:${ghostOpacities[i] || 0.2};stroke-width:1.5" />
        `,
      )}
      ${currentLine && html`<path d=${currentLine} class="bd-line-current" />`}
      ${predictedLine && html`<path d=${predictedLine} class="bd-line-predicted" />`}
      ${yTicks.map(
        (t) => html`
          <text x=${padding.left - 8} y=${t.y + 4} class="bd-axis-label" text-anchor="end">
            ${formatAmount(t.value, { decimals: 0 })}
          </text>
        `,
      )}
      ${xTicks.map(
        (t) => html`
          <text x=${t.x} y=${height - 8} class="bd-axis-label" text-anchor="middle">
            Day ${t.day}
          </text>
        `,
      )}
      ${
        hover &&
        html`
        <line
          x1=${x((hover.day - 1) / (current.total_days - 1 || 1))}
          y1=${padding.top}
          x2=${x((hover.day - 1) / (current.total_days - 1 || 1))}
          y2=${padding.top + innerH}
          class="bd-crosshair"
        />
        <circle
          cx=${x((hover.day - 1) / (current.total_days - 1 || 1))}
          cy=${y(Number(hover.cumulative))}
          r="4"
          class="bd-dot-current"
        />
      `
      }
    </svg>
    ${
      hover &&
      html`
      <div class="bd-tooltip hstack gap-4">
        <span class="text-light">Day ${hover.day}</span>
        <span style="font-weight:600">${formatAmount(hover.cumulative, { decimals: 2 })}</span>
        ${
          budget > 0 &&
          html`
          <span class="text-light" style="font-size:var(--text-8)">
            ${((Number(hover.cumulative) / budget) * 100).toFixed(0)}% of budget
          </span>
        `
        }
      </div>
    `
    }
  `;
}

// ---------------------------------------------------------------------------
// Insights page
// ---------------------------------------------------------------------------

function Insights({ categoryId: routeCategoryId }) {
  const [categories, setCategories] = useState(null);
  const [error, setError] = useState(null);
  const [selectedCat, setSelectedCat] = useState(routeCategoryId || "");
  const [burndown, setBurndown] = useState(null);
  const [bdLoading, setBdLoading] = useState(false);
  const [bdError, setBdError] = useState(null);

  function load() {
    api.get("/categories").then(setCategories).catch(setError);
  }

  useEffect(load, []);

  const catMap = useMemo(() => {
    if (!categories) return {};
    const map = {};
    for (const c of categories) map[c.id] = c;
    return map;
  }, [categories]);

  const monthlyCats = useMemo(() => {
    if (!categories) return [];
    return categories.filter((c) => {
      const mode = categoryBudgetMode(catMap, c.id);
      const type = categoryBudgetType(catMap, c.id);
      return mode === "monthly" && type === "variable";
    });
  }, [categories, catMap]);

  useEffect(() => {
    if (!selectedCat && monthlyCats.length > 0) {
      setSelectedCat(monthlyCats[0].id);
    }
  }, [monthlyCats, selectedCat]);

  useEffect(() => {
    if (!selectedCat) return;
    setBdLoading(true);
    setBdError(null);
    api
      .get(`/budgets/burndown?category_id=${selectedCat}`)
      .then((data) => {
        setBurndown(data);
        setBdLoading(false);
      })
      .catch((err) => {
        setBdError(err.message);
        setBdLoading(false);
      });
  }, [selectedCat]);

  if (error)
    return html`<${ErrorPanel} error=${error} onRetry=${() => {
      setError(null);
      load();
    }} />`;
  if (!categories) return html`<p class="text-light">Loading...</p>`;

  return html`
    <div class="vstack gap-4">
      <h2>Insights</h2>

      ${
        monthlyCats.length > 0
          ? html`
        <div class="card" style="padding:1.25rem">
          <div class="hstack gap-4" style="align-items:baseline;margin-bottom:1rem;flex-wrap:wrap">
            <h3 style="margin:0">Budget Burndown</h3>
            <div style="min-width:220px">
              <${CategorySelect}
                value=${selectedCat}
                onChange=${(id) => {
                  setSelectedCat(id);
                  navigateReplace(`/insights/${id}`);
                }}
                categories=${monthlyCats}
                catMap=${catMap}
                placeholder="Select category"
              />
            </div>
          </div>

          ${bdLoading && html`<progress></progress>`}
          ${bdError && html`<p class="text-light">${bdError}</p>`}
          ${
            burndown &&
            !bdLoading &&
            html`
            <div class="hstack gap-6" style="margin-bottom:1rem;flex-wrap:wrap">
              <div class="vstack gap-0">
                <span class="text-light" style="font-size:var(--text-8);text-transform:uppercase;letter-spacing:0.04em">Budget</span>
                <span style="font-size:var(--text-4);font-weight:600">
                  ${formatAmount(burndown.budget_amount, { decimals: 0 })}
                </span>
              </div>
              ${
                burndown.current?.points?.length > 0 &&
                html`
                <div class="vstack gap-0">
                  <span class="text-light" style="font-size:var(--text-8);text-transform:uppercase;letter-spacing:0.04em">Spent</span>
                  <span style="font-size:var(--text-4);font-weight:600;color:${Number(burndown.current.points[burndown.current.points.length - 1].cumulative) > Number(burndown.budget_amount) ? "var(--danger)" : "var(--foreground)"}">
                    ${formatAmount(burndown.current.points[burndown.current.points.length - 1].cumulative, { decimals: 0 })}
                  </span>
                </div>
              `
              }
              ${
                burndown.predicted_landing != null &&
                html`
                <div class="vstack gap-0">
                  <span class="text-light" style="font-size:var(--text-8);text-transform:uppercase;letter-spacing:0.04em">Predicted</span>
                  <span style="font-size:var(--text-4);font-weight:600;color:${Number(burndown.predicted_landing) > Number(burndown.budget_amount) ? "var(--danger)" : "var(--success)"}">
                    ${formatAmount(burndown.predicted_landing, { decimals: 0 })}
                  </span>
                </div>
              `
              }
            </div>
            <${BurndownChart} data=${burndown} />
          `
          }
        </div>
      `
          : html`<p class="text-light">No monthly variable categories yet. Create one on the Categories page to see burndown charts.</p>`
      }
    </div>
  `;
}

// ---------------------------------------------------------------------------
// Balances
// ---------------------------------------------------------------------------

function Balances() {
  const [netWorth, setNetWorth] = useState(null);
  const [accounts, setAccounts] = useState(null);
  const [projection, setProjection] = useState(null);
  const [error, setError] = useState(null);
  const [expandedAcct, setExpandedAcct] = useState(null);
  const [balanceHistory, setBalanceHistory] = useState({});
  const [historyLoading, setHistoryLoading] = useState(null);
  const [recordingAcct, setRecordingAcct] = useState(null);
  const [recordForm, setRecordForm] = useState({
    current: "",
    snapshot_at: "",
  });
  const [recordSaving, setRecordSaving] = useState(false);

  function load() {
    Promise.all([
      api.get("/accounts/net-worth"),
      api.get("/accounts"),
      api.get("/accounts/net-worth/projection?months=12&interval_width=0.8"),
    ])
      .then(([nw, accts, proj]) => {
        setNetWorth(nw);
        setAccounts(accts);
        setProjection(proj);
      })
      .catch(setError);
  }

  useEffect(load, []);

  function toggleAccount(id) {
    if (expandedAcct === id) {
      setExpandedAcct(null);
      return;
    }
    setExpandedAcct(id);
    if (!balanceHistory[id]) {
      setHistoryLoading(id);
      api
        .get(`/accounts/${id}/balances?limit=50`)
        .then((data) => {
          setBalanceHistory((prev) => ({ ...prev, [id]: data }));
          setHistoryLoading(null);
        })
        .catch(() => setHistoryLoading(null));
    }
  }

  async function recordBalance(accountId) {
    if (!recordForm.current) return;
    setRecordSaving(true);
    try {
      const body = { current: recordForm.current };
      if (recordForm.snapshot_at)
        body.snapshot_at = new Date(recordForm.snapshot_at).toISOString();
      await api.post(`/accounts/${accountId}/balances`, body);
      setRecordingAcct(null);
      setRecordForm({ current: "", snapshot_at: "" });
      // Refresh net worth and this account's history
      const [nw, hist] = await Promise.all([
        api.get("/accounts/net-worth"),
        api.get(`/accounts/${accountId}/balances?limit=50`),
      ]);
      setNetWorth(nw);
      setBalanceHistory((prev) => ({ ...prev, [accountId]: hist }));
    } catch (err) {
      setError(err);
    } finally {
      setRecordSaving(false);
    }
  }

  const manualAcctIds = useMemo(() => {
    if (!accounts) return new Set();
    return new Set(accounts.filter((a) => !a.connection_id).map((a) => a.id));
  }, [accounts]);

  const missingManualAccounts = useMemo(() => {
    if (!accounts || !netWorth) return [];
    const hasSnapshot = new Set(netWorth.accounts.map((a) => a.account_id));
    return accounts.filter((a) => !a.connection_id && !hasSnapshot.has(a.id));
  }, [accounts, netWorth]);

  const latestNetWorth = useMemo(() => {
    if (!projection?.history?.length) return null;
    const last = projection.history[projection.history.length - 1];
    return { date: last.date, value: Number(last.value) };
  }, [projection]);

  const forecastEnd = useMemo(() => {
    if (!projection?.forecast?.length) return null;
    const last = projection.forecast[projection.forecast.length - 1];
    return {
      date: last.date,
      value: last.value,
      lower: last.lower,
      upper: last.upper,
    };
  }, [projection]);

  if (error)
    return html`<${ErrorPanel} error=${error} onRetry=${() => {
      setError(null);
      load();
    }} />`;
  if (!netWorth) return html`<p class="text-light">Loading...</p>`;

  return html`
    <div class="vstack gap-4">
      <h2>Balances</h2>

      <!-- Net worth summary -->
      <div class="card" style="padding:1.25rem">
        <h3 style="margin:0 0 1rem">Net Worth</h3>
        <span style="font-size:var(--text-2);font-weight:700;color:${Number(netWorth.total) >= 0 ? "var(--success)" : "var(--danger)"}">
          ${formatAmount(netWorth.total, { decimals: 0 })}
        </span>

        ${
          netWorth.accounts.length > 0 &&
          html`
          <table style="width:100%;margin-top:1rem">
            <tbody>
              ${netWorth.accounts.map(
                (a) => html`
                <tr key=${a.account_id} style="cursor:pointer" onClick=${() => toggleAccount(a.account_id)}>
                  <td>
                    <span>${a.account_name}</span>
                    <span class="text-light" style="margin-left:0.5rem;font-size:var(--text-8)">${a.account_type}</span>
                  </td>
                  <td style="text-align:right;font-variant-numeric:tabular-nums;font-weight:500">
                    ${formatAmount(a.current, { decimals: 0 })}
                  </td>
                  <td class="text-light" style="text-align:right;width:8rem;font-size:var(--text-8)">
                    ${timeAgo(a.snapshot_at)}${" "}${a.is_manual && Date.now() - new Date(a.snapshot_at).getTime() > 7 * 86400_000 ? html`<span class="badge warning small">stale</span>` : ""}
                  </td>
                  <td style="text-align:right;width:2rem;color:var(--muted-foreground)">
                    ${expandedAcct === a.account_id ? "\u25B4" : "\u25BE"}
                  </td>
                </tr>
                ${
                  expandedAcct === a.account_id &&
                  html`
                  <tr>
                    <td colspan="4" style="padding:0.5rem 0 0.5rem 1rem">
                      ${
                        historyLoading === a.account_id
                          ? html`<progress></progress>`
                          : balanceHistory[a.account_id]?.length > 0
                            ? html`
                            <table style="width:100%;font-size:var(--text-8)">
                              <thead>
                                <tr class="text-light">
                                  <th style="text-align:left;font-weight:normal">Date</th>
                                  <th style="text-align:right;font-weight:normal">Balance</th>
                                </tr>
                              </thead>
                              <tbody>
                                ${balanceHistory[a.account_id].map(
                                  (s) => html`
                                  <tr key=${s.id}>
                                    <td>${formatDate(s.snapshot_at)}</td>
                                    <td style="text-align:right;font-variant-numeric:tabular-nums">
                                      ${formatAmount(s.current)}
                                    </td>
                                  </tr>
                                `,
                                )}
                              </tbody>
                            </table>
                          `
                            : html`<p class="text-light">No balance history.</p>`
                      }
                      ${
                        manualAcctIds.has(a.account_id) &&
                        html`
                        ${
                          recordingAcct === a.account_id
                            ? html`
                            <form class="hstack gap-2" style="margin-top:0.5rem;align-items:end" onSubmit=${(
                              e,
                            ) => {
                              e.preventDefault();
                              recordBalance(a.account_id);
                            }}>
                              <div class="vstack gap-0">
                                <label style="font-size:var(--text-8)" class="text-light">Amount</label>
                                <input
                                  type="number"
                                  step="0.01"
                                  required
                                  value=${recordForm.current}
                                  onInput=${(e) => setRecordForm((f) => ({ ...f, current: e.target.value }))}
                                  style="width:8rem"
                                />
                              </div>
                              <div class="vstack gap-0">
                                <label style="font-size:var(--text-8)" class="text-light">Date (optional)</label>
                                <input
                                  type="date"
                                  value=${recordForm.snapshot_at}
                                  onInput=${(e) => setRecordForm((f) => ({ ...f, snapshot_at: e.target.value }))}
                                />
                              </div>
                              <button data-variant="primary" data-compact disabled=${recordSaving}>Save</button>
                              <button type="button" data-compact onClick=${() => setRecordingAcct(null)}>Cancel</button>
                            </form>
                          `
                            : html`<button data-compact style="margin-top:0.5rem" onClick=${() => setRecordingAcct(a.account_id)}>Record Balance</button>`
                        }
                      `
                      }
                    </td>
                  </tr>
                `
                }
              `,
              )}
            </tbody>
          </table>
        `
        }
      </div>

      ${
        missingManualAccounts.length > 0 &&
        html`
        <div class="card" style="padding:1.25rem">
          <h3 style="margin:0 0 1rem">Manual Accounts</h3>
          <table style="width:100%">
            <tbody>
              ${missingManualAccounts.map(
                (a) => html`
                <tr key=${a.id}>
                  <td>
                    <span>${accountDisplayName(a)}</span>
                    <span class="text-light" style="margin-left:0.5rem;font-size:var(--text-8)">${a.account_type}</span>
                  </td>
                  <td style="text-align:right">
                    <span class="badge warning small">No balance recorded</span>
                  </td>
                  <td style="text-align:right;width:10rem">
                    ${
                      recordingAcct === a.id
                        ? html`
                        <form class="hstack gap-2" style="align-items:end;justify-content:flex-end" onSubmit=${(
                          e,
                        ) => {
                          e.preventDefault();
                          recordBalance(a.id);
                        }}>
                          <input
                            type="number"
                            step="0.01"
                            required
                            value=${recordForm.current}
                            onInput=${(e) => setRecordForm((f) => ({ ...f, current: e.target.value }))}
                            style="width:8rem"
                            placeholder="Amount"
                          />
                          <button data-variant="primary" data-compact disabled=${recordSaving}>Save</button>
                          <button type="button" data-compact onClick=${() => setRecordingAcct(null)}>Cancel</button>
                        </form>
                      `
                        : html`<button data-compact onClick=${() => setRecordingAcct(a.id)}>Record Balance</button>`
                    }
                  </td>
                </tr>
              `,
              )}
            </tbody>
          </table>
        </div>
      `
      }

      <!-- Net worth projection -->
      ${
        projection &&
        html`
        <div class="card" style="padding:1.25rem">
          <div class="hstack gap-4" style="align-items:baseline;margin-bottom:1rem;flex-wrap:wrap">
            <h3 style="margin:0">Projection</h3>
            ${projection?.message && html`<span class="badge" data-variant="warning">${projection.message}</span>`}
          </div>

          ${
            latestNetWorth &&
            html`
            <div class="hstack gap-6" style="margin-bottom:1rem;flex-wrap:wrap">
              <div class="vstack gap-0">
                <span class="text-light" style="font-size:var(--text-8);text-transform:uppercase;letter-spacing:0.04em">Current</span>
                <span style="font-size:var(--text-4);font-weight:600;color:${latestNetWorth.value >= 0 ? "var(--success)" : "var(--danger)"}">
                  ${formatAmount(latestNetWorth.value, { decimals: 0 })}
                </span>
                <span class="text-light" style="font-size:var(--text-8)">${formatDateShort(latestNetWorth.date)}</span>
              </div>
              ${
                forecastEnd &&
                html`
                <div class="vstack gap-0">
                  <span class="text-light" style="font-size:var(--text-8);text-transform:uppercase;letter-spacing:0.04em">12-month forecast</span>
                  <span style="font-size:var(--text-4);font-weight:600;color:${forecastEnd.value >= latestNetWorth.value ? "var(--success)" : "var(--danger)"}">
                    ${formatAmount(forecastEnd.value, { decimals: 0 })}
                  </span>
                  <span class="text-light" style="font-size:var(--text-8)">
                    ${formatAmount(forecastEnd.lower, { decimals: 0 })} – ${formatAmount(forecastEnd.upper, { decimals: 0 })}
                  </span>
                </div>
                <div class="vstack gap-0">
                  <span class="text-light" style="font-size:var(--text-8);text-transform:uppercase;letter-spacing:0.04em">Change</span>
                  <span style="font-size:var(--text-4);font-weight:600;color:${forecastEnd.value >= latestNetWorth.value ? "var(--success)" : "var(--danger)"}">
                    ${formatAmount(forecastEnd.value - latestNetWorth.value, { decimals: 0, sign: true })}
                  </span>
                </div>
              `
              }
            </div>
          `
          }

          <${NetWorthChart} data=${projection} />
        </div>
      `
      }
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
      await api.post("/login", { token });
      onLogin();
    } catch {
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
          placeholder="API token"
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
  const [authed, setAuthed] = useState(null); // null = checking
  const route = useRoute();

  // On mount, probe an authenticated endpoint to check if the cookie is valid
  useEffect(() => {
    api
      .get("/accounts")
      .then(() => setAuthed(true))
      .catch(() => setAuthed(false));
  }, []);

  if (authed === null) return null; // loading
  if (!authed) return html`<${Login} onLogin=${() => setAuthed(true)} />`;

  const page = () => {
    const segments = route.split("/").filter(Boolean);
    const [s0, s1] = segments;

    // Dashboard routes: /, /monthly, /monthly/:id, /annual, /projects
    if (!s0 || s0 === "monthly" || s0 === "annual" || s0 === "projects") {
      const tab = s0 || "monthly";
      const monthId = tab === "monthly" && s1 ? s1 : null;
      return html`<${Dashboard} tab=${tab} monthId=${monthId} />`;
    }
    if (s0 === "transactions") return html`<${Transactions} />`;
    if (s0 === "categories") return html`<${Categories} />`;
    if (s0 === "rules") return html`<${Rules} />`;
    if (s0 === "insights")
      return html`<${Insights} categoryId=${s1 || null} />`;
    if (s0 === "balances") return html`<${Balances} />`;
    if (s0 === "connections") return html`<${Connections} />`;
    if (s0 === "jobs") return html`<${Jobs} />`;
    return html`<p class="text-light">Not found.</p>`;
  };

  return html`
    <div data-sidebar-layout>
      <aside data-sidebar>
        <h1>Budget</h1>
        <nav>
          <${NavLink} href="/" match=${(r) => r === "/" || /^\/(monthly|annual|projects)(\/|$)/.test(r)}>Dashboard<//>
          <${NavLink} href="/transactions">Transactions<//>
          <${NavLink} href="/insights">Insights<//>
          <${NavLink} href="/balances">Balances<//>
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
            api.post("/logout").then(() => setAuthed(false));
          }}
          >Sign out</a
        >
      </aside>
      <main class="main">${page()}</main>
    </div>
  `;
}

render(html`<${App} />`, document.getElementById("app"));
