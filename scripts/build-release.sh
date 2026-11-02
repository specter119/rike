#!/usr/bin/env bash
# Builds the TSX bundle, then the release binary with the bundle embedded.
# Usage: scripts/build-release.sh   (result: target/release/rike)
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
command -v nsh >/dev/null || {
  echo "nsh not found: cargo install --locked --git https://github.com/longbridge/nsh --rev a5036af48ddc193b5023ece9b9196c76b8f8d3f5 nsh-cli" >&2
  exit 1
}
# The bundle lives under ui/.nsh/out; nsh compiles ui/ together with crates/rike-native.
(cd "$root/ui" && nsh build --release)
# build.rs only embeds the bundle when it exists at compile time.
touch "$root/crates/rike-app/build.rs"
cargo build --release -p rike-app --manifest-path "$root/Cargo.toml"
echo "built $root/target/release/rike"
