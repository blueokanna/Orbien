use super::Control;
use anyhow::{anyhow, Result};
use orbien_core::msg::{self, Codec, Message, ReqDataConn, StartDataConn};
use orbien_core::transport::DynStream;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

/// A pooled data connection plus the codec its handshake used.
pub struct DataConn {
    pub stream: DynStream,
    pub codec: Codec,
}

impl Control {
    pub async fn push_data_conn(&self, stream: DynStream, codec: Codec) {
        let _ = self.data_tx.send(DataConn { stream, codec }).await;
        self.data_notify.notify_waiters();
    }

    async fn try_pop_data(&self) -> Option<DataConn> {
        let mut rx = self.data_rx.lock().await;
        rx.try_recv().ok()
    }

    async fn spawn_refill(self: &Arc<Self>) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        let ctl = Arc::clone(self);
        self.spawn_bg(async move {
            if ctl.closed.load(Ordering::SeqCst) {
                return;
            }
            let _ = ctl.request_data_conn().await;
        })
        .await;
    }

    pub async fn get_data_conn(self: &Arc<Self>) -> Result<DataConn> {
        if let Some(conn) = self.try_pop_data().await {
            self.spawn_refill().await;
            return Ok(conn);
        }

        self.request_data_conn().await?;

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if self.closed.load(Ordering::SeqCst) {
                return Err(anyhow!("control closed while waiting for data connection"));
            }
            if let Some(conn) = self.try_pop_data().await {
                self.spawn_refill().await;
                return Ok(conn);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!("timeout waiting for data connection"));
            }
            tokio::select! {
                _ = self.data_notify.notified() => {}
                _ = sleep(remaining.min(Duration::from_millis(100))) => {}
            }
        }
    }

    pub(super) async fn request_data_conn(&self) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(anyhow!("control closed"));
        }
        let mut writer = self.writer.lock().await;
        msg::write_msg(
            &mut *writer,
            &Message::ReqDataConn(ReqDataConn {}),
            self.codec(),
        )
        .await?;
        Ok(())
    }

    pub async fn start_data_conn(
        &self,
        conn: DataConn,
        tunnel_name: &str,
        src_addr: String,
        src_port: u16,
        dst_addr: String,
        dst_port: u16,
    ) -> Result<DataConn> {
        let DataConn { mut stream, codec } = conn;
        msg::write_msg(
            &mut stream,
            &Message::StartDataConn(StartDataConn {
                tunnel_name: tunnel_name.to_string(),
                src_addr,
                src_port,
                dst_addr,
                dst_port,
                error: String::new(),
            }),
            codec,
        )
        .await?;
        Ok(DataConn { stream, codec })
    }
}
