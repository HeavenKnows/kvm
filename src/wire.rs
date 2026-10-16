//! Noise-PSK-protected framed protocol. One frame <= 60 KiB, no custom cryptography.
use crate::model::{Platform, Screen, Side};
use rdev::EventType;
use serde::{Serialize, Deserialize};
use sha2::{Digest, Sha256};
use std::{io::{self, Read, Write}, net::TcpStream, sync::{Arc,Mutex,mpsc::{self,SyncSender,Receiver}}};

pub const PORT: u16 = 24881;
const MAX_FRAME: usize = 60_000;
const NOISE: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";

#[derive(Debug, Serialize, Deserialize)]
pub enum Wire {
    Hello { os: Platform, screen:Screen },
    Enter { x:i32,y:i32, side:Side },
    Back,
    Input(EventType),
    MoveBy { dx:i32, dy:i32 },
    /// Text only; receiver sets clipboard before injecting V.
    PasteRemote(String),
    /// Request remote clipboard for a local paste.
    ClipboardRequest,
    ClipboardReply(Option<String>),
}

fn frame_write(s:&mut TcpStream, bytes:&[u8])->io::Result<()> {
    if bytes.is_empty() || bytes.len()>MAX_FRAME { return Err(io::Error::new(io::ErrorKind::InvalidData,"frame size")); }
    s.write_all(&(bytes.len() as u16).to_be_bytes())?;
    s.write_all(bytes)
}
fn frame_read(s:&mut TcpStream)->io::Result<Vec<u8>> {
    let mut h=[0;2]; s.read_exact(&mut h)?;
    let n=u16::from_be_bytes(h) as usize;
    if n==0 || n>MAX_FRAME { return Err(io::Error::new(io::ErrorKind::InvalidData,"invalid frame")); }
    let mut buf=vec![0;n]; s.read_exact(&mut buf)?; Ok(buf)
}

fn err(e: impl std::fmt::Display)->io::Error {io::Error::new(io::ErrorKind::InvalidData,e.to_string())}

fn handshake(s:&mut TcpStream, secret:&str,initiator:bool)->io::Result<snow::TransportState>{
    // UI generates 128-bit secrets. Refuse low entropy/short manual passwords.
    if secret.len()!=32 || !secret.bytes().all(|b|b.is_ascii_hexdigit()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"pairing secret must be exactly 32 hex digits"));
    }
    s.set_read_timeout(Some(std::time::Duration::from_secs(4)))?;
    s.set_write_timeout(Some(std::time::Duration::from_secs(4)))?;
    let secret=Sha256::digest(secret.to_ascii_lowercase().as_bytes());
    let mut psk=[0_u8;32]; psk.copy_from_slice(&secret);
    let params = NOISE.parse().map_err(err)?;
    let builder=snow::Builder::new(params).psk(0,&psk).map_err(err)?;
    let mut h=if initiator {builder.build_initiator().map_err(err)?}
              else {builder.build_responder().map_err(err)?};
    let mut out=vec![0;MAX_FRAME]; let mut plaintext=vec![0;MAX_FRAME];
    if initiator {
        let n=h.write_message(&[],&mut out).map_err(err)?;
        frame_write(s,&out[..n])?;
        let got=frame_read(s)?;
        h.read_message(&got,&mut plaintext).map_err(err)?;
    } else {
        let got=frame_read(s)?;
        h.read_message(&got,&mut plaintext).map_err(err)?;
        let n=h.write_message(&[],&mut out).map_err(err)?;
        frame_write(s,&out[..n])?;
    }
    let state=h.into_transport_mode().map_err(err)?;
    s.set_read_timeout(None)?; s.set_write_timeout(None)?;
    Ok(state)
}

/// Reader and writer use separate cloned TcpStream handles, sharing Noise sequence state briefly.
/// Hook callbacks only ever enqueue; no socket IO, crypto, or clipboard work on hook thread.
pub fn connect(mut stream:TcpStream,secret:&str,initiator:bool)
    ->io::Result<(SyncSender<Wire>,Receiver<Wire>,TcpStream)>
{
    stream.set_nodelay(true)?;
    let noise=Arc::new(Mutex::new(handshake(&mut stream,secret,initiator)?));
    let mut writer=stream.try_clone()?;
    let control=stream.try_clone()?;
    let (tx,tx_rx)=mpsc::sync_channel::<Wire>(2048);
    let (rx_tx,rx)=mpsc::sync_channel::<Wire>(2048);
    let cipher=noise.clone();
    std::thread::spawn(move || {
        let mut out=vec![0;MAX_FRAME];
        while let Ok(msg)=tx_rx.recv() {
            let msg=match rmp_serde::to_vec(&msg){Ok(v)=>v,Err(_)=>break};
            if msg.len()>MAX_FRAME-32 {continue;}
            let n=match cipher.lock().unwrap().write_message(&msg,&mut out){Ok(n)=>n,Err(_)=>break};
            if frame_write(&mut writer,&out[..n]).is_err(){break;}
        }
        let _=writer.shutdown(std::net::Shutdown::Both);
    });
    std::thread::spawn(move || {
        let mut plain=vec![0;MAX_FRAME];
        loop {
            let pkt=match frame_read(&mut stream){Ok(v)=>v,Err(_)=>break};
            let n=match noise.lock().unwrap().read_message(&pkt,&mut plain){Ok(n)=>n,Err(_)=>break};
            let event=match rmp_serde::from_slice::<Wire>(&plain[..n]){Ok(v)=>v,Err(_)=>break};
            if rx_tx.send(event).is_err(){break;}
        }
        let _=stream.shutdown(std::net::Shutdown::Both);
    });
    Ok((tx,rx,control))
}
