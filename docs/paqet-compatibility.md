# paqet Compatibility Contract

This project supports exactly one upstream client contract at a time. The current contract is `hanselime/paqet` `v1.0.0-alpha.20` for Windows x64.

Upstream identifies this version as alpha software under active development. Its release notes state that its wire protocol is not backward compatible with earlier versions. A compatible remote paqet server is therefore required; this pin is not a promise of compatibility with other paqet versions or forks.

The machine-readable counterpart to this document is [`src-tauri/compat/paqet-v1.0.0-alpha.20.json`](../src-tauri/compat/paqet-v1.0.0-alpha.20.json).

## Release Pin

| Property            | Pinned value                                                                                                                     |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| Upstream repository | <https://github.com/hanselime/paqet>                                                                                             |
| Release             | [`v1.0.0-alpha.20`](https://github.com/hanselime/paqet/releases/tag/v1.0.0-alpha.20), published 2026-06-28                       |
| Source commit       | [`f8ee6c130b6d44664e737419e99f7f677a6cf03a`](https://github.com/hanselime/paqet/commit/f8ee6c130b6d44664e737419e99f7f677a6cf03a) |
| Windows archive     | `paqet-windows-amd64-v1.0.0-alpha.20.zip`, 3,555,676 bytes                                                                       |
| Archive SHA-256     | `2a59fec9d486d0d910f423cd33701adfc9de7ab9642565da7e05639ba9c18780`                                                               |
| Executable          | `paqet_windows_amd64.exe`, 9,775,616 bytes                                                                                       |
| Executable SHA-256  | `49b377270473c223534ac1c2846d15c287863318e6fe6ee3c123f36ab97b441c`                                                               |
| Sidecar input       | `src-tauri/binaries/paqet_windows_amd64-x86_64-pc-windows-msvc.exe`                                                              |
| Rust target         | `x86_64-pc-windows-msvc`                                                                                                         |
| Tauri sidecar stem  | `binaries/paqet_windows_amd64` in the manifest and `src-tauri/tauri.sidecar.conf.json`                                           |
| Launch contract     | `paqet_windows_amd64.exe run -c <absolute per-user local-app-data path>\config.yaml`                                             |
| License             | MIT, preserved in [`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md)                                                          |

The archive hash was calculated independently and matches the digest returned by GitHub's release-asset API. The extracted executable reports the pinned version, tag, full commit, Go `1.26.4`, and `windows/amd64` platform.

The release was produced by a successful [GitHub Actions run](https://github.com/hanselime/paqet/actions/runs/28315140746) at the pinned commit. Provenance remains limited: the lightweight tag and commit are unsigned, the release is mutable, the executable has no Authenticode signature, and upstream publishes no standalone checksum file, artifact attestation, SBOM, dependency notice bundle, or reproducible-build claim. Both full commit and archive digest must remain pinned.

## Generated Baseline

The application emits an explicit baseline rather than depending on every upstream omission behavior:

```yaml
role: client
log:
  level: info
socks5:
  - listen: 127.0.0.1:1080
network:
  interface: <selected-interface-name>
  guid: <selected-npcap-device-guid>
  ipv4:
    addr: <selected-local-ip>:0
    router_mac: <selected-gateway-mac>
server:
  addr: <profile-host>:<profile-port>
transport:
  protocol: kcp
  kcp:
    key: <profile-key>
```

`kcp` is the application-selected protocol default and is always emitted because it is the only supported protocol. Tagged source rejects an omitted protocol rather than assigning one. `info` is the application-selected log default and is always emitted because upstream's native omission default is `none`, which would suppress the lifecycle markers used by the GUI. Advanced logging may select `debug` or `info`; higher thresholds and `none` are intentionally unavailable because the pinned client emits required lifecycle markers at INFO.

The unauthenticated SOCKS5 listener address remains fixed to loopback. Its application-global port defaults to `1080`, accepts `1–65535`, and is persisted independently of server profiles. PaqetGUI does not expose SOCKS authentication, multiple listeners, or non-loopback binding. The client address uses port `0`, allowing paqet to select an ephemeral port. This also permits more than one configured KCP connection; tagged validation rejects `transport.conn > 1` when the client address contains an explicit nonzero port.

## Client Field Inventory

“Upstream default” describes behavior in the pinned source. “Application behavior” describes the V1 GUI contract. Deferred fields remain documented and are not silently passed through.

| YAML path                    | Type                 | Upstream default and validation                                                                                  | Application behavior                                                                                                  |
| ---------------------------- | -------------------- | ---------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `role`                       | string               | Required; exactly `client` or `server`                                                                           | Always emit `client`                                                                                                  |
| `log.level`                  | string               | Omission becomes `none`; one of `none`, `debug`, `info`, `warn`, `error`, `fatal`                                | Emit `info`; Advanced supports `debug` and `info`                                                                     |
| `listen.addr`                | string               | Required only for server role                                                                                    | Server-only; omit                                                                                                     |
| `socks5`                     | sequence             | Client warns when both SOCKS5 and forwarding are absent                                                          | Emit one local SOCKS5 endpoint                                                                                        |
| `socks5[].listen`            | address string       | Required; port `1–65535`                                                                                         | Fixed loopback address plus persisted global port; default `127.0.0.1:1080`                                           |
| `socks5[].username`          | string               | Empty is accepted                                                                                                | Deferred with SOCKS authentication UX                                                                                 |
| `socks5[].password`          | string               | Empty is accepted                                                                                                | Deferred with SOCKS authentication and secret handling                                                                |
| `forward`                    | sequence             | Optional                                                                                                         | Deferred; requires forwarding-rule workflows                                                                          |
| `forward[].listen`           | address string       | Required; port `1–65535`                                                                                         | Deferred with `forward`                                                                                               |
| `forward[].target`           | host/port string     | Required; port `1–65535`                                                                                         | Deferred with `forward`                                                                                               |
| `forward[].protocol`         | string               | Runtime supports `tcp` or `udp`; config validation does not reject other values                                  | Deferred with `forward`                                                                                               |
| `network.interface`          | string               | Required, maximum 15 UTF-8 bytes, and must resolve through `net.InterfaceByName`                                 | Derived from an up Ethernet/Wi-Fi interface with a usable IPv4 default route, local address, and resolved gateway MAC |
| `network.guid`               | string               | Required on Windows                                                                                              | Derived Npcap device name in `\Device\NPF_{GUID}` form                                                                |
| `network.ipv4.addr`          | address string       | At least one address family required; port may be `0`                                                            | Derived local IPv4 with port `0`                                                                                      |
| `network.ipv4.router_mac`    | MAC string           | Required for a configured family and parsed by `net.ParseMAC`                                                    | Derived gateway MAC                                                                                                   |
| `network.ipv6.addr`          | address string       | Optional unless server address is IPv6; dual-stack ports must match                                              | Deferred with IPv6 route/neighbor discovery                                                                           |
| `network.ipv6.router_mac`    | MAC string           | Required when IPv6 is configured                                                                                 | Deferred with IPv6                                                                                                    |
| `network.pcap.sockbuf`       | integer bytes        | Client default `4194304`; range `1024–104857600`; non-power-of-two warns                                         | Optional Advanced override                                                                                            |
| `network.tcp.local_flag`     | string sequence      | Default `["PA"]`; 1–64 entries; characters `F S R P A U E C N`                                                   | Optional Advanced override                                                                                            |
| `network.tcp.remote_flag`    | string sequence      | Default `["PA"]`; 1–64 entries; same characters                                                                  | Optional Advanced override                                                                                            |
| `server.addr`                | address string       | Required; port `1–65535`; address family must have matching network configuration                                | Generated from selected profile; literal IPv6 is rejected because V1 emits only IPv4 network configuration            |
| `transport.protocol`         | string               | Required; only `kcp` accepted                                                                                    | Always emit app-selected default `kcp`                                                                                |
| `transport.conn`             | integer              | Default `1`; range `1–256`; only `1` allowed with explicit client port                                           | Optional Advanced override                                                                                            |
| `transport.tcpbuf`           | integer bytes        | `0` becomes `8192`; values below `4096` are clamped to `4096`; no upper bound in tagged validation               | Optional Advanced override, `4096–9223372036854775807`                                                                |
| `transport.udpbuf`           | integer bytes        | `0` becomes `4096`; values below `2048` are clamped to `2048`; no upper bound in tagged validation               | Optional Advanced override, `2048–9223372036854775807`                                                                |
| `transport.kcp`              | mapping              | Required for KCP; tagged code dereferences it during defaulting and validation                                   | Always emit mapping                                                                                                   |
| `transport.kcp.mode`         | string               | Default `fast`; one of `normal`, `fast`, `fast2`, `fast3`, `manual`                                              | Optional Advanced override                                                                                            |
| `transport.kcp.nodelay`      | integer              | Used only by `manual`; example documents `0` or `1`; source performs no range validation                         | Conditional manual-mode Advanced override                                                                             |
| `transport.kcp.interval`     | integer milliseconds | Used only by `manual`; example documents `10–5000`; KCP runtime clamps to that range                             | Conditional manual-mode Advanced override using documented range                                                      |
| `transport.kcp.resend`       | integer              | Used only by `manual`; example documents `0–2`; source performs no range validation                              | Conditional manual-mode Advanced override using documented range                                                      |
| `transport.kcp.nocongestion` | integer              | Used only by `manual`; example documents `0` or `1`; source performs no range validation                         | Conditional manual-mode Advanced override                                                                             |
| `transport.kcp.wdelay`       | boolean              | Used only by `manual`; Go zero value `false`                                                                     | Conditional manual-mode Advanced override                                                                             |
| `transport.kcp.acknodelay`   | boolean              | Used only by `manual`; Go zero value `false`, while example recommends `true`                                    | Conditional manual-mode Advanced override                                                                             |
| `transport.kcp.mtu`          | integer bytes        | Default `1350`; range `50–1500`                                                                                  | Optional Advanced override                                                                                            |
| `transport.kcp.rcvwnd`       | integer              | Client default `512`; range `1–32768`                                                                            | Optional Advanced override                                                                                            |
| `transport.kcp.sndwnd`       | integer              | Client default `512`; range `1–32768`                                                                            | Optional Advanced override                                                                                            |
| `transport.kcp.dshard`       | integer              | Go zero value `0`; used by KCP, but tagged example says FEC is currently disabled and defaults are commented out | Deferred pending explicit FEC support decision                                                                        |
| `transport.kcp.pshard`       | integer              | Go zero value `0`; same tagged-source ambiguity as `dshard`                                                      | Deferred pending explicit FEC support decision                                                                        |
| `transport.kcp.block`        | string               | Default `aes`; accepted values listed below                                                                      | Optional Advanced override                                                                                            |
| `transport.kcp.key`          | string               | Required unless block is `none` or `null`                                                                        | Generated from selected profile; never logged                                                                         |
| `transport.kcp.smuxbuf`      | integer bytes        | Default `4194304`; tagged minimum `1024`; SMUX runtime maximum signed 32-bit                                     | Optional Advanced override with runtime-compatible validation                                                         |
| `transport.kcp.streambuf`    | integer bytes        | Default `2097152`; tagged minimum `1024`; must not exceed `smuxbuf` or signed 32-bit maximum                     | Optional Advanced override with runtime-compatible validation                                                         |
| `transport.kcp.smuxkalive`   | integer seconds      | Default `2`; converted to duration; SMUX runtime requires a positive value                                       | Optional Advanced override with positive application validation                                                       |
| `transport.kcp.smuxktimeout` | integer seconds      | Default `8`; converted to duration; SMUX runtime requires a value at least `smuxkalive`                          | Optional Advanced override with relationship validation                                                               |

Accepted KCP block values are `aes`, `aes-128`, `aes-128-gcm`, `aes-192`, `salsa20`, `blowfish`, `twofish`, `cast5`, `3des`, `tea`, `xtea`, `xor`, `sm4`, `none`, and `null`. PaqetGUI exposes `none` and `null` only after explicit confirmation that they disable encryption and authentication and must exactly match the server. They remain wire-distinct, and PaqetGUI profiles still require an encryption key even when the selected upstream mode does not use it.

For fields where `0` triggers an upstream default, omission is the canonical “no override” representation. The application does not preserve arbitrary unknown YAML fields or expose raw YAML editing.

Generated configuration is serialized from a closed Rust model. The application validates both the tagged paqet rules and the runtime buffer/keepalive relationships above before writing `config.yaml`; disabled overrides and their otherwise-empty parent mappings are omitted. Manual KCP values are emitted only with `mode: manual`. TCP flag overrides require 1–64 nonempty combinations containing only the tagged flag characters.

The generated file is replaced atomically under the current user's local application data directory. It contains the profile key in plaintext because paqet requires it; diagnostics do not include generated YAML or secret values.

## Output Contract

The tagged paqet logger writes configured log records to stdout in this shape:

```text
YYYY-MM-DD HH:MM:SS.mmm [LEVEL] message
```

The standard Go logger writes configuration-load failures to stderr with a different timestamp shape. The signal-shutdown message is raw stdout without a logger prefix. Parsers must classify stable message fragments after tolerating variable timestamps and values.

| Evidence                                     | Stream    | Meaning                                                                                               |
| -------------------------------------------- | --------- | ----------------------------------------------------------------------------------------------------- |
| `Client started:`                            | stdout    | Positive Connected marker                                                                             |
| `connection lost, retrying....`              | stdout    | Connection-loss/degraded marker; process remains alive and retries when a proxied stream is requested |
| `Failed to load configuration:`              | stderr    | Fatal startup failure followed by exit code `1`                                                       |
| `[FATAL] Client encountered an error:`       | stdout    | Fatal runtime initialization failure followed by exit code `1`                                        |
| `Shutdown signal received, shutting down...` | stdout    | Signal observed; remain Disconnecting until process exit                                              |
| Process exit                                 | lifecycle | Authoritative Disconnected transition; nonzero or unexpected exit is failure evidence                 |

`Starting client...`, SOCKS listener messages, and ordinary INFO/DEBUG/ERROR traffic are display records unless a later parser contract explicitly classifies them. In particular, a generic `[ERROR]` line may describe one proxied connection rather than the managed process as a whole and must not automatically force a lifecycle transition.

Representative records live under [`src-tauri/tests/fixtures/paqet-v1.0.0-alpha.20`](../src-tauri/tests/fixtures/paqet-v1.0.0-alpha.20). They preserve source-defined shapes, not captured user data.

## Source Evidence

The contract was reconciled against these files at the pinned commit:

- [CLI configuration flag and fatal load behavior](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/cmd/run/run.go)
- [Client startup and fatal messages](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/cmd/run/client.go)
- [Top-level schema/defaulting/validation](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/internal/conf/conf.go)
- [Windows network requirements](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/internal/conf/network.go)
- [Transport defaults and validation](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/internal/conf/transport.go)
- [KCP schema/defaults/validation](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/internal/conf/kcp.go)
- [KCP mode application](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/internal/tnet/kcp/kcp.go)
- [Positive client marker](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/internal/client/client.go)
- [Connection-loss marker](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/internal/client/dial.go)
- [Logger format and destination](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/internal/flog/flog.go)
- [Complete upstream client example](https://github.com/hanselime/paqet/blob/f8ee6c130b6d44664e737419e99f7f677a6cf03a/example/client.yaml.example)

## Upgrade Checklist

An upstream version change is a compatibility migration, not a dependency-only update.

1. Select an explicit release and record its stability and protocol compatibility notes.
2. Resolve the tag to a full commit. Record whether the tag and commit are signed and whether the release is immutable.
3. Download the Windows amd64 archive from its release URL. Verify archive name, byte length, SHA-256, contents, and executable name.
4. Hash the extracted executable, inspect Authenticode status, and run only `version` and `run --help` to verify embedded identity and CLI shape.
5. Reconcile the release build workflow/run, published checksums, signatures, attestations, SBOM, dependency notices, and test evidence. Do not weaken existing provenance silently.
6. Diff every `internal/conf` YAML tag, default, validation rule, and runtime use. Update the complete field inventory and application override policy.
7. Confirm `run -c`, role handling, SOCKS startup ordering, logger destinations/format, exit behavior, and all lifecycle/error fragments against tagged source and controlled execution.
8. Replace fixtures and update parser tests before changing classifier constants.
9. Generate representative baseline and all-override YAML, then load them with the candidate executable on Windows. Exercise invalid configuration without exposing a real key.
10. Run Windows integration tests for Npcap interface resolution, startup, compatible-server connection, loss/retry, disconnect, unexpected exit, and descendant cleanup.
11. Verify protocol compatibility with the supported server version and state any required coordinated server upgrade publicly.
12. Update the manifest, compatibility document, third-party notices, sidecar input, Tauri external-binary configuration, installer contents, and release checksums as one reviewed change.
