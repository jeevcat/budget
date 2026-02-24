import { useEffect, useState } from "preact/hooks";
import { api, html } from "../app.js";

function emptyForm() {
  return {
    name: "",
    category_id: "",
    start_date: "",
    end_date: "",
    budget_amount: "",
  };
}

function formatAmount(amount) {
  return Number(amount).toFixed(2);
}

function isCompleted(project) {
  if (!project.end_date) return false;
  return project.end_date < new Date().toISOString().slice(0, 10);
}

export function Projects() {
  const [projects, setProjects] = useState(null);
  const [categories, setCategories] = useState(null);
  const [error, setError] = useState(null);
  const [form, setForm] = useState(emptyForm());
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

  function categoryName(id) {
    if (!id) return null;
    const cat = catMap[id];
    if (!cat) return null;
    if (cat.parent_id && catMap[cat.parent_id]) {
      return `${catMap[cat.parent_id].name} > ${cat.name}`;
    }
    return cat.name;
  }

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
    setForm(emptyForm());
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
      setForm(emptyForm());
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

  if (error && !projects) return html`<p class="muted">${error.message}</p>`;
  if (!projects) return html`<p class="muted">Loading...</p>`;

  return html`
    <h2>Projects</h2>
    <p class="muted" style="margin-bottom:1rem">
      ${projects.length} project${projects.length !== 1 ? "s" : ""}
      ${" \u2014 "}A project is a time-bound budget that operates outside the monthly/annual cycle.
    </p>

    ${error && html`<p style="color:var(--danger);margin-bottom:1rem">${error.message}</p>`}

    <form class="project-form" onSubmit=${handleSubmit}>
      <div class="project-form-row">
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
              html`<option value=${c.id}>${categoryName(c.id) ?? c.name}</option>`,
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
          placeholder="End date"
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
        <button class="primary" type="submit" disabled=${submitting}>
          ${editingId ? "Update" : "Add"}
        </button>
        ${
          editingId &&
          html`
          <button type="button" onClick=${cancelEdit}>Cancel</button>
        `
        }
      </div>
    </form>

    ${
      projects.length === 0
        ? html`<p class="muted" style="margin-top:1rem">No projects yet. Add one above.</p>`
        : html`
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
                <td>
                  <span class="project-name">${p.name}</span>
                </td>
                <td>${categoryName(p.category_id) ?? html`<span class="muted">unknown</span>`}</td>
                <td class="mono">${p.start_date}</td>
                <td>
                  ${
                    p.end_date
                      ? html`<span class="mono">${p.end_date}</span>`
                      : html`<span class="badge badge--success">ongoing</span>`
                  }
                </td>
                <td>
                  ${
                    p.budget_amount != null
                      ? html`<span class="mono">${formatAmount(p.budget_amount)}</span>`
                      : html`<span class="badge badge--muted">no budget</span>`
                  }
                </td>
                <td class="project-actions">
                  <button onClick=${() => startEdit(p)} title="Edit project">Edit</button>
                  <button class="danger" onClick=${() => handleDelete(p.id)} title="Delete project">Delete</button>
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
