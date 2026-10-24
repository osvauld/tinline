#!/usr/bin/env bash
# Runs the XCUITest flows (scheme TinlineUI) on the first available iPhone simulator from a clean install and an empty
# Keychain, then exports every screenshot the tests attached to ios/build/ui/shots/ and zips them as ios/build/ui/shots.zip.
# Always exits 0 so the shots can be fetched; the result is the "UI TEST:" line. Run after ios/scripts/build_core.sh and
# xcodegen, from the repo root. macOS only. Optional: ONLY=TinlineUITests/OnboardingUITests/testCreateAccount
set -euo pipefail
OUT=ios/build/ui; DD=ios/build/dd
rm -rf "$OUT"; mkdir -p "$OUT/shots"
UDID=$(xcrun simctl list devices available | grep -m1 -E '^ +iPhone' | grep -oE '[0-9A-F-]{36}')
echo "simulator: $UDID"
xcrun simctl boot "$UDID" 2>/dev/null || true
xcrun simctl bootstatus "$UDID" -b >/dev/null
xcrun simctl uninstall "$UDID" com.osvauld.tinline 2>/dev/null || true
xcrun simctl keychain "$UDID" reset 2>/dev/null || true
status=0
start=$(date '+%Y-%m-%d %H:%M:%S')
xcodebuild test -project ios/Tinline.xcodeproj -scheme TinlineUI -destination "id=$UDID" -derivedDataPath "$DD" \
  -resultBundlePath "$OUT/result.xcresult" ${ONLY:+-only-testing:"$ONLY"} -quiet || status=$?
xcrun xcresulttool export attachments --path "$OUT/result.xcresult" --output-path "$OUT/raw" >/dev/null 2>&1 || true
# Name each exported file after its attachment ("01-welcome.png", ...).
python3 -I - "$OUT/raw" "$OUT/shots" <<'EOF'
import json, os, shutil, sys
raw, shots = sys.argv[1], sys.argv[2]
try:
    manifest = json.load(open(os.path.join(raw, "manifest.json")))
except OSError:
    sys.exit(0)
for test in manifest:
    for a in test.get("attachments", []):
        name = a.get("suggestedHumanReadableName") or a["exportedFileName"]
        shutil.copy(os.path.join(raw, a["exportedFileName"]), os.path.join(shots, name))
EOF
# The app's own log lines (NSLog) from the run, zipped with the shots.
xcrun simctl spawn "$UDID" log show --start "$start" --style compact --predicate 'process == "Tinline"' > "$OUT/shots/app.log" 2>&1 || true
grep -E 'Tinline: ' "$OUT/shots/app.log" | tail -20 || true
(cd "$OUT/shots" && ls && zip -q ../shots.zip ./* 2>/dev/null || true)
xcrun xcresulttool get test-results summary --path "$OUT/result.xcresult" 2>/dev/null | grep -E '"(result|failureText)"' | head -10 || true
if [ "$status" -eq 0 ]; then echo "UI TEST: passed"; else echo "UI TEST: FAILED (xcodebuild exit $status)"; fi
