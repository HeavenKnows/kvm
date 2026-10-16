//! Receiver native cursor operations. macOS uses display *points* (not
//! Retina backing pixels), matching Core Graphics global event coordinates.
#[cfg(target_os="macos")]
mod mac {
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
