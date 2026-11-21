#!/usr/bin/env bash
# Build the release binary and install ~/Applications/DevinUsage.app.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release

app="$HOME/Applications/DevinUsage.app"
mkdir -p "$app/Contents/MacOS"
cp target/release/devin-usage "$app/Contents/MacOS/DevinUsage"

cat > "$app/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>DevinUsage</string>
    <key>CFBundleDisplayName</key><string>Devin Usage</string>
    <key>CFBundleIdentifier</key><string>dev.seyam.devin-usage</string>
    <key>CFBundleVersion</key><string>0.1.0</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>DevinUsage</string>
    <key>LSUIElement</key><true/>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
</dict>
</plist>
EOF

echo "installed $app"
