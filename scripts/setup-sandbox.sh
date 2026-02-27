#!/bin/bash
# Setup script for Claude Code web/mobile sandbox environments.
#
# Detects the gVisor-based cloud sandbox (Linux runsc, no $USER) and
# bootstraps PostgreSQL, Biome, and environment variables so that
# `cargo test` and the pre-commit hook work out of the box.
#
# Safe to run repeatedly — every step is idempotent.

set -euo pipefail

# ---------------------------------------------------------------------------
# Detection: only run inside the cloud sandbox
# ---------------------------------------------------------------------------
if [[ "$(uname -r)" != *runsc* ]] || [[ -n "${USER:-}" ]]; then
  echo "Not a cloud sandbox — skipping setup."
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

# Export DATABASE_URL for cargo test (sqlx::test needs it)
export DATABASE_URL="postgresql://budget@localhost:5432/budget"

# Persist for future shell invocations in this session
ENV_LINE='export DATABASE_URL="postgresql://budget@localhost:5432/budget"'
for rc in /root/.bashrc /root/.profile; do
  if [ -f "$rc" ] && ! grep -qF 'DATABASE_URL' "$rc" 2>/dev/null; then
    echo "$ENV_LINE" >> "$rc"
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
