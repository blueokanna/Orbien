mod access;
mod control;
mod dashboard;
mod metrics;
mod service;
mod tunnel;

use anyhow::Result;
use clap::Parser;
use orbien_core::config::ServerConfig;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "orbien-server",
    about = "orbien server",
    after_help = "Example:\n  orbien-server -c conf/orbien-server.toml"
)]
struct Args {
    #[arg(short, long, value_name = "FILE")]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let args = Args::parse();
    let cfg = load_server_config(&args)?;

    tracing::info!(
        listen = %cfg.listen,
        quic_port = cfg.quic_port,
        kcp_port = cfg.kcp_port,
        http_gw = cfg.http_gw_port,
        https_gw = cfg.https_gw_port,
        "starting orbien-server"
    );

    service::Service::new(cfg)?.run().await
}

fn load_server_config(args: &Args) -> Result<ServerConfig> {
    if let Some(path) = args
        .config
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        tracing::info!(config = %path, "loading config");
        return ServerConfig::load(path);
    }

    tracing::info!("using built-in defaults");
    let mut cfg = ServerConfig::default();
    cfg.complete();
    cfg.validate()?;
    Ok(cfg)
}
