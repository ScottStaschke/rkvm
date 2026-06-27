mod client;
mod config;
mod stream;
mod tls;

use clap::Parser;
use client::{Error, init_tracing, init_config};
use std::path::PathBuf;
use std::process::ExitCode;
use stream::RkvmStream;
use tokio::io::split;
#[cfg(any(not(target_os="windows"), feature="windows-service"))]
use tokio::io::BufStream;
use tokio::signal;

#[cfg(feature="windows-service")]
use tokio::time::{Duration, sleep};

#[cfg(feature="windows-service")]
use tokio::net::windows::named_pipe::ClientOptions;
#[cfg(feature="windows-service")]
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
#[cfg(all(target_os="windows", not(feature="windows-service")))]
use rkvm_input::windows::writer::WritersWindows;
#[cfg(target_os="windows")]
use rkvm_input::windows::writer_simple::WriterWindowsSimple;

#[derive(Parser)]
#[structopt(name = "rkvm-client", about = "The rkvm client application")]
struct Args {
    #[clap(help = "Path to configuration file")]
    config_path: PathBuf,
    #[clap(long, default_value = "info", help = "log filter")]
    log_level: String,
    #[clap(long, help = "output file for the logs")]
    log_file: Option<PathBuf>,
}

#[cfg(any(not(target_os="windows"), not(feature="windows-service")))]
async fn process_args(args: &Args) -> Result<RkvmStream,Error> {
    let config = init_config(&args.config_path).await?;
    let connector = tls::configure(&config.certificate).await?;
    client::init_stream(&config.server.hostname, config.server.port, &connector, &config.password).await
}

#[cfg(feature="windows-service")]
async fn process_args(args: &Args) -> Result<RkvmStream,Error> {
    let pipe = loop {
        match ClientOptions::new().open(args.config_path.clone())  {
            Ok(client) => break client,
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => (),
            Err(e) => return Err(Error::Io(e)),
        }

        sleep(Duration::from_millis(50)).await;
    };
    Ok(RkvmStream::Pipe(BufStream::with_capacity(1024, 1024, pipe)))
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    init_tracing(&args.log_level, &args.log_file);

    tracing::info!("Client starting...");
    let stream = match process_args(&args).await {
        Ok(stream) => stream,
        Err(e) => {
            tracing::error!("Failed to open stream {}", e);
            return ExitCode::FAILURE;
        }
    };

    #[cfg(all(target_os="windows", not(feature="windows-service")))]
    let update = WritersWindows::new(WriterWindowsSimple::new());
    #[cfg(feature="windows-service")]
    let update = WriterWindowsSimple::new();

    let (mut r, mut w) = split(stream);
    tokio::select! {
        result = client::run(&mut r, &mut w, update) => {
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

