#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
mod windows_test {
    use std::{
        env,
        ffi::OsString,
        fs,
        io::{self, Write},
        os::windows::fs::symlink_file,
        os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
        path::{Path, PathBuf},
        process::{Command, ExitCode, Stdio},
        sync::atomic::{AtomicU64, Ordering},
        thread,
        time::{Duration, Instant},
    };

    use paqet_gui_lib::process::{
        LogClassification, OutputStream, PAQET_EXECUTABLE_SHA256, PAQET_EXECUTABLE_SIZE,
        ProcessError, SupervisedPaqet, SupervisorEvent, validate_paqet_executable,
        validate_pinned_paqet_executable,
    };
    use windows_sys::Win32::{
        Foundation::{GetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Security::SECURITY_ATTRIBUTES,
        Storage::FileSystem::SYNCHRONIZE,
        System::Threading::{
            CreateEventW, GetCurrentProcess, GetProcessHandleCount, OpenProcess,
            PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
        },
    };

    const ROOT_ARGUMENTS: [&str; 2] = ["run", "-c"];
    const DESCENDANT_ARGUMENT: &str = "--core005-descendant";
    const READY_TIMEOUT: Duration = Duration::from_secs(20);
    const PROCESS_EXIT_TIMEOUT: Duration = Duration::from_secs(10);
    const ROOT_EXIT_CODE: i32 = 37;
    static NEXT_DIRECTORY_ID: AtomicU64 = AtomicU64::new(1);

    pub fn main() -> ExitCode {
        let arguments: Vec<OsString> = env::args_os().skip(1).collect();
        if arguments.first().is_some_and(|value| value == "run") {
            return root_helper(&arguments);
        }
        if arguments
            .first()
            .is_some_and(|value| value == DESCENDANT_ARGUMENT)
        {
            return descendant_helper(&arguments);
        }

        match run_suite() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("CORE-005 integration failure: {error}");
                ExitCode::FAILURE
            }
        }
    }

    fn run_suite() -> Result<(), String> {
        disconnect_terminates_complete_tree_and_preserves_output_order()?;
        unexpected_root_exit_is_observed_and_descendants_are_reaped()?;
        wait_preserves_ordered_terminal_event()?;
        slow_consumers_do_not_block_output_or_exceed_retention()?;
        dropping_live_supervisor_terminates_complete_tree()?;
        excludes_unlisted_inheritable_handles()?;
        launches_from_unicode_path_with_spaces()?;
        contains_descendant_created_as_root_first_action()?;
        repeated_supervision_does_not_leak_handles()?;
        launch_failure_rolls_back_native_handles()?;
        rejects_executable_with_wrong_pinned_identity()?;
        rejects_final_component_executable_reparse_point()?;
        Ok(())
    }

    fn disconnect_terminates_complete_tree_and_preserves_output_order() -> Result<(), String> {
        let test = ProcessTest::launch("ordered")?;
        let mut supervisor = test.supervisor;
        let (root, descendant) = collect_ready_process_tree(&test.directory, &mut supervisor)?;

        if supervisor.root_process_id() != read_pid(&test.directory.path().join("root.pid"))? {
            return Err("reported root PID did not match the launched process".to_owned());
        }
        assert_process_running(&root, "root")?;
        assert_process_running(&descendant, "descendant")?;

        let exit = supervisor.disconnect().map_err(|error| error.to_string())?;
        if !exit.requested {
            return Err("disconnect was not reported as requested".to_owned());
        }
        assert_process_exited(&root, "root")?;
        assert_process_exited(&descendant, "descendant")?;
        if !matches!(supervisor.disconnect(), Err(ProcessError::AlreadyFinished)) {
            return Err("a completed supervisor accepted a second disconnect".to_owned());
        }
        Ok(())
    }

    fn unexpected_root_exit_is_observed_and_descendants_are_reaped() -> Result<(), String> {
        let test = ProcessTest::launch("ordered")?;
        let mut supervisor = test.supervisor;
        let (root, descendant) = collect_ready_process_tree(&test.directory, &mut supervisor)?;
        fs::write(test.directory.path().join("exit-root"), b"exit")
            .map_err(|error| error.to_string())?;

        assert_process_exited(&root, "root")?;
        assert_process_exited(&descendant, "descendant")?;
        let event = supervisor
            .next_event_timeout(Duration::from_secs(1))
            .map_err(|error| error.to_string())?;
        if event
            != Some(SupervisorEvent::Exited(
                paqet_gui_lib::process::ProcessTreeExit {
                    code: ROOT_EXIT_CODE,
                    requested: false,
                },
            ))
        {
            return Err(format!("terminal event was not ordered last: {event:?}"));
        }
        let exit = supervisor.wait().map_err(|error| error.to_string())?;
        if exit.requested || exit.code != ROOT_EXIT_CODE {
            return Err(format!("unexpected natural-exit observation: {exit:?}"));
        }
        Ok(())
    }

    fn wait_preserves_ordered_terminal_event() -> Result<(), String> {
        let test = ProcessTest::launch("natural-exit")?;
        let mut supervisor = test.supervisor;
        let exit = supervisor.wait().map_err(|error| error.to_string())?;
        if exit.requested || exit.code != ROOT_EXIT_CODE {
            return Err(format!("unexpected wait result: {exit:?}"));
        }
        match supervisor
            .next_event_timeout(Duration::ZERO)
            .map_err(|error| error.to_string())?
        {
            Some(SupervisorEvent::Output(record)) if record.text == "core005:natural-exit" => {}
            event => {
                return Err(format!(
                    "expected retained output after wait, got {event:?}"
                ));
            }
        }
        if supervisor
            .next_event_timeout(Duration::ZERO)
            .map_err(|error| error.to_string())?
            != Some(SupervisorEvent::Exited(exit))
        {
            return Err("wait did not preserve the ordered terminal event".to_owned());
        }
        Ok(())
    }

    fn slow_consumers_do_not_block_output_or_exceed_retention() -> Result<(), String> {
        let test = ProcessTest::launch("flood")?;
        let mut supervisor = test.supervisor;
        wait_for_file(&test.directory.path().join("ready"))?;
        let deadline = Instant::now() + READY_TIMEOUT;
        while !supervisor
            .records()
            .iter()
            .any(|record| record.text == "core005:flood:final")
        {
            if Instant::now() >= deadline {
                return Err("final flood sentinel was not sequenced".to_owned());
            }
            thread::sleep(Duration::from_millis(20));
        }
        let records = supervisor.records();
        if records.len() > 2_000 {
            return Err(format!(
                "retained {} records after output flood",
                records.len()
            ));
        }
        if records.first().is_none_or(|record| record.sequence <= 1)
            || records
                .last()
                .is_none_or(|record| record.text != "core005:flood:final")
        {
            return Err(
                "flood retention did not evict oldest records and keep final output".to_owned(),
            );
        }
        match supervisor
            .next_event_timeout(Duration::from_secs(1))
            .map_err(|error| error.to_string())?
        {
            Some(SupervisorEvent::Gap {
                first_missing: 1,
                classification: Some(LogClassification::Connected),
                ..
            }) => {}
            event => return Err(format!("expected an explicit replay gap, got {event:?}")),
        }
        supervisor.disconnect().map_err(|error| error.to_string())?;
        Ok(())
    }

    fn dropping_live_supervisor_terminates_complete_tree() -> Result<(), String> {
        let test = ProcessTest::launch("quiet")?;
        wait_for_file(&test.directory.path().join("ready"))?;
        let root = open_process(read_pid(&test.directory.path().join("root.pid"))?)?;
        let descendant = open_process(read_pid(&test.directory.path().join("descendant.pid"))?)?;
        let started = Instant::now();
        drop(test.supervisor);
        if started.elapsed() > PROCESS_EXIT_TIMEOUT {
            return Err("dropping the supervisor exceeded its cleanup deadline".to_owned());
        }
        assert_process_exited(&root, "drop root")?;
        assert_process_exited(&descendant, "drop descendant")
    }

    fn excludes_unlisted_inheritable_handles() -> Result<(), String> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: std::ptr::null_mut(),
            bInheritHandle: 1,
        };
        let raw = unsafe { CreateEventW(&attributes, 1, 0, std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error().to_string());
        }
        let sentinel = unsafe { OwnedHandle::from_raw_handle(raw.cast()) };
        let mode = format!("sentinel:{}", sentinel.as_raw_handle() as usize);
        let test = ProcessTest::launch_with_executable(
            &mode,
            &env::current_exe().map_err(|e| e.to_string())?,
        )?;
        wait_for_file(&test.directory.path().join("sentinel-not-inherited"))?;
        drop(test.supervisor);
        Ok(())
    }

    fn launches_from_unicode_path_with_spaces() -> Result<(), String> {
        let directory = TestDirectory::new()?;
        let executable = directory.path().join("helper ü with spaces.exe");
        fs::copy(
            env::current_exe().map_err(|error| error.to_string())?,
            &executable,
        )
        .map_err(|error| error.to_string())?;
        let test = ProcessTest::launch_with_executable("quiet", &executable)?;
        wait_for_file(&test.directory.path().join("ready"))?;
        drop(test.supervisor);
        Ok(())
    }

    fn contains_descendant_created_as_root_first_action() -> Result<(), String> {
        for _ in 0..4 {
            let test = ProcessTest::launch("immediate-descendant")?;
            wait_for_file(&test.directory.path().join("ready"))?;
            let descendant =
                open_process(read_pid(&test.directory.path().join("descendant.pid"))?)?;
            let mut supervisor = test.supervisor;
            supervisor.disconnect().map_err(|error| error.to_string())?;
            assert_process_exited(&descendant, "immediate descendant")?;
        }
        Ok(())
    }

    fn repeated_supervision_does_not_leak_handles() -> Result<(), String> {
        run_quiet_cycle()?;
        let baseline = process_handle_count()?;
        for _ in 0..8 {
            run_quiet_cycle()?;
        }
        let final_count = process_handle_count()?;
        if final_count != baseline {
            return Err(format!(
                "supervision leaked process handles: baseline {baseline}, final {final_count}"
            ));
        }
        Ok(())
    }

    fn launch_failure_rolls_back_native_handles() -> Result<(), String> {
        let config = Path::new("relative-config.yaml");
        expect_launch_failure(config)?;
        let baseline = process_handle_count()?;
        for _ in 0..8 {
            expect_launch_failure(config)?;
        }
        let final_count = process_handle_count()?;
        if final_count != baseline {
            return Err(format!(
                "failed launch leaked process handles: baseline {baseline}, final {final_count}"
            ));
        }
        Ok(())
    }

    fn rejects_executable_with_wrong_pinned_identity() -> Result<(), String> {
        let directory = TestDirectory::new()?;
        let executable = directory.path().join("wrong-identity.exe");
        fs::File::create(&executable)
            .and_then(|file| file.set_len(PAQET_EXECUTABLE_SIZE))
            .map_err(|error| error.to_string())?;
        match validate_pinned_paqet_executable(&executable) {
            Err(ProcessError::ExecutableIdentityMismatch {
                path,
                actual_size,
                actual_sha256,
                ..
            }) if path == executable
                && actual_size == PAQET_EXECUTABLE_SIZE
                && actual_sha256 != PAQET_EXECUTABLE_SHA256
                && !actual_sha256.starts_with("not calculated") =>
            {
                Ok(())
            }
            result => Err(format!(
                "wrong executable identity was not rejected precisely: {result:?}"
            )),
        }
    }

    fn rejects_final_component_executable_reparse_point() -> Result<(), String> {
        let directory = TestDirectory::new()?;
        let target = env::current_exe().map_err(|error| error.to_string())?;
        let executable = directory.path().join("helper-link.exe");
        if let Err(error) = symlink_file(&target, &executable) {
            if error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(1314)
            {
                return Ok(());
            }
            return Err(format!("failed to create executable symlink: {error}"));
        }

        if !matches!(
            validate_paqet_executable(&executable),
            Err(ProcessError::ExecutableIsNotFile(path)) if path == executable
        ) {
            return Err("launch validation accepted an executable reparse point".to_owned());
        }
        if !matches!(
            validate_pinned_paqet_executable(&executable),
            Err(ProcessError::ExecutableIsNotFile(path)) if path == executable
        ) {
            return Err("pinned validation accepted an executable reparse point".to_owned());
        }
        Ok(())
    }

    fn expect_launch_failure(config: &Path) -> Result<(), String> {
        let executable = env::current_exe().map_err(|error| error.to_string())?;
        if SupervisedPaqet::launch_test_executable(&executable, config).is_ok() {
            return Err("an invalid launch contract unexpectedly launched".to_owned());
        }
        Ok(())
    }

    fn run_quiet_cycle() -> Result<(), String> {
        let test = ProcessTest::launch("quiet")?;
        let mut supervisor = test.supervisor;
        wait_for_file(&test.directory.path().join("ready"))?;
        supervisor.disconnect().map_err(|error| error.to_string())?;
        Ok(())
    }

    fn collect_ready_process_tree(
        directory: &TestDirectory,
        supervisor: &mut SupervisedPaqet,
    ) -> Result<(OwnedHandle, OwnedHandle), String> {
        let expected = [
            (OutputStream::Stdout, "core005:1:root:stdout"),
            (OutputStream::Stderr, "core005:2:root:stderr"),
            (OutputStream::Stdout, "core005:3:descendant:stdout"),
            (OutputStream::Stderr, "core005:4:descendant:stderr"),
        ];
        let deadline = Instant::now() + READY_TIMEOUT;
        for (index, (stream, text)) in expected.into_iter().enumerate() {
            let record = loop {
                if Instant::now() >= deadline {
                    return Err(format!("timed out waiting for output record {}", index + 1));
                }
                if let Some(event) = supervisor
                    .next_event_timeout(Duration::from_millis(100))
                    .map_err(|error| error.to_string())?
                {
                    match event {
                        SupervisorEvent::Output(record) => break record,
                        event => return Err(format!("unexpected event before output: {event:?}")),
                    }
                }
            };
            if record.sequence != index as u64 + 1 || record.stream != stream || record.text != text
            {
                return Err(format!("unexpected ordered record {record:?}"));
            }
            fs::write(directory.path().join(format!("ack-{}", index + 1)), b"ack")
                .map_err(|error| error.to_string())?;
        }

        wait_for_file(&directory.path().join("ready"))?;
        let root = open_process(read_pid(&directory.path().join("root.pid"))?)?;
        let descendant = open_process(read_pid(&directory.path().join("descendant.pid"))?)?;
        Ok((root, descendant))
    }

    struct ProcessTest {
        directory: TestDirectory,
        supervisor: SupervisedPaqet,
    }

    impl ProcessTest {
        fn launch(mode: &str) -> Result<Self, String> {
            Self::launch_with_executable(
                mode,
                &env::current_exe().map_err(|error| error.to_string())?,
            )
        }

        fn launch_with_executable(mode: &str, executable: &Path) -> Result<Self, String> {
            let directory = TestDirectory::new()?;
            let config_path = directory.path().join("test plan with spaces.yaml");
            fs::write(&config_path, mode).map_err(|error| error.to_string())?;
            let supervisor = SupervisedPaqet::launch_test_executable(executable, &config_path)
                .map_err(|error| error.to_string())?;
            Ok(Self {
                directory,
                supervisor,
            })
        }
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Result<Self, String> {
            let id = NEXT_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
            let directory =
                env::temp_dir().join(format!("paqet-gui-core005-{}-{id}", std::process::id()));
            fs::create_dir(&directory).map_err(|error| error.to_string())?;
            Ok(Self(directory))
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn root_helper(arguments: &[OsString]) -> ExitCode {
        if arguments.len() != 3
            || arguments[0] != ROOT_ARGUMENTS[0]
            || arguments[1] != ROOT_ARGUMENTS[1]
        {
            return ExitCode::FAILURE;
        }
        let config = PathBuf::from(&arguments[2]);
        let Some(directory) = config.parent() else {
            return ExitCode::FAILURE;
        };
        let mode = fs::read_to_string(&config).unwrap_or_default();
        let mut descendant = if mode == "immediate-descendant" {
            match spawn_descendant(&config) {
                Ok(child) => Some(child),
                Err(_) => return ExitCode::FAILURE,
            }
        } else {
            None
        };
        if let Some(value) = mode.strip_prefix("sentinel:") {
            let raw = value.parse::<usize>().unwrap_or_default() as *mut std::ffi::c_void;
            let mut flags = 0;
            if unsafe { GetHandleInformation(raw, &mut flags) } != 0 {
                return ExitCode::FAILURE;
            }
            if fs::write(directory.join("sentinel-not-inherited"), b"ok").is_err() {
                return ExitCode::FAILURE;
            }
        }
        if fs::write(directory.join("root.pid"), std::process::id().to_string()).is_err() {
            return ExitCode::FAILURE;
        }

        if mode == "ordered"
            && (emit_and_wait(directory, 1, OutputStream::Stdout).is_err()
                || emit_and_wait(directory, 2, OutputStream::Stderr).is_err())
        {
            return ExitCode::FAILURE;
        }
        if mode == "flood" {
            println!("Client started: flood lifecycle marker");
            for sequence in 0..2_500 {
                println!("core005:flood:{sequence}");
            }
            println!("core005:flood:final");
            if io::stdout().flush().is_err() {
                return ExitCode::FAILURE;
            }
        }

        let mut descendant = match descendant
            .take()
            .map_or_else(|| spawn_descendant(&config), Ok)
        {
            Ok(child) => child,
            Err(_) => return ExitCode::FAILURE,
        };
        if mode == "natural-exit" {
            if wait_for_file(&directory.join("ready")).is_err() {
                return ExitCode::FAILURE;
            }
            println!("core005:natural-exit");
            if io::stdout().flush().is_err() {
                return ExitCode::FAILURE;
            }
            return ExitCode::from(ROOT_EXIT_CODE as u8);
        }
        loop {
            if directory.join("exit-root").is_file() {
                return ExitCode::from(ROOT_EXIT_CODE as u8);
            }
            match descendant.try_wait() {
                Ok(Some(_)) | Err(_) => return ExitCode::FAILURE,
                Ok(None) => thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    fn spawn_descendant(config: &Path) -> io::Result<std::process::Child> {
        Command::new(env::current_exe()?)
            .arg(DESCENDANT_ARGUMENT)
            .arg(config)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
    }

    fn descendant_helper(arguments: &[OsString]) -> ExitCode {
        if arguments.len() != 2 {
            return ExitCode::FAILURE;
        }
        let config = PathBuf::from(&arguments[1]);
        let Some(directory) = config.parent() else {
            return ExitCode::FAILURE;
        };
        let mode = fs::read_to_string(&config).unwrap_or_default();
        if fs::write(
            directory.join("descendant.pid"),
            std::process::id().to_string(),
        )
        .is_err()
        {
            return ExitCode::FAILURE;
        }
        if mode == "ordered"
            && (emit_and_wait(directory, 3, OutputStream::Stdout).is_err()
                || emit_and_wait(directory, 4, OutputStream::Stderr).is_err())
        {
            return ExitCode::FAILURE;
        }
        if fs::write(directory.join("ready"), b"ready").is_err() {
            return ExitCode::FAILURE;
        }
        loop {
            thread::sleep(Duration::from_secs(1));
        }
    }

    fn emit_and_wait(directory: &Path, sequence: u64, stream: OutputStream) -> io::Result<()> {
        match stream {
            OutputStream::Stdout => {
                println!("core005:{sequence}:{}:stdout", process_name(sequence));
                io::stdout().flush()?;
            }
            OutputStream::Stderr => {
                eprintln!("core005:{sequence}:{}:stderr", process_name(sequence));
                io::stderr().flush()?;
            }
        }
        wait_for_file(&directory.join(format!("ack-{sequence}"))).map_err(io::Error::other)
    }

    fn process_name(sequence: u64) -> &'static str {
        if sequence <= 2 { "root" } else { "descendant" }
    }

    fn wait_for_file(path: &Path) -> Result<(), String> {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if path.is_file() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(20));
        }
        Err(format!("timed out waiting for {}", path.display()))
    }

    fn read_pid(path: &Path) -> Result<u32, String> {
        fs::read_to_string(path)
            .map_err(|error| error.to_string())?
            .parse()
            .map_err(|error| format!("invalid PID in {}: {error}", path.display()))
    }

    fn open_process(process_id: u32) -> Result<OwnedHandle, String> {
        let handle = unsafe {
            OpenProcess(
                SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                process_id,
            )
        };
        if handle.is_null() {
            return Err(format!(
                "failed to open process {process_id}: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(unsafe { OwnedHandle::from_raw_handle(handle.cast()) })
    }

    fn assert_process_running(process: &OwnedHandle, name: &str) -> Result<(), String> {
        match unsafe { WaitForSingleObject(process.as_raw_handle().cast(), 0) } {
            WAIT_TIMEOUT => Ok(()),
            result => Err(format!("{name} was not running; wait returned {result}")),
        }
    }

    fn assert_process_exited(process: &OwnedHandle, name: &str) -> Result<(), String> {
        match unsafe {
            WaitForSingleObject(
                process.as_raw_handle().cast(),
                PROCESS_EXIT_TIMEOUT.as_millis() as u32,
            )
        } {
            WAIT_OBJECT_0 => Ok(()),
            result => Err(format!("{name} did not exit; wait returned {result}")),
        }
    }

    fn process_handle_count() -> Result<u32, String> {
        let mut count = 0;
        if unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) } == 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        Ok(count)
    }
}

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    windows_test::main()
}
