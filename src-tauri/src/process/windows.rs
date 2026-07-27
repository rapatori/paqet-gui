use std::{
    collections::VecDeque,
    ffi::{OsStr, c_void},
    io::{self, BufRead, BufReader},
    mem::{MaybeUninit, size_of, size_of_val},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use tauri::{Runtime, path::PathResolver};
use windows_sys::Win32::{
    Foundation::{
        HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Security::SECURITY_ATTRIBUTES,
    System::{
        JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        },
        Pipes::CreatePipe,
        Threading::{
            CREATE_NO_WINDOW, CreateProcessW, DeleteProcThreadAttributeList,
            EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, InitializeProcThreadAttributeList,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST, PROCESS_INFORMATION,
            STARTF_USESTDHANDLES, STARTUPINFOEXW, UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};

use super::{
    LogClassification, LogRecord, MAX_LOG_RECORD_BYTES, OutputStream, PAQET_CONFIG_FLAG,
    PAQET_RUN_SUBCOMMAND, ProcessError, SessionLog, open_pinned_paqet_executable,
    resolve_paqet_executable, validate_paqet_executable,
};

const DISCONNECT_EXIT_CODE: u32 = 0x5041_5145;
const TREE_EXIT_TIMEOUT: Duration = Duration::from_secs(10);
const SUPERVISOR_POLL_INTERVAL: Duration = Duration::from_millis(10);
const RAW_OUTPUT_QUEUE_CAPACITY: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessTreeExit {
    pub code: i32,
    pub requested: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SupervisorEvent {
    Output(LogRecord),
    Gap {
        first_missing: u64,
        next_available: u64,
        classification: Option<LogClassification>,
    },
    Exited(ProcessTreeExit),
}

#[derive(Debug)]
struct RawOutput {
    stream: OutputStream,
    text: String,
    truncated: bool,
}

#[derive(Debug)]
struct ReaderThread {
    stream: OutputStream,
    handle: Option<JoinHandle<io::Result<()>>>,
}

enum Control {
    Disconnect { deadline: Instant },
}

pub struct SupervisedPaqet {
    root_process_id: u32,
    job: Option<Arc<OwnedHandle>>,
    control_sender: mpsc::Sender<Control>,
    terminal_receiver: Receiver<Result<ProcessTreeExit, ProcessError>>,
    worker: Option<JoinHandle<()>>,
    shared: Arc<SharedState>,
    next_event_sequence: u64,
    terminal: Option<Result<ProcessTreeExit, ProcessError>>,
    terminal_event_sent: bool,
    finished: bool,
}

#[derive(Debug, Default)]
struct SharedState {
    state: Mutex<EventState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct EventState {
    log: SessionLog,
    significant: VecDeque<(u64, LogClassification)>,
    evicted_summary: Option<LogClassification>,
    terminal_ready: bool,
    revision: u64,
}

impl SupervisedPaqet {
    pub fn launch<R: Runtime>(
        paths: &PathResolver<R>,
        config_path: &Path,
    ) -> Result<Self, ProcessError> {
        let executable = resolve_paqet_executable(paths)?;
        let verified_executable = open_pinned_paqet_executable(&executable)?;
        Self::launch_executable(&executable, config_path, Some(verified_executable))
    }

    pub(crate) fn launch_pinned_executable(
        executable: &Path,
        config_path: &Path,
    ) -> Result<Self, ProcessError> {
        let verified_executable = open_pinned_paqet_executable(executable)?;
        Self::launch_executable(executable, config_path, Some(verified_executable))
    }

    #[doc(hidden)]
    #[cfg(feature = "process-test-support")]
    pub fn launch_test_executable(
        executable: &Path,
        config_path: &Path,
    ) -> Result<Self, ProcessError> {
        Self::launch_executable(executable, config_path, None)
    }

    fn launch_executable(
        executable: &Path,
        config_path: &Path,
        verified_executable: Option<std::fs::File>,
    ) -> Result<Self, ProcessError> {
        validate_launch_paths(executable, config_path)?;
        let created = create_supervised_process(executable, config_path)?;
        drop(verified_executable);
        let CreatedProcess {
            job,
            process,
            process_id,
            stdout,
            stderr,
        } = created;
        let (raw_sender, raw_receiver) = mpsc::sync_channel(RAW_OUTPUT_QUEUE_CAPACITY);
        let stdout_reader = spawn_reader(OutputStream::Stdout, stdout, raw_sender.clone())?;
        let stderr_reader = match spawn_reader(OutputStream::Stderr, stderr, raw_sender) {
            Ok(reader) => reader,
            Err(error) => {
                drop(job);
                drop(raw_receiver);
                join_reader_after_launch_failure(stdout_reader);
                return Err(error);
            }
        };

        let (control_sender, control_receiver) = mpsc::channel();
        let (terminal_sender, terminal_receiver) = mpsc::sync_channel(1);
        let shared = Arc::new(SharedState::default());
        let worker_shared = Arc::clone(&shared);
        let root_process_id = process_id;
        let job = Arc::new(job);
        let supervisor_job = Arc::clone(&job);
        let worker = thread::Builder::new()
            .name("paqet-process-supervisor".to_owned())
            .spawn(move || {
                let result = supervise_process_tree(
                    RunningProcess { job, process },
                    raw_receiver,
                    control_receiver,
                    Arc::clone(&worker_shared),
                    vec![stdout_reader, stderr_reader],
                );
                let _ = terminal_sender.send(result);
                {
                    let mut state = worker_shared
                        .state
                        .lock()
                        .expect("supervisor event state lock must not be poisoned");
                    state.terminal_ready = true;
                    state.revision = state.revision.wrapping_add(1);
                }
                worker_shared.changed.notify_all();
            })
            .map_err(|source| ProcessError::Platform {
                operation: "start the paqet process supervisor",
                source,
            })?;

        Ok(Self {
            root_process_id,
            job: Some(supervisor_job),
            control_sender,
            terminal_receiver,
            worker: Some(worker),
            shared,
            next_event_sequence: 1,
            terminal: None,
            terminal_event_sent: false,
            finished: false,
        })
    }

    pub fn root_process_id(&self) -> u32 {
        self.root_process_id
    }

    pub fn next_event_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<SupervisorEvent>, ProcessError> {
        let deadline = Instant::now() + timeout;
        loop {
            let (event, observed_revision) = self.next_available_event()?;
            if let Some(event) = event {
                return Ok(Some(event));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            let state = self
                .shared
                .state
                .lock()
                .expect("supervisor event state lock must not be poisoned");
            let _ = self
                .shared
                .changed
                .wait_timeout_while(state, remaining, |state| {
                    state.revision == observed_revision
                })
                .expect("supervisor event state lock must not be poisoned");
        }
    }

    pub fn records(&self) -> Vec<LogRecord> {
        self.shared
            .state
            .lock()
            .expect("supervisor event state lock must not be poisoned")
            .log
            .records()
            .cloned()
            .collect()
    }

    pub fn disconnect(&mut self) -> Result<ProcessTreeExit, ProcessError> {
        self.ensure_running()?;
        let deadline = Instant::now() + TREE_EXIT_TIMEOUT;
        let _ = self.control_sender.send(Control::Disconnect { deadline });
        self.finish(Some(deadline))
    }

    pub fn wait(&mut self) -> Result<ProcessTreeExit, ProcessError> {
        self.ensure_running()?;
        self.finish(None)
    }

    fn finish(&mut self, deadline: Option<Instant>) -> Result<ProcessTreeExit, ProcessError> {
        let terminal_result = match deadline {
            Some(deadline) => {
                self.receive_terminal(deadline.saturating_duration_since(Instant::now()))
            }
            None => self.receive_terminal_blocking(),
        };
        if let Err(observation_error) = terminal_result {
            if let Some(job) = self.job.as_deref() {
                let _ = terminate_job(job);
            }
            let worker_panicked = self
                .worker
                .take()
                .is_some_and(|worker| worker.join().is_err());
            self.finished = true;
            self.job.take();
            if worker_panicked {
                return Err(ProcessError::SupervisorPanicked);
            }
            let _ = self.receive_terminal_blocking();
            return Err(observation_error);
        }
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            self.finished = true;
            self.job.take();
            return Err(ProcessError::SupervisorPanicked);
        }
        self.finished = true;
        let terminal = self
            .terminal
            .take()
            .unwrap_or(Err(ProcessError::SupervisorPanicked));
        self.job.take();
        match terminal {
            Ok(exit) => {
                self.terminal = Some(Ok(exit));
                Ok(exit)
            }
            Err(error) => Err(error),
        }
    }

    fn next_available_event(&mut self) -> Result<(Option<SupervisorEvent>, u64), ProcessError> {
        let mut state = self
            .shared
            .state
            .lock()
            .expect("supervisor event state lock must not be poisoned");
        let first_retained = state.log.records().next().map(|record| record.sequence);
        if let Some(first_retained) = first_retained
            && self.next_event_sequence < first_retained
        {
            let first_missing = self.next_event_sequence;
            self.next_event_sequence = first_retained;
            let classification = state.evicted_summary.take();
            let revision = state.revision;
            return Ok((
                Some(SupervisorEvent::Gap {
                    first_missing,
                    next_available: first_retained,
                    classification,
                }),
                revision,
            ));
        }
        let record = state
            .log
            .records()
            .find(|record| record.sequence == self.next_event_sequence)
            .cloned();
        if let Some(record) = record {
            if state
                .significant
                .front()
                .is_some_and(|(sequence, _)| *sequence == record.sequence)
            {
                state.significant.pop_front();
            }
            self.next_event_sequence += 1;
            let revision = state.revision;
            return Ok((Some(SupervisorEvent::Output(record)), revision));
        }
        let terminal_ready = state.terminal_ready;
        let revision = state.revision;
        drop(state);
        if terminal_ready && !self.terminal_event_sent {
            self.receive_terminal(Duration::ZERO)?;
            match self.terminal.take() {
                Some(Ok(exit)) => {
                    self.terminal = Some(Ok(exit));
                    self.terminal_event_sent = true;
                    return Ok((Some(SupervisorEvent::Exited(exit)), revision));
                }
                Some(Err(error)) => {
                    self.terminal = Some(Err(error));
                    return match self.finish(None) {
                        Err(error) => Err(error),
                        Ok(_) => unreachable!("a stored terminal error cannot become success"),
                    };
                }
                None => {}
            }
        }
        Ok((None, revision))
    }

    fn receive_terminal(&mut self, timeout: Duration) -> Result<(), ProcessError> {
        if self.terminal.is_none() {
            self.terminal = Some(self.terminal_receiver.recv_timeout(timeout).map_err(
                |error| ProcessError::Platform {
                    operation: "observe paqet supervisor completion",
                    source: match error {
                        mpsc::RecvTimeoutError::Timeout => io::Error::new(
                            io::ErrorKind::TimedOut,
                            "the process supervisor did not finish before its deadline",
                        ),
                        mpsc::RecvTimeoutError::Disconnected => io::Error::other(
                            "the process supervisor stopped without a terminal result",
                        ),
                    },
                },
            )?);
        }
        Ok(())
    }

    fn receive_terminal_blocking(&mut self) -> Result<(), ProcessError> {
        if self.terminal.is_none() {
            self.terminal = Some(
                self.terminal_receiver
                    .recv()
                    .map_err(|_| ProcessError::SupervisorPanicked)?,
            );
        }
        Ok(())
    }

    fn ensure_running(&self) -> Result<(), ProcessError> {
        if self.finished {
            Err(ProcessError::AlreadyFinished)
        } else {
            Ok(())
        }
    }
}

impl Drop for SupervisedPaqet {
    fn drop(&mut self) {
        if !self.finished {
            let deadline = Instant::now() + TREE_EXIT_TIMEOUT;
            let _ = self.control_sender.send(Control::Disconnect { deadline });
            if self
                .receive_terminal(deadline.saturating_duration_since(Instant::now()))
                .is_err()
            {
                if let Some(job) = self.job.as_deref() {
                    let _ = terminate_job(job);
                }
                if let Some(worker) = self.worker.take() {
                    let _ = worker.join();
                }
                return;
            }
        }
        if self.terminal.is_some()
            && let Some(worker) = self.worker.take()
        {
            let _ = worker.join();
        }
    }
}

struct CreatedProcess {
    job: OwnedHandle,
    process: OwnedHandle,
    process_id: u32,
    stdout: OwnedHandle,
    stderr: OwnedHandle,
}

struct RunningProcess {
    job: Arc<OwnedHandle>,
    process: OwnedHandle,
}

fn supervise_process_tree(
    running: RunningProcess,
    raw_receiver: Receiver<RawOutput>,
    control_receiver: Receiver<Control>,
    shared: Arc<SharedState>,
    mut readers: Vec<ReaderThread>,
) -> Result<ProcessTreeExit, ProcessError> {
    let process_result =
        supervise_running_process(&running, &raw_receiver, &control_receiver, &shared);
    drop(running);
    let (exit, deadline) = match process_result {
        Ok(result) => result,
        Err(error) => {
            let _ = drain_reader_output(
                &raw_receiver,
                &shared,
                &mut readers,
                Instant::now() + TREE_EXIT_TIMEOUT,
            );
            return Err(error);
        }
    };
    let reader_result = drain_reader_output(&raw_receiver, &shared, &mut readers, deadline);
    reader_result?;
    Ok(exit)
}

fn supervise_running_process(
    running: &RunningProcess,
    raw_receiver: &Receiver<RawOutput>,
    control_receiver: &Receiver<Control>,
    shared: &SharedState,
) -> Result<(ProcessTreeExit, Instant), ProcessError> {
    let mut disconnect_requested = false;
    let mut shutdown_deadline = None;
    let code = loop {
        if process_has_exited(&running.process)? {
            break process_exit_code(&running.process)?;
        }
        if let Ok(Control::Disconnect { deadline }) = control_receiver.try_recv()
            && !disconnect_requested
        {
            disconnect_requested = true;
            shutdown_deadline = Some(deadline);
            terminate_job(&running.job)?;
        }
        match raw_receiver.recv_timeout(SUPERVISOR_POLL_INTERVAL) {
            Ok(raw) => sequence_output(raw, shared),
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
        }
        if shutdown_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(ProcessError::Platform {
                operation: "observe paqet process termination",
                source: io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the paqet process did not terminate before its shutdown deadline",
                ),
            });
        }
    };

    let deadline = shutdown_deadline.unwrap_or_else(|| Instant::now() + TREE_EXIT_TIMEOUT);
    if active_process_count(&running.job)? != 0 {
        terminate_job(&running.job)?;
    }
    wait_for_empty_job_and_drain(&running.job, raw_receiver, shared, deadline)?;

    Ok((
        ProcessTreeExit {
            code: code as i32,
            requested: disconnect_requested,
        },
        deadline,
    ))
}

fn sequence_output(raw: RawOutput, shared: &SharedState) {
    let mut state = shared
        .state
        .lock()
        .expect("supervisor event state lock must not be poisoned");
    let record = state
        .log
        .push_captured(raw.stream, &raw.text, raw.truncated);
    if lifecycle_affecting(record.classification) {
        state
            .significant
            .push_back((record.sequence, record.classification));
    }
    let first_retained = state.log.records().next().map(|record| record.sequence);
    if let Some(first_retained) = first_retained {
        while state
            .significant
            .front()
            .is_some_and(|(sequence, _)| *sequence < first_retained)
        {
            let (_, classification) = state.significant.pop_front().unwrap();
            state.evicted_summary = summarize_classification(state.evicted_summary, classification);
        }
    }
    state.revision = state.revision.wrapping_add(1);
    drop(state);
    shared.changed.notify_all();
}

fn lifecycle_affecting(classification: LogClassification) -> bool {
    matches!(
        classification,
        LogClassification::Connected
            | LogClassification::ConnectionLost
            | LogClassification::Fatal { .. }
    )
}

fn summarize_classification(
    current: Option<LogClassification>,
    next: LogClassification,
) -> Option<LogClassification> {
    match (current, next) {
        (
            Some(LogClassification::ConnectionLost | LogClassification::Fatal { .. }),
            next @ (LogClassification::ConnectionLost | LogClassification::Fatal { .. }),
        ) => Some(next),
        (
            Some(failure @ (LogClassification::ConnectionLost | LogClassification::Fatal { .. })),
            _,
        ) => Some(failure),
        (_, next) => Some(next),
    }
}

fn wait_for_empty_job_and_drain(
    job: &OwnedHandle,
    raw_receiver: &Receiver<RawOutput>,
    shared: &SharedState,
    deadline: Instant,
) -> Result<(), ProcessError> {
    loop {
        if active_process_count(job)? == 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(ProcessError::Platform {
                operation: "observe complete paqet process-tree termination",
                source: io::Error::new(io::ErrorKind::TimedOut, "the Job Object remained active"),
            });
        }
        match raw_receiver.recv_timeout(SUPERVISOR_POLL_INTERVAL) {
            Ok(raw) => sequence_output(raw, shared),
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => {}
        }
    }
}

fn drain_reader_output(
    raw_receiver: &Receiver<RawOutput>,
    shared: &SharedState,
    readers: &mut [ReaderThread],
    deadline: Instant,
) -> Result<(), ProcessError> {
    loop {
        match raw_receiver.recv_timeout(SUPERVISOR_POLL_INTERVAL) {
            Ok(raw) => sequence_output(raw, shared),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() >= deadline => {
                return Err(ProcessError::Platform {
                    operation: "finish paqet output readers",
                    source: io::Error::new(
                        io::ErrorKind::TimedOut,
                        "a paqet output pipe remained open after process-tree termination",
                    ),
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }

    let mut first_error = None;
    for reader in readers {
        let Some(handle) = reader.handle.take() else {
            continue;
        };
        let result = match handle.join() {
            Ok(Ok(())) => continue,
            Ok(Err(source)) => ProcessError::OutputReader {
                stream: reader.stream,
                source,
            },
            Err(_) => ProcessError::OutputReaderPanicked(reader.stream),
        };
        if first_error.is_none() {
            first_error = Some(result);
        }
    }
    first_error.map_or(Ok(()), Err)
}

struct PipePair {
    read: OwnedHandle,
    write: OwnedHandle,
}

impl PipePair {
    fn output() -> Result<Self, ProcessError> {
        let pair = create_inheritable_pipe()?;
        clear_handle_inheritance(&pair.read)?;
        Ok(pair)
    }

    fn input() -> Result<Self, ProcessError> {
        let pair = create_inheritable_pipe()?;
        clear_handle_inheritance(&pair.write)?;
        Ok(pair)
    }
}

struct AttributeList {
    storage: Vec<usize>,
    initialized: bool,
}

impl AttributeList {
    fn new(attribute_count: u32) -> Result<Self, ProcessError> {
        let mut required_bytes = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), attribute_count, 0, &mut required_bytes);
        }
        if required_bytes == 0 {
            return Err(platform_error("size the process attribute list"));
        }

        let words = required_bytes.div_ceil(size_of::<usize>());
        let mut list = Self {
            storage: vec![0; words],
            initialized: false,
        };
        if unsafe {
            InitializeProcThreadAttributeList(
                list.pointer(),
                attribute_count,
                0,
                &mut required_bytes,
            )
        } == 0
        {
            return Err(platform_error("initialize the process attribute list"));
        }
        list.initialized = true;
        Ok(list)
    }

    fn pointer(&mut self) -> *mut c_void {
        self.storage.as_mut_ptr().cast()
    }

    fn update(
        &mut self,
        attribute: usize,
        value: *const c_void,
        size: usize,
        operation: &'static str,
    ) -> Result<(), ProcessError> {
        if unsafe {
            UpdateProcThreadAttribute(
                self.pointer(),
                0,
                attribute,
                value,
                size,
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(platform_error(operation));
        }
        Ok(())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.pointer());
            }
        }
    }
}

fn validate_launch_paths(executable: &Path, config_path: &Path) -> Result<(), ProcessError> {
    validate_paqet_executable(executable)?;
    if !executable.is_absolute() {
        return Err(ProcessError::InvalidExecutable {
            path: executable.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "the path must be absolute"),
        });
    }
    if !config_path.is_absolute() {
        return Err(ProcessError::InvalidConfigPath(config_path.to_owned()));
    }
    reject_interior_null("executable", executable)?;
    reject_interior_null("configuration", config_path)
}

fn reject_interior_null(field: &'static str, path: &Path) -> Result<(), ProcessError> {
    if path.as_os_str().encode_wide().any(|unit| unit == 0) {
        return Err(ProcessError::InvalidWindowsPath {
            field,
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn create_supervised_process(
    executable: &Path,
    config_path: &Path,
) -> Result<CreatedProcess, ProcessError> {
    let job = create_job()?;
    let stdout = PipePair::output()?;
    let stderr = PipePair::output()?;
    let stdin = PipePair::input()?;

    let job_handles = [raw_handle(&job)];
    let inherited_handles = [
        raw_handle(&stdin.read),
        raw_handle(&stdout.write),
        raw_handle(&stderr.write),
    ];
    let mut attributes = AttributeList::new(2)?;
    attributes.update(
        PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
        job_handles.as_ptr().cast(),
        size_of_val(&job_handles),
        "assign the paqet process to its job at creation time",
    )?;
    attributes.update(
        PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
        inherited_handles.as_ptr().cast(),
        size_of_val(&inherited_handles),
        "restrict paqet handle inheritance",
    )?;

    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = inherited_handles[0];
    startup.StartupInfo.hStdOutput = inherited_handles[1];
    startup.StartupInfo.hStdError = inherited_handles[2];
    startup.lpAttributeList = attributes.pointer();

    let application = wide_null(executable.as_os_str());
    let mut command_line = encode_command_line(executable, config_path);
    let current_directory = executable
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| wide_null(parent.as_os_str()));
    let current_directory_pointer = current_directory
        .as_ref()
        .map_or(null(), |directory| directory.as_ptr());
    let mut process_information = MaybeUninit::<PROCESS_INFORMATION>::uninit();

    let created = unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_NO_WINDOW,
            null(),
            current_directory_pointer,
            &startup.StartupInfo,
            process_information.as_mut_ptr(),
        )
    };
    if created == 0 {
        return Err(platform_error("launch the supervised paqet process"));
    }

    let process_information = unsafe { process_information.assume_init() };
    let process = unsafe { owned_handle(process_information.hProcess) };
    let thread = unsafe { owned_handle(process_information.hThread) };
    drop(thread);
    drop(attributes);
    drop(stdin);
    drop(stdout.write);
    drop(stderr.write);

    Ok(CreatedProcess {
        job,
        process,
        process_id: process_information.dwProcessId,
        stdout: stdout.read,
        stderr: stderr.read,
    })
}

fn create_job() -> Result<OwnedHandle, ProcessError> {
    let raw = unsafe { CreateJobObjectW(null(), null()) };
    if raw.is_null() {
        return Err(platform_error("create the paqet Job Object"));
    }
    let job = unsafe { owned_handle(raw) };
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            raw_handle(&job),
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(platform_error("configure paqet Job Object cleanup"));
    }
    Ok(job)
}

fn create_inheritable_pipe() -> Result<PipePair, ProcessError> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let mut read = MaybeUninit::<HANDLE>::uninit();
    let mut write = MaybeUninit::<HANDLE>::uninit();
    if unsafe { CreatePipe(read.as_mut_ptr(), write.as_mut_ptr(), &attributes, 0) } == 0 {
        return Err(platform_error("create a paqet standard stream pipe"));
    }
    Ok(PipePair {
        read: unsafe { owned_handle(read.assume_init()) },
        write: unsafe { owned_handle(write.assume_init()) },
    })
}

fn clear_handle_inheritance(handle: &OwnedHandle) -> Result<(), ProcessError> {
    if unsafe { SetHandleInformation(raw_handle(handle), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(platform_error("restrict paqet pipe-handle inheritance"));
    }
    Ok(())
}

fn spawn_reader(
    stream: OutputStream,
    handle: OwnedHandle,
    sender: SyncSender<RawOutput>,
) -> Result<ReaderThread, ProcessError> {
    let handle = thread::Builder::new()
        .name(format!("paqet-{stream}-reader"))
        .spawn(move || read_lines(stream, handle, sender))
        .map_err(|source| ProcessError::Platform {
            operation: "start a paqet output reader",
            source,
        })?;
    Ok(ReaderThread {
        stream,
        handle: Some(handle),
    })
}

fn join_reader_after_launch_failure(mut reader: ReaderThread) {
    if let Some(handle) = reader.handle.take() {
        let _ = handle.join();
    }
}

fn read_lines(
    stream: OutputStream,
    handle: OwnedHandle,
    sender: SyncSender<RawOutput>,
) -> io::Result<()> {
    let mut reader = BufReader::new(std::fs::File::from(handle));
    while let Some((bytes, truncated)) = read_bounded_line(&mut reader)? {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if sender
            .send(RawOutput {
                stream,
                text,
                truncated,
            })
            .is_err()
        {
            return Ok(());
        }
    }
    Ok(())
}

fn read_bounded_line<R: BufRead>(reader: &mut R) -> io::Result<Option<(Vec<u8>, bool)>> {
    let mut retained = Vec::with_capacity(MAX_LOG_RECORD_BYTES);
    let mut saw_bytes = false;
    let mut truncated = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(saw_bytes.then_some((retained, truncated)));
        }
        saw_bytes = true;
        let consumed = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |index| index + 1);
        let remaining = MAX_LOG_RECORD_BYTES.saturating_sub(retained.len());
        let copied = consumed.min(remaining);
        retained.extend_from_slice(&available[..copied]);
        truncated |= copied < consumed;
        let found_newline = available[..consumed].ends_with(b"\n");
        reader.consume(consumed);
        if found_newline {
            return Ok(Some((retained, truncated)));
        }
    }
}

fn terminate_job(job: &OwnedHandle) -> Result<(), ProcessError> {
    if unsafe { TerminateJobObject(raw_handle(job), DISCONNECT_EXIT_CODE) } == 0 {
        return Err(platform_error("terminate the paqet process tree"));
    }
    Ok(())
}

fn process_has_exited(process: &OwnedHandle) -> Result<bool, ProcessError> {
    match unsafe { WaitForSingleObject(raw_handle(process), 0) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        WAIT_FAILED => Err(platform_error("observe the paqet root process")),
        result => Err(ProcessError::Platform {
            operation: "observe the paqet root process",
            source: io::Error::other(format!("unexpected wait result {result}")),
        }),
    }
}

fn process_exit_code(process: &OwnedHandle) -> Result<u32, ProcessError> {
    let mut code = 0;
    if unsafe { GetExitCodeProcess(raw_handle(process), &mut code) } == 0 {
        return Err(platform_error("read the paqet process exit code"));
    }
    Ok(code)
}

fn active_process_count(job: &OwnedHandle) -> Result<u32, ProcessError> {
    let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        QueryInformationJobObject(
            raw_handle(job),
            JobObjectBasicAccountingInformation,
            (&raw mut accounting).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
    {
        return Err(platform_error("query the paqet Job Object"));
    }
    Ok(accounting.ActiveProcesses)
}

fn encode_command_line(executable: &Path, config_path: &Path) -> Vec<u16> {
    let arguments = [
        executable.as_os_str(),
        OsStr::new(PAQET_RUN_SUBCOMMAND),
        OsStr::new(PAQET_CONFIG_FLAG),
        config_path.as_os_str(),
    ];
    let mut command = Vec::new();
    for (index, argument) in arguments.into_iter().enumerate() {
        if index != 0 {
            command.push(' ' as u16);
        }
        append_quoted_argument(&mut command, argument);
    }
    command.push(0);
    command
}

fn append_quoted_argument(command: &mut Vec<u16>, argument: &OsStr) {
    let value: Vec<u16> = argument.encode_wide().collect();
    let needs_quotes = value.is_empty()
        || value
            .iter()
            .any(|character| matches!(*character, 0x20 | 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x22));
    if !needs_quotes {
        command.extend(value);
        return;
    }

    command.push('"' as u16);
    let mut backslashes = 0;
    for character in value {
        if character == '\\' as u16 {
            backslashes += 1;
        } else {
            if character == '"' as u16 {
                command.extend(std::iter::repeat_n('\\' as u16, backslashes + 1));
            }
            command.extend(std::iter::repeat_n('\\' as u16, backslashes));
            backslashes = 0;
            command.push(character);
        }
    }
    command.extend(std::iter::repeat_n('\\' as u16, backslashes * 2));
    command.push('"' as u16);
}

fn wide_null(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn raw_handle(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle().cast()
}

unsafe fn owned_handle(handle: HANDLE) -> OwnedHandle {
    unsafe { OwnedHandle::from_raw_handle(handle.cast()) }
}

fn platform_error(operation: &'static str) -> ProcessError {
    ProcessError::Platform {
        operation,
        source: io::Error::last_os_error(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_quotes_spaces_quotes_and_trailing_backslashes() {
        let command = encode_command_line(
            Path::new(r"C:\Program Files\paqet\paqet.exe"),
            Path::new("C:\\Users\\Test User\\quoted\" folder\\\\"),
        );
        let decoded = String::from_utf16(&command[..command.len() - 1]).unwrap();
        assert_eq!(
            decoded,
            r#""C:\Program Files\paqet\paqet.exe" run -c "C:\Users\Test User\quoted\" folder\\\\""#
        );
    }

    #[test]
    fn bounded_line_reader_discards_oversized_remainder() {
        let input = format!("{}tail\nnext\n", "x".repeat(MAX_LOG_RECORD_BYTES));
        let mut reader = BufReader::new(input.as_bytes());
        let (first, truncated) = read_bounded_line(&mut reader).unwrap().unwrap();
        assert_eq!(first.len(), MAX_LOG_RECORD_BYTES);
        assert!(truncated);
        let (second, truncated) = read_bounded_line(&mut reader).unwrap().unwrap();
        assert_eq!(second, b"next\n");
        assert!(!truncated);
        assert!(read_bounded_line(&mut reader).unwrap().is_none());
    }
}
