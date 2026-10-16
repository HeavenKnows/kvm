use crate::model::Language;

#[derive(Debug, Clone)]
pub enum UiStatus {
    Idle,
    MotionOverflow,
    QueueOverflow,
    BindFailed(String),
    Waiting,
    AcceptFailed(String),
    Authenticating(String),
    PairingFailed(String),
    ConnectedPeer(String),
    Disconnected,
    Connecting,
    InvalidAddress,
    ConnectFailed(String),
    ConnectRefused(String),
    NativeInputStarting,
    AwaitingInfo,
    Ready,
    Returned,
    ClipboardUnavailable,
    ClipboardInvalid,
    ClipboardRemoteEmpty,
    HandshakeTimedOut,
    InvalidHandshake,
    InputHookFailed(String),
    UserDisconnected,
    Stopped,
    PeerDisconnected(String),
    TransportClosed(String),
}
impl UiStatus {
    pub fn text(&self, language: Language) -> String {
        use UiStatus::*;
        let zh = language == Language::Zh;
        match self {
            Idle => (if zh {"未连接"} else {"Disconnected"}).into(),
            MotionOverflow => (if zh {"鼠标发送队列拥塞，已停止远程控制"} else {"Mouse queue full; remote control stopped"}).into(),
            QueueOverflow => (if zh {"发送队列拥塞；已停止远程控制"} else {"Network queue full; remote control stopped"}).into(),
            BindFailed(e) => format!("{}: {e}", if zh {"端口监听失败"} else {"Failed to listen on port"}),
            Waiting => (if zh {"等待另一台电脑连接…"} else {"Waiting for a connection…"}).into(),
            AcceptFailed(e) => format!("{}: {e}", if zh {"接收连接失败"} else {"Accept failed"}),
            Authenticating(peer) => format!("{} {peer}…", if zh {"正在验证"} else {"Authenticating"}),
            PairingFailed(e) => format!("{}: {e}", if zh {"配对失败"} else {"Pairing failed"}),
            ConnectedPeer(peer) => format!("{} {peer}", if zh {"已连接"} else {"Connected to"}),
            Disconnected => (if zh {"连接已断开；请重新启动应用以连接"} else {"Disconnected; restart the app to reconnect"}).into(),
            Connecting => (if zh {"正在连接…"} else {"Connecting…"}).into(),
            InvalidAddress => (if zh {"地址格式应为 IP:端口"} else {"Address must be IP:port"}).into(),
            ConnectFailed(e) => format!("{}: {e}", if zh {"网络连接失败"} else {"Connection failed"}),
            ConnectRefused(addr) => if zh {
                format!("连接被拒绝（{addr}）：请在 Mac 接收端点击「启动接收端」，并核对 IP、端口和防火墙")
            } else {
                format!("Connection refused ({addr}): start the Mac receiver and check IP, port and firewall")
            },
            NativeInputStarting => (if zh {"配对成功，正在初始化本机鼠标捕获…"} else {"Paired; initializing native mouse capture…"}).into(),
            AwaitingInfo => (if zh {"等待接收端信息…"} else {"Waiting for receiver information…"}).into(),
            Ready => (if zh {"已连接；将鼠标推到设置的屏幕边缘（F12 返回）"} else {"Connected; move to the configured screen edge (F12 to return)"}).into(),
            Returned => (if zh {"已返回本机"} else {"Returned to local computer"}).into(),
            ClipboardUnavailable => (if zh {"本机剪贴板不是文本或超过 32 KiB，取消远程粘贴"} else {"Local clipboard unavailable or exceeds 32 KiB; paste cancelled"}).into(),
            ClipboardInvalid => (if zh {"收到的剪贴板文本无效或超限"} else {"Received clipboard text is invalid or too large"}).into(),
            HandshakeTimedOut => (if zh {"等待接收端响应超时（5 秒），可重试"} else {"Receiver response timed out after 5 seconds; retry available"}).into(),
            InvalidHandshake => (if zh {"接收端响应无效，请检查版本与配对密钥"} else {"Invalid receiver response; check version and pairing key"}).into(),
            UserDisconnected => (if zh {"已主动断开"} else {"Disconnected by user"}).into(),
            Stopped => (if zh {"已停止接收，可重新启动"} else {"Receiver stopped; you can start it again"}).into(),
            PeerDisconnected(detail) => format!("{}: {detail}",if zh {"上一个连接已结束"}else{"Previous peer disconnected"}),
            TransportClosed(detail) => format!("{}: {detail}",if zh {"加密连接中断"}else{"Encrypted session closed"}),
            InputHookFailed(e) => format!("{}: {e}", if zh {"输入捕获启动失败"} else {"Input capture failed"}),
            ClipboardRemoteEmpty => (if zh {"远端剪贴板没有可用纯文本"} else {"No plain text available in remote clipboard"}).into(),
        }
    }
}
