#![cfg(target_os="windows")]
mod client;
mod config;
mod stream;
mod tls;
mod windows_daemon;

use crate::stream::{RkvmWriter, LockWriter};
use crate::windows_daemon::client_process::ClientProcess;

use client::{init_tracing, init_config, Error};
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

    let pipe = new_pipe()?;
    let connect = pipe.connect();

    let mut cmd: Vec<u16> = to_wide(format!("\"{}\" --log-file {} --pipe {}", CLIENT_PATH, CLIENT_LOG, SERVICE_PIPE).as_str());
    let _guard = ClientProcess::new(cmd);
    connect.await?;

    let (pipe_r, pipe_w) = split(pipe);

    let config = init_config(SERVICE_CFG).await?;
    let connector = tls::configure(&config.certificate).await?;
    let stream = client::init_stream(&config.server.hostname, config.server.port, &connector, &config.password).await?;
    let (mut stream_r, mut stream_w) = split(stream);

    let pw = Arc::new(Mutex::new(pipe_w));
    let pipe_w = LockWriter::new(pw.clone());
    let srv_update = |update| async {
        tracing::debug!("Forward {:?}", update);
        pw.lock().await.send(update).await
    };

    let span_server = tracing::info_span!("run", stream="server");
    let span_client = tracing::info_span!("run", stream="client");
    tokio::select! {
        res = client::run(&mut stream_r, &mut stream_w, srv_update).instrument(span_server) => res,
        res = run_client(pipe_r, pipe_w).instrument(span_client) => res,
        res = ping_sender(pw.clone()) => res,
        _ = stop_notify.notified() => {
            tracing::info!("Service requested stop");
            Ok(())
        }
    }?;
    Ok(())
}

async fn ping_sender(pw: Arc<Mutex<WriteHalf<NamedPipeServer>>>) -> Result<(),Error> {
    let mut interval = interval(rkvm_net::PING_INTERVAL);
    interval.tick().await;
    loop {
        interval.tick().await;
        pw.lock().await.send(Update::Ping).await?
    }
}

async fn run_client(mut reader: ReadHalf<NamedPipeServer>, mut writer: LockWriter<WriteHalf<NamedPipeServer>>) -> Result<(), Error> {
    let client_update = |update| async move {
        // handle missing communication
        tracing::debug!("client respond {:?}", update);
        Ok(())
    };
    loop {
        let res = client::run(&mut reader, &mut writer, client_update).await;
        if let Err(ref e) = res {
            tracing::info!("Client error: {:?}", e);
            match e {
                Error::Network(ref io) => {
                    if io.kind() == ErrorKind::UnexpectedEof {
                        continue;
                    }
                }
                _ => {}
            }
        }
        return res;
    }
}

fn new_pipe() -> Result<NamedPipeServer, Error> {
    unsafe {
        let sddl = to_wide("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;AU)");

        let mut security_descriptor: PSECURITY_DESCRIPTOR = PSECURITY_DESCRIPTOR(null_mut());

        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut security_descriptor,
            None
        )?;
        tracing::info!("Created descriptor");

        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security_descriptor.0,
            bInheritHandle: false.into(),
        };
        tracing::info!("Created security attributes");
        
        let pipe=ServerOptions::new().first_pipe_instance(true).write_dac(true).create_with_security_attributes_raw(SERVICE_PIPE, addr_of_mut!(sa) as *mut c_void)?;
        tracing::info!("Created pipe");

        Ok(pipe)
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}


