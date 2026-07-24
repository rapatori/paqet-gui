# paqet-gui

[![CI](https://github.com/rapatori/paqet-gui/actions/workflows/ci.yml/badge.svg)](https://github.com/rapatori/paqet-gui/actions/workflows/ci.yml)

`paqet-gui` is an independent, unofficial Windows desktop companion for the [`hanselime/paqet`](https://github.com/hanselime/paqet) client. It is intended to provide a small graphical workflow for managing server profiles, selecting a local network interface, generating a compatible client configuration, controlling the local paqet process, and viewing its output.

This project is not affiliated with or endorsed by the upstream paqet project.

## Project Status

`paqet-gui` is in early development. The repository currently contains the validated Tauri/Svelte application scaffold, the pinned paqet compatibility contract, native profile validation and persistence, and typed Windows network-interface discovery. Profile UI and IPC integration, configuration generation, process control, the production interface, and release packaging are not implemented yet.

There are no supported binaries or installers. Do not treat the current application shell as a functional paqet client.

## Planned V1 Scope

- Windows 10 and Windows 11 on x64.
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

- Windows 10 or Windows 11 x64.
- [Npcap](https://npcap.com/) installed separately.
- Microsoft Edge WebView2 Runtime. The planned offline-capable installer will install the Evergreen runtime when it is absent.
- Connection details for a compatible remote paqet server.

`paqet-gui` does not install, detect, configure, or troubleshoot Npcap in V1.

### Server Setup

For users who need to set up their own Linux VPS, we recommend the community-maintained [`SamNet-dev/paqctl`](https://github.com/SamNet-dev/paqctl) project. Follow its **Paqet server** setup workflow, then use `paqctl info` to obtain the server address, port, and encryption key for your `paqet-gui` profile.

`paqctl` is an independent third-party project and also supports backends that are outside this application's scope. `paqet-gui` supports Paqet only, not GFW-Knocker. Ensure the server is running the compatible Paqet `v1.0.0-alpha.20` release documented above; do not allow an automatic upgrade to select a different version without checking the compatibility contract.

## Data And Security

The planned application stores profiles, including paqet encryption keys, as versioned plaintext JSON in the current user's application configuration directory. Generated `config.yaml` is also plaintext and is stored separately in the current user's local application data because paqet requires it. These files rely on the Windows user profile's access controls and are not encrypted by the application.

Keys must not be included in logs, diagnostics, issues, or vulnerability reports. See [SECURITY.md](SECURITY.md) for the vulnerability reporting policy.

## Development

Development and validation currently run on Windows with:

- Node.js `24.16.x` and npm `12.0.1`.
- Rust `1.97.0` with the `x86_64-pc-windows-msvc` target, `rustfmt`, and Clippy. The repository's `rust-toolchain.toml` installs these through rustup.
- Microsoft C++ Build Tools and the Windows SDK required by Tauri.
- Microsoft Edge WebView2 Runtime for launching the desktop shell.

Install the locked frontend dependencies:

```powershell
npm.cmd ci
```

Start the current development shell:

```powershell
npm.cmd run tauri -- dev
```

The shell is a development placeholder and does not yet launch paqet.

## Validation

Run the same checks used by CI from a Windows PowerShell session:

```powershell
npm.cmd ci
npm.cmd run format:check
npm.cmd run lint
npm.cmd run check
npm.cmd run test:run
npm.cmd run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
npm.cmd run tauri -- build --debug --no-bundle
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution expectations and pull request guidance.

## License And Notices

`paqet-gui` is available under the [MIT License](LICENSE).

paqet and other third-party components remain governed by their own licenses. The pinned paqet attribution and license text are preserved in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). A complete dependency-license inventory for the bundled upstream executable is still required before any distribution.
