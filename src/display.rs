//! Enumerate physical monitors and normalize OS-global coordinates so that
//! every display uses (0,0) at the virtual desktop's top-left. The OS may
//! report negative x/y for monitors left or above the primary screen.
use crate::model::{Display,Screen};
#[derive(Clone,Debug)]
pub struct Desktop {
    pub origin_x:i32,
    pub origin_y:i32,
    pub displays:Vec<Display>,
}
impl Desktop {
    /// Lightweight placeholder. Never query CoreGraphics on the GUI thread.
    pub fn fallback()->Self{
        Self{origin_x:0,origin_y:0,displays:vec![Display{x:0,y:0,w:1920,h:1080}]}
    }
    pub fn screen(&self)->Screen{crate::model::desktop_size(&self.displays)}
    fn from_global(mut monitors:Vec<(Display,bool)>)->Self{
        if monitors.is_empty(){
            monitors.push((Display{x:0,y:0,w:1920,h:1080},true));
        }
        monitors.sort_by_key(|(d,primary)|(!primary,d.x,d.y));
        let min_x=monitors.iter().map(|(d,_)|d.x).min().unwrap_or(0);
        let min_y=monitors.iter().map(|(d,_)|d.y).min().unwrap_or(0);
        Self{origin_x:min_x,origin_y:min_y,displays:monitors.into_iter().map(|(mut d,_)|{
            d.x-=min_x;d.y-=min_y;d
        }).collect()}
    }
}
#[cfg(target_os="macos")]
mod platform {
    use super::*;
    #[repr(C)]#[derive(Clone,Copy)]struct Point{x:f64,y:f64}
    #[repr(C)]#[derive(Clone,Copy)]struct Size{width:f64,height:f64}
    #[repr(C)]#[derive(Clone,Copy)]struct Rect{origin:Point,size:Size}
    #[link(name="ApplicationServices",kind="framework")]
    extern "C"{
        fn CGGetActiveDisplayList(max:u32,ids:*mut u32,count:*mut u32)->i32;
        fn CGDisplayBounds(id:u32)->Rect;
        fn CGMainDisplayID()->u32;
    }
    pub fn enumerate()->Desktop{
        let mut ids=[0u32;32];let mut n=0u32;
        let error=unsafe{CGGetActiveDisplayList(ids.len() as u32,ids.as_mut_ptr(),&mut n)};
        if error!=0 {return Desktop::from_global(Vec::new());}
        let main=unsafe{CGMainDisplayID()};
        // Defensive bound: malformed/out-of-range display counts must never
        // create a slice past the fixed buffer on exotic monitor setups.
        let monitors=ids[..(n as usize).min(ids.len())].iter().map(|&id|{
            let r=unsafe{CGDisplayBounds(id)};
            (Display{x:r.origin.x.round() as i32,y:r.origin.y.round() as i32,
                w:(r.size.width.round() as i32).max(100),
                h:(r.size.height.round() as i32).max(100)},id==main)
        }).collect();
        Desktop::from_global(monitors)
    }
}
#[cfg(target_os="windows")]
mod platform{
    use super::*;
    #[repr(C)]#[derive(Clone,Copy)]struct Rect{left:i32,top:i32,right:i32,bottom:i32}
    #[repr(C)]struct MonitorInfo{size:u32,monitor:Rect,work:Rect,flags:u32}
    #[link(name="user32")]
    extern "system"{
        fn EnumDisplayMonitors(hdc:isize,clip:*const Rect,
            callback:Option<unsafe extern "system" fn(isize,isize,*const Rect,isize)->i32>,
            user:isize)->i32;
        fn GetMonitorInfoW(monitor:isize,info:*mut MonitorInfo)->i32;
    }
    unsafe extern "system" fn cb(handle:isize,_hdc:isize,_rect:*const Rect,user:isize)->i32{
        let output=&mut *(user as *mut Vec<(Display,bool)>);
        let mut info=MonitorInfo{size:std::mem::size_of::<MonitorInfo>() as u32,
            monitor:Rect{left:0,top:0,right:0,bottom:0},
            work:Rect{left:0,top:0,right:0,bottom:0},flags:0};
        if GetMonitorInfoW(handle,&mut info)!=0{
            let r=info.monitor;
            output.push((Display{x:r.left,y:r.top,
                w:(r.right-r.left).max(100),h:(r.bottom-r.top).max(100)},
                info.flags&1!=0));
        }
        1
    }
    pub fn enumerate()->Desktop{
        let mut monitors=Vec::<(Display,bool)>::new();
        unsafe{EnumDisplayMonitors(0,std::ptr::null(),Some(cb),
            &mut monitors as *mut _ as isize);}
        Desktop::from_global(monitors)
    }
}
pub fn current()->Desktop{platform::enumerate()}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn normalize_negative_secondary_monitor(){
        let desktop=Desktop::from_global(vec![
            (Display{x:0,y:0,w:1920,h:1080},true),
            (Display{x:-1280,y:-200,w:1280,h:1024},false)
        ]);
        assert_eq!(desktop.origin_x,-1280);
        assert_eq!(desktop.origin_y,-200);
        assert_eq!(desktop.displays[0],Display{x:1280,y:200,w:1920,h:1080});
        assert_eq!(desktop.displays[1],Display{x:0,y:0,w:1280,h:1024});
        assert_eq!(desktop.screen().w,3200);
    }
}
