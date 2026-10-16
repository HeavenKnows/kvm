#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod model;
mod wire;
mod input;
mod engine;
mod status;
mod network;
mod native_motion;
mod native_cursor;
mod display;
#[cfg(target_os="macos")]
mod macos_tray;

use eframe::egui;
use model::{Language, Layout, Settings, Side};
use std::{
    net::{Ipv4Addr, TcpListener},
    sync::{Arc, atomic::Ordering, mpsc::{self,Receiver,TryRecvError}},
    time::{Duration,Instant},
};

// Startup breadcrumbs are free of IP addresses, secrets, and keystrokes.
#[cfg(target_os="macos")]
fn launch_mark(stage:&str){
    use std::io::Write;
    if let Ok(mut f)=std::fs::OpenOptions::new().create(true).append(true)
        .open(std::env::temp_dir().join("kvm-launch.log")){
        let _=writeln!(f,"{} pid={} {}",std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
            std::process::id(),stage);
    }
}
#[cfg(not(target_os="macos"))]
fn launch_mark(_stage:&str){}

// set_fonts must not race with the GUI's first layout/paint pass.
// The macOS system-font override is temporarily disabled while diagnosing
// startup hangs. The default egui font stack remains available.
fn install_initial_fonts(ctx:&egui::Context){
    #[cfg(not(target_os="macos"))]
    install_chinese_font(ctx);
    #[cfg(target_os="macos")]
    {let _=ctx;launch_mark("macos-system-font-override-disabled");}
}

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
    drag_start_layout: Option<Layout>,
    drag_start_group:Option<(i32,i32)>,
    display_rx:Option<Receiver<display::Desktop>>,
    ip_rx:Option<Receiver<Vec<Ipv4Addr>>>,
    startup_at:Instant,
    display_loaded:bool,
    display_slow:bool,
    auto_start_waiting:bool,
    #[cfg(target_os="macos")]
    tray:Option<macos_tray::MacMenuBar>,
    #[cfg(target_os="macos")]
    tray_attempted:bool,
    frames:u32,
}
impl KvmApp {
    fn new(mut settings: Settings) -> Self {
        if settings.secret.len() != 32 { settings.secret = generate_secret(); }
        // Nonblocking initialization: do not call the OS display enumeration
        // or interface scanner from the eframe app-creation callback.
        launch_mark("shared-start");
        let state=engine::Shared::new(&settings);
        launch_mark("shared-ready");
        let (display_tx,display_rx)=mpsc::channel();
        std::thread::spawn(move ||{
            launch_mark("display-query-start");
            let d=display::current();
            launch_mark("display-query-done");
            let _=display_tx.send(d);
        });
        let (ip_tx,ip_rx)=mpsc::channel();
        std::thread::spawn(move ||{
            launch_mark("network-scan-start");
            let ips=network::local_ipv4_addresses();
            launch_mark("network-scan-done");
            let _=ip_tx.send(ips);
        });
        let auto_start_waiting=settings.role_server && settings.auto_start_receiver;
        Self{
            settings,state,ips:Vec::new(),
            drag_start_layout:None,drag_start_group:None,
            display_rx:Some(display_rx),ip_rx:Some(ip_rx),
            startup_at:Instant::now(),display_loaded:false,display_slow:false,
            auto_start_waiting,
            #[cfg(target_os="macos")]
            tray:None,
            #[cfg(target_os="macos")]
            tray_attempted:false,
            frames:0,
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

    /// Every physical display is drawn as a separate numbered monitor.
    /// Drag a blue panel near ANY grey panel: the nearest physical edge
    /// becomes the crossing anchor and the pair snaps exactly together.
    fn layout_ui(&mut self,ui:&mut egui::Ui,lang:Language){
        use model::{Display,group_origin,snap_to_edge};
        let local=self.state.local_displays();
        let remote=self.state.remote_displays.lock().unwrap().clone();
        if local.is_empty()||remote.is_empty(){return;}
        self.settings.local_display=self.settings.local_display.min(local.len()-1);
        self.settings.remote_display=self.settings.remote_display.min(remote.len()-1);
        ui.heading(tr(lang,"显示器布局","Display arrangement"));
        ui.label(tr(lang,
            "灰色为本机，蓝色为接收端。拖动任意蓝色显示器，靠近灰色显示器边缘时自动吸附。",
            "Grey: this computer. Blue: receiver. Drag any blue display near a grey display to snap to its edge."));
        ui.horizontal_wrapped(|ui|{
            ui.small(format!("{} {}  ·  {} {}",
                tr(lang,"本机","Local"),local.len(),
                tr(lang,"远端","Remote"),remote.len()));
            ui.small(tr(lang,
                "数字代表显示器顺序；跨屏点只在吸附的边缘生效",
                "Numbers identify monitors; switching occurs at the snapped edge"));
        });
        let (area,_)=ui.allocate_exact_size(
            egui::vec2(ui.available_width().min(730.0),365.0),egui::Sense::hover());
        let scale=(area.width()/(self.state.local_screen().w.max(self.state.remote_screen.lock().unwrap().w) as f32*3.1))
            .min(area.height()/(self.state.local_screen().h.max(self.state.remote_screen.lock().unwrap().h) as f32*3.2))
            .clamp(0.012,0.22);
        let local_origin=area.center()-egui::vec2(
            self.state.local_screen().w as f32*scale*0.5,
            self.state.local_screen().h as f32*scale*0.5);
        let painter=ui.painter();
        painter.rect_filled(area,12.0,egui::Color32::from_rgb(19,28,46));
        let to_rect=|d:Display,gx:i32,gy:i32|{
            egui::Rect::from_min_size(
                local_origin+egui::vec2((d.x+gx) as f32*scale,(d.y+gy) as f32*scale),
                egui::vec2((d.w as f32*scale).max(12.0),(d.h as f32*scale).max(9.0)))
        };
        let (gx,gy)=group_origin(self.settings.layout,&local,&remote,
            self.settings.local_display,self.settings.remote_display);
        for (i,d) in local.iter().enumerate(){
            let r=to_rect(*d,0,0);
            let selected=i==self.settings.local_display;
            painter.rect_filled(r,4.0,if selected {
                egui::Color32::from_rgb(108,128,158)
            }else{egui::Color32::from_rgb(76,91,113)});
            painter.rect_stroke(r,4.0,
                egui::Stroke::new(if selected{2.0}else{1.0},
                    egui::Color32::from_rgb(209,222,240)),egui::StrokeKind::Inside);
            painter.text(r.center(),egui::Align2::CENTER_CENTER,
                format!("{} {}",tr(lang,"本机","Local"),i+1),
                egui::FontId::proportional(12.0),egui::Color32::WHITE);
        }
        for (i,d) in remote.iter().enumerate(){
            let r=to_rect(*d,gx,gy);
            let selected=i==self.settings.remote_display;
            painter.rect_filled(r,4.0,if selected{
                egui::Color32::from_rgb(33,128,225)
            }else{egui::Color32::from_rgb(36,100,178)});
            painter.rect_stroke(r,4.0,
                egui::Stroke::new(if selected{2.0}else{1.0},
                    egui::Color32::from_rgb(121,217,252)),egui::StrokeKind::Inside);
            painter.text(r.center(),egui::Align2::CENTER_CENTER,
                format!("{} {}",tr(lang,"远端","Remote"),i+1),
                egui::FontId::proportional(12.0),egui::Color32::WHITE);
            let drag=ui.interact(r,ui.id().with(("remote-monitor",i)),egui::Sense::drag());
            if drag.drag_started(){
                self.drag_start_layout=Some(self.settings.layout);
                self.drag_start_group=Some((gx,gy));
            }
            if drag.dragged(){
                // Every gesture is relative to its ORIGINAL position, not to
                // the continuously snapped preview (avoids cumulative jumps).
                let (sx,sy)=self.drag_start_group.unwrap_or((gx,gy));
                let delta=drag.drag_delta()/scale;
                let dest_x=d.x+sx+delta.x.round() as i32;
                let dest_y=d.y+sy+delta.y.round() as i32;
                let (li,layout)=snap_to_edge(&local,&remote,i,dest_x,dest_y);
                self.settings.local_display=li;
                self.settings.remote_display=i;
                self.settings.layout=layout.clamp(local[li].screen(),d.screen());
                *self.state.layout.lock().unwrap()=self.settings.layout;
                self.state.set_layout_targets(li,i);
            }
            if drag.drag_stopped(){
                self.drag_start_layout=None;
                self.drag_start_group=None;
                self.persist_settings();
            }
        }
        ui.horizontal_wrapped(|ui|{
            ui.label(tr(lang,"吸附方向","Snap direction"));
            for (side,label_zh,label_en) in [
                (Side::Left,"左","Left"),(Side::Right,"右","Right"),
                (Side::Top,"上","Top"),(Side::Bottom,"下","Bottom")
            ]{
                if ui.selectable_label(self.settings.layout.side==side,
                        tr(lang,label_zh,label_en)).clicked(){
                    self.settings.layout.side=side;
                    self.settings.layout=self.settings.layout.clamp(
                        local[self.settings.local_display].screen(),
                        remote[self.settings.remote_display].screen());
                    *self.state.layout.lock().unwrap()=self.settings.layout;
                    self.state.set_layout_targets(self.settings.local_display,self.settings.remote_display);
                    self.persist_settings();
                }
            }
        });
        let ld=local[self.settings.local_display];
        let rd=remote[self.settings.remote_display];
        let bounds=if matches!(self.settings.layout.side,Side::Left|Side::Right){
            (-rd.h+24)..=(ld.h-24)
        }else{(-rd.w+24)..=(ld.w-24)};
        if ui.add(egui::Slider::new(&mut self.settings.layout.offset,bounds)
                .text(tr(lang,"边缘偏移","Edge offset"))).changed(){
            *self.state.layout.lock().unwrap()=self.settings.layout;
            self.persist_settings();
        }
        ui.small(format!("{} {} {} ↔ {} {} · {}",
            tr(lang,"本机","Local"),self.settings.local_display+1,
            tr(lang,"显示器","monitor"),
            tr(lang,"远端","Remote"),self.settings.remote_display+1,
            side_name(self.settings.layout.side,lang)));
        ui.small(tr(lang,
            "插拔显示器后请重启两端，重新连接会同步远端最新的显示器拓扑。",
            "After plugging/unplugging displays, restart both apps; reconnect to refresh remote topology."));
    }

}

impl eframe::App for KvmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames=self.frames.saturating_add(1);
        if self.frames==1{launch_mark("first-ui-frame");}
        if let Some(rx)=&self.display_rx{
            match rx.try_recv(){
                Ok(desktop)=>{
                    launch_mark("display-result-received");
                    let origin=(desktop.origin_x,desktop.origin_y);
                    launch_mark("display-topology-apply-start");
                    if self.state.update_local_desktop(desktop){
                        native_cursor::set_global_origin(origin.0,origin.1);
                        self.display_loaded=true;
                        self.display_slow=false;
                        launch_mark("display-topology-apply-done");
                    }else{
                        self.display_slow=true;
                        launch_mark("display-pending-session-restart");
                    }
                    self.display_rx=None;
                }
                Err(TryRecvError::Empty)=>{}
                Err(TryRecvError::Disconnected)=>{
                    self.display_rx=None;
                    self.display_slow=true;
                    launch_mark("display-worker-disconnected");
                }
            }
        }
        if let Some(rx)=&self.ip_rx{
            match rx.try_recv(){
                Ok(addresses)=>{self.ips=addresses;self.ip_rx=None;}
                Err(TryRecvError::Empty)=>{}
                Err(TryRecvError::Disconnected)=>{self.ip_rx=None;}
            }
        }
        // Never hold the app event loop hostage if an OS monitor API blocks.
        if !self.display_loaded && self.startup_at.elapsed()>Duration::from_secs(2){
            self.display_slow=true;
        }
        if self.auto_start_waiting &&
            (self.display_loaded || self.startup_at.elapsed()>Duration::from_secs(2)){
            self.auto_start_waiting=false;
            launch_mark("receiver-auto-start");
            engine::start_server(self.state.clone(),self.settings.clone());
        }
        #[cfg(target_os="macos")]
        {
            // Status icon must be created after the native event loop starts,
            // not in run_native's app creator. Give the window a first frame.
            if !self.tray_attempted && self.frames>=2 {
                self.tray_attempted=true;
                launch_mark("tray-create-start");
                match macos_tray::MacMenuBar::new(ctx,self.settings.language,self.state.clone()){
                    Ok(tray)=>{self.tray=Some(tray);launch_mark("tray-create-done");}
                    Err(e)=>launch_mark(&format!("tray-create-error: {e}")),
                }
            }
            if let Some(tray)=&mut self.tray{
                tray.set_language(self.settings.language);
                if tray.take_open_request(){
                    launch_mark("tray-open-settings");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    macos_tray::activate_app();
                }
                if ctx.input(|i|i.viewport().close_requested()) && !tray.quitting(){
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
            if self.frames==2{
                // Accessory apps have no Dock to reactivate an obscured window.
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                macos_tray::activate_app();
            }
        }
        // Render a tiny first frame so that accessory windows obtain a valid
        // native surface and the event loop can run before constructing menus
        // or the full multi-monitor editor.
        if self.frames==1 {
            launch_mark("minimal-first-frame-enter");
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.heading("kvm");
                ui.label("Starting settings…");
            });
            launch_mark("minimal-first-frame-return");
            ctx.request_repaint();
            return;
        }
        if self.frames<=3 {
            launch_mark("minimal-startup-frame-enter");
            egui::CentralPanel::default().show(ctx,|ui|{
                ui.heading("kvm");
                ui.label("Opening settings…");
            });
            launch_mark("minimal-startup-frame-return");
            ctx.request_repaint();
            return;
        }
        if self.frames==4{launch_mark("full-ui-start");}
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("kvm");
                ui.label(format!("v{}",env!("CARGO_PKG_VERSION")));
                ui.label(tr(self.settings.language,
                    "跨设备键鼠协作","Cross-device control"));
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
            if self.display_slow{
                ui.colored_label(egui::Color32::YELLOW,tr(lang,
                    "外接显示器检测较慢，当前使用安全的单屏回退布局；界面仍然可以操作。",
                    "External display scan is slow; temporary single-monitor layout is active while the UI remains available."));
            }
            #[cfg(target_os="macos")]
            if self.tray_attempted && self.tray.is_none(){
                ui.colored_label(egui::Color32::YELLOW,tr(lang,
                    "菜单栏入口初始化失败；此窗口保持可用，关闭窗口会退出。",
                    "Menu bar status icon failed; this window remains accessible and closing it quits."));
            }
            ui.label(tr(lang,
                "安全的局域网键盘鼠标共享 · Windows ↔ macOS",
                "Secure LAN keyboard and mouse sharing · Windows ↔ macOS"));
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
            #[cfg(target_os="macos")]
            if self.settings.role_server && !native_cursor::accessibility_trusted() {
                ui.colored_label(egui::Color32::YELLOW,tr(lang,
                    "未获得 macOS「辅助功能」权限：鼠标可移动，但点击和键盘注入可能被系统忽略。请在系统设置 → 隐私与安全性 → 辅助功能中授权 kvm。",
                    "macOS Accessibility permission missing: cursor motion may work but clicks and keys can be blocked. Grant kvm access in System Settings > Privacy & Security > Accessibility."));
            }
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
            if !self.settings.role_server {
                ui.horizontal(|ui|{
                    ui.label(tr(lang,"远程鼠标灵敏度","Remote mouse sensitivity"));
                    if ui.add(egui::Slider::new(&mut self.settings.mouse_speed,0.5..=4.0)
                        .step_by(0.1).suffix("×")).changed(){
                        self.state.set_mouse_speed(self.settings.mouse_speed);
                        self.persist_settings();
                    }
                });
                ui.small(tr(lang,
                    "原始位移不含 Windows 系统加速；连接时也可调整。",
                    "Raw Input bypasses Windows pointer acceleration; adjustable while connected."));
                let rtt=self.state.rtt_ms.load(Ordering::Relaxed);
                ui.small(if rtt==u64::MAX{
                    tr(lang,"网络往返延迟：测量中 / 未连接","Network RTT: measuring / disconnected").to_string()
                }else{
                    format!("{}: {rtt} ms",tr(lang,"网络往返延迟","Network RTT"))
                });
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
                        "kvm {}\nOS: {}\nRole: {}\nAddress: {}\nConfigured listening port: {}\nSession started: {}\nNetwork ready: {}\nMouse sensitivity: {:.1}x\nNetwork RTT: {:?} ms\nNative mouse packets: {}\nMouse captured down/up: {}/{}\nMouse received down/up: {}/{}\nMouse injection failures: {}\nStatus: {}",
                        env!("CARGO_PKG_VERSION"),
                        std::env::consts::OS,
                        if self.settings.role_server {"receiver"} else {"controller"},
                        if self.settings.role_server {"(receiver)"} else {self.settings.address.as_str()},
                        self.settings.listen_port,
                        self.state.started.load(Ordering::SeqCst),
                        self.state.ready.load(Ordering::SeqCst),
                        self.settings.mouse_speed,
                        {let v=self.state.rtt_ms.load(Ordering::Relaxed);if v==u64::MAX{None}else{Some(v)}},
                        self.state.native_motion_packets.load(Ordering::Relaxed),
                        self.state.mouse_down_captured.load(Ordering::Relaxed),
                        self.state.mouse_up_captured.load(Ordering::Relaxed),
                        self.state.mouse_down_received.load(Ordering::Relaxed),
                        self.state.mouse_up_received.load(Ordering::Relaxed),
                        self.state.mouse_injection_failures.load(Ordering::Relaxed),
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
        });
        if self.frames==4{launch_mark("full-ui-returned");}
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

fn main() -> eframe::Result<()> {
    launch_mark("main-enter");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([760.0, 730.0])
            // Mac is a Dock-less menu-bar app, but Settings starts visible.
            .with_visible(true),
        ..Default::default()
    };
    launch_mark("before-run-native");
    eframe::run_native("kvm",options,
        Box::new(|cc| {
            launch_mark("app-creator-enter");
            install_initial_fonts(&cc.egui_ctx);
            let app=KvmApp::new(model::load());
            launch_mark("app-creator-return");
            Ok(Box::new(app))
        }))
}
