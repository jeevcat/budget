import { useEffect, useState } from "preact/hooks";
import { api, html } from "../app.js";

function Budgets() {
  const [periods, setPeriods] = useState(null);
  const [categories, setCategories] = useState(null);
  const [error, setError] = useState(null);

  // Form state for add/edit
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

  function categoryName(id) {
    const cat = catMap[id];
    if (!cat) return id;
    if (cat.parent_id && catMap[cat.parent_id]) {
      return `${catMap[cat.parent_id].name} > ${cat.name}`;
    }
    return cat.name;
  }

  return html`
    <h2>Budget Periods</h2>
    <p class="muted" style="margin-bottom:1rem">
      ${periods.length} budget period${periods.length !== 1 ? "s" : ""}
    </p>

    <form class="budget-form" onSubmit=${handleSubmit}>
      <select
        value=${formCategoryId}
        onInput=${(e) => setFormCategoryId(e.target.value)}
        required
      >
        <option value="" disabled>Category</option>
        ${categories.map(
          (c) => html`<option value=${c.id}>${categoryName(c.id)}</option>`,
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

      <button class="primary" type="submit" disabled=${submitting}>
        ${editingId ? "Update" : "Add Budget"}
      </button>
      ${
        editingId &&
        html`<button type="button" onClick=${resetForm}>Cancel</button>`
      }
    </form>

    ${
      periods.length === 0
        ? html`<p class="muted">
          No budget periods yet. Add one above.
        </p>`
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
                    <td>${categoryName(bp.category_id)}</td>
                    <td>
                      <span
                        class="badge ${
                          bp.period_type === "monthly"
                            ? "badge--info"
                            : "badge--warning"
                        }"
                      >
                        ${bp.period_type}
                      </span>
                    </td>
                    <td class="mono">${Number(bp.amount).toFixed(2)}</td>
                    <td class="budget-actions">
                      <button onClick=${() => startEdit(bp)}>Edit</button>
                      <button class="danger" onClick=${() => handleDelete(bp.id)}>
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

export { Budgets };
