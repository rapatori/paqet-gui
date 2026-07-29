use std::{
    collections::VecDeque,
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, ipc::Channel};

use crate::{
    config::{AdvancedSettings, ConfigError, RuntimeConfigStore, generate},
    network::{NetworkError, NetworkInterface, discover_interfaces},
    process::{
        FailureReason, LifecycleState, LifecycleStatus, LogRecord, MAX_SESSION_LOG_BYTES,
        MAX_SESSION_LOG_RECORDS, ProcessError, ProcessPresence, ProcessTreeExit, SupervisedPaqet,
        SupervisorEvent, resolve_paqet_executable,
    },
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
    #[serde(with = "decimal_u64")]
    pub revision: u64,
    pub profiles: Vec<ProfileSummary>,
    pub selected_profile: Option<Profile>,
    pub interfaces: Vec<NetworkInterface>,
    pub selected_interface_guid: Option<String>,
    pub advanced_settings: AdvancedSettings,
    pub lifecycle: LifecycleSnapshot,
}

mod decimal_u64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum RuntimeEvent {
    Bootstrap {
        #[serde(with = "decimal_u64")]
        revision: u64,
        #[serde(with = "optional_decimal_u64")]
        session_id: Option<u64>,
        lifecycle: LifecycleSnapshot,
        gap: Option<RuntimeGap>,
        records: Vec<LogRecord>,
    },
    Lifecycle {
        #[serde(with = "decimal_u64")]
        revision: u64,
        #[serde(with = "optional_decimal_u64")]
        session_id: Option<u64>,
        lifecycle: LifecycleSnapshot,
    },
    Output {
        #[serde(with = "decimal_u64")]
        revision: u64,
        #[serde(with = "decimal_u64")]
        session_id: u64,
        lifecycle: LifecycleSnapshot,
        record: LogRecord,
    },
    Gap {
        #[serde(with = "decimal_u64")]
        revision: u64,
        #[serde(with = "decimal_u64")]
        session_id: u64,
        #[serde(with = "decimal_u64")]
        first_missing: u64,
        #[serde(with = "decimal_u64")]
        next_available: u64,
        lifecycle: LifecycleSnapshot,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeGap {
    #[serde(with = "decimal_u64")]
    pub first_missing: u64,
    #[serde(with = "decimal_u64")]
    pub next_available: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowCloseRequest {
    #[serde(with = "decimal_u64")]
    pub request_id: u64,
    pub lifecycle: LifecycleSnapshot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowCloseDecision {
    Allow,
    Confirm(WindowCloseRequest),
    Shutdown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationExitDecision {
    Allow,
    Shutdown,
}

mod optional_decimal_u64 {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.map(|value| value.to_string()).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer)?
            .map(|value| value.parse().map_err(serde::de::Error::custom))
            .transpose()
    }
}

#[derive(Debug)]
pub enum StateError {
    Locked,
    InterfaceNotFound,
    ProfileNotSelected,
    InterfaceNotSelected,
    CommandConflict,
    Profile(ProfileError),
    Network(NetworkError),
    Config(ConfigError),
    Process(ProcessError),
    Subscription,
    Unavailable,
}

impl fmt::Display for StateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Locked => formatter.write_str("settings cannot be changed while paqet is active"),
            Self::InterfaceNotFound => formatter.write_str("network interface was not found"),
            Self::ProfileNotSelected => formatter.write_str("a server profile is not selected"),
            Self::InterfaceNotSelected => {
                formatter.write_str("a network interface is not selected")
            }
            Self::CommandConflict => formatter.write_str("the lifecycle command is not available"),
            Self::Profile(error) => error.fmt(formatter),
            Self::Network(error) => error.fmt(formatter),
            Self::Config(error) => error.fmt(formatter),
            Self::Process(error) => error.fmt(formatter),
            Self::Subscription => formatter.write_str("the runtime event channel is unavailable"),
            Self::Unavailable => formatter.write_str("application state is unavailable"),
        }
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Profile(error) => Some(error),
            Self::Network(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::Process(error) => Some(error),
            Self::Locked
            | Self::InterfaceNotFound
            | Self::ProfileNotSelected
            | Self::InterfaceNotSelected
            | Self::CommandConflict
            | Self::Subscription
            | Self::Unavailable => None,
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

impl From<ConfigError> for StateError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<ProcessError> for StateError {
    fn from(error: ProcessError) -> Self {
        Self::Process(error)
    }
}

trait RuntimeProcess: Send {
    fn next_event_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<SupervisorEvent>, ProcessError>;
    fn disconnect(&mut self) -> Result<ProcessTreeExit, ProcessError>;
}

impl RuntimeProcess for SupervisedPaqet {
    fn next_event_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<SupervisorEvent>, ProcessError> {
        Self::next_event_timeout(self, timeout)
    }

    fn disconnect(&mut self) -> Result<ProcessTreeExit, ProcessError> {
        Self::disconnect(self)
    }
}

type RuntimeLauncher =
    Arc<dyn Fn(&Path) -> Result<Box<dyn RuntimeProcess>, ProcessError> + Send + Sync>;

enum RuntimeControl {
    Disconnect {
        response: Sender<Result<AppSnapshot, StateError>>,
    },
}

#[derive(Debug)]
struct RuntimeSession {
    id: u64,
    control: Sender<RuntimeControl>,
}

struct StateData {
    revision: u64,
    profiles: ProfileCollection,
    profile_store: ProfileStore,
    interfaces: Vec<NetworkInterface>,
    selected_interface_guid: Option<String>,
    advanced_settings: AdvancedSettings,
    lifecycle: LifecycleState,
    next_session_id: u64,
    latest_session_id: Option<u64>,
    active_session: Option<RuntimeSession>,
    runtime_records: VecDeque<LogRecord>,
    runtime_record_bytes: usize,
    subscriber: Option<Channel<RuntimeEvent>>,
    next_close_request_id: u64,
    pending_close_request_id: Option<u64>,
    close_confirmation_in_progress: bool,
    close_subscriber: Option<Channel<WindowCloseRequest>>,
    shutdown_requested: bool,
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

    fn publish(&mut self, event: RuntimeEvent) {
        let Some(channel) = self.subscriber.clone() else {
            return;
        };
        if channel.send(event).is_err()
            && self
                .subscriber
                .as_ref()
                .is_some_and(|current| current.id() == channel.id())
        {
            self.subscriber = None;
        }
    }

    fn lifecycle_event(&self, session_id: Option<u64>) -> RuntimeEvent {
        RuntimeEvent::Lifecycle {
            revision: self.revision,
            session_id,
            lifecycle: self.lifecycle.into(),
        }
    }

    fn retain_record(&mut self, record: LogRecord) {
        self.runtime_record_bytes += record.text.len();
        self.runtime_records.push_back(record);
        while self.runtime_records.len() > MAX_SESSION_LOG_RECORDS
            || self.runtime_record_bytes > MAX_SESSION_LOG_BYTES
        {
            let removed = self
                .runtime_records
                .pop_front()
                .expect("runtime log limits require a retained record");
            self.runtime_record_bytes -= removed.text.len();
        }
    }
}

pub struct AppState {
    inner: Arc<Mutex<StateData>>,
    config_store: RuntimeConfigStore,
    launcher: RuntimeLauncher,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            config_store: self.config_store.clone(),
            launcher: Arc::clone(&self.launcher),
        }
    }
}

impl fmt::Debug for AppState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppState")
            .field("config_store", &self.config_store)
            .field("launcher", &"trusted paqet launcher")
            .finish()
    }
}

impl AppState {
    pub fn from_app_handle<R: Runtime>(app: &AppHandle<R>) -> Result<Self, StateError> {
        let test_data_directory = test_data_directory();
        let test_storage_paths = test_data_directory.as_deref().map(test_storage_paths);
        let profile_store = if let Some((profile_path, _)) = &test_storage_paths {
            ProfileStore::new(profile_path.clone())
        } else {
            ProfileStore::from_app_handle(app)?
        };
        let profiles = profile_store.load()?;
        reject_persisted_test_profiles(test_data_directory.as_deref(), &profiles)?;
        let interfaces = discover_interfaces()?;
        let config_store = if let Some((_, config_path)) = test_storage_paths {
            RuntimeConfigStore::new(config_path)
        } else {
            RuntimeConfigStore::from_app_handle(app)?
        };
        let executable = resolve_paqet_executable(app.path())?;
        let launcher = Arc::new(move |config_path: &Path| {
            SupervisedPaqet::launch_pinned_executable(&executable, config_path)
                .map(|process| Box::new(process) as Box<dyn RuntimeProcess>)
        });
        Ok(Self::from_parts(
            profile_store,
            profiles,
            interfaces,
            config_store,
            launcher,
        ))
    }

    #[cfg(test)]
    fn load_with_interfaces(
        profile_store: ProfileStore,
        interfaces: Vec<NetworkInterface>,
    ) -> Result<Self, StateError> {
        let profiles = profile_store.load()?;
        let config_store = RuntimeConfigStore::new(
            profile_store
                .path()
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("config.yaml"),
        );
        let launcher: RuntimeLauncher = Arc::new(
            |_: &Path| -> Result<Box<dyn RuntimeProcess>, ProcessError> {
                Err(ProcessError::AlreadyFinished)
            },
        );
        Ok(Self::from_parts(
            profile_store,
            profiles,
            interfaces,
            config_store,
            launcher,
        ))
    }

    fn from_parts(
        profile_store: ProfileStore,
        profiles: ProfileCollection,
        interfaces: Vec<NetworkInterface>,
        config_store: RuntimeConfigStore,
        launcher: RuntimeLauncher,
    ) -> Self {
        let selected_interface_guid = interfaces.first().map(|interface| interface.guid.clone());
        Self {
            inner: Arc::new(Mutex::new(StateData {
                revision: 0,
                profiles,
                profile_store,
                interfaces,
                selected_interface_guid,
                advanced_settings: AdvancedSettings::default(),
                lifecycle: LifecycleState::default(),
                next_session_id: 1,
                latest_session_id: None,
                active_session: None,
                runtime_records: VecDeque::new(),
                runtime_record_bytes: 0,
                subscriber: None,
                next_close_request_id: 1,
                pending_close_request_id: None,
                close_confirmation_in_progress: false,
                close_subscriber: None,
                shutdown_requested: false,
            })),
            config_store,
            launcher,
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

    pub fn subscribe_runtime_events(
        &self,
        channel: Channel<RuntimeEvent>,
    ) -> Result<(), StateError> {
        let mut state = self.lock()?;
        state.subscriber = Some(channel.clone());
        let event = RuntimeEvent::Bootstrap {
            revision: state.revision,
            session_id: state.latest_session_id,
            lifecycle: state.lifecycle.into(),
            gap: state
                .runtime_records
                .front()
                .filter(|record| record.sequence > 1)
                .map(|record| RuntimeGap {
                    first_missing: 1,
                    next_available: record.sequence,
                }),
            records: state.runtime_records.iter().cloned().collect(),
        };
        if channel.send(event).is_err() {
            state.subscriber = None;
            return Err(StateError::Subscription);
        }
        Ok(())
    }

    pub fn connect(&self) -> Result<AppSnapshot, StateError> {
        let (session_id, profile, interface, settings) = {
            let mut state = self.lock()?;
            if state.shutdown_requested || !state.lifecycle.can_connect() {
                return Err(StateError::CommandConflict);
            }
            let profile = state
                .profiles
                .selected_profile()
                .cloned()
                .ok_or(StateError::ProfileNotSelected)?;
            let interface = state
                .selected_interface_guid
                .as_ref()
                .and_then(|guid| state.interfaces.iter().find(|item| item.guid == *guid))
                .cloned()
                .ok_or(StateError::InterfaceNotSelected)?;
            state.lifecycle.begin_connect().unwrap();
            let session_id = state.next_session_id;
            state.next_session_id = state
                .next_session_id
                .checked_add(1)
                .expect("runtime session identifier exhausted");
            state.latest_session_id = Some(session_id);
            state.runtime_records.clear();
            state.runtime_record_bytes = 0;
            state.advance_revision();
            let event = state.lifecycle_event(Some(session_id));
            state.publish(event);
            (
                session_id,
                profile,
                interface,
                state.advanced_settings.clone(),
            )
        };

        let generated = generate(&profile, &interface, &settings)
            .map_err(|error| self.fail_connect(session_id, error.into()))?;
        self.config_store
            .write(&generated)
            .map_err(|error| self.fail_connect(session_id, error.into()))?;
        if self.lock()?.shutdown_requested {
            return Err(self.fail_connect(session_id, StateError::CommandConflict));
        }
        let process = (self.launcher)(self.config_store.path())
            .map_err(|error| self.fail_connect(session_id, error.into()))?;
        let (control, requests) = mpsc::channel();
        {
            let mut state = self.lock()?;
            if state.latest_session_id != Some(session_id)
                || state.active_session.is_some()
                || state.lifecycle.mark_process_spawned().is_err()
                || state.shutdown_requested
            {
                drop(process);
                drop(state);
                return Err(self.fail_connect(session_id, StateError::CommandConflict));
            }
            state.active_session = Some(RuntimeSession {
                id: session_id,
                control,
            });
            state.advance_revision();
            let event = state.lifecycle_event(Some(session_id));
            state.publish(event);
        }

        let inner = Arc::clone(&self.inner);
        let spawn_result = thread::Builder::new()
            .name(format!("paqet-runtime-{session_id}"))
            .spawn(move || coordinate_runtime(inner, session_id, process, requests));
        if let Err(source) = spawn_result {
            let error = ProcessError::Platform {
                operation: "start the application runtime coordinator",
                source,
            };
            return Err(self.fail_running_session(session_id, error.into()));
        }
        self.snapshot()
    }

    pub fn disconnect(&self) -> Result<AppSnapshot, StateError> {
        self.stop_runtime(false)
    }

    pub fn shutdown(&self) -> Result<AppSnapshot, StateError> {
        {
            let mut state = self.lock()?;
            state.shutdown_requested = true;
            if state.lifecycle.settings_editable() {
                return Ok(state.snapshot());
            }
            if state.lifecycle.process() == ProcessPresence::Absent {
                drop(state);
                return self.wait_for_shutdown();
            }
        }
        self.stop_runtime(true)
    }

    pub fn request_window_close(&self) -> Result<WindowCloseDecision, StateError> {
        let mut state = self.lock()?;
        if state.shutdown_requested {
            return Ok(WindowCloseDecision::Shutdown);
        }
        if state.lifecycle.settings_editable() {
            state.shutdown_requested = true;
            state.pending_close_request_id = None;
            return Ok(WindowCloseDecision::Allow);
        }
        let request_id = match state.pending_close_request_id {
            Some(request_id) => request_id,
            None => {
                let request_id = state.next_close_request_id;
                state.next_close_request_id = state
                    .next_close_request_id
                    .checked_add(1)
                    .expect("window close request identifier exhausted");
                state.pending_close_request_id = Some(request_id);
                request_id
            }
        };
        let request = WindowCloseRequest {
            request_id,
            lifecycle: state.lifecycle.into(),
        };
        let Some(channel) = state.close_subscriber.clone() else {
            state.shutdown_requested = true;
            return Ok(WindowCloseDecision::Shutdown);
        };
        if channel.send(request).is_err() {
            state.close_subscriber = None;
            state.shutdown_requested = true;
            return Ok(WindowCloseDecision::Shutdown);
        }
        Ok(WindowCloseDecision::Confirm(request))
    }

    pub fn subscribe_window_close_requests(
        &self,
        channel: Channel<WindowCloseRequest>,
    ) -> Result<(), StateError> {
        let mut state = self.lock()?;
        state.close_subscriber = Some(channel.clone());
        if state.shutdown_requested {
            return Ok(());
        }
        let Some(request_id) = state.pending_close_request_id else {
            return Ok(());
        };
        let request = WindowCloseRequest {
            request_id,
            lifecycle: state.lifecycle.into(),
        };
        if channel.send(request).is_err() {
            state.close_subscriber = None;
            return Err(StateError::Subscription);
        }
        Ok(())
    }

    pub fn begin_application_exit(&self) -> Result<ApplicationExitDecision, StateError> {
        let mut state = self.lock()?;
        state.shutdown_requested = true;
        Ok(if state.lifecycle.settings_editable() {
            ApplicationExitDecision::Allow
        } else {
            ApplicationExitDecision::Shutdown
        })
    }

    pub fn cancel_window_close(&self, request_id: u64) -> Result<(), StateError> {
        let mut state = self.lock()?;
        if state.pending_close_request_id != Some(request_id)
            || state.close_confirmation_in_progress
            || state.shutdown_requested
        {
            return Err(StateError::CommandConflict);
        }
        state.pending_close_request_id = None;
        Ok(())
    }

    pub fn confirm_window_close(&self, request_id: u64) -> Result<(), StateError> {
        {
            let mut state = self.lock()?;
            if state.pending_close_request_id != Some(request_id)
                || state.close_confirmation_in_progress
            {
                return Err(StateError::CommandConflict);
            }
            state.close_confirmation_in_progress = true;
        }
        if let Err(error) = self.shutdown() {
            self.lock()?.close_confirmation_in_progress = false;
            return Err(error);
        }
        let mut state = self.lock()?;
        state.close_confirmation_in_progress = false;
        if state.pending_close_request_id == Some(request_id) {
            state.pending_close_request_id = None;
        }
        Ok(())
    }

    fn stop_runtime(&self, join_existing: bool) -> Result<AppSnapshot, StateError> {
        let (response, result) = mpsc::channel();
        {
            let mut state = self.lock()?;
            if state.lifecycle.process() == ProcessPresence::Absent {
                return if join_existing {
                    Ok(state.snapshot())
                } else {
                    Err(StateError::CommandConflict)
                };
            }
            if state.lifecycle.status() == LifecycleStatus::Disconnecting {
                if join_existing {
                    drop(state);
                    return self.wait_for_shutdown();
                }
                return Err(StateError::CommandConflict);
            }
            let control = state
                .active_session
                .as_ref()
                .map(|session| session.control.clone())
                .ok_or(StateError::CommandConflict)?;
            control
                .send(RuntimeControl::Disconnect { response })
                .map_err(|_| StateError::Unavailable)?;
            state
                .lifecycle
                .begin_disconnect()
                .map_err(|_| StateError::CommandConflict)?;
            state.advance_revision();
            let event = state.lifecycle_event(state.latest_session_id);
            state.publish(event);
        }
        result.recv().map_err(|_| StateError::Unavailable)?
    }

    fn wait_for_shutdown(&self) -> Result<AppSnapshot, StateError> {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let snapshot = self.snapshot()?;
            if snapshot.lifecycle.settings_editable {
                return Ok(snapshot);
            }
            if Instant::now() >= deadline {
                return Err(StateError::Unavailable);
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn fail_connect(&self, session_id: u64, error: StateError) -> StateError {
        if let Ok(mut state) = self.lock()
            && state.latest_session_id == Some(session_id)
            && state.lifecycle.fail_launch().is_ok()
        {
            state.pending_close_request_id = None;
            state.close_confirmation_in_progress = false;
            state.advance_revision();
            let event = state.lifecycle_event(Some(session_id));
            state.publish(event);
        }
        error
    }

    fn fail_running_session(&self, session_id: u64, error: StateError) -> StateError {
        if let Ok(mut state) = self.lock()
            && state
                .active_session
                .as_ref()
                .is_some_and(|session| session.id == session_id)
        {
            state.active_session = None;
            let _ = state.lifecycle.observe_process_exit(None);
            state.pending_close_request_id = None;
            state.close_confirmation_in_progress = false;
            state.advance_revision();
            let event = state.lifecycle_event(Some(session_id));
            state.publish(event);
        }
        error
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, StateData>, StateError> {
        self.inner.lock().map_err(|_| StateError::Unavailable)
    }
}

#[cfg(debug_assertions)]
fn test_data_directory() -> Option<PathBuf> {
    select_test_data_directory(std::env::var_os("PAQET_GUI_TEST_DATA_DIR"))
}

#[cfg(not(debug_assertions))]
fn test_data_directory() -> Option<PathBuf> {
    select_test_data_directory(std::env::var_os("PAQET_GUI_TEST_DATA_DIR"))
}

#[cfg(debug_assertions)]
fn select_test_data_directory(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from)
}

#[cfg(not(debug_assertions))]
fn select_test_data_directory(_value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    None
}

fn reject_persisted_test_profiles(
    test_directory: Option<&Path>,
    profiles: &ProfileCollection,
) -> Result<(), StateError> {
    if test_directory.is_some() && !profiles.profiles().is_empty() {
        Err(StateError::Unavailable)
    } else {
        Ok(())
    }
}

fn test_storage_paths(directory: &Path) -> (PathBuf, PathBuf) {
    (
        directory.join("config").join("profiles.json"),
        directory.join("local").join("config.yaml"),
    )
}

fn coordinate_runtime(
    inner: Arc<Mutex<StateData>>,
    session_id: u64,
    mut process: Box<dyn RuntimeProcess>,
    requests: Receiver<RuntimeControl>,
) {
    loop {
        if let Ok(RuntimeControl::Disconnect { response }) = requests.try_recv() {
            let result = process.disconnect();
            let mut terminal_snapshot = None;
            loop {
                match process.next_event_timeout(Duration::ZERO) {
                    Ok(Some(event)) => {
                        terminal_snapshot = apply_supervisor_event(&inner, session_id, event);
                    }
                    Ok(None) => break,
                    Err(error) => {
                        drop(process);
                        let _ = response.send(Err(fail_runtime(&inner, session_id, error)));
                        return;
                    }
                }
            }
            let response_result = match result {
                Ok(exit) => terminal_snapshot
                    .or_else(|| {
                        apply_supervisor_event(&inner, session_id, SupervisorEvent::Exited(exit))
                    })
                    .ok_or(StateError::Unavailable),
                Err(error) => {
                    drop(process);
                    Err(fail_runtime(&inner, session_id, error))
                }
            };
            let _ = response.send(response_result);
            return;
        }

        match process.next_event_timeout(Duration::from_millis(25)) {
            Ok(Some(event)) => {
                let terminal = matches!(event, SupervisorEvent::Exited(_));
                let snapshot = apply_supervisor_event(&inner, session_id, event);
                if terminal {
                    if let Ok(RuntimeControl::Disconnect { response }) = requests.try_recv() {
                        let _ = response.send(snapshot.ok_or(StateError::Unavailable));
                    }
                    return;
                }
            }
            Ok(None) => {}
            Err(error) => {
                drop(process);
                fail_runtime(&inner, session_id, error);
                return;
            }
        }
    }
}

fn apply_supervisor_event(
    inner: &Mutex<StateData>,
    session_id: u64,
    event: SupervisorEvent,
) -> Option<AppSnapshot> {
    let Ok(mut state) = inner.lock() else {
        return None;
    };
    if !state
        .active_session
        .as_ref()
        .is_some_and(|session| session.id == session_id)
    {
        return None;
    }
    let event = match event {
        SupervisorEvent::Output(record) => {
            state.lifecycle.observe_output(record.classification);
            state.retain_record(record.clone());
            state.advance_revision();
            RuntimeEvent::Output {
                revision: state.revision,
                session_id,
                lifecycle: state.lifecycle.into(),
                record,
            }
        }
        SupervisorEvent::Gap {
            first_missing,
            next_available,
            classification,
        } => {
            if let Some(classification) = classification {
                state.lifecycle.observe_output(classification);
            }
            state.advance_revision();
            RuntimeEvent::Gap {
                revision: state.revision,
                session_id,
                first_missing,
                next_available,
                lifecycle: state.lifecycle.into(),
            }
        }
        SupervisorEvent::Exited(exit) => {
            state.active_session = None;
            let _ = state
                .lifecycle
                .observe_process_exit_requested(Some(exit.code), exit.requested);
            state.pending_close_request_id = None;
            state.close_confirmation_in_progress = false;
            state.advance_revision();
            state.lifecycle_event(Some(session_id))
        }
    };
    state.publish(event);
    Some(state.snapshot())
}

fn fail_runtime(inner: &Mutex<StateData>, session_id: u64, error: ProcessError) -> StateError {
    if let Ok(mut state) = inner.lock()
        && state
            .active_session
            .as_ref()
            .is_some_and(|session| session.id == session_id)
    {
        state.active_session = None;
        let _ = state.lifecycle.observe_process_exit(None);
        state.pending_close_request_id = None;
        state.close_confirmation_in_progress = false;
        state.advance_revision();
        let event = state.lifecycle_event(Some(session_id));
        state.publish(event);
    }
    StateError::Process(error)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        fs,
        net::Ipv4Addr,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        thread,
        time::{Duration, Instant},
    };

    use serde_json::Value;
    use tauri::ipc::InvokeResponseBody;
    use uuid::Uuid;

    use super::*;
    use crate::{
        config::KcpMode,
        process::{LogClassification, OutputStream},
    };

    #[test]
    fn test_data_override_is_debug_only() {
        let selected = select_test_data_directory(Some(std::ffi::OsString::from("isolated")));
        #[cfg(debug_assertions)]
        assert_eq!(selected, Some(PathBuf::from("isolated")));
        #[cfg(not(debug_assertions))]
        assert_eq!(selected, None);
    }

    #[test]
    fn test_data_override_rejects_persisted_profiles() {
        let mut profiles = ProfileCollection::default();
        profiles.create(draft("Unexpected", "secret")).unwrap();

        assert!(matches!(
            reject_persisted_test_profiles(Some(Path::new("isolated")), &profiles),
            Err(StateError::Unavailable)
        ));
        assert!(reject_persisted_test_profiles(None, &profiles).is_ok());
    }

    #[test]
    fn test_data_override_redirects_both_secret_bearing_stores() {
        assert_eq!(
            test_storage_paths(Path::new("isolated")),
            (
                PathBuf::from("isolated/config/profiles.json"),
                PathBuf::from("isolated/local/config.yaml")
            )
        );
    }

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
            RuntimeConfigStore::new(directory.path().join("config.yaml")),
            unavailable_launcher(),
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
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        let profile_id = state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap()
            .selected_profile
            .unwrap()
            .id;
        let before = state.connect().unwrap();

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
            state.select_interface(&valid_guid()),
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
        assert_eq!(factory.launches.load(Ordering::SeqCst), 1);
        assert!(factory.config_paths.lock().unwrap()[0].is_file());
        state.disconnect().unwrap();
    }

    #[test]
    fn connect_serializes_command_races_and_coordinates_ordered_lifecycle() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();

        let connected = state.connect().unwrap();
        assert_eq!(connected.lifecycle.status, LifecycleStatus::Connecting);
        assert_eq!(connected.lifecycle.process, ProcessPresence::Running);
        assert!(matches!(state.connect(), Err(StateError::CommandConflict)));
        assert_eq!(factory.launches.load(Ordering::SeqCst), 1);
        let yaml = fs::read_to_string(&factory.config_paths.lock().unwrap()[0]).unwrap();
        assert!(yaml.contains("existing-key"));

        factory.send(
            0,
            SupervisorEvent::Output(LogRecord {
                sequence: 1,
                stream: OutputStream::Stdout,
                text: "Client started: test".to_owned(),
                classification: LogClassification::Connected,
                truncated: false,
            }),
        );
        let snapshot = wait_for_snapshot(&state, |snapshot| {
            snapshot.lifecycle.status == LifecycleStatus::Connected
        });
        assert_eq!(snapshot.lifecycle.process, ProcessPresence::Running);

        let disconnected = state.disconnect().unwrap();
        assert_eq!(disconnected.lifecycle.status, LifecycleStatus::Disconnected);
        assert_eq!(disconnected.lifecycle.process, ProcessPresence::Absent);
        assert!(disconnected.lifecycle.settings_editable);
        assert_eq!(factory.disconnects.load(Ordering::SeqCst), 1);
        assert!(matches!(
            state.disconnect(),
            Err(StateError::CommandConflict)
        ));
    }

    #[test]
    fn failed_generation_unlocks_state_without_launching_and_reconnects_with_new_session() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();
        state
            .replace_advanced_settings(AdvancedSettings {
                connection_count: Some(0),
                ..AdvancedSettings::default()
            })
            .unwrap();

        assert!(matches!(state.connect(), Err(StateError::Config(_))));
        let failed = state.snapshot().unwrap();
        assert!(failed.lifecycle.settings_editable);
        assert_eq!(failed.lifecycle.failure, Some(FailureReason::LaunchFailed));
        assert_eq!(factory.launches.load(Ordering::SeqCst), 0);

        state
            .replace_advanced_settings(AdvancedSettings::default())
            .unwrap();
        state.connect().unwrap();
        assert_eq!(state.lock().unwrap().latest_session_id, Some(2));
        state.disconnect().unwrap();
        state.connect().unwrap();
        assert_eq!(state.lock().unwrap().latest_session_id, Some(3));
        state.disconnect().unwrap();
    }

    #[test]
    fn natural_exit_allows_reconnect_and_stale_session_events_are_ignored() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();
        state.connect().unwrap();
        factory.send(
            0,
            SupervisorEvent::Exited(ProcessTreeExit {
                code: 23,
                requested: false,
            }),
        );
        let exited = wait_for_snapshot(&state, |snapshot| {
            snapshot.lifecycle.process == ProcessPresence::Absent
        });
        assert_eq!(
            exited.lifecycle.failure,
            Some(FailureReason::UnexpectedExit { code: Some(23) })
        );

        state.connect().unwrap();
        let before = state.snapshot().unwrap();
        assert!(
            apply_supervisor_event(
                &state.inner,
                1,
                SupervisorEvent::Output(LogRecord {
                    sequence: 99,
                    stream: OutputStream::Stdout,
                    text: "stale".to_owned(),
                    classification: LogClassification::ConnectionLost,
                    truncated: false,
                }),
            )
            .is_none()
        );
        assert_eq!(state.snapshot().unwrap(), before);
        assert_eq!(state.lock().unwrap().latest_session_id, Some(2));
        state.disconnect().unwrap();
    }

    #[test]
    fn pending_natural_exit_is_not_erased_by_disconnect_state() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();
        state.connect().unwrap();
        {
            let mut data = state.lock().unwrap();
            data.lifecycle.begin_disconnect().unwrap();
        }

        let snapshot = apply_supervisor_event(
            &state.inner,
            1,
            SupervisorEvent::Exited(ProcessTreeExit {
                code: 41,
                requested: false,
            }),
        )
        .unwrap();

        assert_eq!(snapshot.lifecycle.status, LifecycleStatus::Disconnected);
        assert_eq!(
            snapshot.lifecycle.failure,
            Some(FailureReason::UnexpectedExit { code: Some(41) })
        );
    }

    #[test]
    fn window_close_requires_confirmation_only_while_state_is_locked() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let disconnected = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));

        assert_eq!(
            disconnected.request_window_close().unwrap(),
            WindowCloseDecision::Allow
        );
        assert!(matches!(
            disconnected.connect(),
            Err(StateError::CommandConflict)
        ));

        let second_directory = TestDirectory::new();
        let state = runtime_state(
            &second_directory,
            vec![valid_interface()],
            Arc::clone(&factory),
        );
        state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();
        state.connect().unwrap();
        let received = Arc::new(Mutex::new(Vec::<WindowCloseRequest>::new()));
        let target = Arc::clone(&received);
        state
            .subscribe_window_close_requests(Channel::new(move |body: InvokeResponseBody| {
                target.lock().unwrap().push(body.deserialize().unwrap());
                Ok(())
            }))
            .unwrap();

        let first = state.request_window_close().unwrap();
        let repeated = state.request_window_close().unwrap();
        assert_eq!(first, repeated);
        let WindowCloseDecision::Confirm(request) = first else {
            panic!("a running process must require close confirmation");
        };
        assert_eq!(request.request_id, 1);
        assert_eq!(request.lifecycle.process, ProcessPresence::Running);
        assert_eq!(received.lock().unwrap().as_slice(), &[request, request]);

        state.cancel_window_close(request.request_id).unwrap();
        assert!(matches!(
            state.cancel_window_close(request.request_id),
            Err(StateError::CommandConflict)
        ));
        let WindowCloseDecision::Confirm(retried) = state.request_window_close().unwrap() else {
            panic!("a running process must require close confirmation");
        };
        assert_eq!(retried.request_id, 2);
        assert!(matches!(
            state.confirm_window_close(request.request_id),
            Err(StateError::CommandConflict)
        ));

        state.confirm_window_close(retried.request_id).unwrap();
        assert_eq!(factory.disconnects.load(Ordering::SeqCst), 1);
        assert_eq!(
            state.snapshot().unwrap().lifecycle.process,
            ProcessPresence::Absent
        );
        assert_eq!(
            state.request_window_close().unwrap(),
            WindowCloseDecision::Shutdown
        );
    }

    #[test]
    fn window_close_without_a_subscriber_falls_back_to_supervised_shutdown() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();
        state.connect().unwrap();

        assert_eq!(
            state.request_window_close().unwrap(),
            WindowCloseDecision::Shutdown
        );
        assert!(matches!(state.connect(), Err(StateError::CommandConflict)));
        state.shutdown().unwrap();
        assert_eq!(factory.disconnects.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn application_exit_atomically_prevents_a_later_connect() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));

        assert_eq!(
            state.begin_application_exit().unwrap(),
            ApplicationExitDecision::Allow
        );
        assert!(matches!(state.connect(), Err(StateError::CommandConflict)));
    }

    #[test]
    fn irreversible_application_exit_rejects_a_pending_close_cancellation() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();
        state.connect().unwrap();
        state
            .subscribe_window_close_requests(Channel::new(|_: InvokeResponseBody| Ok(())))
            .unwrap();
        let WindowCloseDecision::Confirm(request) = state.request_window_close().unwrap() else {
            panic!("a running process must require close confirmation");
        };

        assert_eq!(
            state.begin_application_exit().unwrap(),
            ApplicationExitDecision::Shutdown
        );
        assert!(matches!(
            state.cancel_window_close(request.request_id),
            Err(StateError::CommandConflict)
        ));
        assert_eq!(
            state.request_window_close().unwrap(),
            WindowCloseDecision::Shutdown
        );
        state.shutdown().unwrap();
    }

    #[test]
    fn shutdown_joins_an_in_progress_disconnect() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();
        state.connect().unwrap();
        {
            let mut data = state.lock().unwrap();
            data.lifecycle.begin_disconnect().unwrap();
        }
        let state_for_exit = state.clone();
        let waiter = thread::spawn(move || state_for_exit.shutdown().unwrap());

        factory.send(
            0,
            SupervisorEvent::Exited(ProcessTreeExit {
                code: 0,
                requested: true,
            }),
        );
        let snapshot = waiter.join().unwrap();

        assert_eq!(snapshot.lifecycle.status, LifecycleStatus::Disconnected);
        assert_eq!(snapshot.lifecycle.process, ProcessPresence::Absent);
        assert_eq!(factory.disconnects.load(Ordering::SeqCst), 0);
        assert!(matches!(state.connect(), Err(StateError::CommandConflict)));
    }

    #[test]
    fn natural_exit_invalidates_a_pending_close_confirmation() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();
        state.connect().unwrap();
        state
            .subscribe_window_close_requests(Channel::new(|_: InvokeResponseBody| Ok(())))
            .unwrap();
        let WindowCloseDecision::Confirm(request) = state.request_window_close().unwrap() else {
            panic!("a running process must require close confirmation");
        };

        factory.send(
            0,
            SupervisorEvent::Exited(ProcessTreeExit {
                code: 12,
                requested: false,
            }),
        );
        wait_for_snapshot(&state, |snapshot| {
            snapshot.lifecycle.process == ProcessPresence::Absent
        });

        assert_eq!(
            state.request_window_close().unwrap(),
            WindowCloseDecision::Allow
        );
        assert!(matches!(
            state.confirm_window_close(request.request_id),
            Err(StateError::CommandConflict)
        ));
    }

    #[test]
    fn missing_connect_inputs_do_not_mutate_revision_or_lifecycle() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let no_profile = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        let before = no_profile.snapshot().unwrap();
        assert!(matches!(
            no_profile.connect(),
            Err(StateError::ProfileNotSelected)
        ));
        assert_eq!(no_profile.snapshot().unwrap(), before);

        no_profile
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();
        no_profile.lock().unwrap().selected_interface_guid = None;
        let before = no_profile.snapshot().unwrap();
        assert!(matches!(
            no_profile.connect(),
            Err(StateError::InterfaceNotSelected)
        ));
        assert_eq!(no_profile.snapshot().unwrap(), before);
        assert_eq!(factory.launches.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn subscription_bootstrap_and_live_events_share_one_ordered_channel() {
        let directory = TestDirectory::new();
        let factory = Arc::new(FakeRuntimeFactory::default());
        let state = runtime_state(&directory, vec![valid_interface()], Arc::clone(&factory));
        state
            .create_profile(draft("Existing", "existing-key"))
            .unwrap();
        let received = Arc::new(Mutex::new(Vec::<RuntimeEvent>::new()));
        let target = Arc::clone(&received);
        let channel = Channel::new(move |body: InvokeResponseBody| {
            target.lock().unwrap().push(body.deserialize().unwrap());
            Ok(())
        });

        state.subscribe_runtime_events(channel).unwrap();
        state.connect().unwrap();
        factory.send(
            0,
            SupervisorEvent::Output(LogRecord {
                sequence: 1,
                stream: OutputStream::Stderr,
                text: "diagnostic".to_owned(),
                classification: LogClassification::Display,
                truncated: false,
            }),
        );
        wait_for_snapshot(&state, |snapshot| snapshot.revision >= 4);
        state.disconnect().unwrap();

        let events = received.lock().unwrap();
        assert!(matches!(events[0], RuntimeEvent::Bootstrap { .. }));
        assert!(matches!(events[1], RuntimeEvent::Lifecycle { .. }));
        assert!(events.iter().any(|event| matches!(event, RuntimeEvent::Output { record, .. } if record.text == "diagnostic")));
        let revisions = events
            .iter()
            .map(|event| match event {
                RuntimeEvent::Bootstrap { revision, .. }
                | RuntimeEvent::Lifecycle { revision, .. }
                | RuntimeEvent::Output { revision, .. }
                | RuntimeEvent::Gap { revision, .. } => *revision,
            })
            .collect::<Vec<_>>();
        assert!(revisions.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    fn empty_state(directory: &TestDirectory, interfaces: Vec<NetworkInterface>) -> AppState {
        AppState::load_with_interfaces(
            ProfileStore::new(directory.path().join("profiles.json")),
            interfaces,
        )
        .unwrap()
    }

    fn unavailable_launcher() -> RuntimeLauncher {
        Arc::new(
            |_: &Path| -> Result<Box<dyn RuntimeProcess>, ProcessError> {
                Err(ProcessError::AlreadyFinished)
            },
        )
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

    fn valid_guid() -> String {
        r"\Device\NPF_{12345678-1234-1234-1234-1234567890AB}".to_owned()
    }

    fn valid_interface() -> NetworkInterface {
        interface("Ethernet", &valid_guid())
    }

    fn runtime_state(
        directory: &TestDirectory,
        interfaces: Vec<NetworkInterface>,
        factory: Arc<FakeRuntimeFactory>,
    ) -> AppState {
        let profile_store = ProfileStore::new(directory.path().join("profiles.json"));
        let profiles = profile_store.load().unwrap();
        AppState::from_parts(
            profile_store,
            profiles,
            interfaces,
            RuntimeConfigStore::new(directory.path().join("config.yaml")),
            factory.launcher(),
        )
    }

    fn wait_for_snapshot(
        state: &AppState,
        predicate: impl Fn(&AppSnapshot) -> bool,
    ) -> AppSnapshot {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let snapshot = state.snapshot().unwrap();
            if predicate(&snapshot) {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "timed out waiting for state");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[derive(Default)]
    struct FakeRuntimeFactory {
        launches: AtomicUsize,
        disconnects: AtomicUsize,
        config_paths: Mutex<Vec<PathBuf>>,
        senders: Mutex<VecDeque<mpsc::Sender<SupervisorEvent>>>,
    }

    impl FakeRuntimeFactory {
        fn launcher(self: &Arc<Self>) -> RuntimeLauncher {
            let factory = Arc::clone(self);
            Arc::new(move |config_path: &Path| {
                factory.launches.fetch_add(1, Ordering::SeqCst);
                factory
                    .config_paths
                    .lock()
                    .unwrap()
                    .push(config_path.to_owned());
                let (sender, events) = mpsc::channel();
                factory.senders.lock().unwrap().push_back(sender);
                Ok(Box::new(FakeRuntime {
                    events,
                    factory: Arc::clone(&factory),
                }))
            })
        }

        fn send(&self, index: usize, event: SupervisorEvent) {
            self.senders.lock().unwrap()[index].send(event).unwrap();
        }
    }

    struct FakeRuntime {
        events: mpsc::Receiver<SupervisorEvent>,
        factory: Arc<FakeRuntimeFactory>,
    }

    impl RuntimeProcess for FakeRuntime {
        fn next_event_timeout(
            &mut self,
            timeout: Duration,
        ) -> Result<Option<SupervisorEvent>, ProcessError> {
            match self.events.recv_timeout(timeout) {
                Ok(event) => Ok(Some(event)),
                Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {
                    Ok(None)
                }
            }
        }

        fn disconnect(&mut self) -> Result<ProcessTreeExit, ProcessError> {
            self.factory.disconnects.fetch_add(1, Ordering::SeqCst);
            Ok(ProcessTreeExit {
                code: 0,
                requested: true,
            })
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
