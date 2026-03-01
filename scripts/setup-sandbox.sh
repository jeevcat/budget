#!/usr/bin/env bash
# Session start script for Claude Code.
#
# Two modes:
#   1. Nix environment (flake.nix available) — enters nix develop and exports
#      JAVA_HOME, ANDROID_HOME, GRADLE_OPTS, DATABASE_URL via CLAUDE_ENV_FILE
#   2. Cloud sandbox (hostname "runsc", no $USER) — installs PostgreSQL, JDK,
#      Android SDK, Biome via apt and persists env vars
#
# Safe to run repeatedly — every step is idempotent.

set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

# Helper: write an env var to CLAUDE_ENV_FILE so it persists across Bash calls
persist_env() {
  if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
    echo "export $1=\"$2\"" >> "$CLAUDE_ENV_FILE"
  fi
}

# ---------------------------------------------------------------------------
# Nix environment — use flake devShell to get JDK, Android SDK, DATABASE_URL
# ---------------------------------------------------------------------------
if [ -f "$PROJECT_DIR/flake.nix" ] && command -v nix &>/dev/null; then
  echo "Nix detected — loading dev shell environment..."

  # Capture the env vars that nix develop provides
  NIX_ENV=$(nix develop "$PROJECT_DIR" --command env 2>/dev/null) || true

  for var in JAVA_HOME ANDROID_HOME GRADLE_OPTS DATABASE_URL; do
    val=$(echo "$NIX_ENV" | grep "^${var}=" | head -1 | cut -d= -f2-)
    if [ -n "$val" ]; then
      persist_env "$var" "$val"
      echo "  $var=$val"
    fi
  done

  # Also forward PATH additions (nix adds JDK/SDK bins)
  nix_path=$(echo "$NIX_ENV" | grep "^PATH=" | head -1 | cut -d= -f2-)
  if [ -n "$nix_path" ]; then
    persist_env "PATH" "$nix_path"
  fi

  echo "Nix dev shell environment loaded."
  exit 0
fi

# ---------------------------------------------------------------------------
# Cloud sandbox detection
# ---------------------------------------------------------------------------
if [[ "$(hostname)" != "runsc" ]] || [[ -n "${USER:-}" ]]; then
  echo "Not a Nix or cloud sandbox environment — skipping setup."
  exit 0
fi

echo "Cloud sandbox detected — running environment setup..."

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

export DATABASE_URL="postgresql://budget@localhost:5432/budget"
persist_env "DATABASE_URL" "$DATABASE_URL"

# Run SQL migrations so sqlx compile-time checks (clippy) work.
echo "Running database migrations..."
for f in "$PROJECT_DIR"/migrations/*.sql; do
  psql "$DATABASE_URL" -f "$f" >/dev/null 2>&1 || true
done

# Fetch crate sources so we can find apalis-postgres migrations
echo "Fetching crate sources..."
cargo fetch --manifest-path "$PROJECT_DIR/Cargo.toml" --quiet 2>/dev/null || true

# Run apalis-postgres migrations
APALIS_DIR=$(find /root/.cargo/registry/src -maxdepth 2 -name 'apalis-postgres-*' -type d -print -quit 2>/dev/null)
if [ -n "$APALIS_DIR" ] && [ -d "$APALIS_DIR/migrations" ]; then
  echo "Running apalis-postgres migrations..."
  for f in "$APALIS_DIR"/migrations/*.sql; do
    psql "$DATABASE_URL" -f "$f" >/dev/null 2>&1 || true
  done
fi

# ---------------------------------------------------------------------------
# 2. JDK + Android SDK (for mobile/Gradle builds)
# ---------------------------------------------------------------------------
if ! command -v java &>/dev/null; then
  echo "Installing JDK 21..."
  apt-get update -qq && apt-get install -y -qq openjdk-21-jdk-headless >/dev/null 2>&1
fi

if [ -z "${JAVA_HOME:-}" ]; then
  JAVA_HOME="$(dirname "$(dirname "$(readlink -f "$(which java)")")")"
  export JAVA_HOME
fi
persist_env "JAVA_HOME" "$JAVA_HOME"

ANDROID_HOME="${ANDROID_HOME:-/opt/android-sdk}"
export ANDROID_HOME
persist_env "ANDROID_HOME" "$ANDROID_HOME"

if [ ! -d "$ANDROID_HOME/cmdline-tools" ]; then
  echo "Installing Android SDK command-line tools..."
  mkdir -p "$ANDROID_HOME"
  CMDLINE_ZIP="/tmp/cmdline-tools.zip"
  curl -sL "https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip" \
    -o "$CMDLINE_ZIP"
  unzip -qo "$CMDLINE_ZIP" -d "$ANDROID_HOME/cmdline-tools-tmp"
  mkdir -p "$ANDROID_HOME/cmdline-tools"
  mv "$ANDROID_HOME/cmdline-tools-tmp/cmdline-tools" "$ANDROID_HOME/cmdline-tools/latest"
  rm -rf "$ANDROID_HOME/cmdline-tools-tmp" "$CMDLINE_ZIP"
fi

export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$PATH"
yes | sdkmanager --licenses >/dev/null 2>&1 || true
sdkmanager --install "platforms;android-36" "build-tools;36.0.0" >/dev/null 2>&1 || true

# ---------------------------------------------------------------------------
# 3. Persist env vars for sandbox shell invocations
# ---------------------------------------------------------------------------
ENV_LINES=(
  'export DATABASE_URL="postgresql://budget@localhost:5432/budget"'
  "export JAVA_HOME=\"$JAVA_HOME\""
  "export ANDROID_HOME=\"$ANDROID_HOME\""
)
for rc in /root/.bashrc /root/.profile; do
  if [ -f "$rc" ]; then
    for env_line in "${ENV_LINES[@]}"; do
      var_name="${env_line#export }"
      var_name="${var_name%%=*}"
      if ! grep -qF "$var_name" "$rc" 2>/dev/null; then
        sed -i "2i\\$env_line" "$rc"
      fi
    done
  fi
done

# ---------------------------------------------------------------------------
# 4. Biome (for frontend linting / formatting)
# ---------------------------------------------------------------------------
if ! command -v biome &>/dev/null; then
  echo "Installing Biome..."
  npm install --prefix "$PROJECT_DIR" --save-dev @biomejs/biome >/dev/null 2>&1
  ln -sf "$PROJECT_DIR/node_modules/.bin/biome" /usr/local/bin/biome
fi

# ---------------------------------------------------------------------------
# 5. Pre-commit hook
# ---------------------------------------------------------------------------
echo "Activating pre-commit hook..."
ln -sf ../../.github/hooks/pre-commit "$PROJECT_DIR/.git/hooks/pre-commit"

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
echo "Sandbox setup complete."
echo "  DATABASE_URL=$DATABASE_URL"
echo "  JAVA_HOME=$JAVA_HOME"
echo "  ANDROID_HOME=$ANDROID_HOME"
echo "  java: $(java -version 2>&1 | head -1)"
echo "  biome: $(biome --version 2>/dev/null || echo 'not found')"
echo "  pg_isready: $(pg_isready 2>&1)"
