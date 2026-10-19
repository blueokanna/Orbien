//! Orbien control-plane protocol.
//!
//! This crate is `no_std + alloc` on purpose: it is the part of Orbien that an
//! embedded agent, a WASM client or a mobile SDK has to share with the server.
//! Nothing here touches a socket, a clock or an async runtime.
//!
//! * [`msg`] declares the message contracts with `nextjson` derives, so every
//!   type describes its own schema at compile time.
//! * [`frame`] owns the wire layout: a one-byte type tag, a `u32` length and a
//!   body encoded either as JSON (`nextjson`) or as the bounded binary profile
//!   (`rustbinary`).
//! * [`auth`] implements the HMAC-SHA256 pre-shared-key handshake.
//!
//! Both codecs are accepted on every connection. The high bit of the type tag
//! marks a binary body, which lets a peer upgrade without a negotiation round
//! trip and keeps older peers on JSON.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod auth;
pub mod base64;
pub mod frame;
pub mod hex;
pub mod msg;

pub use frame::{Codec, FrameError, FrameHeader, MAX_FRAME_BODY};
pub use msg::Message;
