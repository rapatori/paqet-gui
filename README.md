# PaqetGUI

[![CI](https://github.com/rapatori/paqet-gui/actions/workflows/ci.yml/badge.svg)](https://github.com/rapatori/paqet-gui/actions/workflows/ci.yml)

PaqetGUI is an independent, unofficial Windows desktop companion for the [`hanselime/paqet`](https://github.com/hanselime/paqet) client. It is intended to provide a small graphical workflow for managing server profiles, selecting a local network interface, generating a compatible client configuration, controlling the local paqet process, and viewing its output.

This project is not affiliated with or endorsed by the upstream paqet project.

## Project Status

PaqetGUI has completed its initial Windows 11 x64 implementation and pre-release qualification. The repository contains the functional Tauri/Svelte client: native profile persistence, Windows network discovery, deterministic paqet configuration, supervised process lifecycle and logs, typed IPC, and the accessible one-page interface are implemented and covered by automated actual-WebView workflows.

The release packaging configuration produces an unsigned Windows 11 x64 NSIS installer. A previously qualified package is retained as historical evidence while accepted product updates are implemented and requalified. There are no published or supported binaries yet.

## Planned V1 Scope

- Windows 11 on x64.
- One fixed-size desktop window for local paqet client operation.
- Locally persisted server profiles containing a server address, port, and encryption key.
- Windows interface discovery and derivation of the interface, Npcap GUID, local address, and gateway MAC required by paqet.
- Deterministic generation of the supported paqet client configuration.
- Start, monitor, and stop one bundled paqet child process.
- Connection status derived from the pinned paqet process lifecycle and output.
- In-memory display of paqet stdout and stderr.

V1 will not provision or administer a remote server, run a paqet server on Windows, support unrelated forks, or independently verify end-to-end tunnel health.

## Compatibility And Prerequisites

The current compatibility target is `hanselime/paqet` `v1.0.0-alpha.20` for Windows x64. Upstream identifies this release as alpha software, and its wire protocol is not backward compatible with earlier releases. A compatible, separately administered remote paqet server is required.

The detailed source commit, artifact hashes, supported configuration fields, process-output contract, and upgrade procedure are documented in [the paqet compatibility contract](docs/paqet-compatibility.md).

Running the completed V1 application will require:

- Windows 11 x64.
- [Npcap](https://npcap.com/) installed separately.
- A present, serviced [Microsoft Edge WebView2 Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/consumer/). Windows 11 normally includes it, but modified, damaged, or managed systems may need to install or repair it from Microsoft's official page.
- Connection details for a compatible remote paqet server.

The PaqetGUI installer does not embed, download, install, update, or repair WebView2. The application uses the shared Evergreen Runtime serviced by Microsoft; compatibility with every historical runtime version is not guaranteed, and no precise minimum is currently claimed. PaqetGUI also does not install, detect, configure, or troubleshoot Npcap in V1.

### Server Setup

For users who need to set up their own Linux VPS, we recommend the community-maintained [`SamNet-dev/paqctl`](https://github.com/SamNet-dev/paqctl) project. Follow its **Paqet server** setup workflow, then use `paqctl info` to obtain the server address, port, and encryption key for your PaqetGUI profile.

`paqctl` is an independent third-party project and also supports backends that are outside this application's scope. PaqetGUI supports Paqet only, not GFW-Knocker. Ensure the server is running the compatible Paqet `v1.0.0-alpha.20` release documented above; do not allow an automatic upgrade to select a different version without checking the compatibility contract.

## Data And Security

The application stores profiles, including paqet encryption keys, as versioned plaintext JSON in the current user's application configuration directory. Generated `config.yaml` is also plaintext and is stored separately in the current user's local application data because paqet requires it. These files rely on the Windows user profile's access controls and are not encrypted by the application.

Keys must not be included in logs, diagnostics, issues, or vulnerability reports. See [SECURITY.md](SECURITY.md) for the vulnerability reporting policy.

## Development

Development and validation currently run on Windows with:

- Node.js `24.16.x` and npm `12.0.1`.
- Rust `1.97.0` with the `x86_64-pc-windows-msvc` target, `rustfmt`, and Clippy. The repository's `rust-toolchain.toml` installs these through rustup.
- Microsoft C++ Build Tools and the Windows SDK required by Tauri.
- A present, serviced [Microsoft Edge WebView2 Evergreen Runtime](https://developer.microsoft.com/microsoft-edge/webview2/consumer/) for launching the desktop shell.

Install the locked frontend dependencies:

```powershell
npm.cmd ci
```

Start the development application:

```powershell
npm.cmd run tauri -- dev
```

Release maintainers stage the checksum-pinned upstream executable separately. Routine development and CI do not download or commit it. To regenerate audited license resources and build the unsigned x64 NSIS installer, install the pinned license generator and run:

```powershell
cargo install cargo-about --version 0.8.4 --locked
npm.cmd run licenses:paqet
npm.cmd run licenses:rust
npm.cmd run package
```

`npm.cmd run package` verifies the staged sidecar before and after the Tauri build, extracts the resulting setup payload, checks release configuration and required notices, confirms the installer is unsigned, and prints its size and SHA-256. It does not install or run the package.

## Validation

Run the same checks used by CI from a Windows PowerShell session:

```powershell
npm.cmd ci
npm.cmd run format:check
npm.cmd run lint
npm.cmd run check
npm.cmd run test:run
npm.cmd run test:sidecar
node --test scripts/verify-release.test.mjs
npm.cmd run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo test --manifest-path src-tauri/Cargo.toml --locked --features process-test-support --test process_supervision_windows
cargo test --manifest-path src-tauri/Cargo.toml --locked --release --features process-test-support --test process_supervision_windows
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets --features process-test-support -- -D warnings
npm.cmd run tauri -- build --debug --no-bundle
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution expectations and pull request guidance.

## License And Notices

PaqetGUI is available under the [MIT License](LICENSE).

paqet and other third-party components remain governed by their own licenses. Attribution and the installed release-notice layout are documented in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). Complete license inventories for the pinned paqet executable, Windows Rust production graph, frontend runtime, and bundled fonts are included with the installer.
