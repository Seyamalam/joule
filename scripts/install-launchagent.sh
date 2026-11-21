#!/usr/bin/env bash
# Install (or reinstall) the LaunchAgent so the app starts at login.
set -euo pipefail

app="$HOME/Applications/DevinUsage.app"
[ -x "$app/Contents/MacOS/DevinUsage" ] || { echo "run scripts/package-app.sh first"; exit 1; }

plist="$HOME/Library/LaunchAgents/local.devin.usage.plist"
cat > "$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>local.devin.usage</string>
    <key>ProgramArguments</key>
    <array>
        <string>$app/Contents/MacOS/DevinUsage</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><false/>
    <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
EOF

uid="$(id -u)"
launchctl bootout "gui/$uid/local.devin.usage" 2>/dev/null || true
launchctl bootstrap "gui/$uid" "$plist"
echo "started local.devin.usage"
