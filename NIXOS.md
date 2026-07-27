# Declarative NixOS server

The Windows client uses protocol 6, so NixOS must build the server from this
fork rather than using the stock `pkgs.rkvm` package.

## Flake input

Pin the release source without treating it as a flake:

```nix
inputs.rkvm-windows = {
  url = "github:ScottStaschke/rkvm/v0.6.1-windows.1";
  flake = false;
};
```

## Package

The repository root is a Cargo workspace, so the package builds the required
executables explicitly:

```nix
rkvmPackage = pkgs.rustPlatform.buildRustPackage {
  pname = "rkvm";
  version = "0.6.1-windows.1";
  src = inputs.rkvm-windows;

  cargoLock.lockFile = "${inputs.rkvm-windows}/Cargo.lock";

  nativeBuildInputs = [
    pkgs.pkg-config
    pkgs.rustPlatform.bindgenHook
  ];

  buildInputs = [
    pkgs.libevdev
  ];

  cargoBuildFlags = [
    "--package=rkvm-server"
    "--package=rkvm-certificate-gen"
  ];

  doCheck = false;

  installPhase = ''
    runHook preInstall
    mkdir -p "$out/bin"

    server="$(find target -type f -path '*/release/rkvm-server' -print -quit)"
    certgen="$(
      find target \
        -type f \
        -path '*/release/rkvm-certificate-gen' \
        -print \
        -quit
    )"

    test -x "$server"
    test -x "$certgen"

    install -Dm755 "$server" "$out/bin/rkvm-server"
    install -Dm755 "$certgen" "$out/bin/rkvm-certificate-gen"
    runHook postInstall
  '';
};
```

Install `rkvmPackage` system-wide and use these executables for the certificate
and server services:

```nix
environment.systemPackages = [ rkvmPackage ];
```

## Configuration

Store the editable server configuration wherever your configuration management
expects it and expose it as `/etc/rkvm/server.toml`.

Example:

```toml
listen = "0.0.0.0:5258"
switch-keys = ["left-meta", "left-alt", "space"]
propagate-switch-keys = false
certificate = "/etc/rkvm/certificate.pem"
key = "/etc/rkvm/key.pem"
password = "replace-this-password"
```

An out-of-store symlink can keep `/etc/rkvm/server.toml` connected to an
editable dotfile:

```nix
mkOutOfStoreSymlink = path:
  pkgs.runCommandLocal "rkvm-server-config" { } ''
    ln -s ${lib.escapeShellArg path} "$out"
  '';

environment.etc."rkvm/server.toml".source =
  mkOutOfStoreSymlink rkvmConfigPath;
```

When Home Manager owns the dotfile, set `rkvmConfigPath` to the resulting
Home Manager path, such as:

```text
/home/<user>/.config/rkvm/server.toml
```

The system module can discover the owning Home Manager user through a custom
option instead of hardcoding a username.

## Certificate generation

Certificates must contain the hostname or IP address used by the Windows
client. A one-shot service can determine the machine's current source address
when the certificate is first generated:

```nix
systemd.tmpfiles.rules = [
  "d /etc/rkvm 0755 root root -"
];

systemd.services.rkvm-certificate = {
  description = "Generate RKVM TLS certificate";
  wantedBy = [ "multi-user.target" ];
  before = [ "rkvm-server.service" ];

  environment.OPENSSL = "${pkgs.openssl}/bin/openssl";

  serviceConfig = {
    Type = "oneshot";
    RemainAfterExit = true;
    UMask = "0077";
    ExecStart = pkgs.writeShellScript "rkvm-certificate-start" ''
      set -euo pipefail

      certificate=/etc/rkvm/certificate.pem
      privateKey=/etc/rkvm/key.pem

      install -d -m 0755 /etc/rkvm

      if [ ! -s "$certificate" ] || [ ! -s "$privateKey" ]; then
        hostName="$(${pkgs.inetutils}/bin/hostname)"
        ipAddress="$(
          ${pkgs.iproute2}/bin/ip -4 route get 1.1.1.1 |
            ${pkgs.gnused}/bin/sed -n 's/.* src \([^ ]*\).*/\1/p' |
            ${pkgs.coreutils}/bin/head -n 1
        )"

        if [ -n "$ipAddress" ]; then
          ${lib.getExe' rkvmPackage "rkvm-certificate-gen"} \
            "$certificate" \
            "$privateKey" \
            --dns-name "$hostName" \
            --dns-name "$hostName.local" \
            --ip-address "$ipAddress" \
            --days 3650
        else
          ${lib.getExe' rkvmPackage "rkvm-certificate-gen"} \
            "$certificate" \
            "$privateKey" \
            --dns-name "$hostName" \
            --dns-name "$hostName.local" \
            --days 3650
        fi
      fi

      chmod 0644 "$certificate"
      chmod 0600 "$privateKey"
    '';
  };
};
```

The directory is mode `0755` so the public certificate can be copied. The
private key remains root-only at mode `0600`.

## Server service

RKVM needs root access to Linux input devices:

```nix
systemd.services.rkvm-server = {
  description = "RKVM server";
  wantedBy = [ "multi-user.target" ];
  wants = [ "network-online.target" ];
  requires = [ "rkvm-certificate.service" ];
  after = [
    "network-online.target"
    "rkvm-certificate.service"
  ];

  serviceConfig = {
    Type = "simple";
    ExecStart =
      "${lib.getExe' rkvmPackage "rkvm-server"} /etc/rkvm/server.toml";
    Restart = "on-failure";
    RestartSec = "2s";
  };
};
```

Allow the configured TCP port if the firewall is enabled:

```nix
networking.firewall.allowedTCPPorts = [ 5258 ];
```

If `uinput` is not already available:

```nix
boot.kernelModules = [ "uinput" ];
```

A kernel change requires rebooting before it becomes active. A module already
provided by the running kernel can usually be loaded immediately with:

```console
sudo modprobe uinput
```

## Verify

```console
systemctl status rkvm-certificate rkvm-server
journalctl -u rkvm-server -n 50 --no-pager
systemctl show rkvm-server -p ExecStart
```

The `ExecStart` path should refer to this fork, not the stock Nixpkgs RKVM
package.
