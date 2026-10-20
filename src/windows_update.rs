#![cfg(target_os="windows")]
//! Windows in-app updater. GitHub Releases is the ONLY allowed download source.
//! New EXEs are SHA-256 verified before use; a detached copy of the old EXE
//! waits for the old process to exit, then does rename/rollback/relaunch.
use std::{
    ffi::{c_void,OsStr,OsString},
    fs::{self,File,OpenOptions},
    io::{Read,Write},
    os::windows::ffi::OsStrExt,
    path::{Path,PathBuf},
    process::{Command,Stdio},
    time::{SystemTime,UNIX_EPOCH},
};
use sha2::{Digest,Sha256};

const API:&str="https://api.github.com/repos/HeavenKnows/kvm/releases?per_page=20";
const NAME:&str="kvm.exe";
const MAX_BYTES:u64=250_000_000;

#[derive(Debug,Clone)]
pub struct Release{
    pub version:String,
    url:String,
    pub digest:String,
}
fn expected_url(version:&str)->String{
    format!("https://github.com/HeavenKnows/kvm/releases/download/v{version}/{NAME}")
}
fn valid_digest(hash:&str)->bool{
    hash.len()==64 && hash.bytes().all(|b|b.is_ascii_hexdigit())
}
fn parse_releases(data:&[u8],current:&str)->Result<Option<Release>,String>{
    if data.len()>2_000_000{return Err("更新列表过大".into());}
    let current=semver::Version::parse(current)
        .map_err(|e|format!("版本无效：{e}"))?;
    let items:Vec<serde_json::Value>=serde_json::from_slice(data)
        .map_err(|e|format!("更新列表无效：{e}"))?;
    let mut best=None::<(semver::Version,Release)>;
    for item in items{
        if item.get("draft").and_then(|v|v.as_bool())!=Some(false){continue;}
        let Some(tag)=item.get("tag_name").and_then(|v|v.as_str())else{continue;};
        let Some(text)=tag.strip_prefix('v')else{continue;};
        let Ok(version)=semver::Version::parse(text)else{continue;};
        if version<=current || best.as_ref().is_some_and(|(v,_)|version<=*v){continue;}
        let url=expected_url(text);
        let Some(asset)=item.get("assets").and_then(|v|v.as_array())
            .and_then(|assets|assets.iter().find(|a|
                a.get("name").and_then(|v|v.as_str())==Some(NAME)
                && a.get("browser_download_url").and_then(|v|v.as_str())==Some(url.as_str())
            ))else{continue;};
        // The digest is mandatory for unsigned Windows builds. An API
        // response without it will not be used as an update source.
        let Some(digest)=asset.get("digest").and_then(|v|v.as_str())
            .and_then(|s|s.strip_prefix("sha256:"))
            .filter(|s|valid_digest(s))
            .map(str::to_ascii_lowercase)else{continue;};
        best=Some((version,Release{version:text.to_owned(),url,digest}));
    }
    Ok(best.map(|(_,r)|r))
}

pub fn check_latest()->Result<Option<Release>,String>{
    let out=Command::new("curl.exe")
        .args(["--fail","--location","--silent","--show-error",
            "--connect-timeout","6","--max-time","20",
            "--proto","=https","--proto-redir","=https",
            "--header","Accept: application/vnd.github+json",
            "--header","User-Agent: kvm-windows-updater",API])
        .creation_flags_no_window()
        .output().map_err(|e|format!("无法运行 Windows curl：{e}"))?;
    if !out.status.success(){
        return Err(format!("检查更新失败：{}",String::from_utf8_lossy(&out.stderr).trim()));
    }
    parse_releases(&out.stdout,env!("CARGO_PKG_VERSION"))
}
trait NoWindow {
    fn creation_flags_no_window(&mut self)->&mut Self;
}
impl NoWindow for Command {
    fn creation_flags_no_window(&mut self)->&mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x08000000) // CREATE_NO_WINDOW
    }
}
fn sha256(path:&Path)->Result<String,String>{
    let mut file=File::open(path).map_err(|e|format!("无法读取下载文件：{e}"))?;
    let mut sha=Sha256::new();
    let mut chunk=[0u8;64*1024];
    loop {
        let n=file.read(&mut chunk).map_err(|e|e.to_string())?;
        if n==0{break;}
        sha.update(&chunk[..n]);
    }
    Ok(format!("{:x}",sha.finalize()))
}
fn verify_exe(path:&Path,digest:&str)->Result<(),String>{
    if !valid_digest(digest){return Err("SHA-256 格式无效".into());}
    let size=fs::metadata(path).map_err(|e|format!("文件不存在：{e}"))?.len();
    if !(100_000..=MAX_BYTES).contains(&size){return Err("更新包大小异常".into());}
    let mut file=File::open(path).map_err(|e|e.to_string())?;
    let mut header=[0u8;2];
    file.read_exact(&mut header).map_err(|e|e.to_string())?;
    if &header!=b"MZ"{return Err("下载文件不是 Windows EXE".into());}
    if sha256(path)?!=digest.to_ascii_lowercase(){
        return Err("SHA-256 不匹配，已拒绝安装".into());
    }
    Ok(())
}
fn stamp()->u128{
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

pub fn download_release(release:&Release,folder:&str)->Result<PathBuf,String>{
    if release.url!=expected_url(&release.version)
       ||semver::Version::parse(&release.version).is_err()
       ||!valid_digest(&release.digest){
        return Err("更新包地址或校验信息无效".into());
    }
    let path=crate::update_cache::prepare(folder,NAME)?;
    if path.is_file() && verify_exe(&path,&release.digest).is_ok(){
        return Ok(path); // Reuse exactly matching verified installer.
    }
    let part=path.with_extension("exe.part");
    if part.exists(){fs::remove_file(&part).map_err(|e|format!("无法清理下载临时文件：{e}"))?;}
    let result=(||{
        let out=Command::new("curl.exe")
            .args(["--fail","--location","--silent","--show-error",
                "--connect-timeout","10","--max-time","240",
                "--proto","=https","--proto-redir","=https",
                "--max-filesize","250000000",
                "--header","User-Agent: kvm-windows-updater"])
            .arg("--output").arg(&part).arg(&release.url)
            .creation_flags_no_window().output()
            .map_err(|e|format!("Windows 下载工具无法启动：{e}"))?;
        if !out.status.success(){
            return Err(format!("下载失败：{}",String::from_utf8_lossy(&out.stderr).trim()));
        }
        verify_exe(&part,&release.digest)?;
        crate::update_cache::replace_verified(&part,&path)?;
        Ok(path.clone())
    })();
    if result.is_err(){let _=fs::remove_file(&part);}
    result
}

#[link(name="kernel32")]
extern "system" {
    fn OpenProcess(access:u32,inherit:i32,pid:u32)->*mut c_void;
    fn WaitForSingleObject(handle:*mut c_void,millis:u32)->u32;
    fn CloseHandle(handle:*mut c_void)->i32;
}
#[link(name="shell32")]
extern "system" {
    fn ShellExecuteW(hwnd:*mut c_void,verb:*const u16,file:*const u16,
        params:*const u16,dir:*const u16,show:i32)->isize;
}
fn wide(s:&OsStr)->Vec<u16>{
    s.encode_wide().chain(std::iter::once(0)).collect()
}
fn wait_parent(pid:u32)->Result<(),String>{
    // SYNCHRONIZE access only: wait without requiring Administrator.
    let handle=unsafe{OpenProcess(0x00100000,0,pid)};
    if handle.is_null(){return Ok(());} // parent exited before helper started
    let status=unsafe{WaitForSingleObject(handle,60_000)};
    unsafe{CloseHandle(handle);}
    if status!=0{return Err("旧版 kvm 未在 60 秒内退出".into());}
    Ok(())
}
fn write_status(message:&str){
    let Some(home)=dirs::data_local_dir()else{return;};
    let dir=home.join("kvm");
    if fs::create_dir_all(&dir).is_ok(){
        let _=fs::write(dir.join("update-result.txt"),message);
    }
}
pub fn take_status()->Option<String>{
    let path=dirs::data_local_dir()?.join("kvm").join("update-result.txt");
    let text=fs::read_to_string(&path).ok()?;
    let _=fs::remove_file(&path);
    Some(text.chars().take(500).collect())
}

fn replace_exe(target:&Path,source:&Path,digest:&str)->Result<(),String>{
    verify_exe(source,digest)?;
    let file=target.file_name().ok_or("找不到程序文件名")?.to_string_lossy();
    if !target.exists() || !target.is_file(){return Err("旧版 EXE 路径无效".into());}
    let next=target.with_file_name(format!("{file}.kvm-next"));
    let backup=target.with_file_name(format!("{file}.kvm-backup"));
    // Never clobber potentially recoverable files left by an interrupted
    // update. The user can inspect/remove them manually.
    if next.exists() || backup.exists(){
        return Err("发现未完成的旧更新文件，已取消以保护原程序".into());
    }
    fs::copy(source,&next).map_err(|e|format!("无权限复制新版到程序目录：{e}"))?;
    if let Err(e)=verify_exe(&next,digest){
        let _=fs::remove_file(&next);
        return Err(e);
    }
    if let Err(e)=fs::rename(target,&backup){
        let _=fs::remove_file(&next);
        return Err(format!("无法备份旧版 EXE：{e}"));
    }
    if let Err(e)=fs::rename(&next,target){
        let rollback=fs::rename(&backup,target);
        return Err(format!("无法安装新版：{e}；恢复旧版：{rollback:?}"));
    }
    Ok(())
}
fn restore_old(target:&Path){
    let Some(file)=target.file_name()else{return;};
    let backup=target.with_file_name(format!("{}.kvm-backup",file.to_string_lossy()));
    if backup.exists(){
        let _=fs::remove_file(target);
        let _=fs::rename(backup,target);
    }
}
fn finalize_install(pid:u32,source:&Path,target:&Path,digest:&str)->Result<(),String>{
    wait_parent(pid)?;
    if let Err(e)=replace_exe(target,source,digest){
        // Always attempt to bring the old app back on any update failure.
        // Users must never be left with no running tray after an error.
        write_status(&format!("Windows 更新失败：{e}"));
        if target.exists(){let _=Command::new(target).spawn();}
        return Err(e);
    }
    // Write status BEFORE launching the new executable: it can boot before
    // the helper returns, so anything written afterward could be missed.
    write_status("Windows kvm 已更新成功 / Update installed successfully");
    let result=Command::new(target).spawn();
    if let Err(e)=result{
        restore_old(target);
        write_status(&format!("更新失败，已尝试恢复旧版：{e}"));
        let _=Command::new(target).spawn();
        return Err(format!("新版无法启动，尝试恢复旧版：{e}"));
    }
    let backup=target.with_file_name(format!("{}.kvm-backup",
        target.file_name().ok_or("程序文件名无效")?.to_string_lossy()));
    let _=fs::remove_file(backup);
    // Keep the verified package for retries and offline reuse; the next
    // newer release will replace it under the same cache filename.
    Ok(())
}

fn install_args(args:&[OsString])->Result<(u32,PathBuf,PathBuf,String),String>{
    if args.len()!=5{return Err("更新器启动参数不完整".into());}
    let pid=args[1].to_string_lossy().parse::<u32>()
        .map_err(|e|format!("进程号无效：{e}"))?;
    let source=PathBuf::from(&args[2]);
    let dest=PathBuf::from(&args[3]);
    let digest=args[4].to_string_lossy().into_owned();
    if !valid_digest(&digest){return Err("校验码无效".into());}
    if !source.is_file() || dest.extension().is_none_or(|x|!x.eq_ignore_ascii_case("exe")){
        return Err("更新路径无效".into());
    }
    Ok((pid,source,dest,digest))
}
/// Called before GUI/tray initialization; helper mode remains windowless.
pub fn handle_helper_if_requested()->bool{
    let args:Vec<OsString>=std::env::args_os().collect();
    if args.get(1).is_none_or(|a|a!="--kvm-apply-update"){return false;}
    let outcome=install_args(&args[1..]).and_then(|(pid,src,dest,sha)|
        finalize_install(pid,&src,&dest,&sha)
    );
    if let Err(e)=outcome{
        write_status(&format!("Windows 更新失败：{e}"));
    }
    true
}
fn can_write_folder(path:&Path)->bool{
    let marker=path.join(format!(".kvm-update-permission-{}-{}",std::process::id(),stamp()));
    match OpenOptions::new().write(true).create_new(true).open(&marker){
        Ok(_)=>{let _=fs::remove_file(marker);true},
        Err(_)=>false,
    }
}
fn safe_wide_params(pid:u32,source:&Path,target:&Path,digest:&str)->Result<Vec<u16>,String>{
    // Windows paths cannot contain a quote character, avoiding command line
    // splitting/injection in the ShellExecuteW RunAs parameter string.
    let a=source.to_string_lossy();
    let b=target.to_string_lossy();
    if a.contains('"') || b.contains('"'){return Err("程序路径无效".into());}
    Ok(wide(OsStr::new(&format!("--kvm-apply-update {pid} \"{a}\" \"{b}\" {digest}"))))
}
/// Starts a detached helper copy, THEN caller stops network/input and exits.
/// If installed under Program Files, Windows will show a normal UAC prompt.
pub fn begin_install(source:&Path,digest:&str)->Result<(),String>{
    verify_exe(source,digest)?;
    let target=std::env::current_exe().map_err(|e|format!("找不到 kvm.exe：{e}"))?;
    if !target.exists(){return Err("当前程序位置无效".into());}
    let folder=target.parent().ok_or("无法取得程序目录")?;
    let helper=std::env::temp_dir().join(format!("kvm-updater-{}-{}.exe",
        std::process::id(),stamp()));
    fs::copy(&target,&helper).map_err(|e|format!("无法创建更新助手：{e}"))?;
    let pid=std::process::id();
    if can_write_folder(folder){
        let result=Command::new(&helper)
            .arg("--kvm-apply-update").arg(pid.to_string())
            .arg(source).arg(&target).arg(digest)
            .creation_flags_no_window().stdin(Stdio::null()).spawn();
        if let Err(e)=result{
            let _=fs::remove_file(&helper);
            return Err(format!("无法运行更新助手：{e}"));
        }
    }else{
        let verb=wide(OsStr::new("runas"));
        let exe=wide(helper.as_os_str());
        let args=safe_wide_params(pid,source,&target,digest)?;
        let result=unsafe{ShellExecuteW(std::ptr::null_mut(),verb.as_ptr(),
            exe.as_ptr(),args.as_ptr(),std::ptr::null(),0)};
        if result<=32{
            let _=fs::remove_file(&helper);
            return Err(format!("管理员授权被拒绝或无法启动更新助手（代码 {result}）"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn semver_digest_and_url_validation(){
        let data=serde_json::json!([
            {"draft":false,"tag_name":"v0.5.0-beta.25","assets":[{
                "name":"kvm.exe","browser_download_url":expected_url("0.5.0-beta.25"),
                "digest":format!("sha256:{}", "a".repeat(64))}]},
            {"draft":false,"tag_name":"v0.5.0-beta.26","assets":[{
                "name":"kvm.exe","browser_download_url":"https://invalid.example/kvm.exe",
                "digest":format!("sha256:{}", "b".repeat(64))}]},
            {"draft":false,"tag_name":"v0.5.0-beta.27","assets":[{
                "name":"kvm.exe","browser_download_url":expected_url("0.5.0-beta.27"),
                "digest":"sha256:nope"}]},
            {"draft":true,"tag_name":"v0.5.0-beta.28","assets":[{
                "name":"kvm.exe","browser_download_url":expected_url("0.5.0-beta.28"),
                "digest":format!("sha256:{}", "b".repeat(64))}]}
        ]);
        let r=parse_releases(&serde_json::to_vec(&data).unwrap(),
            "0.5.0-beta.24").unwrap().unwrap();
        assert_eq!(r.version,"0.5.0-beta.25");
        assert!(parse_releases(&serde_json::to_vec(&data).unwrap(),
            "0.5.0-beta.26").unwrap().is_none());
    }
    #[test]
    fn staged_update_and_rollback_keep_original_bytes(){
        let dir=std::env::temp_dir().join(format!("kvm-update-rollback-{}",stamp()));
        fs::create_dir(&dir).unwrap();
        let old=dir.join("kvm.exe");
        let new=dir.join("download.exe");
        let oldbytes=[b"MZ".as_slice(),&vec![3u8;100_000]].concat();
        let newbytes=[b"MZ".as_slice(),&vec![4u8;100_000]].concat();
        fs::write(&old,&oldbytes).unwrap();
        fs::write(&new,&newbytes).unwrap();
        let digest=sha256(&new).unwrap();
        assert!(replace_exe(&old,&new,&"0".repeat(64)).is_err());
        assert_eq!(fs::read(&old).unwrap(),oldbytes);
        replace_exe(&old,&new,&digest).unwrap();
        assert_eq!(fs::read(&old).unwrap(),newbytes);
        restore_old(&old);
        assert_eq!(fs::read(&old).unwrap(),oldbytes);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn reject_tampered_and_wrong_type(){
        let dir=std::env::temp_dir().join(format!("kvm-updater-test-{}",stamp()));
        fs::create_dir(&dir).unwrap();
        let f=dir.join("kvm.exe");
        let mut output=File::create(&f).unwrap();
        output.write_all(b"MZ").unwrap();
        output.write_all(&vec![7u8;100_000]).unwrap();
        drop(output);
        let digest=sha256(&f).unwrap();
        assert!(verify_exe(&f,&digest).is_ok());
        assert!(verify_exe(&f,&"0".repeat(64)).is_err());
        let _=fs::remove_dir_all(dir);
    }
}
