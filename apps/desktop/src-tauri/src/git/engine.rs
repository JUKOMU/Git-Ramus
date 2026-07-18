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

pub trait GitRunner: Send + Sync {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError>;
}

#[derive(Debug, Clone)]
pub struct SystemGitRunner {
    max_stdout_bytes: usize,
    max_stderr_bytes: usize,
    poll_interval: Duration,
}

const IO_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(windows)]
const CLEANUP_POLL_INTERVAL: Duration = Duration::from_millis(10);
type StdinWriter = (Receiver<io::Result<()>>, Arc<Mutex<Option<ChildStdin>>>);

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
        }
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

    fn command(&self, request: &GitCommand) -> Command {
        let mut command = Command::new("git");
        // `current_dir` is deliberately used instead of composing a shell command. Every
        // caller-supplied argument remains an individual OsString all the way to CreateProcess.
        command
            .current_dir(&request.repo)
            .args(request.args.iter().map(OsString::as_os_str))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        clean_environment(&mut command);
        command
    }
}

impl GitRunner for SystemGitRunner {
    fn run(&self, request: GitCommand) -> Result<GitOutput, AppError> {
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

        let mut command = self.command(&request);
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
        let stdout_thread = spawn_reader(stdout, self.max_stdout_bytes, stdout_limited.clone());
        let stderr_thread = spawn_reader(stderr, self.max_stderr_bytes, stderr_limited.clone());

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
            &stdout_limited,
            &stderr_limited,
        );
        let (status, timed_out) = match wait_result {
            Ok(result) => {
                if let Err(error) = guard.mark_reaped() {
                    guard.terminate();
                    let _ =
                        collect_output_bounded(stdout_thread, stderr_thread, IO_CLEANUP_TIMEOUT);
                    let _ = finish_stdin_writer(stdin_writer, IO_CLEANUP_TIMEOUT);
                    return Err(error);
                }
                result
            }
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

        if timed_out {
            return Err(AppError::Timeout);
        }
        if stdout_limited.load(Ordering::Acquire) || stderr_limited.load(Ordering::Acquire) {
            return Err(AppError::OutputLimit);
        }
        let (stdout, stderr) = output_result?;
        stdin_result?;
        guard.disarm();
        Ok(GitOutput {
            status,
            stdout,
            stderr,
        })
    }
}

fn spawn_reader<R>(
    mut reader: R,
    limit: usize,
    exceeded: Arc<AtomicBool>,
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

fn wait_bounded(
    child: &mut Child,
    tree: &ProcessTree,
    timeout: Duration,
    poll_interval: Duration,
    stdout_limited: &AtomicBool,
    stderr_limited: &AtomicBool,
) -> Result<(ExitStatus, bool), AppError> {
    let started = Instant::now();
    let mut timed_out = false;
    loop {
        if stdout_limited.load(Ordering::Acquire) || stderr_limited.load(Ordering::Acquire) {
            tree.terminate()?;
            let status = terminate_and_reap(child)?;
            return Ok((status, false));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| AppError::Git(wait_error_kind(error)))?
        {
            return Ok((status, timed_out));
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            tree.terminate()?;
            let status = terminate_and_reap(child)?;
            return Ok((status, timed_out));
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        thread::sleep(poll_interval.min(remaining));
    }
}

fn clean_environment(command: &mut Command) {
    clean_environment_from(command, std::env::vars_os());
}

fn clean_environment_from<I>(command: &mut Command, parent: I)
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    command.env_clear();
    // Keep only path/locale and non-executable SSH-agent transport settings. In particular,
    // never inherit GIT_SSH/GIT_ASKPASS/GIT_CONFIG_* or credential-store overrides from a
    // plugin/parent process.
    for (key, value) in parent {
        if is_safe_inherited_env(&key) {
            command.env(key, value);
        }
    }
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("GCM_INTERACTIVE", "Never");
}

fn is_safe_inherited_env(key: &OsStr) -> bool {
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
            | "DISPLAY"
            | "XDG_RUNTIME_DIR"
    ) || normalized == "LANG"
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
        assert!(
            child
                .try_wait()
                .expect("cleanup child can be queried")
                .is_some()
        );
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
