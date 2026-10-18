pub mod auth;
pub mod config;
pub mod io;
pub mod limit;
pub mod logging;
pub mod msg;
pub mod net;
pub mod tls;
pub mod transport;
pub mod udp;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
