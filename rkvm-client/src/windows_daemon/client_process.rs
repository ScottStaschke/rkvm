#![cfg(target_os="windows")]

use crate::client::Error;
use std::ffi::CStr;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{DuplicateTokenEx, GetTokenInformation, SecurityImpersonation, TokenLinkedToken, TokenPrimary, SECURITY_ATTRIBUTES, TOKEN_ALL_ACCESS, TOKEN_ASSIGN_PRIMARY, TOKEN_LINKED_TOKEN};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32, TH32CS_SNAPPROCESS};
use windows::Win32::System::RemoteDesktop::{ProcessIdToSessionId, WTSGetActiveConsoleSessionId, WTSQueryUserToken};
use windows::Win32::System::Threading::{CreateProcessAsUserW, OpenProcess, OpenProcessToken, TerminateProcess, CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, PROCESS_ALL_ACCESS, PROCESS_INFORMATION, STARTUPINFOW};
use windows_core::{PWSTR, PCWSTR};

unsafe fn winlogon_pid(session_id: u32) -> u32 {
    let handle = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
        Err(e) => {
            tracing::warn!("Failed to get process snapshot {:?}", e);
            return 0;
        },
        Ok(h) => h
    };
    let mut proc = PROCESSENTRY32::default();
    proc.dwSize = std::mem::size_of::<PROCESSENTRY32>() as u32;
    if let Err(e) = Process32First(handle, &mut proc) {
       tracing::warn!("Failed to get first process {:?}", e);
       return 0;
    }
    let mut logon_sid:u32=0;
    let mut pid:u32=0;
    loop {
        // TOOD compare proc.szExeFile
        let exe = CStr::from_ptr(proc.szExeFile.as_ptr()).to_string_lossy();
        if exe.eq_ignore_ascii_case("winlogon.exe") && ProcessIdToSessionId(proc.th32ProcessID, &mut logon_sid).is_ok() && logon_sid==session_id {
            pid=proc.th32ProcessID;
            break;
        }
        if let Err(e) = Process32Next(handle, &mut proc) {
            tracing::warn!("Failed to get next process {:?}", e);
            break;
        }
    }
    let _ = CloseHandle(handle);
    return pid;
}

unsafe fn usertoken() -> Result<HANDLE,Error> {
    let session_id = WTSGetActiveConsoleSessionId();

    let winlogon = winlogon_pid(session_id);
    tracing::info!("Found winlogon {}", winlogon);

    let mut token: HANDLE = HANDLE::default();
    match OpenProcess(PROCESS_ALL_ACCESS, false, winlogon) {
        Ok(handle) => {
            let r = OpenProcessToken(handle, TOKEN_ASSIGN_PRIMARY|TOKEN_ALL_ACCESS, &mut token);
            let _ = CloseHandle(handle);
            match r {
                Ok(_) => return Ok(token),
                Err(e) => tracing::warn!("Failed to Get winlogon token {:?}", e)
            }
        },
        Err(e) => tracing::warn!("Failed to open winlogon process {:?}", e)
    };
    WTSQueryUserToken(session_id, &mut token)?;
    
    let mut needed = 0 as u32;
    let _ = GetTokenInformation(token, TokenLinkedToken, None, 0, &mut needed);
    let mut buffer = vec![0u8; needed as usize];
    match GetTokenInformation(token, TokenLinkedToken, Some(buffer.as_mut_ptr() as *mut _), needed, &mut needed) {
        Ok(_) => {
            let token_linked: &TOKEN_LINKED_TOKEN = &*(buffer.as_ptr() as *const TOKEN_LINKED_TOKEN);
            token = token_linked.LinkedToken
        },
        Err(e) => tracing::info!("Failed to get linked token {:?}", e)
    };

    return Ok(token);
}

pub struct ClientProcess {
    handle: HANDLE,
}

impl Drop for ClientProcess {
    fn drop(&mut self) {
        tracing::info!("Droping client");
        unsafe {
            let _ = TerminateProcess(self.handle, 0);
            let _ = CloseHandle(self.handle);
        }
    }
}

impl ClientProcess {
    pub fn new(mut cmd: Vec<u16>) -> Result<Self,Error> {
        tracing::info!("launching client");
        unsafe {
            let user_token = usertoken()?;

            let sa = SECURITY_ATTRIBUTES::default();
            let mut primary_token: HANDLE = HANDLE::default();
            DuplicateTokenEx(user_token, TOKEN_ASSIGN_PRIMARY|TOKEN_ALL_ACCESS, Some(&sa), SecurityImpersonation, TokenPrimary, &mut primary_token)?;
            let _ = CloseHandle(user_token);

            let mut si = STARTUPINFOW::default();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;

            let mut pi = PROCESS_INFORMATION::default();

            CreateProcessAsUserW(Some(primary_token), PCWSTR::null(), Some(PWSTR(cmd.as_mut_ptr())), Some(&sa), None, false, CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT, None, PCWSTR::null(), &si, &mut pi)?;

            let _ = CloseHandle(pi.hThread);
            let _ = CloseHandle(primary_token);
            Ok(ClientProcess{ handle: pi.hProcess })
        }
    }
}
