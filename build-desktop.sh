#!/bin/sh
set -eu
cd "$(dirname "$0")"
pnpm --dir desktop install --frozen-lockfile
pnpm --dir desktop build
cargo build --release --locked --features custom-protocol --manifest-path desktop/src-tauri/Cargo.toml
cargo build --release --locked --bin vnetctl-cli
sh package.sh
