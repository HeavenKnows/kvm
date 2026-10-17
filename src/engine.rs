use crate::{clipboard,display,input,native_cursor,native_motion, model::{self,Display,Language,Layout,Platform,Screen,Settings}, status::UiStatus, wire::{self,Wire}};
use rdev::{self,EventType,Key};
use std::{collections::{HashSet,VecDeque},net::{TcpListener,TcpStream},sync::{Arc,Mutex,atomic::{AtomicBool,AtomicI32,AtomicU32,AtomicU64,Ordering},mpsc::{self,SyncSender}},time::{Duration,Instant,SystemTime,UNIX_EPOCH}};

#[derive(Clone,Copy,PartialEq,Eq,Debug)]
enum Owner { Local, Remote }
#[derive(Debug,PartialEq,Eq)]
enum PasteAction { Native, SendLocalClipboard, RequestRemoteClipboard }
fn paste_action(remote:bool, owner:Owner, enabled:bool)->PasteAction {
    if !enabled {return PasteAction::Native;}
    match (remote,owner){
        (true,Owner::Local)=>PasteAction::SendLocalClipboard,
        (false,Owner::Remote)=>PasteAction::RequestRemoteClipboard,
        _=>PasteAction::Native,
    }
}

/// AppKit pasteboard operations must run on the macOS GUI thread; sending
/// requests here never holds any display or session lock while awaiting them.
#[cfg(target_os="macos")]
pub enum ClipboardUiJob {
    Snapshot(mpsc::SyncSender<Result<Vec<u8>,String>>),
    Install(Vec<u8>,mpsc::SyncSender<Result<(),String>>),
}
#[derive(Default)]
struct WheelScale {x:f32,y:f32}
impl WheelScale {
    fn scale(&mut self,raw_x:i32,raw_y:i32,gain:f32)->(i32,i32) {
        let gain=if gain.is_finite(){gain.clamp(1.0,10.0)}else{3.0};
        fn axis(raw:i32,gain:f32,frac:&mut f32)->i32 {
            let value=raw.clamp(-2400,2400) as f32 * gain/120.0 + *frac;
            let lines=value.trunc() as i32;
            *frac=value-lines as f32;
            lines
        }
        (axis(raw_x,gain,&mut self.x),axis(raw_y,gain,&mut self.y))
    }
}
#[derive(Default)]
struct MotionScale {x:f32,y:f32}
impl MotionScale{
    fn scale(&mut self,dx:i32,dy:i32,gain:f32)->(i32,i32){
        let gain=if gain.is_finite(){gain.clamp(0.5,4.0)}else{1.6};
        let vx=dx.clamp(-2000,2000) as f32*gain+self.x;
        let vy=dy.clamp(-2000,2000) as f32*gain+self.y;
        let x=vx.trunc() as i32;let y=vy.trunc() as i32;
        self.x=vx-x as f32;self.y=vy-y as f32;
        (x,y)
    }
}
pub struct Shared {
    is_receiver:AtomicBool,
    pub status:Mutex<UiStatus>,
    pub history:Mutex<VecDeque<String>>,
    fatal_error:Mutex<Option<UiStatus>>,
    pub layout:Mutex<Layout>,
    local_desktop:Mutex<display::Desktop>,
    pub remote_screen:Mutex<Screen>,
    pub remote_displays:Mutex<Vec<Display>>,
    pub local_display_index:std::sync::atomic::AtomicUsize,
    pub remote_display_index:std::sync::atomic::AtomicUsize,
    pub target:Mutex<Platform>,
    pub active:AtomicBool,
    pending_enter:AtomicBool,
    pending_enter_since:Mutex<Option<std::time::Instant>>,
    pub native_motion_packets:std::sync::atomic::AtomicU64,
    pub mouse_down_captured:std::sync::atomic::AtomicU64,
    pub mouse_up_captured:std::sync::atomic::AtomicU64,
    pub mouse_down_received:std::sync::atomic::AtomicU64,
    pub mouse_up_received:std::sync::atomic::AtomicU64,
    pub mouse_injection_failures:std::sync::atomic::AtomicU64,
    pub motion_gain:AtomicU32,
    motion_fraction:Mutex<MotionScale>,
    pub scroll_gain:AtomicU32,
    scroll_fraction:Mutex<WheelScale>,
    pub wheel_packets:AtomicU64,
    pub side_button_packets:AtomicU64,
    pub rtt_peak_ms:AtomicU64,
    pub rtt_ms:AtomicU64,
    clock:Instant,
    pub ready:AtomicBool,
    pub started:AtomicBool,
    stop_requested:AtomicBool,
    socket:Mutex<Option<TcpStream>>,
    capture_started:AtomicBool,
    native_started:AtomicBool,
    paste_sender:Mutex<Option<SyncSender<()>>>,
    #[cfg(target_os="macos")]
    clipboard_ui_tx:Mutex<Option<mpsc::Sender<ClipboardUiJob>>>,
    #[cfg(target_os="macos")]
    clipboard_ui_ctx:Mutex<Option<eframe::egui::Context>>,
    pub clipboard:AtomicBool,
    pub remap:AtomicBool,
    pub side_buttons_native:AtomicBool,
    owner:Mutex<Owner>,
    pending_dx:AtomicI32,
    pending_dy:AtomicI32,
    entry_local:Mutex<(i32,i32)>,
    out:Mutex<Option<SyncSender<Wire>>>,
}
impl Shared {
    pub fn new(s:&Settings)->Arc<Self>{
        // No CoreGraphics/monitor enumeration here: KvmApp::new runs on
        // the native GUI thread and must be nonblocking with external screens.
        let desktop=display::Desktop::fallback();
        let (w,h)=(desktop.screen().w,desktop.screen().h);
        Arc::new(Self {
            is_receiver:AtomicBool::new(s.role_server),
            status:Mutex::new(UiStatus::Idle),
            history:Mutex::new(VecDeque::with_capacity(32)),fatal_error:Mutex::new(None),
            layout:Mutex::new(s.layout),
            local_desktop:Mutex::new(desktop),
            remote_screen:Mutex::new(Screen::new(1920,1080)),
            remote_displays:Mutex::new(model::single_display(Screen::new(1920,1080))),
            local_display_index:std::sync::atomic::AtomicUsize::new(s.local_display),
            remote_display_index:std::sync::atomic::AtomicUsize::new(s.remote_display),
            target:Mutex::new(Platform::Mac),active:AtomicBool::new(false),
            pending_enter:AtomicBool::new(false),pending_enter_since:Mutex::new(None),
            native_motion_packets:std::sync::atomic::AtomicU64::new(0),
            mouse_down_captured:std::sync::atomic::AtomicU64::new(0),
            mouse_up_captured:std::sync::atomic::AtomicU64::new(0),
            mouse_down_received:std::sync::atomic::AtomicU64::new(0),
            mouse_up_received:std::sync::atomic::AtomicU64::new(0),
            mouse_injection_failures:std::sync::atomic::AtomicU64::new(0),
            motion_gain:AtomicU32::new(s.mouse_speed.to_bits()),
            motion_fraction:Mutex::new(MotionScale::default()),
            scroll_gain:AtomicU32::new(s.scroll_speed.to_bits()),
            scroll_fraction:Mutex::new(WheelScale::default()),
            wheel_packets:AtomicU64::new(0),side_button_packets:AtomicU64::new(0),
            rtt_ms:AtomicU64::new(u64::MAX),rtt_peak_ms:AtomicU64::new(0),
            clock:Instant::now(),
            ready:AtomicBool::new(false),started:AtomicBool::new(false),
            capture_started:AtomicBool::new(false),paste_sender:Mutex::new(None),
            #[cfg(target_os="macos")]
            clipboard_ui_tx:Mutex::new(None),
            #[cfg(target_os="macos")]
            clipboard_ui_ctx:Mutex::new(None),
            stop_requested:AtomicBool::new(false),socket:Mutex::new(None),
            native_started:AtomicBool::new(false),
            clipboard:AtomicBool::new(s.clipboard),remap:AtomicBool::new(s.remap),
            side_buttons_native:AtomicBool::new(s.side_buttons_native),
            owner:Mutex::new(Owner::Local),
            pending_dx:AtomicI32::new(0), pending_dy:AtomicI32::new(0),
            entry_local:Mutex::new((w/2,h/2)),out:Mutex::new(None),
        })
    }
    #[cfg(target_os="macos")]
    pub fn set_clipboard_ui_sender(&self,tx:mpsc::Sender<ClipboardUiJob>){
        *self.clipboard_ui_tx.lock().unwrap()=Some(tx);
    }
    #[cfg(target_os="macos")]
    pub fn set_clipboard_context(&self,ctx:eframe::egui::Context){
        *self.clipboard_ui_ctx.lock().unwrap()=Some(ctx);
    }
    #[cfg(target_os="macos")]
    fn clipboard_gui(&self,job:ClipboardUiJob)->Result<(),String>{
        let tx=self.clipboard_ui_tx.lock().unwrap().clone()
            .ok_or("Clipboard GUI channel is not initialized")?;
        tx.send(job).map_err(|_|"Clipboard GUI channel closed".to_string())?;
        if let Some(ctx)=self.clipboard_ui_ctx.lock().unwrap().as_ref(){
            ctx.request_repaint();
        }
        Ok(())
    }
    fn read_clipboard(&self)->Result<Vec<u8>,String>{
        #[cfg(target_os="macos")]
        {
            let (tx,rx)=mpsc::sync_channel(1);
            self.clipboard_gui(ClipboardUiJob::Snapshot(tx))?;
            return rx.recv_timeout(Duration::from_secs(10))
                .map_err(|e|format!("Clipboard GUI read timed out: {e}"))?;
        }
        #[cfg(not(target_os="macos"))]
        {clipboard::snapshot()}
    }
    fn write_clipboard(&self,data:Vec<u8>)->Result<(),String>{
        #[cfg(target_os="macos")]
        {
            let (tx,rx)=mpsc::sync_channel(1);
            self.clipboard_gui(ClipboardUiJob::Install(data,tx))?;
            return rx.recv_timeout(Duration::from_secs(10))
                .map_err(|e|format!("Clipboard GUI write timed out: {e}"))?;
        }
        #[cfg(not(target_os="macos"))]
        {clipboard::install(&data)}
    }
    pub fn local_desktop(&self)->display::Desktop{
        self.local_desktop.lock().unwrap().clone()
    }
    pub fn local_screen(&self)->Screen{self.local_desktop.lock().unwrap().screen()}
    pub fn local_displays(&self)->Vec<Display>{
        self.local_desktop.lock().unwrap().displays.clone()
    }
    pub fn virtual_origin(&self)->(i32,i32){
        let d=self.local_desktop.lock().unwrap();
        (d.origin_x,d.origin_y)
    }
    /// Apply a background monitor scan before a connection starts.
    /// The session's Hello geometry must not change while a peer is active.
    pub fn update_local_desktop(&self,desktop:display::Desktop)->bool{
        if self.started.load(Ordering::SeqCst){return false;}
        let (w,h)=(desktop.screen().w,desktop.screen().h);
        *self.local_desktop.lock().unwrap()=desktop;
        *self.entry_local.lock().unwrap()=(w/2,h/2);
        true
    }
    pub fn info(&self,status:UiStatus){
        let millis=SystemTime::now().duration_since(UNIX_EPOCH)
            .unwrap_or_default().as_millis();
        let detail=format!("[{millis}] {}",status.text(Language::En));
        *self.status.lock().unwrap()=status;
        let mut h=self.history.lock().unwrap();
        if h.back()!=Some(&detail) {
            h.push_back(detail);
            if h.len()>32{h.pop_front();}
        }
    }
    fn fatal(&self,status:UiStatus) {
        *self.fatal_error.lock().unwrap()=Some(status.clone());
        self.info(status);
    }
    fn transport_reason(fault:&Arc<Mutex<Option<String>>>) -> String {
        fault.lock().unwrap().clone().unwrap_or_else(||"peer closed the connection".into())
    }
    /// Called by native Windows Raw Input / macOS CGEventTap callbacks.
    /// Synthetic pointer warps no longer contribute to movement; native physical
    /// deltas continue arriving even when the OS pointer is at a screen boundary.
    pub fn add_native_motion(&self,dx:i32,dy:i32) {
        if !self.ready.load(Ordering::Relaxed) ||
           !self.active.load(Ordering::Relaxed) { return; }
        self.native_motion_packets.fetch_add(1,Ordering::Relaxed);
        let gain=f32::from_bits(self.motion_gain.load(Ordering::Relaxed));
        let (dx,dy)=self.motion_fraction.lock().unwrap().scale(dx,dy,gain);
        self.pending_dx.fetch_add(dx,Ordering::Relaxed);
        self.pending_dy.fetch_add(dy,Ordering::Relaxed);
    }
    #[cfg(target_os="windows")]
    pub fn add_native_wheel(&self,raw_x:i32,raw_y:i32){
        if !self.ready.load(Ordering::Relaxed) ||
           !self.active.load(Ordering::Relaxed){return;}
        self.wheel_packets.fetch_add(1,Ordering::Relaxed);
        let gain=f32::from_bits(self.scroll_gain.load(Ordering::Relaxed));
        let (dx,dy)=self.scroll_fraction.lock().unwrap().scale(raw_x,raw_y,gain);
        if dx!=0||dy!=0 {self.send(Wire::Scroll{dx,dy});}
    }
    #[cfg(target_os="windows")]
    pub fn add_native_button(&self,button:rdev::Button,pressed:bool){
        if !self.ready.load(Ordering::Relaxed) ||
           !self.active.load(Ordering::Relaxed) {return;}
        if matches!(button,rdev::Button::Unknown(3)|rdev::Button::Unknown(4)){
            self.side_button_packets.fetch_add(1,Ordering::Relaxed);
        }
        if pressed {
            self.mouse_down_captured.fetch_add(1,Ordering::Relaxed);
            self.send(Wire::Input(EventType::ButtonPress(button)));
        }else{
            self.mouse_up_captured.fetch_add(1,Ordering::Relaxed);
            self.send(Wire::Input(EventType::ButtonRelease(button)));
        }
    }
    pub fn set_layout_targets(&self,local:usize,remote:usize){
        self.local_display_index.store(local,Ordering::Relaxed);
        self.remote_display_index.store(remote,Ordering::Relaxed);
    }
    pub fn set_mouse_speed(&self,gain:f32){
        self.motion_gain.store(gain.clamp(0.5,4.0).to_bits(),Ordering::Relaxed);
        *self.motion_fraction.lock().unwrap()=MotionScale::default();
    }
    pub fn set_scroll_speed(&self,gain:f32){
        self.scroll_gain.store(gain.clamp(1.0,10.0).to_bits(),Ordering::Relaxed);
        *self.scroll_fraction.lock().unwrap()=WheelScale::default();
    }
    pub fn native_motion_stopped(&self,error:String){
        self.native_started.store(false,Ordering::SeqCst);
        self.shutdown_peer();
        self.fatal(UiStatus::InputHookFailed(error));
    }
    fn store_socket(&self,sock:&TcpStream)->std::io::Result<()> {
        *self.socket.lock().unwrap()=Some(sock.try_clone()?);
        Ok(())
    }
    fn shutdown_peer(&self) {
        self.active.store(false,Ordering::SeqCst);
        self.pending_enter.store(false,Ordering::SeqCst);
        *self.pending_enter_since.lock().unwrap()=None;
        self.ready.store(false,Ordering::SeqCst);
        self.rtt_ms.store(u64::MAX,Ordering::Relaxed);
        self.rtt_peak_ms.store(0,Ordering::Relaxed);
        if let Some(sock)=self.socket.lock().unwrap().take(){
            let _=sock.shutdown(std::net::Shutdown::Both);
        }
    }
    /// User-initiated disconnect; receiver keeps listening for the next client.
    pub fn disconnect_peer(&self){
        self.pending_enter.store(false,Ordering::SeqCst);
        let was_remote=self.active.swap(false,Ordering::SeqCst);
        if was_remote && !self.is_receiver.load(Ordering::SeqCst) { self.return_cursor(); }
        self.shutdown_peer();
        self.info(UiStatus::UserDisconnected);
    }
    /// Stop listening or abort connecting. The worker clears started on exit.
    pub fn stop(&self){
        self.stop_requested.store(true,Ordering::SeqCst);
        self.disconnect_peer();
    }
    fn flush_motion(&self){
        let dx=self.pending_dx.swap(0,Ordering::Relaxed);
        let dy=self.pending_dy.swap(0,Ordering::Relaxed);
        if dx==0 && dy==0 {return;}
        if let Some(tx)=self.out.lock().unwrap().as_ref(){
            if tx.try_send(Wire::MoveBy{dx,dy}).is_err(){
                self.active.store(false,Ordering::SeqCst);
                self.info(UiStatus::MotionOverflow);
            }
        }
    }
    fn send(&self,msg:Wire){
        // Preserve mouse->click / mouse->key order; coalesce motion between 2ms ticks.
        self.flush_motion();
        if let Some(tx)=self.out.lock().unwrap().as_ref(){
            if tx.try_send(msg).is_err(){
                self.active.store(false,Ordering::SeqCst);
                self.info(UiStatus::QueueOverflow);
            }
        }
    }
    fn return_cursor(&self){
        let (mut x,mut y)=*self.entry_local.lock().unwrap();
        match self.layout.lock().unwrap().side {
            crate::model::Side::Right=>x=self.local_screen().w-75,
            crate::model::Side::Left=>x=75,
            crate::model::Side::Top=>y=75,
            crate::model::Side::Bottom=>y=self.local_screen().h-75,
        }
        let (ox,oy)=self.virtual_origin();
        simulate(EventType::MouseMove{x:(x+ox) as f64,y:(y+oy) as f64});
    }
    fn close(&self){
        self.active.store(false,Ordering::SeqCst);
        self.ready.store(false,Ordering::SeqCst);
        self.shutdown_peer();
        self.out.lock().unwrap().take();
        self.paste_sender.lock().unwrap().take();
        *self.owner.lock().unwrap() = Owner::Local;
        self.pending_dx.store(0,Ordering::Relaxed);
        self.pending_dy.store(0,Ordering::Relaxed);
        *self.motion_fraction.lock().unwrap()=MotionScale::default();
        *self.scroll_fraction.lock().unwrap()=WheelScale::default();
    }
}
fn simulate(event:EventType){let _=rdev::simulate(&event);}
fn inject_key(key:Key,down:bool,held:&HashSet<Key>)->Result<(),String>{
    #[cfg(target_os="macos")]
    {return crate::native_keyboard::inject(key,down,held);}
    #[cfg(not(target_os="macos"))]
    {
        let _=held;
        let event=if down{EventType::KeyPress(key)}else{EventType::KeyRelease(key)};
        rdev::simulate(&event).map_err(|e|format!("Keyboard injection failed: {e:?}"))
    }
}
fn release_keys(held:&mut HashSet<Key>){
    // Drop modifiers one-by-one with updated flags; never leave Command stuck.
    for key in held.iter().copied().collect::<Vec<_>>(){
        held.remove(&key);
        let _=inject_key(key,false,held);
    }
}
fn paste_hotkey(os:Platform){
    #[cfg(target_os="macos")]
    if os==Platform::Mac {
        let _=crate::native_keyboard::paste();
        return;
    }
    let primary=match os {Platform::Windows=>Key::ControlLeft,Platform::Mac=>Key::MetaLeft};
    simulate(EventType::KeyPress(primary));
    simulate(EventType::KeyPress(Key::KeyV));
    simulate(EventType::KeyRelease(Key::KeyV));
    simulate(EventType::KeyRelease(primary));
}

fn clipboard_send_packet(shared:&Shared,mut packet:Wire)->bool {
    // File/image chunks are sent only by workers; leave room for input and
    // mouse events instead of filling the 2048-frame network queue.
    let tx=match shared.out.lock().unwrap().clone(){
        Some(tx)=>tx,None=>return false
    };
    for _ in 0..100 {
        if !shared.ready.load(Ordering::Relaxed){return false;}
        match tx.try_send(packet) {
            Ok(())=>return true,
            Err(mpsc::TrySendError::Full(value))=>{
                packet=value;
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(mpsc::TrySendError::Disconnected(_))=>return false,
        }
    }
    shared.info(UiStatus::QueueOverflow);
    false
}
fn send_clipboard_snapshot(shared:&Shared){
    let bytes=match shared.read_clipboard(){
        Ok(data)=>data,
        Err(_)=>{shared.info(UiStatus::ClipboardUnavailable);return;}
    };
    if !clipboard_send_packet(shared,Wire::ClipboardBegin{bytes:bytes.len() as u32}){return;}
    for chunk in bytes.chunks(clipboard::CHUNK_SIZE){
        if !clipboard_send_packet(shared,Wire::ClipboardChunk(chunk.to_vec())){return;}
        // Do not saturate the same TCP stream carrying real-time cursor input.
        std::thread::sleep(Duration::from_millis(4));
    }
    let _=clipboard_send_packet(shared,Wire::ClipboardEnd);
}
fn start_clipboard_inbound(shared:Arc<Shared>)->mpsc::Sender<clipboard::Incoming>{
    let (tx,rx)=mpsc::channel();
    std::thread::spawn(move || clipboard::receive_loop(rx,|bytes|{
        if !shared.ready.load(Ordering::SeqCst){return;}
        // A slow file transfer must not paste into the wrong machine after
        // the user has switched sides.
        let receiver=shared.is_receiver.load(Ordering::SeqCst);
        if shared.active.load(Ordering::SeqCst)!=receiver {return;}
        match shared.write_clipboard(bytes){
            Ok(())=>{
                if !receiver {*shared.owner.lock().unwrap()=Owner::Local;}
                // Allow Finder/Explorer to observe the new pasteboard flavor.
                std::thread::sleep(Duration::from_millis(30));
                if shared.ready.load(Ordering::SeqCst)
                    && shared.active.load(Ordering::SeqCst)==receiver {
                    paste_hotkey(Platform::local());
                }
            }
            Err(_)=>shared.info(UiStatus::ClipboardInvalid),
        }
    }));
    tx
}

// The receiver keeps listening after a client disconnects. Only one client is
// handled at a time, preserving the existing single-controller design.
pub fn start_server(shared:Arc<Shared>, s:Settings){
    if shared.started.swap(true,Ordering::SeqCst){return;}
    shared.is_receiver.store(true,Ordering::SeqCst);
    shared.stop_requested.store(false,Ordering::SeqCst);
    std::thread::spawn(move || {
        let listener=match TcpListener::bind(("0.0.0.0",s.listen_port)) {
            Ok(v)=>v,
            Err(e)=>{
                shared.info(UiStatus::BindFailed(e.to_string()));
                shared.started.store(false,Ordering::SeqCst);
                return;
            }
        };
        if let Err(e)=listener.set_nonblocking(true){
            shared.info(UiStatus::BindFailed(e.to_string()));
            shared.started.store(false,Ordering::SeqCst);
            return;
        }
        // Do NOT use a receiver-side global mouse listener to detect return
        // edges: injected mouse events are seen by the listener as well,
        // producing erroneous "Back" messages and snap-back at x=0.
        // Return via controller F12 or explicit Disconnect instead.
        shared.info(UiStatus::Waiting);
        while !shared.stop_requested.load(Ordering::SeqCst) {
            let (stream,peer)=match listener.accept(){
                Ok(v)=>v,
                Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>{
                    std::thread::sleep(Duration::from_millis(60));
                    continue;
                }
                Err(e)=>{
                    shared.info(UiStatus::AcceptFailed(e.to_string()));
                    break;
                }
            };
            if shared.stop_requested.load(Ordering::SeqCst){break;}
            // The listener is nonblocking so Stop receiver can release its
            // port. The accepted *connection* must be blocking for the
            // encrypted framed protocol (macOS otherwise returns EAGAIN).
            if let Err(e)=stream.set_nonblocking(false){
                shared.info(UiStatus::AcceptFailed(format!("restore blocking connection: {e}")));
                continue;
            }
            if let Err(e)=shared.store_socket(&stream){
                shared.info(UiStatus::AcceptFailed(e.to_string()));
                continue;
            }
            shared.info(UiStatus::Authenticating(peer.to_string()));
            let (tx,rx,_control,fault)=match wire::connect(stream,&s.secret,false){
                Ok(v)=>v,
                Err(e)=>{
                    shared.shutdown_peer();
                    if shared.stop_requested.load(Ordering::SeqCst){break;}
                    shared.info(UiStatus::PairingFailed(e.to_string()));
                    continue;
                }
            };
            if shared.stop_requested.load(Ordering::SeqCst){
                shared.shutdown_peer();
                break;
            }
            *shared.out.lock().unwrap()=Some(tx);
            shared.ready.store(true,Ordering::SeqCst);
            shared.info(UiStatus::ConnectedPeer(peer.to_string()));
            shared.send(Wire::Hello{os:Platform::local(),screen:shared.local_screen(),displays:shared.local_displays().clone()});
            let receiver_displays=shared.local_displays();
            let mut mouse=(shared.local_screen().w/2,shared.local_screen().h/2);
            let mut pressed=HashSet::<Key>::new();
            let mut buttons=HashSet::<rdev::Button>::new();
            let mut return_display=receiver_displays[0];
            let incoming_clipboard=start_clipboard_inbound(shared.clone());
            let (clipboard_request_tx,clipboard_request_rx)=mpsc::sync_channel::<()>(4);
            let clipboard_sender=shared.clone();
            std::thread::spawn(move||{
                while clipboard_request_rx.recv().is_ok(){
                    if !clipboard_sender.ready.load(Ordering::Relaxed){break;}
                    send_clipboard_snapshot(&clipboard_sender);
                }
            });
            let mut buffered=None::<Wire>;
            let mut armed_return=false;
            let mut first_motion_seen=false;
            let mut entered_at=None::<std::time::Instant>;
            loop {
                if shared.stop_requested.load(Ordering::SeqCst){break;}
                let msg=match buffered.take(){
                    Some(msg)=>msg,
                    None=>match rx.recv_timeout(Duration::from_millis(80)){
                    Ok(msg)=>msg,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout)=>{
                        // Fail open: never strand the controller on its screen
                        // boundary if the receiver does not get MoveBy messages.
                        if shared.active.load(Ordering::Relaxed) && !first_motion_seen
                            && entered_at.is_some_and(|t|t.elapsed()>Duration::from_secs(3)){
                            shared.active.store(false,Ordering::SeqCst);
                            shared.send(Wire::Back);
                            shared.info(UiStatus::MouseNoMovement);
                            entered_at=None;
                        }
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected)=>break,
                    }
                };
                // Merge only consecutive motion frames. A key/button/clipboard
                // frame is an ordering barrier, so drags keep their semantics.
                let msg=if let Wire::MoveBy{mut dx,mut dy}=msg{
                    for _ in 0..64{
                        match rx.try_recv(){
                            Ok(Wire::MoveBy{dx:x,dy:y})=>{
                                dx=dx.saturating_add(x);
                                dy=dy.saturating_add(y);
                            }
                            Ok(other)=>{buffered=Some(other);break;}
                            Err(_)=>break,
                        }
                    }
                    Wire::MoveBy{dx,dy}
                }else{msg};
                match msg {
                    Wire::Enter{x,y,side,remote_display}=>{
                        let mut l=*shared.layout.lock().unwrap();l.side=side;
                        *shared.layout.lock().unwrap()=l;
                        return_display=receiver_displays.get(remote_display)
                            .copied().unwrap_or(receiver_displays[0]);
                        if !return_display.contains(x,y) {
                            shared.send(Wire::EnterAck{accepted:false});
                            continue;
                        }
                        mouse=return_display.closest(x,y);
                        // CGWarpMouseCursorPosition moves the *visible* macOS
                        // cursor. Its coordinates must be in logical points.
                        match native_cursor::move_and_notify(mouse.0,mouse.1){
                            Ok(())=>{
                                shared.active.store(true,Ordering::SeqCst);
                                armed_return=false;
                                first_motion_seen=false;
                                entered_at=Some(std::time::Instant::now());
                                shared.info(UiStatus::RemoteCursorPlaced);
                                shared.send(Wire::EnterAck{accepted:true});
                            }
                            Err(e)=>{
                                shared.active.store(false,Ordering::SeqCst);
                                shared.info(UiStatus::InputHookFailed(e));
                                shared.send(Wire::EnterAck{accepted:false});
                            }
                        }
                    }
                    Wire::Back=>{
                        shared.active.store(false,Ordering::SeqCst);
                        entered_at=None;
                        release_keys(&mut pressed);
                        for b in buttons.drain(){
                            if let Err(e)=native_cursor::button_event(b,false,mouse.0,mouse.1){
                                shared.mouse_injection_failures.fetch_add(1,Ordering::Relaxed);
                                shared.info(UiStatus::InputHookFailed(e));
                            }
                        }
                    }
                    Wire::MoveBy{dx,dy} if shared.active.load(Ordering::Relaxed)=>{
                        if dx==0 && dy==0{continue;}
                        if !first_motion_seen {shared.info(UiStatus::RemoteMouseMoving);}
                        first_motion_seen=true;
                        entered_at=None;
                        let candidate=(mouse.0.saturating_add(dx),mouse.1.saturating_add(dy));
                        mouse=model::closest_on_displays(&receiver_displays,candidate.0,candidate.1);
                        // A pressed mouse button requires Cocoa dragged events.
                        // Ordinary MouseMoved / cursor warps only update the
                        // pointer; macOS can defer window movement until mouse-up.
                        let held=if buttons.contains(&rdev::Button::Left){
                            Some(rdev::Button::Left)
                        }else if buttons.contains(&rdev::Button::Right){
                            Some(rdev::Button::Right)
                        }else if buttons.contains(&rdev::Button::Middle){
                            Some(rdev::Button::Middle)
                        }else{None};
                        let moved=if let Some(button)=held{
                            native_cursor::drag_to(mouse.0,mouse.1,button)
                        }else{native_cursor::move_and_notify(mouse.0,mouse.1)};
                        if let Err(e)=moved{
                            shared.active.store(false,Ordering::SeqCst);
                            shared.info(UiStatus::InputHookFailed(e));
                            shared.send(Wire::Back);
                            continue;
                        }
                        // Safe edge-return: derived from the *network cursor
                        // model*, never the injected event observer. Require
                        // moving inward first so entry at x=3 cannot return.
                        let layout=*shared.layout.lock().unwrap();
                        if !buttons.is_empty(){continue;} // Never return during a drag.
                        let relative=(mouse.0-return_display.x,mouse.1-return_display.y);
                        let size=return_display.screen();
                        if return_display.contains(mouse.0,mouse.1)
                            && !armed_return && layout.inward(relative.0,relative.1,size){
                            armed_return=true;
                        }else if armed_return && return_display.contains(mouse.0,mouse.1)
                            && layout.returning(relative.0,relative.1,size){
                            shared.active.store(false,Ordering::SeqCst);
                            shared.send(Wire::Back);
                            shared.info(UiStatus::Returned);
                            release_keys(&mut pressed);
                            for b in buttons.drain(){
                            if let Err(e)=native_cursor::button_event(b,false,mouse.0,mouse.1){
                                shared.mouse_injection_failures.fetch_add(1,Ordering::Relaxed);
                                shared.info(UiStatus::InputHookFailed(e));
                            }
                        }
                        }
                    }
                    Wire::Scroll{dx,dy} if shared.active.load(Ordering::Relaxed)=>{
                        if let Err(e)=native_cursor::scroll(dx,dy){
                            shared.mouse_injection_failures.fetch_add(1,Ordering::Relaxed);
                            shared.info(UiStatus::InputHookFailed(e));
                        }
                    }
                    Wire::Input(evt) if shared.active.load(Ordering::Relaxed)=>{
                        match evt {
                            EventType::ButtonPress(b)=>{
                                shared.mouse_down_received.fetch_add(1,Ordering::Relaxed);
                                #[cfg(target_os="macos")]
                                if !shared.side_buttons_native.load(Ordering::Relaxed){
                                    let nav=match b{
                                        rdev::Button::Unknown(3)=>Some(Key::LeftBracket),
                                        rdev::Button::Unknown(4)=>Some(Key::RightBracket),
                                        _=>None,
                                    };
                                    if let Some(key)=nav{
                                        if let Err(e)=crate::native_keyboard::shortcut(key){
                                            shared.mouse_injection_failures.fetch_add(1,Ordering::Relaxed);
                                            shared.info(UiStatus::InputHookFailed(e));
                                        }
                                        continue;
                                    }
                                }
                                match native_cursor::button_event(b,true,mouse.0,mouse.1){
                                    Ok(())=>{buttons.insert(b);},
                                    Err(e)=>{
                                        shared.mouse_injection_failures.fetch_add(1,Ordering::Relaxed);
                                        shared.info(UiStatus::InputHookFailed(e));
                                    }
                                }
                            }
                            EventType::ButtonRelease(b)=>{
                                shared.mouse_up_received.fetch_add(1,Ordering::Relaxed);
                                // Navigation-mode side buttons didn't post a down.
                                if !buttons.contains(&b) {continue;}
                                if let Err(e)=native_cursor::button_event(b,false,mouse.0,mouse.1){
                                    shared.mouse_injection_failures.fetch_add(1,Ordering::Relaxed);
                                    shared.info(UiStatus::InputHookFailed(e));
                                }
                                buttons.remove(&b);
                            }
                            EventType::KeyPress(k)=>{
                                let new=pressed.insert(k);
                                #[cfg(target_os="macos")]
                                if !new && crate::native_keyboard::is_modifier(k){continue;}
                                let _=new;
                                if let Err(e)=inject_key(k,true,&pressed){
                                    shared.info(UiStatus::InputHookFailed(e));
                                }
                            }
                            EventType::KeyRelease(k)=>{
                                if pressed.remove(&k){
                                    if let Err(e)=inject_key(k,false,&pressed){
                                        shared.info(UiStatus::InputHookFailed(e));
                                    }
                                }
                            }
                            EventType::Wheel{delta_x,delta_y}=>{
                                if let Err(e)=native_cursor::scroll(
                                    delta_x.clamp(-50,50) as i32,delta_y.clamp(-50,50) as i32){
                                    shared.info(UiStatus::InputHookFailed(e));
                                }
                            }
                            other=>simulate(other),
                        }
                    }
                    Wire::ClipboardRequest if shared.clipboard.load(Ordering::Relaxed)=>{
                        let _=clipboard_request_tx.try_send(());
                    }
                    Wire::ClipboardBegin{bytes}=>{
                        let _=incoming_clipboard.send(clipboard::Incoming::Begin(bytes));
                    }
                    Wire::ClipboardChunk(bytes)=>{
                        let _=incoming_clipboard.send(clipboard::Incoming::Chunk(bytes));
                    }
                    Wire::ClipboardEnd=>{
                        let _=incoming_clipboard.send(clipboard::Incoming::End);
                    }
                    Wire::Ping(tick)=>shared.send(Wire::Pong(tick)),
                    _=>{}
                }
            }
            release_keys(&mut pressed);
            for b in buttons {
                if let Err(e)=native_cursor::button_event(b,false,mouse.0,mouse.1){
                    shared.mouse_injection_failures.fetch_add(1,Ordering::Relaxed);
                    shared.info(UiStatus::InputHookFailed(e));
                }
            }
            let detail=Shared::transport_reason(&fault);
            shared.close();
            if !shared.stop_requested.load(Ordering::SeqCst) {
                shared.info(UiStatus::PeerDisconnected(format!("{peer}: {detail}")));
                shared.info(UiStatus::Waiting);
            }
        }
        shared.close();
        shared.started.store(false,Ordering::SeqCst);
        shared.info(UiStatus::Stopped);
    });
}

pub fn start_client(shared:Arc<Shared>, s:Settings){
    if shared.started.swap(true,Ordering::SeqCst){return;}
    shared.is_receiver.store(false,Ordering::SeqCst);
    shared.stop_requested.store(false,Ordering::SeqCst);
    *shared.fatal_error.lock().unwrap()=None;
    std::thread::spawn(move ||{
        let result=client_session(&shared,s);
        shared.close();
        // An input hook may fail on another thread and close the socket.
        // Preserve its diagnostic rather than overwriting it with "Disconnected".
        let fatal=shared.fatal_error.lock().unwrap().take();
        shared.started.store(false,Ordering::SeqCst);
        if shared.stop_requested.load(Ordering::SeqCst) {
            shared.info(UiStatus::UserDisconnected);
        } else if let Some(status)=fatal {shared.info(status);}
        else if let Err(status)=result {shared.info(status);}
        else {shared.info(UiStatus::Disconnected);}
    });
}

fn client_session(shared:&Arc<Shared>,s:Settings)->Result<(),UiStatus>{
    use std::net::SocketAddr;
    use std::sync::mpsc::RecvTimeoutError;

    shared.info(UiStatus::Connecting);
    let addr:SocketAddr=s.address.parse().map_err(|_|UiStatus::InvalidAddress)?;
    let stream=TcpStream::connect_timeout(&addr,Duration::from_secs(4))
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::ConnectionRefused=>UiStatus::ConnectRefused(s.address.clone()),
            _=>UiStatus::ConnectFailed(e.to_string()),
        })?;
    shared.store_socket(&stream).map_err(|e|UiStatus::ConnectFailed(e.to_string()))?;
    if shared.stop_requested.load(Ordering::SeqCst){return Err(UiStatus::UserDisconnected);}
    let (tx,rx,_control,fault)=wire::connect(stream,&s.secret,true)
        .map_err(|e|UiStatus::PairingFailed(e.to_string()))?;
    if shared.stop_requested.load(Ordering::SeqCst){return Err(UiStatus::UserDisconnected);}
    *shared.out.lock().unwrap()=Some(tx);
    shared.info(UiStatus::AwaitingInfo);

    // Never leave the UI permanently in "Connecting" if the receiver never
    // sends its Hello packet. No grab hook is installed before this completes.
    match rx.recv_timeout(Duration::from_secs(5)){
        Ok(Wire::Hello{os,screen,displays})=>{
            *shared.target.lock().unwrap()=os;
            *shared.remote_screen.lock().unwrap()=screen;
            *shared.remote_displays.lock().unwrap()=if displays.is_empty(){
                model::single_display(screen)
            }else{displays};
        }
        Err(RecvTimeoutError::Timeout)=>return Err(UiStatus::HandshakeTimedOut),
        Err(RecvTimeoutError::Disconnected)=>return Err(UiStatus::TransportClosed(Shared::transport_reason(&fault))),
        _=>return Err(UiStatus::InvalidHandshake),
    }

    if shared.stop_requested.load(Ordering::SeqCst){return Err(UiStatus::UserDisconnected);}
    shared.info(UiStatus::NativeInputStarting);
    // Install OS-native mouse capture first, before activating the input grab.
    // Keep a single background listener across reconnects.
    if !shared.native_started.swap(true,Ordering::SeqCst){
        if let Err(e)=native_motion::start(shared.clone()){
            shared.native_started.store(false,Ordering::SeqCst);
            return Err(UiStatus::InputHookFailed(e));
        }
    }
    let (paste_tx,paste_rx)=mpsc::sync_channel::<()>(16);
    *shared.paste_sender.lock().unwrap()=Some(paste_tx);
    shared.ready.store(true,Ordering::SeqCst);
    *shared.owner.lock().unwrap()=Owner::Local;
    shared.info(UiStatus::Ready);

    // Install a single long-lived global input grab per process. Reconnects
    // reuse it; spawning a new grab per connection would compete with it.
    if !shared.capture_started.swap(true,Ordering::SeqCst){
        let grab_shared=shared.clone();
        std::thread::spawn(move || capture(grab_shared));
    }

    let work_shared=shared.clone();
    std::thread::spawn(move ||{
        while paste_rx.recv().is_ok(){
            if !work_shared.ready.load(Ordering::Relaxed){break;}
            send_clipboard_snapshot(&work_shared);
        }
    });
    let incoming_clipboard=start_clipboard_inbound(shared.clone());
    let motion_shared=shared.clone();
    std::thread::spawn(move ||{
        let mut last_probe=Instant::now()-Duration::from_secs(1);
        let mut last_motion=Instant::now();
        while motion_shared.ready.load(Ordering::Relaxed) {
            if last_probe.elapsed()>=Duration::from_secs(1){
                last_probe=Instant::now();
                motion_shared.send(Wire::Ping(motion_shared.clock.elapsed().as_millis() as u64));
            }
            if motion_shared.active.load(Ordering::Relaxed)
                && last_motion.elapsed()>=Duration::from_millis(8){
                last_motion=Instant::now();
                motion_shared.flush_motion();
            }
            if motion_shared.pending_enter.load(Ordering::Relaxed){
                let timeout=motion_shared.pending_enter_since.lock().unwrap()
                    .is_some_and(|t|t.elapsed()>Duration::from_secs(2));
                if timeout && motion_shared.pending_enter.swap(false,Ordering::SeqCst){
                    *motion_shared.pending_enter_since.lock().unwrap()=None;
                    motion_shared.info(UiStatus::MouseEntryTimedOut);
                    motion_shared.send(Wire::Back);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    });

    for msg in rx {
        if shared.stop_requested.load(Ordering::SeqCst){break;}
        match msg {
            Wire::Pong(tick)=>{
                let current=shared.clock.elapsed().as_millis() as u64;
                if current>=tick{
                    let rtt=current-tick;
                    shared.rtt_ms.store(rtt,Ordering::Relaxed);
                    shared.rtt_peak_ms.fetch_max(rtt,Ordering::Relaxed);
                }
            }
            Wire::EnterAck{accepted}=>{
                if shared.pending_enter.swap(false,Ordering::SeqCst){
                    *shared.pending_enter_since.lock().unwrap()=None;
                    if accepted {
                        shared.active.store(true,Ordering::SeqCst);
                        shared.info(UiStatus::Ready);
                    } else {
                        shared.info(UiStatus::MouseEntryRejected);
                    }
                } else if accepted {
                    // Late acknowledgement after timeout / F12: release peer.
                    shared.send(Wire::Back);
                }
            }
            Wire::Back=>{
                shared.pending_enter.store(false,Ordering::SeqCst);
                *shared.pending_enter_since.lock().unwrap()=None;
                shared.active.store(false,Ordering::SeqCst);
                shared.return_cursor();
                shared.info(UiStatus::Returned);
            }
            Wire::ClipboardBegin{bytes}=>{
                let _=incoming_clipboard.send(clipboard::Incoming::Begin(bytes));
            }
            Wire::ClipboardChunk(bytes)=>{
                let _=incoming_clipboard.send(clipboard::Incoming::Chunk(bytes));
            }
            Wire::ClipboardEnd=>{
                let _=incoming_clipboard.send(clipboard::Incoming::End);
            }
            _=>{}
        }
    }
    if shared.stop_requested.load(Ordering::SeqCst){Ok(())}
    else {Err(UiStatus::TransportClosed(Shared::transport_reason(&fault)))}
}

fn capture(shared:Arc<Shared>) {
    let pressed=Arc::new(Mutex::new(HashSet::<Key>::new()));
    let skip_v=Arc::new(AtomicBool::new(false));
    let skip_f12=Arc::new(AtomicBool::new(false));
    let p=pressed.clone(); let skip=skip_v.clone(); let skip_emergency=skip_f12.clone();
    let me=Platform::local();
    let hook_shared=shared.clone();
    if let Err(e)=rdev::grab(move |event| {
        if !hook_shared.ready.load(Ordering::Relaxed){
            p.lock().unwrap().clear();
            skip.store(false,Ordering::SeqCst);
            skip_emergency.store(false,Ordering::SeqCst);
            return Some(event);
        }
        let et=event.event_type.clone();
        let remote=hook_shared.active.load(Ordering::Relaxed);
        match et {
            EventType::KeyPress(k)=>{
                p.lock().unwrap().insert(k);
                if k==Key::F12 && (remote || hook_shared.pending_enter.swap(false,Ordering::SeqCst)) {
                    skip_emergency.store(true,Ordering::SeqCst);
                    hook_shared.active.store(false,Ordering::SeqCst);
                    *hook_shared.pending_enter_since.lock().unwrap()=None;
                    hook_shared.send(Wire::Back);
                    hook_shared.return_cursor();
                    return None;
                }
                let chord=p.lock().unwrap().iter().any(|&v|input::primary(v,me));
                if chord && (k==Key::KeyC || k==Key::KeyX){
                    *hook_shared.owner.lock().unwrap()=if remote {Owner::Remote} else {Owner::Local};
                }
                if k==Key::KeyV && chord {
                    let owner=*hook_shared.owner.lock().unwrap();
                    let action=paste_action(remote,owner,hook_shared.clipboard.load(Ordering::Relaxed));
                    match action {
                        PasteAction::SendLocalClipboard=>{
                            skip.store(true,Ordering::SeqCst);
                            if let Some(tx)=hook_shared.paste_sender.lock().unwrap().as_ref(){ let _=tx.try_send(()); }
                            return None;
                        }
                        PasteAction::RequestRemoteClipboard=>{
                            skip.store(true,Ordering::SeqCst);
                            hook_shared.send(Wire::ClipboardRequest);
                            return None;
                        }
                        PasteAction::Native=>{}
                    }
                }
            }
            EventType::KeyRelease(k)=>{
                p.lock().unwrap().remove(&k);
                if k==Key::KeyV && skip.swap(false,Ordering::SeqCst){return None;}
                if k==Key::F12 && skip_emergency.swap(false,Ordering::SeqCst){return None;}
            }
            EventType::Wheel{..} if remote && cfg!(target_os="windows") => {
                // Forwarded by the Windows Raw Input listener at full wheel
                // precision. Suppress duplicate rdev wheel callbacks.
                return None;
            }
            EventType::ButtonPress(_) if remote => {
                // Windows forwards RAWMOUSE button flags instead of duplicating
                // these rdev hook events. The hook still suppresses local input.
                #[cfg(not(target_os="windows"))]
                {
                    hook_shared.mouse_down_captured.fetch_add(1,Ordering::Relaxed);
                    hook_shared.send(Wire::Input(et));
                }
                return None;
            }
            EventType::ButtonRelease(_) if remote => {
                #[cfg(not(target_os="windows"))]
                {
                    hook_shared.mouse_up_captured.fetch_add(1,Ordering::Relaxed);
                    hook_shared.send(Wire::Input(et));
                }
                return None;
            }
            EventType::MouseMove{x,y}=>{
                if remote {
                    // Raw Input / CGEventTap sends actual device deltas
                    // independently from this absolute-position grab hook.
                    // Suppress local movement while controlling the peer.
                    return None;
                }
                let (x,y)=(x as i32-hook_shared.virtual_origin().0,
                            y as i32-hook_shared.virtual_origin().1);
                let layout=*hook_shared.layout.lock().unwrap();
                let remote=hook_shared.remote_displays.lock().unwrap();
                let local_idx=hook_shared.local_display_index.load(Ordering::Relaxed);
                let remote_idx=hook_shared.remote_display_index.load(Ordering::Relaxed);
                let entry=model::entering_multi(layout,x,y,
                    &hook_shared.local_displays(),&remote,local_idx,remote_idx);
                drop(remote);
                if !hook_shared.pending_enter.load(Ordering::Relaxed){
                    if let Some((rx,ry))=entry{
                        *hook_shared.entry_local.lock().unwrap()=(x,y);
                        hook_shared.pending_enter.store(true,Ordering::SeqCst);
                        *hook_shared.pending_enter_since.lock().unwrap()=Some(std::time::Instant::now());
                        hook_shared.send(Wire::Enter{x:rx,y:ry,side:layout.side,remote_display:remote_idx});
                        return Some(event);
                    }
                }
                return Some(event);
            }
            _=>{}
        }
        if remote {
            // Windows Fn/Globe is not a portable key. Emitting this as a
            // synthetic macOS event can invoke system Dictation.
            if matches!(et,EventType::KeyPress(Key::Function)|EventType::KeyRelease(Key::Function)){
                return None;
            }
            let target=*hook_shared.target.lock().unwrap();
            let mapped=match et {
                EventType::KeyPress(k)=>EventType::KeyPress(input::remap(k,me,target,hook_shared.remap.load(Ordering::Relaxed))),
                EventType::KeyRelease(k)=>EventType::KeyRelease(input::remap(k,me,target,hook_shared.remap.load(Ordering::Relaxed))),
                x=>x,
            };
            hook_shared.send(Wire::Input(mapped));
            None
        }else{Some(event)}
    }){
        shared.capture_started.store(false,Ordering::SeqCst);
        shared.close();
        shared.fatal(UiStatus::InputHookFailed(format!("{e:?}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn fractional_gain_keeps_subpixel_motion(){
        let mut m=MotionScale::default();
        assert_eq!(m.scale(1,0,0.5),(0,0));
        assert_eq!(m.scale(1,0,0.5),(1,0));
        assert_eq!(m.scale(10,-4,2.0),(20,-8));
        assert_eq!(m.scale(100,-100,4.0),(400,-400));
    }
    #[test] fn precision_wheel_and_direction(){
        let mut s=WheelScale::default();
        assert_eq!(s.scale(0,30,3.0),(0,0));
        assert_eq!(s.scale(0,30,3.0),(0,1));
        assert_eq!(s.scale(0,30,3.0),(0,1));
        assert_eq!(s.scale(0,30,3.0),(0,1));
        assert_eq!(s.scale(-120,120,4.0),(-4,4));
    }
    #[test] fn paste_on_demand_routing(){
        assert_eq!(paste_action(true,Owner::Local,true),PasteAction::SendLocalClipboard);
        assert_eq!(paste_action(false,Owner::Remote,true),PasteAction::RequestRemoteClipboard);
        assert_eq!(paste_action(false,Owner::Local,true),PasteAction::Native);
        assert_eq!(paste_action(true,Owner::Remote,true),PasteAction::Native);
        assert_eq!(paste_action(true,Owner::Local,false),PasteAction::Native);
    }
}
