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
    HeartbeatTimedOut,
    NetworkChanged,
    MouseEntryRejected,
    MouseEntryTimedOut,
    MouseNoMovement,
    RemoteCursorPlaced,
    RemoteMouseMoving,
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
            Disconnected => (if zh {"连接已断开；可重新连接"} else {"Disconnected; reconnect to try again"}).into(),
            Connecting => (if zh {"正在连接…"} else {"Connecting…"}).into(),
            InvalidAddress => (if zh {"地址格式应为 IP:端口"} else {"Address must be IP:port"}).into(),
            ConnectFailed(e) => format!("{}: {e}", if zh {"网络连接失败"} else {"Connection failed"}),
            ConnectRefused(addr) => if zh {
                format!("连接被拒绝（{addr}）：请在接收端启动接收服务，并核对 IP、端口和防火墙")
            } else {
                format!("Connection refused ({addr}): start the receiver and check IP, port and firewall")
            },
            NativeInputStarting => (if zh {"配对成功，正在初始化本机鼠标捕获…"} else {"Paired; initializing native mouse capture…"}).into(),
            AwaitingInfo => (if zh {"等待接收端信息…"} else {"Waiting for receiver information…"}).into(),
            Ready => (if zh {"已连接；将鼠标推到设置的屏幕边缘（F12 返回）"} else {"Connected; move to the configured screen edge (F12 to return)"}).into(),
            Returned => (if zh {"已返回本机"} else {"Returned to local computer"}).into(),
            ClipboardUnavailable => (if zh {"剪贴板没有可传输的文本、图片或文件，或超过 32 MiB（暂不支持文件夹）"} else {"Clipboard unavailable or exceeds 32 MiB (folders not supported)"}).into(),
            ClipboardInvalid => (if zh {"接收到的剪贴板内容无效、超限或无法写入"} else {"Clipboard data invalid, oversized or cannot be written"}).into(),
            HandshakeTimedOut => (if zh {"等待接收端响应超时（5 秒），可重试"} else {"Receiver response timed out after 5 seconds; retry available"}).into(),
            InvalidHandshake => (if zh {"接收端响应无效，请检查版本与配对密钥"} else {"Invalid receiver response; check version and pairing key"}).into(),
            UserDisconnected => (if zh {"已主动断开"} else {"Disconnected by user"}).into(),
            Stopped => (if zh {"已停止接收，可重新启动"} else {"Receiver stopped; you can start it again"}).into(),
            PeerDisconnected(detail) => format!("{}: {detail}",if zh {"上一个连接已结束"}else{"Previous peer disconnected"}),
            TransportClosed(detail) => format!("{}: {detail}",if zh {"加密连接中断"}else{"Encrypted session closed"}),
            HeartbeatTimedOut => (if zh {"对端心跳超时，连接已断开"}else{"Peer heartbeat timed out; session disconnected"}).into(),
            NetworkChanged => (if zh {"本机网络地址已切换，旧连接已断开"}else{"Local network address changed; old session disconnected"}).into(),
            MouseEntryRejected => (if zh {"远端光标定位失败，仍可在本机操作"}else{"Remote cursor placement failed; stayed local"}).into(),
            MouseEntryTimedOut => (if zh {"远端未确认鼠标进入，仍可在本机操作"}else{"Remote entry was not acknowledged; stayed local"}).into(),
            MouseNoMovement => (if zh {"远端未收到鼠标位移，已自动返回本机"}else{"No remote movement received; returned automatically"}).into(),
            RemoteCursorPlaced => (if zh {"远端光标定位成功，等待鼠标位移"}else{"Remote cursor positioned; awaiting mouse deltas"}).into(),
            RemoteMouseMoving => (if zh {"正在接收远程鼠标位移"}else{"Receiving remote mouse deltas"}).into(),
            InputHookFailed(e) => format!("{}: {e}", if zh {"输入捕获启动失败"} else {"Input capture failed"}),
            ClipboardRemoteEmpty => (if zh {"远端剪贴板没有可用内容"} else {"No supported content in remote clipboard"}).into(),
        }
    }
}
