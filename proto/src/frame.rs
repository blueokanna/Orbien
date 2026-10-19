//! Wire framing.
//!
//! ```text
//! +--------+------------------+---------------------+
//! | tag(1) | body length(4)   | body (<= 4 MiB)     |
//! +--------+------------------+---------------------+
//! ```
//!
//! `tag` is one of the `TYPE_*` constants. Bit 7 is set when the body is the
//! `rustbinary` profile instead of JSON, so a peer can switch codecs per
//! message and an older peer that only speaks JSON keeps working unchanged.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::msg::Message;

/// Upper bound for one control-plane body. Matches the pre-4.0 limit so a
/// misbehaving peer cannot make the server allocate without bound.
pub const MAX_FRAME_BODY: u32 = 4 * 1024 * 1024;

/// Marks a `rustbinary` body.
pub const BINARY_FLAG: u8 = 0x80;

/// Type tag plus `u32` little-endian length.
pub const HEADER_LEN: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    Json,
    Binary,
}

impl Codec {
    pub const fn is_binary(self) -> bool {
        matches!(self, Self::Binary)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Binary => "binary",
        }
    }
}

#[derive(Debug)]
pub enum FrameError {
    UnknownType(u8),
    TooLarge(u32),
    Json(nextjson::Error),
    Binary(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownType(byte) => write!(f, "unknown message type: 0x{byte:02x}"),
            Self::TooLarge(len) => write!(f, "message body too large: {len} bytes"),
            Self::Json(err) => write!(f, "json: {err}"),
            Self::Binary(err) => write!(f, "binary: {err}"),
        }
    }
}

impl core::error::Error for FrameError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    pub type_byte: u8,
    pub codec: Codec,
    pub len: u32,
}

impl FrameHeader {
    pub fn new(type_byte: u8, codec: Codec, len: u32) -> Result<Self, FrameError> {
        if type_byte & BINARY_FLAG != 0 {
            return Err(FrameError::UnknownType(type_byte));
        }
        if len > MAX_FRAME_BODY {
            return Err(FrameError::TooLarge(len));
        }
        Ok(Self {
            type_byte,
            codec,
            len,
        })
    }

    pub fn encode(self, out: &mut [u8; HEADER_LEN]) {
        let tag = if self.codec.is_binary() {
            self.type_byte | BINARY_FLAG
        } else {
            self.type_byte
        };
        out[0] = tag;
        out[1..].copy_from_slice(&self.len.to_le_bytes());
    }

    pub fn decode(raw: [u8; HEADER_LEN]) -> Result<Self, FrameError> {
        let tag = raw[0];
        let codec = if tag & BINARY_FLAG != 0 {
            Codec::Binary
        } else {
            Codec::Json
        };
        let type_byte = tag & !BINARY_FLAG;
        if !type_byte.is_ascii_alphabetic() {
            return Err(FrameError::UnknownType(tag));
        }
        let len = u32::from_le_bytes([raw[1], raw[2], raw[3], raw[4]]);
        Self::new(type_byte, codec, len)
    }
}

impl Message {
    /// Encodes the message and returns a complete frame.
    pub fn encode_frame(&self, codec: Codec) -> Result<Vec<u8>, FrameError> {
        let mut frame = Vec::with_capacity(HEADER_LEN + 64);
        self.encode_frame_into(codec, &mut frame)?;
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::{Ping, UdpPacket};
    use alloc::format;

    fn roundtrip(msg: Message, codec: Codec) {
        let frame = msg.encode_frame(codec).unwrap();
        let header = FrameHeader::decode(frame[..HEADER_LEN].try_into().unwrap()).unwrap();
        assert_eq!(header.codec, codec);
        assert_eq!(header.len as usize, frame.len() - HEADER_LEN);
        let decoded =
            Message::decode_body(header.type_byte, &frame[HEADER_LEN..], header.codec).unwrap();
        assert_eq!(format!("{decoded:?}"), format!("{msg:?}"));
    }

    #[test]
    fn both_codecs_roundtrip() {
        let ping = Message::Ping(Ping {
            auth_digest: "deadbeef".into(),
            timestamp: 1_700_000_000,
        });
        roundtrip(ping.clone(), Codec::Json);
        roundtrip(ping, Codec::Binary);

        let udp = Message::UdpPacket(UdpPacket {
            content: alloc::vec![0, 1, 2, 250, 255],
            local_addr: None,
            remote_addr: None,
        });
        roundtrip(udp.clone(), Codec::Json);
        roundtrip(udp, Codec::Binary);
    }

    #[test]
    fn binary_flag_is_not_a_type_byte() {
        assert!(FrameHeader::new(BINARY_FLAG | b'A', Codec::Json, 0).is_err());
        let raw = [BINARY_FLAG | b'A', 0, 0, 0, 0];
        assert_eq!(FrameHeader::decode(raw).unwrap().type_byte, b'A');
    }

    #[test]
    fn binary_is_smaller_than_json() {
        let msg = Message::NewTunnel(crate::msg::NewTunnel {
            tunnel_name: "ssh".into(),
            protocol: "tcp".into(),
            remote_port: 9000,
            local_ip: "127.0.0.1".into(),
            local_port: 22,
            domains: alloc::vec!["ssh.example.com".into()],
            ..Default::default()
        });
        let json = msg.encode_body(Codec::Json).unwrap();
        let binary = msg.encode_body(Codec::Binary).unwrap();
        assert!(
            binary.len() * 2 < json.len(),
            "compact binary ({}) should be well under half the JSON size ({})",
            binary.len(),
            json.len()
        );
    }

    #[test]
    fn concatenated_frames_decode_in_order() {
        let first = Message::UdpPacket(UdpPacket {
            content: alloc::vec![1, 2, 3],
            ..Default::default()
        });
        let second = Message::Ping(Ping {
            auth_digest: "aa".into(),
            timestamp: 7,
        });

        let mut buffer = Vec::new();
        first.encode_frame_into(Codec::Binary, &mut buffer).unwrap();
        second
            .encode_frame_into(Codec::Binary, &mut buffer)
            .unwrap();

        let mut cursor = 0usize;
        let mut decoded = Vec::new();
        while cursor < buffer.len() {
            let raw: [u8; HEADER_LEN] = buffer[cursor..cursor + HEADER_LEN].try_into().unwrap();
            let header = FrameHeader::decode(raw).unwrap();
            cursor += HEADER_LEN;
            let end = cursor + header.len as usize;
            decoded.push(
                Message::decode_body(header.type_byte, &buffer[cursor..end], header.codec).unwrap(),
            );
            cursor = end;
        }
        assert_eq!(decoded.len(), 2);
        assert_eq!(format!("{:?}", decoded[0]), format!("{first:?}"));
        assert_eq!(format!("{:?}", decoded[1]), format!("{second:?}"));
    }

    #[test]
    fn oversized_body_rejected() {
        let raw = [b'A', 0xff, 0xff, 0xff, 0xff];
        assert!(FrameHeader::decode(raw).is_err());
    }

    #[test]
    fn empty_udp_payload_omits_content() {
        let msg = Message::UdpPacket(UdpPacket::default());
        let json = msg.encode_frame(Codec::Json).unwrap();
        let text = core::str::from_utf8(&json[HEADER_LEN..]).unwrap();
        assert_eq!(text, "{}");
    }
}
