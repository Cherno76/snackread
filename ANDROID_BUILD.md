# Android APK 编译说明（给 agent 看）

> 本文说明如何在 macOS 上把一个 Tauri 2 项目交叉编译成 Android APK 并安装到真机。
> **系统级工具链是这台机器共用的**，任何 Tauri 2 Android 项目都能直接用；
> **项目级脚手架（`src-tauri/gen/android`）每项目独立生成**。

## 1. 环境在哪里（本机已装好）

| 组件 | 路径 |
| --- | --- |
| Android SDK | /Users/cherno/Library/Android/sdk |
| NDK | /Users/cherno/Library/Android/sdk/ndk/26.3.11579264 |
| JDK 17 | /opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home |
| tauri-cli | cargo tauri（本机 2.11.4） |
| adb | /opt/homebrew/bin/adb（1.0.41） |

Rust 需要以下 Android 目标：

```text
aarch64-linux-android
armv7-linux-androideabi
i686-linux-android
x86_64-linux-android
```

缺目标时：

```sh
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
```

tauri-cli 缺失时：`cargo install tauri-cli --locked`。

## 2. 每个项目先初始化 Android 工程

在**该项目根目录**（含 `src-tauri/`）执行一次：

```sh
cargo tauri android init
```

生成 `src-tauri/gen/android/`（Gradle 工程、AndroidManifest.xml、build.gradle.kts、
gradlew、MainActivity.kt 等）。**不要跨项目复制该目录**，每个项目各自生成。

## 3. 关键：让 gradle 找到 SDK/NDK

本机**没有**持久化的 JAVA_HOME / ANDROID_HOME / NDK_HOME，`gen/android` 也**没有**
local.properties，所以构建前必须二选一。

### 方式 A：每次构建前 export（当前项目一致使用）

```sh
cd <项目根>
export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
export ANDROID_HOME=/Users/cherno/Library/Android/sdk
export NDK_HOME=/Users/cherno/Library/Android/sdk/ndk/26.3.11579264
cargo tauri android build --apk
```

### 方式 B：写项目级 local.properties（推荐，一劳永逸）

在 `src-tauri/gen/android/local.properties` 写：

```properties
sdk.dir=/Users/cherno/Library/Android/sdk
```

之后构建无需手工 export。

### 方式 C：写进 ~/.zshenv（本机全局，所有项目生效）

```sh
echo 'export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home' >> ~/.zshenv
echo 'export ANDROID_HOME=/Users/cherno/Library/Android/sdk' >> ~/.zshenv
echo 'export NDK_HOME=/Users/cherno/Library/Android/sdk/ndk/26.3.11579264' >> ~/.zshenv
```

## 4. 构建 / 安装 APK

```sh
cargo tauri android build --apk
```

产物：`src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk`

安装到连接的设备（-r 覆盖安装，保留应用数据）：

```sh
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk
adb devices -l
```

adb 守护进程可能被沙箱/防火墙挡：先 `adb start-server` 或提权。

## 5. 版本号与签名

### 版本号同步（三处一致）

打包部署前递增版本，并保持三处一致：

- src-tauri/Cargo.toml 的 version
- src-tauri/tauri.conf.json 的 version
- src-tauri/gen/android/app/tauri.properties 的 tauri.android.versionName 与 tauri.android.versionCode

APK 版本取自 tauri.properties；桌面 `--version` 取自 Cargo.toml。

### 签名（release）

本机签名配置：

- Keystore：/Users/cherno/.android/cshow-release.keystore
- 密码/别名：~/.gradle/gradle.properties 的 CSHOW_KEYSTORE_PASSWORD、CSHOW_KEY_ALIAS、CSHOW_KEY_PASSWORD
- 引用处：src-tauri/gen/android/app/build.gradle.kts 的 signingConfigs.release

> keystore 与包名/应用身份绑定。**换项目或换包名要另建 keystore 或改签名配置**。

### 验证

```sh
# APK 包名/版本
$ANDROID_HOME/build-tools/*/aapt dump badging app-universal-release.apk | grep -E 'package:'

# 是否为 debug（出现 DEBUGGABLE 或 debuggable=true 是 debug；release 无）
aapt dump xmltree app-universal-release.apk AndroidManifest.xml | grep -i debuggable

# 设备上已装版本
adb shell dumpsys package <applicationId> | grep -iE 'versionName|versionCode'
```

## 6. 常见问题

- **gradle 找不到 SDK/NDK**：没 export 环境变量，也没 local.properties。见第 3 节。
- **APK 里混入旧 .so**：cargo tauri android build 会把新库 symlink 进
  src-tauri/gen/android/app/src/main/jniLibs/；改过 crate 名后旧 lib<旧名>.so 会残留。
  清掉整个 jniLibs 再重建：`rm -rf src-tauri/gen/android/app/src/main/jniLibs`（jniLibs 是生成目录，建议 gitignore）。
- **应用包名**：applicationId / namespace 在 build.gradle.kts；改包名会影响设备上的
  应用身份与数据目录，谨慎。
- **Rust crate 名**：Cargo.toml 的 [lib] name 决定生成的 lib<name>.so；改 crate 名会改变 APK 内库名，
  Android 侧用 cargo tauri 自动发现。

## 7. 一句话流程

```sh
cd <项目根>
cargo tauri android init
export JAVA_HOME=/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home
export ANDROID_HOME=/Users/cherno/Library/Android/sdk
export NDK_HOME=/Users/cherno/Library/Android/sdk/ndk/26.3.11579264
cargo tauri android build --apk
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk
```
