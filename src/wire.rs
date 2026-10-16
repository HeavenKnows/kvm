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
    /// Receiver confirms that physical cursor placement succeeded.
    EnterAck { accepted:bool },
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
/// Keep the reason a reader/writer exited, for diagnostics on both peers.
fn record_fault(out:&Arc<Mutex<Option<String>>>,text:String) {
    let mut slot=out.lock().unwrap();
    if slot.is_none(){*slot=Some(text);}
}
pub fn connect(mut stream:TcpStream,secret:&str,initiator:bool)
    ->io::Result<(SyncSender<Wire>,Receiver<Wire>,TcpStream,Arc<Mutex<Option<String>>>)>
{
    // A receiver may accept() from a nonblocking TcpListener. On some
    // systems the accepted socket retains O_NONBLOCK. Noise framing below
    // uses read_exact/write_all in background threads and REQUIRES blocking
    // I/O. EAGAIN/WouldBlock is not a peer disconnect.
    stream.set_nonblocking(false)?;
    stream.set_nodelay(true)?;
    let noise=Arc::new(Mutex::new(handshake(&mut stream,secret,initiator)?));
    let mut writer=stream.try_clone()?;
    let control=stream.try_clone()?;
    let (tx,tx_rx)=mpsc::sync_channel::<Wire>(2048);
    let (rx_tx,rx)=mpsc::sync_channel::<Wire>(2048);
    let fault=Arc::new(Mutex::new(None));
    let cipher=noise.clone();
    let write_fault=fault.clone();
    std::thread::spawn(move || {
        let mut out=vec![0;MAX_FRAME];
        while let Ok(msg)=tx_rx.recv() {
            let payload=match rmp_serde::to_vec(&msg){
                Ok(v)=>v,
                Err(e)=>{record_fault(&write_fault,format!("encode message: {e}"));break;}
            };
            if payload.len()>MAX_FRAME-32 {
                record_fault(&write_fault,format!("outbound message too large: {} bytes",payload.len()));
                break;
            }
            let n=match cipher.lock().unwrap().write_message(&payload,&mut out){
                Ok(n)=>n,
                Err(e)=>{record_fault(&write_fault,format!("encrypt outbound packet: {e}"));break;}
            };
            if let Err(e)=frame_write(&mut writer,&out[..n]){
                record_fault(&write_fault,format!("write encrypted frame: {e}"));
                break;
            }
        }
        let _=writer.shutdown(std::net::Shutdown::Both);
    });
    let read_fault=fault.clone();
    std::thread::spawn(move || {
        let mut plain=vec![0;MAX_FRAME];
        loop {
            let pkt=match frame_read(&mut stream){
                Ok(v)=>v,
                Err(e)=>{
                    record_fault(&read_fault,format!("read encrypted frame: {e}"));
                    break;
                }
            };
            let n=match noise.lock().unwrap().read_message(&pkt,&mut plain){
                Ok(n)=>n,
                Err(e)=>{
                    record_fault(&read_fault,format!("decrypt inbound frame: {e}"));
                    break;
                }
            };
            let msg=match rmp_serde::from_slice::<Wire>(&plain[..n]){
                Ok(v)=>v,
                Err(e)=>{
                    record_fault(&read_fault,format!("decode inbound message: {e}"));
                    break;
                }
            };
            if rx_tx.send(msg).is_err(){break;}
        }
        let _=stream.shutdown(std::net::Shutdown::Both);
    });
    Ok((tx,rx,control,fault))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // Real TCP + Noise handshake + encrypted Hello/MoveBy/Back in both directions.
    // A compile-only test would miss framing, serialization and transport errors.
    #[test]
    fn encrypted_loopback_roundtrip() {
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr=listener.local_addr().unwrap();
        let responder=std::thread::spawn(move || {
            // Simulate the macOS receiver's nonblocking accept loop.
            let socket=loop {
                match listener.accept(){
                    Ok((socket,_))=>break socket,
                    Err(ref e) if e.kind()==io::ErrorKind::WouldBlock=>{
                        std::thread::sleep(Duration::from_millis(2));
                    }
                    Err(e)=>panic!("accept failed: {e}"),
                }
            };
            // Deliberately make the accepted socket nonblocking to reproduce
            // macOS EAGAIN, regardless of whether this OS inherits the flag.
            socket.set_nonblocking(true).unwrap();
            let (tx,rx,_socket,reason)=connect(socket,"0123456789abcdef0123456789abcdef",false).unwrap();
            tx.send(Wire::Hello{os:Platform::Mac,screen:Screen::new(1440,900)}).unwrap();
            tx.send(Wire::EnterAck{accepted:true}).unwrap();
            match rx.recv_timeout(Duration::from_secs(3)).unwrap(){
                Wire::MoveBy{dx:7,dy:-4}=>{},
                msg=>panic!("unexpected receiver packet: {msg:?}, reason={:?}",reason.lock().unwrap()),
            }
            tx.send(Wire::Back).unwrap();
        });
        let socket=TcpStream::connect(addr).unwrap();
        let (tx,rx,_socket,reason)=connect(socket,"0123456789abcdef0123456789abcdef",true).unwrap();
        match rx.recv_timeout(Duration::from_secs(3)).unwrap(){
            Wire::Hello{os:Platform::Mac,screen}=>{
                assert_eq!(screen.w,1440);
                assert_eq!(screen.h,900);
            }
            msg=>panic!("unexpected initial packet: {msg:?}, reason={:?}",reason.lock().unwrap()),
        }
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            Wire::EnterAck{accepted:true}
        ));
        // Both encrypted reader threads must survive a quiet interval.
        // Previously a nonblocking accepted socket died immediately here.
        std::thread::sleep(Duration::from_millis(80));
        tx.send(Wire::MoveBy{dx:7,dy:-4}).unwrap();
        assert!(matches!(rx.recv_timeout(Duration::from_secs(3)).unwrap(),Wire::Back));
        responder.join().unwrap();
    }
}

#[cfg(test)]
mod button_wire_tests {
    use super::*;
    #[test]
    fn mouse_down_up_roundtrip(){
        for event in [
            EventType::ButtonPress(rdev::Button::Left),
            EventType::ButtonRelease(rdev::Button::Left),
            EventType::ButtonPress(rdev::Button::Right),
            EventType::ButtonRelease(rdev::Button::Right),
            EventType::ButtonPress(rdev::Button::Middle),
            EventType::ButtonRelease(rdev::Button::Middle)
        ]{
            let payload=rmp_serde::to_vec(&Wire::Input(event.clone())).unwrap();
            match rmp_serde::from_slice::<Wire>(&payload).unwrap(){
                Wire::Input(decoded)=>assert_eq!(format!("{decoded:?}"),format!("{event:?}")),
                _=>panic!("button input wire event lost")
            }
        }
    }
}
