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
  if (n > 0) return `+${abs}`;
  if (n < 0) return `\u2212${abs}`;
  return abs;
}

function amountClass(amount) {
  const n = Number(amount);
  if (n > 0) return "amount-positive";
  if (n < 0) return "";
  return "";
}

function categoryName(catMap, id) {
  if (!id) return null;
  const cat = catMap[id];
  if (!cat) return null;
  if (cat.parent_id && catMap[cat.parent_id]) {
    return `${catMap[cat.parent_id].name} > ${cat.name}`;
  }
  return cat.name;
}

function paceBadge(pace) {
  if (pace === "under_budget") return "success";
  if (pace === "on_track") return "warning";
  return "danger";
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

function Dashboard() {
  const [status, setStatus] = useState(null);
  const [error, setError] = useState(null);

  useEffect(() => {
    api.get("/budgets/status").then(setStatus).catch(setError);
  }, []);

  if (error) return html`<p class="muted">${error.message}</p>`;
  if (!status) return html`<p class="muted">Loading...</p>`;

  return html`
    <h2>Budget Status</h2>
    <div class="table">
      <table>
        <thead>
          <tr>
            <th>Category</th>
            <th>Budget</th>
            <th>Spent</th>
            <th>Remaining</th>
            <th>Pace</th>
          </tr>
        </thead>
        <tbody>
          ${status.map(
            (s) => html`
              <tr>
                <td>${s.category_name ?? s.category_id}</td>
                <td class="mono">${s.budget_amount}</td>
                <td class="mono">${s.spent}</td>
                <td class="mono">${s.remaining}</td>
                <td>
                  <span class="badge ${paceBadge(s.pace)}">${s.pace}</span>
                </td>
              </tr>
            `,
          )}
        </tbody>
      </table>
    </div>
  `;
}

// ---------------------------------------------------------------------------
// Transactions
// ---------------------------------------------------------------------------

function Transactions() {
  const [txns, setTxns] = useState(null);
  const [categories, setCategories] = useState(null);
  const [accounts, setAccounts] = useState(null);
  const [error, setError] = useState(null);

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

  if (error) return html`<p class="muted">${error.message}</p>`;
  if (!txns) return html`<p class="muted">Loading...</p>`;

  const catMap = Object.fromEntries((categories ?? []).map((c) => [c.id, c]));
  const acctMap = Object.fromEntries((accounts ?? []).map((a) => [a.id, a]));

  if (txns.length === 0)
    return html`
      <h2>Transactions</h2>
      <p class="muted">
        No transactions yet. Connect an account and run a sync job to pull in
        data.
      </p>
    `;

  return html`
    <h2>Transactions</h2>
    <p class="muted" style="margin-bottom:1rem">
      ${txns.length} transaction${txns.length !== 1 ? "s" : ""}
    </p>
    <div class="table">
      <table>
        <thead>
          <tr>
            <th>Date</th>
            <th>Merchant</th>
            <th>Amount</th>
            <th>Category</th>
            <th>Account</th>
          </tr>
        </thead>
        <tbody>
          ${txns.map(
            (t) => html`
              <tr class=${t.correlation_type ? "row-correlated" : ""}>
                <td class="mono">${t.posted_date}</td>
                <td>
                  <span style="font-weight:500">${t.merchant_name || t.description}</span>
                  ${
                    t.description && t.merchant_name
                      ? html`<span class="muted" style="display:block;font-size:0.8rem">${t.description}</span>`
                      : null
                  }
                </td>
                <td class="mono ${amountClass(t.amount)}">${formatAmount(t.amount)}</td>
                <td>
                  ${
                    categoryName(catMap, t.category_id) ??
                    (t.suggested_category
                      ? html`<span class="badge" title="LLM suggestion">${t.suggested_category}</span>`
                      : html`<span class="badge secondary">uncategorized</span>`)
                  }
                  ${
                    t.correlation_type
                      ? html`<span class="badge">${t.correlation_type}</span>`
                      : null
                  }
                </td>
                <td class="muted">${acctMap[t.account_id]?.name ?? ""}</td>
              </tr>
            `,
          )}
        </tbody>
      </table>
    </div>
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
      load();
    } catch (err) {
      setError(err);
    }
  }

  async function acceptSuggestion(categoryName) {
    setAdding(true);
    try {
      const parts = categoryName.split(":");
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
        name: categoryName,
        parent_id: parentIdForNew,
      });
      load();
    } catch (err) {
      setError(err);
    } finally {
      setAdding(false);
    }
  }

  if (error) return html`<p class="muted">${error.message}</p>`;
  if (!categories) return html`<p class="muted">Loading...</p>`;

  const catMap = Object.fromEntries(categories.map((c) => [c.id, c]));
  const existingNames = new Set(categories.map((c) => c.name));
  const roots = categories.filter((c) => !c.parent_id || !catMap[c.parent_id]);
  const childrenOf = (pid) => categories.filter((c) => c.parent_id === pid);

  const rows = [];
  for (const root of roots) {
    rows.push({ ...root, depth: 0 });
    for (const child of childrenOf(root.id)) {
      rows.push({ ...child, depth: 1 });
    }
  }

  const pendingSuggestions = (suggestions ?? []).filter(
    (s) => !existingNames.has(s.category_name),
  );

  return html`
    <h2>Categories</h2>
    <p class="muted" style="margin-bottom:1rem">
      ${categories.length} categor${categories.length !== 1 ? "ies" : "y"}
    </p>

    ${
      pendingSuggestions.length > 0 &&
      html`
        <div style="margin-bottom:1.5rem">
          <h3>LLM Suggestions</h3>
          <p class="muted" style="margin-bottom:0.5rem">
            The LLM suggested these categories for uncategorized transactions.
            Accept to create the category, then re-run categorize.
          </p>
          <table>
            <thead>
              <tr>
                <th>Suggested Category</th>
                <th>Transactions</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              ${pendingSuggestions.map(
                (s) => html`
                  <tr>
                    <td><code>${s.category_name}</code></td>
                    <td class="mono">${s.count}</td>
                    <td style="text-align:right">
                      <button
                        data-variant="primary"
                        class="small"
                        onClick=${() => acceptSuggestion(s.category_name)}
                        disabled=${adding}
                      >
                        Accept
                      </button>
                    </td>
                  </tr>
                `,
              )}
            </tbody>
          </table>
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
      rows.length === 0
        ? html`<p class="muted">No categories yet. Add one above.</p>`
        : html`
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              ${rows.map(
                (c) => html`
                  <tr>
                    <td>
                      <span style="padding-left:${c.depth * 1.5}rem">
                        ${
                          c.depth > 0
                            ? html`<span class="muted" style="font-size:0.85rem;margin-right:0.25rem"
                              >${catMap[c.parent_id]?.name} ></span
                            > `
                            : null
                        }
                        ${c.name}
                      </span>
                    </td>
                    <td style="text-align:right">
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

  if (error) return html`<p class="muted">${error.message}</p>`;
  if (!rules) return html`<p class="muted">Loading...</p>`;

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

  const categorizationRules = rules.filter(
    (r) => r.rule_type === "categorization",
  );
  const correlationRules = rules.filter((r) => r.rule_type === "correlation");

  function typeBadge(ruleType) {
    if (ruleType === "categorization") return "success";
    return "secondary";
  }

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
          <div class="rules-row-main">
            <span class="mono" style="font-size:0.8rem;min-width:1.5rem;text-align:right">${rule.priority}</span>
            <span class="badge ${typeBadge(rule.rule_type)}"
              >${rule.rule_type}</span
            >
            <span class="muted">${fieldLabel(rule.match_field)}</span>
            <code class="">${rule.match_pattern}</code>
            <span class="muted">\u2192</span>
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
      <span class="muted">
        ${rules.length} rule${rules.length !== 1 ? "s" : ""}${" \u2014 "}
        ${categorizationRules.length} categorization, ${correlationRules.length} correlation
      </span>
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

    ${
      showForm &&
      html`
      <form style="border:1px solid var(--oat-border);border-radius:4px;padding:1rem;margin-bottom:1rem;display:flex;flex-direction:column;gap:0.75rem" onSubmit=${handleSubmit}>
        <div style="display:flex;flex-wrap:wrap;gap:0.5rem;align-items:center">${renderFormFields()}</div>
        <button data-variant="primary" type="submit" disabled=${saving}>
          Create Rule
        </button>
      </form>
    `
    }

    ${
      rules.length === 0
        ? html`<p class="muted" style="margin-top:1rem">
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
// Budgets (Budget Periods)
// ---------------------------------------------------------------------------

function Budgets() {
  const [periods, setPeriods] = useState(null);
  const [categories, setCategories] = useState(null);
  const [error, setError] = useState(null);
  const [formCategoryId, setFormCategoryId] = useState("");
  const [formPeriodType, setFormPeriodType] = useState("monthly");
  const [formAmount, setFormAmount] = useState("");
  const [editingId, setEditingId] = useState(null);
  const [submitting, setSubmitting] = useState(false);

  function load() {
    Promise.all([api.get("/budgets/periods"), api.get("/categories")])
      .then(([p, c]) => {
        setPeriods(p);
        setCategories(c);
      })
      .catch(setError);
  }

  useEffect(() => {
    load();
  }, []);

  function resetForm() {
    setFormCategoryId("");
    setFormPeriodType("monthly");
    setFormAmount("");
    setEditingId(null);
  }

  function startEdit(bp) {
    setEditingId(bp.id);
    setFormCategoryId(bp.category_id);
    setFormPeriodType(bp.period_type);
    setFormAmount(String(bp.amount));
  }

  async function handleSubmit(e) {
    e.preventDefault();
    setSubmitting(true);
    try {
      const body = {
        category_id: formCategoryId,
        period_type: formPeriodType,
        amount: formAmount,
      };
      if (editingId) {
        await api.put(`/budgets/periods/${editingId}`, body);
      } else {
        await api.post("/budgets/periods", body);
      }
      resetForm();
      load();
    } catch (err) {
      setError(err);
    } finally {
      setSubmitting(false);
    }
  }

  async function handleDelete(id) {
    try {
      await api.del(`/budgets/periods/${id}`);
      load();
    } catch (err) {
      setError(err);
    }
  }

  if (error) return html`<p class="muted">${error.message}</p>`;
  if (!periods || !categories) return html`<p class="muted">Loading...</p>`;

  const catMap = Object.fromEntries(categories.map((c) => [c.id, c]));

  return html`
    <h2>Budget Periods</h2>
    <p class="muted" style="margin-bottom:1rem">
      ${periods.length} budget period${periods.length !== 1 ? "s" : ""}
    </p>

    <form style="display:flex;gap:0.5rem;align-items:center;flex-wrap:wrap;margin-bottom:1rem" onSubmit=${handleSubmit}>
      <select
        value=${formCategoryId}
        onInput=${(e) => setFormCategoryId(e.target.value)}
        required
      >
        <option value="" disabled>Category</option>
        ${categories.map(
          (c) =>
            html`<option value=${c.id}>${categoryName(catMap, c.id)}</option>`,
        )}
      </select>
      <select
        value=${formPeriodType}
        onInput=${(e) => setFormPeriodType(e.target.value)}
      >
        <option value="monthly">Monthly</option>
        <option value="annual">Annual</option>
      </select>
      <input
        type="number"
        step="0.01"
        min="0"
        placeholder="Amount"
        value=${formAmount}
        onInput=${(e) => setFormAmount(e.target.value)}
        required
      />
      <button data-variant="primary" type="submit" disabled=${submitting}>
        ${editingId ? "Update" : "Add Budget"}
      </button>
      ${
        editingId &&
        html`<button type="button" onClick=${resetForm}>Cancel</button>`
      }
    </form>

    ${
      periods.length === 0
        ? html`<p class="muted">No budget periods yet. Add one above.</p>`
        : html`
          <table>
            <thead>
              <tr>
                <th>Category</th>
                <th>Period</th>
                <th>Amount</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              ${periods.map(
                (bp) => html`
                  <tr>
                    <td>${categoryName(catMap, bp.category_id)}</td>
                    <td>
                      <span class="badge ${bp.period_type === "monthly" ? "" : "secondary"}">
                        ${bp.period_type}
                      </span>
                    </td>
                    <td class="mono">${Number(bp.amount).toFixed(2)}</td>
                    <td>
                      <button class="small" onClick=${() => startEdit(bp)}>Edit</button>
                      <button data-variant="danger" class="small" onClick=${() => handleDelete(bp.id)}>
                        Delete
                      </button>
                    </td>
                  </tr>
                `,
              )}
            </tbody>
          </table>
        `
    }
  `;
}

// ---------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------

function Projects() {
  const [projects, setProjects] = useState(null);
  const [categories, setCategories] = useState(null);
  const [error, setError] = useState(null);
  const emptyForm = {
    name: "",
    category_id: "",
    start_date: "",
    end_date: "",
    budget_amount: "",
  };
  const [form, setForm] = useState(emptyForm);
  const [editingId, setEditingId] = useState(null);
  const [submitting, setSubmitting] = useState(false);

  function load() {
    Promise.all([api.get("/projects"), api.get("/categories")])
      .then(([p, c]) => {
        setProjects(p);
        setCategories(c);
      })
      .catch(setError);
  }

  useEffect(() => {
    load();
  }, []);

  const catMap = Object.fromEntries((categories ?? []).map((c) => [c.id, c]));

  function setField(field, value) {
    setForm((prev) => ({ ...prev, [field]: value }));
  }

  function startEdit(project) {
    setEditingId(project.id);
    setForm({
      name: project.name,
      category_id: project.category_id,
      start_date: project.start_date,
      end_date: project.end_date ?? "",
      budget_amount: project.budget_amount ?? "",
    });
  }

  function cancelEdit() {
    setEditingId(null);
    setForm(emptyForm);
  }

  async function handleSubmit(e) {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    const body = {
      name: form.name,
      category_id: form.category_id,
      start_date: form.start_date,
      end_date: form.end_date || null,
      budget_amount: form.budget_amount || null,
    };
    try {
      if (editingId) {
        await api.put(`/projects/${editingId}`, body);
      } else {
        await api.post("/projects", body);
      }
      setForm(emptyForm);
      setEditingId(null);
      load();
    } catch (err) {
      setError(err);
    } finally {
      setSubmitting(false);
    }
  }

  async function handleDelete(id) {
    setError(null);
    try {
      await api.del(`/projects/${id}`);
      if (editingId === id) cancelEdit();
      load();
    } catch (err) {
      setError(err);
    }
  }

  function isCompleted(project) {
    if (!project.end_date) return false;
    return project.end_date < new Date().toISOString().slice(0, 10);
  }

  if (error && !projects) return html`<p class="muted">${error.message}</p>`;
  if (!projects) return html`<p class="muted">Loading...</p>`;

  return html`
    <h2>Projects</h2>
    <p class="muted" style="margin-bottom:1rem">
      ${projects.length} project${projects.length !== 1 ? "s" : ""}
    </p>

    ${error && html`<p role="alert" data-variant="error">${error.message}</p>`}

    <form style="margin-bottom:1.5rem" onSubmit=${handleSubmit}>
      <div style="display:flex;gap:0.5rem;flex-wrap:wrap;align-items:center">
        <input
          type="text"
          placeholder="Project name"
          value=${form.name}
          onInput=${(e) => setField("name", e.target.value)}
          required
        />
        <select
          value=${form.category_id}
          onChange=${(e) => setField("category_id", e.target.value)}
          required
        >
          <option value="" disabled>Category</option>
          ${(categories ?? []).map(
            (c) =>
              html`<option value=${c.id}>${categoryName(catMap, c.id) ?? c.name}</option>`,
          )}
        </select>
        <input
          type="date"
          value=${form.start_date}
          onInput=${(e) => setField("start_date", e.target.value)}
          required
          title="Start date"
        />
        <input
          type="date"
          value=${form.end_date}
          onInput=${(e) => setField("end_date", e.target.value)}
          title="End date (optional)"
        />
        <input
          type="number"
          step="0.01"
          min="0"
          placeholder="Budget"
          value=${form.budget_amount}
          onInput=${(e) => setField("budget_amount", e.target.value)}
          title="Budget amount (optional)"
        />
        <button data-variant="primary" type="submit" disabled=${submitting}>
          ${editingId ? "Update" : "Add"}
        </button>
        ${
          editingId &&
          html`<button type="button" onClick=${cancelEdit}>Cancel</button>`
        }
      </div>
    </form>

    ${
      projects.length === 0
        ? html`<p class="muted">No projects yet. Add one above.</p>`
        : html`
        <div class="table">
          <table>
            <thead>
              <tr>
                <th>Name</th>
                <th>Category</th>
                <th>Start</th>
                <th>End</th>
                <th>Budget</th>
                <th>Actions</th>
              </tr>
            </thead>
            <tbody>
              ${projects.map(
                (p) => html`
                <tr class=${isCompleted(p) ? "project-completed" : ""}>
                  <td><span style="font-weight:500">${p.name}</span></td>
                  <td>${categoryName(catMap, p.category_id) ?? html`<span class="muted">unknown</span>`}</td>
                  <td class="mono">${p.start_date}</td>
                  <td>
                    ${
                      p.end_date
                        ? html`<span class="mono">${p.end_date}</span>`
                        : html`<span class="badge success">ongoing</span>`
                    }
                  </td>
                  <td>
                    ${
                      p.budget_amount != null
                        ? html`<span class="mono">${Number(p.budget_amount).toFixed(2)}</span>`
                        : html`<span class="badge secondary">no budget</span>`
                    }
                  </td>
                  <td>
                    <button class="small" onClick=${() => startEdit(p)}>Edit</button>
                    <button data-variant="danger" class="small" onClick=${() => handleDelete(p.id)}>Delete</button>
                  </td>
                </tr>
              `,
              )}
            </tbody>
          </table>
        </div>
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

  if (error) return html`<p class="muted">${error.message}</p>`;
  if (!connections) return html`<p class="muted">Loading...</p>`;

  return html`
    <h2>Connections</h2>

    ${
      connections.length === 0
        ? html`<p class="muted" style="margin-bottom:1.5rem">
            No bank connections yet. Search for your bank below to get started.
          </p>`
        : html`
            <p class="muted" style="margin-bottom:1rem">
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
                        <span class="badge ${statusBadge(c.status)}">${c.status}</span>
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
                  : html`<p class="muted">No banks found matching your search.</p>`
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

function Jobs() {
  const [jobs, setJobs] = useState(null);
  const [counts, setCounts] = useState(null);
  const [accounts, setAccounts] = useState(null);
  const [error, setError] = useState(null);
  const [syncAccountId, setSyncAccountId] = useState("");
  const [triggering, setTriggering] = useState(null);

  function loadJobs() {
    Promise.all([api.get("/jobs"), api.get("/accounts")])
      .then(([j, a]) => {
        setJobs(j);
        setAccounts(a);
      })
      .catch(setError);
  }

  function loadCounts() {
    api
      .get("/jobs/counts")
      .then(setCounts)
      .catch(() => {});
  }

  useEffect(() => {
    loadJobs();
    loadCounts();
    const interval = setInterval(loadCounts, 5000);
    return () => clearInterval(interval);
  }, []);

  async function trigger(path, name) {
    setTriggering(name);
    setError(null);
    try {
      await api.post(path);
      loadJobs();
      loadCounts();
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

  function statusBadge(status) {
    if (status === "Done") return "success";
    if (status === "Failed" || status === "Killed") return "danger";
    if (status === "Running") return "primary";
    return "";
  }

  const PIPELINE_STEPS = ["Sync", "Categorize", "Correlate", "Recompute"];

  function friendlyType(job) {
    const name = job.job_type.includes("::")
      ? job.job_type.split("::").pop()
      : job.job_type;
    if (name === "SyncJob") return "Sync";
    if (name === "CategorizeJob") return "Categorize";
    if (name === "CategorizeTransactionJob") return "Categorize (txn)";
    if (name === "CorrelateJob") return "Correlate";
    if (name === "CorrelateTransactionJob") return "Correlate (txn)";
    if (name === "BudgetRecomputeJob") return "Recompute";
    if (name === "Vec<u8>" || job.job_type.includes("Vec<u8>")) {
      const step = PIPELINE_STEPS[job.pipeline_step] ?? "?";
      return `Pipeline / ${step}`;
    }
    return name;
  }

  function formatTs(iso) {
    if (!iso) return "\u2014";
    const d = new Date(iso);
    return d.toLocaleString();
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
            ? html`<span class="badge danger">${c.failed} failed</span>`
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
        <div class="queue-stat-row">
          <div class="queue-stat">
            <span class="queue-stat-label">Active</span>
            <span class="queue-stat-value">${c.active}</span>
          </div>
          <div class="queue-stat">
            <span class="queue-stat-label">Waiting</span>
            <span class="queue-stat-value">${c.waiting}</span>
          </div>
        </div>
      </div>
    `;
  }

  if (error && !jobs) return html`<p class="muted">${error.message}</p>`;
  if (!jobs) return html`<p class="muted">Loading...</p>`;

  return html`
    <h2>Jobs</h2>

    ${error && html`<p role="alert" data-variant="error">${error.message}</p>`}

    <div class="queue-cards">
      ${QUEUE_CARDS.map(renderQueueCard)}
    </div>

    <div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:1rem">
      <span class="muted">
        ${jobs.length} job${jobs.length !== 1 ? "s" : ""} (latest 100)
      </span>
      <button class="small" onClick=${loadJobs}>Refresh</button>
    </div>

    ${
      jobs.length === 0
        ? html`<p class="muted">No jobs yet. Trigger one above to get started.</p>`
        : html`
          <div class="table">
            <table>
              <thead>
                <tr>
                  <th>ID</th>
                  <th>Type</th>
                  <th>Status</th>
                  <th>Attempts</th>
                  <th>Queued</th>
                  <th>Completed</th>
                  <th>Result</th>
                </tr>
              </thead>
              <tbody>
                ${jobs.map(
                  (j) => html`
                    <tr>
                      <td class="mono" title=${j.id}>${j.id.slice(0, 8)}</td>
                      <td>${friendlyType(j)}</td>
                      <td>
                        <span class="badge ${statusBadge(j.status)}">${j.status}</span>
                      </td>
                      <td class="mono">${j.attempts}/${j.max_attempts}</td>
                      <td class="mono" style="font-size:0.85rem">${formatTs(j.run_at)}</td>
                      <td class="mono" style="font-size:0.85rem">${formatTs(j.done_at)}</td>
                      <td>
                        ${
                          j.last_result
                            ? html`<span class="muted" style="font-size:0.85rem" title=${j.last_result}>
                                ${j.last_result.length > 60 ? j.last_result.slice(0, 60) + "..." : j.last_result}
                              </span>`
                            : null
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
        <p class="muted" style="margin-bottom:1rem">Enter your API token.</p>
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
    if (route === "/budgets") return html`<${Budgets} />`;
    if (route === "/projects") return html`<${Projects} />`;
    if (route === "/connections") return html`<${Connections} />`;
    if (route === "/jobs") return html`<${Jobs} />`;
    return html`<p class="muted">Not found.</p>`;
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
          <${NavLink} href="/budgets">Budgets<//>
          <${NavLink} href="/projects">Projects<//>
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
