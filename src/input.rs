//! Explicit, user-visible shortcut rules. No built-in runtime remapping layer.
//! Unmatched keys (and same-OS sessions) always pass through unchanged.
use crate::model::Platform;
use rdev::Key;
use serde::{Deserialize,Serialize};
use std::collections::{HashMap,HashSet};

const CTRL:u8=1;
const SHIFT:u8=2;
const ALT:u8=4;
const META:u8=8;

#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct ShortcutMapping {
    pub from_os:Platform,
    pub source:String,
    pub target:String,
    pub enabled:bool,
}
impl ShortcutMapping {
    pub fn new(from_os:Platform,source:&str,target:&str)->Self{
        Self{from_os,source:source.into(),target:target.into(),enabled:true}
    }
    pub fn validate(&self)->Result<(),String>{
        let _=Chord::parse(&self.source)?;
        let _=Chord::parse(&self.target)?;
        Ok(())
    }
}

/// Visible starter entries saved into settings.json. Deleting all entries
/// keeps the table empty; there is NEVER an invisible fallback mapping.
pub fn presets()->Vec<ShortcutMapping>{
    use Platform::*;
    [
        (Windows,"Ctrl+C","Command+C"),
        (Windows,"Ctrl+V","Command+V"),
        (Windows,"Ctrl+X","Command+X"),
        (Windows,"Ctrl+Z","Command+Z"),
        (Windows,"Ctrl+A","Command+A"),
        (Windows,"Ctrl+S","Command+S"),
        (Windows,"Ctrl+F","Command+F"),
        (Windows,"Ctrl+Shift+Z","Command+Shift+Z"),
        (Windows,"Ctrl+Left","Ctrl+Left"),
        (Windows,"Ctrl+Right","Ctrl+Right"),
        (Windows,"Ctrl+Up","Ctrl+Up"),
        (Windows,"Ctrl+Down","Ctrl+Down"),
        (Windows,"Alt+Tab","Command+Tab"),
        (Mac,"Command+C","Ctrl+C"),
        (Mac,"Command+V","Ctrl+V"),
        (Mac,"Command+X","Ctrl+X"),
        (Mac,"Command+Z","Ctrl+Z"),
        (Mac,"Command+A","Ctrl+A"),
    ].into_iter().map(|(os,from,to)|ShortcutMapping::new(os,from,to)).collect()
}

#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash,Serialize,Deserialize)]
pub struct Chord {pub mods:u8,pub key:Key}
impl Chord {
    pub fn parse(value:&str)->Result<Self,String>{
        if value.len()>100 || value.trim().is_empty(){
            return Err("Shortcut is empty or too long".into());
        }
        let parts:Vec<_>=value.split('+').map(str::trim).collect();
        if parts.iter().any(|x|x.is_empty()){return Err("Empty shortcut component".into());}
        let mut mods=0u8;
        for word in &parts[..parts.len()-1]{
            let bit=match word.to_ascii_lowercase().as_str(){
                "ctrl"|"control"=>CTRL,
                "shift"=>SHIFT,
                "alt"|"option"=>ALT,
                "cmd"|"command"|"meta"|"win"|"windows"=>META,
                _=>return Err(format!("Unknown modifier: {word}")),
            };
            if mods & bit !=0{return Err(format!("Repeated modifier: {word}"));}
            mods|=bit;
        }
        let name=parts.last().unwrap().to_ascii_uppercase();
        let key=match name.as_str(){
            "A"=>Key::KeyA,"B"=>Key::KeyB,"C"=>Key::KeyC,"D"=>Key::KeyD,
            "E"=>Key::KeyE,"F"=>Key::KeyF,"G"=>Key::KeyG,"H"=>Key::KeyH,
            "I"=>Key::KeyI,"J"=>Key::KeyJ,"K"=>Key::KeyK,"L"=>Key::KeyL,
            "M"=>Key::KeyM,"N"=>Key::KeyN,"O"=>Key::KeyO,"P"=>Key::KeyP,
            "Q"=>Key::KeyQ,"R"=>Key::KeyR,"S"=>Key::KeyS,"T"=>Key::KeyT,
            "U"=>Key::KeyU,"V"=>Key::KeyV,"W"=>Key::KeyW,"X"=>Key::KeyX,
            "Y"=>Key::KeyY,"Z"=>Key::KeyZ,
            "0"=>Key::Num0,"1"=>Key::Num1,"2"=>Key::Num2,
            "3"=>Key::Num3,"4"=>Key::Num4,"5"=>Key::Num5,
            "6"=>Key::Num6,"7"=>Key::Num7,"8"=>Key::Num8,"9"=>Key::Num9,
            "LEFT"|"ARROWLEFT"=>Key::LeftArrow,
            "RIGHT"|"ARROWRIGHT"=>Key::RightArrow,
            "UP"|"ARROWUP"=>Key::UpArrow,
            "DOWN"|"ARROWDOWN"=>Key::DownArrow,
            "TAB"=>Key::Tab,"ENTER"|"RETURN"=>Key::Return,
            "SPACE"=>Key::Space,"ESC"|"ESCAPE"=>Key::Escape,
            "BACKSPACE"=>Key::Backspace,"DELETE"|"DEL"=>Key::Delete,
            "HOME"=>Key::Home,"END"=>Key::End,
            "PAGEUP"|"PGUP"=>Key::PageUp,
            "PAGEDOWN"|"PGDN"=>Key::PageDown,
            "INSERT"|"INS"=>Key::Insert,
            "F1"=>Key::F1,"F2"=>Key::F2,"F3"=>Key::F3,
            "F4"=>Key::F4,"F5"=>Key::F5,"F6"=>Key::F6,
            "F7"=>Key::F7,"F8"=>Key::F8,"F9"=>Key::F9,
            "F10"=>Key::F10,"F11"=>Key::F11,
            "MINUS"=>Key::Minus,"EQUAL"=>Key::Equal,
            "LEFTBRACKET"=>Key::LeftBracket,"RIGHTBRACKET"=>Key::RightBracket,
            "SLASH"=>Key::Slash,"BACKSLASH"=>Key::BackSlash,
            "COMMA"=>Key::Comma,"DOT"|"PERIOD"=>Key::Dot,
            "SEMICOLON"=>Key::SemiColon,"QUOTE"=>Key::Quote,
            "BACKQUOTE"|"GRAVE"=>Key::BackQuote,
            "F12"=>return Err("F12 is reserved for emergency return".into()),
            _=>return Err(format!("Unknown key: {name}")),
        };
        // F12 is a reserved emergency-return key and is intentionally not
        // accepted as either source or target of a user-defined rule.
        Ok(Self{mods,key})
    }
    pub fn from_held(key:Key,pressed:&HashSet<Key>)->Self{
        let mut mods=0;
        for k in pressed {
            mods|=match k {
                Key::ControlLeft|Key::ControlRight=>CTRL,
                Key::ShiftLeft|Key::ShiftRight=>SHIFT,
                Key::Alt|Key::AltGr=>ALT,
                Key::MetaLeft|Key::MetaRight=>META,
                _=>0
            };
        }
        Self{mods,key}
    }
    pub fn modifier_keys(self)->Vec<Key>{
        let mut keys=Vec::new();
        if self.mods & CTRL!=0{keys.push(Key::ControlLeft);}
        if self.mods & SHIFT!=0{keys.push(Key::ShiftLeft);}
        if self.mods & ALT!=0{keys.push(Key::Alt);}
        if self.mods & META!=0{keys.push(Key::MetaLeft);}
        keys
    }
    /// Execution backend only: a Control+Arrow TARGET rule uses the macOS
    /// HID event source so Mission Control can recognize the synthetic chord.
    /// This never creates an implicit mapping.
    pub fn is_mac_system_control_arrow(self)->bool{
        self.mods==CTRL && matches!(self.key,
            Key::LeftArrow|Key::RightArrow|Key::UpArrow|Key::DownArrow)
    }
    pub fn is_paste(self,os:Platform)->bool{
        self.key==Key::KeyV && self.native_primary(os)
    }
    pub fn is_copy(self,os:Platform)->bool{
        matches!(self.key,Key::KeyC|Key::KeyX) && self.native_primary(os)
    }
    fn native_primary(self,os:Platform)->bool{
        self.mods==match os{Platform::Windows=>CTRL,Platform::Mac=>META}
    }
}

#[derive(Default)]
pub struct ShortcutTable {rules:HashMap<(bool,Chord),Chord>,errors:usize}
impl ShortcutTable {
    pub fn compile(mappings:&[ShortcutMapping])->Self{
        let mut table=Self::default();
        for rule in mappings.iter().filter(|x|x.enabled){
            match (Chord::parse(&rule.source),Chord::parse(&rule.target)){
                (Ok(from),Ok(to))=>{
                    let src=rule.from_os==Platform::Windows;
                    // Earliest row wins; duplicate later rows never shadow it.
                    if let std::collections::hash_map::Entry::Vacant(slot)=table.rules.entry((src,from)){
                        slot.insert(to);
                    }else{
                        table.errors+=1;
                    }
                }
                _=>table.errors+=1,
            }
        }
        table
    }
    pub fn find(&self,source:Platform,target:Platform,key:Key,pressed:&HashSet<Key>)->Option<Chord>{
        if source==target{return None;}
        self.rules.get(&(source==Platform::Windows,Chord::from_held(key,pressed))).copied()
    }
    pub fn len(&self)->usize{self.rules.len()}
    pub fn errors(&self)->usize{self.errors}
}
pub fn primary(k:Key,p:Platform)->bool{
    match p{
        Platform::Windows=>matches!(k,Key::ControlLeft|Key::ControlRight),
        Platform::Mac=>matches!(k,Key::MetaLeft|Key::MetaRight)
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn presets_are_explicit_and_editable(){
        let p=presets();
        assert!(p.iter().all(|x|x.validate().is_ok()));
        assert!(p.iter().any(|r|r.source=="Ctrl+Left" && r.target=="Ctrl+Left"));
        let table=ShortcutTable::compile(&p);
        assert_eq!(table.errors(),0);
        let held=[Key::ControlLeft].into_iter().collect();
        assert_eq!(table.find(Platform::Windows,Platform::Mac,Key::KeyC,&held),
            Some(Chord::parse("Command+C").unwrap()));
        assert_eq!(table.find(Platform::Windows,Platform::Windows,Key::KeyC,&held),None);
        for (key,target) in [
            (Key::LeftArrow,"Ctrl+Left"),(Key::RightArrow,"Ctrl+Right"),
            (Key::UpArrow,"Ctrl+Up"),(Key::DownArrow,"Ctrl+Down")
        ]{
            let mapped=table.find(Platform::Windows,Platform::Mac,key,&held);
            assert_eq!(mapped,Some(Chord::parse(target).unwrap()));
            assert!(mapped.unwrap().is_mac_system_control_arrow());
        }
        assert!(!Chord::parse("Command+Left").unwrap().is_mac_system_control_arrow());
        let empty=ShortcutTable::compile(&[]);
        assert!(empty.find(Platform::Windows,Platform::Mac,Key::KeyC,&held).is_none());
    }
    #[test]
    fn edits_delete_and_disabled_rows_have_no_fallback(){
        let held=[Key::ControlRight].into_iter().collect();
        let mut p=presets();
        p.retain(|r|r.source!="Ctrl+C");
        assert_eq!(ShortcutTable::compile(&p).find(Platform::Windows,Platform::Mac,Key::KeyC,&held),None);
        let mut disabled=ShortcutMapping::new(Platform::Windows,"Ctrl+C","Command+C");
        disabled.enabled=false;
        assert_eq!(ShortcutTable::compile(&[disabled]).len(),0);
    }
    #[test]
    fn chord_validates_and_supports_chained_modifiers(){
        let x=Chord::parse("Ctrl+Shift+T").unwrap();
        assert_eq!(x.key,Key::KeyT);
        assert_eq!(x.modifier_keys(),vec![Key::ControlLeft,Key::ShiftLeft]);
        assert!(Chord::parse("Ctrl+Control+C").is_err());
        assert!(Chord::parse("Ctrl+F12").is_err());
        assert!(Chord::parse("Ctrl+NOPE").is_err());
        assert!(Chord::parse("Ctrl++").is_err());
        assert!(Chord::parse("Command+V").unwrap().is_paste(Platform::Mac));
        assert!(!Chord::parse("Ctrl+V").unwrap().is_paste(Platform::Mac));
    }
}
