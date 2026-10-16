# kvm：GitHub Actions 构建指南

## 已解决的 Node.js 20 问题

2026 年 9 月 23 日后，GitHub Actions 已移除 Node.js 20 运行时。原工作流使用 `actions/checkout@v4`、`actions/upload-artifact@v4`，需要升级。当前 `.github/workflows/build.yml` 已改成：

```yaml
- uses: actions/checkout@v5
- uses: dtolnay/rust-toolchain@stable
- uses: actions/upload-artifact@v6
```

其中 `checkout@v5` 和 `upload-artifact@v6` **使用 Node.js 24**；`dtolnay/rust-toolchain` 是组合式 shell action，安装 Rust，不需要你另外设置 Node.js。GitHub-hosted runner 支持该版本。若使用自己搭建的 self-hosted runner，需要 GitHub Actions Runner **v2.327.1 或更新**。

不要只加入 `actions/setup-node@v4` 或 `node-version: 24` 来处理这个报错；该设置**不会改变其他 action 在 `action.yml` 中声明的 Node.js 运行时**。

## 通过 GitHub 自动编译和下载

仓库的 `.github/workflows/build.yml` 会在 push 后自动进行三平台 `cargo test` 和 Release 构建。请从仓库的 **Actions → Build kvm (Windows + macOS)** 选择最新一个绿色成功的运行，滑到 **Artifacts** 下载：

| Runner | Target | GitHub Artifact | ZIP 内文件 |
| --- | --- | --- | --- |
| Windows 2025 | x86_64-pc-windows-msvc | kvm-windows-x64 | `kvm.exe` |
| macOS Apple Silicon | aarch64-apple-darwin | kvm-macos-arm64 | `kvm-macos-arm64.dmg` |

**只需解压一次 GitHub Artifact ZIP**：Windows 直接双击 `kvm.exe`；Mac 双击 `.dmg` 后把 `kvm.app` 拖到「应用程序」。`.app` 已恢复为普通前台应用（`LSUIElement=false`）：启动后直接显示设置窗口，在 Dock 和 `Command + Tab` 中可找回；**关闭窗口会退出程序**。因旧菜单栏入口可能不可见，0.5.0-beta.3 暂时移除了菜单栏图标及新版品牌图标嵌入，待实机确认窗口可靠后再考虑恢复。

macOS 的构建仅采用 ad-hoc 签名，**没有 Developer ID 签名和 Apple 公证**，首次启动可能需要到系统设置确认；键鼠输入还需要辅助功能/输入监控授权。

若使用本地 `cargo run`，它并非 Finder 启动的标准 `.app`，系统权限、Dock 行为与菜单栏体验可能不同，请用打包产物进行实机验收。

## 仍然失败怎么办？

先看工作流的**具体失败步骤**：

- `Check out source` 或 `Upload ...` 提示 Node 20：确认替换后的工作流已经提交到对应分支，并核对 `uses` 是否为 `checkout@v5` 和 `upload-artifact@v6`；如果是其他 action 报错，也需要单独升级。
- `Build optimized binary` 报 `error[E...]`：这是 Rust 编译/依赖问题，与 Node.js 24 **不是同一问题**。复制第一个完整的 `error` 和对应平台的日志进行定位。
- `Prepare Windows executable` / `Bundle macOS menu bar app and DMG` 找不到程序：检查 `Cargo.toml` 的 `[package].name = "kvm"`，以及前一编译步骤是否成功。
- 使用了私有 runner：确保 Runner 版本至少为 2.327.1，操作系统也支持 Node.js 24。

**验证说明：** 以当前 PR 的 GitHub Actions 运行记录为准；即使构建和测试通过，也还需要在真实 Windows/Mac 机器上测试菜单栏恢复设置窗口、输入权限及断线重连。

## 本地构建

在安装 Rust 的 Windows：

```powershell
cargo build --release
# target\release\kvm.exe
```

在安装 Xcode Command Line Tools 和 Rust 的 macOS：

```bash
cargo build --release
# target/release/kvm
```

**参考：** [GitHub 关于 Node.js 20 移除的公告](https://github.blog/changelog/2026-09-23-node-20-is-no-longer-available-in-github-actions/)；[checkout v5](https://github.com/actions/checkout)；[upload-artifact v6](https://github.com/actions/upload-artifact/releases/tag/v6.0.0)。

### 0.5.0-beta.4：macOS 菜单栏与外接显示器稳定性

Mac App 恢复 `LSUIElement=true`（没有 Dock 图标），但首次启动仍显示设置窗口。顶部状态栏的 kvm 图标支持「打开设置」「断开当前连接」「退出 kvm」。启动阶段的显示器查询/字体加载/网络枚举已移出 GUI 线程；如果 MacBook 接外屏后应用无响应，可查看 `/tmp/kvm-launch.log`，定位卡在何处。正式品牌 App 图标依然暂停嵌入，以减少权限和签名变量。

### 0.5.0-beta.6：Mac 完整界面死锁

已修复 `remote_screen` 的重复上锁导致 Mac 控制端首次绘制多显示器布局页面时无响应；新增 headless egui UI 回归测试和更细的启动日志。顶部状态栏模式保持不变。

### 0.5.0-beta.7：侧栏分区和 macOS CJK

新版打开后左侧依次显示「概览」「屏幕布局」「偏好设置」「运行诊断」四个导航入口；连接和配对位于概览，诊断日志不再占据主设置页。Mac OS 自带中文字库在后台读取后由 GUI 线程载入，正常应解决中文显示方块问题；如果字体资源缺失可通过启动日志中的 `font-byte-read-missing` 查看。无 Dock / 顶部菜单栏运行方式保持。
