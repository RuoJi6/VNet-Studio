#!/bin/sh
set -eu
cd "$(dirname "$0")"
target="${VNET_BUILD_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}"
version="$(python3 scripts/version.py)"
app="${VNET_APP_PATH:-../VNet Studio.app}"
cli_root="${CARGO_TARGET_DIR:-target}"
desktop_root="${CARGO_TARGET_DIR:-desktop/src-tauri/target}"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$desktop_root/$target/release/vnet-studio-desktop" "$app/Contents/MacOS/vnetctl"
cp "desktop/src-tauri/icons/icon.icns" "$app/Contents/Resources/VNetStudio.icns"
cp "$cli_root/$target/release/vnetctl-cli" "$app/Contents/MacOS/vnetctl-cli"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>VNet Studio</string>
<key>CFBundleDisplayName</key><string>VNet Studio</string>
<key>CFBundleIdentifier</key><string>local.vnetstudio.desktop</string>
<key>CFBundleVersion</key><string>${version%%-*}</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleExecutable</key><string>vnetctl</string>
<key>CFBundleIconFile</key><string>VNetStudio.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSLocalNetworkUsageDescription</key><string>VNet Studio 需要连接你指定的虚拟机或局域网服务，将 Mac 本地入口的 TCP 流量转发到该服务。不会扫描局域网或上传连接内容。</string>
</dict></plist>
PLIST
codesign --force --sign - "$app/Contents/MacOS/vnetctl-cli"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
printf 'Packaged: %s\n' "$app"
