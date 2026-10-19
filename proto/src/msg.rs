//! Control-plane message contracts.
//!
//! Field names are part of the published wire contract (the Java client and the
//! desktop bridge both read them), so they stay snake_case on the JSON path and
//! are unchanged by the binary codec.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::net::{IpAddr, SocketAddr};
use core::str::FromStr;

use nextjson::{NsonDeserialize, NsonSerialize};

pub const TYPE_LOGIN: u8 = b'A';
pub const TYPE_LOGIN_RESP: u8 = b'a';
pub const TYPE_NEW_TUNNEL: u8 = b'T';
pub const TYPE_NEW_TUNNEL_RESP: u8 = b't';
pub const TYPE_CLOSE_TUNNEL: u8 = b'X';
pub const TYPE_NEW_DATA_CONN: u8 = b'W';
pub const TYPE_REQ_DATA_CONN: u8 = b'Q';
pub const TYPE_START_DATA_CONN: u8 = b'S';
pub const TYPE_PING: u8 = b'G';
pub const TYPE_PONG: u8 = b'g';
pub const TYPE_UDP_PACKET: u8 = b'D';
pub const TYPE_KICK_OUT: u8 = b'E';

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct Login {
    #[njson(default)]
    pub version: String,
    #[njson(default)]
    pub hostname: String,
    #[njson(default)]
    pub os: String,
    #[njson(default)]
    pub arch: String,
    #[njson(default)]
    pub user: String,
    #[njson(default)]
    pub agent_id: String,
    #[njson(default)]
    pub auth_digest: String,
    #[njson(default)]
    pub timestamp: i64,
    #[njson(default)]
    pub session_id: String,
    #[njson(default)]
    pub pool_count: i32,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct LoginResp {
    #[njson(default)]
    pub version: String,
    #[njson(default)]
    pub session_id: String,
    #[njson(default)]
    pub error: String,
    /// `"binary"` when the server accepts the `rustbinary` profile on this
    /// connection. Empty for servers that only speak JSON.
    #[njson(default)]
    pub codec: String,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct NewTunnel {
    #[njson(default)]
    pub tunnel_name: String,
    #[njson(default)]
    pub protocol: String,
    #[njson(default)]
    pub remote_port: i32,
    #[njson(default)]
    pub local_ip: String,
    #[njson(default)]
    pub local_port: i32,
    #[njson(default)]
    pub domains: Vec<String>,
    #[njson(default)]
    pub locations: Vec<String>,
    #[njson(default)]
    pub basic_auth_user: String,
    #[njson(default)]
    pub basic_auth_password: String,
    #[njson(default)]
    pub host_header_rewrite: String,
    #[njson(default)]
    pub headers: BTreeMap<String, String>,
    #[njson(default)]
    pub response_headers: BTreeMap<String, String>,
    #[njson(default)]
    pub bandwidth: f64,
    #[njson(default)]
    pub bandwidth_limit_side: String,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct NewTunnelResp {
    #[njson(default)]
    pub tunnel_name: String,
    #[njson(default)]
    pub remote_addr: String,
    #[njson(default)]
    pub error: String,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct CloseTunnel {
    #[njson(default)]
    pub tunnel_name: String,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct ReqDataConn {}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct NewDataConn {
    #[njson(default)]
    pub session_id: String,
    #[njson(default)]
    pub auth_digest: String,
    #[njson(default)]
    pub timestamp: i64,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct StartDataConn {
    #[njson(default)]
    pub tunnel_name: String,
    #[njson(default)]
    pub src_addr: String,
    #[njson(default)]
    pub src_port: u16,
    #[njson(default)]
    pub dst_addr: String,
    #[njson(default)]
    pub dst_port: u16,
    #[njson(default)]
    pub error: String,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct Ping {
    #[njson(default)]
    pub auth_digest: String,
    #[njson(default)]
    pub timestamp: i64,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct Pong {
    #[njson(default)]
    pub error: String,
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct KickOut {
    #[njson(default)]
    pub reason: String,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Hash, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary,
)]
pub struct UdpSocketAddr {
    #[njson(default)]
    pub ip: String,
    #[njson(default)]
    pub port: u16,
}

impl UdpSocketAddr {
    pub fn from_std(addr: SocketAddr) -> Self {
        Self {
            ip: addr.ip().to_string(),
            port: addr.port(),
        }
    }

    pub fn to_std(&self) -> Option<SocketAddr> {
        let ip = IpAddr::from_str(&self.ip).ok()?;
        Some(SocketAddr::new(ip, self.port))
    }
}

#[derive(Debug, Clone, Default, NsonSerialize, NsonDeserialize, rustbinary::CompactBinary)]
pub struct UdpPacket {
    /// Base64 on the JSON path, a length-prefixed byte run on the binary path.
    #[njson(default, with = "b64_bytes", skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<u8>,
    #[njson(default, rename = "local", skip_serializing_if = "Option::is_none")]
    pub local_addr: Option<UdpSocketAddr>,
    #[njson(default, rename = "remote", skip_serializing_if = "Option::is_none")]
    pub remote_addr: Option<UdpSocketAddr>,
}

impl UdpPacket {
    pub fn new(content: Vec<u8>, remote: Option<SocketAddr>) -> Self {
        Self {
            content,
            local_addr: None,
            remote_addr: remote.map(UdpSocketAddr::from_std),
        }
    }

    /// Appends this packet as one `TYPE_UDP_PACKET` frame.
    ///
    /// Encoding straight from `&UdpPacket` lets the server coalesce queued
    /// datagrams without cloning their payloads into a `Message` first.
    pub fn encode_frame_into(
        &self,
        codec: crate::frame::Codec,
        out: &mut Vec<u8>,
    ) -> Result<(), crate::frame::FrameError> {
        append_frame(out, TYPE_UDP_PACKET, codec, self)
    }
}

mod b64_bytes {
    use alloc::vec::Vec;

    use nextjson::{FormatDecoder, FormatEncoder, FormatError};

    pub fn serialize<E: FormatEncoder>(data: &[u8], encoder: &mut E) -> Result<(), E::Error> {
        encoder.write_str(&crate::base64::encode(data))
    }

    pub fn deserialize<'de, D: FormatDecoder<'de>>(decoder: &mut D) -> Result<Vec<u8>, D::Error> {
        let text = decoder.string()?;
        if text.is_empty() {
            return Ok(Vec::new());
        }
        crate::base64::decode(text.as_bytes())
            .ok_or_else(|| D::Error::custom("udp payload is not valid base64"))
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Login(Login),
    LoginResp(LoginResp),
    NewTunnel(NewTunnel),
    NewTunnelResp(NewTunnelResp),
    CloseTunnel(CloseTunnel),
    ReqDataConn(ReqDataConn),
    NewDataConn(NewDataConn),
    StartDataConn(StartDataConn),
    Ping(Ping),
    Pong(Pong),
    UdpPacket(UdpPacket),
    KickOut(KickOut),
}

macro_rules! message_table {
    ($($tag:ident => $variant:ident($payload:ty)),* $(,)?) => {
        impl Message {
            pub fn type_byte(&self) -> u8 {
                match self {
                    $(Self::$variant(_) => $tag,)*
                }
            }

            /// Encodes the payload without the frame header.
            pub fn encode_body(&self, codec: crate::frame::Codec) -> Result<Vec<u8>, crate::frame::FrameError> {
                let mut out = Vec::with_capacity(128);
                self.encode_body_into(codec, &mut out)?;
                Ok(out)
            }

            /// Appends the payload to `out`, reusing its capacity.
            pub fn encode_body_into(
                &self,
                codec: crate::frame::Codec,
                out: &mut Vec<u8>,
            ) -> Result<(), crate::frame::FrameError> {
                match self {
                    $(Self::$variant(payload) => encode_value_into(payload, codec, out),)*
                }
            }

            /// Appends one complete frame to `out`, reusing its capacity.
            pub fn encode_frame_into(
                &self,
                codec: crate::frame::Codec,
                out: &mut Vec<u8>,
            ) -> Result<(), crate::frame::FrameError> {
                match self {
                    $(Self::$variant(payload) => append_frame(out, $tag, codec, payload),)*
                }
            }

            /// Decodes a payload previously produced by [`Message::encode_body`].
            pub fn decode_body(
                type_byte: u8,
                body: &[u8],
                codec: crate::frame::Codec,
            ) -> Result<Self, crate::frame::FrameError> {
                match type_byte {
                    $($tag => Ok(Self::$variant(decode_value::<$payload>(body, codec)?)),)*
                    other => Err(crate::frame::FrameError::UnknownType(other)),
                }
            }
        }
    };
}

message_table! {
    TYPE_LOGIN => Login(Login),
    TYPE_LOGIN_RESP => LoginResp(LoginResp),
    TYPE_NEW_TUNNEL => NewTunnel(NewTunnel),
    TYPE_NEW_TUNNEL_RESP => NewTunnelResp(NewTunnelResp),
    TYPE_CLOSE_TUNNEL => CloseTunnel(CloseTunnel),
    TYPE_REQ_DATA_CONN => ReqDataConn(ReqDataConn),
    TYPE_NEW_DATA_CONN => NewDataConn(NewDataConn),
    TYPE_START_DATA_CONN => StartDataConn(StartDataConn),
    TYPE_PING => Ping(Ping),
    TYPE_PONG => Pong(Pong),
    TYPE_UDP_PACKET => UdpPacket(UdpPacket),
    TYPE_KICK_OUT => KickOut(KickOut),
}

/// Appends one framed value: header, then body encoded straight into `out`.
pub(crate) fn append_frame<T>(
    out: &mut Vec<u8>,
    type_byte: u8,
    codec: crate::frame::Codec,
    value: &T,
) -> Result<(), crate::frame::FrameError>
where
    T: NsonSerialize + rustbinary::CompactEncode + ?Sized,
{
    let start = out.len();
    out.extend_from_slice(&[0u8; crate::frame::HEADER_LEN]);
    if let Err(err) = encode_value_into(value, codec, out) {
        out.truncate(start);
        return Err(err);
    }

    let len = (out.len() - start - crate::frame::HEADER_LEN) as u32;
    let header = crate::frame::FrameHeader::new(type_byte, codec, len)?;
    let mut raw = [0u8; crate::frame::HEADER_LEN];
    header.encode(&mut raw);
    out[start..start + crate::frame::HEADER_LEN].copy_from_slice(&raw);
    Ok(())
}

fn encode_value_into<T>(
    value: &T,
    codec: crate::frame::Codec,
    out: &mut Vec<u8>,
) -> Result<(), crate::frame::FrameError>
where
    T: NsonSerialize + rustbinary::CompactEncode + ?Sized,
{
    match codec {
        crate::frame::Codec::Json => {
            nextjson::to_writer(&mut *out, value).map_err(crate::frame::FrameError::Json)
        }
        crate::frame::Codec::Binary => {
            let compact = compact();
            let start = out.len();
            // Start from the spare capacity: after the first message the buffer
            // already fits, so the happy path encodes exactly once.
            let mut capacity = out.capacity().saturating_sub(start).max(256);
            loop {
                out.resize(start + capacity, 0);
                match compact.serialize_into_slice(&mut out[start..], value) {
                    Ok(len) => {
                        out.truncate(start + len);
                        return Ok(());
                    }
                    Err(rustbinary::Error::BufferTooSmall { required, .. }) => {
                        out.truncate(start);
                        capacity = required;
                    }
                    Err(err) => {
                        out.truncate(start);
                        return Err(crate::frame::FrameError::Binary(alloc::format!("{err}")));
                    }
                }
            }
        }
    }
}

fn decode_value<'de, T>(
    body: &'de [u8],
    codec: crate::frame::Codec,
) -> Result<T, crate::frame::FrameError>
where
    T: NsonDeserialize<'de> + rustbinary::CompactDecode<'de>,
{
    match codec {
        crate::frame::Codec::Json => {
            nextjson::nextdecode(body).map_err(crate::frame::FrameError::Json)
        }
        crate::frame::Codec::Binary => compact()
            .deserialize(body)
            .map_err(|e| crate::frame::FrameError::Binary(alloc::format!("{e}"))),
    }
}

/// The compact profile: no tags, no field names, length-prefixed containers,
/// with the default byte and collection ceilings kept at the trust boundary.
fn compact() -> rustbinary::CompactConfig {
    rustbinary::Config::standard().with_compact_format()
}
