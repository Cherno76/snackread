# cshow-android Android 运行方案

> 目标：让 cshow 阅读器在 Android 手机上跑起来。
> 结论：**可行，采用 Tauri 2 官方 Android 支持**——Rust 后端几乎原样交叉编译，
> Web UI 直接跑在 Android WebView 里。主要工作量在存储路径、书籍导入方式和触摸交互适配。

## 当前进度（2026-08-17）

- ✅ Phase 1 工具链：JDK 17、Android SDK（platform 35/36、build-tools 35/36）、NDK r26d、
  Rust Android 目标、tauri-cli 2.11.4
- ✅ Phase 2 最小可跑：Android 工程已生成（`src-tauri/gen/android`），
  后端路径已适配到应用内部存储（`/data/user/0/com.cherno.cshow_gui/files`），
  APK 构建成功：`src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk`
- ⏳ 待真机验证：手机开 USB 调试后 `adb install -r <apk>`
- ⏳ Phase 3 触摸交互适配（hover→tap、导入入口）未开始

## 1. 可行性分析

### 可以直接复用的部分

- **Rust 后端**：所有依赖（rusqlite bundled、image、zip、roxmltree、ureq、regex 等）
  都支持 Android 交叉编译，EPUB 解包、缩略图、SQLite、`book://` 协议逻辑无需重写。
- **Web 前端**：`ui/` 是纯静态页面，pdf.js 可运行于 Android WebView；
  CSP 已允许 `wasm-unsafe-eval`，正好满足 pdf.js 的 wasm 需求。
- **`book://` 自定义协议**：Tauri 2 的 asset protocol 在 Android 上同样工作。

### 需要改造的部分

| 问题 | 现状（桌面） | Android 改造 |
| --- | --- | --- |
| 工作目录 | `~/Documents/cshow-work`（`dirs::document_dir()`） | 改为应用内部存储目录（`/data/user/0/<包名>/files/...`），用 `cfg(target_os = "android")` 分支显式指定，不依赖 `dirs` 在 Android 上的行为 |
| 书籍导入 | 添加本地任意文件夹为书库 | 第一版：应用内部 `Books/` 目录，用户通过 USB / 文件管理器 / 浏览器下载放入；第二版：接 SAF（Storage Access Framework）选目录 |
| 交互 | 依赖 hover（备注悬停、导航悬停）、键盘快捷键（`Tab`/`←→`/`A-`/`A+`） | hover 改为 tap/长按；阅读控制按钮常显或触摸唤起；键盘快捷键保留（蓝牙键盘可用），但不作为唯一入口 |
| 窗口 | 1350×845 桌面窗口 | Android 全屏，由系统 WebView 接管，无需改 |
| 版本号 | `Cargo.toml` 0.4.36，`tauri.conf.json` 0.3.0（不一致） | 打包前统一同步到 `Cargo.toml` 的版本 |

## 2. 需要安装的工具链（本机现状：均未安装）

```text
JDK 17            Android Gradle Plugin 需要        （brew install --cask temurin@17）
Android SDK       cmdline-tools + platform + build-tools
NDK               Android 交叉编译 Rust 需要          （建议 r26+）
adb               ✅ 已有（/opt/homebrew/bin/adb）
Rust targets      aarch64-linux-android / armv7-linux-androideabi / i686-linux-android / x86_64-linux-android
tauri-cli         cargo install tauri-cli --locked
```

Android SDK + NDK 下载量约 3~5 GB，首次全量构建（Rust 交叉编译 + Gradle 拉依赖）
需要较长时间（预计 30 分钟以上，视网速）。

## 3. 构建与运行流程

```sh
# 1) 脚手架：生成 src-tauri/gen/android（Gradle 工程 + AndroidManifest）
cargo tauri android init

# 2) 构建 APK（debug / release）
cargo tauri android build --apk

# 3) 安装到手机（USB 调试或 adb over Wi-Fi）
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk

# 开发热更新（连接真机/模拟器直接跑）
cargo tauri android dev
```

## 4. 实施阶段

### Phase 1 — 工具链（需网络下载，约 3~5 GB）

1. 安装 JDK 17（temurin）
2. 安装 Android SDK command line tools，接受 licenses，装 platform-34 / build-tools / NDK
3. `rustup target add` 四个 Android target
4. `cargo install tauri-cli --locked`

### Phase 2 — 最小可跑（脚手架 + 打通启动）

1. `cargo tauri android init` 生成 Android 工程
2. 后端加 `cfg(target_os = "android")` 的工作目录逻辑，确保首次启动能建库
3. 内置一个示例书库（`Books/`），验证扫描 / 图片阅读 / PDF 阅读链路
4. 构建 APK，真机安装，确认能启动、能浏览

### Phase 3 — 适配（体验可用）

1. 书籍导入：把文件放进应用 `Books/` 目录即可扫描；加「导入」入口引导
2. 触摸交互：hover 提示改 tap；阅读控制按钮触摸常显策略；横向滑动翻页（已有触控板逻辑，验证触摸事件）
3. 系统栏 / 全屏 / 竖屏横屏适配
4. 同步 `tauri.conf.json` 版本号与 `Cargo.toml` 一致

### Phase 4 — 真机验证

- 需要一台 Android 手机（开 USB 调试）或本机模拟器
- 验证：图片条漫、PDF、EPUB（文字书 + 图像书）、进度记忆、AI 元数据

## 5. 风险与备注

- `dirs` crate 在 Android 上的路径行为需要实测，若不可靠就改用 Tauri 提供的 path API 或直接读 `$HOME`。
- PDF 在大屏手机上字号/分页需要微调（pdf.js 按 viewport 渲染，理论上自适应）。
- 手机上打开超大 PDF / 大量图片的性能取决于机型和 WebView 版本。
- 第一版不追求「选择任意目录」，先保证「能跑、能读」；SAF 选目录作为第二版增强。
