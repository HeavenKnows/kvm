#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod model;
mod wire;
mod input;
mod engine;
mod status;
mod network;
mod native_motion;
#[cfg(target_os="macos")]
mod macos_tray;

use eframe::egui;
use model::{Language, Layout, Settings, Side};
use std::{
    net::{Ipv4Addr, TcpListener},
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

fn tr(lang: Language, zh: &'static str, en: &'static str) -> &'static str {
    if lang == Language::Zh { zh } else { en }
}

fn side_name(side: Side, lang: Language) -> &'static str {
    match side {
        Side::Left => tr(lang, "左侧", "Left"),
        Side::Right => tr(lang, "右侧", "Right"),
        Side::Top => tr(lang, "上方", "Top"),
        Side::Bottom => tr(lang, "下方", "Bottom"),
    }
}

fn generate_secret() -> String {
    use rand::RngCore;
    let mut buf = [0_u8;16];
    rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

fn generate_port(old: u16) -> u16 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    for _ in 0..50 {
        let port = rng.gen_range(20000..=60000);
        if port != old {
            // Probe current availability only; another process could take it later.
            if let Ok(probe) = TcpListener::bind(("0.0.0.0", port)) {
                drop(probe);
                return port;
            }
        }
    }
    old
}

fn install_chinese_font(ctx: &egui::Context) {
    // Read fonts installed by the OS; no font binaries are shipped in the repository.
    #[cfg(target_os = "windows")]
    let candidates: &[&str] = &[
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ];
    #[cfg(target_os = "macos")]
    let candidates: &[&str] = &[
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
    ];
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let candidates: &[&str] = &[];
    for path in candidates {
        if let Ok(data) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "system-cjk".into(),
                egui::FontData::from_owned(data).into(),
            );
            fonts.families.entry(egui::FontFamily::Proportional)
                .or_default().insert(0, "system-cjk".into());
            fonts.families.entry(egui::FontFamily::Monospace)
                .or_default().push("system-cjk".into());
            ctx.set_fonts(fonts);
            break;
        }
    }
}

struct KvmApp {
    settings: Settings,
    state: Arc<engine::Shared>,
    ips: Vec<Ipv4Addr>,
    #[cfg(target_os="macos")]
    tray: Option<macos_tray::MacMenuBar>,
}
impl KvmApp {
    fn new(mut settings: Settings, ctx: &egui::Context) -> Self {
        if settings.secret.len() != 32 { settings.secret = generate_secret(); }
        let state = engine::Shared::new(&settings);
        // A menu-bar receiver should actually listen after launch, not wait
        // invisibly until its Settings window is opened.
        if settings.role_server && settings.auto_start_receiver {
            engine::start_server(state.clone(),settings.clone());
        }
        #[cfg(target_os="macos")]
        let tray = match macos_tray::MacMenuBar::new(ctx, settings.language, state.clone()) {
            Ok(tray) => Some(tray),
            Err(e) => {
                eprintln!("Failed to create status bar icon: {e}");
                // Never strand the user with an invisible window and no menu.
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.request_repaint();
                None
            }
        };
        Self { settings, state, ips: network::local_ipv4_addresses(),
            #[cfg(target_os="macos")]
            tray,
        }
    }
    fn persist_settings(&self) { model::save(&self.settings); }

    fn receiver_addresses(&mut self, ui: &mut egui::Ui, lang: Language) {
        ui.horizontal(|ui| {
            ui.strong(tr(lang, "本机地址（控制端填写以下任一地址）", "Local addresses (enter one on the controller)"));
            if ui.button(tr(lang, "刷新 IP", "Refresh IPs")).clicked() {
                self.ips = network::local_ipv4_addresses();
            }
        });
        if self.ips.is_empty() {
            ui.colored_label(egui::Color32::YELLOW,
                tr(lang, "未找到可用 IPv4 地址，请检查网络连接。", "No usable IPv4 address found. Check network connection."));
        } else {
            for ip in &self.ips {
                let address = format!("{ip}:{}", self.settings.listen_port);
                ui.horizontal(|ui| {
                    ui.monospace(&address);
                    if ui.button(tr(lang, "复制地址", "Copy address")).clicked() {
                        ui.ctx().copy_text(address.clone());
                    }
                });
            }
        }
        ui.small(tr(lang,
            "若存在多个 IP，优先使用与控制端处于同一局域网的地址；VPN 地址可能无法连接。",
            "If multiple addresses appear, use one on the same LAN as the controller; VPN addresses may not work."));
    }

    fn layout_ui(&mut self, ui: &mut egui::Ui, lang: Language) {
        ui.label(tr(lang,
            "拖动蓝色屏幕来调整另一台电脑的位置（支持上下左右及错位）",
            "Drag the blue screen to position the other computer (including offsets)."));
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().min(530.0), 265.0), egui::Sense::hover());
        let local = self.state.local_screen;
        let remote = *self.state.remote_screen.lock().unwrap();
        let scale = (135.0 / local.h.max(remote.h) as f32).min(0.14);
        let base = egui::Rect::from_center_size(rect.center(),
            egui::vec2(local.w as f32 * scale, local.h as f32 * scale));
        let (ox, oy) = self.settings.layout.origin(local, remote);
        let peer = egui::Rect::from_min_size(
            base.min + egui::vec2(ox as f32 * scale, oy as f32 * scale),
            egui::vec2(remote.w as f32 * scale, remote.h as f32 * scale));
        let painter = ui.painter();
        painter.rect_filled(base, 7.0, egui::Color32::from_rgb(95,104,119));
        painter.text(base.center(), egui::Align2::CENTER_CENTER,
            tr(lang,"本机","This PC"), egui::FontId::proportional(14.0), egui::Color32::WHITE);
        painter.rect_filled(peer, 7.0, egui::Color32::from_rgb(49,118,216));
        painter.text(peer.center(), egui::Align2::CENTER_CENTER,
            tr(lang,"另一台电脑","Other PC"), egui::FontId::proportional(14.0), egui::Color32::WHITE);
        let response = ui.interact(peer, ui.id().with("peer-drag"), egui::Sense::drag());
        if response.dragged() {
            let center = peer.center() + response.drag_delta();
            let diff = center - base.center();
            let side = if diff.x.abs() > diff.y.abs() {
                if diff.x >= 0.0 { Side::Right } else { Side::Left }
            } else if diff.y >= 0.0 { Side::Bottom } else { Side::Top };
            let offset = if matches!(side,Side::Left | Side::Right) {
                ((center.y - base.min.y - peer.height() / 2.0) / scale).round() as i32
            } else {
                ((center.x - base.min.x - peer.width() / 2.0) / scale).round() as i32
            };
            self.settings.layout = Layout {side, offset}.clamp(local,remote);
            *self.state.layout.lock().unwrap() = self.settings.layout;
        }
        if response.drag_stopped() { self.persist_settings(); }
        ui.horizontal(|ui| {
            ui.label(format!("{}: {}", tr(lang,"方向","Side"), side_name(self.settings.layout.side,lang)));
            ui.label(format!("{}: {} px", tr(lang,"偏移","Offset"), self.settings.layout.offset));
            if ui.button(tr(lang,"重置布局","Reset layout")).clicked() {
                self.settings.layout = Layout::default();
                *self.state.layout.lock().unwrap() = self.settings.layout;
                self.persist_settings();
            }
        });
    }
}

impl eframe::App for KvmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os="macos")]
        {
            if let Some(tray) = &mut self.tray {
                tray.set_language(self.settings.language);
                if ctx.input(|i| i.viewport().close_requested()) && !tray.quitting() {
                    // Closing Settings hides it; the menu bar keeps KVM running.
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(format!("kvm {}", env!("CARGO_PKG_VERSION")));
                ui.separator();
                ui.label("Language / 语言");
                let selected = if self.settings.language == Language::Zh { "中文" } else { "English" };
                let previous = self.settings.language;
                egui::ComboBox::from_id_salt("language")
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.settings.language, Language::Zh, "中文");
                        ui.selectable_value(&mut self.settings.language, Language::En, "English");
                    });
                if previous != self.settings.language { self.persist_settings(); }
            });
            let lang = self.settings.language;
            ui.label(tr(lang,
                "Windows ↔ macOS 键鼠共享 · 实验版本",
                "Windows ↔ macOS keyboard and mouse sharing · experimental"));
            ui.separator();
            let started = self.state.started.load(Ordering::SeqCst);

            ui.add_enabled_ui(!started, |ui| {
                let previous = self.settings.role_server;
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.settings.role_server, true,
                        tr(lang,"接收端（本机接受远程键鼠）","Receiver (this PC accepts remote input)"));
                    ui.radio_value(&mut self.settings.role_server, false,
                        tr(lang,"控制端（本机控制另一台）","Controller (this PC controls the other)"));
                });
                if previous != self.settings.role_server { self.persist_settings(); }
                if self.settings.role_server {
                    if ui.checkbox(&mut self.settings.auto_start_receiver,
                        tr(lang,"应用启动时自动开始接收（下次启动生效）",
                            "Start listening automatically at app launch (next launch)")).changed(){
                        self.persist_settings();
                    }
                    ui.horizontal(|ui| {
                        ui.label(tr(lang,"监听端口","Listening port"));
                        let port_change = ui.add(egui::DragValue::new(&mut self.settings.listen_port)
                            .range(1024..=65535).speed(1.0)).changed();
                        if port_change { self.persist_settings(); }
                        if ui.button(tr(lang,"重新生成端口","Generate new port")).clicked() {
                            self.settings.listen_port = generate_port(self.settings.listen_port);
                            self.persist_settings();
                        }
                    });
                    ui.label(tr(lang,"配对密钥（填入控制端）","Pairing key (enter on the controller)"));
                    ui.horizontal(|ui| {
                        ui.monospace(&self.settings.secret);
                        if ui.button(tr(lang,"复制密钥","Copy key")).clicked() {
                            ui.ctx().copy_text(self.settings.secret.clone());
                        }
                        if ui.button(tr(lang,"重新生成密钥","Generate new key")).clicked() {
                            self.settings.secret = generate_secret();
                            self.persist_settings();
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.label(tr(lang,"接收端 IP:端口","Receiver IP:port"));
                        if ui.text_edit_singleline(&mut self.settings.address).changed() {
                            self.persist_settings();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(tr(lang,"配对密钥","Pairing key"));
                        if ui.add(egui::TextEdit::singleline(&mut self.settings.secret).password(true)).changed() {
                            self.persist_settings();
                        }
                    });
                }
            });
            if self.settings.role_server {
                self.receiver_addresses(ui,lang);
                ui.small(format!("{}: TCP {}",tr(lang,"当前监听端口","Configured listening port"),self.settings.listen_port));
                ui.small(tr(lang,
                    "端口及密钥修改后会保存；连接启动后需重启程序才能更换端口。",
                    "Port and key are saved; restart the app to change the port after listening starts."));
            }
            if ui.checkbox(&mut self.settings.remap,
                tr(lang,"自动映射 Windows Ctrl ↔ macOS Command","Map Windows Ctrl ↔ macOS Command")).changed() {
                self.state.remap.store(self.settings.remap,Ordering::SeqCst);
                self.persist_settings();
            }
            if ui.checkbox(&mut self.settings.clipboard,
                tr(lang,"按需同步剪贴板（仅纯文本，粘贴时传输）",
                    "Clipboard on demand (plain text, transfer only on paste)")).changed() {
                self.state.clipboard.store(self.settings.clipboard,Ordering::SeqCst);
                self.persist_settings();
            }
            ui.separator();
            if !self.settings.role_server {
                self.layout_ui(ui,lang);
            } else {
                ui.label(tr(lang,
                    "请保持程序运行。macOS 需要授予「辅助功能」和「输入监控」权限。",
                    "Keep the app running. macOS requires Accessibility and Input Monitoring permissions."));
            }
            ui.separator();
            if ui.add_enabled(!started, egui::Button::new(
                if self.settings.role_server {tr(lang,"启动接收端","Start receiver")}
                else {tr(lang,"连接并共享键鼠","Connect and share")})).clicked() {
                self.persist_settings();
                if self.settings.role_server {
                    engine::start_server(self.state.clone(),self.settings.clone());
                } else {
                    engine::start_client(self.state.clone(),self.settings.clone());
                }
            }
            if started {
                let connected=self.state.ready.load(Ordering::SeqCst);
                ui.horizontal(|ui|{
                    if self.settings.role_server {
                        if ui.add_enabled(connected,egui::Button::new(
                            tr(lang,"断开当前控制端","Disconnect current controller"))).clicked(){
                            self.state.disconnect_peer();
                        }
                        if ui.button(tr(lang,"停止接收","Stop receiver")).clicked(){
                            self.state.stop();
                        }
                    } else if ui.button(tr(lang,"断开连接 / 取消连接","Disconnect / cancel connection")).clicked(){
                        self.state.stop();
                    }
                });
            }
            if started && !self.state.ready.load(Ordering::Relaxed) && !self.settings.role_server {
                ui.small(tr(lang,
                    "正在尝试连接：TCP 最长约 4 秒，配对和接收端响应也有超时。",
                    "Connecting: TCP up to 4 seconds; pairing and receiver response are also time-limited."));
            }
            let status = self.state.status.lock().unwrap().clone();
            let status_text=status.text(lang);
            ui.horizontal_wrapped(|ui|{
                ui.label(format!("{}: {}", tr(lang,"状态","Status"),status_text));
                if ui.button(tr(lang,"复制诊断信息","Copy diagnostics")).clicked(){
                    let diagnostic=format!(
                        "kvm {}\nOS: {}\nRole: {}\nAddress: {}\nConfigured listening port: {}\nSession started: {}\nNetwork ready: {}\nStatus: {}",
                        env!("CARGO_PKG_VERSION"),
                        std::env::consts::OS,
                        if self.settings.role_server {"receiver"} else {"controller"},
                        if self.settings.role_server {"(receiver)"} else {self.settings.address.as_str()},
                        self.settings.listen_port,
                        self.state.started.load(Ordering::SeqCst),
                        self.state.ready.load(Ordering::SeqCst),
                        status.text(Language::En)
                    );
                    let history=self.state.history.lock().unwrap().iter()
                        .cloned().collect::<Vec<_>>().join("\n");
                    ui.ctx().copy_text(format!("{diagnostic}\nRecent events:\n{history}"));
                }
            });
            egui::CollapsingHeader::new(tr(lang,"最近连接事件（保留断线原因）","Recent connection events"))
                .default_open(true)
                .show(ui,|ui|{
                    let history=self.state.history.lock().unwrap();
                    for line in history.iter().rev().take(8){
                        ui.small(line);
                    }
                });
            if self.state.active.load(Ordering::Relaxed) {
                ui.colored_label(egui::Color32::LIGHT_GREEN,
                    tr(lang,"正在控制远端 · F12 紧急返回","Remote control active · F12 emergency return"));
            }
            ui.small(tr(lang,
                "当前支持一台控制端和一台接收端；不支持自动发现；请仅在可信局域网使用。",
                "One controller + one receiver; no automatic discovery; trusted LAN only."));
        });
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([760.0, 730.0])
            // Agent mode: start with a status bar item; open settings from its menu.
            .with_visible(!cfg!(target_os="macos")),
        ..Default::default()
    };
    eframe::run_native("kvm",options,
        Box::new(|cc| {
            install_chinese_font(&cc.egui_ctx);
            Ok(Box::new(KvmApp::new(model::load(), &cc.egui_ctx)))
        }))
}
