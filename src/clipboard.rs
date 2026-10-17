//! On-demand clipboard snapshots. Never touch OS clipboard on input hooks or
//! the network/mouse receiver thread. All decoding is size bounded.
use arboard::{Clipboard, ImageData};
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const CHUNK_SIZE: usize = 16 * 1024;
const MAX_FILES: usize = 16;
const MAX_IMAGE_PIXELS: usize = 8_000_000;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
enum Content {
    Text(String),
    Image { width: usize, height: usize, rgba: Vec<u8> },
    Files(Vec<FileData>),
}
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct FileData { name: String, bytes: Vec<u8> }

fn encode(content: &Content) -> Result<Vec<u8>, String> {
    let encoded = rmp_serde::to_vec(content).map_err(|e| e.to_string())?;
    if encoded.len() > MAX_BYTES { return Err("Clipboard exceeds 32 MiB limit".into()); }
    Ok(encoded)
}
fn file_snapshot(paths: Vec<PathBuf>) -> Result<Content, String> {
    if paths.len() > MAX_FILES { return Err("At most 16 files can be pasted at once".into()); }
    let mut files = Vec::new();
    let mut total = 0usize;
    for path in paths {
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        // Do not follow symlinks or recursively walk folders.
        if !metadata.file_type().is_file() { return Err("Only regular files (not folders or symlinks) are supported".into()); }
        let n = usize::try_from(metadata.len()).map_err(|e| e.to_string())?;
        total = total.checked_add(n).ok_or("Clipboard file size overflow")?;
        if total > MAX_BYTES - 8192 { return Err("Files exceed 32 MiB clipboard limit".into()); }
        let name = path.file_name().and_then(|n| n.to_str())
            .filter(|n| !n.is_empty()).ok_or("Invalid file name")?.to_owned();
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if bytes.len() != n { return Err("Clipboard file changed while being read".into()); }
        files.push(FileData { name, bytes });
    }
    Ok(Content::Files(files))
}
pub fn snapshot() -> Result<Vec<u8>, String> {
    let mut c = Clipboard::new().map_err(|e| e.to_string())?;
    // Files first: Finder/Explorer can offer a text fallback containing paths.
    if let Ok(paths) = c.get().file_list() {
        if !paths.is_empty() { return encode(&file_snapshot(paths)?); }
    }
    if let Ok(image) = c.get_image() {
        let pixels = image.width.checked_mul(image.height).ok_or("Image dimensions overflow")?;
        if pixels == 0 || pixels > MAX_IMAGE_PIXELS || image.bytes.len() != pixels * 4 {
            return Err("Image too large or malformed".into());
        }
        return encode(&Content::Image {
            width: image.width, height: image.height, rgba: image.bytes.into_owned(),
        });
    }
    if let Ok(text) = c.get_text() {
        return encode(&Content::Text(text));
    }
    Err("Clipboard has no supported text, image, or file data".into())
}
pub fn install(encoded: &[u8]) -> Result<(), String> {
    if encoded.is_empty() || encoded.len() > MAX_BYTES { return Err("Invalid clipboard size".into()); }
    let data: Content = rmp_serde::from_slice(encoded).map_err(|e| e.to_string())?;
    let mut c = Clipboard::new().map_err(|e| e.to_string())?;
    match data {
        Content::Text(text) => c.set_text(text).map_err(|e| e.to_string()),
        Content::Image { width, height, rgba } => {
            let pixels = width.checked_mul(height).ok_or("Image dimensions overflow")?;
            if pixels == 0 || pixels > MAX_IMAGE_PIXELS || pixels.checked_mul(4) != Some(rgba.len()) {
                return Err("Invalid image dimensions or RGBA length".into());
            }
            c.set_image(ImageData { width, height, bytes: Cow::Owned(rgba) })
                .map_err(|e| e.to_string())
        }
        Content::Files(files) => {
            if files.is_empty() || files.len() > MAX_FILES { return Err("Invalid file list".into()); }
            let stamp = SystemTime::now().duration_since(UNIX_EPOCH)
                .unwrap_or_default().as_nanos();
            let directory = std::env::temp_dir().join("kvm-clipboard")
                .join(format!("{}-{stamp}", std::process::id()));
            std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
            let mut paths = Vec::with_capacity(files.len());
            let mut total = 0usize;
            for (i, file) in files.into_iter().enumerate() {
                let name = std::path::Path::new(&file.name);
                if name.components().count() != 1 || name.file_name().is_none()
                    || file.name == "." || file.name == ".."
                    || file.name.contains('/') || file.name.contains('\\')
                    || file.name.contains(':') {
                    return Err("Unsafe clipboard file name".into());
                }
                total = total.checked_add(file.bytes.len()).ok_or("File length overflow")?;
                if total > MAX_BYTES { return Err("Clipboard files exceed 32 MiB".into()); }
                let mut path = directory.join(&file.name);
                if path.exists() { path = directory.join(format!("{i}-{}", file.name)); }
                std::fs::write(&path, file.bytes).map_err(|e| e.to_string())?;
                paths.push(path);
            }
            c.set().file_list(&paths).map_err(|e| e.to_string())
        }
    }
}

#[derive(Debug)]
pub enum Incoming { Begin(u32), Chunk(Vec<u8>), End }
pub fn receive_loop(rx: std::sync::mpsc::Receiver<Incoming>, on_complete: impl Fn(Vec<u8>)) {
    let mut pending: Option<(usize, Vec<u8>)> = None;
    while let Ok(msg) = rx.recv() {
        match msg {
            Incoming::Begin(len) => {
                let len = len as usize;
                pending = if len > 0 && len <= MAX_BYTES {
                    Some((len, Vec::with_capacity(len)))
                } else { None };
            }
            Incoming::Chunk(chunk) => {
                if let Some((expected, data)) = &mut pending {
                    if chunk.is_empty() || chunk.len() > CHUNK_SIZE ||
                        chunk.len() > *expected - data.len() {
                        pending = None;
                    } else { data.extend_from_slice(&chunk); }
                }
            }
            Incoming::End => {
                if let Some((expected, data)) = pending.take() {
                    if data.len() == expected { on_complete(data); }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoded_text_and_file_bounds() {
        let text = encode(&Content::Text("跨平台 clipboard".into())).unwrap();
        assert_eq!(rmp_serde::from_slice::<Content>(&text).unwrap(),
            Content::Text("跨平台 clipboard".into()));
        assert!(encode(&Content::Text("x".repeat(MAX_BYTES + 1))).is_err());
    }
    #[test]
    fn chunk_reassembly_requires_complete_bounded_transfer() {
        use std::sync::{Arc, Mutex, mpsc};
        let (tx, rx) = mpsc::channel();
        let completed = Arc::new(Mutex::new(Vec::new()));
        let out = completed.clone();
        let thread = std::thread::spawn(move || receive_loop(rx, |bytes| out.lock().unwrap().push(bytes)));
        tx.send(Incoming::Begin(5)).unwrap();
        tx.send(Incoming::Chunk(vec![1,2])).unwrap();
        tx.send(Incoming::Chunk(vec![3,4,5])).unwrap();
        tx.send(Incoming::End).unwrap();
        tx.send(Incoming::Begin(2)).unwrap();
        tx.send(Incoming::Chunk(vec![0,0,0])).unwrap();
        tx.send(Incoming::End).unwrap();
        drop(tx);
        thread.join().unwrap();
        assert_eq!(*completed.lock().unwrap(), vec![vec![1,2,3,4,5]]);
    }
}
