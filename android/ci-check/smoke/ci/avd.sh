#!/bin/bash
# The emulator of the device smoke test on a Linux runner (the same recipe as AlarmFree's devtest/ci/make_avd.sh and
# devtest/linux.sh, which run its suite on GitHub's machines).
# Usage: avd.sh make <sdk28|sdknewest>    makes the AVD (needs ANDROID_HOME)
#        avd.sh start <sdk28|sdknewest>   starts it in the background and returns
#        avd.sh wait                      waits until it has booted
set -euo pipefail
cmd=$1 name=${2:-}
export ANDROID_AVD_HOME="${ANDROID_AVD_HOME:-$HOME/.android/avd}"
bin="$ANDROID_HOME/cmdline-tools/latest/bin"
[ -d "$bin" ] || bin=$(dirname "$(ls "$ANDROID_HOME"/cmdline-tools/*/bin/sdkmanager | head -1)")
adb="$ANDROID_HOME/platform-tools/adb"
port=5554

case "$name" in
    sdk28) api=28 tag=google_apis audio=yes ;;
    sdknewest) api=37.2 tag=google_apis_ps16k audio=no ;;
    "") ;;
    *) echo "unknown AVD $name"; exit 1 ;;
esac

case "$cmd" in
make)
    image="system-images;android-$api;$tag;x86_64"
    yes | "$bin/sdkmanager" --licenses > /dev/null || true
    "$bin/sdkmanager" --install "$image" "emulator" "platform-tools" > /dev/null
    mkdir -p "$ANDROID_AVD_HOME"
    echo no | "$bin/avdmanager" create avd -n "$name" -k "$image" -d pixel --force
    # avdmanager of cmdline-tools 12.0 writes target=android-0 for the newest images; the emulator then does not know the
    # API level and the guest aborts in the graphics mapper (AlarmFree devtest/ci/avd_target.sh).
    sed -i "s/^target=android-0\r\?$/target=android-$api/" "$ANDROID_AVD_HOME/$name.ini"
    grep -q "^target=android-$api\r\?$" "$ANDROID_AVD_HOME/$name.ini" || { echo "AVD $name has no target=android-$api:"; cat "$ANDROID_AVD_HOME/$name.ini"; exit 1; }
    cat >> "$ANDROID_AVD_HOME/$name.avd/config.ini" << EOC
hw.lcd.width=720
hw.lcd.height=1280
hw.lcd.density=320
hw.ramSize=2048
vm.heapSize=256
hw.audioInput=$audio
hw.audioOutput=$audio
hw.camera.back=none
hw.camera.front=none
hw.gps=no
hw.keyboard=yes
hw.gpu.enabled=yes
hw.gpu.mode=swiftshader_indirect
disk.dataPartition.size=6G
hw.cpu.ncore=4
EOC
    echo "AVD $name ready (API $api, image $tag)"
    emu_rev=$(sed -n 's/^Pkg.Revision=\([0-9.]*\).*/\1/p' "$ANDROID_HOME/emulator/source.properties")
    img_rev=$(sed -n 's/^Pkg.Revision=\([0-9.]*\).*/\1/p' "$ANDROID_HOME/system-images/android-$api/$tag/x86_64/source.properties")
    echo "emulator $emu_rev, image android-$api $tag r$img_rev"
    ;;
start)
    setsid nohup "$ANDROID_HOME/emulator/emulator" -avd "$name" -no-window -no-audio -no-boot-anim -no-snapshot \
        -gpu swiftshader_indirect -port $port > emulator.log 2>&1 < /dev/null &
    echo "emulator started"
    ;;
wait)
    "$adb" start-server > /dev/null 2>&1
    timeout 600 "$adb" wait-for-device
    end=$((SECONDS + 600))
    until [ "$("$adb" shell getprop sys.boot_completed 2> /dev/null | tr -d '\r')" = 1 ]; do
        [ $SECONDS -lt $end ] || { echo "the emulator did not finish booting in 10 minutes"; exit 1; }
        sleep 3
    done
    until "$adb" shell pm path android 2> /dev/null | grep -q package; do sleep 2; done
    # No animations, and the screen on, so the first touch of the smoke test does not wait for either.
    for s in window_animation_scale transition_animation_scale animator_duration_scale; do "$adb" shell settings put global $s 0; done
    "$adb" shell input keyevent KEYCODE_WAKEUP
    echo "booted: Android $("$adb" shell getprop ro.build.version.release | tr -d '\r') (API $("$adb" shell getprop ro.build.version.sdk | tr -d '\r')), after ${SECONDS}s"
    ;;
*) echo "usage: avd.sh make|start|wait"; exit 1 ;;
esac
