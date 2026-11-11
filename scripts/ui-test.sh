#!/usr/bin/env bash
# Runs the TSX UI tests headless against a scratch copy of the basic fixture vault.
# Usage: scripts/ui-test.sh [nsh test args...]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.local/bin:$PATH"
# nsh builds its generated crates with $CARGO; the fork cargo's shared-target
# cache can reuse stale nsh artifacts across nsh revs, so use the stock one.
export CARGO="${CARGO:-$HOME/.local/share/rustup-cargo/bin/cargo}"
export TMPDIR="${TMPDIR:-$HOME/.cache/sx-tmp}" CARGO_NET_OFFLINE=true
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/rike-ui-target}"
mkdir -p "$TMPDIR"
vault="$(mktemp -d "$TMPDIR/rike-ui-test.XXXXXX")"
trap 'rm -rf "$vault"' EXIT
# Each test boots its own vault under $vault (Rike.devReset), seeded from a fixture folder.
export RIKE_DEV_VAULT="$vault" RIKE_DEV_SEED="$root/fixtures/vaults" RIKE_DEV_TODAY=2026-11-01
cd "$root/ui"
nsh test "$@"
