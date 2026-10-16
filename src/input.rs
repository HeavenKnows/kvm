//! Shortcut translation is semantic: a Windows Ctrl chord on a Mac becomes a Command chord.
//! Hardware key layout, IME and OS-reserved shortcuts still need on-device tests.
use crate::model::Platform;
use rdev::Key;

pub fn primary(k:Key,p:Platform)->bool {
    match p {Platform::Windows => matches!(k,Key::ControlLeft|Key::ControlRight),
              Platform::Mac => matches!(k,Key::MetaLeft|Key::MetaRight)}
}
pub fn remap(k:Key,source:Platform,target:Platform,enabled:bool)->Key {
    if !enabled || source==target {return k;}
    match (source,target,k) {
        (Platform::Windows,Platform::Mac,Key::ControlLeft)=>Key::MetaLeft,
        (Platform::Windows,Platform::Mac,Key::ControlRight)=>Key::MetaRight,
        (Platform::Mac,Platform::Windows,Key::MetaLeft)=>Key::ControlLeft,
        (Platform::Mac,Platform::Windows,Key::MetaRight)=>Key::ControlRight,
        _=>k,
    }
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn translate_both_directions(){
        assert_eq!(remap(Key::ControlLeft,Platform::Windows,Platform::Mac,true),Key::MetaLeft);
        assert_eq!(remap(Key::MetaRight,Platform::Mac,Platform::Windows,true),Key::ControlRight);
        assert_eq!(remap(Key::Alt,Platform::Windows,Platform::Mac,true),Key::Alt);
        assert_eq!(remap(Key::ControlLeft,Platform::Windows,Platform::Mac,false),Key::ControlLeft);
    }
}
