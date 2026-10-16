# kvm — Windows ↔ macOS 键鼠共享（Rust 原型）

> **版本：0.4.6，实验源码版。** 本项目尚未完成 Windows/macOS 实机联调；GitHub Actions 会检查三个目标平台的编译与单元测试；通过编译仍不等于完成实体机验证或达到低延迟目标。暂不推荐在正式工作环境中依赖它。

## 能做什么、还不能做什么

| 需求 | 当前情况 |
| --- | --- |
| Windows ↔ macOS 一套键鼠跨屏 | 已编写原型，真实系统输入拦截仍待测试 |
| 低延迟 | TCP_NODELAY、鼠标位移合并与独立线程；**尚无实际延迟测量** |
| 双向剪贴板 | **仅纯文本**；按粘贴请求传输，不持续轮询 |
| 跨平台快捷键 | 控制端 `Ctrl`（Windows）与 `⌘ Command`（macOS）之间的基本修饰键映射，可关闭 |
| 自定义复杂快捷键 | **未实现**，例如 `Alt+Tab ↔ ⌘Tab` 还不能保证 |
| 自定义屏幕排列 | 图形界面拖动，支持上下左右和错位；目前只处理主显示器 |
| 界面语言 | **中文默认 / English 切换**，设置会保存；从本机加载中文字体 |
| 接收端网络信息 | 展示本机 IPv4 地址与端口，可复制 `IP:端口`，支持刷新网卡列表 |
| 监听端口 | 默认 **TCP 24881**，可手动修改或重新生成并持久保存；正在监听时需要重启程序才能变更 |
| Windows 黑色控制台 | Windows GUI 构建默认不再弹出控制台窗口 |
| 控制端断线重连 | 连接失败后允许直接重试；TCP 连接超时 4 秒、配对读写超时 4 秒、接收端 Hello 等待 5 秒；已连接后断线可重新连接 |
| 接收端断线处理 | 接收端保持 TCP 监听，等待控制端再次连接；断线时释放已按下的远程按键 |
| macOS 菜单栏常驻 | CI 打包 `.app` + `.dmg`；运行时隐藏 Dock 图标，菜单栏可打开设置和退出 |
| 自动发现 / Windows 安装器 | **未实现**，控制端首次手动填写 IP:端口与配对密钥 |

这是**单向控制架构**：一台电脑作为控制端（键鼠接在这台），另一台作为接收端。当前临时关闭了远端碰边自动返回；使用控制端 F12 或任一端的断开按钮返回本机，但**不是**两台电脑各自的键鼠都能随意接管对方。

## 快速开始

1. 在 Windows/macOS 安装 [Rust stable](https://rustup.rs/) 及对应平台编译工具。Windows 推荐 MSVC 工具链和 Visual Studio C++ Build Tools；macOS 需要 Xcode Command Line Tools。
2. 克隆仓库，进入**包含 `Cargo.toml` 的根目录**：

   ```sh
   cargo run --release
   ```

3. **接收端**选择 `接收端`，窗口会列出一个或多个局域网 IPv4 地址。默认端口为 **TCP 24881**，可以手动修改或点击 `重新生成端口`；点击 `复制地址` 可复制完整 `IP:端口`。点击 `复制密钥` 获取配对密钥，然后点击 `启动接收端`。防火墙需要允许选定端口的 TCP 入站连接（仅在可信局域网开放）。
4. **控制端**选择 `控制端`，将刚才复制的 `IP:端口` 粘贴到 `接收端 IP:端口`，输入相同的配对密钥，点击 `连接并共享键鼠`。窗口右上方 `Language / 语言` 可以切换到 English。
5. 连接失败会给出错误或超时提示，按钮会重新可用，直接检查 IP / 端口 / 密钥后重试。接收端断线后会继续等待新的连接，无需重启接收端。控制端可主动取消连接或断开；接收端可断开当前控制端或停止接收。在控制端界面拖动蓝色的另一台屏幕位置；把鼠标推过相邻边缘进入远端。远端自动碰边返回目前暂时停用，请在控制端按 **F12** 返回本机，或在任一端点击断开（部分 Mac 键盘需要 `Fn+F12`）。

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
| `windows-2025` | Windows x64 | 下载 Artifact ZIP，直接得到 `kvm.exe` |
| `macos-15` | Apple Silicon Mac | Artifact ZIP 内是 `kvm-macos-arm64.dmg`（包含 `kvm.app`） |
| `macos-15-intel` | Intel Mac | Artifact ZIP 内是 `kvm-macos-intel.dmg`（包含 `kvm.app`） |

打开 GitHub 仓库的 **Actions → Build kvm (Windows + macOS) → 最近一次运行 → Artifacts** 下载。

- **Windows**：解压 GitHub 下载的 ZIP，直接双击 `kvm.exe`；已关闭 Windows 控制台窗口。
- **macOS**：解压 GitHub Artifact ZIP，双击里面的 `.dmg`，将 `kvm.app` 拖到「应用程序」，然后从 Finder 启动。不需要 Terminal，**启动时会显示设置窗口**（避免菜单栏图标不可见时完全无法操作）。平时不占用 Dock，在屏幕顶部菜单栏保留 kvm 图标；关闭设置窗口后继续在后台运行，点击图标 →「打开设置」可以再次打开窗口。「退出 kvm」才会结束进程。如果菜单栏图标创建失败，窗口会显示警告，关闭窗口将正常退出。**当前不包含开机自动启动**。
- macOS CI 使用 **临时（ad-hoc）代码签名**而不是 Apple Developer ID 签名，也没有 Apple 公证。Gatekeeper 仍可能阻止打开，首次请在「系统设置 → 隐私与安全性」审核该来自可信仓库的构建；全局键鼠控制还需要辅助功能 / 输入监控权限。
- GitHub 的 Artifact 自身是 ZIP，因此现在下载只有**一个 ZIP 外层**；Windows ZIP 内直接是 exe，Mac ZIP 内直接是 dmg，不再套第二层 ZIP/tar.gz。

**Node.js 24 说明：** `actions/checkout@v5` 与 `actions/upload-artifact@v6` 已取代原来使用 Node.js 20 的版本；Rust 工具链步骤仍为 `dtolnay/rust-toolchain@stable`。这里的 Node.js 只是 GitHub Actions 使用的运行时，**本项目源码不需要安装 Node.js**。如果之后出现 `error[E...]` 或 `cargo build` 报错，那是另一类 Rust/系统依赖问题，并不意味着 Node 24 升级失效。

更详细的步骤见 [GITHUB_BUILD.md](GITHUB_BUILD.md)。

## 端口、IP 与语言设置

- 默认端口 **24881** 不会随重启自动变化。自定义端口为 **1024–65535**，修改或重新生成后存入 `kvm/settings.json`。重新生成端口只影响网络端口，**不会改变配对密钥**。
- 如已点击“启动接收端”，端口输入会被锁定；需要重新启动应用后才能生效。重新生成端口会检查当时是否可以临时绑定，但不能保证之后不被别的程序占用。
- IP 来自本机网络接口。若显示多条，选择与控制端在同一局域网内、可互相访问的 IPv4；网络切换后点击“刷新 IP”。当前不包含公网地址自动探测、NAT 穿透或设备自动发现。
- Windows 版运行 `kvm.exe` 不会保留黑色控制台窗口；这也意味着错误信息主要通过 GUI 状态栏查看。macOS 构建目前仍是未签名的命令行可执行文件，并非 `.app`。
- 中文 UI 会尝试从 Windows/macOS 的已安装字体路径加载中文字形，不在仓库中附带字体文件。若系统没有对应字体，中文显示可能缺字。

## 控制端重试与超时

按下连接按钮后，TCP 建连最长约 **4 秒**，Noise 握手读写各有 **4 秒超时**，收到加密连接后等待接收端初始 `Hello` 消息至多 **5 秒**。错误发生后状态栏会显示原因，连接按钮会恢复为可点击；无需退出程序。已连接后若网络断开，本地输入钩子保留并暂停拦截，下一次连接复用该钩子，避免重复注册系统级监听。接收端会继续监听新的连接。

## 主动断开与鼠标边缘修复（v0.4.6）

- **控制端**连接后有「断开连接 / 取消连接」按钮，点击会关闭实际 TCP 连接、停止远端输入转发并恢复本机鼠标；可随后再次连接。
- **接收端**提供「断开当前控制端」（踢掉现有连接，但继续监听下一次连接）和「停止接收」（释放端口并允许重新启动）。接收端待机时无法踢掉尚不存在的控制端。
- **跨屏鼠标**：0.4.6 改为平台原生相对位移捕获。Windows 使用 Raw Input `WM_INPUT`，即使光标被拦截在屏幕边缘也能持续读取设备移动；macOS 使用 `CGEventTap` 的 `MOUSE_EVENT_DELTA_X/Y`。不再以绝对坐标差或反复回中计算移动量。
- **远端碰边自动返回已临时关闭**：旧版 Mac 端的全局监听无法区分程序注入事件和真实鼠标事件，可能触发误返回。请在控制端使用 **F12** 返回本机，或在任意一端主动断开连接。等待后续替换为平台原生相对位移后，再恢复安全的边缘返回。
- **当前局限**：键盘与鼠标按钮仍由 `rdev::grab` 拦截，输入层尚未全部原生化；Raw Input 使用未加速的设备位移，和系统鼠标速度设置可能存在手感差异。多屏、Retina 缩放和系统授权依然需要实机验证。

## DMG 拖拽安装

macOS 的 `.dmg` 中现在同时包含 **`kvm.app`** 和指向系统 **`/Applications`** 的 **`Applications` 快捷方式**。打开 DMG 后，直接将 `kvm.app` 拖动到 `Applications` 即可，无需打开终端。这是常见的拖拽安装结构；当前没有自定义 Finder 背景和动画箭头。

## 配置和安全

- 程序设置保存在操作系统用户配置目录的 **`kvm/settings.json`**。若该文件不存在，程序会尝试读取之前 **`crosskey/settings.json`** 的设置，下一次保存时写入新路径。
- 配对码是应用生成的 32 位十六进制字符串（128 位随机值），传输层使用 `snow` 的 Noise PSK 加密。它**未接受独立安全审计**，设置文件中的配对码可能被本机其他程序读取。
- 仅在可信局域网使用，不要将接收端监听端口（默认 TCP 24881）映射到公网，不要将 `settings.json`、密钥、凭证提交到 GitHub。
- macOS 提供未公证、仅 ad-hoc 签名的 `.app` / `.dmg`，并非受 Apple Developer ID 信任的正式分发；Windows 未提供安装器。Windows UAC 安全桌面、macOS 锁屏/登录界面不支持控制。

## 开发和已知风险

```sh
cargo check
cargo test
cargo build --release
```

代码结构：`src/native_motion.rs` 为原生 Windows Raw Input / Mac CGEventTap 位移捕获；`src/main.rs` 为双语 egui 图形界面；`src/model.rs` 为配置和布局；`src/network.rs` 为本机 IPv4 发现；`src/status.rs` 为运行状态的本地化；`src/input.rs` 为快捷键映射；`src/wire.rs` 为网络加密协议；`src/engine.rs` 为键鼠钩子、输入注入和按需剪贴板。

低延迟尚未达标验证：0.4.6 的鼠标运动已使用 Windows Raw Input / macOS CGEventTap 原生相对位移。键盘和鼠标按钮仍使用 rdev::grab；需要在实际 Windows ↔ Mac 设备上确认系统权限、坐标缩放、滑动速度、连续拖动及断线恢复表现。

**验证状态：** v0.4.6 调整了断线重连、握手超时、GitHub Artifact 打包及 macOS 菜单栏运行方式。应以本次 PR 的 GitHub Actions 结果确认编译与自动测试；macOS 菜单栏重新显示设置窗口、系统输入权限及实际跨屏延迟仍需真实设备验收。

许可证：MIT。

## 0.4.6 原生鼠标位移实现

- **Windows 控制端**：独立隐藏消息窗口注册 `RIDEV_INPUTSINK`，读取 `WM_INPUT` / `RAWMOUSE.lLastX/lLastY`，只有进入远端控制模式时才累加位移。不会依赖光标能否越过 Windows 物理屏幕右边缘。
- **macOS 控制端**：安装独立的 `CGEventTap` 被动监听器，读取 `MOUSE_EVENT_DELTA_X/Y`，保留 `rdev::grab` 来决定跨屏和本机输入抑制。
- **接收端**：只处理网络收到的 `MoveBy` 增量，不根据注入的鼠标移动自动触发返回；通过控制端的 F12 或双方主动断开来退出远端控制。
- **初始化失败保护**：系统不允许创建原生监听器时，界面显示错误，连接过程清理旧状态，不会留下一个“已连接但鼠标不动”的会话。
- **特别说明**：本次原生输入层为新代码实现，并非复制 Lan Mouse/Deskflow 代码；两者启发了设计，但并未移植 GPL 源码。GitHub Actions 编译成功不代表实机操控/权限/低延迟验证通过。

## 0.4.6 连接排错与接收端自动监听

- Mac/Windows 配置为接收端时，应用重启后默认自动监听保存的 IP 对应端口（默认 TCP 24881），Mac 菜单栏运行模式不再要求每次重新打开设置按「启动接收端」。接收端设置中可以关闭下次启动时自动监听。
- Windows 控制端如果提示「连接被拒绝」，请确认 Mac 程序正在接收、两台电脑在同一局域网、控制端地址格式是接收端的 `IP:端口`，以及防火墙允许该 TCP 端口。
- “复制诊断信息”不会复制配对密钥，可用于确认失败发生在网络连接阶段还是 Windows Raw Input 初始化阶段。
- Windows GitHub Actions 添加原生 Raw Input 启动测试。编译通过不等于已在用户机器上验证，具体启动失败原因仍需依据界面诊断信息。

## 0.4.6 断线诊断

- 控制端不再将其他线程发生的输入钩子启动错误覆盖成笼统的「连接已断开」。
- 两端界面新增「最近连接事件」，显示最近 8 条记录并保留更早的历史供「复制诊断信息」复制；接收端在恢复等待连接后也不会丢失上一次的断线原因。
- 加密传输层现在会报告 TCP 读写、加密、解密或协议解码失败的具体阶段；错误文本不包含配对密钥。
- 三平台单元测试新增真实 TCP + Noise 加密回环：发送 Hello、MoveBy 和 Back，保证基本握手、帧读写和消息编码正常。
- **仍需实机**：如果新版本再次断线，请分别在 Windows 和 Mac 点击「复制诊断信息」，发出 Recent events（无需配对密钥），可以精确定位是哪一侧关闭连接。

## v0.4.6 修复 macOS 连接瞬断（EAGAIN / os error 35）

- **问题原因**：接收端的 `TcpListener` 设为非阻塞，便于停止监听。部分平台的 `accept()` 连接可能保留非阻塞标志，而加密传输层使用 `read_exact` 读取 TCP 帧；无数据可读时返回 `EAGAIN / WouldBlock`，旧代码将其误判为断线。Windows 随之收到 `failed to fill whole buffer`。
- **修复**：Mac 接收端在 `accept()` 后显式恢复该连接为阻塞模式；协议层 `wire::connect()` 也强制使用阻塞模式，以防其他调用方传入非阻塞 socket。监听器本身仍为非阻塞，支持停止接收。
- **回归测试**：加密 TCP 回环测试现在使用非阻塞监听器，并故意将接受的流设为非阻塞，验证协议可以恢复阻塞读写，经过短暂空闲后仍能发送 `Hello / MoveBy / Back`。
- **验证**：仍须在 Windows 控制端与 macOS 接收端实机检查跨屏是否稳定，不能把 GitHub Actions 构建通过等同于实机验证。

## v0.4.6 修复可见光标和四方向布局

- Mac 接收端通过 `CGWarpMouseCursorPosition` 移动系统实际可见光标；坐标使用 `CGDisplayBounds` 的逻辑点（points），而非 Retina 物理像素。
- 跨屏前等待接收端 `EnterAck` 确认，拒绝或超时时 Windows 不会立即进入远程拦截状态。控制端 F12 仍是紧急返回。
- 接收端只有先向内部移动再向入口边缘返回时才向控制端发送 `Back`，不再监听自己模拟产生的鼠标事件。进入后长时间未收到第一条鼠标位移则自动返回。
- 布局画布缩放以同时容纳上下左右的位置；拖拽累计位移只基于拖拽开始的布局，新增方向按钮和偏移滑块。
- 两台设备都需要更新为 v0.4.6。CI 只能验证构建和单元测试，Mac 可见光标与 Windows 输入钩子仍需要实机验证。

## v0.4.6 鼠标按钮注入与诊断

- Windows 捕获远端模式下的鼠标左/右/中键按下、松开事件，并记录各自的数量。
- Mac 接收端通过原生 Core Graphics `CGEventCreateMouseEvent` / `CGEventPost` 注入点击事件，使用当前可见光标坐标，并区分正确的左/右/中键与按下/松开事件。原有的 `rdev::simulate` 不再负责 Mac 端鼠标点击。
- 双端的「复制诊断信息」加入 `Mouse captured down/up`、`Mouse received down/up` 和 `Mouse injection failures`。如果点击仍无效，可以判断是 Windows 没捕获事件、网络没送达还是 Mac 注入失败。macOS 仍需要授予应用辅助功能权限。
- CI 暂时只构建 **Windows x64** 和 **macOS Apple Silicon**，按要求停止构建 Intel Mac。
- 当前版本进行了鼠标事件序列化单元测试；真正点击应用按钮需要实机验证，GitHub CI 不具备 macOS 交互式桌面测试条件。

macOS 注入鼠标点击前会检查 `AXIsProcessTrusted()`，若尚未授权会显示明确错误；接收端界面也会有醒目的辅助功能权限提醒。重装或替换未公证的 DMG 应用后，系统可能需要重新授权。请在 macOS「系统设置 → 隐私与安全性 → 辅助功能」中确认已为运行中的 `kvm.app` 授权。

- Windows Raw Input 现在同时读取 `RAWMOUSE.usButtonFlags` 的左/右/中键按下与松开事件，与真实鼠标位移使用同一个系统输入源。原来的 `rdev::grab` 仍会阻止本机接收远程控制期间的点击，但 Windows 不再通过这个钩子重复发送鼠标按键。这消除了两个捕获通道分离可能造成的点击丢失。

## v0.4.6 实时拖动与灵敏度

- macOS 左/右/中键按住时，连续使用 Core Graphics `LeftMouseDragged / RightMouseDragged / OtherMouseDragged` 而非普通 `MouseMoved`，应能使窗口在拖动中实时更新；结束会话时也使用原生鼠标释放事件清理按键。
- Windows Raw Input 设备计数不经系统指针加速，控制端提供 0.5–4.0 倍灵敏度，默认 1.6 倍，支持连接中调整并保留小数余量以减少低倍率卡顿。
- 每秒通过加密信道发送 `Ping/Pong`，控制端显示网络往返 RTT（毫秒）。RTT 仅代表网络及协议队列往返，不包括显示刷新和端到端鼠标事件延迟；网络 RTT 很低时仍卡顿，重点检查本机事件注入和 CPU/系统权限。
- 版本两端均升级才能兼容新消息格式；Intel Mac 构建仍暂停。CI 不具备交互式拖窗场景，实机手感仍需确认。

## 0.5.0-beta.1 多显示器布局与产品外观

- 两端会自动枚举物理显示器（含负坐标的副屏），通过加密握手同步真实位置和逻辑分辨率。Windows 使用 `EnumDisplayMonitors` / `GetMonitorInfoW`；macOS 使用 `CGGetActiveDisplayList` / `CGDisplayBounds`。
- Windows 控制端的布局画布使用灰色区分本机显示器、蓝色区分远端显示器；拖动蓝色显示器可对最近的灰色显示器上下左右自动吸附，记录跨屏生效的显示器对、方向及边缘偏移。手动选择方向与滑块作为补充。
- 鼠标仅在选择的本机显示器真实暴露的边缘切屏，不会把相邻两块本机显示器之间的边界误判为远端。远端鼠标坐标映射到实际物理显示器；虚拟桌面间的空白区域不作为有效光标区域。
- 图标资源在 CI 由 `scripts/generate_icon.py` 稳定生成 1024px 图像，Windows 嵌入 ICO 资源，macOS .app 内包含 ICNS 图标并在 Info.plist 注册。图标基于双显示器和跨屏光标轨迹概念，自定义绘制，不包含外部品牌标识。
- 仍为 beta：必须两端都安装新版本；更换显示器数量、顺序、缩放、排列后请完全退出并重新启动双方。没有完成所有真实多屏拓扑的跨设备硬件测试，也没有 Developer ID 正式签名或 Apple notarization。

## 0.5.0-beta.2 macOS 设置窗口可见性修复

- macOS `kvm.app` 现在**每次新进程启动都打开设置窗口**，不再因 `ViewportBuilder::with_visible(false)` 把唯一可操作界面藏起来。
- 菜单栏工作方式不变：在菜单栏正常创建时，关闭设置窗口会隐藏窗口并继续在后台接收；菜单栏「打开设置」会重新显示。
- 如菜单栏初始化失败，设置窗口仍可见并给出警告，不依赖初始化阶段发出的可见性命令；此时关闭窗口正常退出。
- 仍保留 macOS `LSUIElement=true`，因此不显示 Dock 图标。GitHub Actions 编译通过不等于已经在实体 Mac 验证了第一次启动、窗口隐藏/再次打开完整行为。

## 0.5.0-beta.3 恢复 macOS 可见窗口（临时回退图标）

- 暂停新的 Windows EXE / macOS .app 品牌图标嵌入；CI 不再执行 `scripts/generate_icon.py`、`iconutil` 或 Windows 资源嵌入。
- macOS 暂时恢复**标准前台应用**：`LSUIElement=false`，从 Finder 打开会显示设置窗口，Dock 和 `Command + Tab` 中都能找到 kvm。
- 暂时停用菜单栏常驻模式及菜单栏图标，优先保证窗口能找到；**关闭窗口即退出程序**，不再像之前一样隐藏到菜单栏后台。之后确认窗口行为可靠，才会重新评估菜单栏交互。
- 保留 0.5.0-beta.1 的多显示器、拖拽吸附布局及已有键鼠功能。
- 安装时先从「活动监视器」退出旧版 kvm，再把新 .app 拖到「应用程序」覆盖。新版本可能需要在系统设置中再次授权辅助功能/输入监控。CI 通过只能证明编译和包配置，仍需实际 Mac 启动验收。

## 0.5.0-beta.4 MacBook 外接屏幕启动卡死专项修复

- MacBook 内置屏 + 外接显示器时，显示器枚举（`CGGetActiveDisplayList` / `CGDisplayBounds`）不再阻塞 eframe 的 GUI 初始化；网络接口扫描、系统中文字体读取也改为后台线程。GUI 先使用临时单屏布局，完成枚举后更新真实多屏拓扑。
- macOS 恢复 **顶部状态栏图标** 和 **无 Dock** 运行方式（`LSUIElement=true`），首次启动设置窗口保持可见，点击状态栏「打开设置」重新显示；「退出 kvm」才真正退出。
- 状态栏图标仅在 eframe 已经开始渲染后创建，遵守 tray-icon 对 macOS 主线程事件循环的要求。打开设置时额外触发 macOS 应用激活。
- 启动日志位于 macOS 的临时目录，通常可在终端使用 `cat /tmp/kvm-launch.log` 查看；包含 `main-enter`、`app-creator-enter`、`first-ui-frame`、`display-query-start/done`、`tray-create-start/done` 等阶段，不包含配对密钥或键盘输入。
- **说明**：真实屏幕查询如果长期卡住，应用会在约 2 秒后仍允许窗口运行及接收服务使用临时单屏几何。此时需停止接收、重新连接或重启以更新真实多屏信息；不要在接收已开始的会话中更改屏幕拓扑。
- 构建成功并不等于已在真实外接显示器的 MacBook 上通过验收。安装前应彻底退出旧版 kvm 进程。

## 0.5.0-beta.5 Mac 启动第一帧卡住修复

收到的实机日志显示 `first-ui-frame` 后再无第二帧，而 `display-query-done` 已完成，因此不能把故障继续归咎于显示器查询本身，菜单栏图标也尚未开始创建。

- macOS 不再在后台线程执行 `egui::Context::set_fonts`；这个操作会和首帧 GUI 字体排版竞争。**本版暂时禁用 macOS 自定义中文系统字体覆盖**以验证启动稳定性，界面中文字形显示可能受影响，待启动故障确认后采用安全方式恢复；Windows 字体保持原样。
- macOS GUI 第一帧只绘制简化启动界面，主动请求下一帧；第二帧才创建顶部菜单栏图标，第三帧再应用异步获得的多显示器拓扑，第四帧才绘制完整设置界面。每个阶段在启动日志中留有标记。
- 启动日志写入 `$TMPDIR/kvm-launch.log`，新增 `minimal-first-frame-enter/return`、`tray-create-start/done`、`display-topology-apply-start/done`、`full-ui-start/returned`，可诊断是否是字体、多显示器或者完整 UI 卡住。
- 顶部菜单栏常驻、`LSUIElement=true`（不显示 Dock）仍保留。只打包 Windows x64 和 macOS Apple Silicon。**CI 不具备真实 MacBook 外接屏幕 GUI 的验收能力**。
