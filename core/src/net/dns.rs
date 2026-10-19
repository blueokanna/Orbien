//! Hostname resolution.
//!
//! Lookups go through `recurse-x`: a bounded resolver with its own cache,
//! retry budget and response validation. The platform's configured nameservers
//! are used as upstreams when they exist — plenty of networks drop UDP/53 to the
//! roots — and the system stub resolver is the last resort, so a restricted
//! network degrades instead of refusing to start.
//!
//! The resolver is process-wide and lazily created; `recurse-x` owns its cache,
//! its retry budget and its transport pool, so one instance is the right number.

use anyhow::{anyhow, bail, Context, Result};
use recurse_x::{Name, Resolver, ResolverConfig};
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::OnceLock;
use std::time::Duration;

/// Hard cap on one lookup. `recurse-x` has its own query budget; this bound
/// only keeps a wedged future from parking a tunnel setup indefinitely.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);

fn resolver() -> &'static Resolver {
    static RESOLVER: OnceLock<Resolver> = OnceLock::new();
    RESOLVER.get_or_init(|| Resolver::new(build_config()))
}

/// Prefer the platform's configured nameservers as upstreams.
///
/// Root recursion is the right default for a resolver that must not trust the
/// host, but plenty of networks drop UDP/53 to the roots. When the platform
/// publishes a nameserver, forward to it and keep `recurse-x`'s cache,
/// validation and retry policy in front of it.
fn build_config() -> ResolverConfig {
    let mut config = ResolverConfig::default();
    // Keep one failed lookup cheap: the fallback below is what guarantees a
    // working answer, so the recursive attempt must not dominate startup.
    config.engine.timeout_ms = 1_200;
    config.engine.query_budget_ms = 1_500;
    config.engine.max_attempts_per_server = 1;

    for server in system_nameservers() {
        let text = match server {
            IpAddr::V4(ip) => format!("{ip}"),
            IpAddr::V6(ip) => format!("[{ip}]"),
        };
        match recurse_x::config::parse_upstream(&text) {
            Ok(forwarder) => config.engine.forwarders.push(forwarder),
            Err(err) => tracing::debug!(%server, error = %err, "ignoring nameserver"),
        }
    }
    if config.engine.forwarders.is_empty() {
        tracing::debug!("no system nameserver found; resolving from the root");
    } else {
        tracing::debug!(
            count = config.engine.forwarders.len(),
            "resolving through system nameservers"
        );
    }
    config
}

fn system_nameservers() -> Vec<IpAddr> {
    #[cfg(unix)]
    {
        resolv_conf_servers()
    }
    #[cfg(windows)]
    {
        registry_servers()
    }
    #[cfg(not(any(unix, windows)))]
    {
        Vec::new()
    }
}

#[cfg(unix)]
fn resolv_conf_servers() -> Vec<IpAddr> {
    let Ok(text) = std::fs::read_to_string("/etc/resolv.conf") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("nameserver "))
        .filter_map(|addr| IpAddr::from_str(addr.trim()).ok())
        .collect()
}

#[cfg(windows)]
fn registry_servers() -> Vec<IpAddr> {
    const KEY: &str = r"HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces";
    let Ok(output) = std::process::Command::new("reg")
        .args(["query", KEY, "/s"])
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut servers: Vec<IpAddr> = Vec::new();
    for line in text.lines() {
        let Some((name, value)) = line.split_once("REG_SZ") else {
            continue;
        };
        let name = name.trim();
        if name != "NameServer" && name != "DhcpNameServer" {
            continue;
        }
        for token in value.split([' ', ',', ';']) {
            if let Ok(ip) = IpAddr::from_str(token.trim()) {
                if !servers.contains(&ip) {
                    servers.push(ip);
                }
            }
        }
    }
    servers
}

/// Resolves `host` to its addresses, following CNAME/DNAME chains.
///
/// Literal IPv4/IPv6 addresses and `localhost` never touch the network.
pub async fn resolve_host(host: &str) -> Result<Vec<IpAddr>> {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() {
        bail!("empty host name");
    }

    if let Ok(ip) = IpAddr::from_str(host) {
        return Ok(vec![ip]);
    }
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(vec![
            IpAddr::from([127, 0, 0, 1]),
            IpAddr::from([0u16, 0, 0, 0, 0, 0, 0, 1]),
        ]);
    }

    let name = Name::from_ascii(host).map_err(|e| anyhow!("invalid host name {host}: {e}"))?;
    let host_owned = host.to_owned();

    if !degraded() {
        let lookup = tokio::task::spawn_blocking(move || resolver().resolve_addresses(&name)).await;
        match lookup {
            Ok(addrs) if !addrs.is_empty() => return Ok(addrs),
            Ok(_) => mark_degraded(),
            Err(err) => tracing::debug!(%host, error = %err, "dns worker failed"),
        }
    }

    // Last resort. A network that blocks both the roots and its own advertised
    // nameserver still has a stub resolver; using it is better than refusing to
    // start. The window keeps a broken recursive path from being retried on
    // every tunnel setup.
    tracing::debug!(%host, "resolving through the system stub resolver");
    let system: Vec<IpAddr> = tokio::net::lookup_host((host, 0u16))
        .await
        .with_context(|| format!("system lookup for {host}"))?
        .map(|addr| addr.ip())
        .collect();
    if system.is_empty() {
        bail!("no address records for {host_owned}");
    }
    Ok(system)
}

/// How long the recursive resolver is skipped after it produced nothing.
const DEGRADED_WINDOW: Duration = Duration::from_secs(60);

fn degraded() -> bool {
    degraded_until() > unix_now()
}

fn mark_degraded() {
    DEGRADED_UNTIL.store(
        unix_now() + DEGRADED_WINDOW.as_secs(),
        AtomicOrdering::Relaxed,
    );
}

fn degraded_until() -> u64 {
    DEGRADED_UNTIL.load(AtomicOrdering::Relaxed)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

static DEGRADED_UNTIL: AtomicU64 = AtomicU64::new(0);

/// Resolves `host:port` to a socket address, preferring IPv4.
///
/// IPv4 is tried first because a v6-first choice fails on hosts with a broken
/// v6 route, which is common on the machines Orbien is deployed to.
pub async fn resolve_socket_addr(host: &str, port: u16) -> Result<SocketAddr> {
    let addrs = tokio::time::timeout(LOOKUP_TIMEOUT, resolve_host(host))
        .await
        .map_err(|_| anyhow!("dns lookup for {host} timed out"))??;
    let ip = addrs
        .iter()
        .find(|ip| ip.is_ipv4())
        .or_else(|| addrs.first())
        .copied()
        .ok_or_else(|| anyhow!("no address records for {host}"))?;
    Ok(SocketAddr::new(ip, port))
}

/// Splits `host:port`, accepting `[v6]:port` and a missing port.
pub fn split_host_port(input: &str, default_port: u16) -> Result<(String, u16)> {
    let input = input.trim();
    if input.is_empty() {
        bail!("empty address");
    }
    if let Some(rest) = input.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or_else(|| anyhow!("unterminated IPv6 literal in {input}"))?;
        let port = match tail.trim_start_matches(':') {
            "" => default_port,
            p => p
                .parse()
                .with_context(|| format!("invalid port in {input}"))?,
        };
        return Ok((host.to_string(), port));
    }
    match input.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => Ok((
            host.to_string(),
            port.parse()
                .with_context(|| format!("invalid port in {input}"))?,
        )),
        _ => Ok((input.to_string(), default_port)),
    }
}

/// Resolves `host:port` or a bare host to a socket address.
pub async fn resolve_endpoint(endpoint: &str, default_port: u16) -> Result<SocketAddr> {
    let (host, port) = split_host_port(endpoint, default_port)?;
    resolve_socket_addr(&host, port).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_host_ports() {
        assert_eq!(
            split_host_port("example.com:443", 80).unwrap(),
            ("example.com".into(), 443)
        );
        assert_eq!(
            split_host_port("example.com", 80).unwrap(),
            ("example.com".into(), 80)
        );
        assert_eq!(
            split_host_port("[::1]:9000", 80).unwrap(),
            ("::1".into(), 9000)
        );
        assert_eq!(split_host_port("::1", 80).unwrap(), ("::1".into(), 80));
        assert!(split_host_port("example.com:notaport", 80).is_err());
    }

    #[tokio::test]
    async fn literals_skip_the_resolver() {
        assert_eq!(
            resolve_host("127.0.0.1").await.unwrap(),
            vec![IpAddr::from([127, 0, 0, 1])]
        );
        assert_eq!(
            resolve_host("localhost").await.unwrap()[0],
            IpAddr::from([127, 0, 0, 1])
        );
    }

    /// Network-dependent: `cargo test -p orbien-core -- --ignored`.
    #[tokio::test]
    #[ignore = "requires outbound DNS"]
    async fn resolves_a_public_name() {
        let addrs = resolve_host("example.com").await.unwrap();
        assert!(!addrs.is_empty());
    }
}
