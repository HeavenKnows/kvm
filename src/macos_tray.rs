//! macOS status bar integration. Built only on macOS.
//! The menu lives on the main thread; events request eframe viewport commands.
use crate::{model::Language,engine::Shared};
use eframe::egui;
use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
use tray_icon::{Icon,TrayIcon,TrayIconBuilder,menu::{Menu,MenuItem,MenuEvent}};

pub struct MacMenuBar {
    _tray: TrayIcon,
    open: MenuItem,
    disconnect: MenuItem,
    quit: MenuItem,
    quitting: Arc<AtomicBool>,
    open_requested:Arc<AtomicBool>,
    language: Language,
}

fn status_icon() -> Result<Icon, String> {
    let (w,h)=(20u32,20u32);
    let mut rgba=vec![0u8;(w*h*4) as usize];
    // Small pair of overlapping displays, drawn using the alpha channel.
    for y in 0..h {
        for x in 0..w {
            let a=(x>=1 && x<=12 && (y==3 || y==12))
                || (y>=3 && y<=12 && (x==1 || x==12))
                || (x>=7 && x<=18 && (y==7 || y==16))
                || (y>=7 && y<=16 && (x==7 || x==18));
            let i=((y*w+x)*4) as usize;
            if a { rgba[i]=0;rgba[i+1]=0;rgba[i+2]=0;rgba[i+3]=255; }
        }
    }
    Icon::from_rgba(rgba,w,h).map_err(|e|e.to_string())
}
impl MacMenuBar {
    pub fn new(ctx:&egui::Context,language:Language,state:Arc<Shared>)->Result<Self,String>{
        let open=MenuItem::new(
            if language==Language::Zh{"打开设置"}else{"Open settings"},true,None);
        let disconnect=MenuItem::new(
            if language==Language::Zh{"断开当前连接"}else{"Disconnect current session"},true,None);
        let quit=MenuItem::new(
            if language==Language::Zh{"退出 kvm"}else{"Quit kvm"},true,None);
        let menu=Menu::new();
        menu.append(&open).map_err(|e|e.to_string())?;
        menu.append(&disconnect).map_err(|e|e.to_string())?;
        menu.append(&quit).map_err(|e|e.to_string())?;
        let open_id=open.id().clone();
        let disconnect_id=disconnect.id().clone();
        let quit_id=quit.id().clone();
        let quitting=Arc::new(AtomicBool::new(false));
        let open_requested=Arc::new(AtomicBool::new(false));
        let will_open=open_requested.clone();
        let will_quit=quitting.clone();
        let ctx=ctx.clone();
        MenuEvent::set_event_handler(Some(move |e:MenuEvent|{
            if e.id==open_id {
                will_open.store(true,Ordering::SeqCst);
                // GUI thread will show/focus and activate the app. Do not
                // call AppKit from this menu event handler thread.
                ctx.request_repaint();
            } else if e.id==disconnect_id {
                if state.ready.load(Ordering::SeqCst) { state.disconnect_peer(); }
                ctx.request_repaint();
            } else if e.id==quit_id {
                will_quit.store(true,Ordering::SeqCst);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                ctx.request_repaint();
            }
        }));
        let tray=TrayIconBuilder::new()
            .with_tooltip("kvm")
            .with_menu(Box::new(menu))
            .with_icon_templated(status_icon()?)
            .build().map_err(|e|e.to_string())?;
        Ok(Self{_tray:tray,open,disconnect,quit,quitting,open_requested,language})
    }
    pub fn quitting(&self)->bool{self.quitting.load(Ordering::SeqCst)}
    pub fn take_open_request(&self)->bool{
        self.open_requested.swap(false,Ordering::SeqCst)
    }
    pub fn set_language(&mut self, language:Language){
        if self.language==language{return;}
        self.language=language;
        self.open.set_text(if language==Language::Zh{"打开设置"}else{"Open settings"});
        self.disconnect.set_text(if language==Language::Zh{"断开当前连接"}else{"Disconnect current session"});
        self.quit.set_text(if language==Language::Zh{"退出 kvm"}else{"Quit kvm"});
    }
}

/// Make the settings window visible when running as an LSUIElement accessory
/// (no Dock or Cmd+Tab). Must run on the macOS main thread.
pub fn activate_app(){
    // objc::msg_send! expands to sel!, which must be explicitly in scope.
    use objc::{sel,sel_impl};
    unsafe {
        let cls=objc::class!(NSApplication);
        let app:*mut objc::runtime::Object=objc::msg_send![cls,sharedApplication];
        if !app.is_null(){
            #[allow(deprecated)]
            let _:()=objc::msg_send![app,activateIgnoringOtherApps: 1_i8];
        }
    }
}
