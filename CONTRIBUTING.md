# Contributing

Thank you for considering a contribution to `paqet-gui`. The project is an early-stage, Windows-only desktop companion for a pinned paqet client release. Changes should preserve that deliberately narrow boundary unless an issue or maintainer discussion explicitly expands it.

## Before Starting

- Check existing issues and pull requests to avoid duplicating work.
- Open an issue before making a large feature, architecture, dependency, compatibility, or user-experience change.
- Keep pull requests focused on one independently reviewable concern.
- Do not include server credentials, encryption keys, generated `config.yaml`, private logs, build output, or upstream binaries in a contribution.
- Use [the security policy](SECURITY.md), not a public issue, for suspected vulnerabilities.

The supported upstream behavior is version-specific. Changes involving paqet configuration, invocation, output parsing, or packaging must remain consistent with [the compatibility contract](docs/paqet-compatibility.md) and update its pinned evidence when the upstream version changes.

## Development Setup

Development is supported on Windows 11 x64. Install:

- Node.js `24.16.x` and npm `12.0.1`.
- [rustup](https://rustup.rs/). The repository pins Rust `1.97.0`, the MSVC target, `rustfmt`, and Clippy in `rust-toolchain.toml`.
- Microsoft C++ Build Tools and the Windows SDK required by Tauri.
- Microsoft Edge WebView2 Runtime.

Install locked frontend dependencies with:

```powershell
npm.cmd ci
```

Run the desktop development shell with:

```powershell
npm.cmd run tauri -- dev
```

The development application is a functional paqet client. Its deterministic test suite uses local fixtures and helper executables, so Npcap, server credentials, and the upstream paqet binary are not required for routine validation.

Release-sidecar builds use `npm.cmd run tauri:sidecar`. That command requires the exact pinned executable at `src-tauri/binaries/paqet_windows_amd64-x86_64-pc-windows-msvc.exe`, verifies its byte length and SHA-256, and then applies `src-tauri/tauri.sidecar.conf.json`. The upstream binary remains untracked and must not be included in contributions.

## Engineering Expectations

- Keep trusted behavior in Rust: persistence, configuration generation, network inspection, process lifecycle, log classification, and canonical application state.
- Keep the Svelte frontend focused on presentation, form drafts, and typed calls to the narrow Tauri API.
- Preserve deny-by-default Tauri capabilities and do not expose arbitrary shell, filesystem, network, executable, or argument access to the webview.
- Never log or place encryption keys in errors, test snapshots, DOM attributes, or fixtures.
- Add focused tests for behavior changes. Use Rust tests for native behavior and Vitest with Testing Library for Svelte behavior.
- Keep Windows-specific system access behind small typed boundaries that can be tested deterministically.
- Avoid drive-by formatting, dependency updates, or unrelated refactors.

## Validation

Run the complete Windows validation baseline before submitting a pull request:

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

Release packaging additionally requires the checksum-pinned paqet sidecar and `cargo-about 0.8.4`. Use the documented commands in [README.md](README.md); do not publish an installer until its separate install, launch, and uninstall qualification is complete.

Pull requests should explain the user-visible or technical problem, the chosen solution, relevant security or compatibility implications, and the exact validation performed. CI must pass before merge.

## Documentation And Licensing

Update public documentation when behavior, prerequisites, compatibility, security properties, or development commands change. Keep third-party attribution in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) accurate whenever dependencies or bundled artifacts change.

By submitting a contribution, you agree that it is licensed under the project's [MIT License](LICENSE). Contributions must be your own work or include the permissions and attribution required for incorporated third-party material.
