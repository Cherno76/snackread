# SnackRead 项目约定 (styling / naming)

> 产品名为 **SnackRead**，但内部标识刻意保持原 cshow-gui 命名，详见下方「标识与命名」。

## 标识与命名（刻意保留）

- **产品名 / 显示名**：`SnackRead`（macOS app、窗口标题、zip 文件名均用此名）。
- **包标识符**：`com.cherno.cshow-gui`。
- **可执行文件名**：`snack-read`（`src-tauri` crate 名 / 二进制名，`CFBundleExecutable`
  与 app 内二进制均为 `snack-read`）。
- **配置目录**：`~/Library/Application Support/cshow-gui`（`src-tauri/src/lib.rs`
  的 `app_config_dir()` 使用 `cshow-gui` 子目录）。

这些是**刻意**的：保持 macOS 应用身份、沿用既有用户数据与配置目录不变。
不要把这些当作遗漏去改成 `SnackRead`/`com.cherno.snackread`，除非用户明确要求迁移。

## 版本号

与 cshow 相同：每完成一轮修改、准备打包部署前，把 `src-tauri/Cargo.toml` 的
`version` 递增一个小版本（补丁号）：`0.1.0 → 0.1.1 → … → 0.1.99 → 0.2.0`
（补丁到 99 后进位到次版本并把补丁清零）。
`--version` 只显示这个语义版本，用户靠它判断每次部署的先后。

`--version` 输出形态为 `snack-read <semantic_version>`。打包部署前
请同步把 `src-tauri/tauri.conf.json` 的 `version` 字段改到与 `Cargo.toml` 一致；
`package.sh` 实际取版本自 `Cargo.toml`。

## 打包部署

用户说“打包”时，执行：

1. `scripts/package.sh`（release 编译 → 组装 .app → ad-hoc 签名 → zip →
   自动复制到 `/Applications/SnackRead.app`）
2. 验证：`/Applications/SnackRead.app/Contents/MacOS/snack-read --version`
   输出与 `Cargo.toml` 版本一致。
