#!/usr/bin/env bash
# Runs scripts/backup-rooms.mjs every night at 03:00 on this Mac (plan 018).
#
#   scripts/install-backup-agent.sh            # install, or update, and run once now
#   scripts/install-backup-agent.sh --remove   # stop and remove it
#
# A launchd agent of this user: no root, nothing to pay for. A Mac asleep at
# 03:00 runs the job when it wakes. Backups go to ~/OntimeBackups, the log to
# ~/OntimeBackups/backup.log.
set -euo pipefail

label="th.ontime.backup-rooms"
plist="$HOME/Library/LaunchAgents/$label.plist"
repo="$(cd "$(dirname "$0")/.." && pwd)"
out="$HOME/OntimeBackups"

if [ "${1:-}" = "--remove" ]; then
  launchctl bootout "gui/$(id -u)" "$plist" 2>/dev/null || true
  rm -f "$plist"
  echo "removed $label (the backups in $out are kept)"
  exit 0
fi

# absolute paths: launchd starts with almost no PATH, and a version manager's
# per-shell node path disappears with the shell
node="$(readlink -f "$(command -v node)")"
cargo_dir="$(dirname "$(command -v cargo)")"
mkdir -p "$out"
chmod 700 "$out"

cat >"$plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key>
  <array>
    <string>$node</string>
    <string>$repo/scripts/backup-rooms.mjs</string>
    <string>--out</string><string>$out</string>
  </array>
  <key>WorkingDirectory</key><string>$repo</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>$cargo_dir:/usr/bin:/bin:/usr/sbin:/sbin</string>
  </dict>
  <key>StartCalendarInterval</key>
  <dict><key>Hour</key><integer>3</integer><key>Minute</key><integer>0</integer></dict>
  <key>StandardOutPath</key><string>$out/backup.log</string>
  <key>StandardErrorPath</key><string>$out/backup.log</string>
</dict>
</plist>
EOF

launchctl bootout "gui/$(id -u)" "$plist" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$plist"
echo "installed $label: every night at 03:00, into $out"
launchctl kickstart "gui/$(id -u)/$label"
echo "started a first run now; see $out/backup.log"
