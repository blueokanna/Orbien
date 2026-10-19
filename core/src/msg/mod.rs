//! Async framing over the `no_std` contracts in [`orbien_proto`].
//!
//! The contracts, the auth primitives and the frame layout live in
//! `orbien-proto` so an agent can be built without `std`. This module only adds
//! the tokio adapter.

pub use orbien_proto::auth::{AuthFailure, AUTH_SKEW_SECS};
pub use orbien_proto::msg::*;
pub use orbien_proto::{Codec, FrameError, FrameHeader, MAX_FRAME_BODY};

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Anything that can append itself as one wire frame.
///
/// Implemented for [`Message`] and [`UdpPacket`] so a writer can coalesce a
/// queue of either without cloning payloads into the enum first.
pub trait FramePayload {
    fn encode_frame_into(&self, codec: Codec, out: &mut Vec<u8>) -> Result<(), FrameError>;
}

impl FramePayload for Message {
    fn encode_frame_into(&self, codec: Codec, out: &mut Vec<u8>) -> Result<(), FrameError> {
        Message::encode_frame_into(self, codec, out)
    }
}

impl FramePayload for UdpPacket {
    fn encode_frame_into(&self, codec: Codec, out: &mut Vec<u8>) -> Result<(), FrameError> {
        UdpPacket::encode_frame_into(self, codec, out)
    }
}

/// A reusable frame buffer bound to one writer.
///
/// The control plane writes one message at a time; UDP writes a batch per
/// syscall. Both encode straight into the same buffer, so a steady stream of
/// datagrams does not allocate a fresh `Vec` per frame.
pub struct FrameWriter<W> {
    inner: W,
    frame: Vec<u8>,
}

impl<W> FrameWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            frame: Vec::with_capacity(1024),
        }
    }

    pub fn with_capacity(inner: W, capacity: usize) -> Self {
        Self {
            inner,
            frame: Vec::with_capacity(capacity),
        }
    }

    pub fn get_mut(&mut self) -> &mut W {
        &mut self.inner
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: AsyncWrite + Unpin> FrameWriter<W> {
    /// Writes one frame and flushes it.
    pub async fn write(&mut self, msg: &Message, codec: Codec) -> Result<(), MessageWriteError> {
        self.frame.clear();
        msg.encode_frame_into(codec, &mut self.frame)?;
        self.inner.write_all(&self.frame).await?;
        self.inner.flush().await?;
        Ok(())
    }

    /// Encodes several frames into one buffer and writes them with one call.
    ///
    /// Returns the number of bytes written.
    pub async fn write_batch<T: FramePayload>(
        &mut self,
        msgs: &[T],
        codec: Codec,
    ) -> Result<usize, MessageWriteError> {
        self.frame.clear();
        for msg in msgs {
            msg.encode_frame_into(codec, &mut self.frame)?;
        }
        if self.frame.is_empty() {
            return Ok(0);
        }
        self.inner.write_all(&self.frame).await?;
        self.inner.flush().await?;
        Ok(self.frame.len())
    }
}

#[derive(Debug, Error)]
pub enum MessageReadError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Frame(#[from] FrameError),
}

#[derive(Debug, Error)]
pub enum MessageWriteError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Frame(#[from] FrameError),
}

/// Reads one frame.
///
/// Returns the codec the peer used so a responder can answer in kind; a peer
/// that only understands JSON never sets the binary flag.
pub async fn read_msg<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<(Message, Codec), MessageReadError> {
    let mut body = Vec::new();
    read_msg_into(reader, &mut body).await
}

/// Reads one frame, reusing `body` for the payload.
///
/// The decoded message owns its data, so `body` is free again once this
/// returns — which is what makes a per-connection scratch buffer worthwhile on
/// a datagram path.
pub async fn read_msg_into<R: AsyncRead + Unpin>(
    reader: &mut R,
    body: &mut Vec<u8>,
) -> Result<(Message, Codec), MessageReadError> {
    let mut raw = [0u8; orbien_proto::frame::HEADER_LEN];
    reader.read_exact(&mut raw).await?;
    let header = FrameHeader::decode(raw)?;

    body.clear();
    body.resize(header.len as usize, 0);
    reader.read_exact(body).await?;

    let msg = Message::decode_body(header.type_byte, body, header.codec)?;
    Ok((msg, header.codec))
}

/// Writes one frame and flushes it.
pub async fn write_msg<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &Message,
    codec: Codec,
) -> Result<(), MessageWriteError> {
    let frame = msg.encode_frame(codec)?;
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn batches_frames_in_order() {
        let (client, mut server) = tokio::io::duplex(64 * 1024);
        let mut writer = FrameWriter::new(client);
        let msgs = vec![
            Message::Ping(Ping {
                auth_digest: "aa".into(),
                timestamp: 1,
            }),
            Message::Pong(Pong::default()),
        ];

        let written = writer.write_batch(&msgs, Codec::Binary).await.unwrap();
        assert!(written > 0);
        drop(writer);

        let (first, codec) = read_msg(&mut server).await.unwrap();
        assert_eq!(codec, Codec::Binary);
        let (second, _) = read_msg(&mut server).await.unwrap();
        assert!(matches!(first, Message::Ping(_)));
        assert!(matches!(second, Message::Pong(_)));
    }

    #[tokio::test]
    async fn read_into_reuses_the_body_buffer() {
        let (client, mut server) = tokio::io::duplex(64 * 1024);
        let mut writer = FrameWriter::new(client);
        for _ in 0..3 {
            writer
                .write(
                    &Message::Ping(Ping {
                        auth_digest: "bb".into(),
                        timestamp: 2,
                    }),
                    Codec::Json,
                )
                .await
                .unwrap();
        }
        drop(writer);

        let mut body = Vec::new();
        let mut capacity_after_first = 0usize;
        for i in 0..3 {
            let (msg, codec) = read_msg_into(&mut server, &mut body).await.unwrap();
            assert_eq!(codec, Codec::Json);
            assert!(matches!(msg, Message::Ping(_)));
            if i == 0 {
                capacity_after_first = body.capacity();
                assert!(capacity_after_first > 0);
            } else {
                assert_eq!(
                    body.capacity(),
                    capacity_after_first,
                    "the scratch buffer must not grow after the first frame"
                );
            }
        }
    }
}
