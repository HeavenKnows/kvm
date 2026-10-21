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
        // CoreGraphics CGEvent.h declares wheel1 as a FIXED argument,
        // and only wheel2/3 are variadic. This is essential on Apple arm64:
        // fixed values are register-passed, varargs are stack-passed.
        fn CGEventCreateScrollWheelEvent(
            source:*const c_void,units:u32,wheel_count:u32,wheel1:i32,...
        )->*mut c_void;
        #[cfg(test)]
        fn CGEventGetIntegerValueField(event:*const c_void,field:u32)->i64;
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
            (rdev::Button::Unknown(3),true)=>Some((25,3)),
            (rdev::Button::Unknown(3),false)=>Some((26,3)),
            (rdev::Button::Unknown(4),true)=>Some((25,4)),
            (rdev::Button::Unknown(4),false)=>Some((26,4)),
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
    const ACCESSIBILITY_SETTINGS_URL:&str=
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
    pub fn request_permission()->Result<(),String>{
        if trusted(){return Ok(());}
        // Open the exact System Settings pane directly. Do NOT also call
        // AXIsProcessTrustedWithOptions(prompt=true): macOS can show its own
        // confirmation for that API, so combining both paths creates two
        // navigation prompts for one button click. The user must still enable
        // kvm manually in System Settings; this app never grants TCC access.
        let status=std::process::Command::new("/usr/bin/open")
            .arg(ACCESSIBILITY_SETTINGS_URL)
            .status().map_err(|e|format!("无法打开辅助功能设置：{e}"))?;
        if status.success(){Ok(())}else{
            Err(format!("无法打开辅助功能设置（open 退出码：{status}）"))
        }
    }
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
        fn accessibility_button_uses_one_direct_settings_route(){
            assert_eq!(ACCESSIBILITY_SETTINGS_URL,
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility");
        }
        #[test]
        fn left_right_middle_button_events(){
            assert_eq!(button_mapping(rdev::Button::Left,true),Some((1,0)));
            assert_eq!(button_mapping(rdev::Button::Left,false),Some((2,0)));
            assert_eq!(button_mapping(rdev::Button::Right,true),Some((3,1)));
            assert_eq!(button_mapping(rdev::Button::Right,false),Some((4,1)));
            assert_eq!(button_mapping(rdev::Button::Middle,true),Some((25,2)));
            assert_eq!(button_mapping(rdev::Button::Middle,false),Some((26,2)));
            assert_eq!(button_mapping(rdev::Button::Unknown(3),true),Some((25,3)));
            assert_eq!(button_mapping(rdev::Button::Unknown(4),false),Some((26,4)));
            assert_eq!(movement_kind(Some(rdev::Button::Left)),(6,0));
            assert_eq!(movement_kind(Some(rdev::Button::Right)),(7,1));
            assert_eq!(movement_kind(Some(rdev::Button::Middle)),(27,2));
            assert_eq!(movement_kind(None),(5,0));
        }
        #[test]
        fn apple_silicon_scroll_abi_preserves_vertical_direction(){
            // This test calls the ACTUAL CoreGraphics function in the macOS
            // CI runner, rather than merely checking a Rust sign conversion.
            // It would catch the beta.12/13 declaration where wheel1 was
            // incorrectly classified as a variadic argument.
            for &(win_x,win_y) in &[(0,4),(0,-4),(3,0),(-3,0),
                                   (3,4),(-3,-4)] {
                let event=create_scroll_event(win_x,win_y).unwrap();
                let point_y=unsafe{CGEventGetIntegerValueField(event,96)};
                let point_x=unsafe{CGEventGetIntegerValueField(event,97)};
                unsafe{CFRelease(event);}
                assert_eq!(point_y,win_y as i64,
                    "CoreGraphics vertical sign/ABI mismatch for ({win_x},{win_y})");
                assert_eq!(point_x,-(win_x as i64),
                    "CoreGraphics horizontal sign/ABI mismatch for ({win_x},{win_y})");
            }
        }
    }
    pub fn dimensions()->(i32,i32){
        let s=crate::display::current().screen();(s.w,s.h)
    }
    /// Build a native pixel scroll event without posting. Uses the EXACT C
    /// signature so the event's vertical axis cannot read uninitialized data
    /// on Apple Silicon. Windows and Quartz share the vertical sign
    /// (positive=up); their horizontal sign differs.
    fn create_scroll_event(x:i32,y:i32)->Result<*mut c_void,String>{
        // kCGScrollEventUnitPixel=0. Axis 1=vertical (fixed int32),
        // axis 2=horizontal (first variadic int32).
        let event=unsafe{CGEventCreateScrollWheelEvent(
            std::ptr::null(),0,2,y,x.saturating_neg()
        )};
        if event.is_null(){Err("Cannot create macOS scroll event".into())}else{Ok(event)}
    }
    pub fn scroll(x:i32,y:i32)->Result<(),String>{
        if x==0 && y==0{return Ok(());}
        if !trusted(){return Err("macOS Accessibility permission is not granted".into());}
        // Apple warns that large single wheel deltas can have unexpected
        // effects in apps. Split high-speed/combined input into small signed
        // events and bound the total work per inbound network message.
        let(mut left_x,mut left_y)=(x.clamp(-160,160),y.clamp(-160,160));
        while left_x!=0||left_y!=0 {
            let(step_x,step_y)=(left_x.clamp(-10,10),left_y.clamp(-10,10));
            let event=create_scroll_event(step_x,step_y)?;
            unsafe{CGEventPost(0,event);CFRelease(event);}
            left_x-=step_x;
            left_y-=step_y;
        }
        Ok(())
    }
    pub fn move_to(x:i32,y:i32)->Result<(),String>{
        let point=global_point(x,y);
        let code=unsafe{CGWarpMouseCursorPosition(point)};
        if code==0{Ok(())}else{Err(format!("macOS cursor warp failed: CGError {code}"))}
    }
    pub fn move_and_notify(x:i32,y:i32)->Result<(),String>{
        // rdev::simulate for every MoveBy injects an additional high-level
        // event; create the hover event directly with CoreGraphics instead.
        move_to(x,y)?;
        let event=unsafe{CGEventCreateMouseEvent(std::ptr::null(),5,global_point(x,y),0)};
        if event.is_null(){return Err("Cannot create macOS mouse moved event".into());}
        unsafe{CGEventPost(0,event);CFRelease(event);}
        Ok(())
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

/// Warp and notify hover on macOS without the rdev high-level simulation path.
pub fn move_and_notify(x:i32,y:i32)->Result<(),String>{
    #[cfg(target_os="macos")]
    {return mac::move_and_notify(x,y);}
    #[cfg(not(target_os="macos"))]
    {move_to(x,y)}
}

/// Send an OS-native scroll event. Call on the receiver's network worker.
pub fn scroll(x:i32,y:i32)->Result<(),String>{
    #[cfg(target_os="macos")]
    {return mac::scroll(x,y);}
    #[cfg(not(target_os="macos"))]
    {rdev::simulate(&rdev::EventType::Wheel{
        delta_x:x as i64,delta_y:y as i64
    }).map_err(|e|format!("Cannot scroll: {e:?}"))}
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

/// Prompt macOS to open its Accessibility permission pane. This deliberately
/// never claims to auto-authorize: user approval is mandatory.
pub fn request_accessibility_permission()->Result<(),String>{
    #[cfg(target_os="macos")]
    {return mac::request_permission();}
    #[cfg(not(target_os="macos"))]
    {Ok(())}
}

/// Send a native mouse drag event on every motion update while a button is held.
pub fn drag_to(x:i32,y:i32,button:rdev::Button)->Result<(),String>{
    #[cfg(target_os="macos")]
    {return mac::motion_while_held(x,y,button);}
    #[cfg(not(target_os="macos"))]
    {let _=button;move_to(x,y)}
}
