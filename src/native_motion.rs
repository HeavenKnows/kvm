//! OS-native *relative* mouse motion, independent of the cursor's screen edge.
//! The existing rdev grab still decides which peer owns the pointer and
//! suppresses local mouse/key events. This module only supplies motion deltas.
use crate::engine::Shared;
use std::sync::{Arc, mpsc::{self, SyncSender}};
use std::time::Duration;

pub fn start(shared: Arc<Shared>) -> Result<(), String> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("kvm-native-mouse".into())
        .spawn(move || {
            let result = platform::run(shared.clone(), tx);
            // If the backend unexpectedly exits, release captured input.
            shared.native_motion_stopped(result.err().unwrap_or_else(|| "Native mouse monitor stopped".into()));
        })
        .map_err(|e| format!("Cannot start native mouse thread: {e}"))?;
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|e| format!("Native mouse initialization timed out: {e}"))?
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use core_foundation::runloop::{CFRunLoop,kCFRunLoopCommonModes};
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapPlacement, CGEventTapOptions,
        CGEventType, CallbackResult, EventField,
    };
    pub fn run(shared:Arc<Shared>, ready:SyncSender<Result<(),String>>)->Result<(),String> {
        // Listen at the head of the session chain: rdev's suppressing grab may
        // otherwise swallow events before their native deltas can be read.
        let callback_shared=shared.clone();
        let tap=match CGEventTap::new(
            CGEventTapLocation::Session,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            vec![CGEventType::MouseMoved,CGEventType::LeftMouseDragged,
                 CGEventType::RightMouseDragged,CGEventType::OtherMouseDragged],
            move |_proxy,kind,event| {
                if matches!(kind,CGEventType::MouseMoved|CGEventType::LeftMouseDragged|
                                 CGEventType::RightMouseDragged|CGEventType::OtherMouseDragged) {
                    let dx=event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_X);
                    let dy=event.get_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y);
                    callback_shared.add_native_motion(
                        dx.clamp(-2000,2000) as i32,
                        dy.clamp(-2000,2000) as i32);
                }
                CallbackResult::Keep
            },
        ){
            Ok(tap)=>tap,
            Err(())=>{
                let message="Cannot create CGEventTap; check Accessibility and Input Monitoring permissions".to_string();
                let _=ready.send(Err(message.clone()));
                return Err(message);
            }
        };
        let source=match tap.mach_port().create_runloop_source(0){
            Ok(v)=>v,
            Err(())=>{
                let message="Cannot create macOS CGEventTap run loop source".to_string();
                let _=ready.send(Err(message.clone()));
                return Err(message);
            }
        };
        CFRunLoop::get_current().add_source(&source,unsafe{kCFRunLoopCommonModes});
        tap.enable();
        let _=ready.send(Ok(()));
        // The native mouse monitor is intentionally process-long. Reconnects
        // reuse one tap, never registering a second event stream.
        CFRunLoop::run_current();
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::{cell::RefCell,ffi::c_void,mem::{size_of,zeroed},ptr};

    const WM_INPUT:u32=0x00ff;
    const RID_INPUT:u32=0x1000_0003;
    const RIDEV_INPUTSINK:u32=0x0000_0100;
    const RIM_TYPEMOUSE:u32=0;
    const MOUSE_MOVE_ABSOLUTE:u16=0x0001;
    // RAWMOUSE.usButtonFlags, see Win32 RI_MOUSE_*_BUTTON_{DOWN,UP}.
    fn button_changes(flags:u16)->Vec<(rdev::Button,bool)>{
        let mut result=Vec::new();
        for (down,up,button) in [
            (0x0001,0x0002,rdev::Button::Left),
            (0x0004,0x0008,rdev::Button::Right),
            (0x0010,0x0020,rdev::Button::Middle)
        ] {
            if flags&down!=0 {result.push((button,true));}
            if flags&up!=0 {result.push((button,false));}
        }
        result
    }
    const HWND_MESSAGE:isize=-3;

    #[repr(C)]
    struct WindowClass {
        cb_size:u32, style:u32,
        wnd_proc:Option<unsafe extern "system" fn(isize,u32,usize,isize)->isize>,
        cls_extra:i32, wnd_extra:i32, instance:isize,
        icon:isize, cursor:isize, background:isize,
        menu_name:*const u16, class_name:*const u16, small_icon:isize,
    }
    #[repr(C)]
    struct RawInputDevice {page:u16,usage:u16,flags:u32,window:isize}
    #[repr(C)]
    struct RawInputHeader {kind:u32,size:u32,device:isize,wparam:usize}
    #[repr(C)]
    struct Point {x:i32,y:i32}
    #[repr(C)]
    struct Message {
        window:isize, message:u32,wparam:usize,lparam:isize,
        time:u32,point:Point,private:u32,
    }

    #[link(name="user32")]
    extern "system" {
        fn RegisterClassExW(cls:*const WindowClass)->u16;
        fn CreateWindowExW(ex:u32,class:*const u16,title:*const u16,style:u32,
            x:i32,y:i32,width:i32,height:i32,parent:isize,menu:isize,
            instance:isize,param:*const c_void)->isize;
        fn RegisterRawInputDevices(devices:*const RawInputDevice,count:u32,size:u32)->i32;
        fn GetRawInputData(raw:isize,command:u32,data:*mut c_void,size:*mut u32,header_size:u32)->u32;
        fn DefWindowProcW(window:isize,message:u32,wparam:usize,lparam:isize)->isize;
        fn GetMessageW(message:*mut Message,window:isize,min:u32,max:u32)->i32;
        fn TranslateMessage(message:*const Message)->i32;
        fn DispatchMessageW(message:*const Message)->isize;
        fn DestroyWindow(window:isize)->i32;
    }
    #[link(name="kernel32")]
    extern "system" {
        fn GetModuleHandleW(name:*const u16)->isize;
    }

    thread_local! {
        static MOUSE_TARGET:RefCell<Option<Arc<Shared>>>=const{RefCell::new(None)};
    }
    unsafe extern "system" fn wnd_proc(hwnd:isize,msg:u32,wparam:usize,lparam:isize)->isize {
        if msg==WM_INPUT {
            // RAWINPUTHEADER is 24 bytes in the x64 Windows build matrix.
            // RAWMOUSE: USHORT usFlags, pad2, ULONG buttons, ULONG rawButtons,
            // LONG lLastX, LONG lLastY, ULONG extra.
            let mut raw=[0u8;128];
            let mut n=raw.len() as u32;
            let read=GetRawInputData(lparam,RID_INPUT,raw.as_mut_ptr().cast(),
                &mut n,size_of::<RawInputHeader>() as u32);
            let h=size_of::<RawInputHeader>();
            if read!=u32::MAX && (read as usize)>=h+20 {
                let kind=ptr::read_unaligned(raw.as_ptr().cast::<u32>());
                if kind==RIM_TYPEMOUSE {
                    let data=raw.as_ptr().add(h);
                    let flags=ptr::read_unaligned(data.cast::<u16>());
                    // usButtonFlags lives at offset +4 in RAWMOUSE.
                    let button_flags=ptr::read_unaligned(data.add(4).cast::<u16>());
                    let dx=ptr::read_unaligned(data.add(12).cast::<i32>());
                    let dy=ptr::read_unaligned(data.add(16).cast::<i32>());
                    MOUSE_TARGET.with(|slot|{
                        if let Some(target)=slot.borrow().as_ref(){
                            if (flags&MOUSE_MOVE_ABSOLUTE)==0 && (dx!=0||dy!=0){
                                target.add_native_motion(dx,dy);
                            }
                            if button_flags!=0 {
                                for (button,pressed) in button_changes(button_flags) {
                                    target.add_native_button(button,pressed);
                                }
                            }
                        }
                    });
                }
            }
        }
        DefWindowProcW(hwnd,msg,wparam,lparam)
    }

    pub fn run(shared:Arc<Shared>,ready:SyncSender<Result<(),String>>)->Result<(),String>{
        let class_name:Vec<u16>="KvmRawMouseListenerV1".encode_utf16().chain(Some(0)).collect();
        let instance=unsafe{GetModuleHandleW(ptr::null())};
        let wc=WindowClass{
            cb_size:size_of::<WindowClass>() as u32,
            style:0,wnd_proc:Some(wnd_proc),cls_extra:0,wnd_extra:0,
            instance,icon:0,cursor:0,background:0,
            menu_name:ptr::null(),class_name:class_name.as_ptr(),small_icon:0,
        };
        if unsafe{RegisterClassExW(&wc)}==0{
            let msg=format!("RegisterClassExW failed: {}",std::io::Error::last_os_error());
            let _=ready.send(Err(msg.clone()));
            return Err(msg);
        }
        let hwnd=unsafe{CreateWindowExW(0,class_name.as_ptr(),ptr::null(),0,
                0,0,0,0,HWND_MESSAGE,0,instance,ptr::null())};
        if hwnd==0{
            let msg=format!("CreateWindowExW failed: {}",std::io::Error::last_os_error());
            let _=ready.send(Err(msg.clone()));
            return Err(msg);
        }
        MOUSE_TARGET.with(|slot|*slot.borrow_mut()=Some(shared));
        let device=RawInputDevice{page:0x01,usage:0x02,flags:RIDEV_INPUTSINK,window:hwnd};
        if unsafe{RegisterRawInputDevices(&device,1,size_of::<RawInputDevice>() as u32)}==0{
            let msg=format!("RegisterRawInputDevices failed: {}",std::io::Error::last_os_error());
            unsafe{DestroyWindow(hwnd);}
            let _=ready.send(Err(msg.clone()));
            return Err(msg);
        }
        let _=ready.send(Ok(()));
        loop{
            let mut msg:Message=unsafe{zeroed()};
            let n=unsafe{GetMessageW(&mut msg,0,0,0)};
            if n<=0{break;}
            unsafe{TranslateMessage(&msg);DispatchMessageW(&msg);}
        }
        unsafe{DestroyWindow(hwnd);}
        Ok(())
    }
}

#[cfg(all(test, target_os="windows"))]
mod tests {
    use super::*;

    // Windows CI smoke test: compilation alone cannot reveal failed window-class
    // registration, wrong FFI struct layout, or RegisterRawInputDevices errors.
    #[test]
    fn mouse_button_flag_mapping(){
        use rdev::Button;
        let events=platform::button_changes(0x0001 | 0x0008 | 0x0010);
        assert_eq!(events,vec![(Button::Left,true),(Button::Right,false),(Button::Middle,true)]);
        assert!(platform::button_changes(0).is_empty());
    }
    #[test]
    fn raw_input_registration_works() {
        let shared=crate::engine::Shared::new(&crate::model::Settings::default());
        start(shared).expect("Windows raw mouse listener should initialize");
    }
}
