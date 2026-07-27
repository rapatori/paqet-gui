use std::fmt;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    config::AdvancedSettings,
    profiles::{ProfileDraft, ProfileError, ProfileField, ProfileId, ValidationKind},
    state::{AppSnapshot, AppState, StateError},
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
    NetworkDiscovery,
    StateUnavailable,
}

impl From<StateError> for IpcError {
    fn from(error: StateError) -> Self {
        match error {
            StateError::Locked => Self::SettingsLocked,
            StateError::InterfaceNotFound => Self::InterfaceNotFound,
            StateError::Profile(error) => Self::from(error),
            StateError::Network(_) => Self::NetworkDiscovery,
            StateError::Unavailable => Self::StateUnavailable,
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
pub fn replace_advanced_settings(
    state: State<'_, ManagedAppState>,
    settings: AdvancedSettings,
) -> Result<AppSnapshot, IpcError> {
    app_state(&state)?
        .replace_advanced_settings(settings)
        .map_err(Into::into)
}

fn app_state<'a>(state: &'a State<'_, ManagedAppState>) -> Result<&'a AppState, IpcError> {
    state.as_ref().map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use std::io;

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
}
