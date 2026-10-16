use serde::Serialize;

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub code: i32,
    pub msg: String,
    pub data: T,
}
impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            code: 200,
            msg: "success".into(),
            data,
        }
    }
}
#[derive(Serialize)]
pub struct Page<T: Serialize> {
    pub total: usize,
    pub page: usize,
    #[serde(rename = "pageSize")]
    pub page_size: usize,
    pub items: Vec<T>,
}
#[derive(Serialize)]
pub struct SystemInfo {
    pub version: String,
    pub config: SystemConfig,
    pub status: SystemStatus,
}
#[derive(Serialize)]
pub struct SystemConfig {
    pub listen: String,
    #[serde(rename = "quicPort")]
    pub quic_port: u16,
    #[serde(rename = "kcpPort")]
    pub kcp_port: u16,
    #[serde(rename = "httpGwPort")]
    pub http_gw_port: u16,
    #[serde(rename = "httpsGwPort")]
    pub https_gw_port: u16,
    #[serde(rename = "rootDomain")]
    pub root_domain: String,
    #[serde(rename = "tcpMux")]
    pub tcp_mux: bool,
    #[serde(rename = "tlsForce")]
    pub tls_force: bool,
    #[serde(rename = "maxConnPool")]
    pub max_conn_pool: i64,
    #[serde(rename = "heartbeatTimeout")]
    pub heartbeat_timeout: i64,
}
#[derive(Serialize)]
pub struct SystemStatus {
    #[serde(rename = "clientCounts")]
    pub client_counts: usize,
    #[serde(rename = "totalClientCounts")]
    pub total_client_counts: usize,
    #[serde(rename = "tunnelTypeCount")]
    pub tunnel_type_count: std::collections::BTreeMap<String, usize>,
    #[serde(rename = "activeConnections")]
    pub active_connections: usize,
    #[serde(rename = "totalTrafficIn")]
    pub total_traffic_in: u64,
    #[serde(rename = "totalTrafficOut")]
    pub total_traffic_out: u64,
}
#[derive(Serialize)]
pub struct ClientInfo {
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "agentId")]
    pub agent_id: String,
    pub user: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    #[serde(rename = "clientIP")]
    pub client_ip: String,
    pub version: String,
    #[serde(rename = "tunnelCount")]
    pub tunnel_count: usize,
    #[serde(rename = "activeConnections")]
    pub active_connections: usize,
    #[serde(rename = "connectedSecs")]
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
#[derive(Serialize)]
pub struct TunnelInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub tunnel_type: String,
    #[serde(rename = "remoteAddr")]
    pub remote_addr: String,
    #[serde(rename = "localAddr")]
    pub local_addr: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub status: String,
    #[serde(rename = "todayTrafficIn")]
    pub today_traffic_in: u64,
    #[serde(rename = "todayTrafficOut")]
    pub today_traffic_out: u64,
    #[serde(rename = "activeConnections")]
    pub active_connections: usize,
    #[serde(rename = "lastStartTime", skip_serializing_if = "Option::is_none")]
    pub last_start_time: Option<String>,
}
#[derive(Serialize)]
pub struct TunnelTrafficPoint {
    pub date: String,
    #[serde(rename = "trafficIn")]
    pub traffic_in: u64,
    #[serde(rename = "trafficOut")]
    pub traffic_out: u64,
}
#[derive(Serialize)]
pub struct TunnelTrafficResp {
    pub name: String,
    pub unit: &'static str,
    pub granularity: &'static str,
    pub history: Vec<TunnelTrafficPoint>,
}
