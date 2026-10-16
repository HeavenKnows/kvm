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
    pub fn click(button:rdev::Button,pressed:bool,x:i32,y:i32)->Result<(),String>{
        let (kind,which)=button_mapping(button,pressed)
            .ok_or_else(||format!("Unsupported mouse button: {button:?}"))?;
        let bounds=unsafe{CGDisplayBounds(CGMainDisplayID())};
        let point=Point{x:bounds.origin.x+x as f64,y:bounds.origin.y+y as f64};
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
        }
    }
    pub fn dimensions()->(i32,i32){
        let rect=unsafe{CGDisplayBounds(CGMainDisplayID())};
        ((rect.size.width.round() as i32).max(100),
         (rect.size.height.round() as i32).max(100))
    }
    pub fn move_to(x:i32,y:i32)->Result<(),String>{
        let rect=unsafe{CGDisplayBounds(CGMainDisplayID())};
        let point=Point{x:rect.origin.x+x as f64,y:rect.origin.y+y as f64};
        let code=unsafe{CGWarpMouseCursorPosition(point)};
        if code==0{Ok(())}else{Err(format!("macOS cursor warp failed: CGError {code}"))}
    }
}
pub fn screen_size()->(i32,i32){
    #[cfg(target_os="macos")]
    {return mac::dimensions();}
    #[cfg(not(target_os="macos"))]
    {let (w,h)=rdev::display_size().unwrap_or((1920,1080));(w as i32,h as i32)}
}
pub fn move_to(x:i32,y:i32)->Result<(),String>{
    #[cfg(target_os="macos")]
    {return mac::move_to(x,y);}
    #[cfg(not(target_os="macos"))]
    {rdev::simulate(&rdev::EventType::MouseMove{x:x as f64,y:y as f64})
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
