import htm from "https://cdn.jsdelivr.net/npm/htm@3/dist/htm.module.js";
import {
  h,
  render,
} from "https://cdn.jsdelivr.net/npm/preact@10/dist/preact.module.js";
import {
  useEffect,
  useState,
} from "https://cdn.jsdelivr.net/npm/preact@10/hooks/dist/hooks.module.js";

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
// Pages (stubs — each will grow into its own component tree)
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
                <span class="badge badge--${paceBadge(s.pace)}">${s.pace}</span>
              </td>
            </tr>
          `,
        )}
      </tbody>
    </table>
  `;
}

function paceBadge(pace) {
  if (pace === "under_budget") return "success";
  if (pace === "on_track") return "warning";
  return "danger";
}

function Transactions() {
  const [txns, setTxns] = useState(null);

  useEffect(() => {
    api.get("/transactions").then(setTxns);
  }, []);

  if (!txns) return html`<p class="muted">Loading...</p>`;

  return html`
    <h2>Transactions</h2>
    <table>
      <thead>
        <tr>
          <th>Date</th>
          <th>Merchant</th>
          <th>Description</th>
          <th>Amount</th>
          <th>Category</th>
        </tr>
      </thead>
      <tbody>
        ${txns.map(
          (t) => html`
            <tr>
              <td>${t.posted_date}</td>
              <td>${t.merchant_name ?? ""}</td>
              <td>${t.description ?? ""}</td>
              <td class="mono">${t.amount}</td>
              <td>${t.category_id ?? html`<span class="muted">--</span>`}</td>
            </tr>
          `,
        )}
      </tbody>
    </table>
  `;
}

function Categories() {
  return html`<h2>Categories</h2>
    <p class="muted">Coming soon.</p>`;
}

function Rules() {
  return html`<h2>Rules</h2>
    <p class="muted">Coming soon.</p>`;
}

function Budgets() {
  return html`<h2>Budget Periods</h2>
    <p class="muted">Coming soon.</p>`;
}

function Projects() {
  return html`<h2>Projects</h2>
    <p class="muted">Coming soon.</p>`;
}

function Connections() {
  return html`<h2>Connections</h2>
    <p class="muted">Coming soon.</p>`;
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
      await api.get("/budgets/status");
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
        ${error && html`<p style="color:var(--danger)">${error}</p>`}
        <input
          type="password"
          value=${token}
          onInput=${(e) => setToken(e.target.value)}
          placeholder="Bearer token"
          style="width:100%;margin-bottom:0.5rem"
        />
        <button class="primary" style="width:100%">Sign in</button>
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
    return html`<p class="muted">Not found.</p>`;
  };

  return html`
    <div class="shell">
      <nav class="nav">
        <h1>Budget</h1>
        <${NavLink} href="/">Dashboard<//>
        <${NavLink} href="/transactions">Transactions<//>
        <${NavLink} href="/categories">Categories<//>
        <${NavLink} href="/rules">Rules<//>
        <${NavLink} href="/budgets">Budgets<//>
        <${NavLink} href="/projects">Projects<//>
        <${NavLink} href="/connections">Connections<//>
        <a
          href="#"
          style="margin-top:auto;color:var(--muted)"
          onClick=${(e) => {
            e.preventDefault();
            localStorage.removeItem("budget_token");
            api.token = "";
            setAuthed(false);
          }}
          >Sign out</a
        >
      </nav>
      <main class="main">${page()}</main>
    </div>
  `;
}

render(html`<${App} />`, document.getElementById("app"));
