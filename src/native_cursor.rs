//! Receiver native cursor operations. macOS uses display *points* (not
//! Retina backing pixels), matching Core Graphics global event coordinates.
#[cfg(target_os="macos")]
mod mac {
    use std::ffi::c_void;
    #[repr(C)] #[derive(Clone,Copy)]
    struct Point{x:f64,y:f64}
    #[repr(C)] #[derive(Clone,Copy)]
    struct Size{width:f64,height:f64}
    #[repr(C)] #[derive(Clone,Copy)]
    struct Rect{origin:Point,size:Size}
    #[link(name="ApplicationServices",kind="framework")]
    extern "C" {
        fn CGMainDisplayID()->u32;
        fn CGDisplayBounds(display:u32)->Rect;
        fn CGWarpMouseCursorPosition(p:Point)->i32;
        fn CGEventCreateMouseEvent(source:*const c_void,event_type:u32,point:Point,button:u32)->*mut c_void;
        fn CGEventPost(tap_location:u32,event:*mut c_void);
        fn AXIsProcessTrusted()->bool;
    }
    #[link(name="CoreFoundation",kind="framework")]
    extern "C" {
        fn CFRelease(obj:*mut c_void);
    }
    // CGEventType: left down/up 1/2, right 3/4, other 25/26.
    // CGMouseButton: left 0, right 1, center 2.
    fn button_mapping(button:rdev::Button,pressed:bool)->Option<(u32,u32)>{
        match (button,pressed){
            (rdev::Button::Left,true)=>Some((1,0)),
            (rdev::Button::Left,false)=>Some((2,0)),
            (rdev::Button::Right,true)=>Some((3,1)),
            (rdev::Button::Right,false)=>Some((4,1)),
            (rdev::Button::Middle,true)=>Some((25,2)),
            (rdev::Button::Middle,false)=>Some((26,2)),
            _=>None,
        }
    }
    // CGEventType: MouseMoved=5, LeftDragged=6, RightDragged=7,
    // OtherDragged=27. Cocoa needs drag events while the button stays held.
    fn movement_kind(held:Option<rdev::Button>)->(u32,u32){
        match held {
            Some(rdev::Button::Left)=>(6,0),
            Some(rdev::Button::Right)=>(7,1),
            Some(rdev::Button::Middle)=>(27,2),
            _=>(5,0),
        }
    }
    pub fn motion_while_held(x:i32,y:i32,button:rdev::Button)->Result<(),String>{
        if !trusted(){return Err("macOS Accessibility permission is not granted".into());}
        let (kind,which)=movement_kind(Some(button));
        let pos=global_point(x,y);
        let event=unsafe{CGEventCreateMouseEvent(std::ptr::null(),kind,pos,which)};
        if event.is_null(){return Err(format!("CGEventCreateMouseEvent for drag type {kind} failed"));}
        // Do not warp during a held drag: CGEventPost dispatches actual
        // LeftMouseDragged/RightMouseDragged to the destination window.
        unsafe{CGEventPost(0,event);CFRelease(event);}
        Ok(())
    }
    static ORIGIN_X:std::sync::atomic::AtomicI32=std::sync::atomic::AtomicI32::new(0);
    static ORIGIN_Y:std::sync::atomic::AtomicI32=std::sync::atomic::AtomicI32::new(0);
    pub fn set_origin(x:i32,y:i32){
        use std::sync::atomic::Ordering;
        ORIGIN_X.store(x,Ordering::Relaxed);
        ORIGIN_Y.store(y,Ordering::Relaxed);
    }
    fn global_point(x:i32,y:i32)->Point{
        use std::sync::atomic::Ordering;
        Point{
            x:(ORIGIN_X.load(Ordering::Relaxed)+x) as f64,
            y:(ORIGIN_Y.load(Ordering::Relaxed)+y) as f64,
        }
    }
    pub fn trusted()->bool{unsafe{AXIsProcessTrusted()}}
    pub fn click(button:rdev::Button,pressed:bool,x:i32,y:i32)->Result<(),String>{
        if !trusted() {
            return Err("macOS Accessibility permission is not granted to kvm.app; enable it in System Settings > Privacy & Security > Accessibility".into());
        }
        let (kind,which)=button_mapping(button,pressed)
            .ok_or_else(||format!("Unsupported mouse button: {button:?}"))?;
        let point=global_point(x,y);
        let event=unsafe{CGEventCreateMouseEvent(std::ptr::null(),kind,point,which)};
        if event.is_null(){return Err(format!("CGEventCreateMouseEvent failed: {button:?} pressed={pressed}"));}
        // kCGHIDEventTap = 0. Requires Accessibility permission for this app.
        unsafe{CGEventPost(0,event);CFRelease(event);}
        Ok(())
    }
    #[cfg(test)]
    mod tests{
        use super::*;
        #[test]
        fn left_right_middle_button_events(){
            assert_eq!(button_mapping(rdev::Button::Left,true),Some((1,0)));
            assert_eq!(button_mapping(rdev::Button::Left,false),Some((2,0)));
            assert_eq!(button_mapping(rdev::Button::Right,true),Some((3,1)));
            assert_eq!(button_mapping(rdev::Button::Right,false),Some((4,1)));
            assert_eq!(button_mapping(rdev::Button::Middle,true),Some((25,2)));
            assert_eq!(button_mapping(rdev::Button::Middle,false),Some((26,2)));
            assert_eq!(movement_kind(Some(rdev::Button::Left)),(6,0));
            assert_eq!(movement_kind(Some(rdev::Button::Right)),(7,1));
            assert_eq!(movement_kind(Some(rdev::Button::Middle)),(27,2));
            assert_eq!(movement_kind(None),(5,0));
        }
    }
    pub fn dimensions()->(i32,i32){
        let s=crate::display::current().screen();(s.w,s.h)
    }
    pub fn move_to(x:i32,y:i32)->Result<(),String>{
        let point=global_point(x,y);
        let code=unsafe{CGWarpMouseCursorPosition(point)};
        if code==0{Ok(())}else{Err(format!("macOS cursor warp failed: CGError {code}"))}
    }
}
/// Update macOS global-coordinate origin after the background monitor scan.
pub fn set_global_origin(x:i32,y:i32){
    #[cfg(target_os="macos")]
    {mac::set_origin(x,y);}
    #[cfg(not(target_os="macos"))]
    {let _=(x,y);}
}
pub fn screen_size()->(i32,i32){
    #[cfg(target_os="macos")]
    {return mac::dimensions();}
    #[cfg(not(target_os="macos"))]
    {let s=crate::display::current().screen();(s.w,s.h)}
}
pub fn move_to(x:i32,y:i32)->Result<(),String>{
    #[cfg(target_os="macos")]
    {return mac::move_to(x,y);}
    #[cfg(not(target_os="macos"))]
    {let desktop=crate::display::current();
     rdev::simulate(&rdev::EventType::MouseMove{
         x:(x+desktop.origin_x) as f64,y:(y+desktop.origin_y) as f64})
        .map_err(|e|format!("Cannot simulate mouse movement: {e:?}"))}
}

/// Inject real low-level mouse down/up at the receiver's current cursor
/// location. Never silently ignore a mouse-button injection failure.
pub fn button_event(button:rdev::Button,pressed:bool,x:i32,y:i32)->Result<(),String>{
    #[cfg(target_os="macos")]
    {return mac::click(button,pressed,x,y);}
    #[cfg(not(target_os="macos"))]
    {
        let _=(x,y);
        let event=if pressed {rdev::EventType::ButtonPress(button)}
                  else {rdev::EventType::ButtonRelease(button)};
        rdev::simulate(&event).map_err(|e|format!("Cannot simulate {event:?}: {e:?}"))
    }
}

/// Whether macOS has authorized this application to synthesize mouse clicks.
pub fn accessibility_trusted()->bool{
    #[cfg(target_os="macos")]
    {mac::trusted()}
    #[cfg(not(target_os="macos"))]
    {true}
}

/// Send a native mouse drag event on every motion update while a button is held.
pub fn drag_to(x:i32,y:i32,button:rdev::Button)->Result<(),String>{
    #[cfg(target_os="macos")]
    {return mac::motion_while_held(x,y,button);}
    #[cfg(not(target_os="macos"))]
    {let _=button;move_to(x,y)}
}
