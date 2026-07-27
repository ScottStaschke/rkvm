# rkvm

rkvm shares a keyboard and mouse between computers by forwarding raw Linux
input events over an encrypted connection. The Linux server captures the
physical input devices, and each configured shortcut cycles control between the
server and connected clients.

This fork adds an experimental native Windows client that runs as a Windows
service and supports the normal desktop, UAC secure desktop, and the Windows
lock/sign-in desktop.

## Features

- TLS encryption using [rustls](https://github.com/rustls/rustls)
- Linux server and Linux client
- Native Windows client service
- UAC and lock-screen input support on Windows
- Display-server independent: X11, Wayland, and a graphical session are not
  required
- Raw key forwarding without keyboard-layout translation
- Configurable multi-key switching shortcut

## Compatibility

The Windows client in this fork uses protocol version 6. The server and client
must come from the same release of this fork. The stock RKVM 0.6.1 package uses
protocol version 5 and is not compatible.

## Documentation

- [Windows installation, configuration, and troubleshooting](WINDOWS.md)
- [Declarative NixOS server setup](NIXOS.md)
- [Supported switching keys](switch-keys.md)

## Linux requirements

- The `uinput` kernel module. Confirm that `/dev/uinput` exists.
- libevdev development files (`sudo apt install libevdev-dev` on Debian/Ubuntu)
- Clang/LLVM (`sudo apt install clang` on Debian/Ubuntu)

## Build from source

```console
cargo build --release
sudo install -Dm755 target/release/rkvm-server /usr/local/bin/rkvm-server
sudo install -Dm755 target/release/rkvm-client /usr/local/bin/rkvm-client
sudo install -Dm755 target/release/rkvm-certificate-gen \
  /usr/local/bin/rkvm-certificate-gen
```

For Windows, download the ZIP attached to the latest GitHub release instead of
building manually.

## Basic server setup

Generate a certificate and private key:

```console
sudo install -d -m 0755 /etc/rkvm
sudo rkvm-certificate-gen \
  /etc/rkvm/certificate.pem \
  /etc/rkvm/key.pem \
  --dns-name "$(hostname)" \
  --ip-address 192.0.2.10
sudo chmod 0644 /etc/rkvm/certificate.pem
sudo chmod 0600 /etc/rkvm/key.pem
```

Replace `192.0.2.10` with the server address used by clients. Copy
`example/server.toml` to `/etc/rkvm/server.toml`, then change the password and
switching shortcut.

Test the server before enabling it permanently:

```console
sudo rkvm-server /etc/rkvm/server.toml --shutdown-after 15
```

The example service files are in [`systemd`](systemd).

## Security boundaries

The Windows service runs as LocalSystem because input on UAC and Winlogon
desktops requires elevated desktop access. Its local named pipe is restricted
to LocalSystem and administrators.

Windows deliberately prevents applications from synthesizing
Ctrl+Alt+Delete. Keep a physical input device available for the secure-attention
sequence and during initial testing.

## Project structure

- `rkvm-server` - captures Linux input and forwards it to clients
- `rkvm-client` - Linux client and Windows service/client processes
- `rkvm-input` - Linux input handling and Windows input injection
- `rkvm-net` - protocol encoding, authentication, and TLS
- `rkvm-certificate-gen` - certificate generation utility
- `windows-service` - Windows installer and uninstaller

## Origin and license

This project is based on [htrefil/rkvm](https://github.com/htrefil/rkvm) and
includes work derived from the experimental Windows client by
[Unknow0/rkvm](https://github.com/Unknow0/rkvm).

Contributions are welcome. If you find RKVM useful, you can support the original
author through [Ko-fi](https://ko-fi.com/htrefil).

Licensed under the [MIT License](LICENSE).
