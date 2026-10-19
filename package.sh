#!/bin/sh
set -eu
cd "$(dirname "$0")"
app="../VNet Studio.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "desktop/src-tauri/target/release/vnet-studio-desktop" "$app/Contents/MacOS/vnetctl"
cp "desktop/src-tauri/icons/icon.icns" "$app/Contents/Resources/VNetStudio.icns"
cp "${CARGO_TARGET_DIR:-target}"/release/vnetctl-cli "$app/Contents/MacOS/vnetctl-cli"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>VNet Studio</string>
<key>CFBundleDisplayName</key><string>VNet Studio</string>
<key>CFBundleIdentifier</key><string>local.vnetstudio.desktop</string>
<key>CFBundleVersion</key><string>22</string>
<key>CFBundleShortVersionString</key><string>0.10.6</string>
<key>CFBundleExecutable</key><string>vnetctl</string>
<key>CFBundleIconFile</key><string>VNetStudio.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSLocalNetworkUsageDescription</key><string>VNet Studio 需要连接你指定的虚拟机或局域网服务，将 Mac 本地入口的 TCP 流量转发到该服务。不会扫描局域网或上传连接内容。</string>
</dict></plist>
PLIST
codesign --force --sign - "$app/Contents/MacOS/vnetctl-cli"
codesign --force --sign - "$app"
printf 'Packaged: %s\n' "$app"
