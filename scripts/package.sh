#!/bin/bash
# 打包 SnackRead（macOS）：release 编译 -> 组装 .app -> 签名 -> zip -> 自动复制到 /Applications
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="SnackRead"
APP="$ROOT/dist/$APP_NAME.app"

cd "$ROOT/src-tauri"
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
ZIP="$ROOT/dist/$APP_NAME-$VERSION-macos.zip"

echo "== cargo build --release"
cargo build --release

echo "== 组装 $APP_NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/snack-read "$APP/Contents/MacOS/snack-read"
cp icons/icon.icns "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundleName</key><string>SnackRead</string>
  <key>CFBundleDisplayName</key><string>SnackRead</string>
  <key>CFBundleExecutable</key><string>snack-read</string>
  <key>CFBundleIdentifier</key><string>com.cherno.cshow-gui</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>__VERSION__</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict>
</plist>
PLIST
sed -i '' "s/__VERSION__/$VERSION/" "$APP/Contents/Info.plist"

echo "== codesign (ad-hoc)"
codesign --force --deep -s - "$APP"

echo "== zip"
rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

echo "== 清理旧版本 zip（只保留当前版本）"
find "$ROOT/dist" -maxdepth 1 -name 'SnackRead-*-macos.zip' ! -name "$(basename "$ZIP")" -delete

echo "== 复制到 /Applications"
rm -rf "/Applications/$APP_NAME.app"
ditto "$APP" "/Applications/$APP_NAME.app"

echo "== 完成"
echo "   $APP"
echo "   $ZIP"
echo "   /Applications/$APP_NAME.app"
