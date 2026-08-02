use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

const SCHEMA_VERSION: u32 = 1;
const SETTINGS_FILE_NAME: &str = "settings.json";
const BACKUP_FILE_EXTENSION: &str = "bak";

pub const DEFAULT_SOCKS_PORT: u16 = 1080;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApplicationSettings {
    socks_port: u16,
}

impl ApplicationSettings {
    pub fn with_socks_port(port: u32) -> Result<Self, ApplicationSettingsError> {
        let socks_port = u16::try_from(port).map_err(|_| invalid_socks_port())?;
        if socks_port == 0 {
            return Err(invalid_socks_port());
        }
        Ok(Self { socks_port })
    }

    pub fn socks_port(self) -> u16 {
        self.socks_port
    }
}

impl Default for ApplicationSettings {
    fn default() -> Self {
        Self {
            socks_port: DEFAULT_SOCKS_PORT,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationSettingsField {
    SocksPort,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationSettingsValidationKind {
    OutOfRange,
}

#[derive(Debug)]
pub enum ApplicationSettingsError {
    Validation {
        field: ApplicationSettingsField,
        kind: ApplicationSettingsValidationKind,
    },
    UnsupportedSchemaVersion(u32),
    CorruptData,
    Io {
        operation: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for ApplicationSettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation { .. } => {
                formatter.write_str("SOCKS port is outside the supported range")
            }
            Self::UnsupportedSchemaVersion(version) => {
                write!(
                    formatter,
                    "application settings schema version {version} is not supported"
                )
            }
            Self::CorruptData => {
                formatter.write_str("application settings are invalid or corrupted")
            }
            Self::Io { operation, source } => write!(formatter, "could not {operation}: {source}"),
        }
    }
}

impl std::error::Error for ApplicationSettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ApplicationSettingsStore {
    path: PathBuf,
}

impl ApplicationSettingsStore {
    pub fn from_app_handle<R: Runtime>(
        app: &AppHandle<R>,
    ) -> Result<Self, ApplicationSettingsError> {
        let directory =
            app.path()
                .app_config_dir()
                .map_err(|source| ApplicationSettingsError::Io {
                    operation: "resolve the application settings directory",
                    source: io::Error::other(source),
                })?;
        Ok(Self::new(directory.join(SETTINGS_FILE_NAME)))
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> Result<ApplicationSettings, ApplicationSettingsError> {
        match self.load_file(&self.path) {
            Ok(Some(settings)) => Ok(settings),
            Ok(None) => self.recover_or_default(),
            Err(ApplicationSettingsError::UnsupportedSchemaVersion(version)) => {
                Err(ApplicationSettingsError::UnsupportedSchemaVersion(version))
            }
            Err(ApplicationSettingsError::Io { operation, source }) => {
                Err(ApplicationSettingsError::Io { operation, source })
            }
            Err(_) => self.recover_from_backup(),
        }
    }

    pub fn save(&self, settings: &ApplicationSettings) -> Result<(), ApplicationSettingsError> {
        let bytes = serialize_settings(settings)?;
        self.write_atomically(&bytes, true)
    }

    fn recover_or_default(&self) -> Result<ApplicationSettings, ApplicationSettingsError> {
        match self.load_file(&self.backup_path())? {
            Some(settings) => {
                self.write_atomically(&serialize_settings(&settings)?, false)?;
                Ok(settings)
            }
            None => Ok(ApplicationSettings::default()),
        }
    }

    fn recover_from_backup(&self) -> Result<ApplicationSettings, ApplicationSettingsError> {
        let settings = self
            .load_file(&self.backup_path())?
            .ok_or(ApplicationSettingsError::CorruptData)?;
        self.write_atomically(&serialize_settings(&settings)?, false)?;
        Ok(settings)
    }

    fn load_file(
        &self,
        path: &Path,
    ) -> Result<Option<ApplicationSettings>, ApplicationSettingsError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(ApplicationSettingsError::Io {
                    operation: "read application settings",
                    source,
                });
            }
        };
        let header: StoredSettingsHeader =
            serde_json::from_slice(&bytes).map_err(|_| ApplicationSettingsError::CorruptData)?;
        if header.schema_version != SCHEMA_VERSION {
            return Err(ApplicationSettingsError::UnsupportedSchemaVersion(
                header.schema_version,
            ));
        }
        let stored: StoredApplicationSettings =
            serde_json::from_slice(&bytes).map_err(|_| ApplicationSettingsError::CorruptData)?;
        ApplicationSettings::with_socks_port(stored.socks_port)
            .map(Some)
            .map_err(|_| ApplicationSettingsError::CorruptData)
    }

    fn write_atomically(
        &self,
        bytes: &[u8],
        keep_backup: bool,
    ) -> Result<(), ApplicationSettingsError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| ApplicationSettingsError::Io {
                operation: "resolve the application settings directory",
                source: io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "application settings path has no parent",
                ),
            })?;
        fs::create_dir_all(parent).map_err(|source| ApplicationSettingsError::Io {
            operation: "create the application settings directory",
            source,
        })?;

        let temp_path = self.unique_temp_path("tmp");
        let write_result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .map_err(|source| ApplicationSettingsError::Io {
                    operation: "create temporary application settings",
                    source,
                })?;
            file.write_all(bytes)
                .map_err(|source| ApplicationSettingsError::Io {
                    operation: "write temporary application settings",
                    source,
                })?;
            file.sync_all()
                .map_err(|source| ApplicationSettingsError::Io {
                    operation: "flush temporary application settings",
                    source,
                })?;
            drop(file);

            if self.path.exists() {
                if keep_backup {
                    self.refresh_backup()?;
                }
                replace_file(&self.path, &temp_path).map_err(|source| {
                    ApplicationSettingsError::Io {
                        operation: "replace application settings",
                        source,
                    }
                })
            } else {
                fs::rename(&temp_path, &self.path).map_err(|source| ApplicationSettingsError::Io {
                    operation: "install application settings",
                    source,
                })
            }
        })();
        if write_result.is_err() {
            let _ = remove_file_if_exists(&temp_path);
        }
        write_result
    }

    fn refresh_backup(&self) -> Result<(), ApplicationSettingsError> {
        let backup_path = self.backup_path();
        let backup_temp_path = self.unique_temp_path("bak.tmp");
        let refresh_result = (|| {
            fs::copy(&self.path, &backup_temp_path).map_err(|source| {
                ApplicationSettingsError::Io {
                    operation: "create temporary application settings backup",
                    source,
                }
            })?;
            OpenOptions::new()
                .write(true)
                .open(&backup_temp_path)
                .and_then(|file| file.sync_all())
                .map_err(|source| ApplicationSettingsError::Io {
                    operation: "flush temporary application settings backup",
                    source,
                })?;
            if backup_path.exists() {
                replace_file(&backup_path, &backup_temp_path).map_err(|source| {
                    ApplicationSettingsError::Io {
                        operation: "replace application settings backup",
                        source,
                    }
                })
            } else {
                fs::rename(&backup_temp_path, &backup_path).map_err(|source| {
                    ApplicationSettingsError::Io {
                        operation: "install application settings backup",
                        source,
                    }
                })
            }
        })();
        if refresh_result.is_err() {
            let _ = remove_file_if_exists(&backup_temp_path);
        }
        refresh_result
    }

    fn backup_path(&self) -> PathBuf {
        self.path.with_extension(BACKUP_FILE_EXTENSION)
    }

    fn unique_temp_path(&self, suffix: &str) -> PathBuf {
        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(SETTINGS_FILE_NAME);
        self.path
            .with_file_name(format!("{file_name}.{}.{suffix}", uuid::Uuid::new_v4()))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSettingsHeader {
    schema_version: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredApplicationSettings {
    schema_version: u32,
    socks_port: u32,
}

fn serialize_settings(settings: &ApplicationSettings) -> Result<Vec<u8>, ApplicationSettingsError> {
    let stored = StoredApplicationSettings {
        schema_version: SCHEMA_VERSION,
        socks_port: u32::from(settings.socks_port),
    };
    let mut bytes =
        serde_json::to_vec_pretty(&stored).map_err(|_| ApplicationSettingsError::CorruptData)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn invalid_socks_port() -> ApplicationSettingsError {
    ApplicationSettingsError::Validation {
        field: ApplicationSettingsField::SocksPort,
        kind: ApplicationSettingsValidationKind::OutOfRange,
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
    use uuid::Uuid;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("paqet-gui-settings-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn store(&self) -> ApplicationSettingsStore {
            ApplicationSettingsStore::new(self.0.join(SETTINGS_FILE_NAME))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn validates_the_complete_port_range() {
        assert_eq!(
            ApplicationSettings::with_socks_port(1)
                .unwrap()
                .socks_port(),
            1
        );
        assert_eq!(
            ApplicationSettings::with_socks_port(65_535)
                .unwrap()
                .socks_port(),
            65_535
        );
        for port in [0, 65_536, u32::MAX] {
            assert!(matches!(
                ApplicationSettings::with_socks_port(port),
                Err(ApplicationSettingsError::Validation {
                    field: ApplicationSettingsField::SocksPort,
                    kind: ApplicationSettingsValidationKind::OutOfRange
                })
            ));
        }
    }

    #[test]
    fn defaults_without_data_and_round_trips_versioned_json() {
        let directory = TestDirectory::new();
        let store = directory.store();
        assert_eq!(store.load().unwrap(), ApplicationSettings::default());

        let settings = ApplicationSettings::with_socks_port(20_080).unwrap();
        store.save(&settings).unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(store.path()).unwrap()).unwrap();
        assert_eq!(json["schemaVersion"], SCHEMA_VERSION);
        assert_eq!(json["socksPort"], 20_080);
        assert_eq!(store.load().unwrap(), settings);
    }

    #[test]
    fn recovers_last_known_good_settings_and_repairs_primary() {
        let directory = TestDirectory::new();
        let store = directory.store();
        let original = ApplicationSettings::with_socks_port(10_801).unwrap();
        let newer = ApplicationSettings::with_socks_port(10_802).unwrap();
        let newest = ApplicationSettings::with_socks_port(10_803).unwrap();
        store.save(&original).unwrap();
        store.save(&newer).unwrap();
        store.save(&newest).unwrap();
        fs::write(store.path(), b"{ interrupted write").unwrap();
        assert_eq!(store.load().unwrap(), newer);
        assert_eq!(store.load().unwrap(), newer);
    }

    #[test]
    fn rejects_future_schema_without_using_an_older_backup() {
        let directory = TestDirectory::new();
        let store = directory.store();
        store.save(&ApplicationSettings::default()).unwrap();
        store
            .save(&ApplicationSettings::with_socks_port(20_080).unwrap())
            .unwrap();
        let document = serde_json::json!({
            "schemaVersion": SCHEMA_VERSION + 1,
            "socksPort": 30_080,
            "futureSetting": true
        });
        let future_bytes = serde_json::to_vec(&document).unwrap();
        fs::write(store.path(), &future_bytes).unwrap();

        assert!(matches!(
            store.load(),
            Err(ApplicationSettingsError::UnsupportedSchemaVersion(version))
                if version == SCHEMA_VERSION + 1
        ));
        assert_eq!(fs::read(store.path()).unwrap(), future_bytes);
    }

    #[test]
    fn concurrent_stores_use_independent_temporary_files() {
        use std::{sync::Arc, thread};

        let directory = TestDirectory::new();
        let store = Arc::new(directory.store());
        let mut writers = Vec::new();
        for index in 0..8 {
            let store = Arc::clone(&store);
            writers.push(thread::spawn(move || {
                store.save(&ApplicationSettings::with_socks_port(20_000 + index).unwrap())
            }));
        }
        let successes = writers
            .into_iter()
            .map(|writer| writer.join().unwrap())
            .filter(Result::is_ok)
            .count();

        assert!(successes >= 1);
        let port = store.load().unwrap().socks_port();
        assert!((20_000..20_008).contains(&port));
    }
}
