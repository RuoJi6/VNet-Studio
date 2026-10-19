#!/bin/bash
# Build native macOS GUI + CLI archives without starting any network services.
set -euo pipefail
cd "$(dirname "$0")/.."

target="${VNET_BUILD_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
case "$target" in
  aarch64-apple-darwin) arch=arm64; macho=arm64 ;;
  x86_64-apple-darwin) arch=x86_64; macho=x86_64 ;;
  *) echo "Unsupported release target: $target" >&2; exit 1 ;;
esac
version="$(python3 scripts/version.py)"
export VNET_BUILD_TARGET="$target"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
export VNET_APP_PATH="$PWD/target/release-bundle/$arch/VNet Studio.app"
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"
if [[ "$CARGO_TARGET_DIR" != /* ]]; then
  export CARGO_TARGET_DIR="$PWD/$CARGO_TARGET_DIR"
fi
bash build-desktop.sh

cli="$CARGO_TARGET_DIR/$target/release/vnetctl-cli"
gui="$VNET_APP_PATH/Contents/MacOS/vnetctl"
for binary in "$cli" "$gui" "$VNET_APP_PATH/Contents/MacOS/vnetctl-cli"; do
  actual="$(lipo -archs "$binary")"
  [[ "$actual" == "$macho" ]] || { echo "Wrong architecture: $binary ($actual)" >&2; exit 1; }
done
[[ "$("$cli" --version)" == "vnetctl-cli $version" ]]
"$cli" --help > /dev/null
codesign --force --sign - "$cli"
codesign --verify --strict "$cli"

out="$PWD/dist"
stage="$PWD/target/cli-bundle/$arch"
mkdir -p "$out" "$stage"
cp "$cli" "$stage/vnetctl-cli"
cp LICENSE "$stage/LICENSE"
cat > "$stage/README.txt" <<EOF
VNet Studio CLI $version — macOS $arch

Run ./vnetctl-cli --help or ./vnetctl-cli --version.
To install, copy vnetctl-cli into a directory in your PATH, such as ~/.local/bin.
Parallels guest management requires Parallels Desktop and working Parallels Tools.
Host-local TCP forwarding does not require Parallels.
The executable is ad-hoc signed and is not Apple notarized.
EOF
ditto -c -k --sequesterRsrc --keepParent "$VNET_APP_PATH" "$out/VNet-Studio-$version-macOS-$arch.zip"
COPYFILE_DISABLE=1 tar -czf "$out/vnetctl-cli-$version-macOS-$arch.tar.gz" -C "$stage" vnetctl-cli LICENSE README.txt
printf 'Release archives: %s\n' "$out"
