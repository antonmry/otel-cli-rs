use clap::Parser;
use std::net::SocketAddr;
use tonic::transport::Server;
use crate::error::DashboardError;
use tokio::sync::mpsc;

mod error;
mod metrics;
mod ui;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = "127.0.0.1:4317")]
    address: SocketAddr,

    #[arg(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<(), DashboardError> {
    let args = Args::parse();

    let log_level = if args.debug { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .init();

    let (tx, rx) = mpsc::unbounded_channel();
    let tui_handle = tokio::spawn(ui::run_tui(rx));

    let addr = args.address;
    let metrics_service = metrics::create_metrics_service(args.debug, tx);

    tracing::info!("Starting OTLP receiver on {}", addr);

    let server_handle = tokio::spawn(
        Server::builder()
            .add_service(metrics_service)
            .serve(addr)
    );

    tokio::select! {
        _ = tui_handle => println!("TUI closed"),
        _ = server_handle => println!("Server closed"),
    }

    Ok(())
}
