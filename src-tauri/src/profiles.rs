use std::{
    collections::HashSet,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    net::IpAddr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 1;
const PROFILES_FILE_NAME: &str = "profiles.json";
const TEMP_FILE_EXTENSION: &str = "tmp";
const BACKUP_FILE_EXTENSION: &str = "bak";
const BACKUP_TEMP_FILE_EXTENSION: &str = "bak.tmp";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProfileId(Uuid);

impl ProfileId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProfileId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    pub server_host: String,
    pub port: u16,
    pub encryption_key: String,
}

impl fmt::Debug for Profile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Profile")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("server_host", &self.server_host)
            .field("port", &self.port)
            .field("encryption_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct ProfileDraft {
    pub name: String,
    pub server_host: String,
    pub port: u16,
    pub encryption_key: String,
}

impl fmt::Debug for ProfileDraft {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProfileDraft")
            .field("name", &self.name)
            .field("server_host", &self.server_host)
            .field("port", &self.port)
            .field("encryption_key", &"[REDACTED]")
            .finish()
    }
}

impl ProfileDraft {
    fn validate_and_normalize(self) -> Result<Self, ProfileError> {
        let name = self.name.trim().to_owned();
        validate_name(&name)?;

        let server_host = self.server_host.trim().to_owned();
        validate_server_host(&server_host)?;

        if self.port == 0 {
            return Err(ProfileError::Validation {
                field: ProfileField::Port,
                kind: ValidationKind::OutOfRange,
            });
        }

        if self.encryption_key.is_empty() {
            return Err(ProfileError::Validation {
                field: ProfileField::EncryptionKey,
                kind: ValidationKind::Required,
            });
        }

        Ok(Self {
            name,
            server_host,
            port: self.port,
            encryption_key: self.encryption_key,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileField {
    Name,
    ServerHost,
    Port,
    EncryptionKey,
}

impl fmt::Display for ProfileField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Name => "name",
            Self::ServerHost => "server host",
            Self::Port => "port",
            Self::EncryptionKey => "encryption key",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationKind {
    Required,
    InvalidFormat,
    OutOfRange,
    ContainsControlCharacters,
}

impl fmt::Display for ValidationKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Required => "is required",
            Self::InvalidFormat => "has an invalid format",
            Self::OutOfRange => "is outside the supported range",
            Self::ContainsControlCharacters => "contains control characters",
        })
    }
}

#[derive(Debug)]
pub enum ProfileError {
    Validation {
        field: ProfileField,
        kind: ValidationKind,
    },
    DuplicateName,
    DuplicateId,
    NotFound,
    SelectionNotFound,
    UnsupportedSchemaVersion(u32),
    CorruptData,
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation { field, kind } => write!(formatter, "{field} {kind}"),
            Self::DuplicateName => formatter.write_str("a profile with this name already exists"),
            Self::DuplicateId => {
                formatter.write_str("profile data contains a duplicate identifier")
            }
            Self::NotFound => formatter.write_str("profile was not found"),
            Self::SelectionNotFound => {
                formatter.write_str("selected profile does not exist in profile data")
            }
            Self::UnsupportedSchemaVersion(version) => {
                write!(
                    formatter,
                    "profile schema version {version} is not supported"
                )
            }
            Self::CorruptData => formatter.write_str("profile data is invalid or corrupted"),
            Self::Io { operation, source } => write!(formatter, "could not {operation}: {source}"),
        }
    }
}

impl std::error::Error for ProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProfileCollection {
    profiles: Vec<Profile>,
    selected_profile_id: Option<ProfileId>,
}

impl ProfileCollection {
    pub fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    pub fn selected_profile_id(&self) -> Option<ProfileId> {
        self.selected_profile_id
    }

    pub fn selected_profile(&self) -> Option<&Profile> {
        let selected_id = self.selected_profile_id?;
        self.profiles
            .iter()
            .find(|profile| profile.id == selected_id)
    }

    pub fn create(&mut self, draft: ProfileDraft) -> Result<&Profile, ProfileError> {
        let draft = draft.validate_and_normalize()?;
        self.ensure_unique_name(&draft.name, None)?;

        let profile = Profile {
            id: ProfileId::new(),
            name: draft.name,
            server_host: draft.server_host,
            port: draft.port,
            encryption_key: draft.encryption_key,
        };
        let id = profile.id;
        self.profiles.push(profile);
        if self.selected_profile_id.is_none() {
            self.selected_profile_id = Some(id);
        }

        Ok(self.profiles.last().expect("created profile must exist"))
    }

    pub fn update(&mut self, id: ProfileId, draft: ProfileDraft) -> Result<&Profile, ProfileError> {
        let draft = draft.validate_and_normalize()?;
        self.ensure_unique_name(&draft.name, Some(id))?;
        let profile = self
            .profiles
            .iter_mut()
            .find(|profile| profile.id == id)
            .ok_or(ProfileError::NotFound)?;

        profile.name = draft.name;
        profile.server_host = draft.server_host;
        profile.port = draft.port;
        profile.encryption_key = draft.encryption_key;
        Ok(profile)
    }

    pub fn delete(&mut self, id: ProfileId) -> Result<Profile, ProfileError> {
        let index = self
            .profiles
            .iter()
            .position(|profile| profile.id == id)
            .ok_or(ProfileError::NotFound)?;
        let deleted = self.profiles.remove(index);

        if self.selected_profile_id == Some(id) {
            let replacement_index = index.min(self.profiles.len().saturating_sub(1));
            self.selected_profile_id = self
                .profiles
                .get(replacement_index)
                .map(|profile| profile.id);
        }

        Ok(deleted)
    }

    pub fn select(&mut self, id: ProfileId) -> Result<(), ProfileError> {
        if !self.profiles.iter().any(|profile| profile.id == id) {
            return Err(ProfileError::NotFound);
        }
        self.selected_profile_id = Some(id);
        Ok(())
    }

    fn ensure_valid(&self) -> Result<(), ProfileError> {
        let mut ids = HashSet::with_capacity(self.profiles.len());
        let mut names = HashSet::with_capacity(self.profiles.len());

        for profile in &self.profiles {
            let normalized = ProfileDraft {
                name: profile.name.clone(),
                server_host: profile.server_host.clone(),
                port: profile.port,
                encryption_key: profile.encryption_key.clone(),
            }
            .validate_and_normalize()?;
            if normalized.name != profile.name || normalized.server_host != profile.server_host {
                return Err(ProfileError::CorruptData);
            }

            if !ids.insert(profile.id) {
                return Err(ProfileError::DuplicateId);
            }
            if !names.insert(profile.name.to_lowercase()) {
                return Err(ProfileError::DuplicateName);
            }
        }

        if self.selected_profile_id.is_some_and(|selected_id| {
            !self
                .profiles
                .iter()
                .any(|profile| profile.id == selected_id)
        }) {
            return Err(ProfileError::SelectionNotFound);
        }

        Ok(())
    }

    fn ensure_unique_name(
        &self,
        candidate: &str,
        excluded_id: Option<ProfileId>,
    ) -> Result<(), ProfileError> {
        let normalized_candidate = candidate.to_lowercase();
        if self.profiles.iter().any(|profile| {
            Some(profile.id) != excluded_id && profile.name.to_lowercase() == normalized_candidate
        }) {
            return Err(ProfileError::DuplicateName);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ProfileStore {
    path: PathBuf,
}

impl ProfileStore {
    pub fn from_app_handle<R: Runtime>(app: &AppHandle<R>) -> Result<Self, ProfileError> {
        let directory = app
            .path()
            .app_config_dir()
            .map_err(|source| ProfileError::Io {
                operation: "resolve the profile directory",
                source: io::Error::other(source),
            })?;
        Ok(Self::new(directory.join(PROFILES_FILE_NAME)))
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<ProfileCollection, ProfileError> {
        match self.load_file(&self.path) {
            Ok(Some(profiles)) => {
                self.remove_stale_temp_file();
                Ok(profiles)
            }
            Ok(None) => self.recover_or_default(),
            Err(ProfileError::UnsupportedSchemaVersion(version)) => {
                Err(ProfileError::UnsupportedSchemaVersion(version))
            }
            Err(ProfileError::Io { operation, source }) => {
                Err(ProfileError::Io { operation, source })
            }
            Err(_) => self.recover_from_backup(),
        }
    }

    pub fn save(&self, profiles: &ProfileCollection) -> Result<(), ProfileError> {
        profiles.ensure_valid()?;
        let document = StoredProfiles::from(profiles);
        let mut bytes =
            serde_json::to_vec_pretty(&document).map_err(|_| ProfileError::CorruptData)?;
        bytes.push(b'\n');
        self.write_atomically(&bytes, true)
    }

    fn recover_or_default(&self) -> Result<ProfileCollection, ProfileError> {
        let backup_path = self.backup_path();
        match self.load_file(&backup_path)? {
            Some(profiles) => {
                let bytes = serialize_collection(&profiles)?;
                self.write_atomically(&bytes, false)?;
                Ok(profiles)
            }
            None => {
                self.remove_stale_temp_file();
                Ok(ProfileCollection::default())
            }
        }
    }

    fn recover_from_backup(&self) -> Result<ProfileCollection, ProfileError> {
        let backup_path = self.backup_path();
        let profiles = self
            .load_file(&backup_path)?
            .ok_or(ProfileError::CorruptData)?;
        let bytes = serialize_collection(&profiles)?;
        self.write_atomically(&bytes, false)?;
        Ok(profiles)
    }

    fn load_file(&self, path: &Path) -> Result<Option<ProfileCollection>, ProfileError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(ProfileError::Io {
                    operation: "read profile data",
                    source,
                });
            }
        };

        let document: StoredProfiles =
            serde_json::from_slice(&bytes).map_err(|_| ProfileError::CorruptData)?;
        if document.schema_version != SCHEMA_VERSION {
            return Err(ProfileError::UnsupportedSchemaVersion(
                document.schema_version,
            ));
        }

        let profiles = ProfileCollection {
            profiles: document.profiles,
            selected_profile_id: document.selected_profile_id,
        };
        profiles.ensure_valid()?;
        Ok(Some(profiles))
    }

    fn write_atomically(&self, bytes: &[u8], keep_backup: bool) -> Result<(), ProfileError> {
        let parent = self.path.parent().ok_or_else(|| ProfileError::Io {
            operation: "resolve the profile directory",
            source: io::Error::new(io::ErrorKind::InvalidInput, "profile path has no parent"),
        })?;
        fs::create_dir_all(parent).map_err(|source| ProfileError::Io {
            operation: "create the profile directory",
            source,
        })?;

        let temp_path = self.temp_path();
        remove_file_if_exists(&temp_path).map_err(|source| ProfileError::Io {
            operation: "remove stale temporary profile data",
            source,
        })?;

        let write_result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .map_err(|source| ProfileError::Io {
                    operation: "create temporary profile data",
                    source,
                })?;
            file.write_all(bytes).map_err(|source| ProfileError::Io {
                operation: "write temporary profile data",
                source,
            })?;
            file.sync_all().map_err(|source| ProfileError::Io {
                operation: "flush temporary profile data",
                source,
            })?;
            drop(file);

            if self.path.exists() {
                if keep_backup {
                    self.refresh_backup()?;
                }
                replace_file(&self.path, &temp_path).map_err(|source| ProfileError::Io {
                    operation: "replace profile data",
                    source,
                })
            } else {
                fs::rename(&temp_path, &self.path).map_err(|source| ProfileError::Io {
                    operation: "install profile data",
                    source,
                })
            }
        })();

        if write_result.is_err() {
            let _ = remove_file_if_exists(&temp_path);
        }
        write_result
    }

    fn refresh_backup(&self) -> Result<(), ProfileError> {
        let backup_path = self.backup_path();
        let backup_temp_path = self.backup_temp_path();
        remove_file_if_exists(&backup_temp_path).map_err(|source| ProfileError::Io {
            operation: "remove stale temporary profile backup",
            source,
        })?;

        let refresh_result = (|| {
            fs::copy(&self.path, &backup_temp_path).map_err(|source| ProfileError::Io {
                operation: "create temporary profile backup",
                source,
            })?;
            OpenOptions::new()
                .write(true)
                .open(&backup_temp_path)
                .and_then(|file| file.sync_all())
                .map_err(|source| ProfileError::Io {
                    operation: "flush temporary profile backup",
                    source,
                })?;

            if backup_path.exists() {
                replace_file(&backup_path, &backup_temp_path).map_err(|source| ProfileError::Io {
                    operation: "replace profile backup",
                    source,
                })
            } else {
                fs::rename(&backup_temp_path, &backup_path).map_err(|source| ProfileError::Io {
                    operation: "install profile backup",
                    source,
                })
            }
        })();

        if refresh_result.is_err() {
            let _ = remove_file_if_exists(&backup_temp_path);
        }
        refresh_result
    }

    fn temp_path(&self) -> PathBuf {
        self.path.with_extension(TEMP_FILE_EXTENSION)
    }

    fn backup_path(&self) -> PathBuf {
        self.path.with_extension(BACKUP_FILE_EXTENSION)
    }

    fn backup_temp_path(&self) -> PathBuf {
        self.path.with_extension(BACKUP_TEMP_FILE_EXTENSION)
    }

    fn remove_stale_temp_file(&self) {
        let _ = remove_file_if_exists(&self.temp_path());
        let _ = remove_file_if_exists(&self.backup_temp_path());
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredProfiles {
    schema_version: u32,
    profiles: Vec<Profile>,
    selected_profile_id: Option<ProfileId>,
}

impl From<&ProfileCollection> for StoredProfiles {
    fn from(profiles: &ProfileCollection) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            profiles: profiles.profiles.clone(),
            selected_profile_id: profiles.selected_profile_id,
        }
    }
}

fn serialize_collection(profiles: &ProfileCollection) -> Result<Vec<u8>, ProfileError> {
    let mut bytes = serde_json::to_vec_pretty(&StoredProfiles::from(profiles))
        .map_err(|_| ProfileError::CorruptData)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn validate_name(name: &str) -> Result<(), ProfileError> {
    if name.is_empty() {
        return Err(ProfileError::Validation {
            field: ProfileField::Name,
            kind: ValidationKind::Required,
        });
    }
    if name.chars().any(char::is_control) {
        return Err(ProfileError::Validation {
            field: ProfileField::Name,
            kind: ValidationKind::ContainsControlCharacters,
        });
    }
    Ok(())
}

fn validate_server_host(host: &str) -> Result<(), ProfileError> {
    if host.is_empty() {
        return Err(ProfileError::Validation {
            field: ProfileField::ServerHost,
            kind: ValidationKind::Required,
        });
    }

    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(_)) => return Ok(()),
        Ok(IpAddr::V6(_)) => return Err(invalid_host()),
        Err(_) => {}
    }

    let hostname = host.strip_suffix('.').unwrap_or(host);
    if hostname.is_empty() || hostname.len() > 253 || !hostname.split('.').all(valid_dns_label) {
        return Err(invalid_host());
    }
    Ok(())
}

fn valid_dns_label(label: &str) -> bool {
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
}

fn invalid_host() -> ProfileError {
    ProfileError::Validation {
        field: ProfileField::ServerHost,
        kind: ValidationKind::InvalidFormat,
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
fn replace_file(replaced: &Path, replacement: &Path) -> io::Result<()> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

    let replaced = replaced
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let replacement = replacement
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: All pointers reference null-terminated UTF-16 buffers that remain alive for the call.
    let result = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(replaced: &Path, replacement: &Path) -> io::Result<()> {
    fs::rename(replacement, replaced)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-secret-that-must-not-leak";

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("paqet-gui-test-{}", Uuid::new_v4()));
            fs::create_dir(&path).expect("test directory should be created");
            Self(path)
        }

        fn store(&self) -> ProfileStore {
            ProfileStore::new(self.0.join(PROFILES_FILE_NAME))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn draft(name: &str, host: &str) -> ProfileDraft {
        ProfileDraft {
            name: name.to_owned(),
            server_host: host.to_owned(),
            port: 9999,
            encryption_key: SECRET.to_owned(),
        }
    }

    #[test]
    fn validates_and_normalizes_profile_fields() {
        let mut profiles = ProfileCollection::default();
        let profile = profiles
            .create(draft("  Primary  ", "  vpn.example.com  "))
            .expect("valid profile should be created");
        assert_eq!(profile.name, "Primary");
        assert_eq!(profile.server_host, "vpn.example.com");

        let invalid_hosts = [
            "",
            "bad host",
            "-vpn.example.com",
            "vpn..example.com",
            "2001:db8::1",
        ];
        for host in invalid_hosts {
            let error = ProfileDraft {
                server_host: host.to_owned(),
                ..draft("Invalid", "unused.example")
            }
            .validate_and_normalize()
            .expect_err("invalid host should be rejected");
            assert!(matches!(
                error,
                ProfileError::Validation {
                    field: ProfileField::ServerHost,
                    ..
                }
            ));
        }

        let mut zero_port = draft("Invalid", "127.0.0.1");
        zero_port.port = 0;
        assert!(matches!(
            zero_port.validate_and_normalize(),
            Err(ProfileError::Validation {
                field: ProfileField::Port,
                kind: ValidationKind::OutOfRange
            })
        ));
    }

    #[test]
    fn crud_preserves_selection_and_rejects_duplicate_names() {
        let mut profiles = ProfileCollection::default();
        let first_id = profiles
            .create(draft("Primary", "192.0.2.1"))
            .expect("first profile should be created")
            .id;
        let second_id = profiles
            .create(draft("Backup", "vpn.example.com"))
            .expect("second profile should be created")
            .id;
        assert_eq!(profiles.selected_profile_id(), Some(first_id));

        assert!(matches!(
            profiles.create(draft("primary", "192.0.2.2")),
            Err(ProfileError::DuplicateName)
        ));
        profiles
            .select(second_id)
            .expect("existing profile should be selectable");
        profiles
            .update(second_id, draft("Secondary", "198.51.100.4"))
            .expect("profile should be updated");
        assert_eq!(profiles.selected_profile().unwrap().name, "Secondary");

        profiles
            .delete(second_id)
            .expect("selected profile should be deleted");
        assert_eq!(profiles.selected_profile_id(), Some(first_id));
        profiles
            .delete(first_id)
            .expect("last profile should be deleted");
        assert_eq!(profiles.selected_profile_id(), None);
    }

    #[test]
    fn saves_versioned_json_and_round_trips_selected_profile() {
        let directory = TestDirectory::new();
        let store = directory.store();
        let mut profiles = ProfileCollection::default();
        let first_id = profiles.create(draft("Primary", "192.0.2.1")).unwrap().id;
        let selected_id = profiles
            .create(draft("Backup", "vpn.example.com"))
            .unwrap()
            .id;
        profiles.select(selected_id).unwrap();

        store.save(&profiles).expect("profiles should be saved");
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert_eq!(json["schemaVersion"], SCHEMA_VERSION);
        assert_eq!(json["selectedProfileId"], selected_id.to_string());
        assert_eq!(json["profiles"][0]["id"], first_id.to_string());
        assert_eq!(json["profiles"][0]["encryptionKey"], SECRET);

        let loaded = store.load().expect("profiles should load");
        assert_eq!(loaded, profiles);
        assert!(!store.temp_path().exists());
    }

    #[test]
    fn recovers_last_known_good_data_and_repairs_primary_file() {
        let directory = TestDirectory::new();
        let store = directory.store();
        let mut original = ProfileCollection::default();
        original.create(draft("Original", "192.0.2.1")).unwrap();
        store.save(&original).unwrap();

        let mut newer = original.clone();
        newer.create(draft("Newer", "198.51.100.2")).unwrap();
        store.save(&newer).unwrap();
        assert!(store.backup_path().exists());

        let mut newest = newer.clone();
        newest.create(draft("Newest", "203.0.113.3")).unwrap();
        store.save(&newest).unwrap();
        assert_eq!(store.load().unwrap(), newest);
        fs::write(store.path(), b"{ interrupted write").unwrap();
        fs::write(store.temp_path(), b"partial temporary data").unwrap();

        let recovered = store.load().expect("backup should be recovered");
        assert_eq!(recovered, newer);
        assert!(!store.temp_path().exists());
        assert_eq!(store.load().unwrap(), newer);
    }

    #[test]
    fn rejects_future_schema_without_rolling_back_to_backup() {
        let directory = TestDirectory::new();
        let store = directory.store();
        let mut profiles = ProfileCollection::default();
        profiles.create(draft("Original", "192.0.2.1")).unwrap();
        store.save(&profiles).unwrap();
        let mut changed = profiles.clone();
        changed.create(draft("Changed", "198.51.100.2")).unwrap();
        store.save(&changed).unwrap();

        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        document["schemaVersion"] = serde_json::json!(SCHEMA_VERSION + 1);
        fs::write(store.path(), serde_json::to_vec(&document).unwrap()).unwrap();

        assert!(matches!(
            store.load(),
            Err(ProfileError::UnsupportedSchemaVersion(version))
                if version == SCHEMA_VERSION + 1
        ));
    }

    #[test]
    fn errors_and_debug_output_never_contain_encryption_keys() {
        let draft = draft("Primary", "not a host");
        let draft_debug = format!("{draft:?}");
        let error = draft
            .clone()
            .validate_and_normalize()
            .expect_err("host should be invalid");
        let error_output = format!("{error:?} {error}");
        assert!(!draft_debug.contains(SECRET));
        assert!(!error_output.contains(SECRET));

        let profile = Profile {
            id: ProfileId::new(),
            name: "Primary".to_owned(),
            server_host: "192.0.2.1".to_owned(),
            port: 9999,
            encryption_key: SECRET.to_owned(),
        };
        assert!(!format!("{profile:?}").contains(SECRET));
    }

    #[test]
    fn invalid_persisted_selection_is_rejected_without_secret_leakage() {
        let directory = TestDirectory::new();
        let store = directory.store();
        let document = serde_json::json!({
            "schemaVersion": SCHEMA_VERSION,
            "profiles": [{
                "id": ProfileId::new(),
                "name": "Primary",
                "serverHost": "192.0.2.1",
                "port": 9999,
                "encryptionKey": SECRET
            }],
            "selectedProfileId": ProfileId::new()
        });
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), serde_json::to_vec(&document).unwrap()).unwrap();

        let error = store.load().expect_err("invalid selection should fail");
        let output = format!("{error:?} {error}");
        assert!(matches!(error, ProfileError::CorruptData));
        assert!(!output.contains(SECRET));
    }

    #[test]
    fn rejects_noncanonical_persisted_profile_values() {
        let directory = TestDirectory::new();
        let store = directory.store();
        let document = serde_json::json!({
            "schemaVersion": SCHEMA_VERSION,
            "profiles": [{
                "id": ProfileId::new(),
                "name": " Primary ",
                "serverHost": " vpn.example.com ",
                "port": 9999,
                "encryptionKey": SECRET
            }],
            "selectedProfileId": null
        });
        fs::create_dir_all(store.path().parent().unwrap()).unwrap();
        fs::write(store.path(), serde_json::to_vec(&document).unwrap()).unwrap();

        assert!(matches!(store.load(), Err(ProfileError::CorruptData)));
    }
}
