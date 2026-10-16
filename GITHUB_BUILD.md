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

## 上传或替换仓库文件

1. 打开你已有的 **`kvm`** GitHub 仓库。
2. 替换仓库根目录的 `Cargo.toml`、`README.md`、`GITHUB_BUILD.md`、`.github/workflows/build.yml`、`src/main.rs` 和 `src/model.rs`（如果从以前的源码升级）。
3. 确保仓库根目录有 `Cargo.toml`，并且名称为 `kvm`；不要把这些文件放进多出来的一层目录。
4. 提交后进入 GitHub 的 **Actions → Build kvm (Windows + macOS)**；`push` 会自动启动，也可以点击 **Run workflow** 手动触发。
5. 进入一次运行记录，成功后从 **Artifacts** 下载平台产物。

若你已有 Git 本地仓库，在其根目录完成文件替换后：

```bash
git add Cargo.toml README.md GITHUB_BUILD.md .github/workflows/build.yml src/main.rs src/model.rs
git commit -m "Fix Node 24 actions, rename kvm and correct docs"
git push
```

## 构建矩阵

| runner | target | Artifact 名称 | 内部文件 |
| --- | --- | --- | --- |
| `windows-2025` | `x86_64-pc-windows-msvc` | `kvm-windows-x64` | `kvm-windows-x64.zip` → `kvm.exe` |
| `macos-15` | `aarch64-apple-darwin` | `kvm-macos-arm64` | `kvm-macos-arm64.tar.gz` → `kvm` |
| `macos-15-intel` | `x86_64-apple-darwin` | `kvm-macos-intel` | `kvm-macos-intel.tar.gz` → `kvm` |

GitHub 下载 Artifact 时有时还会包一层 ZIP，解压后才是表中打包文件。

## 仍然失败怎么办？

先看工作流的**具体失败步骤**：

- `Check out source` 或 `Upload ...` 提示 Node 20：确认替换后的工作流已经提交到对应分支，并核对 `uses` 是否为 `checkout@v5` 和 `upload-artifact@v6`；如果是其他 action 报错，也需要单独升级。
- `Build optimized binary` 报 `error[E...]`：这是 Rust 编译/依赖问题，与 Node.js 24 **不是同一问题**。复制第一个完整的 `error` 和对应平台的日志进行定位。
- `Package ...` 找不到 `kvm.exe` / `kvm`：检查 `Cargo.toml` 的 `[package].name = "kvm"`，以及前一编译步骤是否成功。
- 使用了私有 runner：确保 Runner 版本至少为 2.327.1，操作系统也支持 Node.js 24。

**验证说明：** 此工作流已做静态语法及文件路径检查，但还没有在你的 GitHub 仓库真正跑过，因此不能保证当前 Rust 原型在全部平台上编译通过。

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
