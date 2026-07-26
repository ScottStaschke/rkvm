use std::mem::size_of;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant};
use windows::{
    core,
    Win32::{
        Foundation::{GetLastError, GENERIC_WRITE},
        System::StationsAndDesktops::{
            CloseDesktop, OpenInputDesktop, SetThreadDesktop, DESKTOP_ACCESS_FLAGS,
            DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, DESKTOP_SWITCHDESKTOP,
        },
        UI::Input::KeyboardAndMouse::{INPUT, SendInput},
    },
};

const DESKTOP_REFRESH_INTERVAL: Duration = Duration::from_millis(10);
const INJECTION_LOG_LIMIT: usize = 20;

static TX: LazyLock<Sender<INPUT>> = LazyLock::new(create);
static INJECTION_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

pub fn send_input(input: INPUT) {
    if let Err(error) = TX.send(input) {
        tracing::error!("Windows input injector stopped: {}", error);
    }
}

fn create() -> Sender<INPUT> {
    let (tx, rx) = channel();
    spawn(rx);
    tx
}

struct DesktopBinding {
    handle: Option<windows::Win32::System::StationsAndDesktops::HDESK>,
    last_refresh: Option<Instant>,
}

impl DesktopBinding {
    fn new() -> Self {
        Self {
            handle: None,
            last_refresh: None,
        }
    }

    fn refresh(&mut self, force: bool) -> Result<(), core::Error> {
        if !force
            && self
                .last_refresh
                .is_some_and(|last| last.elapsed() < DESKTOP_REFRESH_INTERVAL)
        {
            return Ok(());
        }

        self.last_refresh = Some(Instant::now());

        unsafe {
            // OpenInputDesktop follows desktop transitions. In particular, UAC and
            // the lock screen switch away from WinSta0\Default to a secure desktop.
            // The injector thread owns no windows or hooks, so Windows permits it
            // to change its desktop with SetThreadDesktop.
            let next = OpenInputDesktop(
                DESKTOP_CONTROL_FLAGS::default(),
                false,
                DESKTOP_ACCESS_FLAGS(
                    DESKTOP_SWITCHDESKTOP.0 | DESKTOP_READOBJECTS.0 | GENERIC_WRITE.0,
                ),
            )?;

            if let Err(error) = SetThreadDesktop(next) {
                let _ = CloseDesktop(next);
                return Err(error);
            }

            if let Some(previous) = self.handle.replace(next) {
                let _ = CloseDesktop(previous);
            }
        }

        Ok(())
    }
}

impl Drop for DesktopBinding {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            unsafe {
                let _ = CloseDesktop(handle);
            }
        }
    }
}

fn spawn(rx: Receiver<INPUT>) -> thread::JoinHandle<Result<(), core::Error>> {
    thread::Builder::new()
        .name("rkvm-input-injector".into())
        .spawn(move || {
            let mut desktop = DesktopBinding::new();

            if let Err(error) = desktop.refresh(true) {
                tracing::warn!("Failed to attach to the initial input desktop: {:?}", error);
            }

            while let Ok(input) = rx.recv() {
                if let Err(error) = desktop.refresh(false) {
                    tracing::warn!("Failed to follow input desktop transition: {:?}", error);
                }

                let mut sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
                let attempt_number = INJECTION_ATTEMPTS.fetch_add(1, Ordering::Relaxed);

                if attempt_number < INJECTION_LOG_LIMIT {
                    tracing::info!(
                        attempt_number = attempt_number + 1,
                        input_type = input.r#type.0,
                        sent,
                        last_error = ?unsafe { GetLastError() },
                        "SendInput result"
                    );
                }

                if sent == 0 {
                    let first_error = unsafe { GetLastError() };
                    tracing::warn!(
                        "SendInput failed ({:?}); rebinding to the input desktop and retrying",
                        first_error
                    );

                    if desktop.refresh(true).is_ok() {
                        sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
                    }

                    if sent == 0 {
                        tracing::error!(
                            "SendInput still failed after desktop rebind: {:?}",
                            unsafe { GetLastError() }
                        );
                    }
                }
            }

            Ok(())
        })
        .expect("failed to create Windows input injector thread")
}
