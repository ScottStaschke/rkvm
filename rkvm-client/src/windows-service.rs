#![cfg(target_os="windows")]
mod client;
mod config;
mod stream;
mod tls;
mod windows_daemon;

use crate::stream::{RkvmWriter, LockWriter};
use crate::windows_daemon::{writer::ClientWriter, client_process::ClientProcess};

use client::{init_tracing, init_config, Error};
use rkvm_input::windows::writer::WritersWindows;
use rkvm_net::Update;
use std::io::ErrorKind;
use std::ffi::{OsString, CStr, c_void};
use std::path::PathBuf;
use std::ptr::{addr_of_mut, null_mut};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{ReadHalf, WriteHalf, split};
use tokio::net::windows::named_pipe::{ServerOptions, NamedPipeServer};
use tokio::sync::{Mutex, Notify};
use tokio::time::interval;
use tracing::Instrument;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{SECURITY_ATTRIBUTES, GetTokenInformation, TokenLinkedToken, TOKEN_ALL_ACCESS, TOKEN_LINKED_TOKEN, PSECURITY_DESCRIPTOR};
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPPROCESS, PROCESSENTRY32, Process32First, Process32Next };
use windows::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken, ProcessIdToSessionId};
use windows::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_ALL_ACCESS};
use windows::core::{PCWSTR};
use windows_service::define_windows_service;
use windows_service::service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,ServiceType};
use windows_service::service_control_handler::{register, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

const SERVICE_NAME: &str = "RkvmService";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;
const SERVICE_LOG: &str = r"C:\ProgramData\rkvm\service.log";
const SERVICE_CFG: &str = r"C:\ProgramData\rkvm\client.toml";
const SERVICE_PIPE: &str = r"\\.\pipe\rkvm";
const CLIENT_PATH: &str = r"C:\ProgramData\rkvm\rkvm-client.exe";
const CLIENT_LOG: &str = r"C:\ProgramData\rkvm\client.log";

define_windows_service!(ffi_service_main, service_main);

fn main() -> windows_service::Result<()> {
    service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
    Ok(())
}

fn service_main(_arguments: Vec<OsString>) {
    init_tracing(&"info".to_string(), &Some(PathBuf::from(SERVICE_LOG)));
    if let Err(e) = run_service() {
        tracing::error!("Service error: {:?}", e);
    }
}

fn run_service() -> windows_service::Result<()> {
    let stop_notify = Arc::new(Notify::new());
    let stop_clone = stop_notify.clone();
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                tracing::info!("Service stop requested");
                stop_clone.notify_waiters();
                ServiceControlHandlerResult::NoError
            }

            ServiceControl::SessionChange(change) => {
                tracing::info!("Session change: {:?}", change);
                ServiceControlHandlerResult::NoError
            }

            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = register(SERVICE_NAME, event_handler)?;

    let status = ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP
            | ServiceControlAccept::SESSION_CHANGE,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    };
    status_handle.set_service_status(status)?;

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
    match rt.block_on(process(stop_notify.clone())) {
          Ok(_) => {
            tracing::info!("Service stopped normally");

            status_handle.set_service_status(ServiceStatus {
                service_type: SERVICE_TYPE,
                current_state: ServiceState::Stopped,
                controls_accepted: ServiceControlAccept::empty(),
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })?;
        }
        Err(e) => {
            tracing::error!(error = ?e, "Service crashed");

            status_handle.set_service_status(ServiceStatus {
                service_type: SERVICE_TYPE,
                current_state: ServiceState::Stopped,
                controls_accepted: ServiceControlAccept::empty(),
                exit_code: ServiceExitCode::Win32(1),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })?;
        }
    }
    Ok(())
}

async fn process(stop_notify: Arc<Notify>) -> Result<(), Error> {
    tracing::info!("Service started");

    let mut cl:ClientProcess = ClientProcess::new().await?;

    let config = init_config(SERVICE_CFG).await?;
    let connector = tls::configure(&config.certificate).await?;
    let stream = client::init_stream(&config.server.hostname, config.server.port, &connector, &config.password).await?;
    let (mut stream_r, mut stream_w) = split(stream);

    let srv_update = WritersWindows::new(ClientWriter::new(cl.writer()));

    let ping_w = cl.writer();
    let span_server = tracing::info_span!("run", stream="server");
    let span_client = tracing::info_span!("run", stream="client");
    tokio::select! {
        res = client::run(&mut stream_r, &mut stream_w, srv_update).instrument(span_server) => res,
        res = cl.run().instrument(span_client) => res,
        res = ping_sender(ping_w) => res,
        _ = stop_notify.notified() => {
            tracing::info!("Service requested stop");
            Ok(())
        }
    }?;
    Ok(())
}

async fn ping_sender(mut w: LockWriter<WriteHalf<NamedPipeServer>>) -> Result<(),Error> {
    let mut interval = interval(rkvm_net::PING_INTERVAL);
    interval.tick().await;
    loop {
        interval.tick().await;
        w.send(Update::Ping).await?
    }
}
