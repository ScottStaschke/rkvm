# RKVM 0.6.1 Windows Preview 1

This preview adds a native Windows client to the RKVM Linux input-sharing
server.

## Highlights

- Installs as an automatically started LocalSystem Windows service.
- Launches a supervised input injector in the active interactive session.
- Follows Windows desktop changes for the normal desktop, UAC prompts, and the
  lock/sign-in desktop.
- Uses TLS and password authentication with the Linux server.
- Restarts automatically after failures and session changes.
- Restricts the local service pipe to LocalSystem and administrators.
- Includes an Administrator installer, uninstaller, log rotation, and packaged
  Windows ZIP.
- Documents Windows setup, declarative NixOS packaging, certificate generation,
  protocol compatibility, and VirtualBox testing.

## Compatibility

This preview uses protocol version 6. Use the server and Windows client from
this same release. The stock RKVM 0.6.1 server uses protocol version 5 and is
not compatible.

## Important limitations

- Windows blocks software-generated Ctrl+Alt+Delete. Keep a physical keyboard
  available when the secure-attention sequence is required.
- UAC and lock-screen input depend on Windows version and security policy and
  remain experimental.
- When the Linux server runs inside VirtualBox on the Windows client computer,
  VirtualBox mouse integration or input capture can route injected events back
  into the guest. See `WINDOWS.md`.

## Installation

Download `rkvm-windows-x86_64.zip`, then follow `WINDOWS.md` inside the archive
or in the repository.
