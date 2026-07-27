use std::{fmt, sync::Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

use crate::{
    config::AdvancedSettings,
    network::{NetworkError, NetworkInterface, discover_interfaces},
    process::{FailureReason, LifecycleState, LifecycleStatus, ProcessPresence},
    profiles::{Profile, ProfileCollection, ProfileDraft, ProfileError, ProfileId, ProfileStore},
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileSummary {
    pub id: ProfileId,
    pub name: String,
    pub server_host: String,
    pub port: u16,
}

impl From<&Profile> for ProfileSummary {
    fn from(profile: &Profile) -> Self {
        Self {
            id: profile.id,
            name: profile.name.clone(),
            server_host: profile.server_host.clone(),
            port: profile.port,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifecycleSnapshot {
    pub status: LifecycleStatus,
    pub process: ProcessPresence,
    pub failure: Option<FailureReason>,
    pub settings_editable: bool,
}

impl From<LifecycleState> for LifecycleSnapshot {
    fn from(lifecycle: LifecycleState) -> Self {
        Self {
            status: lifecycle.status(),
            process: lifecycle.process(),
            failure: lifecycle.failure(),
            settings_editable: lifecycle.settings_editable(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppSnapshot {
    pub revision: u64,
    pub profiles: Vec<ProfileSummary>,
    pub selected_profile: Option<Profile>,
    pub interfaces: Vec<NetworkInterface>,
    pub selected_interface_guid: Option<String>,
    pub advanced_settings: AdvancedSettings,
    pub lifecycle: LifecycleSnapshot,
}

#[derive(Debug)]
pub enum StateError {
    Locked,
    InterfaceNotFound,
    Profile(ProfileError),
    Network(NetworkError),
    Unavailable,
}

impl fmt::Display for StateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked => formatter.write_str("settings cannot be changed while paqet is active"),
            Self::InterfaceNotFound => formatter.write_str("network interface was not found"),
            Self::Profile(error) => error.fmt(formatter),
            Self::Network(error) => error.fmt(formatter),
            Self::Unavailable => formatter.write_str("application state is unavailable"),
        }
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Profile(error) => Some(error),
            Self::Network(error) => Some(error),
            Self::Locked | Self::InterfaceNotFound | Self::Unavailable => None,
        }
    }
}

impl From<ProfileError> for StateError {
    fn from(error: ProfileError) -> Self {
        Self::Profile(error)
    }
}

impl From<NetworkError> for StateError {
    fn from(error: NetworkError) -> Self {
        Self::Network(error)
    }
}

#[derive(Debug)]
struct StateData {
    revision: u64,
    profiles: ProfileCollection,
    profile_store: ProfileStore,
    interfaces: Vec<NetworkInterface>,
    selected_interface_guid: Option<String>,
    advanced_settings: AdvancedSettings,
    lifecycle: LifecycleState,
}

impl StateData {
    fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            revision: self.revision,
            profiles: self
                .profiles
                .profiles()
                .iter()
                .map(ProfileSummary::from)
                .collect(),
            selected_profile: self.profiles.selected_profile().cloned(),
            interfaces: self.interfaces.clone(),
            selected_interface_guid: self.selected_interface_guid.clone(),
            advanced_settings: self.advanced_settings.clone(),
            lifecycle: self.lifecycle.into(),
        }
    }

    fn ensure_editable(&self) -> Result<(), StateError> {
        if self.lifecycle.settings_editable() {
            Ok(())
        } else {
            Err(StateError::Locked)
        }
    }

    fn commit_profiles(
        &mut self,
        mutation: impl FnOnce(&mut ProfileCollection) -> Result<(), ProfileError>,
    ) -> Result<AppSnapshot, StateError> {
        self.ensure_editable()?;
        let mut candidate = self.profiles.clone();
        mutation(&mut candidate)?;
        self.profile_store.save(&candidate)?;
        self.profiles = candidate;
        self.advance_revision();
        Ok(self.snapshot())
    }

    fn replace_interfaces(
        &mut self,
        interfaces: Vec<NetworkInterface>,
    ) -> Result<AppSnapshot, StateError> {
        self.ensure_editable()?;
        let selected_still_exists = self
            .selected_interface_guid
            .as_ref()
            .is_some_and(|guid| interfaces.iter().any(|interface| interface.guid == *guid));
        let selected_interface_guid = if selected_still_exists {
            self.selected_interface_guid.clone()
        } else {
            interfaces.first().map(|interface| interface.guid.clone())
        };

        self.interfaces = interfaces;
        self.selected_interface_guid = selected_interface_guid;
        self.advance_revision();
        Ok(self.snapshot())
    }

    fn advance_revision(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("application state revision exhausted");
    }
}

#[derive(Debug)]
pub struct AppState {
    inner: Mutex<StateData>,
}

impl AppState {
    pub fn from_app_handle<R: Runtime>(app: &AppHandle<R>) -> Result<Self, StateError> {
        let profile_store = ProfileStore::from_app_handle(app)?;
        let profiles = profile_store.load()?;
        let interfaces = discover_interfaces()?;
        Ok(Self::from_parts(profile_store, profiles, interfaces))
    }

    #[cfg(test)]
    fn load_with_interfaces(
        profile_store: ProfileStore,
        interfaces: Vec<NetworkInterface>,
    ) -> Result<Self, StateError> {
        let profiles = profile_store.load()?;
        Ok(Self::from_parts(profile_store, profiles, interfaces))
    }

    fn from_parts(
        profile_store: ProfileStore,
        profiles: ProfileCollection,
        interfaces: Vec<NetworkInterface>,
    ) -> Self {
        let selected_interface_guid = interfaces.first().map(|interface| interface.guid.clone());
        Self {
            inner: Mutex::new(StateData {
                revision: 0,
                profiles,
                profile_store,
                interfaces,
                selected_interface_guid,
                advanced_settings: AdvancedSettings::default(),
                lifecycle: LifecycleState::default(),
            }),
        }
    }

    pub fn snapshot(&self) -> Result<AppSnapshot, StateError> {
        Ok(self.lock()?.snapshot())
    }

    pub fn create_profile(&self, draft: ProfileDraft) -> Result<AppSnapshot, StateError> {
        self.lock()?
            .commit_profiles(|profiles| profiles.create(draft).map(|_| ()))
    }

    pub fn update_profile(
        &self,
        id: ProfileId,
        draft: ProfileDraft,
    ) -> Result<AppSnapshot, StateError> {
        self.lock()?
            .commit_profiles(|profiles| profiles.update(id, draft).map(|_| ()))
    }

    pub fn delete_profile(&self, id: ProfileId) -> Result<AppSnapshot, StateError> {
        self.lock()?
            .commit_profiles(|profiles| profiles.delete(id).map(|_| ()))
    }

    pub fn select_profile(&self, id: ProfileId) -> Result<AppSnapshot, StateError> {
        self.lock()?.commit_profiles(|profiles| profiles.select(id))
    }

    pub fn refresh_interfaces(&self) -> Result<AppSnapshot, StateError> {
        self.lock()?.ensure_editable()?;
        let interfaces = discover_interfaces()?;
        self.lock()?.replace_interfaces(interfaces)
    }

    pub fn select_interface(&self, guid: &str) -> Result<AppSnapshot, StateError> {
        let mut state = self.lock()?;
        state.ensure_editable()?;
        if !state
            .interfaces
            .iter()
            .any(|interface| interface.guid == guid)
        {
            return Err(StateError::InterfaceNotFound);
        }
        state.selected_interface_guid = Some(guid.to_owned());
        state.advance_revision();
        Ok(state.snapshot())
    }

    pub fn replace_advanced_settings(
        &self,
        settings: AdvancedSettings,
    ) -> Result<AppSnapshot, StateError> {
        let mut state = self.lock()?;
        state.ensure_editable()?;
        state.advanced_settings = settings;
        state.advance_revision();
        Ok(state.snapshot())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, StateData>, StateError> {
        self.inner.lock().map_err(|_| StateError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        net::Ipv4Addr,
        path::{Path, PathBuf},
    };

    use serde_json::Value;
    use uuid::Uuid;

    use super::*;
    use crate::config::KcpMode;

    #[test]
    fn snapshot_lists_profiles_without_unselected_keys() {
        let directory = TestDirectory::new();
        let state = empty_state(&directory, vec![interface("Ethernet", "guid-a")]);
        state
            .create_profile(draft("Primary", "primary-key"))
            .unwrap();
        state.create_profile(draft("Backup", "backup-key")).unwrap();
        let backup_id = state.snapshot().unwrap().profiles[1].id;
        let snapshot = state.select_profile(backup_id).unwrap();

        let value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(value["profiles"][0].get("encryptionKey"), None);
        assert_eq!(value["profiles"][1].get("encryptionKey"), None);
        assert_eq!(
            value["selectedProfile"]["encryptionKey"],
            Value::String("backup-key".to_owned())
        );
        assert!(
            !serde_json::to_string(&snapshot)
                .unwrap()
                .contains("primary-key")
        );
    }

    #[test]
    fn successful_profile_mutations_are_persisted_as_one_canonical_collection() {
        let directory = TestDirectory::new();
        let path = directory.path().join("profiles.json");
        let state =
            AppState::load_with_interfaces(ProfileStore::new(path.clone()), Vec::new()).unwrap();

        let first = state.create_profile(draft("First", "first-key")).unwrap();
        let first_id = first.selected_profile.unwrap().id;
        let second = state.create_profile(draft("Second", "second-key")).unwrap();
        let second_id = second.profiles[1].id;
        state.select_profile(second_id).unwrap();
        state
            .update_profile(second_id, draft("Updated", "updated-key"))
            .unwrap();
        let final_snapshot = state.delete_profile(first_id).unwrap();

        let reloaded = AppState::load_with_interfaces(ProfileStore::new(path), Vec::new())
            .unwrap()
            .snapshot()
            .unwrap();
        assert_eq!(reloaded.revision, 0);
        assert_eq!(reloaded.profiles, final_snapshot.profiles);
        assert_eq!(reloaded.selected_profile, final_snapshot.selected_profile);
        assert_eq!(reloaded.interfaces, final_snapshot.interfaces);
        assert_eq!(
            reloaded.selected_interface_guid,
            final_snapshot.selected_interface_guid
        );
        assert_eq!(reloaded.advanced_settings, final_snapshot.advanced_settings);
        assert_eq!(reloaded.lifecycle, final_snapshot.lifecycle);
        assert_eq!(reloaded.profiles.len(), 1);
        assert_eq!(reloaded.selected_profile.unwrap().name, "Updated");
    }

    #[test]
    fn failed_profile_write_leaves_memory_unchanged() {
        let directory = TestDirectory::new();
        let state = AppState::from_parts(
            ProfileStore::new(directory.path().to_owned()),
            ProfileCollection::default(),
            Vec::new(),
        );
        let before = state.snapshot().unwrap();

        let error = state
            .create_profile(draft("Unsaved", "unsaved-key"))
            .unwrap_err();

        assert!(matches!(
            error,
            StateError::Profile(ProfileError::Io { .. })
        ));
        assert_eq!(state.snapshot().unwrap(), before);
    }

    #[test]
    fn interface_refresh_preserves_or_replaces_selection_deterministically() {
        let directory = TestDirectory::new();
        let state = empty_state(
            &directory,
            vec![
                interface("Ethernet", "guid-a"),
                interface("Wi-Fi", "guid-b"),
            ],
        );
        state.select_interface("guid-b").unwrap();

        let preserved = state
            .lock()
            .unwrap()
            .replace_interfaces(vec![
                interface("Wi-Fi renamed", "guid-b"),
                interface("Ethernet", "guid-a"),
            ])
            .unwrap();
        assert_eq!(preserved.selected_interface_guid.as_deref(), Some("guid-b"));

        let replaced = state
            .lock()
            .unwrap()
            .replace_interfaces(vec![interface("Mobile", "guid-c")])
            .unwrap();
        assert_eq!(replaced.selected_interface_guid.as_deref(), Some("guid-c"));

        let empty = state
            .lock()
            .unwrap()
            .replace_interfaces(Vec::new())
            .unwrap();
        assert_eq!(empty.selected_interface_guid, None);
    }

    #[test]
    fn invalid_interface_selection_does_not_mutate_state() {
        let directory = TestDirectory::new();
        let state = empty_state(&directory, vec![interface("Ethernet", "guid-a")]);
        let before = state.snapshot().unwrap();

        assert!(matches!(
            state.select_interface("missing"),
            Err(StateError::InterfaceNotFound)
        ));
        assert_eq!(state.snapshot().unwrap(), before);
    }

    #[test]
    fn advanced_settings_are_canonical_and_revisioned() {
        let directory = TestDirectory::new();
        let state = empty_state(&directory, Vec::new());
        let settings = AdvancedSettings {
            kcp_mode: Some(KcpMode::Fast3),
            connection_count: Some(3),
            ..AdvancedSettings::default()
        };

        let snapshot = state.replace_advanced_settings(settings.clone()).unwrap();

        assert_eq!(snapshot.revision, 1);
        assert_eq!(snapshot.advanced_settings, settings);
        assert_eq!(state.snapshot().unwrap(), snapshot);
    }

    #[test]
    fn every_disconnected_mutation_is_rejected_after_connect_begins() {
        let directory = TestDirectory::new();
        let state = empty_state(&directory, vec![interface("Ethernet", "guid-a")]);
        let profile_id = state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap()
            .selected_profile
            .unwrap()
            .id;
        state
            .inner
            .lock()
            .unwrap()
            .lifecycle
            .begin_connect()
            .unwrap();
        let before = state.snapshot().unwrap();

        assert!(matches!(
            state.create_profile(draft("Locked", "locked-key")),
            Err(StateError::Locked)
        ));
        assert!(matches!(
            state.update_profile(profile_id, draft("Changed", "changed-key")),
            Err(StateError::Locked)
        ));
        assert!(matches!(
            state.delete_profile(profile_id),
            Err(StateError::Locked)
        ));
        assert!(matches!(
            state.select_profile(profile_id),
            Err(StateError::Locked)
        ));
        assert!(matches!(
            state.select_interface("guid-a"),
            Err(StateError::Locked)
        ));
        assert!(matches!(
            state.replace_advanced_settings(AdvancedSettings::default()),
            Err(StateError::Locked)
        ));
        assert!(matches!(
            state
                .lock()
                .unwrap()
                .replace_interfaces(vec![interface("Wi-Fi", "guid-b")]),
            Err(StateError::Locked)
        ));
        assert_eq!(state.snapshot().unwrap(), before);
        assert!(!before.lifecycle.settings_editable);
    }

    fn empty_state(directory: &TestDirectory, interfaces: Vec<NetworkInterface>) -> AppState {
        AppState::load_with_interfaces(
            ProfileStore::new(directory.path().join("profiles.json")),
            interfaces,
        )
        .unwrap()
    }

    fn draft(name: &str, encryption_key: &str) -> ProfileDraft {
        ProfileDraft {
            name: name.to_owned(),
            server_host: "192.0.2.10".to_owned(),
            port: 9999,
            encryption_key: encryption_key.to_owned(),
        }
    }

    fn interface(friendly_name: &str, guid: &str) -> NetworkInterface {
        NetworkInterface {
            friendly_name: friendly_name.to_owned(),
            interface_name: friendly_name.to_owned(),
            guid: guid.to_owned(),
            local_address: Ipv4Addr::new(192, 0, 2, 20),
            gateway_address: Ipv4Addr::new(192, 0, 2, 1),
            gateway_mac: "00:11:22:33:44:55".to_owned(),
        }
    }

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("paqet-state-test-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
