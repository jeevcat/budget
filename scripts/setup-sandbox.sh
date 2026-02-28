#!/bin/bash
# Setup script for Claude Code web/mobile sandbox environments.
#
# Detects the Claude Code web/mobile sandbox (hostname "runsc", no $USER)
# and bootstraps PostgreSQL, Biome, and environment variables so that
# `cargo test` and the pre-commit hook work out of the box.
#
# Safe to run repeatedly — every step is idempotent.

set -euo pipefail

# ---------------------------------------------------------------------------
# Detection: only run inside the Claude Code web/mobile sandbox
# ---------------------------------------------------------------------------
if [[ "$(hostname)" != "runsc" ]] || [[ -n "${USER:-}" ]]; then
  echo "Not a Claude Code web/mobile sandbox — skipping setup."
  exit 0
fi

echo "Cloud sandbox detected — running environment setup..."

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

# ---------------------------------------------------------------------------
# 1. PostgreSQL
# ---------------------------------------------------------------------------
if ! pg_isready -q 2>/dev/null; then
  echo "Starting PostgreSQL..."
  pg_ctlcluster 16 main start >/dev/null 2>&1 || true
fi

# Switch local TCP auth to trust (the sandbox has no secrets to protect)
PG_HBA="/etc/postgresql/16/main/pg_hba.conf"
if grep -q 'scram-sha-256' "$PG_HBA" 2>/dev/null; then
  echo "Configuring PostgreSQL for trust auth..."
  sed -i 's/scram-sha-256/trust/g' "$PG_HBA"
  pg_ctlcluster 16 main reload >/dev/null 2>&1
fi

# Create the budget role + database if they don't exist
if ! sudo -u postgres psql -tAc "SELECT 1 FROM pg_roles WHERE rolname='budget'" | grep -q 1; then
  echo "Creating PostgreSQL user 'budget'..."
  sudo -u postgres psql -c "CREATE USER budget WITH CREATEDB;" >/dev/null
fi
if ! sudo -u postgres psql -tAc "SELECT 1 FROM pg_database WHERE datname='budget'" | grep -q 1; then
  echo "Creating PostgreSQL database 'budget'..."
  sudo -u postgres psql -c "CREATE DATABASE budget OWNER budget;" >/dev/null
fi

# Export DATABASE_URL for cargo/sqlx (compile-time checks + sqlx::test)
export DATABASE_URL="postgresql://budget@localhost:5432/budget"

# Run SQL migrations so sqlx compile-time checks (clippy) work.
# Use the URL (TCP) rather than -U/-d which defaults to peer auth and fails
# when the OS user (root) doesn't match the PG role (budget).
echo "Running database migrations..."
for f in "$PROJECT_DIR"/migrations/*.sql; do
  psql "$DATABASE_URL" -f "$f" >/dev/null 2>&1 || true
done

# Fetch crate sources so we can find apalis-postgres migrations
echo "Fetching crate sources..."
cargo fetch --manifest-path "$PROJECT_DIR/Cargo.toml" --quiet 2>/dev/null || true

# Run apalis-postgres migrations (its sqlx::query_file_as! macros need the
# apalis schema to exist so we can compile without SQLX_OFFLINE)
APALIS_DIR=$(find /root/.cargo/registry/src -maxdepth 2 -name 'apalis-postgres-*' -type d -print -quit 2>/dev/null)
if [ -n "$APALIS_DIR" ] && [ -d "$APALIS_DIR/migrations" ]; then
  echo "Running apalis-postgres migrations..."
  for f in "$APALIS_DIR"/migrations/*.sql; do
    psql "$DATABASE_URL" -f "$f" >/dev/null 2>&1 || true
  done
fi

# Persist for future shell invocations in this session.
# .bashrc typically has `[ -z "$PS1" ] && return` near the top, so anything
# appended at the bottom is invisible to non-interactive shells (hooks, Bash
# tool).  Insert the export *before* that guard line so every shell sees it.
ENV_LINE='export DATABASE_URL="postgresql://budget@localhost:5432/budget"'
for rc in /root/.bashrc /root/.profile; do
  if [ -f "$rc" ] && ! grep -qF 'DATABASE_URL' "$rc" 2>/dev/null; then
    # Insert at line 2 (after the shebang / comment header) so it runs
    # before any early-return guards.
    sed -i "2i\\$ENV_LINE" "$rc"
  fi
done

# ---------------------------------------------------------------------------
# 2. Biome (for frontend linting / formatting)
# ---------------------------------------------------------------------------
if ! command -v biome &>/dev/null; then
  echo "Installing Biome..."
  npm install --prefix "$PROJECT_DIR" --save-dev @biomejs/biome >/dev/null 2>&1

  # Symlink into a PATH directory so the pre-commit hook and Claude hooks find it
  ln -sf "$PROJECT_DIR/node_modules/.bin/biome" /usr/local/bin/biome
fi

# ---------------------------------------------------------------------------
# 3. Pre-commit hook
# ---------------------------------------------------------------------------
echo "Activating pre-commit hook..."
ln -sf ../../.github/hooks/pre-commit "$PROJECT_DIR/.git/hooks/pre-commit"

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
echo "Sandbox setup complete."
echo "  DATABASE_URL=$DATABASE_URL"
echo "  biome: $(biome --version 2>/dev/null || echo 'not found')"
echo "  pg_isready: $(pg_isready 2>&1)"
