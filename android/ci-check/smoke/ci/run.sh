#!/bin/bash
# Installs the smoke test APK on the connected device or emulator and runs it. Usage: run.sh [apk] (default: the APK of
# `gradle -p android/ci-check :sentinel-smoke:assembleDebugAndroidTest`). Leaves instrument.txt and logcat.txt in the
# current directory. Exit code 0 only when every test passed.
set -u
here=$(cd "$(dirname "$0")" && pwd)
apk=${1:-$here/../build/outputs/apk/androidTest/debug/sentinel-smoke-debug-androidTest.apk}
adb=${ADB:-adb}
[ -f "$apk" ] || { echo "no APK at $apk"; exit 2; }
pkg=com.mobilesentinel.smoke.test
sdk=$("$adb" shell getprop ro.build.version.sdk | tr -d '\r')
echo "device: API $sdk"
"$adb" install -r -t -g "$apk" || exit 2
# Android 12 and 13 ask for the exact alarm permission in a settings page: allow it the way that page does.
"$adb" shell cmd appops set $pkg SCHEDULE_EXACT_ALARM allow > /dev/null 2>&1
# Android 14 and newer ask for the full-screen intent permission in a settings page too.
"$adb" shell cmd appops set $pkg USE_FULL_SCREEN_INTENT allow > /dev/null 2>&1
su0() { "$adb" shell su 0 "$@" 2> /dev/null; }
su0 logcat -c || "$adb" logcat -c
timeout 600 "$adb" shell am instrument -w -r -e class com.mobilesentinel.smoke.DeviceSmokeTest \
    $pkg/androidx.test.runner.AndroidJUnitRunner > instrument.txt 2>&1
rc=$?
"$adb" logcat -d > logcat.txt 2>&1
grep -E "^(OK|FAILURES)|INSTRUMENTATION_(STATUS|RESULT)(_CODE)?: (test=|class=|stack|Error|shortMsg|-1|-2)" instrument.txt | head -40
if [ $rc -eq 0 ] && grep -q '^OK (' instrument.txt; then echo "PASS  $(grep '^OK (' instrument.txt)"; exit 0; fi
echo "FAIL  (rc $rc)"
grep -A25 'stack=' instrument.txt | head -80
exit 1
