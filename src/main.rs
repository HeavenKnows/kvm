#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod model;
mod wire;
mod input;
mod engine;
mod clipboard;
mod status;
mod network;
mod native_motion;
mod native_cursor;
#[cfg(target_os="macos")]
mod native_keyboard;
mod display;
#[cfg(target_os="macos")]
mod macos_tray;
#[cfg(target_os="windows")]
mod windows_tray;

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
        let entry=format!("{} pid={} {}\n",std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
            std::process::id(),stage);
        let _=f.write_all(entry.as_bytes());
    }
}
#[cfg(not(target_os="macos"))]
fn launch_mark(_stage:&str){}

// The macOS worker ONLY reads font bytes. set_fonts must run on the GUI
// thread after the first complete settings frame, never on the worker.
fn install_initial_fonts(ctx:&egui::Context){
    #[cfg(not(target_os="macos"))]
    install_chinese_font(ctx);
    #[cfg(target_os="macos")]
    {let _=ctx;launch_mark("macos-cjk-font-worker-enabled");}
}
fn font_definitions_with_cjk(data:Vec<u8>)->egui::FontDefinitions{
    let mut fonts=egui::FontDefinitions::default();
    fonts.font_data.insert("system-cjk".into(),
        egui::FontData::from_owned(data).into());
    for family in [egui::FontFamily::Proportional,egui::FontFamily::Monospace]{
        fonts.families.entry(family).or_default().insert(0,"system-cjk".into());
    }
    fonts
}
#[cfg(target_os="macos")]
fn load_system_cjk_font()->Option<(&'static str,Vec<u8>)>{
    // Try the OS-provided CJK faces; none are bundled or distributed.
    const PATHS:&[&str]=&[
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
    ];
    PATHS.iter().copied().find_map(|path|{
        let len=std::fs::metadata(path).ok()?.len();
        if len==0 || len>90*1024*1024{return None;}
        let bytes=std::fs::read(path).ok()?;
        if bytes.is_empty(){None}else{Some((path,bytes))}
    })
}

/// Update the cached receiver addresses without mistaking the initial
/// network scan for a Wi-Fi change. Empty results must clear stale addresses,
/// and repeated identical scans must not show a false reconnect warning.
fn apply_ip_snapshot(
    current:&mut Vec<Ipv4Addr>,
    snapshot_seen:&mut bool,
    changed_notice:&mut bool,
    mut next:Vec<Ipv4Addr>
)->bool{
    next.sort_unstable();
    next.dedup();
    let changed=*current!=next;
    if *snapshot_seen && changed {*changed_notice=true;}
    *snapshot_seen=true;
    if changed {*current=next;}
    changed
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

/// Valid Noise-PSK material must be 32 hexadecimal characters. Return
/// whether a new key was generated so the caller can persist it immediately.
fn ensure_pairing_secret(settings:&mut Settings)->bool{
    if settings.secret.len()==32 && settings.secret.bytes().all(|b|b.is_ascii_hexdigit()){
        return false;
    }
    settings.secret=generate_secret();
    true
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

#[derive(Clone,Copy,PartialEq,Eq,Debug)]
enum UiPage {Overview,Displays,Preferences,Diagnostics}
impl UiPage {
    fn label(self,lang:Language)->&'static str{
        match self {
            Self::Overview=>tr(lang,"概览","Overview"),
            Self::Displays=>tr(lang,"屏幕布局","Displays"),
            Self::Preferences=>tr(lang,"偏好设置","Preferences"),
            Self::Diagnostics=>tr(lang,"运行诊断","Diagnostics"),
        }
    }
}
fn section_card<R>(ui:&mut egui::Ui,title:&str,render:impl FnOnce(&mut egui::Ui)->R)->R{
    egui::Frame::group(ui.style())
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui,|ui|{
            ui.set_width(ui.available_width());
            ui.heading(title);
            ui.add_space(9.0);
            render(ui)
        }).inner
}

/// Two fixed-direction tables. Never ask the user to choose the OS on each
/// row: column headers identify which keyboard is pressed and which receives
/// the resulting event. The underlying persisted list stays backward compatible.
fn shortcut_mapping_group_ui(
    ui:&mut egui::Ui,
    rules:&mut Vec<input::ShortcutMapping>,
    from_os:model::Platform,
    lang:Language,
)->bool {
    ui.push_id(("shortcut-group",from_os==model::Platform::Windows),|ui|{
        let windows_to_mac=from_os==model::Platform::Windows;
        ui.heading(if windows_to_mac{
            tr(lang,"Windows → Mac 快捷键","Windows → Mac shortcuts")
        }else{
            tr(lang,"Mac → Windows 快捷键","Mac → Windows shortcuts")
        });

        let mut changed=false;
        let mut remove=None;
        egui::ScrollArea::vertical().max_height(315.0)
            .id_salt("shortcut-scroller").show(ui,|ui|{
                egui::Grid::new("shortcut-grid").striped(true).min_col_width(0.0)
                    .spacing([10.0,7.0]).show(ui,|ui|{
                    ui.strong(tr(lang,"启用","On"));
                    ui.strong(if windows_to_mac{
                        tr(lang,"Windows 输入快捷键","Windows input shortcut")
                    }else{
                        tr(lang,"Mac 输入快捷键","Mac input shortcut")
                    });
                    ui.strong(if windows_to_mac{
                        tr(lang,"Mac 接收端快捷键","Mac receiver shortcut")
                    }else{
                        tr(lang,"Windows 接收端快捷键","Windows receiver shortcut")
                    });
                    ui.strong(tr(lang,"操作","Action"));
                    ui.end_row();
                    for (index,rule) in rules.iter_mut().enumerate(){
                        if rule.from_os!=from_os{continue;}
                        changed|=ui.checkbox(&mut rule.enabled,"").changed();
                        changed|=ui.add(egui::TextEdit::singleline(&mut rule.source)
                            .desired_width(145.0)).changed();
                        changed|=ui.add(egui::TextEdit::singleline(&mut rule.target)
                            .desired_width(145.0)).changed();
                        ui.horizontal(|ui|{
                            if let Err(message)=rule.validate(){
                                ui.colored_label(egui::Color32::from_rgb(240,125,110),"⚠")
                                    .on_hover_text(message);
                            }
                            if ui.small_button(tr(lang,"删除","Delete")).clicked(){
                                remove=Some(index);
                            }
                        });
                        ui.end_row();
                    }
                });
            });
        if let Some(index)=remove {
            rules.remove(index);
            changed=true;
        }
        if ui.button(if windows_to_mac{
            tr(lang,"＋ 添加 Windows → Mac 映射","＋ Add Windows → Mac mapping")
        }else{
            tr(lang,"＋ 添加 Mac → Windows 映射","＋ Add Mac → Windows mapping")
        }).clicked(){
            rules.push(input::ShortcutMapping{
                from_os,source:String::new(),target:String::new(),enabled:false,
            });
            changed=true;
        }

        changed
    }).inner
}

/// Translate one complete pointer gesture into virtual desktop pixels.
/// `egui::Response::drag_delta` measures only the LAST FRAME, so it must
/// never be added to a stored drag origin. This helper always uses two
/// absolute pointer positions, including the final mouse-up event.
fn drag_screen_position(
    screen_origin:(i32,i32),
    press:egui::Pos2,
    current:egui::Pos2,
    scale:f32
)->(i32,i32){
    let delta=(current-press)/scale.max(0.001);
    (
        screen_origin.0.saturating_add(delta.x.round() as i32),
        screen_origin.1.saturating_add(delta.y.round() as i32)
    )
}

struct KvmApp {
    settings: Settings,
    state: Arc<engine::Shared>,
    ips: Vec<Ipv4Addr>,
    ip_snapshot_seen:bool,
    ip_changed_notice:bool,
    ip_monitor_disconnected:bool,
    ip_refresh_tx:mpsc::Sender<()>,
    drag_start_group:Option<(i32,i32)>,
    drag_start_pointer:Option<egui::Pos2>,
    /// The free position of the remote display while dragging.
    drag_preview:Option<(i32,i32)>,
    dragging_remote:Option<usize>,
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
    #[cfg(target_os="windows")]
    win_tray:Option<windows_tray::WindowsTray>,
    #[cfg(target_os="windows")]
    win_tray_attempted:bool,
    #[cfg(target_os="windows")]
    win_tray_error:Option<String>,
    frames:u32,
    page:UiPage,
    #[cfg(target_os="macos")]
    font_rx:Option<Receiver<Option<(&'static str,Vec<u8>)>>>,
    #[cfg(target_os="macos")]
    clipboard_job_rx:Receiver<engine::ClipboardUiJob>,
    #[cfg(target_os="macos")]
    font_failed:bool,
    #[cfg(target_os="macos")]
    font_ready:bool,
}
impl KvmApp {
    fn new(mut settings: Settings) -> Self {
        // Previously this secret was generated in RAM and *not saved* until
        // some unrelated setting changed. Restarting then silently rotated
        // the receiver PSK, so TCP connected but pairing immediately failed.
        if ensure_pairing_secret(&mut settings) && !cfg!(test) {
            model::save(&settings);
            launch_mark("pairing-key-generated-and-persisted");
        }
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
        // A single background worker tracks network interface changes for
        // the ENTIRE app lifetime. Wi-Fi rejoin/DHCP lease changes invalidate
        // the address shown on the Mac receiver, not just at startup.
        // Never call get_if_addrs() from an egui event callback.
        let (ip_tx,ip_rx)=mpsc::channel();
        let (ip_refresh_tx,ip_refresh_rx)=mpsc::channel::<()>();
        std::thread::spawn(move ||{
            launch_mark("network-monitor-start");
            let mut previous:Option<Vec<Ipv4Addr>>=None;
            loop {
                let addresses=network::local_ipv4_addresses();
                if previous.as_ref()!=Some(&addresses){
                    if ip_tx.send(addresses.clone()).is_err(){break;}
                    if previous.is_some(){launch_mark("network-ip-change-detected");}
                    previous=Some(addresses);
                }
                match ip_refresh_rx.recv_timeout(Duration::from_secs(2)){
                    Ok(())|Err(mpsc::RecvTimeoutError::Timeout)=>{},
                    Err(mpsc::RecvTimeoutError::Disconnected)=>break,
                }
            }
            launch_mark("network-monitor-stop");
        });
        let auto_start_waiting=settings.role_server && settings.auto_start_receiver;
        #[cfg(target_os="macos")]
        let clipboard_job_rx={
            let (tx,rx)=mpsc::channel();
            state.set_clipboard_ui_sender(tx);
            rx
        };
        #[cfg(target_os="macos")]
        let font_rx={
            let (tx,rx)=mpsc::channel();
            std::thread::spawn(move||{
                launch_mark("font-byte-read-start");
                let font=load_system_cjk_font();
                launch_mark(if font.is_some(){"font-byte-read-done"}else{"font-byte-read-missing"});
                let _=tx.send(font);
            });
            Some(rx)
        };
        Self{
            settings,state,ips:Vec::new(),
            ip_snapshot_seen:false,ip_changed_notice:false,
            ip_monitor_disconnected:false,ip_refresh_tx,
            drag_start_group:None,
            drag_start_pointer:None,
            drag_preview:None,dragging_remote:None,
            display_rx:Some(display_rx),ip_rx:Some(ip_rx),
            startup_at:Instant::now(),display_loaded:false,display_slow:false,
            auto_start_waiting,
            #[cfg(target_os="macos")]
            tray:None,
            #[cfg(target_os="macos")]
            tray_attempted:false,
            #[cfg(target_os="windows")]
            win_tray:None,
            #[cfg(target_os="windows")]
            win_tray_attempted:false,
            #[cfg(target_os="windows")]
            win_tray_error:None,
            frames:0,
            page:UiPage::Overview,
            #[cfg(target_os="macos")]
            font_rx,
            #[cfg(target_os="macos")]
            clipboard_job_rx,
            #[cfg(target_os="macos")]
            font_failed:false,
            #[cfg(target_os="macos")]
            font_ready:false,
        }
    }
    fn persist_settings(&self) { model::save(&self.settings); }

    fn receiver_addresses(&mut self, ui: &mut egui::Ui, lang: Language) {
        ui.horizontal(|ui| {
            ui.strong(tr(lang, "本机地址（控制端填写以下任一地址）", "Local addresses (enter one on the controller)"));
            if ui.button(tr(lang, "立即检查", "Check now")).clicked() {
                let _=self.ip_refresh_tx.send(());
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

        if self.ip_monitor_disconnected{
            ui.colored_label(egui::Color32::YELLOW,tr(lang,
                "IP 自动监测已停止，请重新启动 KVM。",
                "IP address monitoring has stopped; restart KVM."));
        }
    }

    /// Every physical display is drawn as a separate numbered monitor.
    /// Drag a blue panel near ANY grey panel: the nearest physical edge
    /// becomes the crossing anchor and the pair snaps exactly together.
    fn layout_ui(&mut self,ui:&mut egui::Ui,lang:Language){
        use model::{Display,group_origin,snap_to_edge};
        let local=self.state.local_displays();
        let remote=self.state.remote_displays.lock().unwrap().clone();
        if local.is_empty()||remote.is_empty(){return;}
        if self.frames==4{launch_mark("layout-displays-snapshotted");}
        self.settings.local_display=self.settings.local_display.min(local.len()-1);
        self.settings.remote_display=self.settings.remote_display.min(remote.len()-1);
        ui.heading(tr(lang,"显示器布局","Display arrangement"));

        // ONE stable egui interaction rectangle owns the entire gesture.
        // The remote display graphic moves independently; its changing
        // bounds must NOT become the gesture's hit target each frame.
        let (area,canvas)=ui.allocate_exact_size(
            egui::vec2(ui.available_width().min(730.0),365.0),egui::Sense::drag());
        let local_size=self.state.local_screen();
        let remote_size={*self.state.remote_screen.lock().unwrap()};
        let scale=(area.width()/(local_size.w.max(remote_size.w) as f32*3.1))
            .min(area.height()/(local_size.h.max(remote_size.h) as f32*3.2))
            .clamp(0.012,0.22);
        let local_origin=area.center()-egui::vec2(
            local_size.w as f32*scale*0.5,
            local_size.h as f32*scale*0.5);
        if self.frames==4{launch_mark("layout-screen-scale-computed");}
        let painter=ui.painter();
        painter.rect_filled(area,12.0,egui::Color32::from_rgb(19,28,46));
        let to_rect=|d:Display,gx:i32,gy:i32|{
            egui::Rect::from_min_size(
                local_origin+egui::vec2((d.x+gx) as f32*scale,(d.y+gy) as f32*scale),
                egui::vec2((d.w as f32*scale).max(12.0),(d.h as f32*scale).max(9.0)))
        };
        let (gx,gy)=group_origin(self.settings.layout,&local,&remote,
            self.settings.local_display,self.settings.remote_display);
        // A pointer-down inside a blue monitor chooses WHICH remote screen
        // moves. From then on the stable canvas ID retains pointer capture
        // even when the blue rectangle moves away from the original click.
        if canvas.drag_started(){
            let pressed=ui.ctx().input(|i|i.pointer.press_origin());
            if let Some(press)=pressed {
                let selected=(0..remote.len()).rev().find(|&i|{
                    to_rect(remote[i],gx,gy).contains(press)
                });
                if let Some(i)=selected{
                    let origin=(remote[i].x+gx,remote[i].y+gy);
                    self.dragging_remote=Some(i);
                    self.drag_start_group=Some(origin);
                    self.drag_start_pointer=Some(press);
                    self.drag_preview=Some(origin);
                }
            }
        }
        if canvas.dragged(){
            if let (Some(_),Some(origin),Some(press))=(
                self.dragging_remote,self.drag_start_group,self.drag_start_pointer
            ){
                if let Some(pointer)=canvas.interact_pointer_pos(){
                    self.drag_preview=Some(drag_screen_position(origin,press,pointer,scale));
                }
            }
        }
        if canvas.drag_stopped(){
            if let Some(i)=self.dragging_remote{
                let origin=self.drag_start_group.unwrap_or((remote[i].x+gx,remote[i].y+gy));
                let dropped=match (self.drag_start_pointer,canvas.interact_pointer_pos()){
                    (Some(press),Some(pointer))=>drag_screen_position(origin,press,pointer,scale),
                    _=>self.drag_preview.unwrap_or(origin),
                };
                let (li,layout)=snap_to_edge(&local,&remote,i,dropped.0,dropped.1);
                // The chosen adjacent edges touch EXACTLY (no gap).
                // Perpendicular offset stays free for different resolutions.
                self.settings.local_display=li;
                self.settings.remote_display=i;
                self.settings.layout=layout.clamp(local[li].screen(),remote[i].screen());
                *self.state.layout.lock().unwrap()=self.settings.layout;
                self.state.set_layout_targets(li,i);
                self.persist_settings();
            }
            self.dragging_remote=None;
            self.drag_preview=None;
            self.drag_start_group=None;
            self.drag_start_pointer=None;
        }
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
            let (x,y)=if self.dragging_remote==Some(i){
                self.drag_preview.unwrap_or((d.x+gx,d.y+gy))
            }else{(d.x+gx,d.y+gy)};
            let r=to_rect(*d,x-d.x,y-d.y);
            let selected=i==self.settings.remote_display||
                self.dragging_remote==Some(i);
            painter.rect_filled(r,4.0,if selected{
                egui::Color32::from_rgb(33,128,225)
            }else{egui::Color32::from_rgb(36,100,178)});
            painter.rect_stroke(r,4.0,
                egui::Stroke::new(if selected{2.0}else{1.0},
                    egui::Color32::from_rgb(121,217,252)),egui::StrokeKind::Inside);
            painter.text(r.center(),egui::Align2::CENTER_CENTER,
                format!("{} {}",tr(lang,"远端","Remote"),i+1),
                egui::FontId::proportional(12.0),egui::Color32::WHITE);
        }
        let ld=local[self.settings.local_display];
        let rd=remote[self.settings.remote_display];
        let bounds=if matches!(self.settings.layout.side,Side::Left|Side::Right){
            (-rd.h+24)..=(ld.h-24)
        }else{(-rd.w+24)..=(ld.w-24)};
        let before_offset=self.settings.layout.offset;
        ui.horizontal_wrapped(|ui|{
            ui.add(egui::Slider::new(&mut self.settings.layout.offset,bounds.clone())
                .text(tr(lang,"边缘偏移","Edge offset")));
            ui.add(egui::DragValue::new(&mut self.settings.layout.offset)
                .range(bounds.clone()).speed(1.0).suffix(" px"));
            if ui.button("−1 px").clicked(){
                self.settings.layout.offset=self.settings.layout.offset.saturating_sub(1)
                    .clamp(*bounds.start(),*bounds.end());
            }
            if ui.button("+1 px").clicked(){
                self.settings.layout.offset=self.settings.layout.offset.saturating_add(1)
                    .clamp(*bounds.start(),*bounds.end());
            }
        });
        if self.settings.layout.offset!=before_offset{
            *self.state.layout.lock().unwrap()=self.settings.layout;
            self.persist_settings();
        }


    }


    fn ui_overview(&mut self,ui:&mut egui::Ui,lang:Language){
        let started=self.state.started.load(Ordering::SeqCst);
        let connected=self.state.ready.load(Ordering::SeqCst);
        section_card(ui,tr(lang,"连接状态","Connection status"),|ui|{
            let status=self.state.status.lock().unwrap().clone();
            ui.horizontal_wrapped(|ui|{
                let (indicator,colour)=if connected {
                    (tr(lang,"● 已连接","● Connected"),egui::Color32::from_rgb(72,192,139))
                }else if started{
                    (tr(lang,"● 正在运行","● Running"),egui::Color32::from_rgb(236,182,78))
                }else{
                    (tr(lang,"● 未连接","● Disconnected"),ui.visuals().weak_text_color())
                };
                ui.colored_label(colour,egui::RichText::new(indicator).strong());
                ui.separator();
                ui.label(status.text(lang));
            });
            if self.state.active.load(Ordering::Relaxed){
                ui.colored_label(egui::Color32::from_rgb(72,192,139),
                    tr(lang,"正在控制另一台电脑 · F12 返回本机","Controlling remote computer · F12 to return"));
            }

        });
        ui.add_space(12.0);
        section_card(ui,tr(lang,"设备与连接","Devices & connection"),|ui|{
            ui.label(tr(lang,"本机角色","This computer's role"));
            ui.add_enabled_ui(!started,|ui|{
                ui.horizontal_wrapped(|ui|{
                    let old=self.settings.role_server;
                    ui.radio_value(&mut self.settings.role_server,true,
                        tr(lang,"接收模式","Receiver mode"));
                    ui.radio_value(&mut self.settings.role_server,false,
                        tr(lang,"控制模式","Controller mode"));
                    if old!=self.settings.role_server{self.persist_settings();}
                });
            });
            ui.add_space(8.0);
            if self.settings.role_server {
                ui.horizontal_wrapped(|ui|{
                    ui.label(format!("{}: {}",tr(lang,"监听端口","Listening port"),self.settings.listen_port));
                    ui.separator();
                    ui.label(tr(lang,"配对密钥","Pairing key"));
                });
                ui.horizontal_wrapped(|ui|{
                    ui.monospace(&self.settings.secret);
                    if ui.button(tr(lang,"复制密钥","Copy key")).clicked(){
                        ui.ctx().copy_text(self.settings.secret.clone());
                    }
                });
                self.receiver_addresses(ui,lang);
                if !started {
                    ui.colored_label(egui::Color32::from_rgb(236,182,78),
                        tr(lang,
                            "接收服务尚未监听。请点击下方「启动接收端」，否则 Windows 会被拒绝连接。",
                            "Receiver is not listening. Click Start receiver below or Windows connections will be refused."));
                }

                #[cfg(target_os="macos")]
                if !native_cursor::accessibility_trusted(){
                    ui.colored_label(egui::Color32::YELLOW,tr(lang,
                        "未获得辅助功能权限；鼠标点击和键盘注入可能无法工作。",
                        "Accessibility permission missing; remote clicks/keys may not work."));
                }
            }else{
                ui.horizontal_wrapped(|ui|{
                    ui.label(tr(lang,"接收端 IP:端口","Receiver IP:port"));
                    if ui.text_edit_singleline(&mut self.settings.address).changed(){
                        self.persist_settings();
                    }
                });
                ui.horizontal_wrapped(|ui|{
                    ui.label(tr(lang,"配对密钥","Pairing key"));
                    if ui.add(egui::TextEdit::singleline(&mut self.settings.secret)
                        .password(true).desired_width(260.0)).changed(){
                        self.persist_settings();
                    }
                });
            }
            ui.add_space(9.0);
            if ui.add_enabled(!started,egui::Button::new(
                if self.settings.role_server {
                    tr(lang,"启动接收端","Start receiver")
                }else{tr(lang,"连接并开始共享","Connect & share")}
            )).clicked(){
                self.persist_settings();
                if self.settings.role_server {
                    engine::start_server(self.state.clone(),self.settings.clone());
                }else{engine::start_client(self.state.clone(),self.settings.clone());}
            }
            if started{
                ui.horizontal_wrapped(|ui|{
                    if self.settings.role_server {
                        if ui.add_enabled(connected,egui::Button::new(
                            tr(lang,"断开当前控制端","Disconnect controller"))).clicked(){
                            self.state.disconnect_peer();
                        }
                        if ui.button(tr(lang,"停止接收","Stop receiver")).clicked(){
                            self.state.stop();
                        }
                    }else if ui.button(tr(lang,"断开连接","Disconnect")).clicked(){
                        self.state.stop();
                    }
                });
            }
        });
        ui.add_space(12.0);
    }

    fn ui_displays(&mut self,ui:&mut egui::Ui,lang:Language){
        if self.settings.role_server {
            section_card(ui,tr(lang,"本机显示器","Local monitors"),|ui|{
                let displays=self.state.local_displays();
                ui.label(format!("{}: {}",tr(lang,"检测到的显示器","Detected monitors"),displays.len()));
                for (i,d) in displays.iter().enumerate(){
                    ui.horizontal_wrapped(|ui|{
                        ui.strong(format!("{} {}",tr(lang,"显示器","Monitor"),i+1));
                        ui.label(format!("{} × {} · ({}, {})",d.w,d.h,d.x,d.y));
                    });
                }
                ui.add_space(5.0);

            });
        }else{
            section_card(ui,tr(lang,"拖拽与自动吸附","Drag & snap"),|ui|{
                self.layout_ui(ui,lang);
            });
        }
    }

    fn ui_preferences(&mut self,ui:&mut egui::Ui,lang:Language){
        section_card(ui,tr(lang,"通用设置","General"),|ui|{
            ui.horizontal_wrapped(|ui|{
                ui.label("Language / 语言");
                let name=if lang==Language::Zh{"中文"}else{"English"};
                let old=self.settings.language;
                egui::ComboBox::from_id_salt("language-preferences").selected_text(name)
                    .show_ui(ui,|ui|{
                        ui.selectable_value(&mut self.settings.language,Language::Zh,"中文");
                        ui.selectable_value(&mut self.settings.language,Language::En,"English");
                    });
                if old!=self.settings.language{self.persist_settings();}
            });
            if ui.checkbox(&mut self.settings.clipboard,
                tr(lang,"剪贴板按需同步（文本、图片、文件；单次最多 32 MiB）",
                    "Clipboard on demand (text, images, files; 32 MiB per paste)")).changed(){
                self.state.clipboard.store(self.settings.clipboard,Ordering::SeqCst);
                self.persist_settings();
            }
        });
        ui.add_space(12.0);
        section_card(ui,tr(lang,"快捷键映射（显式配置）","Shortcut mappings (explicit)"),|ui|{

            let mut changed=false;
            changed|=shortcut_mapping_group_ui(ui,&mut self.settings.shortcut_mappings,
                model::Platform::Windows,lang);
            ui.separator();
            changed|=shortcut_mapping_group_ui(ui,&mut self.settings.shortcut_mappings,
                model::Platform::Mac,lang);
            ui.separator();
            if ui.button(tr(lang,"恢复两张表的可见预设","Restore visible presets for both lists")).clicked(){
                self.settings.shortcut_mappings=input::presets();
                changed=true;
            }
            if changed {
                self.state.set_shortcuts(&self.settings.shortcut_mappings);
                self.persist_settings();
            }
            let table=input::ShortcutTable::compile(&self.settings.shortcut_mappings);
            if table.errors()>0{
                ui.colored_label(egui::Color32::from_rgb(240,125,110),
                    format!("{}: {}",tr(lang,"无效或重复映射","Invalid or duplicate shortcuts"),table.errors()));
            }
        });
        ui.add_space(12.0);
        if self.settings.role_server{
            section_card(ui,tr(lang,"接收端配置","Receiver configuration"),|ui|{
                if ui.checkbox(&mut self.settings.auto_start_receiver,
                    tr(lang,"下次启动时自动接收","Auto-start receiver on next launch")).changed(){
                    self.persist_settings();
                }
                ui.horizontal_wrapped(|ui|{
                    ui.label(tr(lang,"监听端口","Listening port"));
                    let old=self.settings.listen_port;
                    ui.add_enabled_ui(!self.state.started.load(Ordering::SeqCst),|ui|{
                        if ui.add(egui::DragValue::new(&mut self.settings.listen_port)
                            .range(1024..=65535).speed(1.0)).changed(){
                            self.persist_settings();
                        }
                        if ui.button(tr(lang,"生成新端口","Generate port")).clicked(){
                            self.settings.listen_port=generate_port(old);
                            self.persist_settings();
                        }
                    });
                });
                ui.horizontal_wrapped(|ui|{
                    ui.label(tr(lang,"配对密钥","Pairing key"));
                    if ui.add_enabled(!self.state.started.load(Ordering::SeqCst),
                        egui::Button::new(tr(lang,"重新生成密钥","Regenerate pairing key"))).clicked(){
                        self.settings.secret=generate_secret();
                        self.persist_settings();
                    }
                });


            });
            section_card(ui,tr(lang,"鼠标侧键","Extra mouse buttons"),|ui|{
                let was=self.settings.side_buttons_native;
                ui.checkbox(&mut self.settings.side_buttons_native,
                    tr(lang,"发送原生鼠标第 4/5 键（而不是浏览器后退/前进）",
                       "Send native mouse buttons 4/5 (instead of browser back/forward)"));
                if was!=self.settings.side_buttons_native {
                    self.state.side_buttons_native.store(
                        self.settings.side_buttons_native,Ordering::SeqCst);
                    self.persist_settings();
                }

            });
        }else{
            section_card(ui,tr(lang,"鼠标体验","Mouse & pointer"),|ui|{
                ui.label(tr(lang,"远程鼠标灵敏度","Remote mouse sensitivity"));
                if ui.add(egui::Slider::new(&mut self.settings.mouse_speed,0.5..=4.0)
                    .step_by(0.1).suffix("×")).changed(){
                    self.state.set_mouse_speed(self.settings.mouse_speed);
                    self.persist_settings();
                }

                ui.add_space(8.0);
                ui.label(tr(lang,"远端滚轮速度（像素级，1× ≈ 4 px/格）",
                    "Remote wheel speed (pixel-level, 1x ≈ 4 px/notch)"));
                if ui.add(egui::Slider::new(&mut self.settings.scroll_speed,0.1..=16.0)
                    .step_by(0.1).suffix("×")).changed(){
                    self.state.set_scroll_speed(self.settings.scroll_speed);
                    self.persist_settings();
                }

            });
        }
    }

    fn ui_diagnostics(&mut self,ui:&mut egui::Ui,lang:Language){
        section_card(ui,tr(lang,"会话指标","Session metrics"),|ui|{
            let rtt=self.state.rtt_ms.load(Ordering::Relaxed);
            let connected=self.state.ready.load(Ordering::SeqCst);
            let status=self.state.status.lock().unwrap().clone();
            ui.label(format!("{}: {}",tr(lang,"状态","Status"),status.text(lang)));
            #[cfg(target_os="windows")]
            if let Some(error)=&self.win_tray_error{
                ui.colored_label(egui::Color32::from_rgb(240,125,110),
                    format!("{}: {error}",tr(lang,"Windows 托盘不可用","Windows tray unavailable")));
            }
            ui.label(format!("{}: {}",tr(lang,"网络往返延迟","Network RTT"),
                if rtt==u64::MAX{"—".to_string()}else{format!("{rtt} ms")}));
            ui.label(format!("{}: {} ms",tr(lang,"本次连接 RTT 峰值","Session RTT peak"),
                self.state.rtt_peak_ms.load(Ordering::Relaxed)));
            ui.label(format!("{}: {}",tr(lang,"Windows 原生滚轮数据包","Native wheel packets"),
                self.state.wheel_packets.load(Ordering::Relaxed)));
            ui.label(format!("{}: {}",tr(lang,"侧键事件（Windows 捕获）","Side button events captured"),
                self.state.side_button_packets.load(Ordering::Relaxed)));
            ui.label(format!("{}: {}",tr(lang,"快捷键匹配次数（控制端）","Shortcut matches (controller)"),
                self.state.shortcuts_matched.load(Ordering::Relaxed)));
            ui.label(format!("{}: {} / {}",
                tr(lang,"快捷键注入尝试 / 报错（接收端）","Shortcut injection attempts / errors (receiver)"),
                self.state.shortcuts_injected.load(Ordering::Relaxed),
                self.state.shortcuts_failed.load(Ordering::Relaxed)));
            ui.label(format!("{}: {}",tr(lang,"原生鼠标数据包","Native mouse packets"),
                self.state.native_motion_packets.load(Ordering::Relaxed)));
            ui.label(format!("{}: {} / {}",tr(lang,"鼠标按下 / 松开（捕获）","Mouse down/up captured"),
                self.state.mouse_down_captured.load(Ordering::Relaxed),
                self.state.mouse_up_captured.load(Ordering::Relaxed)));
            ui.label(format!("{}: {} / {}",tr(lang,"鼠标按下 / 松开（接收）","Mouse down/up received"),
                self.state.mouse_down_received.load(Ordering::Relaxed),
                self.state.mouse_up_received.load(Ordering::Relaxed)));
            ui.label(format!("{}: {}",tr(lang,"注入失败次数","Injection failures"),
                self.state.mouse_injection_failures.load(Ordering::Relaxed)));
            if !connected {
    
            }
            if ui.button(tr(lang,"复制完整诊断","Copy full diagnostics")).clicked(){
                let diagnostic=format!(
                    "kvm {}\nOS: {}\nRole: {}\nAddress: {}\nConfigured listening port: {}\nSession started: {}\nNetwork ready: {}\nMouse sensitivity: {:.1}x\nScroll speed: {:.1}x (4 px/notch at 1x)\nNetwork RTT: {:?} ms\nRTT peak: {} ms\nWheel packets: {}\nSide button events: {}\nShortcut matches (controller): {}\nShortcut injection attempts (receiver): {}\nShortcut injection errors (receiver): {}\nNative mouse packets: {}\nMouse captured down/up: {}/{}\nMouse received down/up: {}/{}\nMouse injection failures: {}\nStatus: {}",
                    env!("CARGO_PKG_VERSION"),
                    std::env::consts::OS,
                    if self.settings.role_server{"receiver"}else{"controller"},
                    if self.settings.role_server{"(receiver)"}else{self.settings.address.as_str()},
                    self.settings.listen_port,
                    self.state.started.load(Ordering::SeqCst),
                    self.state.ready.load(Ordering::SeqCst),
                    self.settings.mouse_speed,
                    self.settings.scroll_speed,
                    if rtt==u64::MAX{None}else{Some(rtt)},
                    self.state.rtt_peak_ms.load(Ordering::Relaxed),
                    self.state.wheel_packets.load(Ordering::Relaxed),
                    self.state.side_button_packets.load(Ordering::Relaxed),
                    self.state.shortcuts_matched.load(Ordering::Relaxed),
                    self.state.shortcuts_injected.load(Ordering::Relaxed),
                    self.state.shortcuts_failed.load(Ordering::Relaxed),
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
        ui.add_space(12.0);
        section_card(ui,tr(lang,"最近连接事件","Recent connection events"),|ui|{
            let history=self.state.history.lock().unwrap();
            if history.is_empty(){
                ui.small(tr(lang,"暂无事件","No events yet"));
            }else{
                for line in history.iter().rev().take(20){
                    ui.monospace(line);
                }
            }
        });
    }

}

impl eframe::App for KvmApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames=self.frames.saturating_add(1);
        if self.frames==1{launch_mark("first-ui-frame");}
        #[cfg(target_os="macos")]
        {
            if self.frames==1 {self.state.set_clipboard_context(ctx.clone());}
            // Run NSPasteboard calls on the AppKit GUI thread, never in the
            // receiver/network worker. Do not hold Shared mutexes here.
            for _ in 0..4 {
                match self.clipboard_job_rx.try_recv(){
                    Ok(engine::ClipboardUiJob::Snapshot(reply))=>{
                        let _=reply.send(clipboard::snapshot());
                    }
                    Ok(engine::ClipboardUiJob::Install(data,reply))=>{
                        let _=reply.send(clipboard::install(&data));
                    }
                    Err(_)=>break,
                }
            }
        }
        #[cfg(target_os="macos")]
        if self.frames>=6 {
            if let Some(rx)=self.font_rx.take(){
                match rx.try_recv(){
                    Ok(Some((path,bytes)))=>{
                        launch_mark("font-install-on-gui-start");
                        ctx.set_fonts(font_definitions_with_cjk(bytes));
                        self.font_ready=true;
                        launch_mark(&format!("font-install-on-gui-done: {path}"));
                        ctx.request_repaint();
                    }
                    Ok(None)|Err(TryRecvError::Disconnected)=>{
                        self.font_failed=true;
                        launch_mark("font-install-failed-no-system-cjk-font");
                    }
                    Err(TryRecvError::Empty)=>{self.font_rx=Some(rx);}
                }
            }
        }
        if self.frames>=3 {
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
        }
        let mut newest_addresses=None;
        let mut monitor_closed=false;
        if let Some(rx)=&self.ip_rx {
            // Drain snapshots if Wi-Fi rapidly disconnects and rejoins;
            // the latest observed interface state is what the UI must show.
            loop {
                match rx.try_recv(){
                    Ok(addresses)=>{newest_addresses=Some(addresses);}
                    Err(TryRecvError::Empty)=>break,
                    Err(TryRecvError::Disconnected)=>{
                        monitor_closed=true;
                        break;
                    }
                }
            }
        }
        if let Some(addresses)=newest_addresses {
            if apply_ip_snapshot(&mut self.ips,&mut self.ip_snapshot_seen,
                &mut self.ip_changed_notice,addresses){
                ctx.request_repaint();
            }
        }
        if monitor_closed {
            self.ip_rx=None;
            self.ip_monitor_disconnected=true;
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
        #[cfg(target_os="windows")]
        {
            // The Windows tray must be created on the GUI/message-loop thread.
            // Never hide the window if creating the tray failed.
            if !self.win_tray_attempted && self.frames>=2 {
                self.win_tray_attempted=true;
                match windows_tray::WindowsTray::new(ctx,self.settings.language,self.state.clone()){
                    Ok(tray)=>self.win_tray=Some(tray),
                    Err(error)=>self.win_tray_error=Some(error),
                }
            }
            if let Some(tray)=&mut self.win_tray {
                tray.set_language(self.settings.language);
                // Process Open before Close, and never intercept explicit Quit.
                if tray.take_open_request(){
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                if ctx.input(|i|i.viewport().close_requested())
                    && windows_tray::should_hide_close(true,tray.quitting()){
                    ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
            }
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
        let lang=self.settings.language;
        egui::SidePanel::left("kvm-navigation")
            .resizable(false)
            .exact_width(182.0)
            .show(ctx,|ui|{
                ui.add_space(18.0);
                ui.heading(egui::RichText::new("kvm").size(30.0).strong());
                ui.small(format!("v{}",env!("CARGO_PKG_VERSION")));

                ui.add_space(22.0);
                for page in [UiPage::Overview,UiPage::Displays,UiPage::Preferences,UiPage::Diagnostics]{
                    let label=page.label(lang);
                    if ui.add_sized([ui.available_width(),38.0],
                        egui::Button::selectable(self.page==page,label)).clicked(){
                        self.page=page;
                    }
                    ui.add_space(4.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT),|ui|{
                    ui.add_space(12.0);
                    let connected=self.state.ready.load(Ordering::Relaxed);
                    let started=self.state.started.load(Ordering::Relaxed);
                    let colour=if connected{egui::Color32::from_rgb(72,192,139)}
                        else if started{egui::Color32::from_rgb(236,182,78)}
                        else{ui.visuals().weak_text_color()};
                    ui.colored_label(colour,if connected {
                        tr(lang,"● 已连接","● Connected")
                    }else if started{
                        tr(lang,"● 正在运行","● Running")
                    }else{tr(lang,"● 未连接","● Offline")});
                });
            });
        egui::CentralPanel::default().show(ctx,|ui|{
            ui.add_space(12.0);
            ui.heading(self.page.label(lang));
            ui.separator();
            egui::ScrollArea::vertical().id_salt("kvm-main-scroll")
                .auto_shrink([false,false]).show(ui,|ui|{
                    ui.add_space(10.0);
                    if self.display_slow{
                        ui.colored_label(egui::Color32::YELLOW,tr(lang,
                            "外接屏幕检测较慢，暂时使用单屏回退；重启后可刷新布局。",
                            "Monitor scan is slow; using temporary single-screen fallback."));
                        ui.add_space(12.0);
                    }
                    if self.settings.role_server && self.ip_changed_notice {
                        egui::Frame::group(ui.style())
                            .fill(ui.visuals().extreme_bg_color)
                            .show(ui,|ui|{
                                ui.colored_label(egui::Color32::YELLOW,
                                    tr(lang,
                                        "检测到 Mac 网络地址变化。Windows 中保存的旧 IP 可能失效，请在「概览」复制最新 IP:端口并重新连接。",
                                        "This Mac's IP address changed. The old address saved on Windows may no longer work. Copy the current IP:port from Overview and reconnect."));
                                if ui.button(tr(lang,"知道了","Dismiss")).clicked(){
                                    self.ip_changed_notice=false;
                                }
                            });
                        ui.add_space(12.0);
                    }
                    match self.page{
                        UiPage::Overview=>self.ui_overview(ui,lang),
                        UiPage::Displays=>self.ui_displays(ui,lang),
                        UiPage::Preferences=>self.ui_preferences(ui,lang),
                        UiPage::Diagnostics=>self.ui_diagnostics(ui,lang),
                    }
                    ui.add_space(22.0);
                });
        });
        if self.frames==4{launch_mark("full-ui-returned");}
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}

fn main() -> eframe::Result<()> {
    launch_mark("main-enter");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 720.0])
            .with_min_inner_size([720.0, 520.0])
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

/// Exercise the actual egui multi-monitor editor, not just geometry helpers.
/// Before the one-snapshot fix, it tried to lock remote_screen a second time
/// inside the same let expression and blocked the test thread indefinitely.
#[cfg(test)]
mod cjk_font_tests{
    use super::*;
    #[test]
    fn cjk_font_precedes_default_fallbacks(){
        let fonts=font_definitions_with_cjk(vec![0,1,2,3]);
        assert!(fonts.font_data.contains_key("system-cjk"));
        for family in [egui::FontFamily::Proportional,egui::FontFamily::Monospace]{
            assert_eq!(fonts.families.get(&family).and_then(|names|names.first()).map(String::as_str),
                Some("system-cjk"));
        }
    }
}

#[cfg(test)]
mod ui_deadlock_tests {
    use super::*;
    #[test]
    fn generated_pairing_key_is_valid_and_stable(){
        let mut s=Settings::default();
        assert!(ensure_pairing_secret(&mut s));
        assert_eq!(s.secret.len(),32);
        let initial=s.secret.clone();
        assert!(!ensure_pairing_secret(&mut s));
        assert_eq!(s.secret,initial);
        s.secret="not-a-hex-key".into();
        assert!(ensure_pairing_secret(&mut s));
        assert_eq!(s.secret.len(),32);
    }
    #[test]
    fn controller_layout_first_full_frame_completes() {
        let (tx,rx)=mpsc::channel();
        std::thread::spawn(move || {
            let mut app=KvmApp::new(Settings::default());
            let ctx=egui::Context::default();
            let _=ctx.run(egui::RawInput::default(),|ctx|{
                egui::CentralPanel::default().show(ctx,|ui|{
                    app.layout_ui(ui,Language::En);
                });
            });
            let _=tx.send(());
        });
        assert!(rx.recv_timeout(Duration::from_secs(8)).is_ok(),
            "multi-monitor layout hung while rendering its first full frame");
    }
}

/// Navigation regressions: every page must paint without hanging on locks.
#[cfg(test)]
mod page_navigation_tests {
    use super::*;
    #[test]
    fn all_pages_render_in_egui(){
        let (tx,rx)=mpsc::channel();
        std::thread::spawn(move||{
            let mut app=KvmApp::new(Settings::default());
            let ctx=egui::Context::default();
            for page in [
                UiPage::Overview,UiPage::Displays,UiPage::Preferences,UiPage::Diagnostics
            ]{
                let _=ctx.run(egui::RawInput::default(),|ctx|{
                    egui::CentralPanel::default().show(ctx,|ui|{
                        match page {
                            UiPage::Overview=>app.ui_overview(ui,Language::En),
                            UiPage::Displays=>app.ui_displays(ui,Language::En),
                            UiPage::Preferences=>app.ui_preferences(ui,Language::En),
                            UiPage::Diagnostics=>app.ui_diagnostics(ui,Language::En),
                        }
                    });
                });
            }
            let _=tx.send(());
        });
        assert!(rx.recv_timeout(Duration::from_secs(8)).is_ok(),
            "one of the four UI pages did not finish rendering");
    }
}

/// LAN address monitoring tests: fresh DHCP leases must replace cached IPs.
#[cfg(test)]
mod receiver_network_refresh_tests{
    use super::*;
    #[test]
    fn wifi_rejoin_updates_and_does_not_keep_old_address(){
        let first=Ipv4Addr::new(192,168,20,148);
        let next=Ipv4Addr::new(192,168,20,163);
        let mut ips=Vec::new();
        let mut seen=false;
        let mut notice=false;
        assert!(apply_ip_snapshot(&mut ips,&mut seen,&mut notice,vec![first]));
        assert_eq!(ips,vec![first]);
        assert!(!notice,"initial scan must not be called a network change");
        assert!(!apply_ip_snapshot(&mut ips,&mut seen,&mut notice,vec![first]));
        assert!(!notice,"unchanged Wi-Fi address should not trigger warning");
        assert!(apply_ip_snapshot(&mut ips,&mut seen,&mut notice,Vec::new()));
        assert!(ips.is_empty(),"disconnected Wi-Fi must clear stale IP");
        assert!(notice,"a change after initial scan needs a reconnect warning");
        notice=false;
        assert!(apply_ip_snapshot(&mut ips,&mut seen,&mut notice,vec![next]));
        assert_eq!(ips,vec![next]);
        assert!(notice);
        assert!(!ips.contains(&first));
    }
    #[test]
    fn duplicate_and_reordered_ips_are_stable(){
        let one=Ipv4Addr::new(10,0,0,4);
        let two=Ipv4Addr::new(192,168,20,151);
        let mut ips=Vec::new();
        let mut seen=false;
        let mut notice=false;
        assert!(apply_ip_snapshot(&mut ips,&mut seen,&mut notice,vec![two,one,one]));
        assert_eq!(ips,vec![one,two]);
        assert!(!apply_ip_snapshot(&mut ips,&mut seen,&mut notice,vec![one,two]));
        assert!(!notice);
    }
}

#[cfg(test)]
mod layout_drag_regression_tests{
    use super::*;
    #[test]
    fn movement_uses_full_gesture_not_last_frame(){
        // Screens have different resolutions. Several frames of motion must
        // accumulate from the INITIAL click rather than springing back.
        let start=(1920,200);
        let press=egui::pos2(400.0,200.0);
        let frame1=drag_screen_position(start,press,egui::pos2(390.0,210.0),0.1);
        let frame2=drag_screen_position(start,press,egui::pos2(290.0,250.0),0.1);
        let frame3=drag_screen_position(start,press,egui::pos2(285.0,270.0),0.1);
        assert_eq!(frame1,(1820,300));
        assert_eq!(frame2,(820,700));
        assert_eq!(frame3,(770,900));
        assert_eq!(drag_screen_position(start,press,egui::pos2(285.0,270.0),0.1),frame3);
        let local=model::single_display(model::Screen::new(2560,1440));
        let remote=model::single_display(model::Screen::new(1920,1080));
        let (_,snap)=model::snap_to_edge(&local,&remote,0,-1920,220);
        assert_eq!(snap.side,Side::Left);
        assert_eq!(snap.offset,220);
        assert_eq!(model::group_origin(snap,&local,&remote,0,0).0,-1920);
    }
}
