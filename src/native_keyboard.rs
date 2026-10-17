//! Quartz keyboard injection with explicit modifier flags. Keep this backend
//! isolated from cursor operations and the input hook to avoid lock inversions.
#![cfg(target_os = "macos")]
use rdev::Key;
use std::{collections::HashSet, ffi::c_void};
#[link(name="ApplicationServices", kind="framework")]
extern "C" {
    fn CGEventCreateKeyboardEvent(source:*const c_void,keycode:u16,key_down:bool)->*mut c_void;
    fn CGEventSetFlags(event:*mut c_void,flags:u64);
    fn CGEventSetType(event:*mut c_void,event_type:u32);
    fn CGEventPost(tap_location:u32,event:*mut c_void);
}
#[link(name="CoreFoundation",kind="framework")]
extern "C" { fn CFRelease(obj:*mut c_void); }

// Carbon/HIToolbox virtual keycodes: physical ANSI-US positions, not glyphs.
fn keycode(key:Key)->Option<u16>{
    use Key::*;
    Some(match key {
        KeyA=>0x00, KeyS=>0x01, KeyD=>0x02, KeyF=>0x03,
        KeyH=>0x04, KeyG=>0x05, KeyZ=>0x06, KeyX=>0x07,
        KeyC=>0x08, KeyV=>0x09, KeyB=>0x0B, KeyQ=>0x0C,
        KeyW=>0x0D, KeyE=>0x0E, KeyR=>0x0F, KeyY=>0x10,
        KeyT=>0x11, Num1=>0x12, Num2=>0x13, Num3=>0x14,
        Num4=>0x15, Num6=>0x16, Num5=>0x17, Equal=>0x18,
        Num9=>0x19, Num7=>0x1A, Minus=>0x1B, Num8=>0x1C,
        Num0=>0x1D, RightBracket=>0x1E, KeyO=>0x1F,
        KeyU=>0x20, LeftBracket=>0x21, KeyI=>0x22, KeyP=>0x23,
        KeyL=>0x25, KeyJ=>0x26, Quote=>0x27, KeyK=>0x28,
        SemiColon=>0x29, BackSlash=>0x2A, Comma=>0x2B,
        Slash=>0x2C, KeyN=>0x2D, KeyM=>0x2E, Dot=>0x2F,
        BackQuote=>0x32, IntlBackslash=>0x0A,
        Return=>0x24, Tab=>0x30, Space=>0x31, Backspace=>0x33,
        Escape=>0x35, MetaLeft=>0x37, MetaRight=>0x36,
        ShiftLeft=>0x38, ShiftRight=>0x3C, CapsLock=>0x39,
        Alt=>0x3A, AltGr=>0x3D, ControlLeft=>0x3B, ControlRight=>0x3E,
        KpDelete=>0x41, KpMultiply=>0x43, KpPlus=>0x45,
        NumLock=>0x47, KpDivide=>0x4B, KpReturn=>0x4C,
        KpMinus=>0x4E, Kp0=>0x52, Kp1=>0x53, Kp2=>0x54,
        Kp3=>0x55, Kp4=>0x56, Kp5=>0x57, Kp6=>0x58,
        Kp7=>0x59, Kp8=>0x5B, Kp9=>0x5C,
        F1=>0x7A, F2=>0x78, F3=>0x63, F4=>0x76,
        F5=>0x60, F6=>0x61, F7=>0x62, F8=>0x64,
        F9=>0x65, F10=>0x6D, F11=>0x67, F12=>0x6F,
        Insert=>0x72, Home=>0x73, PageUp=>0x74, Delete=>0x75,
        End=>0x77, PageDown=>0x79, LeftArrow=>0x7B,
        RightArrow=>0x7C, DownArrow=>0x7D, UpArrow=>0x7E,
        // Windows-only media keys or function/globe must not synthesize an
        // arbitrary macOS keycode (Fn could start Dictation).
        _=>return None,
    })
}
fn flag(key:Key)->u64 {
    match key {
        Key::MetaLeft|Key::MetaRight=>1<<20,
        Key::ShiftLeft|Key::ShiftRight=>1<<17,
        Key::ControlLeft|Key::ControlRight=>1<<18,
        Key::Alt|Key::AltGr=>1<<19,
        _=>0,
    }
}
pub fn is_modifier(key:Key)->bool {flag(key)!=0}
fn flags(held:&HashSet<Key>)->u64{held.iter().fold(0u64,|v,&key|v|flag(key))}
fn post(key:Key,down:bool,mask:u64)->Result<(),String>{
    if matches!(key,Key::Function|Key::Unknown(_)){return Ok(());}
    let code=match keycode(key) {
        Some(v)=>v,
        None=>return Ok(()), // unsupported hardware key, never emit Fn
    };
    let event=unsafe{CGEventCreateKeyboardEvent(std::ptr::null(),code,down)};
    if event.is_null(){return Err(format!("CGEventCreateKeyboardEvent failed for {key:?}"));}
    unsafe {
        if is_modifier(key) {
            // Modifiers must be FlagsChanged, not ordinary KeyDown/KeyUp.
            CGEventSetType(event,12);
        }
        CGEventSetFlags(event,mask);
        CGEventPost(0,event);
        CFRelease(event);
    }
    Ok(())
}
pub fn inject(key:Key,down:bool,held:&HashSet<Key>)->Result<(),String>{
    post(key,down,flags(held))
}
pub fn shortcut(key:Key)->Result<(),String>{
    // A self-contained Command shortcut is used for browser back/forward
    // buttons. It never modifies the receiver's pressed-key state.
    post(Key::MetaLeft,true,1<<20)?;
    post(key,true,1<<20)?;
    post(key,false,1<<20)?;
    post(Key::MetaLeft,false,0)
}
/// Emit Control+Arrow for macOS Spaces and Mission Control.
/// Temporarily lift remapped Command from the Windows Ctrl key so it cannot
/// turn the system shortcut into Command+Control+Arrow. Restore all modifiers
/// afterwards without altering the receiver's logical pressed-key snapshot.
pub fn control_arrow(key:Key,held:&HashSet<Key>)->Result<(),String>{
    if !crate::input::mac_control_arrow(key){
        return Err(format!("Not a macOS Control+Arrow key: {key:?}"));
    }
    let mut current=held.clone();
    let mut lifted=Vec::new();
    for command in [Key::MetaLeft,Key::MetaRight]{
        if current.remove(&command) {
            inject(command,false,&current)?;
            lifted.push(command);
        }
    }
    let added_control=current.insert(Key::ControlLeft);
    if added_control{inject(Key::ControlLeft,true,&current)?;}
    let action=(|| {
        inject(key,true,&current)?;
        inject(key,false,&current)
    })();
    if added_control {
        current.remove(&Key::ControlLeft);
        inject(Key::ControlLeft,false,&current)?;
    }
    for command in lifted {
        current.insert(command);
        inject(command,true,&current)?;
    }
    action
}
pub fn paste()->Result<(),String>{shortcut(Key::KeyV)}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_shortcuts_are_physical_and_keep_flags(){
        assert_eq!(keycode(Key::KeyC),Some(8));
        assert_eq!(keycode(Key::KeyV),Some(9));
        assert_eq!(keycode(Key::MetaRight),Some(0x36));
        assert_eq!(keycode(Key::LeftBracket),Some(0x21));
        assert_eq!(keycode(Key::RightBracket),Some(0x1E));
        let held=[Key::MetaLeft,Key::ShiftRight,Key::Alt].into_iter().collect();
        assert_eq!(flags(&held),(1<<20)|(1<<17)|(1<<19));
        assert_eq!(keycode(Key::Function),None);
        assert_eq!(keycode(Key::Unknown(1)),None);
        assert!(crate::input::mac_control_arrow(Key::UpArrow));
        assert!(!crate::input::mac_control_arrow(Key::KeyZ));
    }
    #[test]
    fn modifier_key_up_keeps_other_side_pressed(){
        let held=[Key::MetaRight].into_iter().collect();
        assert_eq!(flags(&held),1<<20);
    }
}
