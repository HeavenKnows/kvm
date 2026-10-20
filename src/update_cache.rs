//! Shared update-package storage. Only files inside our own cache directory
//! are created/overwritten; selecting Downloads must not overwrite unrelated
//! files named kvm.exe or kvm-macos-arm64.dmg.
use std::path::{Path,PathBuf};

pub fn cache_directory(root:&str)->Result<PathBuf,String>{
    let parent=if root.trim().is_empty(){std::env::temp_dir()}else{PathBuf::from(root)};
    if !parent.is_absolute(){return Err("下载目录必须是绝对路径".into());}
    Ok(parent.join("kvm-updates"))
}
pub fn package_path(root:&str,name:&str)->Result<PathBuf,String>{
    if name!="kvm.exe" && name!="kvm-macos-arm64.dmg"{
        return Err("非法更新包文件名".into());
    }
    Ok(cache_directory(root)?.join(name))
}
pub fn prepare(root:&str,name:&str)->Result<PathBuf,String>{
    let path=package_path(root,name)?;
    std::fs::create_dir_all(path.parent().ok_or("下载目录无效")?)
        .map_err(|e|format!("无法创建更新缓存目录：{e}"))?;
    Ok(path)
}
pub fn replace_verified(part:&Path,dest:&Path)->Result<(),String>{
    // Staged file MUST be verified before this function is called. Windows
    // cannot atomically rename over an existing file, unlike Unix.
    if dest.exists(){
        std::fs::remove_file(dest).map_err(|e|format!("无法覆盖旧更新包：{e}"))?;
    }
    std::fs::rename(part,dest).map_err(|e|format!("无法保存更新包：{e}"))
}
#[cfg(target_os="macos")]
pub fn choose_folder()->Result<Option<PathBuf>,String>{
    let out=std::process::Command::new("/usr/bin/osascript")
        .args(["-e","POSIX path of (choose folder with prompt \"选择 KVM 更新下载目录\")"])
        .output().map_err(|e|format!("无法打开目录选择器：{e}"))?;
    if !out.status.success(){return Ok(None);}
    let path=String::from_utf8(out.stdout)
        .map_err(|_|"目录包含无效字符".to_owned())?;
    let path=PathBuf::from(path.trim());
    if !path.is_dir(){return Err("所选目录不存在".into());}
    Ok(Some(path))
}
#[cfg(target_os="windows")]
pub fn choose_folder()->Result<Option<PathBuf>,String>{
    use std::{ffi::c_void,os::windows::ffi::OsStringExt};
    #[repr(C)]
    struct BrowseInfo {
        hwnd_owner:*mut c_void,
        pidl_root:*mut c_void,
        display_name:*mut u16,
        title:*const u16,
        flags:u32,
        callback:*mut c_void,
        param:isize,
        image:i32,
    }
    #[link(name="shell32")]
    extern "system" {
        fn SHBrowseForFolderW(info:*const BrowseInfo)->*mut c_void;
        fn SHGetPathFromIDListW(pidl:*mut c_void,folder:*mut u16)->i32;
    }
    #[link(name="ole32")]
    extern "system" {fn CoTaskMemFree(p:*mut c_void);}
    let title:Vec<u16>="Choose kvm update download folder\0".encode_utf16().collect();
    let mut selected=[0u16;32768];
    let info=BrowseInfo{
        hwnd_owner:std::ptr::null_mut(),pidl_root:std::ptr::null_mut(),
        display_name:std::ptr::null_mut(),title:title.as_ptr(),
        flags:0x1 | 0x10 | 0x40,callback:std::ptr::null_mut(),param:0,image:0
    };
    let pidl=unsafe{SHBrowseForFolderW(&info)};
    if pidl.is_null(){return Ok(None);}
    let success=unsafe{SHGetPathFromIDListW(pidl,selected.as_mut_ptr())}!=0;
    unsafe{CoTaskMemFree(pidl);}
    if !success{return Err("无法取得选中的下载目录".into());}
    let length=selected.iter().position(|x|*x==0).unwrap_or(selected.len());
    let path=PathBuf::from(std::ffi::OsString::from_wide(&selected[..length]));
    if !path.is_dir(){return Err("所选下载目录不存在".into());}
    Ok(Some(path))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn defaults_and_safe_namespace(){
        assert_eq!(cache_directory("").unwrap(),std::env::temp_dir().join("kvm-updates"));
        let parent=std::env::temp_dir().join("测试 Cache With Spaces");
        assert_eq!(package_path(parent.to_str().unwrap(),"kvm.exe").unwrap(),
            parent.join("kvm-updates").join("kvm.exe"));
        assert!(package_path("relative","kvm.exe").is_err());
        assert!(package_path("","../../other.exe").is_err());
    }
    #[test]
    fn verified_replacement_does_not_accumulate_versions(){
        let base=std::env::temp_dir().join(format!("kvm-cache-test-{}-{:?}",
            std::process::id(),std::thread::current().id()));
        std::fs::create_dir_all(&base).unwrap();
        let dst=base.join("kvm.exe");
        let tmp=base.join("kvm.exe.part");
        std::fs::write(&dst,b"old").unwrap();
        std::fs::write(&tmp,b"new").unwrap();
        replace_verified(&tmp,&dst).unwrap();
        assert_eq!(std::fs::read(&dst).unwrap(),b"new");
        assert!(!tmp.exists());
        assert_eq!(std::fs::read_dir(&base).unwrap().count(),1);
        let _=std::fs::remove_dir_all(base);
    }
}
