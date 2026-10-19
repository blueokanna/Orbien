//! Dashboard API payloads.
//!
//! These types are the published JSON contract with `server-ui`; the field names
//! are asserted by the Vue layer, so they stay camelCase on the wire.

use std::collections::BTreeMap;

use nextjson::NsonSerialize;

#[derive(NsonSerialize)]
#[njson(rename_all = "camelCase")]
pub struct ApiResponse<T: NsonSerialize> {
    pub code: i32,
    pub msg: String,
    pub data: T,
}

impl<T: NsonSerialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            code: 200,
            msg: "success".into(),
            data,
        }
    }
}

#[derive(NsonSerialize)]
#[njson(rename_all = "camelCase")]
pub struct Page<T: NsonSerialize> {
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
    pub items: Vec<T>,
}

#[derive(NsonSerialize)]
#[njson(rename_all = "camelCase")]
pub struct SystemInfo {
    pub version: String,
    pub config: SystemConfig,
    pub status: SystemStatus,
}

#[derive(NsonSerialize)]
#[njson(rename_all = "camelCase")]
pub struct SystemConfig {
    pub listen: String,
    pub quic_port: u16,
    pub kcp_port: u16,
    pub http_gw_port: u16,
    pub https_gw_port: u16,
    pub root_domain: String,
    pub tcp_mux: bool,
    pub tls_force: bool,
    pub max_conn_pool: i64,
    pub heartbeat_timeout: i64,
}

#[derive(NsonSerialize)]
#[njson(rename_all = "camelCase")]
pub struct SystemStatus {
    pub client_counts: usize,
    pub total_client_counts: usize,
    pub tunnel_type_count: BTreeMap<String, usize>,
    pub active_connections: usize,
    pub total_traffic_in: u64,
    pub total_traffic_out: u64,
}

#[derive(NsonSerialize)]
#[njson(rename_all = "camelCase")]
pub struct ClientInfo {
    pub session_id: String,
    pub agent_id: String,
    pub user: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub client_ip: String,
    pub version: String,
    pub tunnel_count: usize,
    pub active_connections: usize,
    pub connected_secs: u64,
    pub status: String,
}

impl ClientInfo {
    pub fn display_id(&self) -> &str {
        if self.agent_id.is_empty() {
            &self.session_id
        } else {
            &self.agent_id
        }
    }
}

#[derive(NsonSerialize)]
#[njson(rename_all = "camelCase")]
pub struct TunnelInfo {
    pub name: String,
    #[njson(rename = "type")]
    pub tunnel_type: String,
    pub remote_addr: String,
    pub local_addr: String,
    pub session_id: String,
    pub status: String,
    pub today_traffic_in: u64,
    pub today_traffic_out: u64,
    pub active_connections: usize,
    #[njson(skip_serializing_if = "Option::is_none")]
    pub last_start_time: Option<String>,
}

#[derive(NsonSerialize)]
#[njson(rename_all = "camelCase")]
pub struct TunnelTrafficPoint {
    pub date: String,
    pub traffic_in: u64,
    pub traffic_out: u64,
}

#[derive(NsonSerialize)]
pub struct TunnelTrafficResp {
    pub name: String,
    pub unit: &'static str,
    pub granularity: &'static str,
    pub history: Vec<TunnelTrafficPoint>,
}
