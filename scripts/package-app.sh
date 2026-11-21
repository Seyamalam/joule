#!/usr/bin/env bash
# Build the release binary and install ~/Applications/Joule.app.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release

app="$HOME/Applications/Joule.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/joule "$app/Contents/MacOS/Joule"

# app icon from the SVG logo, if we can rasterize it
iconset="$(mktemp -d)/Joule.iconset"
mkdir -p "$iconset"
if qlmanage -t -s 1024 -o "$iconset" assets/logo.svg >/dev/null 2>&1 \
    && [ -f "$iconset/logo.svg.png" ]; then
    png="$iconset/logo.svg.png"
    for spec in "16:icon_16x16" "32:icon_16x16@2x" "32:icon_32x32" "64:icon_32x32@2x" \
                "128:icon_128x128" "256:icon_128x128@2x" "256:icon_256x256" \
                "512:icon_256x256@2x" "512:icon_512x512" "1024:icon_512x512@2x"; do
        s="${spec%%:*}"; f="${spec##*:}"
        sips -z "$s" "$s" "$png" --out "$iconset/$f.png" >/dev/null 2>&1
    done
    iconutil -c icns -o "$app/Contents/Resources/Joule.icns" "$iconset" 2>/dev/null || true
fi

cat > "$app/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Joule</string>
    <key>CFBundleDisplayName</key><string>Joule</string>
    <key>CFBundleIdentifier</key><string>dev.joule.tracker</string>
    <key>CFBundleVersion</key><string>0.1.0</string>
    <key>CFBundleShortVersionString</key><string>0.1.0</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleExecutable</key><string>Joule</string>
    <key>CFBundleIconFile</key><string>Joule</string>
    <key>LSUIElement</key><true/>
    <key>LSMinimumSystemVersion</key><string>14.0</string>
</dict>
</plist>
EOF

echo "installed $app"
