#!/usr/bin/env bash
# Install (or reinstall) the LaunchAgent so Joule starts at login.
set -euo pipefail

app="$HOME/Applications/Joule.app"
[ -x "$app/Contents/MacOS/Joule" ] || { echo "run scripts/package-app.sh first"; exit 1; }

plist="$HOME/Library/LaunchAgents/dev.joule.tracker.plist"
cat > "$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>dev.joule.tracker</string>
    <key>ProgramArguments</key>
    <array>
        <string>$app/Contents/MacOS/Joule</string>
    </array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><false/>
    <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
EOF

uid="$(id -u)"
# remove a previous-era agent if still registered
launchctl bootout "gui/$uid/local.devin.usage" 2>/dev/null || true
rm -f "$HOME/Library/LaunchAgents/local.devin.usage.plist"
launchctl bootout "gui/$uid/dev.joule.tracker" 2>/dev/null || true
launchctl bootstrap "gui/$uid" "$plist"
echo "started dev.joule.tracker"
