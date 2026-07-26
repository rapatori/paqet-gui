use std::{
    collections::VecDeque,
    fmt, fs,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{
    Runtime,
    path::{BaseDirectory, PathResolver},
};

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{ProcessTreeExit, SupervisedPaqet, SupervisorEvent};

pub const MAX_SESSION_LOG_RECORDS: usize = 2_000;
pub const MAX_SESSION_LOG_BYTES: usize = 512 * 1024;
pub const MAX_LOG_RECORD_BYTES: usize = 16 * 1024;
pub const PAQET_EXECUTABLE_NAME: &str = "paqet_windows_amd64.exe";
pub const PAQET_EXECUTABLE_SIZE: u64 = 9_775_616;
pub const PAQET_EXECUTABLE_SHA256: &str =
    "49b377270473c223534ac1c2846d15c287863318e6fe6ee3c123f36ab97b441c";
pub const PAQET_RUN_SUBCOMMAND: &str = "run";
pub const PAQET_CONFIG_FLAG: &str = "-c";

const TRUNCATION_SUFFIX: &str = "... [truncated]";
const CONNECTED_MARKER: &str = "Client started:";
const CONNECTION_LOST_MARKER: &str = "connection lost, retrying....";
const CONFIGURATION_FATAL_MARKER: &str = "Failed to load configuration:";
const CLIENT_FATAL_MARKER: &str = "[FATAL] Client encountered an error:";
const SHUTDOWN_MARKER: &str = "Shutdown signal received, shutting down...";

#[derive(Debug)]
pub enum ProcessError {
    ResolveResource(tauri::Error),
    InvalidExecutable {
        path: PathBuf,
        source: std::io::Error,
    },
    ExecutableIsNotFile(PathBuf),
    ExecutableIdentityMismatch {
        path: PathBuf,
        expected_size: u64,
        actual_size: u64,
        expected_sha256: &'static str,
        actual_sha256: String,
    },
    InvalidConfigPath(PathBuf),
    InvalidWindowsPath {
        field: &'static str,
        path: PathBuf,
    },
    Platform {
        operation: &'static str,
        source: std::io::Error,
    },
    OutputReader {
        stream: OutputStream,
        source: std::io::Error,
    },
    OutputReaderPanicked(OutputStream),
    SupervisorPanicked,
    AlreadyFinished,
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResolveResource(error) => {
                write!(
                    formatter,
                    "failed to resolve the bundled paqet executable: {error}"
                )
            }
            Self::InvalidExecutable { path, source } => write!(
                formatter,
                "cannot use the bundled paqet executable at {}: {source}",
                path.display()
            ),
            Self::ExecutableIsNotFile(path) => write!(
                formatter,
                "the bundled paqet executable is not a file: {}",
                path.display()
            ),
            Self::ExecutableIdentityMismatch {
                path,
                expected_size,
                actual_size,
                expected_sha256,
                actual_sha256,
            } => write!(
                formatter,
                "the bundled paqet executable at {} does not match the pinned artifact (size {actual_size}/{expected_size}, SHA-256 {actual_sha256}/{expected_sha256})",
                path.display()
            ),
            Self::InvalidConfigPath(path) => write!(
                formatter,
                "the paqet configuration path must be absolute: {}",
                path.display()
            ),
            Self::InvalidWindowsPath { field, path } => write!(
                formatter,
                "the paqet {field} path contains a null character: {}",
                path.display()
            ),
            Self::Platform { operation, source } => {
                write!(formatter, "failed to {operation}: {source}")
            }
            Self::OutputReader { stream, source } => {
                write!(formatter, "failed to read paqet {stream}: {source}")
            }
            Self::OutputReaderPanicked(stream) => {
                write!(formatter, "the paqet {stream} reader stopped unexpectedly")
            }
            Self::SupervisorPanicked => {
                formatter.write_str("the paqet process supervisor stopped unexpectedly")
            }
            Self::AlreadyFinished => formatter.write_str("the paqet process has already finished"),
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ResolveResource(error) => Some(error),
            Self::InvalidExecutable { source, .. }
            | Self::Platform { source, .. }
            | Self::OutputReader { source, .. } => Some(source),
            Self::ExecutableIsNotFile(_)
            | Self::ExecutableIdentityMismatch { .. }
            | Self::InvalidConfigPath(_)
            | Self::InvalidWindowsPath { .. }
            | Self::OutputReaderPanicked(_)
            | Self::SupervisorPanicked
            | Self::AlreadyFinished => None,
        }
    }
}

pub fn resolve_paqet_executable<R: Runtime>(
    paths: &PathResolver<R>,
) -> Result<PathBuf, ProcessError> {
    paths
        .resolve(PAQET_EXECUTABLE_NAME, BaseDirectory::Resource)
        .map_err(ProcessError::ResolveResource)
}

pub fn validate_paqet_executable(path: &Path) -> Result<(), ProcessError> {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    let metadata =
        fs::symlink_metadata(path).map_err(|source| ProcessError::InvalidExecutable {
            path: path.to_owned(),
            source,
        })?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ProcessError::ExecutableIsNotFile(path.to_owned()));
    }
    Ok(())
}

pub fn validate_pinned_paqet_executable(path: &Path) -> Result<(), ProcessError> {
    open_pinned_paqet_executable(path).map(|_| ())
}

pub(crate) fn open_pinned_paqet_executable(path: &Path) -> Result<File, ProcessError> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
    };

    let mut file = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|source| ProcessError::InvalidExecutable {
            path: path.to_owned(),
            source,
        })?;
    let metadata = file
        .metadata()
        .map_err(|source| ProcessError::InvalidExecutable {
            path: path.to_owned(),
            source,
        })?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(ProcessError::ExecutableIsNotFile(path.to_owned()));
    }
    let actual_size = metadata.len();
    if actual_size != PAQET_EXECUTABLE_SIZE {
        return Err(ProcessError::ExecutableIdentityMismatch {
            path: path.to_owned(),
            expected_size: PAQET_EXECUTABLE_SIZE,
            actual_size,
            expected_sha256: PAQET_EXECUTABLE_SHA256,
            actual_sha256: "not calculated because size differed".to_owned(),
        });
    }

    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|source| ProcessError::InvalidExecutable {
                path: path.to_owned(),
                source,
            })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual_sha256 = format!("{:x}", hasher.finalize());
    if actual_sha256 != PAQET_EXECUTABLE_SHA256 {
        return Err(ProcessError::ExecutableIdentityMismatch {
            path: path.to_owned(),
            expected_size: PAQET_EXECUTABLE_SIZE,
            actual_size,
            expected_sha256: PAQET_EXECUTABLE_SHA256,
            actual_sha256,
        });
    }
    Ok(file)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LifecycleStatus {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LifecyclePhase {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessPresence {
    Absent,
    Running,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum FailureReason {
    LaunchFailed,
    ConnectionLost,
    ConfigurationRejected,
    ClientFailed,
    UnexpectedExit { code: Option<i32> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleAction {
    BeginConnect,
    MarkProcessSpawned,
    FailLaunch,
    BeginDisconnect,
    ObserveProcessExit,
}

impl fmt::Display for LifecycleAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BeginConnect => "begin connection",
            Self::MarkProcessSpawned => "mark the paqet process as spawned",
            Self::FailLaunch => "record a paqet launch failure",
            Self::BeginDisconnect => "begin disconnection",
            Self::ObserveProcessExit => "observe paqet process exit",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleTransitionError {
    action: LifecycleAction,
    status: LifecycleStatus,
    process: ProcessPresence,
}

impl LifecycleTransitionError {
    pub fn action(&self) -> LifecycleAction {
        self.action
    }

    pub fn status(&self) -> LifecycleStatus {
        self.status
    }

    pub fn process(&self) -> ProcessPresence {
        self.process
    }
}

impl fmt::Display for LifecycleTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "cannot {} while lifecycle is {:?} and process is {:?}",
            self.action, self.status, self.process
        )
    }
}

impl std::error::Error for LifecycleTransitionError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleState {
    phase: LifecyclePhase,
    process: ProcessPresence,
    failure: Option<FailureReason>,
}

impl Default for LifecycleState {
    fn default() -> Self {
        Self {
            phase: LifecyclePhase::Disconnected,
            process: ProcessPresence::Absent,
            failure: None,
        }
    }
}

impl LifecycleState {
    pub fn status(&self) -> LifecycleStatus {
        if self.phase == LifecyclePhase::Disconnected {
            LifecycleStatus::Disconnected
        } else if self.failure.is_some() {
            LifecycleStatus::Failed
        } else {
            match self.phase {
                LifecyclePhase::Disconnected => LifecycleStatus::Disconnected,
                LifecyclePhase::Connecting => LifecycleStatus::Connecting,
                LifecyclePhase::Connected => LifecycleStatus::Connected,
                LifecyclePhase::Disconnecting => LifecycleStatus::Disconnecting,
            }
        }
    }

    pub fn process(&self) -> ProcessPresence {
        self.process
    }

    pub fn failure(&self) -> Option<FailureReason> {
        self.failure
    }

    pub fn settings_editable(&self) -> bool {
        self.process == ProcessPresence::Absent && self.phase == LifecyclePhase::Disconnected
    }

    pub fn can_connect(&self) -> bool {
        self.settings_editable()
    }

    pub fn can_disconnect(&self) -> bool {
        self.process == ProcessPresence::Running
            && matches!(
                self.phase,
                LifecyclePhase::Connected | LifecyclePhase::Connecting
            )
            && (self.phase == LifecyclePhase::Connected || self.failure.is_some())
    }

    pub fn begin_connect(&mut self) -> Result<(), LifecycleTransitionError> {
        if !self.can_connect() {
            return Err(self.transition_error(LifecycleAction::BeginConnect));
        }
        self.phase = LifecyclePhase::Connecting;
        self.failure = None;
        Ok(())
    }

    pub fn mark_process_spawned(&mut self) -> Result<(), LifecycleTransitionError> {
        if self.phase != LifecyclePhase::Connecting || self.process != ProcessPresence::Absent {
            return Err(self.transition_error(LifecycleAction::MarkProcessSpawned));
        }
        self.process = ProcessPresence::Running;
        Ok(())
    }

    pub fn fail_launch(&mut self) -> Result<(), LifecycleTransitionError> {
        if self.phase != LifecyclePhase::Connecting || self.process != ProcessPresence::Absent {
            return Err(self.transition_error(LifecycleAction::FailLaunch));
        }
        self.phase = LifecyclePhase::Disconnected;
        self.failure = Some(FailureReason::LaunchFailed);
        Ok(())
    }

    pub fn begin_disconnect(&mut self) -> Result<(), LifecycleTransitionError> {
        let may_stop_running_process = self.process == ProcessPresence::Running
            && matches!(
                self.phase,
                LifecyclePhase::Connecting | LifecyclePhase::Connected
            );
        if !may_stop_running_process {
            return Err(self.transition_error(LifecycleAction::BeginDisconnect));
        }
        self.phase = LifecyclePhase::Disconnecting;
        self.failure = None;
        Ok(())
    }

    pub fn observe_output(&mut self, classification: LogClassification) {
        if self.process != ProcessPresence::Running {
            return;
        }
        match classification {
            LogClassification::Connected if self.phase == LifecyclePhase::Connecting => {
                self.phase = LifecyclePhase::Connected;
                self.failure = None;
            }
            LogClassification::ConnectionLost
                if matches!(
                    self.phase,
                    LifecyclePhase::Connecting | LifecyclePhase::Connected
                ) =>
            {
                self.failure = Some(FailureReason::ConnectionLost);
            }
            LogClassification::Fatal { fatal_kind }
                if matches!(
                    self.phase,
                    LifecyclePhase::Connecting | LifecyclePhase::Connected
                ) =>
            {
                self.failure = Some(match fatal_kind {
                    FatalKind::Configuration => FailureReason::ConfigurationRejected,
                    FatalKind::Client => FailureReason::ClientFailed,
                });
            }
            _ => {}
        }
    }

    pub fn observe_process_exit(
        &mut self,
        code: Option<i32>,
    ) -> Result<(), LifecycleTransitionError> {
        if self.process != ProcessPresence::Running {
            return Err(self.transition_error(LifecycleAction::ObserveProcessExit));
        }
        self.process = ProcessPresence::Absent;
        match self.phase {
            LifecyclePhase::Disconnecting => {
                self.phase = LifecyclePhase::Disconnected;
                self.failure = None;
            }
            LifecyclePhase::Connecting | LifecyclePhase::Connected => {
                self.phase = LifecyclePhase::Disconnected;
                if self.failure.is_none() {
                    self.failure = Some(FailureReason::UnexpectedExit { code });
                }
            }
            LifecyclePhase::Disconnected => {
                unreachable!("a disconnected lifecycle cannot own a running process")
            }
        }
        Ok(())
    }

    fn transition_error(&self, action: LifecycleAction) -> LifecycleTransitionError {
        LifecycleTransitionError {
            action,
            status: self.status(),
            process: self.process,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputStream {
    Stdout,
    Stderr,
}

impl fmt::Display for OutputStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FatalKind {
    Configuration,
    Client,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum LogClassification {
    Display,
    Connected,
    ConnectionLost,
    Fatal {
        #[serde(rename = "fatalKind")]
        fatal_kind: FatalKind,
    },
    ShutdownRequested,
}

pub fn classify_output(stream: OutputStream, text: &str) -> LogClassification {
    let text = text.trim_end_matches(['\r', '\n']);
    match stream {
        OutputStream::Stdout if text.contains(CLIENT_FATAL_MARKER) => LogClassification::Fatal {
            fatal_kind: FatalKind::Client,
        },
        OutputStream::Stderr if text.contains(CONFIGURATION_FATAL_MARKER) => {
            LogClassification::Fatal {
                fatal_kind: FatalKind::Configuration,
            }
        }
        OutputStream::Stdout if text.contains(CONNECTION_LOST_MARKER) => {
            LogClassification::ConnectionLost
        }
        OutputStream::Stdout if text.contains(CONNECTED_MARKER) => LogClassification::Connected,
        OutputStream::Stdout if text == SHUTDOWN_MARKER => LogClassification::ShutdownRequested,
        _ => LogClassification::Display,
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogRecord {
    pub sequence: u64,
    pub stream: OutputStream,
    pub text: String,
    pub classification: LogClassification,
    pub truncated: bool,
}

#[derive(Debug)]
pub struct SessionLog {
    records: VecDeque<LogRecord>,
    retained_bytes: usize,
    next_sequence: u64,
}

impl Default for SessionLog {
    fn default() -> Self {
        Self {
            records: VecDeque::new(),
            retained_bytes: 0,
            next_sequence: 1,
        }
    }
}

impl SessionLog {
    pub fn push(&mut self, stream: OutputStream, text: &str) -> LogRecord {
        self.push_captured(stream, text, false)
    }

    pub(super) fn push_captured(
        &mut self,
        stream: OutputStream,
        text: &str,
        externally_truncated: bool,
    ) -> LogRecord {
        let normalized = text.trim_end_matches(['\r', '\n']);
        let classification = classify_output(stream, normalized);
        let (mut text, mut truncated) = truncate_record(normalized, MAX_LOG_RECORD_BYTES);
        if externally_truncated && !truncated {
            let maximum_prefix = MAX_LOG_RECORD_BYTES.saturating_sub(TRUNCATION_SUFFIX.len());
            let mut prefix_end = maximum_prefix.min(text.len());
            while !text.is_char_boundary(prefix_end) {
                prefix_end -= 1;
            }
            text.truncate(prefix_end);
            text.push_str(TRUNCATION_SUFFIX);
            truncated = true;
        }
        let record = LogRecord {
            sequence: self.next_sequence,
            stream,
            text,
            classification,
            truncated,
        };
        self.next_sequence = self
            .next_sequence
            .checked_add(1)
            .expect("session log sequence exhausted");
        self.retained_bytes += record.text.len();
        self.records.push_back(record.clone());
        self.enforce_retention();
        record
    }

    pub fn records(&self) -> impl ExactSizeIterator<Item = &LogRecord> {
        self.records.iter()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.retained_bytes = 0;
    }

    fn enforce_retention(&mut self) {
        while self.records.len() > MAX_SESSION_LOG_RECORDS
            || self.retained_bytes > MAX_SESSION_LOG_BYTES
        {
            let removed = self
                .records
                .pop_front()
                .expect("retention limits require a retained record");
            self.retained_bytes -= removed.text.len();
        }
    }
}

fn truncate_record(text: &str, maximum_bytes: usize) -> (String, bool) {
    if text.len() <= maximum_bytes {
        return (text.to_owned(), false);
    }
    let prefix_limit = maximum_bytes.saturating_sub(TRUNCATION_SUFFIX.len());
    let mut prefix_end = prefix_limit.min(text.len());
    while !text.is_char_boundary(prefix_end) {
        prefix_end -= 1;
    }
    let mut truncated = String::with_capacity(maximum_bytes);
    truncated.push_str(&text[..prefix_end]);
    truncated.push_str(TRUNCATION_SUFFIX);
    (truncated, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running_connecting() -> LifecycleState {
        let mut lifecycle = LifecycleState::default();
        lifecycle.begin_connect().unwrap();
        lifecycle.mark_process_spawned().unwrap();
        lifecycle
    }

    #[test]
    fn successful_lifecycle_waits_for_markers_and_exit() {
        let mut lifecycle = LifecycleState::default();
        assert!(lifecycle.settings_editable());

        lifecycle.begin_connect().unwrap();
        assert_eq!(lifecycle.status(), LifecycleStatus::Connecting);
        assert!(!lifecycle.settings_editable());
        lifecycle.mark_process_spawned().unwrap();
        lifecycle.observe_output(LogClassification::Display);
        assert_eq!(lifecycle.status(), LifecycleStatus::Connecting);

        lifecycle.observe_output(LogClassification::Connected);
        assert_eq!(lifecycle.status(), LifecycleStatus::Connected);
        assert!(lifecycle.can_disconnect());
        lifecycle.begin_disconnect().unwrap();
        lifecycle.observe_output(LogClassification::ShutdownRequested);
        assert_eq!(lifecycle.status(), LifecycleStatus::Disconnecting);
        lifecycle.observe_process_exit(Some(0)).unwrap();

        assert_eq!(lifecycle, LifecycleState::default());
        assert!(lifecycle.settings_editable());
    }

    #[test]
    fn connection_loss_preserves_process_ownership_until_exit() {
        let mut lifecycle = running_connecting();
        lifecycle.observe_output(LogClassification::Connected);
        lifecycle.observe_output(LogClassification::ConnectionLost);

        assert_eq!(lifecycle.status(), LifecycleStatus::Failed);
        assert_eq!(lifecycle.process(), ProcessPresence::Running);
        assert_eq!(lifecycle.failure(), Some(FailureReason::ConnectionLost));
        assert!(!lifecycle.settings_editable());
        assert!(lifecycle.can_disconnect());

        lifecycle.observe_process_exit(Some(1)).unwrap();
        assert_eq!(lifecycle.status(), LifecycleStatus::Disconnected);
        assert_eq!(lifecycle.process(), ProcessPresence::Absent);
        assert_eq!(lifecycle.failure(), Some(FailureReason::ConnectionLost));
        assert!(lifecycle.settings_editable());
        lifecycle.begin_connect().unwrap();
        assert_eq!(lifecycle.failure(), None);
    }

    #[test]
    fn fatal_markers_map_to_typed_failures() {
        let cases = [
            (
                FatalKind::Configuration,
                FailureReason::ConfigurationRejected,
            ),
            (FatalKind::Client, FailureReason::ClientFailed),
        ];
        for (fatal_kind, expected) in cases {
            let mut lifecycle = running_connecting();
            lifecycle.observe_output(LogClassification::Fatal { fatal_kind });
            assert_eq!(lifecycle.status(), LifecycleStatus::Failed);
            assert_eq!(lifecycle.failure(), Some(expected));
            assert_eq!(lifecycle.process(), ProcessPresence::Running);
        }
    }

    #[test]
    fn launch_and_unexpected_exit_failures_are_distinct() {
        let mut launch_failure = LifecycleState::default();
        launch_failure.begin_connect().unwrap();
        launch_failure.fail_launch().unwrap();
        assert_eq!(launch_failure.status(), LifecycleStatus::Disconnected);
        assert_eq!(launch_failure.failure(), Some(FailureReason::LaunchFailed));
        assert!(launch_failure.settings_editable());

        let mut unexpected_exit = running_connecting();
        unexpected_exit.observe_process_exit(Some(23)).unwrap();
        assert_eq!(unexpected_exit.status(), LifecycleStatus::Disconnected);
        assert_eq!(
            unexpected_exit.failure(),
            Some(FailureReason::UnexpectedExit { code: Some(23) })
        );
        assert!(unexpected_exit.settings_editable());
    }

    #[test]
    fn command_and_process_transitions_reject_illegal_sources() {
        let mut disconnected = LifecycleState::default();
        let cases = [
            (
                disconnected.mark_process_spawned().unwrap_err(),
                LifecycleAction::MarkProcessSpawned,
            ),
            (
                disconnected.fail_launch().unwrap_err(),
                LifecycleAction::FailLaunch,
            ),
            (
                disconnected.begin_disconnect().unwrap_err(),
                LifecycleAction::BeginDisconnect,
            ),
            (
                disconnected.observe_process_exit(None).unwrap_err(),
                LifecycleAction::ObserveProcessExit,
            ),
        ];
        for (error, action) in cases {
            assert_eq!(error.action(), action);
            assert_eq!(error.status(), LifecycleStatus::Disconnected);
            assert_eq!(error.process(), ProcessPresence::Absent);
        }

        disconnected.begin_connect().unwrap();
        let error = disconnected.begin_connect().unwrap_err();
        assert_eq!(error.action(), LifecycleAction::BeginConnect);

        disconnected.mark_process_spawned().unwrap();
        disconnected.observe_output(LogClassification::Connected);
        assert_eq!(
            disconnected.mark_process_spawned().unwrap_err().action(),
            LifecycleAction::MarkProcessSpawned
        );
        assert_eq!(
            disconnected.fail_launch().unwrap_err().action(),
            LifecycleAction::FailLaunch
        );
        assert_eq!(
            disconnected.begin_connect().unwrap_err().action(),
            LifecycleAction::BeginConnect
        );

        disconnected.begin_disconnect().unwrap();
        assert_eq!(
            disconnected.begin_disconnect().unwrap_err().action(),
            LifecycleAction::BeginDisconnect
        );
        assert_eq!(
            disconnected.begin_connect().unwrap_err().action(),
            LifecycleAction::BeginConnect
        );
    }

    #[test]
    fn classifier_is_stream_specific_and_ignores_generic_errors() {
        let cases = [
            (
                OutputStream::Stdout,
                "2026-01-01 [INFO] Client started: example",
                LogClassification::Connected,
            ),
            (
                OutputStream::Stdout,
                "2026-01-01 [INFO] connection lost, retrying....\r\n",
                LogClassification::ConnectionLost,
            ),
            (
                OutputStream::Stderr,
                "2026/01/01 Failed to load configuration: invalid",
                LogClassification::Fatal {
                    fatal_kind: FatalKind::Configuration,
                },
            ),
            (
                OutputStream::Stdout,
                "2026-01-01 [FATAL] Client encountered an error: invalid",
                LogClassification::Fatal {
                    fatal_kind: FatalKind::Client,
                },
            ),
            (
                OutputStream::Stdout,
                "Shutdown signal received, shutting down...\n",
                LogClassification::ShutdownRequested,
            ),
            (
                OutputStream::Stdout,
                "2026-01-01 [ERROR] one proxied connection failed",
                LogClassification::Display,
            ),
            (
                OutputStream::Stderr,
                "Client started: wrong stream",
                LogClassification::Display,
            ),
            (
                OutputStream::Stderr,
                "connection lost, retrying....",
                LogClassification::Display,
            ),
            (
                OutputStream::Stdout,
                "Failed to load configuration: wrong stream",
                LogClassification::Display,
            ),
            (
                OutputStream::Stderr,
                "[FATAL] Client encountered an error: wrong stream",
                LogClassification::Display,
            ),
            (
                OutputStream::Stderr,
                "Shutdown signal received, shutting down...",
                LogClassification::Display,
            ),
        ];
        for (stream, text, expected) in cases {
            assert_eq!(classify_output(stream, text), expected, "line: {text}");
        }
    }

    #[test]
    fn session_log_preserves_order_and_record_limit() {
        let mut log = SessionLog::default();
        for index in 0..=MAX_SESSION_LOG_RECORDS {
            log.push(OutputStream::Stdout, &format!("record {index}"));
        }

        assert_eq!(log.len(), MAX_SESSION_LOG_RECORDS);
        assert_eq!(log.records().next().unwrap().sequence, 2);
        assert_eq!(log.records().last().unwrap().sequence, 2_001);
    }

    #[test]
    fn session_log_enforces_byte_and_individual_record_limits() {
        let mut log = SessionLog::default();
        let oversized = "x".repeat(MAX_LOG_RECORD_BYTES + 1);
        let first = log.push(OutputStream::Stdout, &oversized);
        assert!(first.truncated);
        assert_eq!(first.text.len(), MAX_LOG_RECORD_BYTES);
        assert!(first.text.ends_with(TRUNCATION_SUFFIX));

        let utf8 = "é".repeat(MAX_LOG_RECORD_BYTES);
        let utf8_record = log.push(OutputStream::Stderr, &utf8);
        assert!(utf8_record.truncated);
        assert_eq!(utf8_record.text.len(), MAX_LOG_RECORD_BYTES - 1);
        assert!(utf8_record.text.is_char_boundary(utf8_record.text.len()));

        while log.retained_bytes() <= MAX_SESSION_LOG_BYTES - MAX_LOG_RECORD_BYTES {
            log.push(OutputStream::Stdout, &"y".repeat(MAX_LOG_RECORD_BYTES));
        }
        log.push(OutputStream::Stdout, &"z".repeat(MAX_LOG_RECORD_BYTES));
        assert!(log.retained_bytes() <= MAX_SESSION_LOG_BYTES);
        assert_eq!(log.records().next().unwrap().sequence, 2);
        assert_eq!(
            log.records().last().unwrap().text,
            "z".repeat(MAX_LOG_RECORD_BYTES)
        );
    }

    #[test]
    fn clearing_records_does_not_reset_stream_sequence() {
        let mut log = SessionLog::default();
        assert_eq!(log.push(OutputStream::Stdout, "first").sequence, 1);
        log.clear();
        assert!(log.is_empty());
        assert_eq!(log.retained_bytes(), 0);
        assert_eq!(log.push(OutputStream::Stderr, "second").sequence, 2);
    }

    #[test]
    fn serialized_records_use_the_camel_case_tagged_wire_shape() {
        let record = LogRecord {
            sequence: 7,
            stream: OutputStream::Stdout,
            text: "failure".to_owned(),
            classification: LogClassification::Fatal {
                fatal_kind: FatalKind::Client,
            },
            truncated: false,
        };

        assert_eq!(
            serde_json::to_value(record).unwrap(),
            serde_json::json!({
                "sequence": 7,
                "stream": "stdout",
                "text": "failure",
                "classification": {
                    "kind": "fatal",
                    "fatalKind": "client"
                },
                "truncated": false
            })
        );
        assert_eq!(
            serde_json::to_value(FailureReason::UnexpectedExit { code: None }).unwrap(),
            serde_json::json!({ "kind": "unexpectedExit", "code": null })
        );
    }
}
