# CrossKey v0.3 — 纯 Rust 重构（实验源码版）

> **状态：提供完整 Rust 项目源代码，但并未在本环境完成 Cargo 编译，也未在 Windows + macOS 实机验证。不是可直接双击的发布版本。**

## 与 v0.2 的区别

- **单语言：** 核心协议、GUI、剪贴板、键鼠控制代码均为 Rust。没有 Python、Qt/C++、Node/Tauri 前端或 Objective-C++ 项目文件。
- **低延迟思路：** 全局输入钩子只写入有界队列，网络线程处理序列化和加密；TCP_NODELAY；无视频截图和远程桌面。
- **剪贴板：** 完全删掉 250ms 轮询及自动广播。**只有在键盘粘贴时**读取源头文本、请求对端或同步后自动触发目标 OS 的粘贴。先支持最多 32 KiB UTF-8 纯文本。
- **快捷键：** 跨 Windows/macOS 自动映射 `Ctrl` ↔ `Command`，保留 Alt/Option 和其他按键。可一键关闭转换以传原始物理键位。任意用户自定义复杂组合键（Alt+Tab ↔ Cmd+Tab）**尚未实现**。
- **布局：** `egui/eframe` 中直接拖动蓝色屏幕，支持四边以及错位排布，自动保存。
- **加密：** 使用 `snow` 的 Noise `NNpsk0` 和系统随机生成 128-bit 配对码，避免自制 AEAD 协议。首次手动填写接收端 IP 和配对码，之后保存在本机用户配置目录。**只有持有配对码的电脑才能建立认证加密链路。**

## 运行（需在 Windows/macOS 自行编译）

安装 [Rust stable 工具链](https://rustup.rs/)，在两台机器运行：

```sh
cargo run --release
```

接收端：选 **This PC receives mouse/keyboard**，记下 Pairing key，按 **Start receiver**；确定 Windows 防火墙放行 **TCP 24881（仅专用网络）**，或 macOS 防火墙允许当前 app。

控制端：选 **This PC controls the other**，填接收端局域网 IP、端口和配对码，按 **Connect and share**；通过拖蓝色屏幕设置位置。跨入边缘后把输入发给对端。**F12** 为紧急返回，接收端反向跨边缘返回也有初步逻辑。

**macOS 必须打开** 系统设置 → 隐私与安全性 → 辅助功能、必要时输入监控，对运行程序或 Terminal 授权。部分功能键需要 Fn+F12。

## 为什么只在 Ctrl+V / Cmd+V 时检查剪贴板？

假设键鼠在 Windows：

1. **Windows Ctrl+C → Mac Ctrl+V**：控制端记录复制来源为 Windows；按粘贴时，后台线程读取 Windows 剪贴板，将文本发送 Mac，Mac 设置剪贴板后执行 `Cmd+V`。
2. **Mac Ctrl+C（被转成 Cmd+C）→ Windows Ctrl+V**：控制端记录复制来源为 Mac；回到 Windows 按粘贴时，向 Mac 请求当前文本，Windows 设置剪贴板后执行 `Ctrl+V`。
3. **同机复制、同机粘贴**：保持操作系统原生粘贴，不做网络传输。

这个实现**不会监听右键菜单中的 Copy / Cut 或第三方应用自行修改剪贴板**，因此复制来源判断可能失效，导致取错内容。正式版应只监听两台机器的**剪贴板变更计数/通知（不读取内容）**作为来源信号；仍然只在粘贴时读取/传输真正的数据。图片、富文本、文件暂不支持。

## 已知尚未解决的关键问题

- **鼠标的低延迟/连续移动：** 当前 `rdev::grab` 是跨平台验证用的拦截层，抓取的是绝对指针坐标并抑制源事件；**在拦截后它可能无法提供连续的真实鼠标位移**，尤其在屏幕边缘。这不是已解决的高性能输入后端。下一步必须分别用 Windows Raw Input / 低级钩子和 macOS CGEventTap 的相对位移替换 `src/engine.rs` 的鼠标捕获段。
- Windows 键盘布局、macOS 输入法、死键、高 DPI/Retina、多显示器和 OS 保留快捷键未联调。当前仅主显示器。
- Windows UAC 安全桌面及 macOS 登录/锁屏等受保护画面不可控制。
- 连接失败当前需要关闭程序重新连接；局域网自动发现、后台托盘、开机自启、程序签名/安装器均未做。
- 当前粘贴会**覆盖目标系统原有纯文本剪贴板**。若希望粘贴后还原剪贴板，需要按操作系统处理临时保存与撤销，且可能影响剪贴板管理器。
- 源码采用第三方库而非从头重写每个系统 API；“纯 Rust”是指**应用层全部 Rust**，这些库在内部仍使用系统 C ABI。这不等于没有底层 FFI。
- `snow` Noise-PSK 用于加密认证，但本项目尚未进行任何独立安全审计。配对码存储在系统用户配置目录的 `settings.json` 中，可能被其他本地进程读取。请仅在可信局域网测试，别向公网映射端口。

## 开发

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets
cargo run --release
```

代码目录：

- `src/model.rs`: 几何布局、设置与单元测试
- `src/input.rs`: Windows/Mac 快捷键映射和单元测试
- `src/wire.rs`: Noise 加密的 TCP 帧和类型化协议
- `src/engine.rs`: 连接、系统输入钩子、按需粘贴
- `src/main.rs`: egui 图形界面、屏幕布局拖放

**本执行环境没有 Rust/Cargo 且无法联网安装工具链，因此不能声称 cargo test、Windows/macOS 编译或实际低延迟测试已通过。**
