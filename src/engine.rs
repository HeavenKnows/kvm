use crate::{cursor,input, model::{Layout,Platform,Screen,Settings}, status::UiStatus, wire::{self,Wire}};
use rdev::{self,EventType,Key};
use std::{collections::HashSet,net::{TcpListener,TcpStream},sync::{Arc,Mutex,atomic::{AtomicBool,AtomicI32,Ordering},mpsc::{self,SyncSender}},time::Duration};
const MAX_TEXT:usize=32*1024;

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

pub struct Shared {
    pub status:Mutex<UiStatus>,
    pub layout:Mutex<Layout>,
    pub local_screen:Screen,
    pub remote_screen:Mutex<Screen>,
    pub target:Mutex<Platform>,
    pub active:AtomicBool,
    pub ready:AtomicBool,
    pub started:AtomicBool,
    stop_requested:AtomicBool,
    socket:Mutex<Option<TcpStream>>,
    recenter_requested:AtomicBool,
    ignore_warp:AtomicBool,
    cursor_last:Mutex<Option<(i32,i32)>>,
    capture_started:AtomicBool,
    paste_sender:Mutex<Option<SyncSender<()>>>,
    pub clipboard:AtomicBool,
    pub remap:AtomicBool,
    owner:Mutex<Owner>,
    pending_dx:AtomicI32,
    pending_dy:AtomicI32,
    entry_local:Mutex<(i32,i32)>,
    out:Mutex<Option<SyncSender<Wire>>>,
}
impl Shared {
    pub fn new(s:&Settings)->Arc<Self>{
        let (w,h)=rdev::display_size().unwrap_or((1920,1080));
        Arc::new(Self {
            status:Mutex::new(UiStatus::Idle), layout:Mutex::new(s.layout),
            local_screen:Screen::new(w as i32,h as i32),
            remote_screen:Mutex::new(Screen::new(1920,1080)),
            target:Mutex::new(Platform::Mac),active:AtomicBool::new(false),
            ready:AtomicBool::new(false),started:AtomicBool::new(false),
            capture_started:AtomicBool::new(false),paste_sender:Mutex::new(None),
            stop_requested:AtomicBool::new(false),socket:Mutex::new(None),
            recenter_requested:AtomicBool::new(false),ignore_warp:AtomicBool::new(false),
            cursor_last:Mutex::new(None),
            clipboard:AtomicBool::new(s.clipboard),remap:AtomicBool::new(s.remap),
            owner:Mutex::new(Owner::Local),
            pending_dx:AtomicI32::new(0), pending_dy:AtomicI32::new(0),
            entry_local:Mutex::new((w as i32/2,h as i32/2)),out:Mutex::new(None),
        })
    }
    pub fn info(&self,status:UiStatus){*self.status.lock().unwrap()=status;}
    fn store_socket(&self,sock:&TcpStream)->std::io::Result<()> {
        *self.socket.lock().unwrap()=Some(sock.try_clone()?);
        Ok(())
    }
    fn shutdown_peer(&self) {
        self.active.store(false,Ordering::SeqCst);
        self.ready.store(false,Ordering::SeqCst);
        self.recenter_requested.store(false,Ordering::SeqCst);
        self.ignore_warp.store(false,Ordering::SeqCst);
        if let Some(sock)=self.socket.lock().unwrap().take(){
            let _=sock.shutdown(std::net::Shutdown::Both);
        }
    }
    /// User-initiated disconnect; receiver keeps listening for the next client.
    pub fn disconnect_peer(&self){
        if self.active.load(Ordering::SeqCst) {
            self.return_cursor();
        }
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
            crate::model::Side::Right=>x=self.local_screen.w-75,
            crate::model::Side::Left=>x=75,
            crate::model::Side::Top=>y=75,
            crate::model::Side::Bottom=>y=self.local_screen.h-75,
        }
        simulate(EventType::MouseMove{x:x as f64,y:y as f64});
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
        *self.cursor_last.lock().unwrap()=None;
    }
}
fn clipboard_get()->Option<String>{
    let mut c=arboard::Clipboard::new().ok()?;
    c.get_text().ok().filter(|s|s.len()<=MAX_TEXT)
}
fn clipboard_set(s:&str)->bool {
    if s.len()>MAX_TEXT {return false;}
    arboard::Clipboard::new().and_then(|mut c|c.set_text(s.to_owned())).is_ok()
}
fn simulate(event:EventType){let _=rdev::simulate(&event);}
fn paste_hotkey(os:Platform){
    let primary=match os {Platform::Windows=>Key::ControlLeft,Platform::Mac=>Key::MetaLeft};
    simulate(EventType::KeyPress(primary));
    simulate(EventType::KeyPress(Key::KeyV));
    simulate(EventType::KeyRelease(Key::KeyV));
    simulate(EventType::KeyRelease(primary));
}

// The receiver keeps listening after a client disconnects. Only one client is
// handled at a time, preserving the existing single-controller design.
pub fn start_server(shared:Arc<Shared>, s:Settings){
    if shared.started.swap(true,Ordering::SeqCst){return;}
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
            if let Err(e)=shared.store_socket(&stream){
                shared.info(UiStatus::AcceptFailed(e.to_string()));
                continue;
            }
            shared.info(UiStatus::Authenticating(peer.to_string()));
            let (tx,rx,_control)=match wire::connect(stream,&s.secret,false){
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
            shared.send(Wire::Hello{os:Platform::local(),screen:shared.local_screen});
            let mut mouse=(shared.local_screen.w/2,shared.local_screen.h/2);
            let mut pressed=HashSet::<Key>::new();
            let mut buttons=HashSet::<rdev::Button>::new();
            for msg in rx {
                if shared.stop_requested.load(Ordering::SeqCst){break;}
                match msg {
                    Wire::Enter{x,y,side}=>{
                        let mut l=*shared.layout.lock().unwrap(); l.side=side;
                        *shared.layout.lock().unwrap()=l;
                        shared.active.store(true,Ordering::SeqCst);
                        mouse=(x.clamp(3,shared.local_screen.w-4),
                               y.clamp(3,shared.local_screen.h-4));
                        simulate(EventType::MouseMove{x:mouse.0 as f64,y:mouse.1 as f64});
                    }
                    Wire::Back=>{
                        shared.active.store(false,Ordering::SeqCst);
                        for k in pressed.drain(){simulate(EventType::KeyRelease(k));}
                        for b in buttons.drain(){simulate(EventType::ButtonRelease(b));}
                    }
                    Wire::MoveBy{dx,dy} if shared.active.load(Ordering::Relaxed)=>{
                        mouse.0=(mouse.0+dx).clamp(0,shared.local_screen.w-1);
                        mouse.1=(mouse.1+dy).clamp(0,shared.local_screen.h-1);
                        simulate(EventType::MouseMove{x:mouse.0 as f64,y:mouse.1 as f64});
                    }
                    Wire::Input(evt) if shared.active.load(Ordering::Relaxed)=>{
                        match evt {
                            EventType::KeyPress(k)=>{pressed.insert(k);},
                            EventType::KeyRelease(k)=>{pressed.remove(&k);},
                            EventType::ButtonPress(b)=>{buttons.insert(b);},
                            EventType::ButtonRelease(b)=>{buttons.remove(&b);},
                            _=>{}
                        }
                        simulate(evt);
                    }
                    Wire::PasteRemote(text) if shared.active.load(Ordering::Relaxed)=>{
                        if clipboard_set(&text){paste_hotkey(Platform::local());}
                    }
                    Wire::ClipboardRequest=>{
                        shared.send(Wire::ClipboardReply(clipboard_get()));
                    }
                    _=>{}
                }
            }
            for k in pressed {simulate(EventType::KeyRelease(k));}
            for b in buttons {simulate(EventType::ButtonRelease(b));}
            shared.close();
            if !shared.stop_requested.load(Ordering::SeqCst) {
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
    std::thread::spawn(move ||{
        let result=client_session(&shared,s);
        shared.close();
        // This is deliberately the final step: a retry cannot start while any
        // previous session still owns the sender or consumes incoming messages.
        shared.started.store(false,Ordering::SeqCst);
        if let Err(status)=result {shared.info(status);}
        else {shared.info(UiStatus::Disconnected);}
    });
}

fn client_session(shared:&Arc<Shared>,s:Settings)->Result<(),UiStatus>{
    use std::net::SocketAddr;
    use std::sync::mpsc::RecvTimeoutError;

    shared.info(UiStatus::Connecting);
    let addr:SocketAddr=s.address.parse().map_err(|_|UiStatus::InvalidAddress)?;
    let stream=TcpStream::connect_timeout(&addr,Duration::from_secs(4))
        .map_err(|e|UiStatus::ConnectFailed(e.to_string()))?;
    let (tx,rx)=wire::connect(stream,&s.secret,true)
        .map_err(|e|UiStatus::PairingFailed(e.to_string()))?;
    *shared.out.lock().unwrap()=Some(tx);
    shared.info(UiStatus::AwaitingInfo);

    // Never leave the UI permanently in "Connecting" if the receiver never
    // sends its Hello packet. No grab hook is installed before this completes.
    match rx.recv_timeout(Duration::from_secs(5)){
        Ok(Wire::Hello{os,screen})=>{
            *shared.target.lock().unwrap()=os;
            *shared.remote_screen.lock().unwrap()=screen;
        }
        Err(RecvTimeoutError::Timeout)=>return Err(UiStatus::HandshakeTimedOut),
        Err(RecvTimeoutError::Disconnected)=>return Err(UiStatus::Disconnected),
        _=>return Err(UiStatus::InvalidHandshake),
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
            if let Some(text)=clipboard_get(){work_shared.send(Wire::PasteRemote(text));}
            else {work_shared.info(UiStatus::ClipboardUnavailable);}
        }
    });
    let motion_shared=shared.clone();
    std::thread::spawn(move ||{
        while motion_shared.ready.load(Ordering::Relaxed){
            if motion_shared.active.load(Ordering::Relaxed){motion_shared.flush_motion();}
            std::thread::sleep(Duration::from_millis(2));
        }
    });

    for msg in rx {
        match msg {
            Wire::Back=>{
                shared.active.store(false,Ordering::SeqCst);
                shared.return_cursor();
                shared.info(UiStatus::Returned);
            }
            Wire::ClipboardReply(Some(text))=>{
                if clipboard_set(&text){
                    *shared.owner.lock().unwrap()=Owner::Local;
                    paste_hotkey(Platform::local());
                } else {shared.info(UiStatus::ClipboardInvalid);}
            }
            Wire::ClipboardReply(None)=>shared.info(UiStatus::ClipboardRemoteEmpty),
            _=>{}
        }
    }
    Ok(())
}

fn capture(shared:Arc<Shared>) {
    let pressed=Arc::new(Mutex::new(HashSet::<Key>::new()));
    let skip_v=Arc::new(AtomicBool::new(false));
    let skip_f12=Arc::new(AtomicBool::new(false));
    let pos=Arc::new(Mutex::new(None::<(i32,i32)>));
    let p=pressed.clone(); let skip=skip_v.clone(); let skip_emergency=skip_f12.clone(); let last=pos.clone();
    let me=Platform::local();
    let hook_shared=shared.clone();
    if let Err(e)=rdev::grab(move |event| {
        if !hook_shared.ready.load(Ordering::Relaxed){
            p.lock().unwrap().clear();
            skip.store(false,Ordering::SeqCst);
            skip_emergency.store(false,Ordering::SeqCst);
            *last.lock().unwrap()=None;
            return Some(event);
        }
        let et=event.event_type.clone();
        let remote=hook_shared.active.load(Ordering::Relaxed);
        match et {
            EventType::KeyPress(k)=>{
                p.lock().unwrap().insert(k);
                if k==Key::F12 && remote {
                    skip_emergency.store(true,Ordering::SeqCst);
                    hook_shared.active.store(false,Ordering::SeqCst);
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
            EventType::MouseMove{x,y}=>{
                let x=x as i32;let y=y as i32;
                let mut previous=last.lock().unwrap();
                let delta=previous.map(|(px,py)|(x-px,y-py)).unwrap_or((0,0));
                *previous=Some((x,y));
                if remote {
                    if delta!=(0,0) {
                        hook_shared.pending_dx.fetch_add(delta.0.clamp(-200,200),Ordering::Relaxed);
                        hook_shared.pending_dy.fetch_add(delta.1.clamp(-200,200),Ordering::Relaxed);
                    }
                    return None;
                }
                let layout=*hook_shared.layout.lock().unwrap();
                let other=*hook_shared.remote_screen.lock().unwrap();
                if layout.entering(x,y,hook_shared.local_screen,other){
                    let (rx,ry)=layout.entry(x,y,hook_shared.local_screen,other);
                    *hook_shared.entry_local.lock().unwrap()=(x,y);
                    hook_shared.active.store(true,Ordering::SeqCst);
                    hook_shared.send(Wire::Enter{x:rx,y:ry,side:layout.side});
                    return None;
                }
                return Some(event);
            }
            _=>{}
        }
        if remote {
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
        shared.started.store(false,Ordering::SeqCst);
        shared.info(UiStatus::InputHookFailed(format!("{e:?}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn paste_on_demand_routing(){
        assert_eq!(paste_action(true,Owner::Local,true),PasteAction::SendLocalClipboard);
        assert_eq!(paste_action(false,Owner::Remote,true),PasteAction::RequestRemoteClipboard);
        assert_eq!(paste_action(false,Owner::Local,true),PasteAction::Native);
        assert_eq!(paste_action(true,Owner::Remote,true),PasteAction::Native);
        assert_eq!(paste_action(true,Owner::Local,false),PasteAction::Native);
    }
}
