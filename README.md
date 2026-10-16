# kvm — Windows ↔ macOS 键鼠共享（Rust 原型）

> **版本：0.3.1，实验源码版。** 本项目尚未完成 Windows/macOS 实机联调；GitHub Actions 配置已更新，但本次交付并不代表三个系统均已编译成功或达到低延迟目标。暂不推荐在正式工作环境中依赖它。

## 能做什么、还不能做什么

| 需求 | 当前情况 |
| --- | --- |
| Windows ↔ macOS 一套键鼠跨屏 | 已编写原型，真实系统输入拦截仍待测试 |
| 低延迟 | TCP_NODELAY、鼠标位移合并与独立线程；**尚无实际延迟测量** |
| 双向剪贴板 | **仅纯文本**；按粘贴请求传输，不持续轮询 |
| 跨平台快捷键 | 控制端 `Ctrl`（Windows）与 `⌘ Command`（macOS）之间的基本修饰键映射，可关闭 |
| 自定义复杂快捷键 | **未实现**，例如 `Alt+Tab ↔ ⌘Tab` 还不能保证 |
| 自定义屏幕排列 | 图形界面拖动，支持上下左右和错位；目前只处理主显示器 |
| 自动发现 / 安装器 / 托盘 | **未实现**，首次手动填写 IP 与配对密钥 |

这是**单向控制架构**：一台电脑作为控制端（键鼠接在这台），另一台作为接收端。远端鼠标回边缘时可尝试返回本机，但**不是**两台电脑各自的键鼠都能随意接管对方。

## 快速开始

1. 在 Windows/macOS 安装 [Rust stable](https://rustup.rs/) 及对应平台编译工具。Windows 推荐 MSVC 工具链和 Visual Studio C++ Build Tools；macOS 需要 Xcode Command Line Tools。
2. 克隆仓库，进入**包含 `Cargo.toml` 的根目录**：

   ```sh
   cargo run --release
   ```

3. **接收端**选择 `This PC receives mouse/keyboard`，记录窗口显示的配对密钥，点击 `Start receiver`。允许本机防火墙上的 **TCP 24881**（只建议可信局域网）。
4. **控制端**选择 `This PC controls the other`，填接收端 IP 与端口（如 `192.168.1.20:24881`），填相同的配对密钥，点击 `Connect and share`。
5. 在控制端界面拖动蓝色的另一台屏幕位置；把鼠标推过相邻边缘进入远端。尝试通过远端相应的返回边缘回到本机，必要时在控制端按 **F12** 紧急返回（部分 Mac 键盘需要 `Fn+F12`）。

macOS 需要为运行程序（或启动它的终端）在 **系统设置 → 隐私与安全性 → 辅助功能** 中授权；根据系统提示还可能需要 **输入监控**。全局输入钩子、系统快捷键和高 DPI/Retina 行为均需要实机验证。

### Windows / macOS 的快捷键到底按什么？

**本机原生快捷键**：Windows 使用 `Ctrl+C` / `Ctrl+V`，macOS 使用 `⌘C` / `⌘V`（`⌘` 就是 Command 键）。

默认开启“Translate Ctrl (Windows) ↔ Command (Mac)”后，**控制端使用自己习惯的按键**，程序尝试将主修饰键映射为目标系统的按键。例如：

- **Windows 控制 Mac**：你在 Windows 键盘上按 `Ctrl+V`，Mac 目标应用收到的应是 `⌘V`。
- **Mac 控制 Windows**：你在 Mac 键盘上按 `⌘V`，Windows 目标应用收到的应是 `Ctrl+V`。

如果关闭映射则尽量发送原始键位；操作系统保留快捷键、输入法、复杂组合键及左右修饰键仍有平台差异，不能视为已经全部支持。

## 剪贴板：只在粘贴时传输

默认仅在检测到**控制端键盘**的粘贴快捷键时处理；平时不持续读取或上传剪贴板内容。当前最大为 **32 KiB 的 UTF-8 纯文本**。

用下面两种跨机方向举例：

1. **Windows 复制 → Mac 粘贴**：在 Windows 上 `Ctrl+C`，鼠标切到 Mac；仍在 Windows **物理键盘上按 `Ctrl+V`**。程序读取 Windows 的文本并发给 Mac；Mac 写入本机剪贴板后，模拟 **`⌘V`**。
2. **Mac 复制 → Windows 粘贴**（**Windows 是控制端**）：鼠标切到 Mac，在 Windows 物理键盘按 `Ctrl+C`，映射后 Mac 收到 **`⌘C`**；返回 Windows 再按 `Ctrl+V`。程序向 Mac 请求文本，写入 Windows 剪贴板后模拟 **`Ctrl+V`**。

同机复制同机粘贴时不需要联网同步。若**Mac 是控制端**，物理键盘应使用 `⌘C/⌘V`，传给 Windows 后才映射为 `Ctrl+C/Ctrl+V`。

**重要限制：** 当前复制来源仅通过控制端上的 `Ctrl/⌘+C` 或 `Ctrl/⌘+X` 键盘事件推断。右键复制、应用菜单、剪贴板管理器、接收端自己的键盘复制等操作**无法可靠识别**，可能导致粘贴旧文本或错误来源。图片、文件、富文本和密码管理器的特殊粘贴尚不支持。此外，跨机粘贴**会覆盖目标系统当前纯文本剪贴板**，还没有自动恢复。

将来更可靠的实现是监听各系统剪贴板的**变化通知/变更计数（只记录来源，不读取内容）**，仍旧等到粘贴时才传输内容。

## GitHub Actions 自动编译（Node.js 24）

项目包含 [`.github/workflows/build.yml`](.github/workflows/build.yml)，在 `push` 或手动触发时分别构建：

| Runner | 目标平台 | 打包文件 |
| --- | --- | --- |
| `windows-2025` | Windows x64 | `kvm-windows-x64.zip`，里面是 `kvm.exe` |
| `macos-15` | Apple Silicon Mac | `kvm-macos-arm64.tar.gz`，里面是 `kvm` |
| `macos-15-intel` | Intel Mac | `kvm-macos-intel.tar.gz`，里面是 `kvm` |

打开 GitHub 仓库的 **Actions → Build kvm (Windows + macOS) → 最近一次运行 → Artifacts** 下载（GitHub Artifacts 下载可能额外包一层 ZIP）。

**Node.js 24 说明：** `actions/checkout@v5` 与 `actions/upload-artifact@v6` 已取代原来使用 Node.js 20 的版本；Rust 工具链步骤仍为 `dtolnay/rust-toolchain@stable`。这里的 Node.js 只是 GitHub Actions 使用的运行时，**本项目源码不需要安装 Node.js**。如果之后出现 `error[E...]` 或 `cargo build` 报错，那是另一类 Rust/系统依赖问题，并不意味着 Node 24 升级失效。

更详细的步骤见 [GITHUB_BUILD.md](GITHUB_BUILD.md)。

## 配置和安全

- 程序设置保存在操作系统用户配置目录的 **`kvm/settings.json`**。若该文件不存在，程序会尝试读取之前 **`crosskey/settings.json`** 的设置，下一次保存时写入新路径。
- 配对码是应用生成的 32 位十六进制字符串（128 位随机值），传输层使用 `snow` 的 Noise PSK 加密。它**未接受独立安全审计**，设置文件中的配对码可能被本机其他程序读取。
- 仅在可信局域网使用，不要将 TCP 24881 映射到公网，不要将 `settings.json`、密钥、凭证提交到 GitHub。
- macOS 的程序尚未签名、公证，也没有 `.app` 安装包；Windows 未提供安装器。Windows UAC 安全桌面、macOS 锁屏/登录界面不支持控制。

## 开发和已知风险

```sh
cargo check
cargo test
cargo build --release
```

代码结构：`src/main.rs` 为 egui 图形界面；`src/model.rs` 为配置和布局；`src/input.rs` 为快捷键映射；`src/wire.rs` 为网络加密协议；`src/engine.rs` 为键鼠钩子、输入注入和按需剪贴板。

低延迟尚未达标验证：目前 `rdev::grab` 基于平台钩子，鼠标移动使用绝对位置差估算；屏幕边缘、系统加速度与多显示器可能出现无法连续获取位移等问题。后续建议 Windows 使用原生输入 API、macOS 使用 CGEventTap 相对位移，并做实机延迟测试。

**验证状态：** 这份仓库更新侧重修复 Node 24 的 CI 配置、命名及文档错误；并未在真实 Windows/Mac 上验证功能，也未保证 Rust 依赖和系统输入层可以一次编译成功。

许可证：MIT。
