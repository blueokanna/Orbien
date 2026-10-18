mod access;
mod control;
mod dashboard;
mod metrics;
mod service;
mod tunnel;

use anyhow::Result;
use clap::Parser;
use orbien_core::config::ServerConfig;
use orbien_core::logging;

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
    let args = Args::parse();
    let (cfg, config_src) = load_server_config(&args)?;
    let _guard = logging::init(&cfg.log)?;

    match config_src {
        Some(path) => tracing::info!(config = %path, "config loaded"),
        None => tracing::info!("using built-in defaults"),
    }
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

fn load_server_config(args: &Args) -> Result<(ServerConfig, Option<String>)> {
    if let Some(path) = args
        .config
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        return Ok((ServerConfig::load(path)?, Some(path.to_string())));
    }

    let mut cfg = ServerConfig::default();
    cfg.complete();
    cfg.validate()?;
    Ok((cfg, None))
}
