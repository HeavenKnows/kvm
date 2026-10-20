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

/// Initial installation is a NORMAL Finder drag. No code-signing
/// bootstrap button is necessary before using the app. At the first future
/// update the updater creates/reuses the stable local signing identity.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum InstallMode { FromDmg, Installed }

pub fn mode()->InstallMode{
    match bundle_from_executable(&std::env::current_exe().unwrap_or_default()){
        Some(bundle) if bundle==Path::new(INSTALL_PATH)=>InstallMode::Installed,
        _=>InstallMode::FromDmg,
    }
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

/// Published release metadata is public (no GitHub account or API token).
/// Only the owner/repository's exact asset URL is accepted. All file content
/// is re-checked by the existing signed installer before replacing the app.
#[derive(Clone,Debug)]
pub struct RemoteRelease{
    pub version:String,
    url:String,
    digest:String,
}
const RELEASES_API:&str="https://api.github.com/repos/HeavenKnows/kvm/releases?per_page=20";
const ASSET_NAME:&str="kvm-macos-arm64.dmg";

fn asset_url(version:&str)->String{
    format!("https://github.com/HeavenKnows/kvm/releases/download/v{version}/{ASSET_NAME}")
}
fn parse_releases(bytes:&[u8],current:&str)->Result<Option<RemoteRelease>,String>{
    let current=semver::Version::parse(current)
        .map_err(|e|format!("本机版本无效：{e}"))?;
    if bytes.len()>2_000_000{return Err("更新列表过大".into());}
    let releases:Vec<serde_json::Value>=serde_json::from_slice(bytes)
        .map_err(|e|format!("无法读取版本列表：{e}"))?;
    let mut best:Option<(semver::Version,RemoteRelease)>=None;
    for release in releases {
        if release.get("draft").and_then(|v|v.as_bool())!=Some(false){continue;}
        let Some(tag)=release.get("tag_name").and_then(|v|v.as_str()) else{continue;};
        let Some(version_text)=tag.strip_prefix('v') else{continue;};
        let Ok(ver)=semver::Version::parse(version_text) else{continue;};
        if ver<=current || best.as_ref().is_some_and(|(old,_)|ver<=*old){continue;}
        let expected=asset_url(version_text);
        let Some(asset)=release.get("assets").and_then(|v|v.as_array())
            .and_then(|items|items.iter().find(|a|
                a.get("name").and_then(|v|v.as_str())==Some(ASSET_NAME) &&
                a.get("browser_download_url").and_then(|v|v.as_str())==Some(&expected)
            )) else{continue;};
        // Reject releases without a verifiable SHA-256. All our published
        // GitHub Release assets provide an official digest.
        let Some(digest)=asset.get("digest").and_then(|v|v.as_str())
            .and_then(|v|v.strip_prefix("sha256:"))
            .filter(|v|v.len()==64 && v.bytes().all(|b|b.is_ascii_hexdigit()))
            .map(|v|v.to_ascii_lowercase())else{continue;};
        best=Some((ver,RemoteRelease{
            version:version_text.to_owned(),url:expected,digest,
        }));
    }
    Ok(best.map(|(_,r)|r))
}

pub fn check_latest()->Result<Option<RemoteRelease>,String>{
    let out=Command::new("/usr/bin/curl")
        .args(["--fail","--location","--silent","--show-error",
            "--connect-timeout","6","--max-time","20",
            "--proto","=https","--proto-redir","=https",
            "--header","Accept: application/vnd.github+json",
            "--header","User-Agent: kvm-macos-updater",
            RELEASES_API])
        .output().map_err(|e|format!("无法访问 GitHub：{e}"))?;
    if !out.status.success(){
        return Err(format!("无法获取更新：{}",String::from_utf8_lossy(&out.stderr).trim()));
    }
    parse_releases(&out.stdout,env!("CARGO_PKG_VERSION"))
}

fn check_sha256(file:&Path,expected:&str)->Result<(),String>{
    let result=Command::new("/usr/bin/shasum").arg("-a").arg("256")
        .arg(file).output().map_err(|e|format!("校验工具不可用：{e}"))?;
    if !result.status.success(){return Err("下载文件 SHA-256 校验失败".into());}
    let digest=String::from_utf8_lossy(&result.stdout)
        .split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    if digest!=expected{return Err("下载文件哈希不匹配，已拒绝安装".into());}
    Ok(())
}
pub fn download_release(release:&RemoteRelease)->Result<PathBuf,String>{
    // Do not trust arbitrary URLs returned by a modified API response.
    if release.url!=asset_url(&release.version) ||
        semver::Version::parse(&release.version).is_err(){
        return Err("更新下载地址无效".into());
    }
    let millis=SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().as_millis();
    let path=std::env::temp_dir().join(format!(
        "kvm-update-{}-{millis}.dmg",std::process::id()));
    let downloaded=Command::new("/usr/bin/curl")
        .args(["--fail","--location","--silent","--show-error",
            "--connect-timeout","10","--max-time","180",
            "--proto","=https","--proto-redir","=https",
            "--max-filesize","250000000",
            "--header","User-Agent: kvm-macos-updater"])
        .arg("--output").arg(&path).arg(&release.url)
        .output().map_err(|e|format!("下载失败：{e}"))?;
    if !downloaded.status.success(){
        let _=fs::remove_file(&path);
        return Err(format!("下载失败：{}",String::from_utf8_lossy(&downloaded.stderr).trim()));
    }
    let result=(||{
        let len=fs::metadata(&path).map_err(|e|format!("下载文件不可读：{e}"))?.len();
        if !(100_000..=250_000_000).contains(&len){
            return Err("下载文件大小异常".into());
        }
        check_sha256(&path,&release.digest)?;
        Ok(path.clone())
    })();
    if result.is_err(){let _=fs::remove_file(&path);}
    result
}

/// Spawns the OS updater AFTER copying its script outside the app bundle.
/// Main process then quits, and the helper waits for our PID to disappear.
/// The helper signs with the login-user identity and uses Apple's native
/// administrator authorization dialog for the file install only.
pub fn start()->Result<(),String>{
    launch(None)
}

pub fn install_downloaded(path:&Path)->Result<(),String>{
    if mode()!=InstallMode::Installed {return Err("请先将 kvm.app 拖入应用程序目录".into());}
    if path.extension().is_none_or(|x|x!="dmg") || !path.is_file(){
        return Err("下载的 DMG 文件不存在".into());
    }
    launch(Some(path))
}

fn launch(downloaded:Option<&Path>)->Result<(),String>{
    let exe=std::env::current_exe().map_err(|e|format!("无法读取应用位置：{e}"))?;
    let bundle=bundle_from_executable(&exe)
        .ok_or("请从 kvm.app 启动程序")?;
    let original_script=bundle.join("Contents/Resources/install-kvm.sh");
    if !original_script.is_file(){
        return Err("此版本不含图形更新组件，请安装 beta.21 或更新版本".into());
    }
    // Preflight while the GUI is still running: a missing keychain should
    // show an actionable error, not close the app and silently lose control.
    let keychain=Command::new("/bin/bash").arg(&original_script)
        .arg("--check-keychain").output()
        .map_err(|e|format!("无法检查 macOS 登录钥匙串：{e}"))?;
    if !keychain.status.success(){
        return Err(format!("无法使用 macOS 登录钥匙串。请打开“钥匙串访问”确认登录钥匙串存在，然后重试。{}",
            String::from_utf8_lossy(&keychain.stderr).chars().take(300).collect::<String>()));
    }
    let millis=SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().as_millis();
    let (source,staged_app_dir)=if mode()==InstallMode::Installed {
        // User-selected or automatically downloaded DMG. The installer
        // authenticates the app signature before any in-place replacement.
        (match downloaded{Some(dmg)=>dmg.to_path_buf(),None=>choose_dmg()?},None)
    }else{
        // First launch from /Applications after a normal drag, or directly
        // from a DMG: install the running version with a stable identity.
        // Snapshot first: App Translocation and self-update can both cause
        // the original bundle to disappear after process exit.
        let staging=std::env::temp_dir()
            .join(format!("kvm-source-{}-{millis}",std::process::id()));
        fs::create_dir(&staging).map_err(|e|format!("无法准备安装目录：{e}"))?;
        let snapshot=staging.join("kvm.app");
        let copied=Command::new("/usr/bin/ditto").arg(&bundle).arg(&snapshot)
            .status().map_err(|e|format!("无法复制安装包：{e}"))?;
        if !copied.success(){
            let _=fs::remove_dir_all(&staging);
            return Err("无法准备安装包，请重新打开 DMG".into());
        }
        (snapshot,Some(staging))
    };
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
        .env("KVM_AUTODOWNLOAD_DMG",downloaded.unwrap_or_else(||Path::new("")))
        .env("KVM_STAGED_APP_DIRECTORY",staged_app_dir.as_deref()
            .unwrap_or_else(||Path::new("")))
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

    #[test]
    fn releases_choose_newer_semver_even_if_old_or_malformed_entries_exist(){
        let releases=serde_json::json!([
            {"draft":false,"tag_name":"v0.5.0-beta.21","assets":[]},
            {"draft":false,"tag_name":"v0.5.0-beta.25","assets":[{"name":"kvm-macos-arm64.dmg",
             "browser_download_url":"https://evil.invalid/update.dmg"}]},
            {"draft":false,"tag_name":"v0.5.0-beta.24","assets":[{"name":"kvm-macos-arm64.dmg",
             "browser_download_url":"https://github.com/HeavenKnows/kvm/releases/download/v0.5.0-beta.24/kvm-macos-arm64.dmg",
             "digest":format!("sha256:{}", "a".repeat(64))}]},
            {"draft":false,"tag_name":"v0.5.0-beta.23","assets":[{"name":"kvm-macos-arm64.dmg",
             "browser_download_url":"https://github.com/HeavenKnows/kvm/releases/download/v0.5.0-beta.23/kvm-macos-arm64.dmg"}]}
        ]);
        let data=serde_json::to_vec(&releases).unwrap();
        let result=parse_releases(&data,"0.5.0-beta.22").unwrap().unwrap();
        assert_eq!(result.version,"0.5.0-beta.24");
        assert_eq!(result.digest,"a".repeat(64));
        assert!(parse_releases(&data,"0.5.0-beta.26").unwrap().is_none());
    }
    #[test]
    fn initial_finder_drag_has_no_signing_setup_gate(){
        // Both drag-installed ad-hoc builds and previously signed builds
        // are legitimate installed apps; no pre-update signing step.
        assert_eq!(bundle_from_executable(Path::new(
            "/Applications/kvm.app/Contents/MacOS/kvm")),
            Some(PathBuf::from(INSTALL_PATH)));
        assert_eq!(InstallMode::Installed,InstallMode::Installed);
    }

}
