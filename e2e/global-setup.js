// Database creation and env setup happen in playwright.config.js (top-level)
// so they execute before the webServer starts. globalSetup runs *after* the
// webServer is available, so it cannot influence server configuration.
export default async function globalSetup() {}
