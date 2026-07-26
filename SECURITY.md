# Security Policy

## Supported Versions

`paqet-gui` is in early development and has not published a supported release. The current source tree receives security fixes on a best-effort basis. No binary or installer should currently be treated as production-ready.

This policy will be updated with explicit supported release lines before the first public release.

## Reporting A Vulnerability

Do not disclose a suspected vulnerability in a public issue, discussion, pull request, log, or screenshot.

Use the repository's **Security** tab and select **Report a vulnerability** to send a private report through GitHub Private Vulnerability Reporting. If that option is unavailable, open a public issue containing only a request for a private reporting channel; do not include vulnerability details until a maintainer provides one.

Include only the information needed to investigate safely:

- Affected commit or version.
- A concise description of the impact and preconditions.
- Reproduction steps or a minimal proof of concept.
- Relevant Windows, WebView2, Npcap, and paqet versions.
- Whether the issue may expose profile data, encryption keys, generated configuration, arbitrary process execution, or process-supervision failures.

Never include working server credentials, encryption keys, generated `config.yaml`, or unrelated personal data. Use redacted or disposable test values.

Maintainers will acknowledge reports when practical, coordinate investigation privately, and credit reporters who request attribution. Response and remediation timelines cannot be guaranteed before the project has a supported release.

## Security Boundary

The native process-control boundary is implemented in Rust but is not yet wired to the development shell. It uses fixed trusted-resource and argument selection, creation-time Windows Job Object supervision of the complete paqet process tree, deny-by-default Tauri capabilities, and secret-safe diagnostics. The webview has no arbitrary shell or executable access.

Profiles and generated paqet configuration are intentionally stored as plaintext under the current user's Windows application-data directories. Anyone or any process able to read those files can read their connection details. This is accepted product behavior, not an encryption-at-rest claim.

The upstream paqet binary is a separately licensed compatibility dependency. Its current pin, provenance limitations, checksums, and upgrade policy are documented in [docs/paqet-compatibility.md](docs/paqet-compatibility.md) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
