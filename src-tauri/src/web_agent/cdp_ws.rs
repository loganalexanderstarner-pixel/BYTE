//! A small WebSocket client for the DevTools protocol on this computer (127.0.0.1 only).
//!
//! Written here instead of adding a crate: the protocol needs only text frames to and from a browser on the
//! same machine, and every dependency would also have to be vendored for the Windows build. Frames, the
//! handshake check and the HTTP discovery reply are plain functions with tests; the socket code is thin.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::oneshot;

use super::cdp::{encode_request, parse_incoming, Incoming, Transport};
use crate::error::{AppError, AppResult};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// Largest message accepted from the browser (a big accessibility tree is a few MB; more than 16 MB ends the connection).
pub const MAX_MESSAGE: usize = 16 * 1024 * 1024;
const REPLY_WAIT: Duration = Duration::from_secs(40);

/// `Sec-WebSocket-Accept` for a key (RFC 6455).
pub fn accept_key(key: &str) -> String {
    use base64::Engine;
    let d = ring::digest::digest(&ring::digest::SHA1_FOR_LEGACY_USE_ONLY, format!("{key}{GUID}").as_bytes());
    base64::engine::general_purpose::STANDARD.encode(d.as_ref())
}

/// One frame sent by the client (always masked).
pub fn encode_frame(opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut out = vec![0x80 | (opcode & 0x0f)];
    let n = payload.len();
    if n < 126 {
        out.push(0x80 | n as u8);
    } else if n <= 0xffff {
        out.push(0x80 | 126);
        out.extend_from_slice(&(n as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(n as u64).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    out
}

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub fin: bool,
    pub opcode: u8,
    pub payload: Vec<u8>,
}

/// Takes one frame off the front of `buf`: `Ok(None)` when more bytes are needed, `Err` when the browser
/// sends something too big.
pub fn parse_frame(buf: &[u8]) -> Result<Option<(Frame, usize)>, String> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let fin = buf[0] & 0x80 != 0;
    let opcode = buf[0] & 0x0f;
    let masked = buf[1] & 0x80 != 0;
    let mut len = (buf[1] & 0x7f) as usize;
    let mut at = 2;
    if len == 126 {
        if buf.len() < 4 {
            return Ok(None);
        }
        len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
        at = 4;
    } else if len == 127 {
        if buf.len() < 10 {
            return Ok(None);
        }
        let mut b = [0u8; 8];
        b.copy_from_slice(&buf[2..10]);
        let l = u64::from_be_bytes(b);
        if l > MAX_MESSAGE as u64 {
            return Err("the browser sent a message that is too large".into());
        }
        len = l as usize;
        at = 10;
    }
    if len > MAX_MESSAGE {
        return Err("the browser sent a message that is too large".into());
    }
    let mask = if masked {
        if buf.len() < at + 4 {
            return Ok(None);
        }
        let m = [buf[at], buf[at + 1], buf[at + 2], buf[at + 3]];
        at += 4;
        Some(m)
    } else {
        None
    };
    if buf.len() < at + len {
        return Ok(None);
    }
    let mut payload = buf[at..at + len].to_vec();
    if let Some(m) = mask {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= m[i % 4];
        }
    }
    Ok(Some((Frame { fin, opcode, payload }, at + len)))
}

/// The HTTP reply's body (de-chunked when the server chunks it) and whether the status was 200.
pub fn http_body(raw: &[u8]) -> Option<(bool, Vec<u8>)> {
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_lowercase();
    let ok = head.lines().next().is_some_and(|l| l.contains(" 200"));
    let body = &raw[split + 4..];
    if head.contains("transfer-encoding: chunked") {
        let mut out = Vec::new();
        let mut rest = body;
        loop {
            let nl = rest.windows(2).position(|w| w == b"\r\n")?;
            let size = usize::from_str_radix(String::from_utf8_lossy(&rest[..nl]).split(';').next()?.trim(), 16).ok()?;
            rest = &rest[nl + 2..];
            if size == 0 {
                break;
            }
            if rest.len() < size {
                return None;
            }
            out.extend_from_slice(&rest[..size]);
            rest = rest.get(size + 2..)?;
        }
        return Some((ok, out));
    }
    Some((ok, body.to_vec()))
}

fn random_bytes<const N: usize>() -> [u8; N] {
    use ring::rand::SecureRandom;
    let mut b = [0u8; N];
    let _ = ring::rand::SystemRandom::new().fill(&mut b);
    b
}

/// GET on the browser's discovery port (127.0.0.1 only), returning the JSON body.
pub async fn http_get_json(port: u16, path: &str) -> AppResult<Value> {
    let mut s = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(("127.0.0.1", port))).await.map_err(|_| AppError::msg("the browser didn't answer"))??;
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes()).await?;
    let mut raw = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), (&mut s).take(1024 * 1024).read_to_end(&mut raw)).await.map_err(|_| AppError::msg("the browser didn't answer"))??;
    let (ok, body) = http_body(&raw).ok_or_else(|| AppError::msg("the browser's answer wasn't readable"))?;
    if !ok {
        return Err(AppError::msg("the browser refused the request"));
    }
    serde_json::from_slice(&body).map_err(|e| AppError::msg(format!("the browser's answer wasn't readable: {e}")))
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

struct Inner {
    write: tokio::sync::Mutex<OwnedWriteHalf>,
    pending: Pending,
    next: AtomicU64,
    closed: AtomicBool,
}

impl Inner {
    async fn write_frame(&self, opcode: u8, payload: &[u8]) -> AppResult<()> {
        let frame = encode_frame(opcode, payload, random_bytes::<4>());
        self.write.lock().await.write_all(&frame).await?;
        Ok(())
    }

    fn fail_all(&self, why: &str) {
        self.closed.store(true, Ordering::SeqCst);
        if let Ok(mut p) = self.pending.lock() {
            for (_, tx) in p.drain() {
                let _ = tx.send(Err(why.to_string()));
            }
        }
    }
}

/// A connection to one page of the browser.
pub struct WsTransport {
    inner: Arc<Inner>,
}

impl WsTransport {
    /// Connects to `ws://127.0.0.1:<port><path>` (the page address the browser itself reported).
    pub async fn connect(port: u16, path: &str) -> AppResult<WsTransport> {
        Self::connect_as(port, path, None).await
    }

    // --- track:found begin --- a tab of the live research job: other pages are left alone (`cdp::ANY_TAB`)
    pub async fn connect_tab(port: u16, path: &str) -> AppResult<WsTransport> {
        Self::connect_as(port, path, Some(super::cdp::ANY_TAB)).await
    }
    // --- track:found end ---

    async fn connect_as(port: u16, path: &str, own: Option<&str>) -> AppResult<WsTransport> {
        let own_target = own.map(str::to_string).unwrap_or_else(|| path.rsplit('/').next().unwrap_or("").to_string());
        let mut s = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(("127.0.0.1", port))).await.map_err(|_| AppError::msg("couldn't connect to the browser"))??;
        use base64::Engine;
        let key = base64::engine::general_purpose::STANDARD.encode(random_bytes::<16>());
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").as_bytes()).await?;
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() > 16 * 1024 {
                return Err(AppError::msg("the browser's handshake was too long"));
            }
            let n = tokio::time::timeout(Duration::from_secs(5), s.read(&mut byte)).await.map_err(|_| AppError::msg("the browser didn't finish the handshake"))??;
            if n == 0 {
                return Err(AppError::msg("the browser closed the connection"));
            }
            head.push(byte[0]);
        }
        let text = String::from_utf8_lossy(&head).to_string();
        let accepted = text.lines().next().is_some_and(|l| l.contains(" 101"))
            && text.lines().any(|l| l.to_ascii_lowercase().starts_with("sec-websocket-accept:") && l.split_once(':').is_some_and(|(_, v)| v.trim() == accept_key(&key)));
        if !accepted {
            return Err(AppError::msg("the browser refused the connection (is another program using its debugging port?)"));
        }
        let (mut read, write) = s.into_split();
        let inner = Arc::new(Inner { write: tokio::sync::Mutex::new(write), pending: Arc::new(Mutex::new(HashMap::new())), next: AtomicU64::new(1), closed: AtomicBool::new(false) });
        let reader = inner.clone();
        let own = own_target.clone();
        tokio::spawn(async move {
            let mut buf: Vec<u8> = Vec::new();
            let mut message: Vec<u8> = Vec::new();
            let mut chunk = vec![0u8; 64 * 1024];
            'outer: loop {
                match read.read(&mut chunk).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
                loop {
                    match parse_frame(&buf) {
                        Ok(None) => break,
                        Err(e) => {
                            log::warn!("browser connection: {e}");
                            break 'outer;
                        }
                        Ok(Some((frame, used))) => {
                            buf.drain(..used);
                            match frame.opcode {
                                // text, binary or a continuation
                                0x0 | 0x1 | 0x2 => {
                                    message.extend_from_slice(&frame.payload);
                                    if message.len() > MAX_MESSAGE {
                                        break 'outer;
                                    }
                                    if frame.fin {
                                        let text = String::from_utf8_lossy(&message).to_string();
                                        message.clear();
                                        handle_message(&reader, &own, &text);
                                    }
                                }
                                0x8 => break 'outer,
                                0x9 => {
                                    let _ = reader.write_frame(0xA, &frame.payload).await;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            reader.fail_all("the browser window was closed");
        });
        Ok(WsTransport { inner })
    }
}

fn handle_message(inner: &Arc<Inner>, own_target: &str, text: &str) {
    match parse_incoming(text) {
        Some(Incoming::Reply { id, result }) => {
            if let Some(tx) = inner.pending.lock().ok().and_then(|mut p| p.remove(&id)) {
                let _ = tx.send(result);
            }
        }
        Some(Incoming::Event { method, params }) => {
            // Dialogs, new pages, documents and frames are dealt with at once (see `cdp::react`); the replies are ignored.
            for (m, p) in super::cdp::react(own_target, &method, &params) {
                let me = inner.clone();
                tokio::spawn(async move {
                    let id = me.next.fetch_add(1, Ordering::SeqCst);
                    let _ = me.write_frame(0x1, encode_request(id, &m, &p).as_bytes()).await;
                });
            }
        }
        None => {}
    }
}

impl Transport for WsTransport {
    fn send<'a>(&'a self, method: &'a str, params: Value) -> BoxFuture<'a, AppResult<Value>> {
        Box::pin(async move {
            if self.inner.closed.load(Ordering::SeqCst) {
                return Err(AppError::msg("the browser window was closed"));
            }
            let id = self.inner.next.fetch_add(1, Ordering::SeqCst);
            let (tx, rx) = oneshot::channel();
            self.inner.pending.lock().map_err(|_| AppError::msg("browser state poisoned"))?.insert(id, tx);
            if let Err(e) = self.inner.write_frame(0x1, encode_request(id, method, &params).as_bytes()).await {
                self.inner.pending.lock().ok().map(|mut p| p.remove(&id));
                return Err(e);
            }
            match tokio::time::timeout(REPLY_WAIT, rx).await {
                Ok(Ok(Ok(v))) => Ok(v),
                Ok(Ok(Err(e))) => Err(AppError::msg(e)),
                Ok(Err(_)) => Err(AppError::msg("the browser window was closed")),
                Err(_) => {
                    self.inner.pending.lock().ok().map(|mut p| p.remove(&id));
                    Err(AppError::msg("the browser didn't answer in time"))
                }
            }
        })
    }

    fn closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }
}

impl Drop for WsTransport {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handshake_key_matches_the_rfc_example() {
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn client_frames_are_masked_and_round_trip() {
        for len in [0usize, 5, 125, 126, 300, 70_000] {
            let payload: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            let f = encode_frame(0x1, &payload, [1, 2, 3, 4]);
            assert_eq!(f[1] & 0x80, 0x80, "masked");
            let (frame, used) = parse_frame(&f).unwrap().unwrap();
            assert_eq!(used, f.len());
            assert!(frame.fin);
            assert_eq!(frame.opcode, 1);
            assert_eq!(frame.payload, payload);
        }
    }

    #[test]
    fn server_frames_come_unmasked_and_partial_ones_wait() {
        let mut f = vec![0x81, 0x05];
        f.extend_from_slice(b"hello");
        f.extend_from_slice(&[0x81, 0x02, b'o']); // a second, unfinished frame
        let (frame, used) = parse_frame(&f).unwrap().unwrap();
        assert_eq!(frame.payload, b"hello");
        assert_eq!(used, 7);
        assert!(parse_frame(&f[used..]).unwrap().is_none());
        assert!(parse_frame(&[0x81]).unwrap().is_none());
        // 64-bit length beyond the cap is refused.
        let mut huge = vec![0x81, 127];
        huge.extend_from_slice(&u64::MAX.to_be_bytes());
        assert!(parse_frame(&huge).is_err());
    }

    #[test]
    fn http_replies_are_split_and_dechunked() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        assert_eq!(http_body(plain), Some((true, b"{}".to_vec())));
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n[1,2\r\n2\r\n]\n\r\n0\r\n\r\n";
        assert_eq!(http_body(chunked).unwrap().1, b"[1,2]\n".to_vec());
        assert_eq!(http_body(b"HTTP/1.1 404 Not Found\r\n\r\nno").map(|r| r.0), Some(false));
        assert!(http_body(b"garbage").is_none());
    }

    /// A browser-like server on a local port: handshake, then answers requests. Checks the whole client path.
    #[tokio::test]
    async fn a_request_gets_its_reply_over_a_local_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = s.read(&mut buf).await.unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let key = req.lines().find_map(|l| l.strip_prefix("Sec-WebSocket-Key: ")).unwrap().trim().to_string();
            s.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n", accept_key(&key)).as_bytes()).await.unwrap();
            let mut acc: Vec<u8> = Vec::new();
            loop {
                let n = s.read(&mut buf).await.unwrap();
                if n == 0 {
                    return;
                }
                acc.extend_from_slice(&buf[..n]);
                while let Some((f, used)) = parse_frame(&acc).unwrap() {
                    acc.drain(..used);
                    let v: Value = serde_json::from_slice(&f.payload).unwrap();
                    let reply = json!({ "id": v["id"], "result": { "echo": v["method"] } }).to_string();
                    let mut out = vec![0x81];
                    out.push(reply.len() as u8);
                    out.extend_from_slice(reply.as_bytes());
                    // Replies over 125 bytes use the 2-byte length form.
                    if reply.len() > 125 {
                        out = vec![0x81, 126, (reply.len() >> 8) as u8, reply.len() as u8];
                        out.extend_from_slice(reply.as_bytes());
                    }
                    s.write_all(&out).await.unwrap();
                }
            }
        });
        let t = WsTransport::connect(port, "/devtools/page/x").await.unwrap();
        let r = t.send("Page.enable", json!({})).await.unwrap();
        assert_eq!(r["echo"], "Page.enable");
        let r = t.send("DOM.enable", json!({})).await.unwrap();
        assert_eq!(r["echo"], "DOM.enable");
        assert!(!t.closed());
    }

    #[tokio::test]
    async fn a_refused_handshake_is_an_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let _ = s.read(&mut buf).await;
            s.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await.unwrap();
        });
        assert!(WsTransport::connect(port, "/x").await.is_err());
    }
}
