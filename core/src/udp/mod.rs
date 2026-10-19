use crate::msg::{Message, Ping, UdpPacket, UdpSocketAddr};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::time::Instant;

pub const CHANNEL_CAP: usize = 1024;
pub const CLIENT_IDLE: Duration = Duration::from_secs(30);
pub const DATA_PING_INTERVAL: Duration = Duration::from_secs(30);
pub const SERVER_DATA_READ_DEADLINE: Duration = Duration::from_secs(60);

pub async fn forward_user_conn(
    udp: Arc<UdpSocket>,
    mut read_rx: mpsc::Receiver<UdpPacket>,
    send_tx: mpsc::Sender<UdpPacket>,
    buf_size: usize,
) {
    let udp_w = Arc::clone(&udp);
    let writer = tokio::spawn(async move {
        while let Some(pkt) = read_rx.recv().await {
            let Some(remote) = pkt.remote_addr.as_ref().and_then(|a| a.to_std()) else {
                continue;
            };
            if let Err(e) = udp_w.send_to(&pkt.content, remote).await {
                tracing::debug!(error = %e, "udp data→local write failed");
                break;
            }
        }
    });

    let mut buf = vec![0u8; buf_size.max(512)];
    loop {
        match udp.recv_from(&mut buf).await {
            Ok((n, remote)) => {
                let pkt = UdpPacket::new(buf[..n].to_vec(), Some(remote));
                if send_tx.try_send(pkt).is_err() {
                    tracing::trace!("udp sendCh full; drop packet");
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "udp local→data read ended");
                break;
            }
        }
    }

    writer.abort();
}

pub async fn forwarder(
    local_addr: SocketAddr,
    mut read_rx: mpsc::Receiver<UdpPacket>,
    send_tx: mpsc::Sender<Message>,
    buf_size: usize,
    proxy_protocol_version: Option<String>,
) {
    let map: Arc<Mutex<HashMap<SocketAddr, Arc<UdpSocket>>>> = Arc::new(Mutex::new(HashMap::new()));
    let pp_ver = proxy_protocol_version
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    while let Some(pkt) = read_rx.recv().await {
        let Some(remote) = pkt.remote_addr.as_ref().and_then(|a| a.to_std()) else {
            continue;
        };

        let existing = {
            let guard = map.lock().unwrap_or_else(|e| e.into_inner());
            guard.get(&remote).map(Arc::clone)
        };

        let (udp_conn, is_new) = match existing {
            Some(conn) => (conn, false),
            None => {
                let sock = match UdpSocket::bind("0.0.0.0:0").await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::debug!(error = %e, "udp bind ephemeral failed");
                        continue;
                    }
                };
                if let Err(e) = sock.connect(local_addr).await {
                    tracing::debug!(error = %e, %local_addr, "udp dial local failed");
                    continue;
                }
                let sock = Arc::new(sock);
                let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
                match guard.entry(remote) {
                    std::collections::hash_map::Entry::Occupied(e) => (Arc::clone(e.get()), false),
                    std::collections::hash_map::Entry::Vacant(e) => {
                        e.insert(Arc::clone(&sock));
                        (sock, true)
                    }
                }
            }
        };

        let payload = if is_new {
            if let Some(ref ver) = pp_ver {
                match crate::net::build_proxy_protocol_header(remote, local_addr, ver) {
                    Ok(hdr) => {
                        let mut buf = Vec::with_capacity(hdr.len() + pkt.content.len());
                        buf.extend_from_slice(&hdr);
                        buf.extend_from_slice(&pkt.content);
                        buf
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "udp PROXY Protocol header build failed; send raw");
                        pkt.content
                    }
                }
            } else {
                pkt.content
            }
        } else {
            pkt.content
        };

        if let Err(e) = udp_conn.send(&payload).await {
            tracing::debug!(error = %e, "udp write to local failed");
            map.lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&remote);
            continue;
        }

        if is_new {
            let map_w = Arc::clone(&map);
            let send_tx = send_tx.clone();
            let udp_conn = Arc::clone(&udp_conn);
            tokio::spawn(async move {
                writer_fn(LocalReaderCtx {
                    udp_conn,
                    remote,
                    map: map_w,
                    send_tx,
                    buf_size,
                })
                .await;
            });
        }
    }

    map.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

struct LocalReaderCtx {
    udp_conn: Arc<UdpSocket>,
    remote: SocketAddr,
    map: Arc<Mutex<HashMap<SocketAddr, Arc<UdpSocket>>>>,
    send_tx: mpsc::Sender<Message>,
    buf_size: usize,
}

async fn writer_fn(ctx: LocalReaderCtx) {
    let mut buf = vec![0u8; ctx.buf_size.max(512)];
    // One timer per connection, reset on activity. A `timeout` around every
    // `recv` would register and cancel a timer per datagram.
    let idle = tokio::time::sleep(CLIENT_IDLE);
    tokio::pin!(idle);

    loop {
        tokio::select! {
            received = ctx.udp_conn.recv(&mut buf) => {
                let Ok(n) = received else { break };
                idle.as_mut().reset(Instant::now() + CLIENT_IDLE);
                let pkt = UdpPacket {
                    content: buf[..n].to_vec(),
                    local_addr: None,
                    remote_addr: Some(UdpSocketAddr::from_std(ctx.remote)),
                };
                if ctx.send_tx.try_send(Message::UdpPacket(pkt)).is_err() {
                    break;
                }
            }
            _ = &mut idle => break,
        }
    }
    ctx.map
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&ctx.remote);
}

pub fn spawn_data_ping(send_tx: mpsc::Sender<Message>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(DATA_PING_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        tick.tick().await;
        loop {
            tick.tick().await;
            if send_tx.try_send(Message::Ping(Ping::default())).is_err() {
                break;
            }
        }
    })
}
