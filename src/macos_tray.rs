//! macOS status bar integration. Built only on macOS.
//! The menu lives on the main thread; events request eframe viewport commands.
use crate::model::Language;
use eframe::egui;
use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
use tray_icon::{Icon,TrayIcon,TrayIconBuilder,menu::{Menu,MenuItem,MenuEvent}};

pub struct MacMenuBar {
    _tray: TrayIcon,
    open: MenuItem,
    quit: MenuItem,
    quitting: Arc<AtomicBool>,
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
    pub fn new(ctx:&egui::Context,language:Language)->Result<Self,String>{
        let open=MenuItem::new(
            if language==Language::Zh{"打开设置"}else{"Open settings"},true,None);
        let quit=MenuItem::new(
            if language==Language::Zh{"退出 kvm"}else{"Quit kvm"},true,None);
        let menu=Menu::new();
        menu.append(&open).map_err(|e|e.to_string())?;
        menu.append(&quit).map_err(|e|e.to_string())?;
        let open_id=open.id().clone();
        let quit_id=quit.id().clone();
        let quitting=Arc::new(AtomicBool::new(false));
        let will_quit=quitting.clone();
        let ctx=ctx.clone();
        MenuEvent::set_event_handler(Some(move |e:MenuEvent|{
            if e.id==open_id {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
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
        Ok(Self{_tray:tray,open,quit,quitting,language})
    }
    pub fn quitting(&self)->bool{self.quitting.load(Ordering::SeqCst)}
    pub fn set_language(&mut self, language:Language){
        if self.language==language{return;}
        self.language=language;
        self.open.set_text(if language==Language::Zh{"打开设置"}else{"Open settings"});
        self.quit.set_text(if language==Language::Zh{"退出 kvm"}else{"Quit kvm"});
    }
}
