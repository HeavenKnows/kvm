#![cfg(target_os="macos")]
//! macOS GUI installer/updater. Users select a downloaded DMG in the
//! ALREADY INSTALLED application; never double-click an unnotarized .command,
//! overwrite the signed app via Finder, or paste Terminal commands.
use std::{
    fs::{self,OpenOptions},
    io::Write,
    path::{Path,PathBuf},
    process::{Command,Stdio},
    time::{SystemTime,UNIX_EPOCH},
};

const INSTALL_PATH:&str="/Applications/kvm.app";

fn bundle_from_executable(path:&Path)->Option<PathBuf>{
    let macos=path.parent()?;
    if macos.file_name()? != "MacOS"{return None;}
    let contents=macos.parent()?;
    if contents.file_name()? != "Contents"{return None;}
    let bundle=contents.parent()?;
    if bundle.extension()? != "app"{return None;}
    Some(bundle.to_path_buf())
}

pub fn installed()->bool{
    bundle_from_executable(&std::env::current_exe().unwrap_or_default())
        .is_some_and(|p|p==Path::new(INSTALL_PATH))
}

/// A native file picker. No Finder execution of the downloaded app or script.
/// It intentionally runs on a user click (a modal picker is expected).
fn choose_dmg()->Result<PathBuf,String>{
    let output=Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg("POSIX path of (choose file with prompt \"选择新版 kvm DMG\" of type {\"dmg\"})")
        .output().map_err(|e|format!("文件选择器无法启动：{e}"))?;
    if !output.status.success(){
        return Err("已取消选择".into());
    }
    let path=PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    if path.extension().is_none_or(|ext|ext!="dmg") || !path.is_file(){
        return Err("请选择下载的 .dmg 文件".into());
    }
    Ok(path)
}

/// Spawns the OS updater AFTER copying its script outside the app bundle.
/// Main process then quits, and the helper waits for our PID to disappear.
/// The helper signs with the login-user identity and uses Apple's native
/// administrator authorization dialog for the file install only.
pub fn start()->Result<(),String>{
    let exe=std::env::current_exe().map_err(|e|format!("无法读取应用位置：{e}"))?;
    let bundle=bundle_from_executable(&exe)
        .ok_or("请从 kvm.app 启动程序")?;
    let original_script=bundle.join("Contents/Resources/install-kvm.sh");
    if !original_script.is_file(){
        return Err("此版本不含图形更新组件，请安装 beta.21 或更新版本".into());
    }
    let source=if bundle==Path::new(INSTALL_PATH) {
        choose_dmg()?
    }else{
        // Running the app directly from the newly downloaded DMG:
        // one click installs THIS version without any file chooser.
        bundle
    };
    let millis=SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().as_millis();
    let script_copy=std::env::temp_dir()
        .join(format!("kvm-updater-{}-{millis}.sh",std::process::id()));
    let script=fs::read(&original_script).map_err(|e|format!("无法读取安装组件：{e}"))?;
    {
        let mut out=OpenOptions::new().create_new(true).write(true)
            .open(&script_copy).map_err(|e|format!("无法准备安装程序：{e}"))?;
        out.write_all(&script).map_err(|e|format!("无法复制安装程序：{e}"))?;
    }
    let logs=dirs::home_dir().ok_or("无法找到用户目录")?.join("Library/Logs");
    fs::create_dir_all(&logs).map_err(|e|format!("无法建立日志目录：{e}"))?;
    let log=OpenOptions::new().create(true).append(true)
        .open(logs.join("kvm-updater.log")).map_err(|e|format!("无法写入更新日志：{e}"))?;
    let err_log=log.try_clone().map_err(|e|e.to_string())?;
    Command::new("/bin/bash")
        .arg(&script_copy).arg("--gui").arg("--wait-pid")
        .arg(std::process::id().to_string()).arg(&source)
        .env("KVM_INSTALL_SCRIPT_COPY","1")
        .stdin(Stdio::null()).stdout(Stdio::from(log)).stderr(Stdio::from(err_log))
        .spawn().map_err(|e|format!("更新程序无法启动：{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn installed_app_detection_is_path_and_role_independent(){
        assert_eq!(bundle_from_executable(Path::new(
            "/Applications/kvm.app/Contents/MacOS/kvm")).unwrap(),
            PathBuf::from(INSTALL_PATH));
        assert_eq!(bundle_from_executable(Path::new(
            "/Volumes/kvm 1/kvm.app/Contents/MacOS/kvm")).unwrap(),
            PathBuf::from("/Volumes/kvm 1/kvm.app"));
        assert!(bundle_from_executable(Path::new("/tmp/kvm")).is_none());
    }
}
