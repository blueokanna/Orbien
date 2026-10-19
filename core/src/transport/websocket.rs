//! RFC 6455 transport.
//!
//! The frame codec and the handshake primitives come from `courierust_ws`
//! (pure `no_std`, no I/O); the socket stays on tokio so a tunnel keeps one task
//! per direction instead of a thread per connection.
//!
//! The stream is a *byte* tunnel: binary fragments are concatenated on read and
//! every write becomes one binary frame. Orbien layers TLS and yamux on top, so
//! the WebSocket carries opaque bytes and never inspects them.

use super::stream::{boxed_stream, DynStream};
use anyhow::{bail, Context, Result};
use courierust::courierust_ws::frame::{FrameHeader, Mask, OpCode};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{ready, Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

pub const ORBIEN_WEBSOCKET_PATH: &str = "/~!orbien";

const MAX_HANDSHAKE_BYTES: usize = 8 * 1024;
const MAX_HEADER_LEN: usize = 14;

pub fn normalize_ws_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        ORBIEN_WEBSOCKET_PATH.to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn is_websocket_http_request(peeked: &[u8], path: &str) -> bool {
    let path = normalize_ws_path(path);
    let mut prefix = Vec::with_capacity(5 + path.len());
    prefix.extend_from_slice(b"GET ");
    prefix.extend_from_slice(path.as_bytes());
    peeked.starts_with(&prefix)
}

/// Server side: completes the upgrade and returns the byte tunnel.
pub async fn accept_websocket(stream: tokio::net::TcpStream) -> Result<DynStream> {
    let stream = server_upgrade(stream).await?;
    Ok(WsByteStream::server(stream).boxed())
}

/// Client side: completes the upgrade and returns the byte tunnel.
///
/// `host_header` keeps the configured name (the one the certificate is issued
/// for) even though the socket is already resolved to an address.
pub async fn dial_websocket(
    addr: SocketAddr,
    host_header: &str,
    path: &str,
    keepalive: crate::net::TcpKeepaliveConfig,
) -> Result<DynStream> {
    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .with_context(|| format!("tcp dial for websocket {addr}"))?;
    crate::net::tune_tcp_stream(&stream, keepalive);

    let path = normalize_ws_path(path);
    let key = courierust::courierust_ws::handshake::generate_key()
        .map_err(|e| anyhow::anyhow!("websocket key: {e}"))?;
    let request = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {host_header}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {key}\r\n\
         Sec-WebSocket-Version: 13\r\n\
         \r\n"
    );

    stream
        .write_all(request.as_bytes())
        .await
        .context("websocket upgrade write")?;
    stream.flush().await?;

    let head = read_http_head(&mut stream).await?;
    let head = std::str::from_utf8(&head).context("websocket upgrade response is not utf-8")?;
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or_default();
    if !status.starts_with("HTTP/1.1 101") && !status.starts_with("HTTP/1.0 101") {
        bail!("websocket upgrade rejected: {status}");
    }
    let expected = courierust::courierust_ws::handshake::accept_key(&key)
        .map_err(|e| anyhow::anyhow!("websocket accept key: {e}"))?;
    if header_value(lines, "sec-websocket-accept").as_deref() != Some(expected.as_str()) {
        bail!("websocket upgrade returned an invalid Sec-WebSocket-Accept");
    }

    Ok(WsByteStream::client(stream).boxed())
}

async fn server_upgrade(mut stream: tokio::net::TcpStream) -> Result<tokio::net::TcpStream> {
    let head = read_http_head(&mut stream).await?;
    let head = std::str::from_utf8(&head).context("websocket upgrade request is not utf-8")?;
    let mut lines = head.split("\r\n");
    let request = lines.next().unwrap_or_default();
    if !request.starts_with("GET ") {
        bail!("websocket upgrade expects a GET request");
    }
    if !header_has_token(lines.clone(), "connection", "upgrade")
        || !header_has_token(lines.clone(), "upgrade", "websocket")
    {
        bail!("websocket upgrade headers missing");
    }
    let key = header_value(lines, "sec-websocket-key").unwrap_or_default();
    if !courierust::courierust_ws::handshake::is_valid_key(&key) {
        bail!("websocket upgrade key is invalid");
    }
    let accept = courierust::courierust_ws::handshake::accept_key(&key)
        .map_err(|e| anyhow::anyhow!("websocket accept key: {e}"))?;

    let reply = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\
         \r\n"
    );
    stream
        .write_all(reply.as_bytes())
        .await
        .context("websocket upgrade reply")?;
    stream.flush().await?;
    Ok(stream)
}

async fn read_http_head<S: AsyncRead + Unpin>(stream: &mut S) -> Result<Vec<u8>> {
    let mut head = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    while head.len() < MAX_HANDSHAKE_BYTES {
        stream.read_exact(&mut byte).await?;
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            return Ok(head);
        }
    }
    bail!("websocket handshake exceeded {MAX_HANDSHAKE_BYTES} bytes")
}

fn header_value<'a>(lines: impl Iterator<Item = &'a str>, name: &str) -> Option<String> {
    lines
        .filter_map(|line| line.split_once(':'))
        .find_map(|(key, value)| {
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
}

fn header_has_token<'a>(lines: impl Iterator<Item = &'a str>, name: &str, token: &str) -> bool {
    header_value(lines, name).is_some_and(|value| {
        value
            .split(',')
            .any(|item| item.trim().eq_ignore_ascii_case(token))
    })
}

/// Mask keys are not a security boundary (they exist so intermediaries do not
/// cache frames), but they must be unpredictable per frame. A freshly seeded
/// `RandomState` is OS-entropy backed, so this needs no extra dependency.
struct MaskKeys {
    state: RandomState,
    counter: u64,
}

impl MaskKeys {
    fn new() -> Self {
        Self {
            state: RandomState::new(),
            counter: 0,
        }
    }

    fn next(&mut self) -> [u8; 4] {
        self.counter = self.counter.wrapping_add(1);
        let mut hasher = self.state.build_hasher();
        hasher.write_u64(self.counter);
        let bytes = hasher.finish().to_le_bytes();
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    }
}

pub struct WsByteStream<S> {
    inner: S,
    header: [u8; MAX_HEADER_LEN],
    header_len: usize,
    payload_remaining: u64,
    payload_mask: Option<Mask>,
    payload_offset: u64,
    control: Option<(OpCode, Vec<u8>)>,
    pending: Vec<u8>,
    pending_pos: usize,
    write_frame: Vec<u8>,
    write_pos: usize,
    write_ack: usize,
    client: bool,
    keys: MaskKeys,
    closed: bool,
}

impl<S> WsByteStream<S> {
    pub fn client(inner: S) -> Self {
        Self::new(inner, true)
    }

    pub fn server(inner: S) -> Self {
        Self::new(inner, false)
    }

    fn new(inner: S, client: bool) -> Self {
        Self {
            inner,
            header: [0u8; MAX_HEADER_LEN],
            header_len: 0,
            payload_remaining: 0,
            payload_mask: None,
            payload_offset: 0,
            control: None,
            pending: Vec::new(),
            pending_pos: 0,
            write_frame: Vec::new(),
            write_pos: 0,
            write_ack: 0,
            client,
            keys: MaskKeys::new(),
            closed: false,
        }
    }

    pub fn boxed(self) -> DynStream
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        boxed_stream(self)
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> WsByteStream<S> {
    /// Reads payload bytes, unmasking in place. Control-frame payloads are
    /// diverted into [`WsByteStream::control`] and never reach the caller.
    fn poll_read_payload(
        &mut self,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.payload_remaining == 0 {
            return Poll::Ready(Ok(()));
        }

        if self.control.is_some() {
            let mut chunk = [0u8; 125];
            let want = (self.payload_remaining as usize).min(chunk.len());
            let produced = {
                let mut scratch = ReadBuf::new(&mut chunk[..want]);
                ready!(Pin::new(&mut self.inner).poll_read(cx, &mut scratch))?;
                scratch.filled().len()
            };
            if produced == 0 {
                return Poll::Ready(Err(std::io::ErrorKind::UnexpectedEof.into()));
            }
            if let Some(mask) = self.payload_mask {
                mask.apply(self.payload_offset as usize, &mut chunk[..produced]);
            }
            self.payload_offset += produced as u64;
            self.payload_remaining -= produced as u64;
            if let Some((_, sink)) = self.control.as_mut() {
                sink.extend_from_slice(&chunk[..produced]);
            }
            if self.payload_remaining == 0 {
                let (opcode, payload) = self.control.take().expect("control frame in flight");
                self.handle_control(opcode, payload);
            }
            return Poll::Ready(Ok(()));
        }

        let want = (buf.remaining() as u64).min(self.payload_remaining) as usize;
        if want == 0 {
            return Poll::Ready(Ok(()));
        }
        let produced = {
            let mut scratch = ReadBuf::new(buf.initialize_unfilled_to(want));
            ready!(Pin::new(&mut self.inner).poll_read(cx, &mut scratch))?;
            scratch.filled().len()
        };
        if produced == 0 {
            return Poll::Ready(Err(std::io::ErrorKind::UnexpectedEof.into()));
        }
        if let Some(mask) = self.payload_mask {
            mask.apply(
                self.payload_offset as usize,
                buf.initialize_unfilled_to(produced),
            );
        }
        buf.advance(produced);
        self.payload_offset += produced as u64;
        self.payload_remaining -= produced as u64;
        Poll::Ready(Ok(()))
    }

    fn handle_control(&mut self, opcode: OpCode, payload: Vec<u8>) {
        match opcode {
            OpCode::Ping => self.queue_control(OpCode::Pong, &payload),
            OpCode::Close => self.closed = true,
            _ => {}
        }
    }

    fn queue_control(&mut self, opcode: OpCode, payload: &[u8]) {
        if !self.write_frame.is_empty() {
            return;
        }
        let (masked, mask_key) = if self.client {
            (true, self.keys.next())
        } else {
            (false, [0u8; 4])
        };
        let mut header = FrameHeader::data(opcode, true, payload.len() as u64);
        header.masked = masked;
        header.mask_key = mask_key;

        let mut raw = [0u8; MAX_HEADER_LEN];
        let header_len = header.write(&mut raw);

        self.write_frame.clear();
        self.write_frame.extend_from_slice(&raw[..header_len]);
        if masked {
            let mut body = payload.to_vec();
            Mask::new(mask_key).apply(0, &mut body);
            self.write_frame.extend_from_slice(&body);
        } else {
            self.write_frame.extend_from_slice(payload);
        }
        self.write_pos = 0;
        self.write_ack = 0;
    }

    fn poll_write_frame(&mut self, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        while self.write_pos < self.write_frame.len() {
            let written = ready!(
                Pin::new(&mut self.inner).poll_write(cx, &self.write_frame[self.write_pos..])
            )?;
            if written == 0 {
                return Poll::Ready(Err(std::io::ErrorKind::WriteZero.into()));
            }
            self.write_pos += written;
        }
        self.write_frame.clear();
        self.write_pos = 0;
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for WsByteStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();

        loop {
            if buf.remaining() == 0 {
                return Poll::Ready(Ok(()));
            }

            if this.pending_pos < this.pending.len() {
                let n = buf.remaining().min(this.pending.len() - this.pending_pos);
                buf.put_slice(&this.pending[this.pending_pos..this.pending_pos + n]);
                this.pending_pos += n;
                if this.pending_pos == this.pending.len() {
                    this.pending.clear();
                    this.pending_pos = 0;
                }
                return Poll::Ready(Ok(()));
            }

            if this.closed {
                return Poll::Ready(Ok(()));
            }

            if this.payload_remaining > 0 {
                let before = buf.filled().len();
                ready!(this.poll_read_payload(cx, buf))?;
                if buf.filled().len() > before {
                    return Poll::Ready(Ok(()));
                }
                continue;
            }

            if this.header_len < 2 {
                let filled = {
                    let mut scratch = ReadBuf::new(&mut this.header[this.header_len..2]);
                    ready!(Pin::new(&mut this.inner).poll_read(cx, &mut scratch))?;
                    scratch.filled().len()
                };
                if filled == 0 {
                    return Poll::Ready(Ok(()));
                }
                this.header_len += filled;
                continue;
            }

            let Some(total) = FrameHeader::header_len_hint(&this.header[..this.header_len]) else {
                continue;
            };
            if this.header_len < total {
                let filled = {
                    let mut scratch = ReadBuf::new(&mut this.header[this.header_len..total]);
                    ready!(Pin::new(&mut this.inner).poll_read(cx, &mut scratch))?;
                    scratch.filled().len()
                };
                if filled == 0 {
                    return Poll::Ready(Err(std::io::ErrorKind::UnexpectedEof.into()));
                }
                this.header_len += filled;
                continue;
            }

            let header = match FrameHeader::parse(&this.header[..total]) {
                Ok(Some(header)) => header,
                Ok(None) => continue,
                Err(err) => return Poll::Ready(Err(std::io::Error::other(err.to_string()))),
            };
            this.header_len = 0;

            if let Err(err) = header.check_reserved(false) {
                return Poll::Ready(Err(std::io::Error::other(err.to_string())));
            }

            if header.opcode == OpCode::Close {
                this.closed = true;
                return Poll::Ready(Ok(()));
            }

            this.payload_remaining = header.payload_len;
            this.payload_offset = 0;
            this.payload_mask = header.masked.then(|| Mask::new(header.mask_key));

            if header.opcode.is_control() {
                this.control = Some((
                    header.opcode,
                    Vec::with_capacity(header.payload_len as usize),
                ));
                if header.payload_len == 0 {
                    let (opcode, payload) = this.control.take().expect("just set");
                    this.handle_control(opcode, payload);
                }
            }
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for WsByteStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();

        if this.closed {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "websocket closed",
            )));
        }

        if !this.write_frame.is_empty() {
            ready!(this.poll_write_frame(cx))?;
            return Poll::Ready(Ok(this.write_ack));
        }

        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        let (masked, mask_key) = if this.client {
            (true, this.keys.next())
        } else {
            (false, [0u8; 4])
        };
        let mut header = FrameHeader::data(OpCode::Binary, true, buf.len() as u64);
        header.masked = masked;
        header.mask_key = mask_key;

        let mut raw = [0u8; MAX_HEADER_LEN];
        let header_len = header.write(&mut raw);

        this.write_frame.clear();
        this.write_frame.reserve(header_len + buf.len());
        this.write_frame.extend_from_slice(&raw[..header_len]);
        if masked {
            let start = this.write_frame.len();
            this.write_frame.extend_from_slice(buf);
            Mask::new(mask_key).apply(0, &mut this.write_frame[start..]);
        } else {
            this.write_frame.extend_from_slice(buf);
        }

        this.write_pos = 0;
        this.write_ack = buf.len();
        ready!(this.poll_write_frame(cx))?;
        Poll::Ready(Ok(this.write_ack))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if !this.write_frame.is_empty() {
            ready!(this.poll_write_frame(cx))?;
        }
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if !this.write_frame.is_empty() {
            ready!(this.poll_write_frame(cx))?;
        }
        if !this.closed {
            this.closed = true;
            this.queue_control(OpCode::Close, &1000u16.to_be_bytes());
            let _ = this.poll_write_frame(cx);
        }
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_upgrade_prefix() {
        assert!(is_websocket_http_request(
            b"GET /~!orbien HTTP/1.1",
            "/~!orbien"
        ));
        assert!(!is_websocket_http_request(
            b"GET /other HTTP/1.1",
            "/~!orbien"
        ));
        assert!(is_websocket_http_request(b"GET /~!orbien HTTP/1.1", ""));
    }

    #[test]
    fn header_lookup_is_case_insensitive() {
        let head = "GET / HTTP/1.1\r\nUpgrade: WebSocket\r\nConnection: keep-alive, Upgrade\r\n";
        let lines = head.split("\r\n");
        assert_eq!(
            header_value(lines.clone(), "upgrade").as_deref(),
            Some("WebSocket")
        );
        assert!(header_has_token(lines.clone(), "connection", "upgrade"));
        assert!(header_has_token(lines, "upgrade", "websocket"));
    }
}
