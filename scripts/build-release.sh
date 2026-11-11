#!/usr/bin/env bash
# Builds the TSX bundle, then the release binary with the bundle embedded.
# Usage: scripts/build-release.sh   (result: target/release/rike)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.local/bin:$PATH"
# nsh builds its generated crates with $CARGO; the fork cargo's shared-target
# cache can reuse stale nsh artifacts across nsh revs, so use the stock one.
export CARGO="${CARGO:-$HOME/.local/share/rustup-cargo/bin/cargo}"
command -v nsh >/dev/null || {
  echo "nsh not found: cargo install --locked --git https://github.com/longbridge/nsh nsh-cli" >&2
  exit 1
}
# The bundle lives under ui/.nsh/out; nsh compiles ui/ together with crates/rike-native.
(cd "$root/ui" && nsh build --release)
# build.rs only embeds the bundle when it exists at compile time.
touch "$root/crates/rike-app/build.rs"
cargo build --release -p rike-app --manifest-path "$root/Cargo.toml"
echo "built $root/target/release/rike"
