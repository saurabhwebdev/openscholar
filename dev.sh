#!/usr/bin/env bash
# OpenScholar dev loop: rebuilds + restarts the app on every change.
# Usage: ./dev.sh   (runs in foreground; Ctrl-C to stop)
set -euo pipefail
cd "$(dirname "$0")"

export DATABASE_URL="${DATABASE_URL:-postgresql://admin1:dev-local-only@127.0.0.1:5432/school}"
export BIND_ADDR="${BIND_ADDR:-127.0.0.1:3001}"
export ADMIN_EMAIL="${ADMIN_EMAIL:-admin@example.com}"
export ADMIN_PASSWORD="${ADMIN_PASSWORD:-changeme-admin-123}"
export ADMIN_NAME="${ADMIN_NAME:-Administrator}"
export RUST_LOG="${RUST_LOG:-opencodetest=debug,tower_http=debug,sqlx=warn}"

# Self-hosted Tailwind: build once, then watch in the background so new
# utility classes in templates are compiled on save. Falls back to the
# committed static/css/tailwind.css when the binary is absent.
TAILWIND="${TAILWINDCSS_BIN:-$HOME/.local/bin/tailwindcss}"
if [ -x "$TAILWIND" ]; then
  "$TAILWIND" -i static/css/input.css -o static/css/tailwind.css --minify
  "$TAILWIND" -i static/css/input.css -o static/css/tailwind.css --watch >/tmp/opencode/tailwind.log 2>&1 &
else
  echo "tailwindcss not found; serving committed static/css/tailwind.css" >&2
fi

exec watchexec --restart \
  --watch src --watch templates --watch static --watch migrations \
  --watch Cargo.toml \
  --debounce 500ms \
  -- cargo run
