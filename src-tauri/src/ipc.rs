use std::fmt;

use serde::{Deserialize, Serialize};
use tauri::{State, Window, ipc::Channel};

use crate::{
    config::{AdvancedSettings, ConfigError, ConfigField, ConfigValidationKind},
    profiles::{ProfileDraft, ProfileError, ProfileField, ProfileId, ValidationKind},
    settings::{
        ApplicationSettingsError, ApplicationSettingsField, ApplicationSettingsValidationKind,
    },
    state::{AppSnapshot, AppState, RuntimeEvent, StateError, WindowCloseRequest},
};

pub type ManagedAppState = Result<AppState, IpcError>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileDraftRequest {
    pub name: String,
    pub server_host: String,
    pub port: u16,
    pub encryption_key: String,
}

impl fmt::Debug for ProfileDraftRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProfileDraftRequest")
            .field("name", &self.name)
            .field("server_host", &self.server_host)
            .field("port", &self.port)
            .field("encryption_key", &"[REDACTED]")
            .finish()
    }
}

impl From<ProfileDraftRequest> for ProfileDraft {
    fn from(request: ProfileDraftRequest) -> Self {
        Self {
            name: request.name,
            server_host: request.server_host,
            port: request.port,
            encryption_key: request.encryption_key,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProfileFieldName {
    Name,
    ServerHost,
    Port,
    EncryptionKey,
}

impl From<ProfileField> for ProfileFieldName {
    fn from(field: ProfileField) -> Self {
        match field {
            ProfileField::Name => Self::Name,
            ProfileField::ServerHost => Self::ServerHost,
            ProfileField::Port => Self::Port,
            ProfileField::EncryptionKey => Self::EncryptionKey,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ValidationIssue {
    Required,
    InvalidFormat,
    OutOfRange,
    ContainsControlCharacters,
    InvalidCombination,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigFieldName {
    InterfaceName,
    InterfaceGuid,
    LocalAddress,
    GatewayMac,
    ServerAddress,
    SocksPort,
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

impl From<ConfigField> for ConfigFieldName {
    fn from(field: ConfigField) -> Self {
        match field {
            ConfigField::InterfaceName => Self::InterfaceName,
            ConfigField::InterfaceGuid => Self::InterfaceGuid,
            ConfigField::LocalAddress => Self::LocalAddress,
            ConfigField::GatewayMac => Self::GatewayMac,
            ConfigField::ServerAddress => Self::ServerAddress,
            ConfigField::SocksPort => Self::SocksPort,
            ConfigField::EncryptionKey => Self::EncryptionKey,
            ConfigField::PcapSocketBuffer => Self::PcapSocketBuffer,
            ConfigField::LocalTcpFlags => Self::LocalTcpFlags,
            ConfigField::RemoteTcpFlags => Self::RemoteTcpFlags,
            ConfigField::ConnectionCount => Self::ConnectionCount,
            ConfigField::TcpBuffer => Self::TcpBuffer,
            ConfigField::UdpBuffer => Self::UdpBuffer,
            ConfigField::KcpMode => Self::KcpMode,
            ConfigField::KcpNoDelay => Self::KcpNoDelay,
            ConfigField::KcpInterval => Self::KcpInterval,
            ConfigField::KcpResend => Self::KcpResend,
            ConfigField::KcpNoCongestion => Self::KcpNoCongestion,
            ConfigField::KcpMtu => Self::KcpMtu,
            ConfigField::KcpReceiveWindow => Self::KcpReceiveWindow,
            ConfigField::KcpSendWindow => Self::KcpSendWindow,
            ConfigField::SmuxBuffer => Self::SmuxBuffer,
            ConfigField::StreamBuffer => Self::StreamBuffer,
            ConfigField::SmuxKeepalive => Self::SmuxKeepalive,
            ConfigField::SmuxTimeout => Self::SmuxTimeout,
        }
    }
}

impl From<ValidationKind> for ValidationIssue {
    fn from(kind: ValidationKind) -> Self {
        match kind {
            ValidationKind::Required => Self::Required,
            ValidationKind::InvalidFormat => Self::InvalidFormat,
            ValidationKind::OutOfRange => Self::OutOfRange,
            ValidationKind::ContainsControlCharacters => Self::ContainsControlCharacters,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum IpcError {
    SettingsLocked,
    InterfaceNotFound,
    ProfileNotSelected,
    InterfaceNotSelected,
    CommandConflict,
    ProfileValidation {
        field: ProfileFieldName,
        issue: ValidationIssue,
    },
    ProfileDuplicateName,
    ProfileNotFound,
    ProfileDataUnsupported {
        version: u32,
    },
    ProfileDataInvalid,
    ProfileStorage,
    SettingsValidation {
        field: SettingsFieldName,
        issue: ValidationIssue,
    },
    SettingsDataUnsupported {
        version: u32,
    },
    SettingsDataInvalid,
    SettingsStorage,
    NetworkDiscovery,
    ConfigValidation {
        field: ConfigFieldName,
        issue: ValidationIssue,
    },
    ConfigGeneration,
    ConfigStorage,
    ProcessLaunch,
    RuntimeSubscription,
    StateUnavailable,
}

impl From<StateError> for IpcError {
    fn from(error: StateError) -> Self {
        match error {
            StateError::Locked => Self::SettingsLocked,
            StateError::InterfaceNotFound => Self::InterfaceNotFound,
            StateError::ProfileNotSelected => Self::ProfileNotSelected,
            StateError::InterfaceNotSelected => Self::InterfaceNotSelected,
            StateError::CommandConflict => Self::CommandConflict,
            StateError::Profile(error) => Self::from(error),
            StateError::Network(_) => Self::NetworkDiscovery,
            StateError::Config(error) => Self::from(error),
            StateError::Settings(error) => Self::from(error),
            StateError::Process(_) => Self::ProcessLaunch,
            StateError::Subscription => Self::RuntimeSubscription,
            StateError::Unavailable => Self::StateUnavailable,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SettingsFieldName {
    SocksPort,
}

impl From<ApplicationSettingsError> for IpcError {
    fn from(error: ApplicationSettingsError) -> Self {
        match error {
            ApplicationSettingsError::Validation { field, kind } => Self::SettingsValidation {
                field: match field {
                    ApplicationSettingsField::SocksPort => SettingsFieldName::SocksPort,
                },
                issue: match kind {
                    ApplicationSettingsValidationKind::OutOfRange => ValidationIssue::OutOfRange,
                },
            },
            ApplicationSettingsError::UnsupportedSchemaVersion(version) => {
                Self::SettingsDataUnsupported { version }
            }
            ApplicationSettingsError::CorruptData => Self::SettingsDataInvalid,
            ApplicationSettingsError::Io { .. } => Self::SettingsStorage,
        }
    }
}

impl From<ConfigError> for IpcError {
    fn from(error: ConfigError) -> Self {
        match error {
            ConfigError::Validation { field, kind } => Self::ConfigValidation {
                field: field.into(),
                issue: match kind {
                    ConfigValidationKind::Required => ValidationIssue::Required,
                    ConfigValidationKind::InvalidFormat => ValidationIssue::InvalidFormat,
                    ConfigValidationKind::OutOfRange => ValidationIssue::OutOfRange,
                    ConfigValidationKind::InvalidCombination => ValidationIssue::InvalidCombination,
                },
            },
            ConfigError::Serialization => Self::ConfigGeneration,
            ConfigError::Io { .. } => Self::ConfigStorage,
        }
    }
}

impl From<ProfileError> for IpcError {
    fn from(error: ProfileError) -> Self {
        match error {
            ProfileError::Validation { field, kind } => Self::ProfileValidation {
                field: field.into(),
                issue: kind.into(),
            },
            ProfileError::DuplicateName => Self::ProfileDuplicateName,
            ProfileError::NotFound => Self::ProfileNotFound,
            ProfileError::UnsupportedSchemaVersion(version) => {
                Self::ProfileDataUnsupported { version }
            }
            ProfileError::DuplicateId
            | ProfileError::SelectionNotFound
            | ProfileError::CorruptData => Self::ProfileDataInvalid,
            ProfileError::Io { .. } => Self::ProfileStorage,
        }
    }
}

#[tauri::command]
pub fn get_app_snapshot(state: State<'_, ManagedAppState>) -> Result<AppSnapshot, IpcError> {
    app_state(&state)?.snapshot().map_err(Into::into)
}

#[tauri::command]
pub fn create_profile(
    state: State<'_, ManagedAppState>,
    draft: ProfileDraftRequest,
) -> Result<AppSnapshot, IpcError> {
    app_state(&state)?
        .create_profile(draft.into())
        .map_err(Into::into)
}

#[tauri::command]
pub fn update_profile(
    state: State<'_, ManagedAppState>,
    id: ProfileId,
    draft: ProfileDraftRequest,
) -> Result<AppSnapshot, IpcError> {
    app_state(&state)?
        .update_profile(id, draft.into())
        .map_err(Into::into)
}

#[tauri::command]
pub fn delete_profile(
    state: State<'_, ManagedAppState>,
    id: ProfileId,
) -> Result<AppSnapshot, IpcError> {
    app_state(&state)?.delete_profile(id).map_err(Into::into)
}

#[tauri::command]
pub fn select_profile(
    state: State<'_, ManagedAppState>,
    id: ProfileId,
) -> Result<AppSnapshot, IpcError> {
    app_state(&state)?.select_profile(id).map_err(Into::into)
}

#[tauri::command]
pub fn refresh_interfaces(state: State<'_, ManagedAppState>) -> Result<AppSnapshot, IpcError> {
    app_state(&state)?.refresh_interfaces().map_err(Into::into)
}

#[tauri::command]
pub fn select_interface(
    state: State<'_, ManagedAppState>,
    guid: String,
) -> Result<AppSnapshot, IpcError> {
    app_state(&state)?
        .select_interface(&guid)
        .map_err(Into::into)
}

#[tauri::command]
pub fn set_socks_port(
    state: State<'_, ManagedAppState>,
    port: u32,
) -> Result<AppSnapshot, IpcError> {
    app_state(&state)?.set_socks_port(port).map_err(Into::into)
}

#[tauri::command]
pub fn replace_advanced_settings(
    state: State<'_, ManagedAppState>,
    settings: AdvancedSettings,
) -> Result<AppSnapshot, IpcError> {
    app_state(&state)?
        .replace_advanced_settings(settings)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn connect(state: State<'_, ManagedAppState>) -> Result<AppSnapshot, IpcError> {
    let state = app_state(&state)?.clone();
    tauri::async_runtime::spawn_blocking(move || state.connect())
        .await
        .map_err(|_| IpcError::StateUnavailable)?
        .map_err(Into::into)
}

#[tauri::command]
pub async fn disconnect(state: State<'_, ManagedAppState>) -> Result<AppSnapshot, IpcError> {
    let state = app_state(&state)?.clone();
    tauri::async_runtime::spawn_blocking(move || state.disconnect())
        .await
        .map_err(|_| IpcError::StateUnavailable)?
        .map_err(Into::into)
}

#[tauri::command]
pub fn subscribe_runtime_events(
    state: State<'_, ManagedAppState>,
    on_event: Channel<RuntimeEvent>,
) -> Result<(), IpcError> {
    app_state(&state)?
        .subscribe_runtime_events(on_event)
        .map_err(Into::into)
}

#[tauri::command]
pub fn cancel_window_close(
    state: State<'_, ManagedAppState>,
    request_id: String,
) -> Result<(), IpcError> {
    app_state(&state)?
        .cancel_window_close(parse_request_id(&request_id)?)
        .map_err(Into::into)
}

#[tauri::command]
pub fn subscribe_window_close_requests(
    state: State<'_, ManagedAppState>,
    on_request: Channel<WindowCloseRequest>,
) -> Result<(), IpcError> {
    app_state(&state)?
        .subscribe_window_close_requests(on_request)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn confirm_window_close(
    state: State<'_, ManagedAppState>,
    window: Window,
    request_id: String,
) -> Result<(), IpcError> {
    if window.label() != "main" {
        return Err(IpcError::CommandConflict);
    }
    let request_id = parse_request_id(&request_id)?;
    let state = app_state(&state)?.clone();
    tauri::async_runtime::spawn_blocking(move || state.confirm_window_close(request_id))
        .await
        .map_err(|_| IpcError::StateUnavailable)??;
    window.destroy().map_err(|_| IpcError::StateUnavailable)
}

fn parse_request_id(request_id: &str) -> Result<u64, IpcError> {
    request_id.parse().map_err(|_| IpcError::CommandConflict)
}

fn app_state<'a>(state: &'a State<'_, ManagedAppState>) -> Result<&'a AppState, IpcError> {
    state.as_ref().map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use std::{io, path::PathBuf};

    use super::*;

    #[test]
    fn profile_validation_errors_preserve_stable_field_details() {
        let error = IpcError::from(StateError::Profile(ProfileError::Validation {
            field: ProfileField::ServerHost,
            kind: ValidationKind::InvalidFormat,
        }));

        assert_eq!(
            error,
            IpcError::ProfileValidation {
                field: ProfileFieldName::ServerHost,
                issue: ValidationIssue::InvalidFormat,
            }
        );
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "profileValidation",
                "field": "serverHost",
                "issue": "invalidFormat"
            })
        );
    }

    #[test]
    fn settings_validation_errors_preserve_stable_field_details() {
        let error = IpcError::from(StateError::Settings(ApplicationSettingsError::Validation {
            field: ApplicationSettingsField::SocksPort,
            kind: ApplicationSettingsValidationKind::OutOfRange,
        }));

        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "settingsValidation",
                "field": "socksPort",
                "issue": "outOfRange"
            })
        );
    }

    #[test]
    fn storage_errors_do_not_expose_paths_or_operating_system_details() {
        let error = StateError::Profile(ProfileError::Io {
            operation: "write profile data containing secret-value",
            source: io::Error::new(
                io::ErrorKind::PermissionDenied,
                r"secret-value at C:\Users\Example\profiles.json",
            ),
        });

        let json = serde_json::to_string(&IpcError::from(error)).unwrap();

        assert_eq!(json, r#"{"kind":"profileStorage"}"#);
        assert!(!json.contains("secret-value"));
        assert!(!json.contains("Users"));
    }

    #[test]
    fn profile_draft_debug_output_redacts_the_encryption_key() {
        let request = ProfileDraftRequest {
            name: "Primary".to_owned(),
            server_host: "192.0.2.10".to_owned(),
            port: 9999,
            encryption_key: "secret-value".to_owned(),
        };

        let debug = format!("{request:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("secret-value"));
    }

    #[test]
    fn initialization_errors_remain_structured_and_cloneable() {
        let state: ManagedAppState = Err(IpcError::NetworkDiscovery);

        assert_eq!(
            state.as_ref().unwrap_err().clone(),
            IpcError::NetworkDiscovery
        );
    }

    #[test]
    fn runtime_storage_and_process_errors_are_secret_safe_categories() {
        let config = IpcError::from(StateError::Config(ConfigError::Io {
            operation: "write secret-value configuration",
            source: io::Error::new(
                io::ErrorKind::PermissionDenied,
                r"secret-value at C:\Users\Example\config.yaml",
            ),
        }));
        let process = IpcError::from(StateError::Process(
            crate::process::ProcessError::ExecutableIsNotFile(PathBuf::from(
                r"C:\secret-value\paqet.exe",
            )),
        ));

        assert_eq!(
            serde_json::to_string(&config).unwrap(),
            r#"{"kind":"configStorage"}"#
        );
        assert_eq!(
            serde_json::to_string(&process).unwrap(),
            r#"{"kind":"processLaunch"}"#
        );
    }

    #[test]
    fn close_request_ids_require_decimal_u64_strings() {
        assert_eq!(parse_request_id("18446744073709551615"), Ok(u64::MAX));
        assert_eq!(parse_request_id("9"), Ok(9));
        assert_eq!(parse_request_id("9.0"), Err(IpcError::CommandConflict));
        assert_eq!(parse_request_id("-1"), Err(IpcError::CommandConflict));
        assert_eq!(parse_request_id(""), Err(IpcError::CommandConflict));
    }
}
