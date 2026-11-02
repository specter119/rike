#!/usr/bin/env bash
# Runs Rike against an NSH dev server (hot reload of the TSX UI).
# Usage: scripts/dev.sh [rike args...]   e.g. scripts/dev.sh ~/notes
#
# `nsh dev --server-only` listens on 0.0.0.0 and has no bind option, so anything on your
# network can reach it while this runs. Use it on a trusted network only.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
port="${RIKE_DEV_PORT:-8790}"
(cd "$root/ui" && exec nsh dev --server-only --port "$port") &
server=$!
trap 'kill "$server" 2>/dev/null || true' EXIT
sleep 2
cargo run --manifest-path "$root/Cargo.toml" -p rike-app --features dev -- \
  --dev-server "ws://127.0.0.1:$port/ws" "$@"
