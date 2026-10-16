mod model;
mod wire;
mod input;
mod engine;

use eframe::egui;
use model::{Layout,Screen,Settings,Side};
use std::{sync::{Arc,atomic::Ordering}, time::Duration};

struct KvmApp {
    settings:Settings,
    state:Arc<engine::Shared>,
}
fn generate_secret()->String {
    use rand::RngCore;
    let mut buf=[0_u8;16]; rand::thread_rng().fill_bytes(&mut buf);
    buf.iter().map(|b|format!("{b:02x}")).collect()
}
impl KvmApp {
    fn new(mut s:Settings)->Self {
        if s.secret.len()!=32 {s.secret=generate_secret();}
        let state=engine::Shared::new(&s);
        Self{settings:s,state}
    }
    fn save(&self){model::save(&self.settings);}
    fn layout_ui(&mut self, ui:&mut egui::Ui) {
        ui.label("Drag the blue screen to change layout (L / R / Top / Bottom)");
        let (rect,_) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(530.0),285.0),egui::Sense::hover());
        let local=self.state.local_screen;
        let remote=*self.state.remote_screen.lock().unwrap();
        let scale=(135.0/local.h.max(remote.h) as f32).min(0.14);
        let base=egui::Rect::from_center_size(rect.center(),egui::vec2(local.w as f32*scale,local.h as f32*scale));
        let (ox,oy)=self.settings.layout.origin(local,remote);
        let peer=egui::Rect::from_min_size(base.min + egui::vec2(ox as f32*scale,oy as f32*scale),
            egui::vec2(remote.w as f32*scale,remote.h as f32*scale));
        let painter=ui.painter();
        painter.rect_filled(base,7.0,egui::Color32::from_rgb(95,104,119));
        painter.text(base.center(),egui::Align2::CENTER_CENTER,"This PC",egui::FontId::proportional(14.0),egui::Color32::WHITE);
        painter.rect_filled(peer,7.0,egui::Color32::from_rgb(49,118,216));
        painter.text(peer.center(),egui::Align2::CENTER_CENTER,"Other PC",egui::FontId::proportional(14.0),egui::Color32::WHITE);
        let response=ui.interact(peer,ui.id().with("peer-drag"),egui::Sense::drag());
        if response.dragged() {
            let center=peer.center()+response.drag_delta();
            let diff=center-base.center();
            let side=if diff.x.abs()>diff.y.abs(){if diff.x>=0.0 {Side::Right}else{Side::Left}}
                     else {if diff.y>=0.0 {Side::Bottom}else{Side::Top}};
            let offset=if matches!(side,Side::Left|Side::Right) { ((center.y-base.min.y-peer.height()/2.0)/scale).round() as i32 }
                       else { ((center.x-base.min.x-peer.width()/2.0)/scale).round() as i32 };
            self.settings.layout=Layout{side,offset}.clamp(local,remote);
            *self.state.layout.lock().unwrap()=self.settings.layout;
        }
        if response.drag_stopped(){self.save();}
        ui.horizontal(|ui|{
            ui.label(format!("Side: {:?}",self.settings.layout.side));
            ui.label(format!("Offset: {} px",self.settings.layout.offset));
            if ui.button("Reset layout").clicked(){
                self.settings.layout=Layout::default();
                *self.state.layout.lock().unwrap()=self.settings.layout;
                self.save();
            }
        });
    }
}
impl eframe::App for KvmApp {
    fn update(&mut self, ctx:&egui::Context,_frame:&mut eframe::Frame){
        egui::CentralPanel::default().show(ctx,|ui|{
            ui.heading("kvm 0.3.1 · Rust");
            ui.label("Windows <-> macOS software KVM · experimental source edition");
            ui.separator();
            let started=self.state.started.load(Ordering::SeqCst);
            ui.add_enabled_ui(!started,|ui|{
                ui.horizontal(|ui|{
                    ui.radio_value(&mut self.settings.role_server,true,"This PC receives mouse/keyboard");
                    ui.radio_value(&mut self.settings.role_server,false,"This PC controls the other");
                });
                if self.settings.role_server {
                    ui.label("Pairing key (copy this into the controlling PC):");
                    ui.horizontal(|ui|{
                        ui.monospace(&self.settings.secret);
                        if ui.button("Copy key").clicked(){ui.ctx().copy_text(self.settings.secret.clone());}
                        if ui.button("Regenerate").clicked(){self.settings.secret=generate_secret();}
                    });
                    ui.label(format!("Listen port: {} · same LAN required",wire::PORT));
                }else{
                    ui.horizontal(|ui|{
                        ui.label("Other PC address:");
                        ui.text_edit_singleline(&mut self.settings.address);
                    });
                    ui.horizontal(|ui|{
                        ui.label("Pairing key:");
                        ui.add(egui::TextEdit::singleline(&mut self.settings.secret).password(true));
                    });
                }
            });
            if ui.checkbox(&mut self.settings.remap,"Translate Ctrl (Windows) <-> Command (Mac)").changed(){
                self.state.remap.store(self.settings.remap,Ordering::SeqCst); self.save();
            }
            if ui.checkbox(&mut self.settings.clipboard,"Clipboard on demand at paste (text only)").changed(){
                self.state.clipboard.store(self.settings.clipboard,Ordering::SeqCst); self.save();
            }
            ui.separator();
            if !self.settings.role_server {self.layout_ui(ui);} else {
                ui.label("Keep this window running. macOS needs Accessibility + Input Monitoring permissions.");
            }
            ui.separator();
            if ui.add_enabled(!started,egui::Button::new(if self.settings.role_server{"Start receiver"}else{"Connect and share"})).clicked(){
                self.save();
                if self.settings.role_server {engine::start_server(self.state.clone(),self.settings.clone());}
                else {engine::start_client(self.state.clone(),self.settings.clone());}
            }
            let status=self.state.status.lock().unwrap().clone();
            ui.label(format!("Status: {status}"));
            if self.state.active.load(Ordering::Relaxed){ui.colored_label(egui::Color32::LIGHT_GREEN,"Remote control active · F12 emergency return");}
            ui.small("One controller -> one receiver · clipboard isn't read until paste · no auto-discovery yet");
        });
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}
fn main()->eframe::Result<()> {
    // GUI/main thread belongs to the native window run loop; blocking input hooks run separately.
    eframe::run_native("kvm",eframe::NativeOptions::default(),
        Box::new(|_cc|Ok(Box::new(KvmApp::new(model::load())))))
}
