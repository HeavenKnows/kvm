# GitHub Actions 自动编译 CrossKey

本项目根目录已经包含 `.github/workflows/build.yml`，GitHub Actions 会在每次 push、或者 Actions 页面手动运行时编译三个版本：

- `crosskey-windows-x64`：Windows Intel/AMD，内部是 `crosskey-windows-x64.zip`（含 `crosskey.exe`）。
- `crosskey-macos-arm64`：Apple Silicon Mac，内部是 `crosskey-macos-arm64.tar.gz`（含 `crosskey`）。
- `crosskey-macos-intel`：Intel Mac，内部是 `crosskey-macos-intel.tar.gz`（含 `crosskey`）。

## 上传方式

1. 在 https://github.com/new 创建空仓库（可选 **Private**）。不要初始化 README 等文件。
2. 解压本 ZIP，在 `CrossKey-Rust-v0.3` 内打开终端（目录中可直接看到 `Cargo.toml`）。
3. 运行以下命令，并替换自己的 GitHub 用户名、仓库名：

```bash
git init
git add .
git commit -m "Initial CrossKey Rust with CI"
git branch -M main
git remote add origin https://github.com/你的用户名/你的仓库名.git
git push -u origin main
```

4. 打开该仓库 **Actions → Build CrossKey (Windows + macOS)**。提交代码自动触发，也能点 **Run workflow** 手动触发。
5. 点击对应工作流的运行记录，在底部 **Artifacts** 下载各平台构建产物。

或者使用 GitHub 网页上传源文件，并在网页中创建 `.github/workflows/build.yml`（把项目中的同名文件内容复制过去）。请确保 `Cargo.toml` 在仓库根目录。

## macOS 本地编译

安装 Xcode Command Line Tools 和 rustup：

```bash
xcode-select --install
# 然后通过 https://rustup.rs/ 安装 rustup
cargo build --release
```

本地二进制在 `target/release/crosskey`。使用 Apple Silicon Mac 生成 ARM64 程序，Intel Mac 生成 x86_64 程序。

## 注意

- 这份 CrossKey v0.3 源码**仍是未经 Windows/macOS 编译验证的实验版**。GitHub Actions 的第一次运行可能报 Rust 依赖或平台 API 的编译错误；工作流只负责构建，不能替代修复源码。
- 下载的 macOS 文件是**未签名的原生可执行文件**，不是 `.app` 安装包。需要在本机处理 macOS 安全提示并授予辅助功能/输入监控权限；发布给其他用户还需要签名和公证。
- 不要上传配对码、运行时 `settings.json`、任何凭证或本机配置。存储在代码中的密钥也不能提交到公开仓库。
- 若 GitHub Actions 构建失败，请从失败任务的日志中复制第一处 `error` 开始的输出以定位源码问题。
