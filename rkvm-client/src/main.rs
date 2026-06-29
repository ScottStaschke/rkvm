mod client;
mod config;
mod tls;
mod stream;
#[cfg(target_os="windows")]
mod windows;


use clap::Parser;
use client::{Error, init_tracing};
use std::path::PathBuf;
use std::process::ExitCode;
use tokio::io::split;
use tokio::signal;

#[cfg(target_os="windows")]
use windows::{init_stream, init_writers};


#[derive(Parser)]
#[structopt(name = "rkvm-client", about = "The rkvm client application")]
pub struct Args {
    #[clap(help = "Path to configuration file")]
    config_path: PathBuf,
    #[clap(long, default_value = "info", help = "log filter")]
    log_level: String,
    #[clap(long, help = "output file for the logs")]
    log_file: Option<PathBuf>,
}

#[cfg(not(target_os="windows"))]
async fn process_args(args: &Args) -> Result<RkvmStream,Error> {
    let config = client::init_config(&args.config_path).await?;
    let connector = tls::configure(&config.certificate).await?;
    client::init_stream(&config.server.hostname, config.server.port, &connector, &config.password).await
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    init_tracing(&args.log_level, &args.log_file);

    tracing::info!("Client starting...");
    let stream = match init_stream(&args).await {
        Ok(stream) => stream,
        Err(e) => {
            tracing::error!("Failed to open stream {}", e);
            return ExitCode::FAILURE;
        }
    };

    let writers = init_writers();

    let (mut r, mut w) = split(stream);
    tokio::select! {
        result = client::run(&mut r, &mut w, writers) => {
            if let Err(err) = result {
                tracing::error!("Error: {}", err);
                return ExitCode::FAILURE;
            }
        }
        // This is needed to properly clean libevdev stuff up.
        result = signal::ctrl_c() => {
            if let Err(err) = result {
                tracing::error!("Error setting up signal handler: {}", err);
                return ExitCode::FAILURE;
            }

            tracing::info!("Exiting on signal");
        }
    }

    ExitCode::SUCCESS
}

