#!/usr/bin/env bash
# Builds the app for the simulator, installs and launches it on the first available iPhone simulator, then saves
# a screenshot, the app's log and any crash report to ios/build/sim/. Run after ios/scripts/build_core.sh and
# xcodegen, from the repo root. macOS only.
set -euo pipefail
OUT=ios/build/sim; DD=ios/build/dd
rm -rf "$OUT"; mkdir -p "$OUT"
xcodebuild -project ios/Tinline.xcodeproj -scheme Tinline -destination "generic/platform=iOS Simulator" \
  -derivedDataPath "$DD" -quiet build
APP=$DD/Build/Products/Debug-iphonesimulator/Tinline.app
UDID=$(xcrun simctl list devices available | grep -m1 -E '^ +iPhone' | grep -oE '[0-9A-F-]{36}')
echo "simulator: $(xcrun simctl list devices available | grep "$UDID")"
xcrun simctl boot "$UDID" 2>/dev/null || true
xcrun simctl bootstatus "$UDID" -b >/dev/null
xcrun simctl install "$UDID" "$APP"
start=$(date '+%Y-%m-%d %H:%M:%S')
xcrun simctl launch "$UDID" com.osvauld.tinline
sleep 10
xcrun simctl io "$UDID" screenshot "$OUT/launch.png"
# Still running? A crash right after launch leaves no Tinline process.
# (grep -c, not -q: -q closes the pipe early and pipefail then reports launchctl's SIGPIPE as a failure.)
if [ "$(xcrun simctl spawn "$UDID" launchctl list | grep -c com.osvauld.tinline)" -gt 0 ]; then echo "app: running"; else echo "app: NOT running"; fi
xcrun simctl spawn "$UDID" log show --start "$start" --style compact --predicate 'process == "Tinline"' > "$OUT/app.log" 2>&1 || true
find ~/Library/Logs/DiagnosticReports -name 'Tinline*' -newermt "$start" -exec cp {} "$OUT/" \; 2>/dev/null || true
ls "$OUT"
