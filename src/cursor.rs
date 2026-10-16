//! Native cursor warp used only on the *controller*.
//! This is a temporary compatibility backend until platform raw relative input
//! (Win32 Raw Input / macOS CGEventTap deltaX/deltaY) is implemented.
#[cfg(target_os="windows")]
#[link(name="user32")]
extern "system" {
    fn SetCursorPos(x:i32, y:i32) -> i32;
}
#[cfg(target_os="macos")]
#[repr(C)]
struct CGPoint { x:f64, y:f64 }
#[cfg(target_os="macos")]
#[link(name="ApplicationServices", kind="framework")]
extern "C" {
    fn CGWarpMouseCursorPosition(new_cursor_position:CGPoint) -> i32;
}

pub fn warp(x:i32,y:i32)->bool{
    #[cfg(target_os="windows")]
    { unsafe { SetCursorPos(x,y)!=0 } }
    #[cfg(target_os="macos")]
    { unsafe { CGWarpMouseCursorPosition(CGPoint{x:x as f64,y:y as f64})==0 } }
    #[cfg(not(any(target_os="windows",target_os="macos")))]
    { let _=(x,y); false }
}

pub fn warp_anchor(w:i32,h:i32)->(i32,i32){
    (w.max(100)/2,h.max(100)/2)
}

pub fn needs_recentering(x:i32,y:i32,anchor:(i32,i32))->bool{
    (x-anchor.0).abs()>48 || (y-anchor.1).abs()>48
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mouse_recenter_threshold(){
        let center=warp_anchor(1920,1080);
        assert_eq!(center,(960,540));
        assert!(!needs_recentering(975,535,center));
        assert!(needs_recentering(1919,540,center));
    }
}
