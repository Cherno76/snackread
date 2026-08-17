# cshow-gui 项目约定

## 版本号

与 cshow 相同：每完成一轮修改、准备打包部署前，把 `src-tauri/Cargo.toml` 的
`version` 递增一个小版本（补丁号）：`0.1.0 → 0.1.1 → … → 0.1.99 → 0.2.0`
（补丁到 99 后进位到次版本并把补丁清零）。
`--version` 只显示这个语义版本，用户靠它判断每次部署的先后。

## 打包部署

用户说“打包”时，执行：

1. `scripts/package.sh`（release 编译 → 组装 .app → ad-hoc 签名 → zip →
   自动复制到 `/Applications/cshow-gui.app`）
2. 验证：`/Applications/cshow-gui.app/Contents/MacOS/cshow-gui --version`
   输出与 `Cargo.toml` 版本一致。
