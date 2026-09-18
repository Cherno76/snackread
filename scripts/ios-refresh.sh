#!/bin/bash
# SnackRead iPhone 版自动续期
#
# 免费个人 team 的 provisioning profile 只有 7 天有效期，到期后 App 会打不开。
# 这个脚本在到期前一天开始尝试：删掉本地缓存的 profile（否则 Xcode 会复用旧的 7 天）
# → 重新构建 → 装回手机。由 launchd（com.cherno.snackread-ios-refresh）每小时调用，
# 平时秒退，只有临近/已过期时才真正干活。
#
# 用法：
#   scripts/ios-refresh.sh            # 给 launchd 用：只在需要时干活
#   scripts/ios-refresh.sh --status   # 只看当前到期时间
#   scripts/ios-refresh.sh --force    # 强制走一遍（验证「续签是否真的延长」用）
#
# 前置条件（缺一不可）：
#   - 手机在同一个 Wi-Fi、且处于可连接状态（锁屏/离线会失败，脚本会在下一次再试）
#   - Mac 已登录 Xcode 的 Apple ID（会话失效时构建会报 provisioning 错误）
#   - 登录钥匙串解锁（codesign 需要 Apple Development 私钥）
set -uo pipefail

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_DIR="$ROOT/src-tauri/gen/apple/build/snack-read_iOS.xcarchive/Products/Applications/SnackRead.app"
APP_PROFILE="$APP_DIR/embedded.mobileprovision"
XCODE_PROFILES="$HOME/Library/Developer/Xcode/UserData/Provisioning Profiles"
BUNDLE_ID="com.cherno.cshow-gui"
UDID="${SNACKREAD_IPHONE_UDID:-00008150-001555C82E13C01C}"
# 提前多少小时开始尝试
THRESHOLD_HOURS="${SNACKREAD_REFRESH_THRESHOLD_HOURS:-24}"
NOTIFY="$HOME/bin/notify-iphone.sh"
LOG="$HOME/Library/Logs/snackread-ios-refresh.log"
LOCK="/tmp/snackread-ios-refresh.lock"

log() { echo "$(date '+%F %T') $*" >>"$LOG"; }

notify() {
  [ -x "$NOTIFY" ] || return 0
  "$NOTIFY" "$1" "SnackRead 续期" >/dev/null 2>&1
}

# 读某个 profile 的过期时间（epoch 秒）
profile_expiry() {
  local file="$1" raw
  [ -f "$file" ] || return 1
  raw=$(security cms -D -i "$file" 2>/dev/null | plutil -extract ExpirationDate raw - 2>/dev/null) || return 1
  [ -n "$raw" ] || return 1
  python3 -c 'import datetime,sys;print(int(datetime.datetime.strptime(sys.argv[1],"%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=datetime.timezone.utc).timestamp()))' "$raw" 2>/dev/null
}

# 当前生效的到期时间：优先用上次构建产物里内嵌的那份（= 手机上装的），否则看 Xcode 缓存
current_expiry() {
  profile_expiry "$APP_PROFILE" && return 0
  local newest
  newest=$(ls -t "$XCODE_PROFILES"/*.mobileprovision 2>/dev/null | head -1)
  [ -n "$newest" ] || return 1
  profile_expiry "$newest"
}

# 手机是否真的连得上（配对但离线也会显示 available，所以要真发一条命令）
device_reachable() {
  xcrun devicectl device info files --device "$UDID" --domain-type appDataContainer \
    --domain-identifier "$BUNDLE_ID" --subdirectory Documents >/dev/null 2>&1
}

force=0
status_only=0
for arg in "$@"; do
  case "$arg" in
    --force) force=1 ;;
    --status) status_only=1 ;;
  esac
done

now=$(date +%s)
if expiry=$(current_expiry); then
  remain_h=$(( (expiry - now) / 3600 ))
else
  expiry=0
  remain_h=-999
fi

if [ "$status_only" = 1 ]; then
  if [ "$remain_h" = -999 ]; then
    echo "找不到 profile（还没构建过？），需要构建一次"
  else
    echo "profile 到期：$(date -r "$expiry" '+%F %T')（剩余 ${remain_h} 小时）"
  fi
  exit 0
fi

# 平时安静退出，不写日志
if [ "$force" != 1 ] && [ "$remain_h" != -999 ] && [ "$remain_h" -gt "$THRESHOLD_HOURS" ]; then
  exit 0
fi

# 互斥：上一次还在跑就跳过
if ! mkdir "$LOCK" 2>/dev/null; then
  log "上一次刷新还在进行，跳过"
  exit 0
fi
trap 'rmdir "$LOCK" 2>/dev/null' EXIT

log "开始刷新（剩余 ${remain_h}h，force=${force}）"

if ! device_reachable; then
  log "手机不可达（不在同一 Wi-Fi / 锁屏 / 离线），本次跳过，下次再试"
  exit 0
fi

# 删掉本地缓存的 profile，逼 Xcode 去 Apple 那边重新签发（不删它会复用旧的 7 天）。
# 先备份：构建失败就放回去，免得把「还能构建」这条后路也弄丢了。
BACKUP_DIR="/tmp/snackread-profile-backup"
rm -rf "$BACKUP_DIR"
mkdir -p "$BACKUP_DIR"
if compgen -G "$XCODE_PROFILES/*.mobileprovision" >/dev/null; then
  cp "$XCODE_PROFILES"/*.mobileprovision "$BACKUP_DIR/" 2>/dev/null
fi
rm -f "$XCODE_PROFILES"/*.mobileprovision

restore_profile_backup() {
  if compgen -G "$BACKUP_DIR/*.mobileprovision" >/dev/null; then
    mkdir -p "$XCODE_PROFILES"
    cp "$BACKUP_DIR"/*.mobileprovision "$XCODE_PROFILES/" 2>/dev/null
  fi
}

# 让 Xcode 去 Apple 那边注册设备 / 签发新 profile。
# 实测：不指定 destination（tauri 默认按 "Any iOS Device" 构建）时 Apple 会回
# "Your team has no devices from which to generate a provisioning profile"；
# 必须 -destination id=<手机 UDID> 再加 -allowProvisioningDeviceRegistration 才会签发。
# 这一步必然在 tauri 的 Rust 脚本阶段失败（缺 CLI 的 IPC），所以忽略退出码，
# 只看有没有新 profile 落进缓存。
( cd "$ROOT/src-tauri/gen/apple" && xcodebuild -allowProvisioningUpdates \
    -allowProvisioningDeviceRegistration -scheme snack-read_iOS \
    -workspace snack-read.xcodeproj/project.xcworkspace/ -sdk iphoneos \
    -configuration release -destination "id=$UDID" build ) >>"$LOG" 2>&1

if ! compgen -G "$XCODE_PROFILES/*.mobileprovision" >/dev/null; then
  restore_profile_backup
  log "签发新 profile 失败（Apple 没给）"
  notify "SnackRead iPhone 版续期失败：Apple 没能签发新的描述文件。请确认手机解锁并连着 Mac（USB 或同一 Wi-Fi）、Xcode 里 Apple ID 会话有效。一小时后再试。"
  exit 1
fi

if ! ( cd "$ROOT/src-tauri" && cargo tauri ios build --target aarch64 ) >>"$LOG" 2>&1; then
  restore_profile_backup
  log "构建失败（已把原 profile 放回缓存）"
  notify "SnackRead iPhone 版续期失败：构建报错。多半是 Apple ID 会话失效，或手机当时不在 Xcode 可用状态（锁屏/离线）。下个小时会再试。"
  exit 1
fi

if new_expiry=$(profile_expiry "$APP_PROFILE"); then :; else new_expiry=0; fi

if ! xcrun devicectl device install app --device "$UDID" "$APP_DIR" >>"$LOG" 2>&1; then
  log "安装失败（手机可能锁屏/离线）"
  notify "SnackRead 续期：已重新构建，但装到 iPhone 失败，下次再试"
  exit 1
fi

if [ "$new_expiry" != 0 ] && [ "$new_expiry" -gt "$expiry" ]; then
  log "续期成功，新到期：$(date -r "$new_expiry" '+%F %T')"
  notify "SnackRead iPhone 版已自动续期，新到期：$(date -r "$new_expiry" '+%m-%d %H:%M')"
else
  log "已重新构建并安装，但到期时间没变（$new_expiry）——Apple 复用了同一个 profile，等过期后再试"
  notify "SnackRead：已重新构建并安装，但 profile 到期时间没变（Apple 复用了同一个）。过期后会自动再试。"
fi
