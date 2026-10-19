//! Dashboard HTTP surface.
//!
//! Served by `courierust_server` on its own scheduler threads; the handler
//! bridges into the tokio runtime that owns the tunnel state. The dashboard is
//! low-traffic and entirely independent of the data plane, which is why it can
//! live on a separate concurrency model without touching the hot path.

use super::model::{
    ApiResponse, ClientInfo, Page, SystemConfig, SystemInfo, SystemStatus, TunnelInfo,
    TunnelTrafficPoint, TunnelTrafficResp,
};
use super::DashState;
use crate::metrics::{TrafficWindow, TunnelTrafficHistory};
use courierust::courierust_body::Body;
use courierust::courierust_http::header::HeaderValue;
use courierust::courierust_http::method::Method;
use courierust::courierust_http::request::Request;
use courierust::courierust_http::response::Response;
use courierust::courierust_http::status::StatusCode;
use courierust::courierust_server::Handler;
use orbien_core::VERSION;
use rust_embed::Embed;
use std::sync::Arc;

#[derive(Embed)]
#[folder = "assets/"]
struct Assets;

pub struct Dashboard {
    state: Arc<DashState>,
    runtime: tokio::runtime::Handle,
}

impl Dashboard {
    pub fn new(state: Arc<DashState>, runtime: tokio::runtime::Handle) -> Self {
        Self { state, runtime }
    }

    fn blocking<T>(&self, fut: impl std::future::Future<Output = T>) -> T {
        self.runtime.block_on(fut)
    }
}

impl Handler for Dashboard {
    fn handle(&self, req: Request<Body>) -> Response<Body> {
        if !authorized(&self.state, &req) {
            return text(StatusCode::UNAUTHORIZED, "unauthorized")
                .header("www-authenticate", "Basic realm=\"Restricted\"");
        }

        let path = req.uri.path();
        let query = req.uri.query().unwrap_or_default();

        match (req.method.clone(), path) {
            (Method::GET, "/healthz") => text(StatusCode::OK, "ok"),
            (Method::GET, "/") => serve_asset("index.html"),
            (Method::GET, "/favicon.ico") => try_embedded("favicon.ico")
                .unwrap_or_else(|| text(StatusCode::NOT_FOUND, "not found")),
            (Method::GET, "/api/v1/system/info") => json(self.system_info()),
            (Method::GET, "/api/v1/system/traffic") => json(self.system_traffic(query)),
            (Method::GET, "/api/v1/clients") => json(self.list_clients(query)),
            (Method::GET, "/api/v1/tunnels") => json(self.list_tunnels(query)),
            (Method::POST, p) if p.starts_with("/api/v1/clients/") && p.ends_with("/kick") => {
                let id = p
                    .trim_start_matches("/api/v1/clients/")
                    .trim_end_matches("/kick");
                json(self.kick_client(&decode_component(id)))
            }
            (Method::GET, p) if p.starts_with("/api/v1/clients/") => {
                let id = decode_component(p.trim_start_matches("/api/v1/clients/"));
                self.get_client(&id)
            }
            (Method::GET, p) if p.starts_with("/api/v1/tunnels/") && p.ends_with("/traffic") => {
                let name = decode_component(
                    p.trim_start_matches("/api/v1/tunnels/")
                        .trim_end_matches("/traffic"),
                );
                self.tunnel_traffic(&name, query)
            }
            (Method::GET, p) => {
                let rel = p.trim_start_matches('/');
                if is_safe_asset_path(rel) {
                    if let Some(res) = try_embedded(rel) {
                        return res;
                    }
                }
                serve_asset("index.html")
            }
            _ => text(StatusCode::NOT_FOUND, "not found"),
        }
    }
}

impl Dashboard {
    fn system_info(&self) -> ApiResponse<SystemInfo> {
        let snap = self.blocking(self.state.svc.dashboard_snapshot());
        let cfg = self.state.svc.cfg();
        ApiResponse::ok(SystemInfo {
            version: VERSION.to_string(),
            config: SystemConfig {
                listen: cfg.listen.clone(),
                quic_port: cfg.quic_port,
                kcp_port: cfg.kcp_port,
                http_gw_port: cfg.http_gw_port,
                https_gw_port: cfg.https_gw_port,
                root_domain: cfg.root_domain.clone(),
                tcp_mux: cfg.transport.tcp_mux,
                tls_force: cfg.transport.tls.force,
                max_conn_pool: cfg.transport.max_conn_pool,
                heartbeat_timeout: cfg.transport.heartbeat_timeout,
            },
            status: SystemStatus {
                client_counts: snap
                    .clients
                    .iter()
                    .filter(|c| c.status.is_empty() || c.status == "online")
                    .count(),
                total_client_counts: snap.total_client_counts,
                tunnel_type_count: snap.tunnel_type_count,
                active_connections: snap.active_connections,
                total_traffic_in: snap.total_traffic_in,
                total_traffic_out: snap.total_traffic_out,
            },
        })
    }

    fn system_traffic(&self, query: &str) -> ApiResponse<TunnelTrafficResp> {
        let window = traffic_window(query);
        let hist = self.state.svc.metrics().server_traffic(window);
        ApiResponse::ok(traffic_resp(hist))
    }

    fn list_clients(&self, query: &str) -> ApiResponse<Page<ClientInfo>> {
        let (page, page_size) = page_params(query);
        let snap = self.blocking(self.state.svc.dashboard_snapshot());
        let total = snap.clients.len();
        let start = (page - 1).saturating_mul(page_size);
        ApiResponse::ok(Page {
            total,
            page,
            page_size,
            items: snap
                .clients
                .into_iter()
                .skip(start)
                .take(page_size)
                .collect(),
        })
    }

    fn get_client(&self, id: &str) -> Response<Body> {
        let snap = self.blocking(self.state.svc.dashboard_snapshot());
        match snap
            .clients
            .into_iter()
            .find(|c| c.session_id == id || c.agent_id == id)
        {
            Some(client) => json(ApiResponse::ok(client)),
            None => text(StatusCode::NOT_FOUND, "not found"),
        }
    }

    fn kick_client(&self, id: &str) -> ApiResponse<()> {
        match self.blocking(self.state.svc.kick_client(id)) {
            Ok(()) => ApiResponse::ok(()),
            Err(err) => ApiResponse {
                code: 404,
                msg: err.to_string(),
                data: (),
            },
        }
    }

    fn list_tunnels(&self, query: &str) -> ApiResponse<Page<TunnelInfo>> {
        let (page, page_size) = page_params(query);
        let session_id = query_param(query, "sessionId").unwrap_or_default();
        let needle = query_param(query, "q")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let session_id = session_id.trim();

        let snap = self.blocking(self.state.svc.dashboard_snapshot());
        let filtered: Vec<TunnelInfo> = snap
            .tunnels
            .into_iter()
            .filter(|t| session_id.is_empty() || t.session_id == session_id)
            .filter(|t| needle.is_empty() || t.name.to_ascii_lowercase().contains(&needle))
            .collect();
        let total = filtered.len();
        let start = (page - 1).saturating_mul(page_size);
        ApiResponse::ok(Page {
            total,
            page,
            page_size,
            items: filtered.into_iter().skip(start).take(page_size).collect(),
        })
    }

    fn tunnel_traffic(&self, name: &str, query: &str) -> Response<Body> {
        let window = traffic_window(query);
        match self.state.svc.metrics().tunnel_traffic(name, window) {
            Some(hist) => json(ApiResponse::ok(traffic_resp(hist))),
            None => text(StatusCode::NOT_FOUND, "not found"),
        }
    }
}

fn authorized(state: &DashState, req: &Request<Body>) -> bool {
    // Fail closed: an unconfigured dashboard has no credentials to compare
    // against, and an open dashboard can kick clients.
    if state.cfg.user.is_empty() && state.cfg.password.is_empty() {
        return false;
    }
    let Some(value) = req.headers.get("authorization") else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let Some(encoded) = value
        .strip_prefix("Basic ")
        .or_else(|| value.strip_prefix("basic "))
    else {
        return false;
    };
    let Some(raw) = orbien_proto::base64::decode(encoded.trim().as_bytes()) else {
        return false;
    };
    let Ok(text) = String::from_utf8(raw) else {
        return false;
    };
    let Some((user, password)) = text.split_once(':') else {
        return false;
    };
    constant_time_eq(user.as_bytes(), state.cfg.user.as_bytes())
        & constant_time_eq(password.as_bytes(), state.cfg.password.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

fn page_params(query: &str) -> (usize, usize) {
    let page = query_param(query, "page")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);
    let page_size = query_param(query, "pageSize")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .clamp(1, 200);
    (page, page_size)
}

fn traffic_window(query: &str) -> TrafficWindow {
    TrafficWindow::parse(&query_param(query, "range").unwrap_or_default())
}

fn traffic_resp(hist: TunnelTrafficHistory) -> TunnelTrafficResp {
    TunnelTrafficResp {
        name: hist.name,
        unit: hist.unit,
        granularity: hist.granularity,
        history: hist
            .history
            .into_iter()
            .map(|p| TunnelTrafficPoint {
                date: p.date,
                traffic_in: p.traffic_in,
                traffic_out: p.traffic_out,
            })
            .collect(),
    }
}

fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (decode_component(key) == name).then(|| decode_component(value))
    })
}

fn decode_component(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(high), Some(low)) => {
                    out.push((high << 4) | low);
                    i += 3;
                }
                _ => {
                    out.push(bytes[i]);
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn is_safe_asset_path(path: &str) -> bool {
    !path.is_empty() && !path.contains("..") && !path.contains('\\') && !path.contains('\0')
}

fn json<T: nextjson::NsonSerialize>(body: T) -> Response<Body> {
    match nextjson::nextencode(&body) {
        Ok(bytes) => Response::new(StatusCode::OK)
            .header("content-type", "application/json; charset=utf-8")
            .header("cache-control", "no-store")
            .with_body(Body::from(bytes)),
        Err(err) => {
            tracing::error!(error = %err, "dashboard response encoding failed");
            text(StatusCode::INTERNAL_SERVER_ERROR, "encoding error")
        }
    }
}

fn text(status: StatusCode, body: &'static str) -> Response<Body> {
    Response::new(status)
        .header("content-type", "text/plain; charset=utf-8")
        .with_body(Body::from(body))
}

fn serve_asset(path: &str) -> Response<Body> {
    try_embedded(path).unwrap_or_else(|| {
        text(
            StatusCode::NOT_FOUND,
            "dashboard assets missing — run `npm --prefix server-ui run build` and rebuild orbien-server",
        )
    })
}

fn try_embedded(path: &str) -> Option<Response<Body>> {
    let file = Assets::get(path)?;
    Some(
        Response::new(StatusCode::OK)
            .header("content-type", content_type(path))
            .header("cache-control", cache_control(path))
            .with_body(Body::from(file.data.into_owned())),
    )
}

fn cache_control(path: &str) -> &'static str {
    if path.ends_with("index.html") || !path.contains("/assets/") {
        "no-cache"
    } else {
        "public, max-age=31536000, immutable"
    }
}

fn content_type(path: &str) -> HeaderValue {
    let value = if path.ends_with(".js") || path.ends_with(".mjs") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".ico") {
        "image/x-icon"
    } else if path.ends_with(".woff2") {
        "font/woff2"
    } else if path.ends_with(".json") || path.ends_with(".map") {
        "application/json"
    } else {
        "application/octet-stream"
    };
    HeaderValue::from(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_params_decode() {
        assert_eq!(query_param("a=1&b=x%20y", "b").as_deref(), Some("x y"));
        assert_eq!(
            query_param("page=3&pageSize=10", "pageSize").as_deref(),
            Some("10")
        );
        assert_eq!(query_param("a=1", "missing"), None);
        assert_eq!(page_params("page=0&pageSize=9999"), (1, 200));
    }

    #[test]
    fn rejects_traversal() {
        assert!(!is_safe_asset_path("../etc/passwd"));
        assert!(!is_safe_asset_path("a\\b"));
        assert!(is_safe_asset_path("assets/app.js"));
    }

    #[test]
    fn constant_time_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
