# SnackRead iPhone (iOS) 运行方案

> 目标：让 SnackRead 在 iPhone/iPad 上跑起来。与 Android 一样用 Tauri 2 官方 iOS 支持；Rust 后端交叉编译成静态库，Web UI 直接跑在 WKWebView 里。

## 当前进度（2026-08-26）

- iOS Rust 编译目标已装：`aarch64-apple-ios` / `aarch64-apple-ios-sim` / `x86_64-apple-ios`
- Xcode 26.6 已装，许可已接受，`xcode-select` 已切到 Xcode
- iOS 26.5 模拟器运行时已装（`xcodebuild -downloadPlatform iOS`）
- `xcodegen` + `cocoapods` 已安装（brew）
- iOS 工程已生成：`src-tauri/gen/apple/snack-read.xcodeproj`（Tauri 2 用统一的 `gen/apple`）
- 后端路径已适配：iOS 起始目录 → 应用容器 `Documents`（用户导入的书在这）；工作目录（解包缓存/缩略图/SQLite）→ `Library/Application Support/cshow-gui/cshow-work`（见 `lib.rs` 的 `ios_default_start_dir` / `default_work_dir`）
- Info.plist 已开文件导入：`UIFileSharingEnabled` + `LSSupportsOpeningDocumentsInPlace`，并声明 EPUB/PDF/TXT/ZIP 等文档类型（`project.yml` + 生成的 `Info.plist`）
- **✅ 模拟器构建成功并运行**：`cargo tauri ios build --target aarch64-sim` →
  `src-tauri/gen/apple/build/arm64-sim/SnackRead.app`。已在 iPhone 17 Pro (iOS 26.5) 安装启动，
  截图确认：SnackRead 标题栏、空书库页、底部状态栏渲染正常，无崩溃；
  数据容器里 `Library/Application Support/cshow-gui/cshow-work/`（epub/thumbs/library.sqlite3）自动创建，
  `Documents` 保持干净（留给用户导入的书）。
- **✅ iOS 图标已对齐 Android**：`cargo tauri icon src-tauri/icons/icon.png` 重生成，
  iOS 的 `Assets.xcassets/AppIcon.appiconset/` 现为 SnackRead 女孩 logo（与 Android `ic_launcher` 一致）。
- 待办：真机签名（Apple Developer Team）、导入真实书籍验证 `book://` 协议与朗读（Web Speech）等完整阅读链路。

## 2026-09-18 Xcode 27 适配（真机 arm64）

Xcode 从 `~/Downloads/Xcode-beta.app` 换成了官方的 `/Applications/Xcode.app`（27.0 / iOS SDK 27.0），
旧的临时补丁失效，同时暴露出三个 Xcode 27 相关的坑，都已处理：

| 问题 | 现象 | 处理 |
| --- | --- | --- |
| 链接路径指向已删除的 Xcode-beta | `-L /Users/cherno/Downloads/Xcode-beta.app/...` 不存在 | `src-tauri/.cargo/config.toml` 改指 `/Applications/Xcode.app`（另：tauri 自己的 build script 现在也会输出正确路径，这段补丁已属冗余） |
| swift-rs 1.0.7 不认 Xcode 27 的 SwiftPM | swiftc 被追加 `-sdk MacOSX -target arm64-apple-macos12.0`，手机端包按 macOS 编译，报 `OpenGLES/EAGL.h not found`、`could not build module 'CoreServices'` | 升级到 **swift-rs 1.0.8**（2026-08-17 发布，含 `--triple`、部署目标下限、产物目录重定位三项 Xcode 27 修复） |
| Xcode 27 把静态库里 `@_cdecl` 导出符号内部化 | 1.0.8 只恢复「本包模块」的符号，`SwiftRs.o` 的 `_string_from_bytes` / `_retain_object` / `_release_object` 仍是 `t`，Rust 链接报 `Undefined symbols` | 1) `rustup component add llvm-tools`（swift-rs 靠它调 `llvm-objcopy --globalize-symbol`）；2) `src-tauri/vendor/swift-rs` 打了本地补丁：`globalize_cdecl_symbols` 处理归档里**所有**成员，经 `[patch.crates-io]` 生效 |

另外，swift-rs 1.0.8 通过 `xcrun --sdk iphoneos/iphonesimulator` 自己定位 SDK，不再依赖
`IPHONEOS_DEPLOYMENT_TARGET` 环境变量；iOS 部署目标仍由 `project.yml` 的 `deploymentTarget` 决定（15.0）。

验证结果：`cargo tauri ios build --target aarch64` 在**关闭签名**的前提下走完了 Rust 交叉编译 →
Swift 包 → Xcode 打包全流程，产出 `gen/apple/build/arm64/SnackRead.ipa`（未签名，仅用于验证编译链）。

仍然阻塞真机安装的唯一环节是 **Xcode 的 Apple ID 登录**：

```
error: Unable to log in with account 'cherno@gmail.com'. The login details for account
'cherno@gmail.com' were rejected.
error: No profiles for 'com.cherno.cshow-gui' were found
```

证书本身没问题（`Apple Development: cherno@gmail.com (MDJ2YCD8LH)`，team `8AX6Q9V88D`，有效期到 2027-08-26），
但 `~/Library/Developer/Xcode/UserData/Provisioning Profiles/` 是空的，且 Xcode 无法用已失效的会话去换新 profile。
在 Xcode → Settings → Accounts 重新登录一次（需要密码 + 两步验证）后，直接重新执行：

```sh
cargo tauri ios build --target aarch64
```

### 真机安装后启动闪退（UIScene 生命周期，2026-09-18）

签名搞定后装上真机仍然闪退，串起来一共四层，都在 `gen/apple` 配置和 tao 里：

| 现象（崩溃栈） | 根因 | 处理 |
| --- | --- | --- |
| `EXC_BREAKPOINT` / `signal 5`，`_UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption` → 设备日志写 *"UIScene life cycle is required for apps built with this SDK"* | Info.plist 里没有 `UIApplicationSceneManifest`。iOS 26/27 SDK 构建的 App 必须采用 UIScene 生命周期，否则 UIKit 在首个 scene 创建时直接终止 | `project.yml` + 生成的 `Info.plist` 增加 `UIApplicationSceneManifest`（`tao` 已有 `TaoSceneDelegate`，只差声明） |
| 仍闪退，同样落在上面那个 runtime issue | tao 0.35.3 只在 `multiple_scenes_enabled()` 为真时才 `add_method(application:configurationForConnectingSceneSession:options:)`；声明了 manifest 却没有回调，UIKit 仍判定未采用 | `vendor/tao` 按上游 `a3ff3f03` 改成**无条件注册** |
| `EXC_BAD_ACCESS` / `signal 11`，`-[UIApplication _connectUISceneFromFBSScene:…]` → `objc_retain` | tao 用 `Retained::as_ptr(&config)` 返回 `UISceneConfiguration`，函数返回后对象随 `Retained` 释放，UIKit 拿到悬垂指针 | `vendor/tao` 按上游 `f2163508` 改用 `Retained::autorelease_ptr(config)`（返回 +0 的 autorelease 指针） |
| `EXC_CRASH` / `signal 6`，`abort()`，栈里 tao 帧在 `_sceneForFBSScene:` 之下 | `UIApplicationSupportsMultipleScenes = false` 会让 tao 在 `did_finish_launching` 先调一次 `on_app_ready()`，随后 `connect_scene` 再调一次，第二次命中 `bug!("unexpected state")` | manifest 里 `UIApplicationSupportsMultipleScenes` 设为 **true**（iPhone 只支持单场景，不会真开多窗口） |

最终状态：`cargo tauri ios build --target aarch64` → 签名 → 安装 → 启动正常，
`Library/Application Support/cshow-gui/cshow-work/library.sqlite3` 与 `Library/WebKit/` 都有写入，
说明 Rust 后端与 WKWebView 都活着。

> `tauri-runtime-wry` 2.11.4 锁的是 `tao = "0.35.0"`，拿不到 0.36/0.37 的修复，
> 所以 `src-tauri/vendor/tao` 是 0.35.3 的本地副本 + 上面两处改动，
> 经 `[patch.crates-io]` 生效。tauri 放宽 tao 版本后可以删掉。

### 书库访问容器外的文件夹（系统文件夹选择器 + security-scoped bookmark）

iOS 沙盒下应用只能读自己容器里的东西。原来的「书库管理」是应用内浏览（`list_dir`
在容器里上下走），所以往上翻只有 `Documents / Library / tmp`，看不到 iCloud Drive、
其他 App 的「我的 iPhone」目录、外接存储——不是 bug，是沙盒。

现在的做法（v0.5.76+）：

1. 书库管理对话框在 iOS 上多一个 **📂 选择其他文件夹…**（`#lib-pick-row`，桌面端隐藏）。
2. 点击后由 `main.mm` 弹 `UIDocumentPickerViewController`（`UTTypeFolder`），用户选中即授权。
3. 选中后立刻 `startAccessingSecurityScopedResource`，并把 `security-scoped bookmark`
   存到容器内 `Library/Application Support/cshow-gui/ios_bookmarks.json`
   （`save_ios_bookmark`）；选中的路径直接作为书库浏览位置，接着点「添加此文件夹」即可。
4. 每次启动 `run()` 里调 `restore_ios_bookmarks()`，把 bookmark 重新 resolve 并恢复访问，
   所以书库指向 iCloud Drive 之类的目录也能跨启动保留。

> **FFI 方向**：Rust 调 ObjC 会让 iOS 的 `cdylib` 链接报
> `Undefined symbols: _snackread_pick_folder`（main.mm 的符号在 app target 里，cdylib
> 链接时还看不见）。所以改成 ObjC → Rust：`main()` 里先把
> `snackread_pick_folder` / `snackread_restore_bookmark` 通过
> `snackread_register_bridge` 交给 Rust（`src/lib.rs`），Rust 只持有函数指针。
> 以后往 iOS 加原生能力时沿用这个方向。

> `gen/apple/Sources/snack-read/main.mm` 和 `project.yml` / `Info.plist` 一样是可再生成
> 目录里的手工改动；重跑 `cargo tauri ios init` 会被覆盖，需要照本节重新加回。

### iOS data container 路径会变（v0.5.81+ 已修）

iOS 的 `Data/Application/<UUID>` 这个 UUID **不是长期稳定的**：每次重装、甚至系统升级后
都会换一个（本机实测一天内换过三次）。所以数据库里存**绝对路径**的字段一旦落在应用容器内，
下次启动就会全部指向不存在的文件——表现就是「元数据还在（存在 SQLite 里），封面/进度却没了」。

两处修法：

- 封面改为只存**文件名**（`covers/` 目录内），读取时用 `resolve_cover_path()` 现拼绝对路径。
- 启动时 `repair_container_paths()` 扫一遍库，把指向旧容器、且当前容器下同名文件确实存在的
  记录改写回当前路径；覆盖 `libraries.path`、`books.path`、`books.cover`、
  `positions.volume_path`、`settings.scope_path`、`dir_state.path` 和 `app_state.cwd`。

> 书库放在 iCloud Drive / 文件 App 的目录不受影响（那些在别的容器里，路径稳定）；
> 但放在应用自己的 `Documents` 下就会踩这个坑，所以不要持久化容器内的绝对路径。

### 外部数据目录（v0.5.82+）

应用容器里的东西重装/卸载就没了（UUID 还会变）。所以把工作目录（DB、缩略图、解包缓存、
封面）搬到**「我的 iPhone」下的目录**——那属于本地文件提供者容器，路径稳定，重装也还在。

- 用户在「书库管理 → 数据目录」或首次启动提示里选一个**父目录**，真正的工作目录是
  `<父目录>/.SnackRead`（点开头，书库扫描会跳过）。
- 位置以 security-scoped bookmark 为准（`app_config_dir()/ios_workdir.json`），
  不持久化绝对路径；启动时 `ios_startup_restore()` 解析 bookmark → 建目录 → 必要时迁移。
- **必须在 `work_dir()` 之前调用**（`run()` 开头），否则解析出的还是应用内部目录。
- 首次切换会把旧工作目录整个搬过去（同卷 rename）；如果目标里已经有 `library.sqlite3`
  就直接接管（全新安装后重新指定同一个目录就是这条路，等于恢复数据）。
- `set_work_dir` 与 `app_config_dir()/workdir` 在 iOS 上被禁用/忽略：那里存的是绝对路径，
  容器 UUID 一变就会让 `db::open()` 的 `create_dir_all` 失败，启动直接 panic。
- 不要选「我的 iPhone → SnackRead」——那是应用自己的 `Documents`，仍在容器里，重装照丢。

> 迁移之后 `cshow-work`（旧位置）会空掉；`presets.json` 仍留在 `Documents/`，方便用户拖文件。

### 阅读模式隐藏系统状态栏（v0.5.78+）

`Info.plist` 没设 `UIViewControllerBasedStatusBarAppearance`，默认 `YES`：系统状态栏由
「状态栏控制器」（窗口的根视图控制器）的 `prefersStatusBarHidden` 决定。

根控制器是 tao 在 Rust 里创建的，没法子类化，所以 `main.mm` 用 **isa-swizzling**：
首次调用时给根控制器实例换上一个动态子类（`objc_allocateClassPair` +
`class_addMethod` 覆写 `prefersStatusBarHidden` + `object_setClass`），再
`setNeedsStatusBarAppearanceUpdate`。选这个方案而不是在 `UIViewController` 上加 category
或改 Info.plist，是因为：category 会波及文件选择器等其它控制器；改 Info.plist 是全局
生效（书库界面也要跟着变）；换根控制器（container VC）则会影响旋转方向等交给根控制器的
决策。isa-swizzling 只影响这一个实例，也不动视图层级。

前端在 `setFocus()` 里切阅读模式时调用 `invoke('set_status_bar_hidden', { hidden })`，
退出阅读再恢复。

## 环境要求（本机现状）

| 组件 | 状态 |
| --- | --- |
| Xcode 27.0（iOS SDK 27.0 + 模拟器运行时） | 已装于 `/Applications/Xcode.app`，`xcode-select` 已指向它 |
| rustup `llvm-tools` | 已装（`llvm-objcopy`，Xcode 27 链接必需） |
| iOS Rust targets | 已装 |
| xcodegen | 已装（`/opt/homebrew/bin/xcodegen`） |
| CocoaPods | 已装（`/opt/homebrew/bin/pod`） |
| tauri-cli | 2.11.4 |

## 为什么必须装 Xcode

Tauri 的 iOS 构建在 `cargo tauri ios xcode-script` 里调用 `xcrun --sdk iphoneos`；`rusqlite`(bundled sqlite)、`image` 等 `cc`-编译的 C 依赖也需要 iOS SDK 头文件。没有 Xcode，iOS 交叉编译会在 C 依赖的 build script 处失败，进不到 App 本体。

装好后：`sudo xcode-select -s /Applications/Xcode.app/Contents/Developer`

## 后续步骤（装好 Xcode 后）

```sh
cd <项目根>
# 模拟器构建 / 运行
cargo tauri ios build --target aarch64-apple-ios-sim
cargo tauri ios dev --target aarch64-apple-ios-sim
# 真机（需配置 Apple Developer Team，或设 APPLE_DEVELOPMENT_TEAM）
cargo tauri ios build --target aarch64-apple-ios
```

## 文件导入模型（iOS）

- 主路径（零代码）：开 `UIFileSharingEnabled` + `LSSupportsOpeningDocumentsInPlace` 后，用户把书拖进「文件」App 的 *On My iPhone → SnackRead*（或连接电脑的 Finder）。后端 `initial_dir` 指向容器 `Documents`，扫描/阅读不受影响。
- **元数据预置库**：`presets.json` 由 iOS 的 `external_presets_path()` 定位到应用 `Documents/presets.json`
  （[lib.rs](src-tauri/src/lib.rs) 的 `external_presets_path`）。也就是把桌面上「导出元数据」生成的
  `presets.json` 用 Finder 文件共享拖进 **SnackRead 文件夹**即可；App 启动时读取，无元数据的书会自动填充。
  书库网格只会显示文件夹/PDF/EPUB/TXT，纯 `.json` 会被过滤，不会混进书库。
- 增强（可选，未实现）：从其他 App「分享为 / 打开方式」把书拷进 App，需要给 iOS 入口（`ffi::start_app`，见 `main.mm`）接一个 `application(_:open:options:)` 处理，把外部文件复制到 `Documents`。当前 `CFBundleDocumentTypes` 已声明，但处理器尚未接。

## 前端适配状况

- 安全区：`ui/index.html` 已带 `viewport-fit=cover`，`style.css` 已用 `env(safe-area-inset-top/bottom)`，iOS 无需改。
- 触摸：`ui/app.js` 的 `IS_TOUCH` 已含 `iPhone|iPad|iPod`，与 Android 共用触屏逻辑。
- 朗读：iOS 无 `window.AndroidTts`，自动回退到 Web Speech API（`ttsEngine === 'web'`）。WKWebView 支持 `speechSynthesis`；暂停/进度用字符范围事件，fallback 到整句重读。
- 状态栏（电池/网络）：iOS 无 `window.AndroidStatus`，前端优雅隐藏电池/网络，只显示时间。
- `book://` 协议：iOS 走非 Android 分支即 `book://localhost`。WKWebView 对注册的 `book` 自定义 scheme 通常可用；若实测加载白屏，把 `ui/app.js` 第 8 行的 `BOOK_ORIGIN` 改成与 Android 一致用 `http://book.localhost` 兜底。

## 风险 / 待验证

- `gen/apple` 是可再生成目录：跑 `cargo tauri ios init` 会覆盖 `project.yml` 与 `xcodeproj`。改过 `project.yml` 后重新生成一次：`cd src-tauri/gen/apple && xcodegen generate`。
- 签名/真机：免费个人账号可侧载（7 天过期），长期用需 Apple Developer Program（$99/年）。
- 长书 / 大 PDF / 横向翻页性能需真机验证（类比 Android Phase 3/4）。

## 一句话流程

```sh
# 先装 Xcode，再：
cargo tauri ios build --target aarch64-apple-ios-sim
```
