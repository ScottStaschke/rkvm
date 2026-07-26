# RKVM Windows client

This branch contains an experimental Windows client for an RKVM server running
on Linux. The service runs as LocalSystem and launches the input injector on the
interactive window station.

## UAC and lock-screen behavior

The injector follows changes to the active input desktop. This allows it to
continue working when Windows switches from `WinSta0\Default` to a UAC or
Winlogon desktop, provided the RKVM service is installed and running.

Windows deliberately prevents applications from synthesizing the secure
attention sequence (`Ctrl+Alt+Delete`). If the computer's security policy
requires that sequence before sign-in, use the computer's physical keyboard or
a signed virtual HID driver. RKVM does not bypass that Windows security
boundary.

Input on UAC and sign-in surfaces should be treated as experimental until it has
been tested on the target Windows version and security policy. Keep a physical
keyboard and mouse connected during testing.

## Install

1. Put the server's `certificate.pem` and an edited `client.toml` beside
   `install.bat`.
2. Run `install.bat` from an Administrator command prompt.
3. Check `C:\ProgramData\rkvm\rkvm-service.log` and
   `C:\ProgramData\rkvm\client.log`.

The installer copies the configuration, certificate, service, and client to
`C:\ProgramData\rkvm` and starts the `RkvmService` service.

## Uninstall

Run `uninstall.bat` from an Administrator command prompt. Configuration and log
files are retained in `C:\ProgramData\rkvm`.
