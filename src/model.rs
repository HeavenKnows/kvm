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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub role_server: bool,
    pub address: String,
    pub secret: String,
    pub layout: Layout,
    pub remap: bool,
    pub clipboard: bool,
}
impl Default for Settings {
    fn default()->Self { Self {
        role_server: false, address:"192.168.1.10:24881".into(),
        secret:String::new(), layout:Layout::default(), remap:true, clipboard:true,
    } }
}

pub fn config_path()->Option<std::path::PathBuf> {
    dirs::config_dir().map(|p|p.join("kvm").join("settings.json"))
}
pub fn load()->Settings {
    // Keep existing settings for users upgrading from CrossKey: if no kvm config
    // exists, read the old file and save into the kvm directory on next change.
    let new_path = config_path();
    let legacy_path = dirs::config_dir().map(|p|p.join("crosskey").join("settings.json"));
    new_path.and_then(|p| std::fs::read(p).ok())
        .or_else(|| legacy_path.and_then(|p| std::fs::read(p).ok()))
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
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
    #[test] fn offset_mapping(){
        let a=Screen::new(1920,1080); let b=Screen::new(1440,900);
        let l=Layout {side: Side::Right, offset:100};
        assert!(l.entering(1919,200,a,b));
        assert_eq!(l.entry(1919,200,a,b),(3,100));
        assert!(!l.entering(1919,20,a,b));
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
