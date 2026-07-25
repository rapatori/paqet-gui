use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::{network::NetworkInterface, profiles::Profile};

const CONFIG_FILE_NAME: &str = "config.yaml";
const DEFAULT_SMUX_BUFFER: u32 = 4 * 1024 * 1024;
const DEFAULT_STREAM_BUFFER: u32 = 2 * 1024 * 1024;
const DEFAULT_SMUX_KEEPALIVE: u32 = 2;
const DEFAULT_SMUX_TIMEOUT: u32 = 8;
const MAX_GO_INT: u64 = i64::MAX as u64;
const MAX_SMUX_BUFFER: u32 = i32::MAX as u32;
static RUNTIME_CONFIG_WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    #[default]
    Info,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KcpMode {
    Normal,
    #[default]
    Fast,
    Fast2,
    Fast3,
    Manual,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KcpBlock {
    #[default]
    Aes,
    #[serde(rename = "aes-128")]
    Aes128,
    #[serde(rename = "aes-128-gcm")]
    Aes128Gcm,
    #[serde(rename = "aes-192")]
    Aes192,
    Salsa20,
    Blowfish,
    Twofish,
    Cast5,
    #[serde(rename = "3des")]
    TripleDes,
    Tea,
    Xtea,
    Xor,
    Sm4,
    None,
    Null,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManualKcpSettings {
    pub no_delay: Option<u8>,
    pub interval: Option<u16>,
    pub resend: Option<u8>,
    pub no_congestion: Option<u8>,
    pub write_delay: Option<bool>,
    pub ack_no_delay: Option<bool>,
}

impl ManualKcpSettings {
    fn is_empty(&self) -> bool {
        self.no_delay.is_none()
            && self.interval.is_none()
            && self.resend.is_none()
            && self.no_congestion.is_none()
            && self.write_delay.is_none()
            && self.ack_no_delay.is_none()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdvancedSettings {
    pub log_level: Option<LogLevel>,
    pub pcap_socket_buffer: Option<u32>,
    pub local_tcp_flags: Option<Vec<String>>,
    pub remote_tcp_flags: Option<Vec<String>>,
    pub connection_count: Option<u16>,
    pub tcp_buffer: Option<u64>,
    pub udp_buffer: Option<u64>,
    pub kcp_mode: Option<KcpMode>,
    pub manual_kcp: ManualKcpSettings,
    pub kcp_mtu: Option<u16>,
    pub kcp_receive_window: Option<u16>,
    pub kcp_send_window: Option<u16>,
    pub kcp_block: Option<KcpBlock>,
    pub smux_buffer: Option<u32>,
    pub stream_buffer: Option<u32>,
    pub smux_keepalive: Option<u32>,
    pub smux_timeout: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigField {
    InterfaceName,
    InterfaceGuid,
    LocalAddress,
    GatewayMac,
    ServerAddress,
    EncryptionKey,
    PcapSocketBuffer,
    LocalTcpFlags,
    RemoteTcpFlags,
    ConnectionCount,
    TcpBuffer,
    UdpBuffer,
    KcpMode,
    KcpNoDelay,
    KcpInterval,
    KcpResend,
    KcpNoCongestion,
    KcpMtu,
    KcpReceiveWindow,
    KcpSendWindow,
    SmuxBuffer,
    StreamBuffer,
    SmuxKeepalive,
    SmuxTimeout,
}

impl fmt::Display for ConfigField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InterfaceName => "network interface",
            Self::InterfaceGuid => "Npcap interface GUID",
            Self::LocalAddress => "local IPv4 address",
            Self::GatewayMac => "gateway MAC address",
            Self::ServerAddress => "server address",
            Self::EncryptionKey => "encryption key",
            Self::PcapSocketBuffer => "PCAP socket buffer",
            Self::LocalTcpFlags => "local TCP flags",
            Self::RemoteTcpFlags => "remote TCP flags",
            Self::ConnectionCount => "connection count",
            Self::TcpBuffer => "TCP buffer",
            Self::UdpBuffer => "UDP buffer",
            Self::KcpMode => "KCP mode",
            Self::KcpNoDelay => "KCP nodelay",
            Self::KcpInterval => "KCP interval",
            Self::KcpResend => "KCP resend",
            Self::KcpNoCongestion => "KCP nocongestion",
            Self::KcpMtu => "KCP MTU",
            Self::KcpReceiveWindow => "KCP receive window",
            Self::KcpSendWindow => "KCP send window",
            Self::SmuxBuffer => "SMUX buffer",
            Self::StreamBuffer => "stream buffer",
            Self::SmuxKeepalive => "SMUX keepalive",
            Self::SmuxTimeout => "SMUX timeout",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigValidationKind {
    Required,
    InvalidFormat,
    OutOfRange,
    InvalidCombination,
}

impl fmt::Display for ConfigValidationKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Required => "is required",
            Self::InvalidFormat => "has an invalid format",
            Self::OutOfRange => "is outside the supported range",
            Self::InvalidCombination => "conflicts with another setting",
        })
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Validation {
        field: ConfigField,
        kind: ConfigValidationKind,
    },
    Serialization,
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation { field, kind } => write!(formatter, "{field} {kind}"),
            Self::Serialization => formatter.write_str("could not serialize paqet configuration"),
            Self::Io { operation, source } => write!(formatter, "could not {operation}: {source}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub struct GeneratedConfig {
    yaml: String,
}

impl GeneratedConfig {
    fn as_bytes(&self) -> &[u8] {
        self.yaml.as_bytes()
    }

    #[cfg(test)]
    pub fn as_str(&self) -> &str {
        &self.yaml
    }
}

impl fmt::Debug for GeneratedConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeneratedConfig")
            .field("yaml", &"[REDACTED]")
            .field("byte_length", &self.yaml.len())
            .finish()
    }
}

pub fn generate(
    profile: &Profile,
    interface: &NetworkInterface,
    settings: &AdvancedSettings,
) -> Result<GeneratedConfig, ConfigError> {
    validate_required_values(profile, interface)?;
    validate_settings(settings)?;

    let pcap = settings
        .pcap_socket_buffer
        .map(|sockbuf| PcapConfig { sockbuf });
    let tcp = if settings.local_tcp_flags.is_some() || settings.remote_tcp_flags.is_some() {
        Some(TcpConfig {
            local_flag: settings.local_tcp_flags.as_deref(),
            remote_flag: settings.remote_tcp_flags.as_deref(),
        })
    } else {
        None
    };
    let manual = &settings.manual_kcp;
    let manual_enabled = settings.kcp_mode == Some(KcpMode::Manual);
    let config = PaqetConfig {
        role: "client",
        log: LogConfig {
            level: settings.log_level.unwrap_or_default(),
        },
        socks5: [Socks5Config {
            listen: "127.0.0.1:1080",
        }],
        network: NetworkConfig {
            interface: &interface.interface_name,
            guid: &interface.guid,
            ipv4: Ipv4Config {
                addr: format!("{}:0", interface.local_address),
                router_mac: &interface.gateway_mac,
            },
            pcap,
            tcp,
        },
        server: ServerConfig {
            addr: format!("{}:{}", profile.server_host, profile.port),
        },
        transport: TransportConfig {
            protocol: "kcp",
            conn: settings.connection_count,
            tcpbuf: settings.tcp_buffer,
            udpbuf: settings.udp_buffer,
            kcp: KcpConfig {
                mode: settings.kcp_mode,
                nodelay: manual_enabled.then_some(manual.no_delay).flatten(),
                interval: manual_enabled.then_some(manual.interval).flatten(),
                resend: manual_enabled.then_some(manual.resend).flatten(),
                nocongestion: manual_enabled.then_some(manual.no_congestion).flatten(),
                wdelay: manual_enabled.then_some(manual.write_delay).flatten(),
                acknodelay: manual_enabled.then_some(manual.ack_no_delay).flatten(),
                mtu: settings.kcp_mtu,
                rcvwnd: settings.kcp_receive_window,
                sndwnd: settings.kcp_send_window,
                block: settings.kcp_block,
                key: &profile.encryption_key,
                smuxbuf: settings.smux_buffer,
                streambuf: settings.stream_buffer,
                smuxkalive: settings.smux_keepalive,
                smuxktimeout: settings.smux_timeout,
            },
        },
    };

    let yaml = serde_yaml_ng::to_string(&config).map_err(|_| ConfigError::Serialization)?;
    Ok(GeneratedConfig { yaml })
}

fn validate_required_values(
    profile: &Profile,
    interface: &NetworkInterface,
) -> Result<(), ConfigError> {
    if profile.port == 0 || !valid_server_host(&profile.server_host) {
        return Err(validation(
            ConfigField::ServerAddress,
            ConfigValidationKind::InvalidFormat,
        ));
    }
    if profile.encryption_key.is_empty() {
        return Err(validation(
            ConfigField::EncryptionKey,
            ConfigValidationKind::Required,
        ));
    }
    if interface.interface_name.is_empty() {
        return Err(validation(
            ConfigField::InterfaceName,
            ConfigValidationKind::Required,
        ));
    }
    if interface.interface_name.len() > 15 || interface.interface_name.chars().any(char::is_control)
    {
        return Err(validation(
            ConfigField::InterfaceName,
            ConfigValidationKind::InvalidFormat,
        ));
    }
    if !valid_npcap_guid(&interface.guid) {
        return Err(validation(
            ConfigField::InterfaceGuid,
            ConfigValidationKind::InvalidFormat,
        ));
    }
    if !valid_gateway_mac(&interface.gateway_mac) {
        return Err(validation(
            ConfigField::GatewayMac,
            ConfigValidationKind::InvalidFormat,
        ));
    }
    if interface.local_address.is_unspecified()
        || interface.local_address.is_loopback()
        || interface.local_address.is_link_local()
        || interface.local_address.is_multicast()
        || interface.local_address.is_broadcast()
    {
        return Err(validation(
            ConfigField::LocalAddress,
            ConfigValidationKind::InvalidFormat,
        ));
    }
    Ok(())
}

fn validate_settings(settings: &AdvancedSettings) -> Result<(), ConfigError> {
    validate_range(
        settings.pcap_socket_buffer,
        1024..=100 * 1024 * 1024,
        ConfigField::PcapSocketBuffer,
    )?;
    validate_tcp_flags(
        settings.local_tcp_flags.as_deref(),
        ConfigField::LocalTcpFlags,
    )?;
    validate_tcp_flags(
        settings.remote_tcp_flags.as_deref(),
        ConfigField::RemoteTcpFlags,
    )?;
    validate_range(
        settings.connection_count,
        1..=256,
        ConfigField::ConnectionCount,
    )?;
    validate_range(
        settings.tcp_buffer,
        4096..=MAX_GO_INT,
        ConfigField::TcpBuffer,
    )?;
    validate_range(
        settings.udp_buffer,
        2048..=MAX_GO_INT,
        ConfigField::UdpBuffer,
    )?;

    if !settings.manual_kcp.is_empty() && settings.kcp_mode != Some(KcpMode::Manual) {
        return Err(validation(
            ConfigField::KcpMode,
            ConfigValidationKind::InvalidCombination,
        ));
    }
    validate_binary(settings.manual_kcp.no_delay, ConfigField::KcpNoDelay)?;
    validate_range(
        settings.manual_kcp.interval,
        10..=5000,
        ConfigField::KcpInterval,
    )?;
    validate_range(settings.manual_kcp.resend, 0..=2, ConfigField::KcpResend)?;
    validate_binary(
        settings.manual_kcp.no_congestion,
        ConfigField::KcpNoCongestion,
    )?;
    validate_range(settings.kcp_mtu, 50..=1500, ConfigField::KcpMtu)?;
    validate_range(
        settings.kcp_receive_window,
        1..=32768,
        ConfigField::KcpReceiveWindow,
    )?;
    validate_range(
        settings.kcp_send_window,
        1..=32768,
        ConfigField::KcpSendWindow,
    )?;
    validate_range(
        settings.smux_buffer,
        1024..=MAX_SMUX_BUFFER,
        ConfigField::SmuxBuffer,
    )?;
    validate_range(
        settings.stream_buffer,
        1024..=MAX_SMUX_BUFFER,
        ConfigField::StreamBuffer,
    )?;
    validate_range(
        settings.smux_keepalive,
        1..=u32::MAX,
        ConfigField::SmuxKeepalive,
    )?;
    validate_range(
        settings.smux_timeout,
        1..=u32::MAX,
        ConfigField::SmuxTimeout,
    )?;

    let smux_buffer = settings.smux_buffer.unwrap_or(DEFAULT_SMUX_BUFFER);
    let stream_buffer = settings.stream_buffer.unwrap_or(DEFAULT_STREAM_BUFFER);
    if stream_buffer > smux_buffer {
        return Err(validation(
            ConfigField::StreamBuffer,
            ConfigValidationKind::InvalidCombination,
        ));
    }
    let keepalive = settings.smux_keepalive.unwrap_or(DEFAULT_SMUX_KEEPALIVE);
    let timeout = settings.smux_timeout.unwrap_or(DEFAULT_SMUX_TIMEOUT);
    if timeout < keepalive {
        return Err(validation(
            ConfigField::SmuxTimeout,
            ConfigValidationKind::InvalidCombination,
        ));
    }
    Ok(())
}

fn validate_range<T>(
    value: Option<T>,
    range: std::ops::RangeInclusive<T>,
    field: ConfigField,
) -> Result<(), ConfigError>
where
    T: Copy + PartialOrd,
{
    if value.is_some_and(|value| !range.contains(&value)) {
        return Err(validation(field, ConfigValidationKind::OutOfRange));
    }
    Ok(())
}

fn validate_binary(value: Option<u8>, field: ConfigField) -> Result<(), ConfigError> {
    validate_range(value, 0..=1, field)
}

fn validate_tcp_flags(flags: Option<&[String]>, field: ConfigField) -> Result<(), ConfigError> {
    let Some(flags) = flags else {
        return Ok(());
    };
    if flags.is_empty() || flags.len() > 64 {
        return Err(validation(field, ConfigValidationKind::OutOfRange));
    }
    if flags.iter().any(|combination| {
        combination.is_empty() || !combination.bytes().all(|flag| b"FSRPAUECN".contains(&flag))
    }) {
        return Err(validation(field, ConfigValidationKind::InvalidFormat));
    }
    Ok(())
}

fn valid_npcap_guid(guid: &str) -> bool {
    let Some(value) = guid
        .strip_prefix("\\Device\\NPF_{")
        .and_then(|value| value.strip_suffix('}'))
    else {
        return false;
    };
    uuid::Uuid::parse_str(value).is_ok()
}

fn valid_gateway_mac(mac: &str) -> bool {
    let octets = mac
        .split(':')
        .map(|octet| {
            (octet.len() == 2)
                .then(|| u8::from_str_radix(octet, 16).ok())
                .flatten()
        })
        .collect::<Option<Vec<_>>>();
    octets.is_some_and(|octets| {
        octets.len() == 6 && octets.iter().any(|octet| *octet != 0) && octets[0] & 1 == 0
    })
}

fn valid_server_host(host: &str) -> bool {
    if host.is_empty() || host.chars().any(char::is_control) {
        return false;
    }
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(_)) => return true,
        Ok(IpAddr::V6(_)) => return false,
        Err(_) => {}
    }

    let hostname = host.strip_suffix('.').unwrap_or(host);
    !hostname.is_empty()
        && hostname.len() <= 253
        && hostname.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.is_ascii()
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && label
                    .as_bytes()
                    .first()
                    .is_some_and(u8::is_ascii_alphanumeric)
                && label
                    .as_bytes()
                    .last()
                    .is_some_and(u8::is_ascii_alphanumeric)
        })
}

fn validation(field: ConfigField, kind: ConfigValidationKind) -> ConfigError {
    ConfigError::Validation { field, kind }
}

#[derive(Serialize)]
struct PaqetConfig<'a> {
    role: &'static str,
    log: LogConfig,
    socks5: [Socks5Config; 1],
    network: NetworkConfig<'a>,
    server: ServerConfig,
    transport: TransportConfig<'a>,
}

#[derive(Serialize)]
struct LogConfig {
    level: LogLevel,
}

#[derive(Serialize)]
struct Socks5Config {
    listen: &'static str,
}

#[derive(Serialize)]
struct NetworkConfig<'a> {
    interface: &'a str,
    guid: &'a str,
    ipv4: Ipv4Config<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pcap: Option<PcapConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tcp: Option<TcpConfig<'a>>,
}

#[derive(Serialize)]
struct Ipv4Config<'a> {
    addr: String,
    router_mac: &'a str,
}

#[derive(Serialize)]
struct PcapConfig {
    sockbuf: u32,
}

#[derive(Serialize)]
struct TcpConfig<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    local_flag: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote_flag: Option<&'a [String]>,
}

#[derive(Serialize)]
struct ServerConfig {
    addr: String,
}

#[derive(Serialize)]
struct TransportConfig<'a> {
    protocol: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    conn: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tcpbuf: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    udpbuf: Option<u64>,
    kcp: KcpConfig<'a>,
}

#[derive(Serialize)]
struct KcpConfig<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<KcpMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nodelay: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    interval: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resend: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nocongestion: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wdelay: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    acknodelay: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mtu: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rcvwnd: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sndwnd: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    block: Option<KcpBlock>,
    key: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    smuxbuf: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    streambuf: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    smuxkalive: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    smuxktimeout: Option<u32>,
}

#[derive(Debug)]
pub struct RuntimeConfigStore {
    path: PathBuf,
}

impl RuntimeConfigStore {
    pub fn from_app_handle<R: Runtime>(app: &AppHandle<R>) -> Result<Self, ConfigError> {
        let directory = app
            .path()
            .app_local_data_dir()
            .map_err(|source| ConfigError::Io {
                operation: "resolve the runtime configuration directory",
                source: io::Error::other(source),
            })?;
        Ok(Self::new(directory.join(CONFIG_FILE_NAME)))
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self, config: &GeneratedConfig) -> Result<(), ConfigError> {
        let _guard = RUNTIME_CONFIG_WRITE_LOCK
            .lock()
            .map_err(|_| ConfigError::Io {
                operation: "lock runtime configuration storage",
                source: io::Error::other("runtime configuration storage lock is poisoned"),
            })?;
        let parent = self.path.parent().ok_or_else(|| ConfigError::Io {
            operation: "resolve the runtime configuration directory",
            source: io::Error::new(
                io::ErrorKind::InvalidInput,
                "runtime configuration path has no parent",
            ),
        })?;
        fs::create_dir_all(parent).map_err(|source| ConfigError::Io {
            operation: "create the runtime configuration directory",
            source,
        })?;
        self.remove_stale_temp_files(parent);

        let temp_path = self.temp_path();
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .map_err(|source| ConfigError::Io {
                    operation: "create temporary runtime configuration",
                    source,
                })?;
            file.write_all(config.as_bytes())
                .map_err(|source| ConfigError::Io {
                    operation: "write temporary runtime configuration",
                    source,
                })?;
            file.sync_all().map_err(|source| ConfigError::Io {
                operation: "flush temporary runtime configuration",
                source,
            })?;
            drop(file);

            install_or_replace_file(&self.path, &temp_path).map_err(|source| ConfigError::Io {
                operation: "install runtime configuration",
                source,
            })
        })();
        if write_result.is_err() {
            let _ = remove_file_if_exists(&temp_path);
        }
        write_result
    }

    fn temp_path(&self) -> PathBuf {
        let destination_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(CONFIG_FILE_NAME);
        let file_name = format!("{destination_name}.{}.tmp", uuid::Uuid::new_v4());
        self.path.with_file_name(file_name)
    }

    fn remove_stale_temp_files(&self, parent: &Path) {
        let destination_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(CONFIG_FILE_NAME);
        let prefix = format!("{destination_name}.");
        let Ok(entries) = fs::read_dir(parent) else {
            return;
        };
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            let is_stale_temp = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".tmp"));
            if is_stale_temp {
                let _ = remove_file_if_exists(&path);
            }
        }
    }
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn install_or_replace_file(destination: &Path, replacement: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let replacement = replacement
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: Both pointers reference null-terminated UTF-16 buffers alive for the call.
    let result = unsafe {
        MoveFileExW(
            replacement.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn install_or_replace_file(destination: &Path, replacement: &Path) -> io::Result<()> {
    fs::rename(replacement, destination)
}

#[cfg(test)]
mod tests {
    use std::{
        net::Ipv4Addr,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;
    use crate::profiles::{ProfileCollection, ProfileDraft};

    const SECRET: &str = "test-secret-that-must-not-leak";
    static TEST_DIRECTORY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let sequence = TEST_DIRECTORY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "paqet-gui-config-test-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("test directory should be created");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn profiles(key: &str) -> ProfileCollection {
        let mut profiles = ProfileCollection::default();
        profiles
            .create(ProfileDraft {
                name: "Test".to_owned(),
                server_host: "198.51.100.10".to_owned(),
                port: 9999,
                encryption_key: key.to_owned(),
            })
            .unwrap();
        profiles
    }

    fn interface() -> NetworkInterface {
        NetworkInterface {
            friendly_name: "Ethernet".to_owned(),
            interface_name: "Ethernet".to_owned(),
            guid: "\\Device\\NPF_{12345678-1234-1234-1234-123456789ABC}".to_owned(),
            local_address: Ipv4Addr::new(192, 0, 2, 10),
            gateway_address: Ipv4Addr::new(192, 0, 2, 1),
            gateway_mac: "00:11:22:AA:BB:CC".to_owned(),
        }
    }

    #[test]
    fn baseline_omits_all_upstream_native_overrides() {
        let profiles = profiles(SECRET);
        let generated = generate(
            profiles.selected_profile().unwrap(),
            &interface(),
            &AdvancedSettings::default(),
        )
        .unwrap();
        let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(generated.as_str()).unwrap();

        assert_eq!(document["role"], "client");
        assert_eq!(document["log"]["level"], "info");
        assert_eq!(document["network"]["ipv4"]["addr"], "192.0.2.10:0");
        assert_eq!(document["transport"]["protocol"], "kcp");
        assert_eq!(document["transport"]["kcp"]["key"], SECRET);
        assert!(document["network"].get("pcap").is_none());
        assert!(document["network"].get("tcp").is_none());
        assert!(document["transport"].get("conn").is_none());
        assert!(document["transport"]["kcp"].get("mode").is_none());
    }

    #[test]
    fn serializer_quotes_yaml_sensitive_secret_and_null_block() {
        let profiles = profiles("*alias: # not a comment\nsecond line");
        let settings = AdvancedSettings {
            kcp_block: Some(KcpBlock::Null),
            ..AdvancedSettings::default()
        };
        let generated = generate(
            profiles.selected_profile().unwrap(),
            &interface(),
            &settings,
        )
        .unwrap();
        let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(generated.as_str()).unwrap();

        assert_eq!(document["transport"]["kcp"]["block"], "null");
        assert_eq!(
            document["transport"]["kcp"]["key"],
            "*alias: # not a comment\nsecond line"
        );
    }

    #[test]
    fn validates_manual_mode_and_pinned_ranges() {
        let profiles = profiles(SECRET);
        let profile = profiles.selected_profile().unwrap();
        let interface = interface();
        let cases = [
            AdvancedSettings {
                pcap_socket_buffer: Some(1023),
                ..AdvancedSettings::default()
            },
            AdvancedSettings {
                local_tcp_flags: Some(Vec::new()),
                ..AdvancedSettings::default()
            },
            AdvancedSettings {
                tcp_buffer: Some(4095),
                ..AdvancedSettings::default()
            },
            AdvancedSettings {
                manual_kcp: ManualKcpSettings {
                    no_delay: Some(1),
                    ..ManualKcpSettings::default()
                },
                ..AdvancedSettings::default()
            },
            AdvancedSettings {
                kcp_mode: Some(KcpMode::Manual),
                manual_kcp: ManualKcpSettings {
                    interval: Some(9),
                    ..ManualKcpSettings::default()
                },
                ..AdvancedSettings::default()
            },
            AdvancedSettings {
                kcp_mtu: Some(1501),
                ..AdvancedSettings::default()
            },
        ];

        for settings in cases {
            assert!(matches!(
                generate(profile, &interface, &settings),
                Err(ConfigError::Validation { .. })
            ));
        }
    }

    #[test]
    fn validates_effective_smux_relationships() {
        let profiles = profiles(SECRET);
        let profile = profiles.selected_profile().unwrap();
        let interface = interface();
        let stream_too_large = AdvancedSettings {
            stream_buffer: Some(DEFAULT_SMUX_BUFFER + 1),
            ..AdvancedSettings::default()
        };
        let timeout_too_short = AdvancedSettings {
            smux_keepalive: Some(9),
            ..AdvancedSettings::default()
        };

        assert!(matches!(
            generate(profile, &interface, &stream_too_large),
            Err(ConfigError::Validation {
                field: ConfigField::StreamBuffer,
                kind: ConfigValidationKind::InvalidCombination
            })
        ));
        assert!(matches!(
            generate(profile, &interface, &timeout_too_short),
            Err(ConfigError::Validation {
                field: ConfigField::SmuxTimeout,
                kind: ConfigValidationKind::InvalidCombination
            })
        ));
    }

    #[test]
    fn validates_upstream_interface_byte_limit_and_derived_values() {
        let profiles = profiles(SECRET);
        let profile = profiles.selected_profile().unwrap();
        let mut invalid = interface();
        invalid.interface_name = "éééééééé".to_owned();
        assert_eq!(invalid.interface_name.chars().count(), 8);
        assert_eq!(invalid.interface_name.len(), 16);

        assert!(matches!(
            generate(profile, &invalid, &AdvancedSettings::default()),
            Err(ConfigError::Validation {
                field: ConfigField::InterfaceName,
                ..
            })
        ));

        invalid.interface_name = "Ethernet".to_owned();
        invalid.guid = "not-a-guid".to_owned();
        assert!(matches!(
            generate(profile, &invalid, &AdvancedSettings::default()),
            Err(ConfigError::Validation {
                field: ConfigField::InterfaceGuid,
                ..
            })
        ));

        invalid = interface();
        invalid.local_address = Ipv4Addr::UNSPECIFIED;
        assert!(matches!(
            generate(profile, &invalid, &AdvancedSettings::default()),
            Err(ConfigError::Validation {
                field: ConfigField::LocalAddress,
                ..
            })
        ));

        invalid = interface();
        invalid.gateway_mac = "00:00:00:00:00:00".to_owned();
        assert!(matches!(
            generate(profile, &invalid, &AdvancedSettings::default()),
            Err(ConfigError::Validation {
                field: ConfigField::GatewayMac,
                ..
            })
        ));
    }

    #[test]
    fn revalidates_public_profile_values_at_composition() {
        let profiles = profiles(SECRET);
        let mut profile = profiles.selected_profile().unwrap().clone();
        profile.port = 0;
        assert!(matches!(
            generate(&profile, &interface(), &AdvancedSettings::default()),
            Err(ConfigError::Validation {
                field: ConfigField::ServerAddress,
                ..
            })
        ));

        profile.port = 9999;
        profile.server_host = "2001:db8::1".to_owned();
        assert!(matches!(
            generate(&profile, &interface(), &AdvancedSettings::default()),
            Err(ConfigError::Validation {
                field: ConfigField::ServerAddress,
                ..
            })
        ));
    }

    #[test]
    fn generated_debug_and_errors_do_not_disclose_secrets() {
        let profiles = profiles(SECRET);
        let generated = generate(
            profiles.selected_profile().unwrap(),
            &interface(),
            &AdvancedSettings::default(),
        )
        .unwrap();
        assert!(!format!("{generated:?}").contains(SECRET));

        let mut invalid = interface();
        invalid.guid = SECRET.to_owned();
        let error = generate(
            profiles.selected_profile().unwrap(),
            &invalid,
            &AdvancedSettings::default(),
        )
        .unwrap_err();
        assert!(!format!("{error:?} {error}").contains(SECRET));
    }

    #[test]
    fn atomically_replaces_runtime_configuration_without_backup() {
        let directory = TestDirectory::new();
        let store = RuntimeConfigStore::new(directory.0.join(CONFIG_FILE_NAME));
        let first_profiles = profiles("first-key");
        let first = generate(
            first_profiles.selected_profile().unwrap(),
            &interface(),
            &AdvancedSettings::default(),
        )
        .unwrap();
        store.write(&first).unwrap();
        fs::write(
            store.path().with_file_name("config.yaml.stale.tmp"),
            b"obsolete secret material",
        )
        .unwrap();

        let second_profiles = profiles("second-key");
        let second = generate(
            second_profiles.selected_profile().unwrap(),
            &interface(),
            &AdvancedSettings::default(),
        )
        .unwrap();
        store.write(&second).unwrap();

        assert_eq!(fs::read(store.path()).unwrap(), second.as_bytes());
        assert!(!store.path().with_extension("bak").exists());
        let entries = fs::read_dir(&directory.0).unwrap().count();
        assert_eq!(entries, 1);
    }

    #[test]
    fn concurrent_writers_use_independent_temporary_files() {
        use std::{sync::Arc, thread};

        let directory = TestDirectory::new();
        let store = Arc::new(RuntimeConfigStore::new(directory.0.join(CONFIG_FILE_NAME)));
        let mut writers = Vec::new();
        for index in 0..8 {
            let store = Arc::clone(&store);
            writers.push(thread::spawn(move || {
                let profiles = profiles(&format!("key-{index}"));
                let config = generate(
                    profiles.selected_profile().unwrap(),
                    &interface(),
                    &AdvancedSettings::default(),
                )
                .unwrap();
                store.write(&config)
            }));
        }
        for writer in writers {
            writer.join().unwrap().unwrap();
        }

        let yaml = fs::read_to_string(store.path()).unwrap();
        let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yaml).unwrap();
        assert!(
            document["transport"]["kcp"]["key"]
                .as_str()
                .unwrap()
                .starts_with("key-")
        );
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }
}
