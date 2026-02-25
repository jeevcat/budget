# Oat CSS Reference

Scraped from https://oat.ink — regenerate with `./frontend/gen-oat-reference.py`


---

<!-- source: https://oat.ink/usage/ -->

# Installation and usage

> **IMPORTANT:** The lib is currently sub v1 and is likely to have breaking changes until it hits v1.

### CDN

Include the CSS and JS files directly in your HTML:

```
<link rel="stylesheet" href="https://unpkg.com/@knadh/oat/oat.min.css">
<script src="https://unpkg.com/@knadh/oat/oat.min.js" defer></script>
```

---

### npm

```
npm install @knadh/oat
```

Then import in your project:

```
import '@knadh/oat/oat.min.css';
import '@knadh/oat/oat.min.js';
```

Or import individual files from `@knadh/oat/css` and `@knadh/oat/js`.

---

### Download

Download the CSS and JS files:

```
wget https://raw.githubusercontent.com/knadh/oat/refs/heads/gh-pages/oat.min.css
wget https://raw.githubusercontent.com/knadh/oat/refs/heads/gh-pages/oat.min.js
```

Then include them in your project:

```
<link rel="stylesheet" href="./oat.min.css">
<script src="./oat.min.js" defer></script>
```

## Basic usage

Oat styles semantic HTML elements by default. No classes needed for basic styling:

```
<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>My App</title>
  <link rel="stylesheet" href="oat.css">
  <script src="oat.js" defer></script>
</head>
<body>
  <h1>Hello World</h1>
  <p>This paragraph is styled automatically.</p>
  <button>Click me</button>
</body>
</html>
```

---

# Local dev setup

### Requirements

* [zola](https://github.com/getzola/zola/releases) static site generator installed to preview the docs/demo site and to try out changes.
* [esbuild](https://esbuild.github.io/) installed for bundling+minifying JS and CSS.

### Running

* Clone the [oat repo](https://github.com/knadh/oat)
* `cd docs` and run `zola serve` to access the docs/demo site at http://localhost:1111
* After changing any CSS or JS files, run `make dist`. The demo site auto-updates with the changes.

© [Kailash Nadh](https://nadh.in)


---

<!-- source: https://oat.ink/customizing/ -->

# Customizing

Pretty much all properties of Oat are defined as CSS variables that can be overridden. See [theme.css](https://github.com/knadh/oat/blob/master/src/css/01-theme.css) to see all variables. To override, redefine them in a CSS file in your project and include it after the lib's CSS files.

## Picking and choosing

While it is quite okay to bundle all of Oat given how tiny it is (CSS %KB, JS %KB), it is possible to include components selectively.

##### Must include

* `00-base.css`
* `01-theme.css`
* `base.js`
* `your files after this`

---

## Theming

The following color variables from theme.css control the theme (colour profile). Override them to create your own theme.

```
:root {

  /* Page background */
  --background: rgb(255 255 255);

  /* Primary text color */
  --foreground: rgb(9 9 11);

  /* Card background */
  --card: rgb(255 255 255);

  /* Card text color */
  --card-foreground: rgb(9 9 11);

  /* Primary buttons and links */
  --primary: rgb(24 24 27);

  /* Text color on primary buttons */
  --primary-foreground: rgb(250 250 250);

  /* Secondary button background */
  --secondary: rgb(244 244 245);

  /* Text colour on secondary buttons */
  --secondary-foreground: rgb(24 24 27);

  /* Muted (lighter) background */
  --muted: rgb(244 244 245);

  /* Muted (lighter) text colour */
  --muted-foreground: rgb(113 113 122);

  /* Subtler than muted background */
  --faint: rgb(250 250 250);

  /* Subtler than muted text color */
  --faint-foreground: rgb(161 161 170);

  /* Accent background */
  --accent: rgb(244 244 245);

  /* Accent text color */
  --accent-foreground: rgb(24 24 27);

  /* Error/danger color */
  --danger: rgb(223 81 76);

  /* Text color on danger background */
  --danger-foreground: rgb(250 250 250);

  /* Success color */
  --success: rgb(76 175 80);

  /* Text colour on success background */
  --success-foreground: rgb(250 250 250);

  /* Warning color */
  --warning: rgb(255 140 0);

  /* Text colour on warning background */
  --warning-foreground: rgb(9 9 11);

  /* Border color (boxes) */
  --border: rgb(212 212 216);

  /* Input borders */
  --input: rgb(212 212 216);

  /* Focus ring color */
  --ring: rgb(24 24 27);
}
```

After these, include CSS and JS files the respective components.

## Example themes

### Default Oat brown

```
--background: #fff;
--foreground: #09090b;
--card: #fff;
--card-foreground: #09090b;
--primary: #574747;
--primary-foreground: #fafafa;
--secondary: #f4f4f5;
--secondary-foreground: #574747;
--muted: #f4f4f5;
--muted-foreground: #71717a;
--faint: #fafafa;
--accent: #f4f4f5;
--danger: #df514c;
--danger-foreground: #fafafa;
--success: #4caf50;
--success-foreground: #fafafa;
--warning: #ff8c00;
--warning-foreground: #09090b;
--border: #d4d4d8;
--input: #d4d4d8;
--ring: #574747;
```

---

## Dark mode

Adding `data-theme="dark"` to `<body>` applies the dark theme. Customize the dark theme by redefining the aforementioned theme variables and scoping them inside `[data-theme="dark"] { ... }`

© [Kailash Nadh](https://nadh.in)


---

<!-- source: https://oat.ink/components/ -->

# Components

Oat is an ultra-lightweight HTML + CSS + minimal JS, semantic UI component library with zero dependencies.
No framework or build or dev dependencies of any kind. Just include the tiny CSS and JS bundles.

Semantic tags and attributes are styled contextually out of the box without classes, thereby forcing best practices.
A few dynamic components are WebComponents.

---

## Typography

Base text elements are styled automatically. No classes needed.

```
<h1>Heading 1</h1>
<h2>Heading 2</h2>
<h3>Heading 3</h3>
<h4>Heading 4</h4>
<h5>Heading 5</h5>
<h6>Heading 6</h6>

<p>This is a paragraph with <strong>bold text</strong>, <em>italic text</em>, and <a href="#">a link</a>.</p>

<p>Here's some <code>inline code</code> and a code block:</p>

<pre><code>function hello() {
  console.log('Hello, World!');
}</code></pre>

<blockquote>
  This is a blockquote. It's styled automatically.
</blockquote>

<hr>

<ul>
  <li>Unordered list item 1</li>
  <li>Unordered list item 2</li>
  <li>Unordered list item 3</li>
</ul>

<ol>
  <li>Ordered list item 1</li>
  <li>Ordered list item 2</li>
  <li>Ordered list item 3</li>
</ol>
```

[Link](/components/typography/)

## Accordion

Use native `<details>` and `<summary>` for collapsible content.

```
<details>
  <summary>What is Oat</summary>
  <p>Oat is a minimal, semantic-first UI component library with zero dependencies.</p>
</details>

<details>
  <summary>How do I use it</summary>
  <p>Include the CSS and JS files, then write semantic HTML. Most elements are styled by default.</p>
</details>

<details>
  <summary>Is it accessible</summary>
  <p>Yes! It uses semantic HTML and ARIA attributes. Keyboard navigation works out of the box.</p>
</details>

<details name="same">
  <summary>This is grouped with the next one</summary>
  <p>Using the <code>name</code> attribute groups items like radio.</p>
</details>

<details name="same">
  <summary>This is grouped with the previous one</summary>
  <p>Using the <code>name</code> attribute groups items like radio.</p>
</details>
```

[Link](/components/accordion/)

## Alert

Use `role="alert"` for alert styling. Set `data-variant` for success, warning, or error.

```
<div role="alert">
  <strong>Default Alert</strong> This is a default alert message.
</div>

<div role="alert" data-variant="success">
  <strong>Success!</strong> Your changes have been saved.
</div>

<div role="alert" data-variant="warning">
  <strong>Warning!</strong> Please review before continuing.
</div>

<div role="alert" data-variant="error">
  <strong>Error!</strong> Something went wrong.
</div>
```

[Link](/components/alert/)

## Badge

Use `.badge` class with variant modifiers.

```
<span class="badge">Default</span>
<span class="badge secondary">Secondary</span>
<span class="badge outline">Outline</span>
<span class="badge success">Success</span>
<span class="badge warning">Warning</span>
<span class="badge danger">Danger</span>
```

[Link](/components/badge/)

## Button

The `<button>` element is styled by default. Use `data-variant="primary|secondary|danger"` for semantic variants and classes for visual styles.

```
<button>Primary</button>
<button data-variant="secondary">Secondary</button>
<button data-variant="danger">Danger</button>
<button class="outline">Outline</button>
<button data-variant="danger" class="outline">Danger</button>
<button class="ghost">Ghost</button>
<button disabled>Disabled</button>
```

### Sizes

Use `.small` or `.large` for size variants.

```
<button class="small">Small</button>
<button>Default</button>
<button class="large">Large</button>
<a href="#button" class="button">Hyperlink</a>
```

### Button group

Wrap buttons in `<menu class="buttons">` for connected buttons.

```
<menu class="buttons">
  <li><button class="outline">Left</button></li>
  <li><button class="outline">Center</button></li>
  <li><button class="outline">Right</button></li>
</menu>
```

[Link](/components/button/)

## Card

Use `class="card"` for a visual box-like card look.

```
<article class="card">
  <header>
    <h3>Card Title</h3>
    <p>Card description goes here.</p>
  </header>
  <p>This is the card content. It can contain any HTML.</p>
  <footer class="flex gap-2 mt-4">
    <button class="outline">Cancel</button>
    <button>Save</button>
  </footer>
</article>
```

[Link](/components/card/)

## Dialog

Fully semantic, zero-Javascript, dynamic dialog with `<dialog>`. Use `commandfor` and `command="show-modal"` attributes on an element to open a target dialog. Focus trapping, z placement, keyboard shortcuts all work out of the box.

```
<button commandfor="demo-dialog" command="show-modal">Open dialog</button>
<dialog id="demo-dialog" closedby="any">
  <form method="dialog">
    <header>
      <h3>Title</h3>
      <p>This is a dialog description.</p>
    </header>
    <div>
      <p>Dialog content goes here. You can put any HTML inside.</p>
      <p>Click outside or press Escape to close.</p>
    </div>
    <footer>
      <button type="button" commandfor="demo-dialog" command="close" class="outline">Cancel</button>
      <button value="confirm">Confirm</button>
    </footer>
  </form>
</dialog>
```

### With form fields

Forms inside dialogs work naturally. Use `command="close"` on cancel buttons to close.

```
<button commandfor="demo-dialog-form" command="show-modal">Open form dialog</button>
<dialog id="demo-dialog-form">
  <form method="dialog">
    <header>
      <h3>Edit form</h3>
    </header>
    <div class="vstack">
      <label>Name <input name="name" required></label>
      <label>Email <input name="email" type="email"></label>
    </div>
    <footer>
      <button type="button" commandfor="demo-dialog-form" command="close" class="outline">Cancel</button>
      <button value="save">Save</button>
    </footer>
  </form>
</dialog>
```

### Handling return value

Listen to the native `close` event to get the button value:

```
const dialog = document.querySelector("#demo-dialog");
dialog.addEventListener('close', (e) => {
  console.log(dialog.returnValue); // "confirm"
});
```

or use `onclose` inline:

```
<dialog id="my-dialog" onclose="console.log(this.returnValue)">
```

[Link](/components/dialog/)

## Dropdown

Wrap in `<ot-dropdown>`. Use `popovertarget` on the trigger and `popover` on the target. If a dropdown `<menu>`, items use `role="menuitem"`.

```
<ot-dropdown>
  <button popovertarget="demo-menu" class="outline">
    Options
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="m6 9 6 6 6-6" /></svg>
  </button>
  <menu popover id="demo-menu">
    <button role="menuitem">Profile</button>
    <button role="menuitem">Settings</button>
    <button role="menuitem">Help</button>
    <hr>
    <button role="menuitem">Logout</button>
  </menu>
</ot-dropdown>
```

### Popover

`<ot-dropdown>` can also be used to show popover dropdown elements.

```
<ot-dropdown>
  <button popovertarget="demo-confirm" class="outline">
    Confirm
  </button>
  <article class="card" popover id="demo-confirm">
    <header>
      <h4>Are you sure?</h4>
      <p>This action cannot be undone.</p>
    </header>
    <br />
    <footer>
      <button class="outline small" popovertarget="demo-confirm">Cancel</button>
      <button data-variant="danger" class="small" popovertarget="demo-confirm">Delete</button>
    </footer>
  </article>
</ot-dropdown>
```

[Link](/components/dropdown/)

## Form elements

Form elements are styled automatically. Wrap inputs in `<label>` for proper association.

```
<form>
  <label data-field>
    Name
    <input type="text" placeholder="Enter your name" />
  </label>

  <label data-field>
    Email
    <input type="email" placeholder="you@example.com" />
  </label>

  <label data-field>
    Password
    <input type="password" placeholder="Password" aria-describedby="password-hint" />
    <small id="password-hint" data-hint>This is a small hint</small>
  </label>

  <div data-field>
    <label>Select</label>
    <select aria-label="Select an option">
      <option value="">Select an option</option>
      <option value="a">Option A</option>
      <option value="b">Option B</option>
      <option value="c">Option C</option>
      <option value="d">Option D</option>
      <option value="e">Option E</option>
      <option value="f">Option F</option>
    </select>
  </div>

  <label data-field>
    Message
    <textarea placeholder="Your message..."></textarea>
  </label>

  <label data-field>
    Disabled
    <input type="text" placeholder="Disabled" disabled />
  </label>

  <label data-field>
    File<br />
    <input type="file" placeholder="Pick a file..." />
  </label>

  <label data-field>
    Date and time
    <input type="datetime-local" />
  </label>

  <label data-field>
    Date
    <input type="date" />
  </label>

  <label data-field>
    <input type="checkbox" /> I agree to the terms
  </label>

  <fieldset class="hstack">
    <legend>Preference</legend>
    <label><input type="radio" name="pref">OptionA</label>
    <label><input type="radio" name="pref">Option B</label>
    <label><input type="radio" name="pref">Option C</label>
  </fieldset>

  <label data-field>
    Volume
    <input type="range" min="0" max="100" value="50" />
  </label>

  <button type="submit">Submit</button>
</form>
```

### Input group

Use `.group` on a `<fieldset>` to combine inputs with buttons or labels.

```
<fieldset class="group">
  <legend>https://</legend>
  <input type="url" placeholder="subdomain">
  <select placeholder="Select" aria-label="Select a subdomain">
    <option>.example.com</option>
    <option>.example.net</option>
  </select>
  <button>Go</button>
</fieldset>

<fieldset class="group">
  <input type="text" placeholder="Search" />
  <button>Go</button>
</fieldset>
```

### Validation error

Use `data-field="error"` on field containers to reveal and style error messages.

```
<div data-field="error">
  <label for="error-input">Email</label>
  <input type="email" aria-invalid="true" aria-describedby="error-message" id="error-input" value="invalid-email" />
  <div id="error-message" class="error" role="status">Please enter a valid email address.</div>
</div>
```

[Link](/components/form/)

## Meter

Use `<meter>` for values within a known range. Browser shows colors based on low/high/optimum attributes.

```
<meter value="0.8" min="0" max="1" low="0.3" high="0.7" optimum="1"></meter>
<meter value="0.5" min="0" max="1" low="0.3" high="0.7" optimum="1"></meter>
<meter value="0.2" min="0" max="1" low="0.3" high="0.7" optimum="1"></meter>
```

[Link](/components/meter/)

## Progress

Use the native `<progress>` element.

```
<progress value="60" max="100"></progress>
<progress value="30" max="100"></progress>
<progress value="90" max="100"></progress>
```

[Link](/components/progress/)

## Spinner

Use `aria-busy="true"` on any element to show a loading indicator. Size with `data-spinner="small|large"`.

```
<div class="hstack" style="gap: var(--space-8)">
    <div aria-busy="true" data-spinner="small"></div>
    <div aria-busy="true"></div>
    <div aria-busy="true" data-spinner="large"></div>
    <button aria-busy="true" data-spinner="small" disabled>Loading</button>
</div>
```

### Overlay

Adding `data-spinner="overlay"` dims contents of the container and overlays the spinner on top.

```
<article class="card" aria-busy="true" data-spinner="large overlay">
  <header>
    <h3>Card Title</h3>
    <p>Card description goes here.</p>
  </header>
  <p>This is the card content. It can contain any HTML.</p>
  <footer class="flex gap-2 mt-4">
    <button class="outline">Cancel</button>
    <button>Save</button>
  </footer>
</article>
```

[Link](/components/spinner/)

## Skeleton

Use `.skeleton` with `role="status"` for loading placeholders. Add `.line` for text or `.box` for images.

```
<div role="status" class="skeleton line"></div>
<div role="status" class="skeleton box"></div>
```

### Skeleton card

Put skeleton loader inside `<article>` to get a card layout.

```
<article style="display: flex; gap: var(--space-3); padding: var(--space-6);">
  <div role="status" class="skeleton box"></div>
  <div style="flex: 1; display: flex; flex-direction: column; gap: var(--space-1);">
    <div role="status" class="skeleton line"></div>
    <div role="status" class="skeleton line" style="width: 60%"></div>
  </div>
</article>
```

[Link](/components/skeleton/)

## Sidebar

Use `data-sidebar-layout` on a container (typically `<body>`) with `<aside data-sidebar>` for the sidebar and `<main>` for content. The sidebar stays sticky while the main content scrolls. On mobile, the sidebar becomes a slide-out overlay toggled by a `[data-sidebar-toggle]` button. To make the sidebar collapsible at all widths, set `data-sidebar-layout="always"`.

```
<div data-sidebar-layout>
  <aside data-sidebar>
    <nav>
      <ul>
        <li><a href="#" aria-current="page">Home</a></li>
        <li><a href="#">Users</a></li>
        <li>
          <details open>
            <summary>Settings</summary>
            <ul>
              <li><a href="#">General</a></li>
              <li><a href="#">Security</a></li>
              <li><a href="#">Billing</a></li>
            </ul>
          </details>
        </li>
      </ul>
    </nav>
    <footer>
      <button class="outline" class="sm" style="width: 100%;">Logout</button>
    </footer>
  </aside>
  <main>
    <div style="padding: var(--space-3)">Main content area. Scrolls with the page body.</div>
  </main>
</div>
```

### Always-collapsible

Set `data-sidebar-layout="always"` to keep the toggle visible on all screen sizes.

```
<body data-sidebar-layout="always">
  ...
</body>
```

### With top sticky nav

Add `data-topnav` to a header element for a full-width top navigation bar. The sidebar will adjust to sit below it.

```
<body data-sidebar-layout>
  <nav data-topnav>
    <button data-sidebar-toggle aria-label="Toggle menu" class="outline">☰</button>
    <span>App Name</span>
  </nav>

  <aside data-sidebar>
    <header>Logo</header>
    <nav>...navigation...</nav>
    <footer>Actions</footer>
  </aside>

  <main>
    Main page content.
  </main>
</body>
```

#### Structure

| Attribute | Element |  |
| --- | --- | --- |
| `data-sidebar-layout` | Container | Grid layout wrapper (sidebar + main), typically `<body>` |
| `data-sidebar-layout="always"` | Container | Always-collapsible sidebar (toggle visible on screen sizes) |
| `data-topnav` | `<header>` | Full-width top nav (optional, spans full width) |
| `data-sidebar` | `<aside>` | Sticky sidebar element |
| `data-sidebar-toggle` | `<button>` | Toggles sidebar (mobile) and collapse (always mode) |
| `data-sidebar-open` | Layout | Applied to layout when sidebar is open |

[Link](/components/sidebar/)

## Switch

Add `role="switch"` to a checkbox for toggle switch styling.

```
<label>
  <input type="checkbox" role="switch"> Notifications
</label>
<label>
  <input type="checkbox" role="switch" checked> Confabulation
</label>
```

### Disabled

```
<label>
  <input type="checkbox" role="switch" disabled> Disabled off
</label>
<label>
  <input type="checkbox" role="switch" checked disabled> Disabled on
</label>
```

[Link](/components/switch/)

## Table

Tables are styled by default. Use `<thead>` and `<tbody>` tags. Wrap in a `class="table"` container to get a horizontal scrollbar on small screens.

```
<div class="table">
  <table>
    <thead>
      <tr>
        <th>Name</th>
        <th>Email</th>
        <th>Role</th>
        <th>Status</th>
      </tr>
    </thead>
    <tbody>
      <tr>
        <td>Alice Johnson</td>
        <td>alice@example.com</td>
        <td>Admin</td>
        <td><span class="badge success">Active</span></td>
      </tr>
      <tr>
        <td>Bob Smith</td>
        <td>bob@example.com</td>
        <td>Editor</td>
        <td><span class="badge">Active</span></td>
      </tr>
      <tr>
        <td>Carol White</td>
        <td>carol@example.com</td>
        <td>Viewer</td>
        <td><span class="badge secondary">Pending</span></td>
      </tr>
    </tbody>
  </table>
</div>
```

[Link](/components/table/)

## Tabs

Wrap tab buttons and panels in `<ot-tabs>`. Use `role="tablist"`, `role="tab"`, and `role="tabpanel"`.

```
<ot-tabs>
  <div role="tablist">
    <button role="tab">Account</button>
    <button role="tab">Password</button>
    <button role="tab">Notifications</button>
  </div>
  <div role="tabpanel">
    <h3>Account Settings</h3>
    <p>Manage your account information here.</p>
  </div>
  <div role="tabpanel">
    <h3>Password Settings</h3>
    <p>Change your password here.</p>
  </div>
  <div role="tabpanel">
    <h3>Notification Settings</h3>
    <p>Configure your notification preferences.</p>
  </div>
</ot-tabs>
```

[Link](/components/tabs/)

## Tooltip

Use the standard `title` attribute on any element to render a tooltip with smooth transition.

```
<button title="Save your changes">Save</button>
<button title="Delete this item" data-variant="danger">Delete</button>
<a href="#" title="View your profile">Profile</a>
```

[Link](/components/tooltip/)

## Toast

Show toast notifications with `ot.toast(message, title?, options?)`.

```
<button onclick="ot.toast('Action completed successfully', 'All good', { variant: 'success' })">Success</button>
<button onclick="ot.toast('Something went wrong', 'Oops', { variant: 'danger', placement: 'top-left' })" data-variant="danger">Danger</button>
<button onclick="ot.toast('Please review this warning', 'Warning', { variant: 'warning', placement: 'bottom-right' })" class="outline">Warning</button>
<button onclick="ot.toast('New notification', 'For your attention', { placement: 'top-center' })">Info</button>
```

### Placement

```
ot.toast('Top left', '', { placement: 'top-left' })
ot.toast('Top center', '',{ placement: 'top-center' })
ot.toast('Top right', '',{ placement: 'top-right' })  // default
ot.toast('Bottom left', '', { placement: 'bottom-left' })
ot.toast('Bottom center', '', { placement: 'bottom-center' })
ot.toast('Bottom right', '',{ placement: 'bottom-right' })
```

### Options

| Option | Default | Description |
| --- | --- | --- |
| `variant` | `''` | `'success'`, `'danger'`, `'warning'` |
| `placement` | `'top-right'` | Position on screen |
| `duration` | `4000` | Auto-dismiss in ms (0 = persistent) |

### Custom markup

Use `ot.toast.el(element, options?)` to show toasts with custom HTML content.

```
<template id="undo-toast">
  <output class="toast" data-variant="success">
    <h6 class="toast-title">Changes saved</h6>
    <p>Your document has been updated.</p>
    <button data-variant="secondary" class="small" onclick="this.closest('.toast').remove()">Okay</button>
  </output>
</template>

<button onclick="ot.toast.el(document.querySelector('#undo-toast'), { duration: 8000 })">
  Toast with action
</button>
```

**From a template:**

```
ot.toast.el(document.querySelector('#my-template'))
ot.toast.el(document.querySelector('#my-template'), { duration: 8000, placement: 'bottom-center' })
```

**Dynamic element:**

```
const el = document.createElement('output');
el.className = 'toast';
el.setAttribute('data-variant', 'warning');
el.innerHTML = '<h6 class="toast-title">Warning</h6><p>Custom content here</p>';
ot.toast.el(el);
```

The element is cloned before display, so templates can be reused.

### Clearing toasts

```
ot.toast.clear()              // Clear all
ot.toast.clear('top-right')   // Clear specific placement
```

[Link](/components/toast/)

## Grid

A 12-column grid system using CSS grid. Use `.container`, `.row`, and `.col` classes. Column widths use `.col-{n}` where n is 1-12.

```
<div class="container demo-grid">
  <div class="row">
    <div class="col-4">col-4</div>
    <div class="col-4">col-4</div>
    <div class="col-4">col-4</div>
  </div>
  <div class="row">
    <div class="col-6">col-6</div>
    <div class="col-6">col-6</div>
  </div>
  <div class="row">
    <div class="col-3">col-3</div>
    <div class="col-6">col-6</div>
    <div class="col-3">col-3</div>
  </div>
  <div class="row">
    <div class="col-4 offset-2">col-4 offset-2</div>
    <div class="col-4">col-4</div>
  </div>
  <div class="row">
    <div class="col-3">col-3</div>
    <div class="col-4 col-end">col-4 col-end</div>
  </div>
</div>
```

[Link](/components/grid/)

## Utils and helpers

See [utilities.css](https://github.com/knadh/oat/blob/master/src/css/utilities.css) for commonly used utility and helper classes.

[Link](/components/utilities/)

© [Kailash Nadh](https://nadh.in)
