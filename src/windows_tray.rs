//! Windows notification-area integration. Construct the native icon only
//! after eframe/winit has entered its Windows message loop (on the GUI thread).
//! Closing the settings viewport hides it; only the tray's Quit command exits.
#![cfg(target_os="windows")]
use crate::{engine::Shared,model::Language};
use eframe::egui;
use std::sync::{Arc,atomic::{AtomicBool,Ordering}};
use tray_icon::{Icon,TrayIcon,TrayIconBuilder,TrayIconEvent,MouseButton,MouseButtonState,
    menu::{Menu,MenuEvent,MenuItem}};

pub struct WindowsTray {
    _tray:TrayIcon,
    open:MenuItem,
    quit:MenuItem,
    open_requested:Arc<AtomicBool>,
    quitting:Arc<AtomicBool>,
    language:Language,
}

fn status_icon()->Result<Icon,String>{
    // Crisp 24x24 blue dual-monitor icon, readable on light/dark taskbars.
    let (w,h)=(24u32,24u32);
    let mut rgba=vec![0u8;(w*h*4) as usize];
    for y in 0..h {
        for x in 0..w {
            let rear=(x>=2&&x<=15&&(y==3||y==14))||
                (y>=3&&y<=14&&(x==2||x==15));
            let front=(x>=8&&x<=21&&(y==9||y==20))||
                (y>=9&&y<=20&&(x==8||x==21));
            let i=((y*w+x)*4) as usize;
            if front{rgba[i]=25;rgba[i+1]=141;rgba[i+2]=238;rgba[i+3]=255;}
            else if rear{rgba[i]=130;rgba[i+1]=168;rgba[i+2]=208;rgba[i+3]=255;}
        }
    }
    Icon::from_rgba(rgba,w,h).map_err(|e|e.to_string())
}
pub fn should_hide_close(tray_available:bool,quitting:bool)->bool{
    tray_available && !quitting
}
impl WindowsTray {
    pub fn new(ctx:&egui::Context,language:Language,shared:Arc<Shared>)->Result<Self,String>{
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
        let open_requested=Arc::new(AtomicBool::new(false));
        let will_open=open_requested.clone();
        let will_quit=quitting.clone();
        let menu_ctx=ctx.clone();
        let quit_shared=shared;
        MenuEvent::set_event_handler(Some(move |event:MenuEvent|{
            if event.id==open_id {
                will_open.store(true,Ordering::SeqCst);
                menu_ctx.request_repaint();
            }else if event.id==quit_id {
                will_quit.store(true,Ordering::SeqCst);
                // A hidden viewport has no reliable redraw/Close dispatch.
                // Quit MUST happen in this native menu callback, not in the
                // eframe update loop (which may never be called while hidden).
                // First return remote input ownership and close the socket.
                quit_shared.stop();
                std::process::exit(0);
            }
        }));
        // Users can double-click the icon rather than opening the menu.
        let click_open=open_requested.clone();
        let click_ctx=ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event:TrayIconEvent|{
            if let TrayIconEvent::Click{button:MouseButton::Left,
                    button_state:MouseButtonState::Up,..}=event{
                click_open.store(true,Ordering::SeqCst);
                click_ctx.request_repaint();
            }
        }));
        let tray=TrayIconBuilder::new()
            .with_tooltip("kvm")
            .with_menu(Box::new(menu))
            .with_icon(status_icon()?)
            .build().map_err(|e|e.to_string())?;
        Ok(Self{
            _tray:tray,open,quit,open_requested,quitting,language
        })
    }
    pub fn quitting(&self)->bool{self.quitting.load(Ordering::SeqCst)}
    pub fn take_open_request(&self)->bool{
        self.open_requested.swap(false,Ordering::SeqCst)
    }
    pub fn set_language(&mut self,language:Language){
        if self.language==language{return;}
        self.language=language;
        self.open.set_text(if language==Language::Zh{"打开设置"}else{"Open settings"});
        self.quit.set_text(if language==Language::Zh{"退出 kvm"}else{"Quit kvm"});
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn close_only_hides_with_a_live_tray(){
        assert!(should_hide_close(true,false));
        assert!(!should_hide_close(false,false));
        assert!(!should_hide_close(true,true));
    }
    #[test]
    fn tray_icon_is_valid_rgba(){
        let icon=status_icon();
        assert!(icon.is_ok(),"Windows tray icon must have valid dimensions and bytes");
    }
}
