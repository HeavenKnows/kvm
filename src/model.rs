use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Side { Left, Right, Top, Bottom }

impl Default for Side { fn default() -> Self { Self::Right } }

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Layout {
    pub side: Side,
    /// Origin offset of the remote screen perpendicular to the shared edge, in local pixels.
    pub offset: i32,
}
impl Default for Layout {
    fn default() -> Self { Self { side: Side::Right, offset: 0 } }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Screen { pub w: i32, pub h: i32 }
impl Screen {
    pub fn new(w:i32,h:i32)->Self { Self {w:w.max(100),h:h.max(100)} }
}
#[derive(Debug,Clone,Copy,Serialize,Deserialize,PartialEq,Eq)]
pub struct Display {
    pub x:i32,pub y:i32,pub w:i32,pub h:i32,
}
impl Display {
    pub fn screen(self)->Screen{Screen::new(self.w,self.h)}
    pub fn contains(self,x:i32,y:i32)->bool{
        x>=self.x && x<self.x+self.w && y>=self.y && y<self.y+self.h
    }
    pub fn closest(self,x:i32,y:i32)->(i32,i32){
        (x.clamp(self.x,self.x+self.w-1),y.clamp(self.y,self.y+self.h-1))
    }
}
pub fn desktop_size(displays:&[Display])->Screen {
    let xmax=displays.iter().map(|d|d.x+d.w).max().unwrap_or(1920);
    let ymax=displays.iter().map(|d|d.y+d.h).max().unwrap_or(1080);
    Screen::new(xmax,ymax)
}
pub fn closest_on_displays(displays:&[Display],x:i32,y:i32)->(i32,i32){
    displays.iter().map(|d|d.closest(x,y))
        .min_by_key(|&(px,py)| (px as i64-x as i64).pow(2)+(py as i64-y as i64).pow(2))
        .unwrap_or((x,y))
}
pub fn single_display(screen:Screen)->Vec<Display>{
    vec![Display{x:0,y:0,w:screen.w,h:screen.h}]
}
/// A remote display group is positioned so selected remote/local displays
/// share the chosen edge. Display geometry is normalized to each desktop.
pub fn group_origin(
    layout:Layout,local:&[Display],remote:&[Display],
    local_index:usize,remote_index:usize
)->(i32,i32){
    let l=local.get(local_index).or_else(||local.first()).copied()
        .unwrap_or(Display{x:0,y:0,w:1920,h:1080});
    let r=remote.get(remote_index).or_else(||remote.first()).copied()
        .unwrap_or(Display{x:0,y:0,w:1920,h:1080});
    match layout.side{
        Side::Right=>(l.x+l.w-r.x,l.y+layout.offset-r.y),
        Side::Left=>(l.x-r.w-r.x,l.y+layout.offset-r.y),
        Side::Top=>(l.x+layout.offset-r.x,l.y-r.h-r.y),
        Side::Bottom=>(l.x+layout.offset-r.x,l.y+l.h-r.y),
    }
}
pub fn entering_multi(
    layout:Layout,x:i32,y:i32,local:&[Display],remote:&[Display],
    local_index:usize,remote_index:usize
)->Option<(i32,i32)>{
    let l=local.get(local_index)?;
    let r=remote.get(remote_index)?;
    let (gx,gy)=group_origin(layout,local,remote,local_index,remote_index);
    let (rx,ry)=(x-gx,y-gy);
    let at_edge=match layout.side {
        Side::Right=>x>=l.x+l.w-3 && x<l.x+l.w && y>=l.y && y<l.y+l.h,
        Side::Left=>x<=l.x+2 && x>=l.x && y>=l.y && y<l.y+l.h,
        Side::Top=>y<=l.y+2 && y>=l.y && x>=l.x && x<l.x+l.w,
        Side::Bottom=>y>=l.y+l.h-3 && y<l.y+l.h && x>=l.x && x<l.x+l.w,
    };
    if !at_edge{return None;}
    let (rx,ry)=match layout.side {
        Side::Right=>(r.x+3,ry),
        Side::Left=>(r.x+r.w-4,ry),
        Side::Top=>(rx,r.y+r.h-4),
        Side::Bottom=>(rx,r.y+3),
    };
    if !r.contains(rx,ry){return None;}
    // Do not steal the pointer from a neighboring local monitor. Only an
    // exposed physical edge can switch between computers.
    let (nx,ny)=match layout.side{
        Side::Right=>(x+4,y),Side::Left=>(x-4,y),
        Side::Top=>(x,y-4),Side::Bottom=>(x,y+4),
    };
    if local.iter().any(|d|d.contains(nx,ny)){return None;}
    Some(r.closest(rx,ry))
}
/// Find the nearest exposed local-display edge to a freely dragged remote
/// panel, then determine the snapped side and perpendicular offset.
pub fn snap_to_edge(
    local:&[Display],remote:&[Display],remote_index:usize,
    dragged_x:i32,dragged_y:i32
)->(usize,Layout){
    let r=remote.get(remote_index).or_else(||remote.first()).copied()
        .unwrap_or(Display{x:0,y:0,w:1920,h:1080});
    let mut best=None::<(i64,usize,Layout)>;
    for (i,l) in local.iter().enumerate(){
        let overlap_y=(dragged_y+r.h).min(l.y+l.h)-dragged_y.max(l.y);
        let overlap_x=(dragged_x+r.w).min(l.x+l.w)-dragged_x.max(l.x);
        for (side,edge_gap,overlap,offset) in [
            (Side::Right,(dragged_x-(l.x+l.w)).abs(),overlap_y,dragged_y-l.y),
            (Side::Left,(dragged_x+r.w-l.x).abs(),overlap_y,dragged_y-l.y),
            (Side::Top,(dragged_y+r.h-l.y).abs(),overlap_x,dragged_x-l.x),
            (Side::Bottom,(dragged_y-(l.y+l.h)).abs(),overlap_x,dragged_x-l.x)
        ]{
            let score=edge_gap as i64 + if overlap<=0{(-overlap+100) as i64}else{0};
            let candidate=(score,i,Layout{side,offset});
            if best.as_ref().is_none_or(|v|score<v.0){best=Some(candidate);}
        }
    }
    best.map(|(_,i,layout)|(i,layout)).unwrap_or((0,Layout::default()))
}
impl Layout {
    pub fn origin(self, local: Screen, remote: Screen) -> (i32, i32) {
        match self.side {
            Side::Left => (-remote.w, self.offset),
            Side::Right => (local.w, self.offset),
            Side::Top => (self.offset, -remote.h),
            Side::Bottom => (self.offset, local.h),
        }
    }
    pub fn clamp(self, local: Screen, remote: Screen) -> Self {
        let bound = if matches!(self.side, Side::Left | Side::Right) {
            (-remote.h + 24, local.h - 24)
        } else { (-remote.w + 24, local.w - 24) };
        Self {offset: self.offset.clamp(bound.0, bound.1), ..self }
    }
    pub fn entering(self, x:i32,y:i32,local:Screen, remote:Screen)->bool {
        let (ox,oy)=self.origin(local,remote);
        match self.side {
            Side::Right => x >= local.w-3 && y >= oy && y < oy+remote.h,
            Side::Left => x <= 2 && y >= oy && y < oy+remote.h,
            Side::Top => y <= 2 && x >= ox && x < ox+remote.w,
            Side::Bottom => y >= local.h-3 && x >= ox && x < ox+remote.w,
        }
    }
    pub fn entry(self,x:i32,y:i32,local:Screen,remote:Screen)->(i32,i32) {
        let (ox,oy)=self.origin(local,remote);
        ((x-ox).clamp(3,remote.w-4),(y-oy).clamp(3,remote.h-4))
    }
    pub fn returning(self,x:i32,y:i32,remote:Screen)->bool {
        match self.side {
            Side::Right => x <= 2, Side::Left => x >= remote.w-3,
            Side::Bottom => y <= 2, Side::Top => y >= remote.h-3,
        }
    }
    pub fn inward(self,x:i32,y:i32,remote:Screen)->bool {
        match self.side {
            Side::Right => x >= 42, Side::Left => x <= remote.w-43,
            Side::Bottom => y >= 42, Side::Top => y <= remote.h-43,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform { Windows, Mac }
impl Platform {
    pub fn local()->Self {
        if cfg!(target_os="macos") { Self::Mac } else { Self::Windows }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Language { Zh, En }
impl Default for Language { fn default()->Self { Self::Zh } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub role_server: bool,
    pub address: String,
    pub secret: String,
    pub layout: Layout,
    /// Fully explicit, editable shortcuts. No hidden platform-mapping layer.
    pub shortcut_mappings:Vec<crate::input::ShortcutMapping>,
    pub clipboard: bool,
    pub language: Language,
    pub listen_port: u16,
    pub auto_start_receiver: bool,
    /// Chosen monitor pair used as the crossing edge anchor.
    pub local_display:usize,
    pub remote_display:usize,
    /// Multiplier for unaccelerated device-count deltas on the controller.
    pub mouse_speed:f32,
    /// Pixel-based wheel gain; 1x is four macOS logical pixels per notch.
    /// High-resolution wheel fractions accumulate between packets.
    pub scroll_speed:f32,
    /// On macOS, use native XButton events instead of Command+[ / Command+].
    pub side_buttons_native:bool,
}
impl Default for Settings {
    fn default()->Self { Self {
        role_server: false, address:"192.168.1.10:24881".into(),
        secret:String::new(), layout:Layout::default(),
        shortcut_mappings:crate::input::presets(),clipboard:true,
        language:Language::Zh, listen_port:24881, auto_start_receiver:true,
        mouse_speed:1.6,scroll_speed:3.0,side_buttons_native:false,
        local_display:0,remote_display:0,
    } }
}

pub fn config_path()->Option<std::path::PathBuf> {
    dirs::config_dir().map(|p|p.join("kvm").join("settings.json"))
}
/// Use the receiver role for a fresh Mac installation, while preserving
/// explicitly saved user roles (including Mac controller mode).
pub fn fresh_install_defaults(platform:Platform)->Settings {
    let mut settings=Settings::default();
    if platform==Platform::Mac {settings.role_server=true;}
    settings
}
pub fn load()->Settings {
    // Keep existing settings for users upgrading from CrossKey: if no kvm config
    // exists, read the old file and save into the kvm directory on next change.
    let new_path = config_path();
    let legacy_path = dirs::config_dir().map(|p|p.join("crosskey").join("settings.json"));
    new_path.and_then(|p| std::fs::read(p).ok())
        .or_else(|| legacy_path.and_then(|p| std::fs::read(p).ok()))
        .and_then(|b| decode_settings(&b))
        .map(|(mut s,migrated)| {
            if s.listen_port < 1024 {s.listen_port=24881;}
            if !s.mouse_speed.is_finite() || !(0.5..=4.0).contains(&s.mouse_speed) {
                s.mouse_speed=1.6;
            }
            if !s.scroll_speed.is_finite() || !(0.1..=16.0).contains(&s.scroll_speed){
                s.scroll_speed=3.0;
            }
            if migrated {save(&s);}
            s
        })
        .unwrap_or_else(||fresh_install_defaults(Platform::local()))
}
/// Migrate the legacy remap toggle into explicit, persisted rows. The old
/// disabled setting becomes an EMPTY list, preserving user intent. Once the
/// new field exists, even [] is authoritative and never repopulated.
fn decode_settings(bytes:&[u8])->Option<(Settings,bool)>{
    let value:serde_json::Value=serde_json::from_slice(bytes).ok()?;
    let migrated=value.get("shortcut_mappings").is_none();
    let legacy_disabled=value.get("remap").and_then(|v|v.as_bool())==Some(false);
    let mut settings:Settings=serde_json::from_value(value).ok()?;
    if migrated && legacy_disabled {settings.shortcut_mappings.clear();}
    Some((settings,migrated))
}
pub fn save(s:&Settings){
    if let Some(p)=config_path(){
        if let Some(parent)=p.parent(){ let _=std::fs::create_dir_all(parent); }
        if let Ok(buf)=serde_json::to_vec_pretty(s){let _=std::fs::write(p,buf);}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn dropped_screens_snap_with_zero_gap_on_all_four_sides(){
        // A freely dragged remote rectangle from any side must end with
        // exactly touching edges. No intermediate snapping during drag.
        let local=single_display(Screen::new(1920,1080));
        let remote=single_display(Screen::new(1440,900));
        for (dest_x,dest_y,expected) in [
            (1920,50,Side::Right),
            (-1440,50,Side::Left),
            (100,-900,Side::Top),
            (100,1080,Side::Bottom)
        ] {
            let (li,layout)=snap_to_edge(&local,&remote,0,dest_x,dest_y);
            assert_eq!(li,0);
            assert_eq!(layout.side,expected,
                "Drag to ({dest_x},{dest_y}) selected wrong side");
            let final_layout=layout.clamp(local[li].screen(),remote[0].screen());
            let (origin_x,origin_y)=group_origin(final_layout,&local,&remote,li,0);
            let l=local[li];
            let r=remote[0];
            match expected {
                Side::Right=>assert_eq!(origin_x+r.x,l.x+l.w),
                Side::Left=>assert_eq!(origin_x+r.x+r.w,l.x),
                Side::Top=>assert_eq!(origin_y+r.y+r.h,l.y),
                Side::Bottom=>assert_eq!(origin_y+r.y,l.y+l.h),
            }
        }
    }
    #[test] fn switching_drag_from_right_to_left_is_not_stuck_on_initial_side(){
        let local=single_display(Screen::new(1920,1080));
        let remote=single_display(Screen::new(1440,900));
        let (_,start)=snap_to_edge(&local,&remote,0,1920,100);
        assert_eq!(start.side,Side::Right);
        let (_,drop)=snap_to_edge(&local,&remote,0,-1440,100);
        assert_eq!(drop.side,Side::Left);
        assert_eq!(group_origin(drop,&local,&remote,0,0).0,-1440);
    }
    #[test] fn offset_mapping(){
        let a=Screen::new(1920,1080); let b=Screen::new(1440,900);
        let l=Layout {side: Side::Right, offset:100};
        assert!(l.entering(1919,200,a,b));
        assert_eq!(l.entry(1919,200,a,b),(3,100));
        assert!(!l.entering(1919,20,a,b));
    }
    #[test] fn fresh_mac_install_starts_as_receiver(){
        let mac=fresh_install_defaults(Platform::Mac);
        let windows=fresh_install_defaults(Platform::Windows);
        assert!(mac.role_server);
        assert!(mac.auto_start_receiver);
        assert!(!windows.role_server);
        assert!(windows.auto_start_receiver);
    }
    #[test] fn backwards_compatible_settings(){
        let old=r#"{"role_server":false,"address":"192.168.1.2:24881","secret":"","layout":{"side":"Right","offset":0},"remap":true,"clipboard":true}"#;
        let s:Settings=serde_json::from_str(old).unwrap();
        assert_eq!(s.listen_port,24881);
        assert_eq!(s.language,Language::Zh);
        assert!(s.auto_start_receiver);
        assert_eq!(s.local_display,0);
        assert_eq!(s.remote_display,0);
        assert_eq!(s.mouse_speed,1.6);
        assert_eq!(s.scroll_speed,3.0);
        assert!(!s.shortcut_mappings.is_empty());
        let old=serde_json::to_string(&s).unwrap();
        let latest:Settings=serde_json::from_str(&old).unwrap();
        assert_eq!(latest.scroll_speed,3.0);
    }
    #[test] fn no_hidden_shortcut_mappings_on_migration(){
        let old=br#"{"remap":false,"role_server":false}"#;
        let (disabled,migrated)=decode_settings(old).unwrap();
        assert!(migrated);
        assert!(disabled.shortcut_mappings.is_empty());
        let serialized=serde_json::to_vec(&disabled).unwrap();
        let (reloaded,migrated)=decode_settings(&serialized).unwrap();
        assert!(!migrated);
        assert!(reloaded.shortcut_mappings.is_empty());
        let (enabled,migrated)=decode_settings(br#"{"remap":true}"#).unwrap();
        assert!(migrated);
        assert!(!enabled.shortcut_mappings.is_empty());
    }
    #[test] fn vertical_offset_and_edge_transfer(){
        let local=Screen::new(1920,1080);
        let remote=Screen::new(1440,900);
        let top=Layout{side:Side::Top,offset:150};
        assert!(top.entering(250,0,local,remote));
        assert!(!top.entering(50,0,local,remote));
        assert_eq!(top.entry(250,0,local,remote),(100,remote.h-4));
        let bottom=Layout{side:Side::Bottom,offset:-90};
        assert!(bottom.entering(150,1079,local,remote));
        assert_eq!(bottom.entry(150,1079,local,remote),(240,3));
    }
    #[test] fn multi_monitor_edge_and_snap(){
        let local=vec![
            Display{x:0,y:0,w:1920,h:1080},
            Display{x:1920,y:0,w:1600,h:900}
        ];
        let remote=vec![
            Display{x:0,y:0,w:1440,h:900},
            Display{x:1440,y:0,w:1024,h:768}
        ];
        let right=Layout{side:Side::Right,offset:0};
        assert_eq!(group_origin(right,&local,&remote,1,0),(3520,0));
        assert_eq!(entering_multi(right,3519,400,&local,&remote,1,0),Some((3,400)));
        assert_eq!(entering_multi(right,1919,400,&local,&remote,0,0),None);
        let (i,layout)=snap_to_edge(&local,&remote,0,3540,100);
        assert_eq!(i,1);
        assert_eq!(layout.side,Side::Right);
        assert_eq!(layout.offset,100);
    }
    #[test] fn remote_desktop_gap_is_not_valid_mouse_target(){
        let d=vec![Display{x:0,y:300,w:1000,h:800},Display{x:1000,y:0,w:800,h:800}];
        assert_eq!(closest_on_displays(&d,500,20),(500,300));
        assert_eq!(desktop_size(&d).h,1100);
    }
    #[test] fn each_direction(){
        let a=Screen::new(1920,1080); let b=Screen::new(1440,900);
        for s in [Side::Left,Side::Right,Side::Top,Side::Bottom] {
            let l=Layout{side:s,offset:0};
            let (x,y)=match s {Side::Left=>(0,500), Side::Right=>(1919,500),Side::Top=>(900,0),Side::Bottom=>(900,1079)};
            assert!(l.entering(x,y,a,b));
            let (rx,ry)=l.entry(x,y,a,b);
            assert!(rx>=3 && ry>=3 && rx<=b.w-4 && ry<=b.h-4);
        }
    }
}
