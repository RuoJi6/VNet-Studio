#!/bin/sh
set -eu
cd "$(dirname "$0")"
target="${VNET_BUILD_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
export VNET_BUILD_TARGET="$target"
pnpm --dir desktop install --frozen-lockfile
pnpm --dir desktop build
cargo build --release --locked --target "$target" --features custom-protocol --manifest-path desktop/src-tauri/Cargo.toml
cargo build --release --locked --target "$target" --bin vnetctl-cli
sh package.sh
