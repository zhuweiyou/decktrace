#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

ROOT="$(pwd)"
cargo build --release -p decktrace

APP="$ROOT/dist/decktrace.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/assets"

cp "$ROOT/target/release/decktrace" "$APP/Contents/MacOS/decktrace"
cp -R "$ROOT/crates/decktrace-jj/assets" "$APP/Contents/Resources/assets/jj"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>zh_CN</string>
    <key>CFBundleExecutable</key>
    <string>decktrace</string>
    <key>CFBundleIdentifier</key>
    <string>com.zhuweiyou.decktrace</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>decktrace</string>
    <key>CFBundleDisplayName</key>
    <string>斗地主记牌器</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>12.3</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.games</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSScreenCaptureUsageDescription</key>
    <string>需要截取斗地主窗口来识别牌面并显示剩余牌。</string>
</dict>
</plist>
PLIST

printf 'APPL????' > "$APP/Contents/PkgInfo"

# 本机自签名，便于 TCC 稳定识别；正式分发需要 Developer ID 签名和公证。
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

echo "$APP"
