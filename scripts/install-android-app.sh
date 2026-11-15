#!/usr/bin/env bash
# Builds the release APK for 64-bit ARM phones, signs it with the debug keystore (so it updates the
# installed app in place, keeping its data) and installs it on the phone connected over adb.
set -euo pipefail

: "${ANDROID_HOME:=$HOME/Android/Sdk}"
: "${JAVA_HOME:=$HOME/.local/share/JetBrains/Toolbox/apps/android-studio/jbr}"
: "${NDK_HOME:=$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}"
build_tools="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
export ANDROID_HOME JAVA_HOME NDK_HOME
export PATH="$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$build_tools:$PATH"

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
pnpm tauri android build --apk --target aarch64

unsigned="src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk"
out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT
zipalign -f -p 4 "$unsigned" "$out/aligned.apk"
apksigner sign --ks "$HOME/.android/debug.keystore" --ks-pass pass:android \
  --ks-key-alias androiddebugkey --out "$out/cigmail.apk" "$out/aligned.apk"
adb install -r "$out/cigmail.apk"
