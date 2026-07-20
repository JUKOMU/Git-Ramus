//! A bounded, shell-free process runner for Git.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, RecvTimeoutError},
};
use std::thread;
use std::time::{Duration, Instant};

use crate::error::AppError;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
pub const DEFAULT_MAX_STDOUT_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_STDERR_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_AUTHENTICATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const DEFAULT_NETWORK_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct GitCommand {
    pub repo: PathBuf,
    pub args: Vec<OsString>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct GitOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitExecutionPolicy {
    LocalNonInteractive,
    ForegroundNetworkInteractive,
    BackgroundNetworkNonInteractive,
}

pub trait GitProgressSink: Send + Sync {
    /// Returns true only when the chunk produced a recognized transfer-progress event.
    fn stderr_chunk(&self, chunk: &[u8]) -> bool;
}

#[derive(Clone)]
pub struct GitRunContext {
    pub policy: GitExecutionPolicy,
    pub cancellation: Arc<AtomicBool>,
    pub progress: Option<Arc<dyn GitProgressSink>>,
    network_timeouts: Option<NetworkTimeouts>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NetworkTimeouts {
    authentication: Duration,
    idle: Duration,
}

impl std::fmt::Debug for GitRunContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitRunContext")
            .field("policy", &self.policy)
            .field("cancelled", &self.is_cancelled())
            .field("progress_configured", &self.progress.is_some())
            .field("network_timeouts", &self.network_timeouts)
            .finish()
    }
}

impl GitRunContext {
    pub fn new(policy: GitExecutionPolicy) -> Self {
        let network_timeouts =
            (policy != GitExecutionPolicy::LocalNonInteractive).then_some(NetworkTimeouts {
                authentication: DEFAULT_AUTHENTICATION_TIMEOUT,
                idle: DEFAULT_NETWORK_IDLE_TIMEOUT,
            });
        Self {
            policy,
            cancellation: Arc::new(AtomicBool::new(false)),
            progress: None,
            network_timeouts,
        }
    }

    pub fn with_progress(mut self, progress: Arc<dyn GitProgressSink>) -> Self {
        self.progress = Some(progress);
        self
    }

    #[cfg(test)]
    fn with_network_timeouts(mut self, authentication: Duration, idle: Duration) -> Self {
        self.network_timeouts = Some(NetworkTimeouts {
            authentication,
            idle,
        });
        self
    }

    pub fn cancel(&self) {
        self.cancellation.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.load(Ordering::Acquire)
    }
}

pub trait GitRunner: Send + Sync {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError>;

    fn run_with_context(
        &self,
        command: GitCommand,
        context: GitRunContext,
    ) -> Result<GitOutput, AppError> {
        if context.is_cancelled() {
            return Err(AppError::Canceled);
        }
        self.run(command)
    }
}

#[derive(Debug, Clone)]
pub struct SystemGitRunner {
    max_stdout_bytes: usize,
    max_stderr_bytes: usize,
    poll_interval: Duration,
    sealed_config: Option<SealedGitConfig>,
}

#[derive(Debug, Clone)]
struct SealedGitConfig {
    home: PathBuf,
    xdg_config_home: PathBuf,
    global_config: PathBuf,
}

const IO_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(windows)]
const CLEANUP_POLL_INTERVAL: Duration = Duration::from_millis(10);
type StdinWriter = (Receiver<io::Result<()>>, Arc<Mutex<Option<ChildStdin>>>);

#[derive(Default)]
struct NetworkProgressActivity {
    last_progress: Mutex<Option<Instant>>,
}

impl NetworkProgressActivity {
    fn mark(&self) {
        if let Ok(mut last_progress) = self.last_progress.lock() {
            *last_progress = Some(Instant::now());
        }
    }

    fn timed_out(&self, started: Instant, timeouts: NetworkTimeouts) -> bool {
        match self.last_progress.lock().ok().and_then(|value| *value) {
            Some(last_progress) => last_progress.elapsed() >= timeouts.idle,
            None => started.elapsed() >= timeouts.authentication,
        }
    }
}

enum ProcessTreeKind {
    #[cfg(windows)]
    Windows(WindowsJob),
    #[cfg(unix)]
    Unix { pgid: libc::pid_t },
    #[cfg(not(any(unix, windows)))]
    Other,
}

struct ProcessTree {
    kind: ProcessTreeKind,
    reaped: AtomicBool,
}

impl ProcessTree {
    fn configure(command: &mut Command) -> Result<(), AppError> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(any(windows, not(any(unix, windows))))]
        {
            let _ = command;
        }
        Ok(())
    }

    fn attach(child: &Child) -> Result<Self, AppError> {
        #[cfg(windows)]
        {
            Ok(Self {
                kind: ProcessTreeKind::Windows(WindowsJob::attach(child)?),
                reaped: AtomicBool::new(false),
            })
        }
        #[cfg(unix)]
        {
            Ok(Self {
                kind: ProcessTreeKind::Unix {
                    pgid: child.id() as libc::pid_t,
                },
                reaped: AtomicBool::new(false),
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Ok(Self {
                kind: ProcessTreeKind::Other,
                reaped: AtomicBool::new(false),
            })
        }
    }

    fn terminate(&self) -> Result<(), AppError> {
        if self.reaped.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = match &self.kind {
            #[cfg(windows)]
            ProcessTreeKind::Windows(job) => job.terminate(),
            #[cfg(unix)]
            ProcessTreeKind::Unix { pgid } => {
                let result = unsafe { libc::kill(-*pgid, libc::SIGKILL) };
                if result == -1 {
                    let error = io::Error::last_os_error();
                    if error.raw_os_error() != Some(libc::ESRCH) {
                        return Err(AppError::Git(
                            "unable to terminate Git process group".to_owned(),
                        ));
                    }
                }
                Ok(())
            }
            #[cfg(not(any(unix, windows)))]
            ProcessTreeKind::Other => Ok(()),
        };
        if result.is_err() {
            self.reaped.store(false, Ordering::Release);
        }
        result
    }

    fn finish_after_reap(&self) -> Result<(), AppError> {
        self.reaped.store(true, Ordering::Release);
        Ok(())
    }

    #[cfg(windows)]
    fn fallback_command_for_pid(pid: u32) -> Result<Command, AppError> {
        use std::os::windows::process::CommandExt;
        use std::path::PathBuf;
        let root = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let program = root.join("System32").join("taskkill.exe");
        let mut command = Command::new(program);
        command
            .env_clear()
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .args(["/PID", &pid.to_string(), "/T", "/F"]);
        Ok(command)
    }

    #[cfg(windows)]
    fn fallback_terminate_pid(pid: u32) -> Result<(), AppError> {
        let mut child = Self::fallback_command_for_pid(pid)?
            .spawn()
            .map_err(|_| AppError::Git("unable to invoke process cleanup".to_owned()))?;
        let status = wait_cleanup_process(&mut child, IO_CLEANUP_TIMEOUT, CLEANUP_POLL_INTERVAL)?;
        if status.success() || status.code() == Some(128) {
            Ok(())
        } else {
            Err(AppError::Git("process cleanup failed".to_owned()))
        }
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}

#[cfg(windows)]
struct WindowsJob {
    handle: windows_sys::Win32::Foundation::HANDLE,
    pid: u32,
}

#[cfg(windows)]
impl WindowsJob {
    fn attach(child: &Child) -> Result<Self, AppError> {
        use std::mem::size_of;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            let _ = ProcessTree::fallback_terminate_pid(child.id());
            return Err(AppError::Git("unable to create Git job".to_owned()));
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if configured == 0 {
            let _ = ProcessTree::fallback_terminate_pid(child.id());
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
            return Err(AppError::Git("unable to configure Git job".to_owned()));
        }
        let assigned = unsafe {
            AssignProcessToJobObject(
                handle,
                child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE,
            )
        };
        if assigned == 0 {
            let _ = ProcessTree::fallback_terminate_pid(child.id());
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
            return Err(AppError::Git(
                "unable to assign Git process to job".to_owned(),
            ));
        }
        Ok(Self {
            handle,
            pid: child.id(),
        })
    }

    fn terminate(&self) -> Result<(), AppError> {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        let terminated = unsafe { TerminateJobObject(self.handle, 1) };
        if terminated == 0 {
            return ProcessTree::fallback_terminate_pid(self.pid);
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

struct ChildGuard {
    child: Option<Child>,
    tree: ProcessTree,
    reaped: bool,
}

impl ChildGuard {
    fn new(child: Child, tree: ProcessTree) -> Self {
        Self {
            child: Some(child),
            tree,
            reaped: false,
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child guard must own a process")
    }

    fn mark_reaped(&mut self) -> Result<(), AppError> {
        let result = self.tree.finish_after_reap();
        if result.is_ok() {
            self.reaped = true;
        }
        result
    }

    fn terminate(&mut self) {
        if !self.reaped {
            let mut tree_ok = self.tree.terminate().is_ok();
            let child_ok = if let Some(child) = self.child.as_mut() {
                terminate_and_reap(child).is_ok()
            } else {
                true
            };
            if !tree_ok {
                tree_ok = self.tree.terminate().is_ok();
            }
            self.reaped = tree_ok && child_ok;
        }
    }

    fn disarm(mut self) {
        self.reaped = true;
        let _ = self.child.take();
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

impl Default for SystemGitRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemGitRunner {
    pub fn new() -> Self {
        Self {
            max_stdout_bytes: DEFAULT_MAX_STDOUT_BYTES,
            max_stderr_bytes: DEFAULT_MAX_STDERR_BYTES,
            poll_interval: Duration::from_millis(10),
            sealed_config: None,
        }
    }

    pub fn with_sealed_config(
        mut self,
        home: PathBuf,
        xdg_config_home: PathBuf,
        global_config: PathBuf,
    ) -> Self {
        self.sealed_config = Some(SealedGitConfig {
            home,
            xdg_config_home,
            global_config,
        });
        self
    }

    pub fn with_output_limits(max_stdout_bytes: usize, max_stderr_bytes: usize) -> Self {
        Self {
            max_stdout_bytes,
            max_stderr_bytes,
            ..Self::new()
        }
    }

    /// Alias with a concise name for callers configuring both stream limits.
    pub fn with_limits(max_stdout_bytes: usize, max_stderr_bytes: usize) -> Self {
        Self::with_output_limits(max_stdout_bytes, max_stderr_bytes)
    }

    pub fn max_stdout_bytes(&self) -> usize {
        self.max_stdout_bytes
    }

    pub fn max_stderr_bytes(&self) -> usize {
        self.max_stderr_bytes
    }

    fn command(&self, request: &GitCommand, policy: GitExecutionPolicy) -> Command {
        let mut command = Command::new("git");
        // `current_dir` is deliberately used instead of composing a shell command. Every
        // caller-supplied argument remains an individual OsString all the way to CreateProcess.
        command
            .current_dir(&request.repo)
            .args(request.args.iter().map(OsString::as_os_str))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        clean_environment_for_policy(&mut command, policy);
        if let Some(config) = &self.sealed_config {
            command
                .env("HOME", &config.home)
                .env("USERPROFILE", &config.home)
                .env("XDG_CONFIG_HOME", &config.xdg_config_home)
                .env("GIT_CONFIG_GLOBAL", &config.global_config)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_ATTR_NOSYSTEM", "1")
                .env_remove("HOMEDRIVE")
                .env_remove("HOMEPATH");
        }
        command
    }
}

impl SystemGitRunner {
    fn run_internal(
        &self,
        request: GitCommand,
        context: GitRunContext,
    ) -> Result<GitOutput, AppError> {
        if context.is_cancelled() {
            return Err(AppError::Canceled);
        }
        if request.repo.as_os_str().is_empty() {
            return Err(AppError::InvalidInput(
                "repository path is empty".to_owned(),
            ));
        }
        if request.timeout.is_zero() {
            return Err(AppError::InvalidInput(
                "Git timeout must be positive".to_owned(),
            ));
        }
        if context
            .network_timeouts
            .is_some_and(|timeouts| timeouts.authentication.is_zero() || timeouts.idle.is_zero())
        {
            return Err(AppError::InvalidInput(
                "Git network timeouts must be positive".to_owned(),
            ));
        }

        let mut command = self.command(&request, context.policy);
        ProcessTree::configure(&mut command)?;
        let mut child = command
            .spawn()
            .map_err(|error| AppError::Git(spawn_error_kind(error)))?;
        let tree = match ProcessTree::attach(&child) {
            Ok(tree) => tree,
            Err(error) => {
                let _ = terminate_and_reap(&mut child);
                return Err(error);
            }
        };
        let mut guard = ChildGuard::new(child, tree);

        let stdout = match guard.child_mut().stdout.take() {
            Some(stdout) => stdout,
            None => {
                guard.terminate();
                return Err(AppError::Git("Git stdout pipe unavailable".to_owned()));
            }
        };
        let stderr = match guard.child_mut().stderr.take() {
            Some(stderr) => stderr,
            None => {
                guard.terminate();
                return Err(AppError::Git("Git stderr pipe unavailable".to_owned()));
            }
        };
        let stdout_limited = Arc::new(AtomicBool::new(false));
        let stderr_limited = Arc::new(AtomicBool::new(false));
        let network_activity = context
            .network_timeouts
            .map(|_| Arc::new(NetworkProgressActivity::default()));
        let stdout_thread = spawn_reader(
            stdout,
            self.max_stdout_bytes,
            stdout_limited.clone(),
            None,
            None,
        );
        let stderr_thread = spawn_reader(
            stderr,
            self.max_stderr_bytes,
            stderr_limited.clone(),
            context.progress.clone(),
            network_activity.clone(),
        );

        let stdin_writer = request.stdin.map(|input| {
            let stdin = guard.child_mut().stdin.take();
            spawn_stdin_writer(stdin, input)
        });
        drop(guard.child_mut().stdin.take());

        let wait_result = wait_bounded(
            guard
                .child
                .as_mut()
                .expect("child guard must own a process"),
            &guard.tree,
            request.timeout,
            self.poll_interval,
            WaitControl {
                stdout_limited: &stdout_limited,
                stderr_limited: &stderr_limited,
                cancellation: &context.cancellation,
                network_timeouts: context.network_timeouts,
                network_activity: network_activity.as_deref(),
            },
        );
        let (status, termination) = match wait_result {
            Ok(result) => result,
            Err(error) => {
                // Ensure all process and pipe resources are closed before returning an error.
                guard.terminate();
                let _ = collect_output_bounded(stdout_thread, stderr_thread, IO_CLEANUP_TIMEOUT);
                let _ = finish_stdin_writer(stdin_writer, IO_CLEANUP_TIMEOUT);
                return Err(error);
            }
        };
        let output_result =
            collect_output_bounded(stdout_thread, stderr_thread, IO_CLEANUP_TIMEOUT);
        let stdin_result = finish_stdin_writer(stdin_writer, IO_CLEANUP_TIMEOUT);
        let (stdout, stderr) = finish_io_before_disarm(&mut guard, output_result, stdin_result)?;

        match termination {
            WaitTermination::TimedOut => return Err(AppError::Timeout),
            WaitTermination::Canceled => return Err(AppError::Canceled),
            WaitTermination::Exited => {}
        }
        if stdout_limited.load(Ordering::Acquire) || stderr_limited.load(Ordering::Acquire) {
            return Err(AppError::OutputLimit);
        }
        guard.disarm();
        Ok(GitOutput {
            status,
            stdout,
            stderr,
        })
    }
}

impl GitRunner for SystemGitRunner {
    fn run(&self, request: GitCommand) -> Result<GitOutput, AppError> {
        self.run_internal(
            request,
            GitRunContext::new(GitExecutionPolicy::LocalNonInteractive),
        )
    }

    fn run_with_context(
        &self,
        request: GitCommand,
        context: GitRunContext,
    ) -> Result<GitOutput, AppError> {
        self.run_internal(request, context)
    }
}

fn finish_io_before_disarm<T>(
    guard: &mut ChildGuard,
    output_result: Result<T, AppError>,
    stdin_result: Result<(), AppError>,
) -> Result<T, AppError> {
    let output = match output_result {
        Ok(output) => output,
        Err(error) => {
            guard.terminate();
            return Err(error);
        }
    };
    if let Err(error) = stdin_result {
        guard.terminate();
        return Err(error);
    }
    if let Err(error) = guard.mark_reaped() {
        guard.terminate();
        return Err(error);
    }
    Ok(output)
}

fn spawn_reader<R>(
    mut reader: R,
    limit: usize,
    exceeded: Arc<AtomicBool>,
    progress: Option<Arc<dyn GitProgressSink>>,
    network_activity: Option<Arc<NetworkProgressActivity>>,
) -> Receiver<io::Result<Vec<u8>>>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0_u8; 16 * 1024];
        let result = (|| {
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                let observed_progress = progress
                    .as_ref()
                    .is_some_and(|progress| progress.stderr_chunk(&buffer[..read]));
                if observed_progress && let Some(activity) = &network_activity {
                    activity.mark();
                }
                if output.len().saturating_add(read) > limit {
                    exceeded.store(true, Ordering::Release);
                    // Stop retaining bytes immediately. The parent will terminate the child on its
                    // next poll; draining a bounded chunk keeps the pipe from deadlocking meanwhile.
                    continue;
                }
                output.extend_from_slice(&buffer[..read]);
            }
            Ok(output)
        })();
        let _ = sender.send(result);
    });
    receiver
}

fn receive_reader_bounded(
    receiver: Receiver<io::Result<Vec<u8>>>,
    timeout: Duration,
) -> Result<Vec<u8>, AppError> {
    match receiver.recv_timeout(timeout) {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(error)) => Err(AppError::Git(reader_error_kind(error))),
        Err(RecvTimeoutError::Timeout) => Err(AppError::Timeout),
        Err(RecvTimeoutError::Disconnected) => {
            Err(AppError::Git("Git output reader failed".to_owned()))
        }
    }
}

fn collect_output_bounded(
    stdout: Receiver<io::Result<Vec<u8>>>,
    stderr: Receiver<io::Result<Vec<u8>>>,
    timeout: Duration,
) -> Result<(Vec<u8>, Vec<u8>), AppError> {
    let deadline = Instant::now() + timeout;
    let stdout = receive_reader_until(stdout, deadline);
    let stderr = receive_reader_until(stderr, deadline);
    match (stdout, stderr) {
        (Ok(stdout), Ok(stderr)) => Ok((stdout, stderr)),
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
}

fn receive_reader_until(
    receiver: Receiver<io::Result<Vec<u8>>>,
    deadline: Instant,
) -> Result<Vec<u8>, AppError> {
    receive_reader_bounded(receiver, deadline.saturating_duration_since(Instant::now()))
}

fn spawn_stdin_writer(stdin: Option<ChildStdin>, input: Vec<u8>) -> StdinWriter {
    let control = Arc::new(Mutex::new(stdin));
    let (sender, receiver) = mpsc::sync_channel(1);
    let writer_control = control.clone();
    thread::spawn(move || {
        let stdin = match writer_control.lock() {
            Ok(mut guard) => Ok(guard.take()),
            Err(_) => Err(io::Error::other("stdin lock poisoned")),
        };
        let result = match stdin {
            Ok(Some(mut stdin)) => stdin.write_all(&input).and_then(|_| stdin.flush()),
            Ok(None) => Ok(()),
            Err(error) => Err(error),
        };
        let _ = sender.send(result);
    });
    (receiver, control)
}

fn finish_stdin_writer(writer: Option<StdinWriter>, timeout: Duration) -> Result<(), AppError> {
    let Some((receiver, control)) = writer else {
        return Ok(());
    };
    let deadline = Instant::now() + timeout;
    match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Ok(()),
        Err(RecvTimeoutError::Timeout) => {
            if let Ok(mut guard) = control.lock() {
                let _ = guard.take();
            }
            Err(AppError::Timeout)
        }
        Err(RecvTimeoutError::Disconnected) => Ok(()),
    }
}

fn terminate_and_reap(child: &mut Child) -> Result<ExitStatus, AppError> {
    let _ = child.kill();
    child
        .wait()
        .map_err(|error| AppError::Git(wait_error_kind(error)))
}

#[cfg(windows)]
fn wait_cleanup_process(
    child: &mut Child,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<ExitStatus, AppError> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AppError::Git(wait_error_kind(error)))?
        {
            return Ok(status);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        thread::sleep(poll_interval.min(remaining));
    }

    // A stuck taskkill helper must not keep the Git request stuck.  Request termination, then
    // poll for one bounded grace window so an exited process is reaped without an unbounded
    // `wait()` call.  `try_wait` performs the reap once the process has exited.
    let _ = child.kill();
    let reap_deadline = Instant::now() + timeout;
    loop {
        if child
            .try_wait()
            .map_err(|error| AppError::Git(wait_error_kind(error)))?
            .is_some()
        {
            return Err(AppError::Timeout);
        }
        let remaining = reap_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AppError::Timeout);
        }
        thread::sleep(poll_interval.min(remaining));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitTermination {
    Exited,
    TimedOut,
    Canceled,
}

struct WaitControl<'a> {
    stdout_limited: &'a AtomicBool,
    stderr_limited: &'a AtomicBool,
    cancellation: &'a AtomicBool,
    network_timeouts: Option<NetworkTimeouts>,
    network_activity: Option<&'a NetworkProgressActivity>,
}

fn wait_bounded(
    child: &mut Child,
    tree: &ProcessTree,
    timeout: Duration,
    poll_interval: Duration,
    control: WaitControl<'_>,
) -> Result<(ExitStatus, WaitTermination), AppError> {
    let started = Instant::now();
    loop {
        if control.stdout_limited.load(Ordering::Acquire)
            || control.stderr_limited.load(Ordering::Acquire)
        {
            tree.terminate()?;
            let status = terminate_and_reap(child)?;
            return Ok((status, WaitTermination::Exited));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AppError::Git(wait_error_kind(error)))?
        {
            return Ok((status, WaitTermination::Exited));
        }
        if control.cancellation.load(Ordering::Acquire) {
            tree.terminate()?;
            let status = terminate_and_reap(child)?;
            return Ok((status, WaitTermination::Canceled));
        }
        if control
            .network_timeouts
            .zip(control.network_activity)
            .is_some_and(|(timeouts, activity)| activity.timed_out(started, timeouts))
        {
            tree.terminate()?;
            let status = terminate_and_reap(child)?;
            return Ok((status, WaitTermination::TimedOut));
        }
        if started.elapsed() >= timeout {
            tree.terminate()?;
            let status = terminate_and_reap(child)?;
            return Ok((status, WaitTermination::TimedOut));
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        thread::sleep(poll_interval.min(remaining));
    }
}

fn clean_environment_for_policy(command: &mut Command, policy: GitExecutionPolicy) {
    clean_environment_for_policy_from(command, std::env::vars_os(), policy);
}

#[cfg(test)]
fn clean_environment_from<I>(command: &mut Command, parent: I)
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    clean_environment_for_policy_from(command, parent, GitExecutionPolicy::LocalNonInteractive);
}

fn clean_environment_for_policy_from<I>(
    command: &mut Command,
    parent: I,
    policy: GitExecutionPolicy,
) where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    command.env_clear();
    // Keep only path/locale and non-executable SSH-agent transport settings. In particular,
    // never inherit GIT_SSH/GIT_ASKPASS/GIT_CONFIG_* or credential-store overrides from a
    // plugin/parent process.
    for (key, value) in parent {
        if is_safe_inherited_env(&key, policy) {
            command.env(key, value);
        }
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env(
        "GCM_INTERACTIVE",
        if policy == GitExecutionPolicy::ForegroundNetworkInteractive {
            "Auto"
        } else {
            "Never"
        },
    );
    if policy != GitExecutionPolicy::ForegroundNetworkInteractive {
        command.env("SSH_ASKPASS_REQUIRE", "never");
    }
}

fn is_safe_inherited_env(key: &OsStr, policy: GitExecutionPolicy) -> bool {
    let Some(key) = key.to_str() else {
        return false;
    };
    let normalized = key.to_ascii_uppercase();
    matches!(
        normalized.as_str(),
        "PATH"
            | "HOME"
            | "USERPROFILE"
            | "HOMEDRIVE"
            | "HOMEPATH"
            | "SYSTEMROOT"
            | "TEMP"
            | "TMP"
            | "PATHEXT"
            | "SSH_AUTH_SOCK"
            | "SSH_AGENT_PID"
            | "XDG_RUNTIME_DIR"
    ) || (normalized == "DISPLAY" && policy == GitExecutionPolicy::ForegroundNetworkInteractive)
        || normalized == "LANG"
        || normalized.starts_with("LC_")
}

fn spawn_error_kind(error: io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "git executable unavailable".to_owned(),
        io::ErrorKind::PermissionDenied => "permission denied starting git".to_owned(),
        _ => "unable to start git".to_owned(),
    }
}

fn reader_error_kind(error: io::Error) -> String {
    let _ = error;
    "unable to read git output".to_owned()
}

fn wait_error_kind(error: io::Error) -> String {
    let _ = error;
    "unable to wait for git".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured_environment<const N: usize>(
        policy: GitExecutionPolicy,
        parent: [(&str, &str); N],
    ) -> std::collections::BTreeMap<String, Option<String>> {
        let mut command = Command::new("git");
        clean_environment_for_policy_from(
            &mut command,
            parent
                .into_iter()
                .map(|(key, value)| (OsString::from(key), OsString::from(value))),
            policy,
        );
        command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    #[test]
    fn foreground_network_policy_allows_system_interaction_without_attack_overrides() {
        let environment = captured_environment(
            GitExecutionPolicy::ForegroundNetworkInteractive,
            [
                ("PATH", "/safe/bin"),
                ("SSH_AUTH_SOCK", "/safe/agent"),
                ("GIT_ASKPASS", "/tmp/evil"),
                ("SSH_ASKPASS", "/tmp/evil-ssh"),
                ("GIT_SSH_COMMAND", "evil"),
                ("GCM_INTERACTIVE", "Never"),
            ],
        );
        assert_eq!(
            environment
                .get("GIT_TERMINAL_PROMPT")
                .and_then(Option::as_deref),
            Some("0")
        );
        assert_eq!(
            environment
                .get("GCM_INTERACTIVE")
                .and_then(Option::as_deref),
            Some("Auto")
        );
        assert_eq!(
            environment.get("SSH_AUTH_SOCK").and_then(Option::as_deref),
            Some("/safe/agent")
        );
        for key in ["GIT_ASKPASS", "SSH_ASKPASS", "GIT_SSH_COMMAND"] {
            assert!(!environment.contains_key(key));
        }
    }

    #[test]
    fn background_network_policy_never_allows_interactive_credentials() {
        let environment = captured_environment(
            GitExecutionPolicy::BackgroundNetworkNonInteractive,
            [
                ("PATH", "/safe/bin"),
                ("DISPLAY", ":0"),
                ("GCM_INTERACTIVE", "Auto"),
                ("SSH_ASKPASS_REQUIRE", "force"),
            ],
        );
        assert_eq!(
            environment
                .get("GCM_INTERACTIVE")
                .and_then(Option::as_deref),
            Some("Never")
        );
        assert_eq!(
            environment
                .get("SSH_ASKPASS_REQUIRE")
                .and_then(Option::as_deref),
            Some("never")
        );
        assert!(!environment.contains_key("DISPLAY"));
    }

    #[derive(Default)]
    struct RecordingProgressSink {
        chunks: Mutex<Vec<Vec<u8>>>,
    }

    impl GitProgressSink for RecordingProgressSink {
        fn stderr_chunk(&self, chunk: &[u8]) -> bool {
            self.chunks.lock().unwrap().push(chunk.to_vec());
            !chunk.is_empty()
        }
    }

    #[test]
    fn run_context_cancels_before_spawn_and_streams_real_git_stderr() {
        let canceled = GitRunContext::new(GitExecutionPolicy::ForegroundNetworkInteractive);
        canceled.cancel();
        let result = SystemGitRunner::new().run_with_context(
            GitCommand {
                repo: PathBuf::new(),
                args: vec![OsString::from("status")],
                stdin: None,
                timeout: Duration::ZERO,
            },
            canceled,
        );
        assert!(matches!(result, Err(AppError::Canceled)));

        let directory = tempfile::tempdir().unwrap();
        let sink = Arc::new(RecordingProgressSink::default());
        let output = SystemGitRunner::new()
            .run_with_context(
                GitCommand {
                    repo: directory.path().to_path_buf(),
                    args: vec![OsString::from("status")],
                    stdin: None,
                    timeout: Duration::from_secs(5),
                },
                GitRunContext::new(GitExecutionPolicy::ForegroundNetworkInteractive)
                    .with_progress(sink.clone()),
            )
            .unwrap();
        assert!(!output.status.success());
        assert!(!sink.chunks.lock().unwrap().is_empty());
    }

    #[test]
    fn stderr_reader_streams_bounded_chunks_to_the_progress_sink() {
        let sink = Arc::new(RecordingProgressSink::default());
        let limited = Arc::new(AtomicBool::new(false));
        let activity = Arc::new(NetworkProgressActivity::default());
        let receiver = spawn_reader(
            std::io::Cursor::new(vec![b'x'; 40 * 1024]),
            64 * 1024,
            limited,
            Some(sink.clone()),
            Some(activity.clone()),
        );
        assert_eq!(
            receive_reader_bounded(receiver, Duration::from_secs(1))
                .unwrap()
                .len(),
            40 * 1024
        );
        let chunks = sink.chunks.lock().unwrap();
        assert!(chunks.len() >= 3);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 16 * 1024));
        assert!(activity.last_progress.lock().unwrap().is_some());
    }

    #[test]
    fn cancellation_terminates_and_reaps_a_running_git_process() {
        let mut command = Command::new("git");
        command
            .args(["hash-object", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        ProcessTree::configure(&mut command).unwrap();
        let mut child = command.spawn().unwrap();
        let tree = ProcessTree::attach(&child).unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let trigger = cancellation.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            trigger.store(true, Ordering::Release);
        });
        let stdout_limited = AtomicBool::new(false);
        let stderr_limited = AtomicBool::new(false);
        let (_, termination) = wait_bounded(
            &mut child,
            &tree,
            Duration::from_secs(5),
            Duration::from_millis(5),
            WaitControl {
                stdout_limited: &stdout_limited,
                stderr_limited: &stderr_limited,
                cancellation: &cancellation,
                network_timeouts: None,
                network_activity: None,
            },
        )
        .unwrap();
        assert_eq!(termination, WaitTermination::Canceled);
        tree.finish_after_reap().unwrap();
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn network_authentication_timeout_terminates_a_silent_git_process() {
        let (mut child, tree) = waiting_git_process();
        let cancellation = AtomicBool::new(false);
        let stdout_limited = AtomicBool::new(false);
        let stderr_limited = AtomicBool::new(false);
        let activity = NetworkProgressActivity::default();

        let (_, termination) = wait_bounded(
            &mut child,
            &tree,
            Duration::from_secs(5),
            Duration::from_millis(5),
            WaitControl {
                stdout_limited: &stdout_limited,
                stderr_limited: &stderr_limited,
                cancellation: &cancellation,
                network_timeouts: Some(NetworkTimeouts {
                    authentication: Duration::from_millis(30),
                    idle: Duration::from_secs(1),
                }),
                network_activity: Some(&activity),
            },
        )
        .unwrap();

        assert_eq!(termination, WaitTermination::TimedOut);
        tree.finish_after_reap().unwrap();
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn network_context_owns_fixed_timeouts_while_local_git_has_none() {
        let network = GitRunContext::new(GitExecutionPolicy::ForegroundNetworkInteractive);
        assert_eq!(
            network.network_timeouts,
            Some(NetworkTimeouts {
                authentication: DEFAULT_AUTHENTICATION_TIMEOUT,
                idle: DEFAULT_NETWORK_IDLE_TIMEOUT,
            })
        );
        assert!(
            GitRunContext::new(GitExecutionPolicy::LocalNonInteractive)
                .network_timeouts
                .is_none()
        );
        assert_eq!(
            network
                .with_network_timeouts(Duration::from_secs(1), Duration::from_secs(2))
                .network_timeouts,
            Some(NetworkTimeouts {
                authentication: Duration::from_secs(1),
                idle: Duration::from_secs(2),
            })
        );
    }

    #[test]
    fn network_idle_timeout_starts_after_the_first_progress_chunk() {
        let (mut child, tree) = waiting_git_process();
        let cancellation = AtomicBool::new(false);
        let stdout_limited = AtomicBool::new(false);
        let stderr_limited = AtomicBool::new(false);
        let activity = NetworkProgressActivity::default();
        activity.mark();

        let (_, termination) = wait_bounded(
            &mut child,
            &tree,
            Duration::from_secs(5),
            Duration::from_millis(5),
            WaitControl {
                stdout_limited: &stdout_limited,
                stderr_limited: &stderr_limited,
                cancellation: &cancellation,
                network_timeouts: Some(NetworkTimeouts {
                    authentication: Duration::from_secs(1),
                    idle: Duration::from_millis(30),
                }),
                network_activity: Some(&activity),
            },
        )
        .unwrap();

        assert_eq!(termination, WaitTermination::TimedOut);
        tree.finish_after_reap().unwrap();
        assert!(child.try_wait().unwrap().is_some());
    }

    fn waiting_git_process() -> (Child, ProcessTree) {
        let mut command = Command::new("git");
        command
            .args(["hash-object", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        ProcessTree::configure(&mut command).unwrap();
        let child = command.spawn().unwrap();
        let tree = ProcessTree::attach(&child).unwrap();
        (child, tree)
    }

    #[test]
    fn bounded_reader_cleanup_times_out_instead_of_joining_forever() {
        let (_sender, receiver) = std::sync::mpsc::channel::<io::Result<Vec<u8>>>();
        let started = Instant::now();
        assert!(matches!(
            receive_reader_bounded(receiver, Duration::from_millis(20)),
            Err(AppError::Timeout)
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn io_failure_terminates_and_reaps_before_process_tree_is_disarmed() {
        let mut command = Command::new("git");
        command
            .args(["hash-object", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        ProcessTree::configure(&mut command).expect("process tree configures");
        let Ok(child) = command.spawn() else {
            eprintln!("git executable unavailable; skipping cleanup ordering test");
            return;
        };
        let tree = ProcessTree::attach(&child).expect("process joins tree");
        let mut guard = ChildGuard::new(child, tree);
        assert!(!guard.reaped);
        assert!(!guard.tree.reaped.load(Ordering::Acquire));
        assert!(
            guard
                .child_mut()
                .try_wait()
                .expect("live child can be queried")
                .is_none()
        );

        let result = finish_io_before_disarm::<(Vec<u8>, Vec<u8>)>(
            &mut guard,
            Err(AppError::Timeout),
            Ok(()),
        );

        assert!(matches!(result, Err(AppError::Timeout)));
        assert!(guard.reaped);
        assert!(guard.tree.reaped.load(Ordering::Acquire));
        assert!(
            guard
                .child_mut()
                .try_wait()
                .expect("cleaned child can be queried")
                .is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_io_timeout_kills_a_descendant_that_keeps_the_output_pipe_open() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().expect("temp directory creates");
        let script = directory.path().join("hold-pipe.sh");
        let pid_file = directory.path().join("descendant.pid");
        fs::write(
            &script,
            "#!/bin/sh\ntrap '' HUP\nsleep 30 &\necho \"$!\" > \"$1\"\n",
        )
        .expect("helper script writes");
        let mut permissions = fs::metadata(&script)
            .expect("helper metadata reads")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&script, permissions).expect("helper becomes executable");
        let alias = format!(
            "alias.hold=!\"{}\" \"{}\"",
            script.display(),
            pid_file.display()
        );

        let result = SystemGitRunner::new().run(GitCommand {
            repo: directory.path().to_path_buf(),
            args: [
                OsString::from("-c"),
                OsString::from(alias),
                OsString::from("hold"),
            ]
            .into_iter()
            .collect(),
            stdin: None,
            timeout: Duration::from_secs(5),
        });
        assert!(matches!(result, Err(AppError::Timeout)));

        let pid: libc::pid_t = fs::read_to_string(&pid_file)
            .expect("descendant pid reads")
            .trim()
            .parse()
            .expect("descendant pid parses");
        let deadline = Instant::now() + Duration::from_secs(1);
        while unix_process_exists(pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let still_running = unix_process_exists(pid);
        if still_running {
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }
        assert!(!still_running, "Git descendant survived output cleanup");
    }

    #[cfg(unix)]
    fn unix_process_exists(pid: libc::pid_t) -> bool {
        let result = unsafe { libc::kill(pid, 0) };
        if result == 0 {
            return true;
        }
        io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    }

    #[test]
    fn process_tree_terminates_a_configured_git_process() {
        let mut command = Command::new("git");
        command
            .args(["hash-object", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        ProcessTree::configure(&mut command).expect("process tree configures");
        let Ok(mut child) = command.spawn() else {
            eprintln!("git executable unavailable; skipping process tree test");
            return;
        };
        let tree = ProcessTree::attach(&child).expect("process joins tree");
        tree.terminate().expect("tree terminates");
        let _ = child.wait().expect("direct child is reaped");
        assert!(child.try_wait().expect("try_wait succeeds").is_some());
    }

    #[test]
    fn process_tree_marks_normal_reap_without_signaling_reused_group() {
        let mut command = Command::new("git");
        command
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        ProcessTree::configure(&mut command).expect("process tree configures");
        let Ok(mut child) = command.spawn() else {
            eprintln!("git executable unavailable; skipping reap safety test");
            return;
        };
        let tree = ProcessTree::attach(&child).expect("process joins tree");
        let _ = child.wait().expect("direct child is reaped");
        tree.finish_after_reap().expect("reap state records");
        tree.terminate().expect("reaped tree is inert");
    }

    #[cfg(windows)]
    #[test]
    fn windows_fallback_command_is_absolute_and_shell_free() {
        let command = ProcessTree::fallback_command_for_pid(1234).expect("fallback command");
        assert!(std::path::Path::new(command.get_program()).is_absolute());
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args, vec!["/PID", "1234", "/T", "/F"]);
    }

    #[cfg(windows)]
    #[test]
    fn cleanup_wait_reaps_a_hanging_process_within_the_deadline() {
        let Ok(mut child) = Command::new("git")
            .args(["hash-object", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            eprintln!("git executable unavailable; skipping cleanup wait test");
            return;
        };
        let started = Instant::now();
        let result = wait_cleanup_process(
            &mut child,
            Duration::from_millis(20),
            Duration::from_millis(1),
        );
        assert!(matches!(result, Err(AppError::Timeout)));
        assert!(started.elapsed() < Duration::from_secs(1));
        let terminated_by = Instant::now() + Duration::from_secs(1);
        let reaped = loop {
            if child
                .try_wait()
                .expect("cleanup child can be queried")
                .is_some()
            {
                break true;
            }
            if Instant::now() >= terminated_by {
                break false;
            }
            thread::sleep(Duration::from_millis(1));
        };
        if !reaped {
            let _ = terminate_and_reap(&mut child);
        }
        assert!(reaped, "cleanup did not terminate the helper process");
    }

    #[test]
    fn clean_environment_drops_parent_git_and_credential_overrides() {
        let mut command = Command::new("git");
        let parent = [
            (OsString::from("GIT_SSH"), OsString::from("evil-ssh")),
            (
                OsString::from("GIT_ASKPASS"),
                OsString::from("evil-askpass"),
            ),
            (
                OsString::from("GIT_CONFIG_GLOBAL"),
                OsString::from("evil-global"),
            ),
            (
                OsString::from("GIT_CONFIG_SYSTEM"),
                OsString::from("evil-system"),
            ),
            (OsString::from("GIT_CONFIG_NOSYSTEM"), OsString::from("0")),
            (
                OsString::from("GCM_CREDENTIAL_STORE"),
                OsString::from("evil-store"),
            ),
            (
                OsString::from("SSH_AUTH_SOCK"),
                OsString::from("agent.sock"),
            ),
        ];
        clean_environment_from(&mut command, parent);
        let names = command
            .get_envs()
            .filter_map(|(key, _)| key.to_str())
            .collect::<Vec<_>>();
        for forbidden in [
            "GIT_SSH",
            "GIT_ASKPASS",
            "GIT_CONFIG_GLOBAL",
            "GIT_CONFIG_SYSTEM",
            "GIT_CONFIG_NOSYSTEM",
            "GCM_CREDENTIAL_STORE",
        ] {
            assert!(!names.contains(&forbidden), "forwarded {forbidden}");
        }
        assert!(names.contains(&"SSH_AUTH_SOCK"));
        assert!(names.contains(&"GIT_TERMINAL_PROMPT"));
        assert!(names.contains(&"GCM_INTERACTIVE"));
    }

    #[test]
    fn terminate_and_reap_always_reaps_child() {
        let Ok(mut child) = Command::new("git")
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            eprintln!("git executable unavailable; skipping cleanup helper test");
            return;
        };
        let _ = terminate_and_reap(&mut child).expect("child can be reaped");
        assert!(child.try_wait().expect("try_wait succeeds").is_some());
    }
}
