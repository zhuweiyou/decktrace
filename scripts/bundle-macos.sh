#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

ROOT="$(pwd)"
cargo build --release -p decktrace

APP="$ROOT/dist/decktrace.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources/assets"

cp "$ROOT/target/release/decktrace" "$APP/Contents/MacOS/decktrace"
# 游戏 crate 约定：crates/decktrace-<game>/，且 <game> == GameProfile.id。
# 只有带 src/profile.rs 的 crate 才处理，避免误拷基础设施 crate 的 assets。
for game_dir in "$ROOT"/crates/decktrace-*/assets; do
    crate_dir="$(dirname "$game_dir")"
    [ -f "$crate_dir/src/profile.rs" ] || continue
    game="$(basename "$crate_dir")"
    game="${game#decktrace-}"
    dest="$APP/Contents/Resources/assets/$game"
    if [ -e "$dest" ]; then
        echo "重复的游戏资源目录：$game" >&2
        exit 1
    fi
    cp -R "$game_dir" "$dest"
done
cp "$ROOT/crates/decktrace/assets/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"

VERSION="$(cargo pkgid -p decktrace 2>/dev/null | sed 's/.*#//')"
[ -n "$VERSION" ] || VERSION="0.1.0"

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
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleDisplayName</key>
    <string>斗地主记牌器</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>__VERSION__</string>
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

sed -i '' "s/__VERSION__/$VERSION/" "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# 本机自签名，便于 TCC 稳定识别；正式分发需要 Developer ID 签名和公证。
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true

echo "$APP"
